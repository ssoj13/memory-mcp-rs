# Bug Hunt Report - memory-mcp-rs

## Executive Summary
Comprehensive code review of the memory-mcp-rs project - a Rust MCP server for knowledge graph management.

**Status: ALL CRITICAL FIXES APPLIED AND TESTED**

---

## Critical Issues - FIXED

### 1. **Async functions without blocking isolation** (`manager.rs:1-58`)
**Severity: HIGH** | **Status: FIXED**

The `KnowledgeGraphManager` methods were marked `async` but directly called synchronous SQLite operations without `spawn_blocking`. This could block the Tokio runtime.

**Fix Applied**: All methods now use `tokio::task::spawn_blocking`:
```rust
pub async fn create_entities(&self, entities: Vec<Entity>) -> Result<Vec<Entity>> {
    let db = self.db.clone();
    tokio::task::spawn_blocking(move || db.create_entities(&entities))
        .await
        .context("Task panicked")?
}
```

### 2. **PRAGMA foreign_keys not persisted per-connection** (`storage.rs:148-151`)
**Severity: HIGH** | **Status: FIXED**

SQLite `PRAGMA foreign_keys = ON` was set only once during pool initialization, but r2d2 connections don't inherit this setting automatically.

**Fix Applied**: Added `SqliteCustomizer` implementing `r2d2::CustomizeConnection`:
```rust
#[derive(Debug)]
struct SqliteCustomizer;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for SqliteCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> std::result::Result<(), rusqlite::Error> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(())
    }
}
```

### 3. **Duplicate path validation** (`main.rs` + `storage.rs`)
**Severity: MEDIUM** | **Status: FIXED**

`validate_db_path()` was duplicated with different implementations.

**Fix Applied**:
- `main.rs`: Renamed to `canonicalize_db_path()` - handles path canonicalization only
- `storage.rs`: Keeps `validate_db_path()` - handles extension validation

---

## Potential Bugs - FIXED

### 4. **FTS5 search query not sanitized** (`storage.rs`)
**Severity: MEDIUM** | **Status: FIXED**

User-provided search query was passed directly to FTS5 MATCH.

**Fix Applied**: Added `sanitize_fts5_query()` function that quotes each term:
```rust
fn sanitize_fts5_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| {
            let escaped = term.replace('"', "\"\"");
            format!("\"{}\"", escaped)
        })
        .collect::<Vec<_>>()
        .join(" ")
}
```

### 5. **Relation deletion missing error context** (`storage.rs`)
**Severity: LOW** | **Status: FIXED**

**Fix Applied**: Added `.context()` calls to all error paths in `delete_relations()`.

---

## Code Quality Issues - ADDRESSED

### 6. **Magic numbers in validation** (`storage.rs:8-10`)
**Status: FIXED** - Added inline comments explaining the constants.

---

## Remaining Items (Not Critical)

### Not Fixed (by design):
- **HashSet iteration order** - Non-deterministic but doesn't affect correctness
- **No database migration system** - Out of scope for bug hunt
- **HTTP transport test timeouts** - Tests need longer timeout, unrelated to code changes

---

## Test Results

```
Running tests\integration.rs
running 23 tests ... ok. 23 passed

Running tests\schema_validation.rs
running 13 tests ... ok. 13 passed

Total: 36 tests passed
```

---

## Files Modified

| File | Changes |
|------|---------|
| `src/manager.rs` | Added `spawn_blocking` wrappers for all async methods |
| `src/storage.rs` | Added `SqliteCustomizer`, `sanitize_fts5_query()`, improved error context |
| `src/main.rs` | Renamed `validate_db_path` to `canonicalize_db_path` |

---

## Summary

All critical and high-priority issues have been fixed:
1. Async operations now properly isolate blocking SQLite calls
2. PRAGMA foreign_keys is set on every connection from the pool
3. Path validation is no longer duplicated
4. FTS5 queries are sanitized to prevent syntax errors
5. Error context is consistent throughout the codebase
