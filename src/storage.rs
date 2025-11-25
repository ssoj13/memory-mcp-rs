use rusqlite::{params, OptionalExtension};
use anyhow::{Result, Context, bail};
use std::path::Path;
use r2d2_sqlite::SqliteConnectionManager;
use r2d2::Pool;
use crate::graph::{Entity, Relation, KnowledgeGraph, ObservationInput, ObservationResult, ObservationDeletion};

// Validation constants
const MAX_NAME_LENGTH: usize = 256;
const MAX_TYPE_LENGTH: usize = 128;
const MAX_OBSERVATION_LENGTH: usize = 4096;

/// Validate entity/relation name (alphanumeric, spaces, dashes, underscores, dots)
fn validate_name(name: &str, field: &str) -> Result<()> {
    if name.is_empty() {
        bail!("{} cannot be empty", field);
    }
    if name.len() > MAX_NAME_LENGTH {
        bail!("{} too long (max {} chars)", field, MAX_NAME_LENGTH);
    }
    // Check for control characters and null bytes
    if name.chars().any(|c| c.is_control() || c == '\0') {
        bail!("{} contains invalid characters", field);
    }
    Ok(())
}

/// Validate type (alphanumeric, dashes, underscores)
fn validate_type(type_str: &str, field: &str) -> Result<()> {
    if type_str.is_empty() {
        bail!("{} cannot be empty", field);
    }
    if type_str.len() > MAX_TYPE_LENGTH {
        bail!("{} too long (max {} chars)", field, MAX_TYPE_LENGTH);
    }
    // Only allow alphanumeric, dash, underscore, dot, colon (for namespaced types)
    if !type_str.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ':') {
        bail!("{} contains invalid characters (only alphanumeric, -, _, ., : allowed)", field);
    }
    Ok(())
}

/// Validate observation content
fn validate_observation(obs: &str) -> Result<()> {
    if obs.len() > MAX_OBSERVATION_LENGTH {
        bail!("Observation too long (max {} chars)", MAX_OBSERVATION_LENGTH);
    }
    // Check for null bytes (control characters in observations might be valid)
    if obs.contains('\0') {
        bail!("Observation contains null bytes");
    }
    Ok(())
}

/// Build SQL placeholders for IN queries (?1, ?2, ?3, ...)
/// offset: starting placeholder number (default 1)
fn build_placeholders(count: usize, offset: usize) -> String {
    (offset..offset + count)
        .map(|i| format!("?{}", i))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Validate database file path
fn validate_db_path(path: &Path) -> Result<()> {
    // Check file extension FIRST (before any filesystem operations)
    if let Some(ext) = path.extension() {
        if ext != "db" {
            bail!("Invalid database file extension (must be .db)");
        }
    } else {
        bail!("Database path must have .db extension");
    }
    Ok(())
}

const SCHEMA: &str = r#"
-- Entities table
CREATE TABLE IF NOT EXISTS entities (
    name TEXT PRIMARY KEY NOT NULL,
    entity_type TEXT NOT NULL,
    observations TEXT NOT NULL
) STRICT;

-- Relations table with FOREIGN KEY for cascade delete
CREATE TABLE IF NOT EXISTS relations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    from_entity TEXT NOT NULL,
    to_entity TEXT NOT NULL,
    relation_type TEXT NOT NULL,
    UNIQUE(from_entity, to_entity, relation_type),
    FOREIGN KEY(from_entity) REFERENCES entities(name) ON DELETE CASCADE,
    FOREIGN KEY(to_entity) REFERENCES entities(name) ON DELETE CASCADE
) STRICT;

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_entity_type ON entities(entity_type);
CREATE INDEX IF NOT EXISTS idx_from ON relations(from_entity);
CREATE INDEX IF NOT EXISTS idx_to ON relations(to_entity);
CREATE INDEX IF NOT EXISTS idx_relation_type ON relations(relation_type);

-- Compound indexes for complex queries
CREATE INDEX IF NOT EXISTS idx_relations_from_type ON relations(from_entity, relation_type);
CREATE INDEX IF NOT EXISTS idx_relations_to_type ON relations(to_entity, relation_type);

