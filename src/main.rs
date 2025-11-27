use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

mod graph;
mod logging;
mod manager;
mod storage;

use graph::{Entity, Relation, ObservationInput, ObservationDeletion};
use logging::{init_logging, TransportMode};
use manager::KnowledgeGraphManager;

/// Command-line arguments
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Database file path (default: system data dir/mcp-memory/knowledge_graph.db or MEMORY_FILE_PATH env)
    #[arg(long)]
    db_path: Option<PathBuf>,

    /// Enable streamable HTTP mode (default: stdio)
    #[arg(short = 's', long = "stream")]
    stream_mode: bool,

    /// HTTP port for stream mode
    #[arg(short = 'p', long, default_value = "8000")]
    port: u16,

    /// Bind address for stream mode
    #[arg(short = 'b', long, default_value = "127.0.0.1")]
    bind: String,

    /// Enable file logging. Optionally specify log file name (default: memory-mcp-rs.log)
    #[arg(short = 'l', long, value_name = "FILE", num_args = 0..=1, default_missing_value = "memory-mcp-rs.log")]
    log: Option<String>,
}

#[derive(Clone)]
struct MemoryServer {
    manager: Arc<KnowledgeGraphManager>,
    tool_router: ToolRouter<Self>,
}

impl MemoryServer {
    fn new(manager: Arc<KnowledgeGraphManager>) -> Self {
        Self {
            manager,
            tool_router: Self::tool_router(),
        }
    }

    fn server_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation {
                name: "memory-mcp-rs".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                website_url: None,
                icons: None,
            },
            instructions: None,
        }
    }
}

#[tool_router]
impl MemoryServer {
    /// Create new entities in knowledge graph
    #[tool(
        name = "create_entities",
        description = "Create multiple new entities in the knowledge graph"
    )]
    async fn create_entities(
        &self,
        Parameters(args): Parameters<CreateEntitiesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let created = self
            .manager
            .create_entities(args.entities)
            .await
            .map_err(internal_err("Failed to create entities"))?;

        let summary = format!("{} entities created successfully", created.len());

        Ok(CallToolResult {
            content: vec![Content::text(&summary)],
            structured_content: Some(json!({"entities": created})),
            is_error: Some(false),
            meta: None,
        })
    }

    /// Create relations between entities
    #[tool(
        name = "create_relations",
        description = "Create multiple new relations between entities in the knowledge graph"
    )]
    async fn create_relations(
        &self,
        Parameters(args): Parameters<CreateRelationsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let created = self
            .manager
            .create_relations(args.relations)
            .await
            .map_err(internal_err("Failed to create relations"))?;

        let summary = format!("{} relations created successfully", created.len());

        Ok(CallToolResult {
            content: vec![Content::text(&summary)],
            structured_content: Some(json!({"relations": created})),
            is_error: Some(false),
            meta: None,
        })
    }

    /// Add observations to entities
    #[tool(
        name = "add_observations",
        description = "Add new observations to existing entities in the knowledge graph (batch operation)"
    )]
    async fn add_observations(
        &self,
        Parameters(args): Parameters<AddObservationsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let results = self.manager
            .add_observations(args.observations)
            .await
            .map_err(internal_err("Failed to add observations"))?;

        let summary = format!(
            "Added observations to {} entities",
            results.len()
        );

        Ok(CallToolResult {
            content: vec![Content::text(&summary)],
            structured_content: Some(json!({"results": results})),
            is_error: Some(false),
            meta: None,
        })
    }

    /// Delete entities and their relations
    #[tool(
        name = "delete_entities",
        description = "Delete entities and their associated relations from the knowledge graph"
    )]
    async fn delete_entities(
        &self,
        Parameters(args): Parameters<DeleteEntitiesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let count = self
            .manager
            .delete_entities(args.entity_names)
            .await
            .map_err(internal_err("Failed to delete entities"))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "{} entities deleted successfully",
            count
        ))]))
    }

    /// Delete observations from entities
    #[tool(
        name = "delete_observations",
        description = "Delete specific observations from entities in the knowledge graph (batch operation)"
    )]
    async fn delete_observations(
        &self,
        Parameters(args): Parameters<DeleteObservationsArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.manager
            .delete_observations(args.deletions)
            .await
            .map_err(internal_err("Failed to delete observations"))?;

        Ok(CallToolResult::success(vec![Content::text(
            "Observations deleted successfully",
        )]))
    }

    /// Delete relations
    #[tool(
        name = "delete_relations",
        description = "Delete specific relations from the knowledge graph"
    )]
    async fn delete_relations(
        &self,
        Parameters(args): Parameters<DeleteRelationsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let count = self
            .manager
            .delete_relations(args.relations)
            .await
            .map_err(internal_err("Failed to delete relations"))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "{} relations deleted successfully",
            count
        ))]))
    }

    /// Read entire knowledge graph
    #[tool(
        name = "read_graph",
        description = "Read the entire knowledge graph"
    )]
    async fn read_graph(&self) -> Result<CallToolResult, McpError> {
        let graph = self
            .manager
            .read_graph()
            .await
            .map_err(internal_err("Failed to read graph"))?;

        let summary = format!(
            "Knowledge graph contains {} entities and {} relations",
            graph.entities.len(),
            graph.relations.len()
        );

        Ok(CallToolResult {
            content: vec![Content::text(&summary)],
            structured_content: Some(json!(graph)),
            is_error: Some(false),
            meta: None,
        })
    }

    /// Search nodes by query
    #[tool(
        name = "search_nodes",
        description = "Search for nodes in the knowledge graph using full-text search. Searches across entity names, types, and observations."
    )]
    async fn search_nodes(
        &self,
        Parameters(args): Parameters<SearchNodesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .manager
            .search_nodes(args.query)
            .await
            .map_err(internal_err("Failed to search nodes"))?;

        let summary = format!(
            "Found {} entities and {} relations",
            result.entities.len(),
            result.relations.len()
        );

        Ok(CallToolResult {
            content: vec![Content::text(&summary)],
            structured_content: Some(json!(result)),
            is_error: Some(false),
            meta: None,
        })
    }

    /// Open specific nodes by names
    #[tool(
        name = "open_nodes",
        description = "Open specific nodes in the knowledge graph by their names"
    )]
    async fn open_nodes(
        &self,
        Parameters(args): Parameters<OpenNodesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .manager
            .open_nodes(args.names)
            .await
            .map_err(internal_err("Failed to open nodes"))?;

        let summary = format!(
            "Retrieved {} entities and {} relations",
            result.entities.len(),
            result.relations.len()
        );

        Ok(CallToolResult {
            content: vec![Content::text(&summary)],
            structured_content: Some(json!(result)),
            is_error: Some(false),
            meta: None,
        })
    }
}

