use rusqlite::{Connection, params, OptionalExtension};
use anyhow::{Result, Context};
use std::path::Path;
use crate::graph::{Entity, Relation, KnowledgeGraph, ObservationInput, ObservationResult, ObservationDeletion};

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

-- Note: FTS5 removed for simplicity and compatibility
-- Using LIKE queries for search instead
"#;

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open or create database
    pub fn open(path: &Path) -> Result<Self> {
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)
            .context("Failed to open SQLite database")?;

        // Enable FOREIGN KEY constraints (off by default!)
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        // WAL mode for concurrent reads
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;

        // Create schema
        conn.execute_batch(SCHEMA)?;

        Ok(Self { conn })
    }

    /// Create entities (returns only newly created entities)
    pub fn create_entities(&self, entities: &[Entity]) -> Result<Vec<Entity>> {
        // Check which entities already exist
        let mut existing_names = std::collections::HashSet::new();
        if !entities.is_empty() {
            let placeholders = (1..=entities.len())
                .map(|i| format!("?{}", i))
                .collect::<Vec<_>>()
                .join(", ");

            let query = format!("SELECT name FROM entities WHERE name IN ({})", placeholders);
            let params: Vec<&dyn rusqlite::ToSql> = entities.iter()
                .map(|e| &e.name as &dyn rusqlite::ToSql)
                .collect();

            let mut stmt = self.conn.prepare(&query)?;
            let rows = stmt.query_map(params.as_slice(), |row| row.get::<_, String>(0))?;

            for row in rows {
                existing_names.insert(row?);
            }
        }

        // Filter out existing entities
        let new_entities: Vec<_> = entities.iter()
            .filter(|e| !existing_names.contains(&e.name))
            .cloned()
            .collect();

        // Insert new entities
        if !new_entities.is_empty() {
            let mut stmt = self.conn.prepare_cached(
                "INSERT INTO entities (name, entity_type, observations) VALUES (?1, ?2, ?3)"
            )?;

            for entity in &new_entities {
                let obs_json = serde_json::to_string(&entity.observations)?;
                stmt.execute(params![
                    &entity.name,
                    &entity.entity_type,
                    &obs_json,
                ])?;
            }
        }

        Ok(new_entities)
    }

    /// Create relations (returns only newly created relations)
    pub fn create_relations(&self, relations: &[Relation]) -> Result<Vec<Relation>> {
        // Check which relations already exist
        let mut existing_relations = std::collections::HashSet::new();
        if !relations.is_empty() {
            let mut stmt = self.conn.prepare(
                "SELECT from_entity, to_entity, relation_type FROM relations"
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;

            for row in rows {
                let (from, to, rel_type) = row?;
                existing_relations.insert((from, to, rel_type));
            }
        }

        // Filter out existing relations
        let new_relations: Vec<_> = relations.iter()
            .filter(|r| !existing_relations.contains(&(r.from.clone(), r.to.clone(), r.relation_type.clone())))
            .cloned()
            .collect();

        // Insert new relations
        if !new_relations.is_empty() {
            let mut stmt = self.conn.prepare_cached(
                "INSERT INTO relations (from_entity, to_entity, relation_type) VALUES (?1, ?2, ?3)"
            )?;

            for rel in &new_relations {
                // FOREIGN KEY constraint validates entity existence
                match stmt.execute(params![&rel.from, &rel.to, &rel.relation_type]) {
                    Ok(_) => {},
                    Err(rusqlite::Error::SqliteFailure(err, _)) => {
                        if err.code == rusqlite::ErrorCode::ConstraintViolation {
                            anyhow::bail!(
                                "Entities must exist: from='{}', to='{}'",
                                rel.from, rel.to
                            );
                        }
                        return Err(err.into());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }

        Ok(new_relations)
    }

    /// Add observations to multiple entities (batch operation)
    pub fn add_observations(&self, inputs: &[ObservationInput]) -> Result<Vec<ObservationResult>> {
        let mut results = Vec::new();

        for input in inputs {
            // Get current observations
            let current: Option<String> = self.conn.query_row(
                "SELECT observations FROM entities WHERE name = ?1",
                params![&input.entity_name],
                |row| row.get(0),
            ).optional()?;

            let current = current.with_context(|| format!("Entity '{}' not found", input.entity_name))?;

            // Parse JSON array
            let mut observations: Vec<String> = serde_json::from_str(&current)?;

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
                let obs_json = serde_json::to_string(&observations)?;
                self.conn.execute(
                    "UPDATE entities SET observations = ?1 WHERE name = ?2",
                    params![&obs_json, &input.entity_name],
                )?;
            }

            results.push(ObservationResult {
                entity_name: input.entity_name.clone(),
                added_observations: added,
            });
        }

        Ok(results)
    }

    /// Delete entities (cascade delete via FOREIGN KEY)
    pub fn delete_entities(&self, names: &[String]) -> Result<usize> {
        if names.is_empty() {
            return Ok(0);
        }

        // Prepare placeholders: ?1, ?2, ?3, ...
        let placeholders = (1..=names.len())
            .map(|i| format!("?{}", i))
            .collect::<Vec<_>>()
            .join(", ");

        let query = format!("DELETE FROM entities WHERE name IN ({})", placeholders);

        let params: Vec<&dyn rusqlite::ToSql> = names.iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();

        let count = self.conn.execute(&query, params.as_slice())?;

        // FOREIGN KEY CASCADE auto-deletes relations!

        Ok(count)
    }

    /// Delete observations from multiple entities (batch operation)
    pub fn delete_observations(&self, deletions: &[ObservationDeletion]) -> Result<()> {
        for deletion in deletions {
            let current: Option<String> = self.conn.query_row(
                "SELECT observations FROM entities WHERE name = ?1",
                params![&deletion.entity_name],
                |row| row.get(0),
            ).optional()?;

            let current = current.with_context(|| format!("Entity '{}' not found", deletion.entity_name))?;

            let mut observations: Vec<String> = serde_json::from_str(&current)?;
            observations.retain(|obs| !deletion.observations.contains(obs));

            let obs_json = serde_json::to_string(&observations)?;
            self.conn.execute(
                "UPDATE entities SET observations = ?1 WHERE name = ?2",
                params![&obs_json, &deletion.entity_name],
            )?;
        }

        Ok(())
    }

    /// Delete relations
    pub fn delete_relations(&self, relations: &[Relation]) -> Result<usize> {
        if relations.is_empty() {
            return Ok(0);
        }

        let mut stmt = self.conn.prepare_cached(
            "DELETE FROM relations WHERE from_entity = ?1 AND to_entity = ?2 AND relation_type = ?3"
        )?;

        let mut count = 0;
        for rel in relations {
            count += stmt.execute(params![&rel.from, &rel.to, &rel.relation_type])?;
        }

        Ok(count)
    }

    /// Read entire graph
    pub fn read_graph(&self) -> Result<KnowledgeGraph> {
        let mut entities = Vec::new();
        let mut stmt = self.conn.prepare("SELECT name, entity_type, observations FROM entities")?;
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
        let mut stmt = self.conn.prepare("SELECT from_entity, to_entity, relation_type FROM relations")?;
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

    /// Search using LIKE queries (simple full-text search)
    pub fn search_nodes(&self, query: Option<&str>) -> Result<KnowledgeGraph> {
        let entities = if let Some(q) = query {
            // LIKE search (case-insensitive)
            let pattern = format!("%{}%", q);
            let mut stmt = self.conn.prepare(
                "SELECT name, entity_type, observations
                 FROM entities
                 WHERE name LIKE ?1 COLLATE NOCASE
                    OR entity_type LIKE ?1 COLLATE NOCASE
                    OR observations LIKE ?1 COLLATE NOCASE"
            )?;

            let rows = stmt.query_map(params![&pattern], |row| {
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
            // All entities
            self.read_graph()?.entities
        };

        // Filter relations (only between found entities)
        let entity_names: std::collections::HashSet<_> =
            entities.iter().map(|e| &e.name).collect();

        let mut relations = Vec::new();
        if !entity_names.is_empty() {
            let placeholders_from = (1..=entity_names.len())
                .map(|i| format!("?{}", i))
                .collect::<Vec<_>>()
                .join(", ");

            let placeholders_to = ((entity_names.len() + 1)..=(entity_names.len() * 2))
                .map(|i| format!("?{}", i))
                .collect::<Vec<_>>()
                .join(", ");

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

            let mut stmt = self.conn.prepare(&query)?;
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

        let placeholders = (1..=names.len())
            .map(|i| format!("?{}", i))
            .collect::<Vec<_>>()
            .join(", ");

        // Get entities
        let query = format!(
            "SELECT name, entity_type, observations FROM entities WHERE name IN ({})",
            placeholders
        );

        let params: Vec<&dyn rusqlite::ToSql> = names.iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();

        let mut stmt = self.conn.prepare(&query)?;
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
        let placeholders_from = (1..=names.len())
            .map(|i| format!("?{}", i))
            .collect::<Vec<_>>()
            .join(", ");

        let placeholders_to = ((names.len() + 1)..=(names.len() * 2))
            .map(|i| format!("?{}", i))
            .collect::<Vec<_>>()
            .join(", ");

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

        let mut stmt = self.conn.prepare(&query)?;
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