-- FTS5 virtual table for full-text search
CREATE VIRTUAL TABLE IF NOT EXISTS entities_fts USING fts5(
    name,
    entity_type,
    observations,
    content='entities',
    content_rowid='rowid'
);

-- Triggers to keep FTS5 in sync with entities table
CREATE TRIGGER IF NOT EXISTS entities_ai AFTER INSERT ON entities BEGIN
    INSERT INTO entities_fts(rowid, name, entity_type, observations)
    VALUES (new.rowid, new.name, new.entity_type, new.observations);
END;

CREATE TRIGGER IF NOT EXISTS entities_ad AFTER DELETE ON entities BEGIN
    INSERT INTO entities_fts(entities_fts, rowid, name, entity_type, observations)
    VALUES ('delete', old.rowid, old.name, old.entity_type, old.observations);
END;

CREATE TRIGGER IF NOT EXISTS entities_au AFTER UPDATE ON entities BEGIN
    INSERT INTO entities_fts(entities_fts, rowid, name, entity_type, observations)
    VALUES ('delete', old.rowid, old.name, old.entity_type, old.observations);
    INSERT INTO entities_fts(rowid, name, entity_type, observations)
    VALUES (new.rowid, new.name, new.entity_type, new.observations);
END;
"#;

pub struct Database {
    pool: Pool<SqliteConnectionManager>,
}

impl Database {
    /// Open or create database with connection pool
    pub fn open(path: &Path) -> Result<Self> {
        // Validate path first
        validate_db_path(path)?;

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let manager = SqliteConnectionManager::file(path);
        let pool = Pool::builder()
            .max_size(15) // Allow up to 15 concurrent connections
            .build(manager)
            .context("Failed to create connection pool")?;

        // Initialize schema and pragmas on a connection
        {
            let conn = pool.get().context("Failed to get connection from pool")?;

            // Enable FOREIGN KEY constraints (off by default!)
            conn.execute_batch("PRAGMA foreign_keys = ON;")?;

            // WAL mode for concurrent reads
            conn.execute_batch("PRAGMA journal_mode = WAL;")?;

            // Create schema
            conn.execute_batch(SCHEMA)?;
        }

        Ok(Self { pool })
    }

