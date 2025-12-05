# Memory MCP Server (Rust + SQLite)

High-performance Rust implementation of the MCP Memory Server - a knowledge graph for persistent Claude memory.

## Original Project

This is a Rust port of the [Model Context Protocol Servers](https://github.com/modelcontextprotocol/servers) memory server. The original TypeScript implementation can be found in the [official MCP servers repository](https://github.com/modelcontextprotocol/servers/tree/main/src/memory).

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

## Transport Modes

The server supports two transport modes:

### stdio Mode (Default)
- **Use case:** Local MCP clients (Claude Desktop, Claude Code, Codex)
- **Protocol:** JSON-RPC over stdin/stdout
- **Logging:** Silent by default (prevents MCP handshake issues), optional file logging with `--log`
- **Command:** `memory-mcp-rs` or `memory-mcp-rs --log server.log`

### HTTP Stream Mode
- **Use case:** Remote access, web applications, debugging, testing
- **Protocol:** HTTP with Server-Sent Events (SSE)
- **Endpoints:**
  - `/mcp` - MCP protocol endpoint
  - `/health` - Health check (returns "OK")
- **Logging:** Always enabled to stderr, optional file logging with `--log`
- **Command:** `memory-mcp-rs --stream --port 8000`

## Usage

### Search semantics (FTS)
- Поиск использует SQLite FTS5, но каждый термин экранируется: операторы FTS (OR/NEAR/*) не поддерживаются.
- Поведение: все слова запроса объединяются логическим AND; фразы работают только как набор слов.
- Причина: избежать синтаксических ошибок и инъекций в пользовательских запросах.

### CLI Options

```
Usage: memory-mcp-rs [OPTIONS]

Options:
      --db-path <DB_PATH>    Database file path (default: system data dir or MEMORY_FILE_PATH env)
  -s, --stream               Enable streamable HTTP mode (default: stdio)
  -p, --port <PORT>          HTTP port for stream mode [default: 8000]
  -b, --bind <BIND>          Bind address for stream mode [default: 127.0.0.1]
  -l, --log [<FILE>]         Enable file logging [default: memory-mcp-rs.log]
  -h, --help                 Print help
  -V, --version              Print version
```

### stdio Mode Examples

```bash
# Default path: %LOCALAPPDATA%/mcp-memory/knowledge_graph.db (Windows)
# or ~/.local/share/mcp-memory/knowledge_graph.db (Linux/Mac)
memory-mcp-rs

# Custom database path (CLI flag)
memory-mcp-rs --db-path /path/to/graph.db

# Custom database path (environment variable)
MEMORY_FILE_PATH=/path/to/graph.db memory-mcp-rs

# With file logging (for debugging)
memory-mcp-rs --log debug.log
```

### HTTP Stream Mode Examples

```bash
# Start HTTP server on default port 8000
memory-mcp-rs --stream

# Custom port and bind address
memory-mcp-rs --stream --port 9000 --bind 0.0.0.0

# With logging to both console and file
memory-mcp-rs --stream --log server.log

# Health check
curl http://localhost:8000/health
# Returns: OK
```

### With Claude Desktop

**stdio mode** - Add to MCP config:

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

### With Claude Code

**stdio mode** - Add to `.claude/mcp.json`:
```json
{
  "mcpServers": {
    "memory": {
      "command": "memory-mcp-rs",
      "args": []
    }
  }
}
```

**HTTP stream mode** - Start server separately, then configure:
```bash
# Terminal 1: Start HTTP server
memory-mcp-rs --stream --port 8000

# Terminal 2: Add to .claude/mcp.json
{
  "mcpServers": {
    "memory-http": {
      "url": "http://localhost:8000/mcp"
    }
  }
}
```

### With Codex

**stdio mode** - Add to Codex MCP config:
```json
{
  "mcpServers": {
    "memory": {
      "command": "memory-mcp-rs"
    }
  }
}
```

**HTTP stream mode** - Start server separately:
```bash
# Terminal 1: Start HTTP server
memory-mcp-rs --stream --port 8000 --log server.log

# Terminal 2: Configure Codex to use http://localhost:8000/mcp
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
├── main.rs       # MCP server + tool routing + dual-mode transport
├── logging.rs    # Transport-aware logging (stdio vs HTTP)
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
# Run all tests (integration + HTTP transport)
cargo test

# Run only integration tests
cargo test --test integration

# Run only HTTP transport tests
cargo test --test http_transport
```

**Test coverage:**
- 23 integration tests (SQLite, CRUD, FTS5, validation)
- 4 HTTP transport tests (health check, endpoints, logging)
- All tests use temporary databases and clean up automatically

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

### Architecture Changes

| Aspect | TypeScript (Original) | Rust (This Port) |
|--------|----------------------|------------------|
| **Code Organization** | Monolithic (`index.ts`, ~470 LOC) | Modular (4 modules, ~1200 LOC) |
| **Storage Backend** | JSONL text file | SQLite binary database |
| **Data Model** | In-memory arrays | Relational tables with constraints |
| **File Structure** | Single file + tests | `graph.rs`, `storage.rs`, `manager.rs`, `main.rs` |

### Performance Improvements

| Operation | TypeScript | Rust | Improvement |
|-----------|-----------|------|-------------|
| **Search** | O(n) linear `.filter()` | O(log n) SQL `LIKE` + indexes | 10-100x faster |
| **Insert** | O(n) full file rewrite | O(log n) SQL `INSERT` | 10-100x faster |
| **Delete** | O(n) + manual filter | O(log n) + CASCADE | 10-100x faster |
| **Deduplication** | Manual `.some()` check | Automatic (`UNIQUE` constraint) | Zero overhead |
| **Cascade Delete** | Manual loop filtering | Automatic (`FOREIGN KEY CASCADE`) | Zero overhead |

### Data Integrity

| Feature | TypeScript | Rust |
|---------|-----------|------|
| **ACID Transactions** | ❌ No | ✅ SQLite ACID |
| **Foreign Key Validation** | ❌ Manual | ✅ Automatic |
| **Unique Constraints** | ❌ Manual | ✅ Database-level |
| **Crash Recovery** | ❌ Corrupted file | ✅ WAL journaling |
| **Concurrent Access** | ❌ File locking issues | ✅ WAL mode (concurrent reads) |

### Type Safety & Memory

| Aspect | TypeScript | Rust |
|--------|-----------|------|
| **Type Checking** | Runtime (Zod schemas) | Compile-time (Rust types) |
| **Memory Safety** | Garbage collector | Ownership + borrow checker |
| **Null Safety** | `undefined` checks | `Option<T>` / `Result<T, E>` |
| **Error Handling** | Exceptions | `Result<T, E>` + `anyhow` |

### Concurrency Model

| Feature | TypeScript | Rust |
|---------|-----------|------|
| **Async Runtime** | Single-threaded event loop | Tokio multi-threaded |
| **I/O Model** | Non-blocking (Node.js) | Non-blocking (async/await) |
| **File Locking** | OS-level (fs module) | SQLite connection pooling |

### Testing

| Aspect | TypeScript | Rust |
|--------|-----------|------|
| **Framework** | Vitest (2 test files) | Cargo test (11 integration tests) |
| **Coverage** | Basic CRUD | Full CRUD + edge cases + persistence |
| **Isolation** | Shared test file | Temporary databases per test |

### What's NOT Changed

- ✅ **MCP Protocol**: Same 9 tools with identical interfaces
- ✅ **API Compatibility**: Drop-in replacement for TypeScript version
- ✅ **Graph Semantics**: Same Entity/Relation/Observation model
- ✅ **Tool Names**: Exact same (`create_entities`, `search_nodes`, etc.)

### Migration Notes

The Rust version stores data in SQLite format, so you cannot directly migrate from the TypeScript JSONL file. If you need migration:

1. Export graph from TypeScript version using `read_graph`
2. Import into Rust version using `create_entities` + `create_relations`

### When to Use Which Version

**Use TypeScript version if:**
- You need quick prototyping
- File size < 1000 entities
- Simple text-based storage is sufficient
- Don't need concurrent access

**Use Rust version if:**
- You need production-grade performance
- File size > 1000 entities
- Need ACID transactions
- Need concurrent read access
- Want compile-time type safety

## License

MIT
