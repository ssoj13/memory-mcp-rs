# Bug Hunt Audit Plan - memory-mcp-rs

## Executive Summary

After thorough code review of the memory-mcp-rs project (SQLite-based knowledge graph MCP server), I've identified several issues ranging from potential bugs, dead code, inconsistencies, and improvements.

---

## 1. Critical Issues

### 1.1 Unused thiserror dependency
**Location:** `Cargo.toml:47`
**Issue:** `thiserror = "2.0.17"` is declared as dependency but never used anywhere in code. All error handling uses `anyhow` only.
**Fix:** Remove unused dependency

### 1.2 lib.rs exposes logging module that doesn't exist in public API
**Location:** `src/lib.rs`
**Issue:** The lib.rs only exports `graph`, `manager`, `storage` but `logging` module is private. This is correct, but potentially confusing since logging is declared as `mod` in main.rs but not in lib.rs.
**Verdict:** This is actually correct - logging should not be in lib.

---

## 2. Code Quality Issues

### 2.1 Duplicate code in storage.rs - search_nodes vs read_graph
**Location:** `src/storage.rs:655-677` vs `src/storage.rs:567-608`
**Issue:** The `search_nodes` function when query is `None` or empty has duplicated entity reading logic identical to parts of `read_graph()`. Lines 656-677 could just call `self.read_graph()`.
**Current code (lines 655-677):**
```rust
// All entities - direct read without extra call
let mut stmt = conn.prepare("SELECT name, entity_type, observations FROM entities")?;
// ... same logic as read_graph()
```
**Fix:** Refactor to reuse read_graph for the no-query case.

### 2.2 Inconsistent error context in storage.rs
**Location:** `src/storage.rs:567-568`
**Issue:** `read_graph()` doesn't use `.context()` for error enrichment, while other functions do (e.g., `create_entities`, `add_observations`). This leads to less informative errors.
**Fix:** Add `.context("Failed to read knowledge graph")` to the pool.get() call.

### 2.3 Redundant return statement
**Location:** `src/storage.rs:619-621`  
**Issue:** Early return when empty query:
```rust
if trimmed.is_empty() {
    return self.read_graph();
}
```
This works but then we have similar logic for `None` case at lines 655-677. The whole function could be simplified.

---

## 3. Potential Bugs

### 3.1 FTS5 query escaping may cause empty results
**Location:** `src/storage.rs:90-101` (`sanitize_fts5_query`)
**Issue:** The function quotes every term with double quotes for phrase matching. If user passes query like `"test"` (already quoted), it becomes `"""test"""` which may cause issues.
**Example:**
- Input: `"hello world"` (user wants phrase)
- Output: `"\"hello" "world\""` - incorrect parsing
**Fix:** Consider stripping existing quotes from input before processing.

### 3.2 Relations query in search_nodes creates duplicate params
**Location:** `src/storage.rs:693-699`
**Issue:** The params vector is built by pushing each entity name twice:
```rust
for name in &entity_names {
    params.push(name);
}
for name in &entity_names {
    params.push(name);
}
```
This is correct for the query, but confusing. A comment would help.

---

## 4. Dead Code / Unused Features

### 4.1 ObservationResult's added_observations field
**Location:** `src/graph.rs:48-54`
**Issue:** `ObservationResult` struct is fully defined but the `added_observations` field data is computed but never used by the caller (MCP handler just returns count text).
**Verdict:** This is intentional - structured_content includes full results.

### 4.2 Unused import warnings potential
**Location:** Multiple files
**Issue:** Current code has no unused imports, but `build_placeholders` function could be made more generic.

---

## 5. Architecture Improvements

### 5.1 Transaction handling inconsistency
**Location:** `src/storage.rs`
**Issue:** Some methods use `unchecked_transaction()` (create_entities, create_relations, etc.), while `delete_entities` doesn't use a transaction at all.
```rust
// delete_entities - line 424-451 - no transaction!
let conn = self.pool.get()?;
let count = conn.execute(&query, params.as_slice())?;
```
**Risk:** If deleting multiple entities, a failure mid-way leaves partial state.
**Fix:** Wrap in transaction for atomicity.

### 5.2 Validation constants could be configurable
**Location:** `src/storage.rs:10-13`
**Issue:** `MAX_NAME_LENGTH`, `MAX_TYPE_LENGTH`, `MAX_OBSERVATION_LENGTH` are hardcoded. Consider making them configurable via env vars or config.
**Verdict:** Low priority - current values are reasonable.

---

## 6. Test Coverage Gaps

### 6.1 Missing edge case tests
- Empty string observation (allowed but untested)
- Unicode in entity names (partially tested)
- Concurrent access stress tests
- WAL mode behavior verification

### 6.2 HTTP transport tests are slow
**Location:** `tests/http_transport.rs`
**Issue:** Tests spawn full cargo process which is slow. Consider using the library directly.

---

## 7. Recommended Fixes (Priority Order)

| Priority | Issue | Action |
|----------|-------|--------|
| HIGH | 5.1 - delete_entities no transaction | Add transaction wrapper |
| MEDIUM | 2.1 - Duplicate code in search_nodes | Refactor to reuse read_graph |
| MEDIUM | 2.2 - Inconsistent error context | Add .context() to read_graph |
| LOW | 1.1 - Unused thiserror | Remove from Cargo.toml |
| LOW | 3.1 - FTS5 quote escaping | Add pre-strip of quotes |

---

## 8. Summary of Files Changed

If approved, the following files will be modified:

1. **Cargo.toml** - Remove unused `thiserror` dependency
2. **src/storage.rs** - Multiple fixes:
   - Add transaction to delete_entities
   - Refactor search_nodes to reuse read_graph
   - Add error context to read_graph
   - Improve FTS5 query sanitization
   - Add clarifying comments

---

## Awaiting Approval

Please review this plan and approve to proceed with implementation.
