use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;
use crate::storage::Database;
use crate::graph::{Entity, Relation, KnowledgeGraph, ObservationInput, ObservationResult, ObservationDeletion};

/// Manager for knowledge graph operations
/// Provides async API wrapping SQLite database
pub struct KnowledgeGraphManager {
    db: Arc<Database>,
}

impl KnowledgeGraphManager {
    /// Create new manager with database at given path
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let db = Database::open(&db_path)?;
        Ok(Self {
            db: Arc::new(db),
        })
    }

    /// Create entities (returns only newly created entities)
    pub async fn create_entities(&self, entities: Vec<Entity>) -> Result<Vec<Entity>> {
        self.db.create_entities(&entities)
    }

    /// Create relations (returns only newly created relations)
    pub async fn create_relations(&self, relations: Vec<Relation>) -> Result<Vec<Relation>> {
        self.db.create_relations(&relations)
    }

    /// Add observations to multiple entities (batch operation)
    pub async fn add_observations(&self, inputs: Vec<ObservationInput>) -> Result<Vec<ObservationResult>> {
        self.db.add_observations(&inputs)
    }

    /// Delete entities (cascade deletes relations via FOREIGN KEY)
    pub async fn delete_entities(&self, names: Vec<String>) -> Result<usize> {
        self.db.delete_entities(&names)
    }

    /// Delete observations from multiple entities (batch operation)
    pub async fn delete_observations(&self, deletions: Vec<ObservationDeletion>) -> Result<()> {
        self.db.delete_observations(&deletions)
    }

    /// Delete relations
    pub async fn delete_relations(&self, relations: Vec<Relation>) -> Result<usize> {
        self.db.delete_relations(&relations)
    }

    /// Read entire knowledge graph
    pub async fn read_graph(&self) -> Result<KnowledgeGraph> {
        self.db.read_graph()
    }

    /// Search nodes using FTS5 full-text search
    pub async fn search_nodes(&self, query: Option<String>) -> Result<KnowledgeGraph> {
        self.db.search_nodes(query.as_deref())
    }

    /// Open specific nodes by names
    pub async fn open_nodes(&self, names: Vec<String>) -> Result<KnowledgeGraph> {
        self.db.open_nodes(&names)
    }
}