    /// Create entities (returns only newly created entities)
    /// Optimized: Uses INSERT OR IGNORE with tracking, no full table scan
    /// Wrapped in transaction for atomicity
    pub fn create_entities(&self, entities: &[Entity]) -> Result<Vec<Entity>> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }

        // Validate all entities before starting transaction
        for entity in entities {
            validate_name(&entity.name, "Entity name")?;
            validate_type(&entity.entity_type, "Entity type")?;
            for obs in &entity.observations {
                validate_observation(obs)?;
            }
        }

        let conn = self.pool.get()
            .context("Failed to get database connection from pool")?;
        let tx = conn.unchecked_transaction()
            .context("Failed to start transaction for creating entities")?;
        let mut new_entities = Vec::new();

        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO entities (name, entity_type, observations) VALUES (?1, ?2, ?3)"
            )
            .context("Failed to prepare insert statement for entities")?;

            // INSERT OR IGNORE returns 0 if row already exists, 1 if inserted
            for entity in entities {
                let obs_json = serde_json::to_string(&entity.observations)
                    .context(format!("Failed to serialize observations for entity '{}'", entity.name))?;
                let rows_affected = stmt.execute(params![
                    &entity.name,
                    &entity.entity_type,
                    &obs_json,
                ])
                .with_context(|| format!("Failed to insert entity '{}'", entity.name))?;

                // Track only newly inserted entities
                if rows_affected > 0 {
                    new_entities.push(entity.clone());
                }
            }
        }

        tx.commit()
            .context("Failed to commit transaction for creating entities")?;
        Ok(new_entities)
    }

    /// Create relations (returns only newly created relations)
    /// Optimized: Uses INSERT OR IGNORE with tracking, no full table scan
    /// Wrapped in transaction for atomicity
    pub fn create_relations(&self, relations: &[Relation]) -> Result<Vec<Relation>> {
        if relations.is_empty() {
            return Ok(Vec::new());
        }

        // Validate all relations before starting transaction
        for rel in relations {
            validate_name(&rel.from, "From entity")?;
            validate_name(&rel.to, "To entity")?;
            validate_type(&rel.relation_type, "Relation type")?;
        }

        let conn = self.pool.get()
            .context("Failed to get database connection from pool")?;
        let tx = conn.unchecked_transaction()
            .context("Failed to start transaction for creating relations")?;
        let mut new_relations = Vec::new();

        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO relations (from_entity, to_entity, relation_type) VALUES (?1, ?2, ?3)"
            )
            .context("Failed to prepare insert statement for relations")?;

            // INSERT OR IGNORE returns 0 if duplicate, 1 if inserted
            for rel in relations {
                // FOREIGN KEY constraint validates entity existence
                match stmt.execute(params![&rel.from, &rel.to, &rel.relation_type]) {
                    Ok(rows_affected) => {
                        // Track only newly inserted relations
                        if rows_affected > 0 {
                            new_relations.push(rel.clone());
                        }
                    }
                    Err(rusqlite::Error::SqliteFailure(err, _)) => {
                        if err.code == rusqlite::ErrorCode::ConstraintViolation {
                            anyhow::bail!(
                                "Cannot create relation '{}' -> '{}' (type: '{}'): one or both entities do not exist",
                                rel.from, rel.to, rel.relation_type
                            );
                        }
                        return Err(err).with_context(|| format!(
                            "Database error creating relation '{}' -> '{}'",
                            rel.from, rel.to
                        ));
                    }
                    Err(e) => return Err(e).with_context(|| format!(
                        "Failed to insert relation '{}' -> '{}' (type: '{}')",
                        rel.from, rel.to, rel.relation_type
                    )),
                }
            }
        }

        tx.commit()
            .context("Failed to commit transaction for creating relations")?;
        Ok(new_relations)
    }

    /// Add observations to multiple entities (batch operation)
    /// Wrapped in transaction for atomicity
    pub fn add_observations(&self, inputs: &[ObservationInput]) -> Result<Vec<ObservationResult>> {
        // Validate all inputs before starting transaction
        for input in inputs {
            validate_name(&input.entity_name, "Entity name")?;
            for obs in &input.contents {
                validate_observation(obs)?;
            }
        }

        let conn = self.pool.get()
            .context("Failed to get database connection from pool")?;
        let tx = conn.unchecked_transaction()
            .context("Failed to start transaction for adding observations")?;
        let mut results = Vec::new();

        for input in inputs {
            // Get current observations
            let current: Option<String> = tx.query_row(
                "SELECT observations FROM entities WHERE name = ?1",
                params![&input.entity_name],
                |row| row.get(0),
            )
            .optional()
            .with_context(|| format!("Database error querying entity '{}'", input.entity_name))?;

            let current = current.with_context(|| format!(
                "Cannot add observations: entity '{}' does not exist",
                input.entity_name
            ))?;

            // Parse JSON array
            let mut observations: Vec<String> = serde_json::from_str(&current)
                .with_context(|| format!("Corrupted observations data for entity '{}'", input.entity_name))?;

            // Track which observations are actually added
            let mut added = Vec::new();
            for obs in &input.contents {
                if !observations.contains(obs) {
                    observations.push(obs.clone());
                    added.push(obs.clone());
                }
            }

            // Update only if something was added
            if !added.is_empty() {
                let obs_json = serde_json::to_string(&observations)
                    .with_context(|| format!("Failed to serialize observations for entity '{}'", input.entity_name))?;
                tx.execute(
                    "UPDATE entities SET observations = ?1 WHERE name = ?2",
                    params![&obs_json, &input.entity_name],
                )
                .with_context(|| format!("Failed to update observations for entity '{}'", input.entity_name))?;
            }

            results.push(ObservationResult {
                entity_name: input.entity_name.clone(),
                added_observations: added,
            });
        }

        tx.commit()
            .context("Failed to commit transaction for adding observations")?;
        Ok(results)
    }

    /// Delete entities (cascade delete via FOREIGN KEY)
    pub fn delete_entities(&self, names: &[String]) -> Result<usize> {
        if names.is_empty() {
            return Ok(0);
        }

        // Validate all entity names
        for name in names {
            validate_name(name, "Entity name")?;
        }

        let conn = self.pool.get()
            .context("Failed to get database connection from pool")?;

        let placeholders = build_placeholders(names.len(), 1);
        let query = format!("DELETE FROM entities WHERE name IN ({})", placeholders);

        let params: Vec<&dyn rusqlite::ToSql> = names.iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();

        let count = conn.execute(&query, params.as_slice())
            .context(format!("Failed to delete {} entities", names.len()))?;

        // FOREIGN KEY CASCADE auto-deletes relations!

        Ok(count)
    }

    /// Delete observations from multiple entities (batch operation)
    /// Wrapped in transaction for atomicity
    pub fn delete_observations(&self, deletions: &[ObservationDeletion]) -> Result<()> {
        // Validate all deletions before starting transaction
        for deletion in deletions {
            validate_name(&deletion.entity_name, "Entity name")?;
        }

        let conn = self.pool.get()
            .context("Failed to get database connection from pool")?;
        let tx = conn.unchecked_transaction()
            .context("Failed to start transaction for deleting observations")?;

        for deletion in deletions {
            let current: Option<String> = tx.query_row(
                "SELECT observations FROM entities WHERE name = ?1",
                params![&deletion.entity_name],
                |row| row.get(0),
            )
            .optional()
            .with_context(|| format!("Database error querying entity '{}'", deletion.entity_name))?;

            let current = current.with_context(|| format!(
                "Cannot delete observations: entity '{}' does not exist",
                deletion.entity_name
            ))?;

            let mut observations: Vec<String> = serde_json::from_str(&current)
                .with_context(|| format!("Corrupted observations data for entity '{}'", deletion.entity_name))?;
            observations.retain(|obs| !deletion.observations.contains(obs));

            let obs_json = serde_json::to_string(&observations)
                .with_context(|| format!("Failed to serialize observations for entity '{}'", deletion.entity_name))?;
            tx.execute(
                "UPDATE entities SET observations = ?1 WHERE name = ?2",
                params![&obs_json, &deletion.entity_name],
            )
            .with_context(|| format!("Failed to delete observations from entity '{}'", deletion.entity_name))?;
        }

        tx.commit()
            .context("Failed to commit transaction for deleting observations")?;
        Ok(())
    }

    /// Delete relations
    /// Wrapped in transaction for atomicity
    pub fn delete_relations(&self, relations: &[Relation]) -> Result<usize> {
        if relations.is_empty() {
            return Ok(0);
        }

        // Validate all relations before starting transaction
        for rel in relations {
            validate_name(&rel.from, "From entity")?;
            validate_name(&rel.to, "To entity")?;
            validate_type(&rel.relation_type, "Relation type")?;
        }

        let conn = self.pool.get()?;
        let tx = conn.unchecked_transaction()?;
        let mut count = 0;

        {
            let mut stmt = tx.prepare_cached(
                "DELETE FROM relations WHERE from_entity = ?1 AND to_entity = ?2 AND relation_type = ?3"
            )?;

            for rel in relations {
                count += stmt.execute(params![&rel.from, &rel.to, &rel.relation_type])?;
            }
        }

        tx.commit()?;
        Ok(count)
    }

    /// Read entire graph
    pub fn read_graph(&self) -> Result<KnowledgeGraph> {
        let conn = self.pool.get()?;
        let mut entities = Vec::new();
        let mut stmt = conn.prepare("SELECT name, entity_type, observations FROM entities")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        for row in rows {
            let (name, entity_type, obs_json) = row?;
            let observations: Vec<String> = serde_json::from_str(&obs_json)?;
            entities.push(Entity { name, entity_type, observations });
        }

        let mut relations = Vec::new();
        let mut stmt = conn.prepare("SELECT from_entity, to_entity, relation_type FROM relations")?;
        let rows = stmt.query_map([], |row| {
            Ok(Relation {
                from: row.get(0)?,
                to: row.get(1)?,
                relation_type: row.get(2)?,
            })
        })?;

        for row in rows {
            relations.push(row?);
        }

        Ok(KnowledgeGraph { entities, relations })
    }

    /// Search using FTS5 full-text search
    pub fn search_nodes(&self, query: Option<&str>) -> Result<KnowledgeGraph> {
        let conn = self.pool.get()?;
        let entities = if let Some(q) = query {
            // FTS5 search - much faster than LIKE for text search
            let mut stmt = conn.prepare(
                "SELECT e.name, e.entity_type, e.observations
                 FROM entities e
                 INNER JOIN entities_fts fts ON e.rowid = fts.rowid
                 WHERE entities_fts MATCH ?1"
            )?;

            let rows = stmt.query_map(params![q], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;

            let mut entities = Vec::new();
            for row in rows {
                let (name, entity_type, obs_json) = row?;
                let observations: Vec<String> = serde_json::from_str(&obs_json)?;
                entities.push(Entity { name, entity_type, observations });
            }
            entities
        } else {
            // All entities - direct read without extra call
            let mut stmt = conn.prepare("SELECT name, entity_type, observations FROM entities")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;

            let mut entities = Vec::new();
            for row in rows {
                let (name, entity_type, obs_json) = row?;
                let observations: Vec<String> = serde_json::from_str(&obs_json)?;
                entities.push(Entity { name, entity_type, observations });
            }
            entities
        };

        // Filter relations (only between found entities)
        let entity_names: std::collections::HashSet<_> =
            entities.iter().map(|e| &e.name).collect();

        let mut relations = Vec::new();
        if !entity_names.is_empty() {
            let placeholders_from = build_placeholders(entity_names.len(), 1);
            let placeholders_to = build_placeholders(entity_names.len(), entity_names.len() + 1);

            let query = format!(
                "SELECT from_entity, to_entity, relation_type FROM relations
                 WHERE from_entity IN ({}) AND to_entity IN ({})",
                placeholders_from, placeholders_to
            );

            let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
            for name in &entity_names {
                params.push(name);
            }
            for name in &entity_names {
                params.push(name);
            }

            let mut stmt = conn.prepare(&query)?;
            let rows = stmt.query_map(params.as_slice(), |row| {
                Ok(Relation {
                    from: row.get(0)?,
                    to: row.get(1)?,
                    relation_type: row.get(2)?,
                })
            })?;

            for row in rows {
                relations.push(row?);
            }
        }

        Ok(KnowledgeGraph { entities, relations })
    }

    /// Open specific nodes
    pub fn open_nodes(&self, names: &[String]) -> Result<KnowledgeGraph> {
        if names.is_empty() {
            return Ok(KnowledgeGraph::default());
        }

        // Validate all entity names
        for name in names {
            validate_name(name, "Entity name")?;
        }

        let conn = self.pool.get()?;

        let placeholders = build_placeholders(names.len(), 1);

        // Get entities
        let query = format!(
            "SELECT name, entity_type, observations FROM entities WHERE name IN ({})",
            placeholders
        );

        let params: Vec<&dyn rusqlite::ToSql> = names.iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();

        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        let mut entities = Vec::new();
        for row in rows {
            let (name, entity_type, obs_json) = row?;
            let observations: Vec<String> = serde_json::from_str(&obs_json)?;
            entities.push(Entity { name, entity_type, observations });
        }

        // Get relations
        let placeholders_from = build_placeholders(names.len(), 1);
        let placeholders_to = build_placeholders(names.len(), names.len() + 1);

        let query = format!(
            "SELECT from_entity, to_entity, relation_type FROM relations
             WHERE from_entity IN ({}) AND to_entity IN ({})",
            placeholders_from, placeholders_to
        );

        let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
        for name in names {
            params.push(name);
        }
        for name in names {
            params.push(name);
        }

        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok(Relation {
                from: row.get(0)?,
                to: row.get(1)?,
                relation_type: row.get(2)?,
            })
        })?;

        let mut relations = Vec::new();
        for row in rows {
            relations.push(row?);
        }

        Ok(KnowledgeGraph { entities, relations })
    }
}