#[tool_handler]
impl ServerHandler for MemoryServer {
    fn get_info(&self) -> ServerInfo {
        self.server_info()
    }
}

// Tool argument schemas

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateEntitiesArgs {
    entities: Vec<Entity>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateRelationsArgs {
    relations: Vec<Relation>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AddObservationsArgs {
    observations: Vec<ObservationInput>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DeleteEntitiesArgs {
    entity_names: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DeleteObservationsArgs {
    deletions: Vec<ObservationDeletion>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DeleteRelationsArgs {
    relations: Vec<Relation>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchNodesArgs {
    query: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct OpenNodesArgs {
    names: Vec<String>,
}

// Helper for error conversion
fn internal_err<T: ToString>(msg: &'static str) -> impl FnOnce(T) -> McpError + Clone {
    move |err| McpError::internal_error(msg, Some(json!({ "error": err.to_string() })))
}

/// Run server in stdio mode (default)
async fn run_stdio_mode(server: MemoryServer) -> Result<(), Box<dyn std::error::Error>> {
    let transport = stdio();
    let svc = server.serve(transport).await?;
    svc.waiting().await?;
    Ok(())
}

/// Run server in streamable HTTP mode
async fn run_stream_mode(
    server: MemoryServer,
    bind: &str,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    use rmcp::transport::StreamableHttpService;
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;

    let addr = format!("{}:{}", bind, port);
    tracing::info!("Starting MCP HTTP server on http://{}/mcp", addr);

    // Create service with session management
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        LocalSessionManager::default().into(),
        Default::default(),
    );

    // Build router with MCP endpoint and health check
    let router = axum::Router::new()
        .nest_service("/mcp", service)
        .route("/health", axum::routing::get(|| async { "OK" }));

    let tcp_listener = tokio::net::TcpListener::bind(&addr).await?;

    // Start server with graceful shutdown
    axum::serve(tcp_listener, router)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;

    Ok(())
}

/// Validate database path to prevent path traversal attacks
fn validate_db_path(path: &std::path::Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Check file extension FIRST (before any filesystem operations)
    if let Some(ext) = path.extension() {
        if ext != "db" {
            return Err("Invalid database file extension (must be .db)".into());
        }
    } else {
        return Err("Database path must have .db extension".into());
    }

    // Canonicalize path to resolve .. and symlinks
    let canonical = path.canonicalize().or_else(|_| -> Result<PathBuf, Box<dyn std::error::Error>> {
        // If file doesn't exist yet, canonicalize parent and append filename
        if let Some(parent) = path.parent() {
            let filename = path.file_name()
                .ok_or("Invalid path: no filename")?;
            std::fs::create_dir_all(parent)?;
            let canonical_parent = parent.canonicalize()?;
            Ok(canonical_parent.join(filename))
        } else {
            Err("Invalid path: no parent directory".into())
        }
    })?;

    // Ensure path is absolute
    if !canonical.is_absolute() {
        return Err("Database path must be absolute".into());
    }

    Ok(canonical)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Determine transport mode
    let mode = if args.stream_mode {
        TransportMode::Stream
    } else {
        TransportMode::Stdio
    };

    // Initialize logging based on mode
    // CRITICAL: stdio mode MUST NOT log to stderr by default!
    // Any stderr output during handshake causes "connection closed" in MCP clients
    init_logging(mode, args.log)?;

    // Get database path from args or environment or use default
    let db_path = args.db_path.or_else(|| {
        std::env::var("MEMORY_FILE_PATH").ok().map(PathBuf::from)
    }).unwrap_or_else(|| {
        let mut path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("mcp-memory");
        path.push("knowledge_graph.db");
        path
    });

    // Create parent directories if needed
    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Validate path to prevent traversal attacks
    let db_path = validate_db_path(&db_path)?;

    // Initialize manager
    let manager = Arc::new(KnowledgeGraphManager::new(db_path)?);

    // Create server
    let server = MemoryServer::new(manager);

    // Run in selected mode
    match mode {
        TransportMode::Stdio => run_stdio_mode(server).await,
        TransportMode::Stream => run_stream_mode(server, &args.bind, args.port).await,
    }
}
