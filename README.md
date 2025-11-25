# Memory MCP Server (Rust + SQLite)

High-performance Rust implementation of the MCP Memory Server - a knowledge graph for persistent Claude memory.

## Features

- **SQLite Backend:** Fast, reliable, ACID-compliant storage
- **Full-Text Search:** FTS5 for efficient searching across names, types, and observations
- **Automatic Deduplication:** SQLite constraints prevent duplicate entities and relations
- **Cascade Deletes:** FOREIGN KEY constraints automatically clean up orphaned relations
- **Async I/O:** Tokio-based for non-blocking operations
- **Indexed Queries:** O(log n) lookups instead of O(n) scans
- **MCP Compliant:** Full MCP protocol support via rmcp SDK
- **Type-Safe:** Rust's type system prevents common bugs

## Installation

```bash
cd rust/memory-mcp-rs
cargo build --release
```

The binary will be at `target/release/memory-mcp-rs` (or `.exe` on Windows).

## Usage

### Standalone

```bash
# Default path: %LOCALAPPDATA%/mcp-memory/knowledge_graph.db (Windows)
# or ~/.local/share/mcp-memory/knowledge_graph.db (Linux/Mac)
memory-mcp-rs

# Custom database path
MEMORY_FILE_PATH=/path/to/graph.db memory-mcp-rs
```

### With Claude Desktop

Add to your Claude Desktop MCP config:

**Windows** (`%APPDATA%\Claude\claude_desktop_config.json`):
```json
{
  "mcpServers": {
    "memory": {
      "command": "C:\\path\\to\\memory-mcp-rs.exe"
    }
  }
}
```

**macOS/Linux** (`~/.config/claude/claude_desktop_config.json`):
```json
{
  "mcpServers": {
    "memory": {
      "command": "/path/to/memory-mcp-rs"
    }
  }
}
```

## MCP Tools

| Tool | Description |
|------|-------------|
| `create_entities` | Create new entities in the knowledge graph |
| `create_relations` | Create relations between entities |
| `add_observations` | Add observations to an entity |
| `delete_entities` | Delete entities (cascade deletes relations) |
| `delete_observations` | Delete specific observations |
| `delete_relations` | Delete specific relations |
| `read_graph` | Read the entire knowledge graph |
| `search_nodes` | Full-text search across entities |
| `open_nodes` | Open specific nodes by name |

## Architecture

```
src/
├── main.rs       # MCP server + tool routing
├── graph.rs      # Data structures (Entity, Relation, KnowledgeGraph)
├── manager.rs    # Async manager wrapping storage
└── storage.rs    # SQLite implementation
```

### SQLite Schema

```sql
-- Entities
CREATE TABLE entities (
    name TEXT PRIMARY KEY,
    entity_type TEXT NOT NULL,
    observations TEXT NOT NULL  -- JSON array
);

-- Relations with cascade delete
CREATE TABLE relations (
    from_entity TEXT NOT NULL,
    to_entity TEXT NOT NULL,
    relation_type TEXT NOT NULL,
    FOREIGN KEY(from_entity) REFERENCES entities(name) ON DELETE CASCADE,
    FOREIGN KEY(to_entity) REFERENCES entities(name) ON DELETE CASCADE
);

-- FTS5 for full-text search
CREATE VIRTUAL TABLE entities_fts USING fts5(
    name, entity_type, observations,
    content=entities
);
```

## Performance

| Operation | JSONL | SQLite |
|-----------|-------|--------|
| Search | O(n) | **O(log n)** with FTS5 |
| Insert | O(n) | **O(log n)** |
| Delete | O(n) | **O(log n)** |
| Cascade Delete | Manual | **Automatic** |

## Testing

```bash
cargo test
```

All tests use temporary databases and clean up automatically.

## Example Usage

```rust
use memory_mcp_rs::{graph::Entity, manager::KnowledgeGraphManager};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let manager = KnowledgeGraphManager::new("graph.db".into())?;

    // Create entity
    manager.create_entities(vec![
        Entity {
            name: "Alice".to_string(),
            entity_type: "person".to_string(),
            observations: vec!["Works at Acme".to_string()],
        }
    ]).await?;

    // Search
    let results = manager.search_nodes(Some("Acme".to_string())).await?;
    println!("Found {} entities", results.entities.len());

    Ok(())
}
```

## Differences from TypeScript Version

| Feature | TypeScript | Rust |
|---------|-----------|------|
| Storage | JSONL | **SQLite** |
| Search | Linear scan | **FTS5 indexed** |
| Deduplication | Manual | **Automatic** |
| Cascade Delete | Manual | **Automatic** |
| Concurrency | Single-threaded | **Async/await** |
| Type Safety | Runtime | **Compile-time** |

## License

MIT
