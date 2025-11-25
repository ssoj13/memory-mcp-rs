use memory_mcp_rs::graph::{Entity, Relation, ObservationInput, ObservationDeletion};
use memory_mcp_rs::manager::KnowledgeGraphManager;
use tempfile::TempDir;

/// Helper to create temp database file with .db extension
fn create_temp_db() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.db");
    (dir, path)
}

#[tokio::test]
async fn test_create_and_read_entities() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    // Create entities
    let entities = vec![Entity {
        name: "Alice".to_string(),
        entity_type: "person".to_string(),
        observations: vec!["Works at Acme Corp".to_string()],
    }];

    let created = manager.create_entities(entities).await.unwrap();
    assert_eq!(created.len(), 1);

    // Read graph
    let graph = manager.read_graph().await.unwrap();
    assert_eq!(graph.entities.len(), 1);
    assert_eq!(graph.entities[0].name, "Alice");
    assert_eq!(graph.entities[0].entity_type, "person");
    assert_eq!(graph.entities[0].observations.len(), 1);
}

#[tokio::test]
async fn test_create_relations() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    // Create entities first
    let entities = vec![
        Entity {
            name: "Alice".to_string(),
            entity_type: "person".to_string(),
            observations: vec![],
        },
        Entity {
            name: "Acme Corp".to_string(),
            entity_type: "organization".to_string(),
            observations: vec![],
        },
    ];
    manager.create_entities(entities).await.unwrap();

    // Create relation
    let relations = vec![Relation {
        from: "Alice".to_string(),
        to: "Acme Corp".to_string(),
        relation_type: "works_at".to_string(),
    }];

    let created = manager.create_relations(relations).await.unwrap();
    assert_eq!(created.len(), 1);

    let graph = manager.read_graph().await.unwrap();
    assert_eq!(graph.relations.len(), 1);
    assert_eq!(graph.relations[0].from, "Alice");
    assert_eq!(graph.relations[0].to, "Acme Corp");
}

#[tokio::test]
async fn test_relation_requires_entities() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    // Try to create relation without entities (should fail)
    let relations = vec![Relation {
        from: "Alice".to_string(),
        to: "Bob".to_string(),
        relation_type: "knows".to_string(),
    }];

    let result = manager.create_relations(relations).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_deduplication() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    // Create same entity twice
    let entity = Entity {
        name: "Alice".to_string(),
        entity_type: "person".to_string(),
        observations: vec![],
    };

    let created1 = manager.create_entities(vec![entity.clone()]).await.unwrap();
    assert_eq!(created1.len(), 1);

    let created2 = manager.create_entities(vec![entity.clone()]).await.unwrap();
    assert_eq!(created2.len(), 0); // Duplicate ignored

    let graph = manager.read_graph().await.unwrap();
    assert_eq!(graph.entities.len(), 1); // Only one Alice
}

#[tokio::test]
async fn test_add_observations() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    // Create entity
    let entity = Entity {
        name: "Alice".to_string(),
        entity_type: "person".to_string(),
        observations: vec!["Works at Acme".to_string()],
    };
    manager.create_entities(vec![entity]).await.unwrap();

    // Add observation
    manager
        .add_observations(vec![ObservationInput {
            entity_name: "Alice".to_string(),
            contents: vec!["Lives in Paris".to_string()],
        }])
        .await
        .unwrap();

    let graph = manager.read_graph().await.unwrap();
    assert_eq!(graph.entities[0].observations.len(), 2);
    assert!(graph.entities[0].observations.contains(&"Works at Acme".to_string()));
    assert!(graph.entities[0].observations.contains(&"Lives in Paris".to_string()));
}

#[tokio::test]
async fn test_cascade_delete() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    // Create entities
    manager
        .create_entities(vec![
            Entity {
                name: "Alice".to_string(),
                entity_type: "person".to_string(),
                observations: vec![],
            },
            Entity {
                name: "Bob".to_string(),
                entity_type: "person".to_string(),
                observations: vec![],
            },
        ])
        .await
        .unwrap();

    // Create relation
    manager
        .create_relations(vec![Relation {
            from: "Alice".to_string(),
            to: "Bob".to_string(),
            relation_type: "knows".to_string(),
        }])
        .await
        .unwrap();

    // Delete Alice (should cascade delete relation)
    let count = manager.delete_entities(vec!["Alice".to_string()]).await.unwrap();
    assert_eq!(count, 1);

    let graph = manager.read_graph().await.unwrap();
    assert_eq!(graph.entities.len(), 1); // Only Bob
    assert_eq!(graph.relations.len(), 0); // Relation deleted
}

#[tokio::test]
async fn test_delete_observations() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    // Create entity with observations
    manager
        .create_entities(vec![Entity {
            name: "Alice".to_string(),
            entity_type: "person".to_string(),
            observations: vec![
                "Works at Acme".to_string(),
                "Lives in Paris".to_string(),
            ],
        }])
        .await
        .unwrap();

    // Delete one observation
    manager
        .delete_observations(vec![ObservationDeletion {
            entity_name: "Alice".to_string(),
            observations: vec!["Lives in Paris".to_string()],
        }])
        .await
        .unwrap();

    let graph = manager.read_graph().await.unwrap();
    assert_eq!(graph.entities[0].observations.len(), 1);
    assert_eq!(graph.entities[0].observations[0], "Works at Acme");
}

#[tokio::test]
async fn test_delete_relations() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    // Setup
    manager
        .create_entities(vec![
            Entity {
                name: "Alice".to_string(),
                entity_type: "person".to_string(),
                observations: vec![],
            },
            Entity {
                name: "Bob".to_string(),
                entity_type: "person".to_string(),
                observations: vec![],
            },
        ])
        .await
        .unwrap();

    manager
        .create_relations(vec![Relation {
            from: "Alice".to_string(),
            to: "Bob".to_string(),
            relation_type: "knows".to_string(),
        }])
        .await
        .unwrap();

    // Delete relation
    let count = manager
        .delete_relations(vec![Relation {
            from: "Alice".to_string(),
            to: "Bob".to_string(),
            relation_type: "knows".to_string(),
        }])
        .await
        .unwrap();

    assert_eq!(count, 1);

    let graph = manager.read_graph().await.unwrap();
    assert_eq!(graph.entities.len(), 2); // Entities still exist
    assert_eq!(graph.relations.len(), 0); // Relation deleted
}

#[tokio::test]
async fn test_search_nodes() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    manager
        .create_entities(vec![
            Entity {
                name: "Alice".to_string(),
                entity_type: "person".to_string(),
                observations: vec!["Lives in Paris".to_string()],
            },
            Entity {
                name: "Bob".to_string(),
                entity_type: "person".to_string(),
                observations: vec!["Lives in London".to_string()],
            },
        ])
        .await
        .unwrap();

    // Search by observation
    let result = manager.search_nodes(Some("Paris".to_string())).await.unwrap();
    assert_eq!(result.entities.len(), 1);
    assert_eq!(result.entities[0].name, "Alice");

    // Search by type
    let result = manager.search_nodes(Some("person".to_string())).await.unwrap();
    assert_eq!(result.entities.len(), 2);

    // Search all
    let result = manager.search_nodes(None).await.unwrap();
    assert_eq!(result.entities.len(), 2);
}

#[tokio::test]
async fn test_open_nodes() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    manager
        .create_entities(vec![
            Entity {
                name: "Alice".to_string(),
                entity_type: "person".to_string(),
                observations: vec![],
            },
            Entity {
                name: "Bob".to_string(),
                entity_type: "person".to_string(),
                observations: vec![],
            },
            Entity {
                name: "Charlie".to_string(),
                entity_type: "person".to_string(),
                observations: vec![],
            },
        ])
        .await
        .unwrap();

    // Open specific nodes
    let result = manager
        .open_nodes(vec!["Alice".to_string(), "Charlie".to_string()])
        .await
        .unwrap();

    assert_eq!(result.entities.len(), 2);
    let names: Vec<_> = result.entities.iter().map(|e| &e.name).collect();
    assert!(names.contains(&&"Alice".to_string()));
    assert!(names.contains(&&"Charlie".to_string()));
    assert!(!names.contains(&&"Bob".to_string()));
}

#[tokio::test]
async fn test_persistence() {
    let (_dir, path) = create_temp_db();
    let path = path;

    // Create and populate graph
    {
        let manager = KnowledgeGraphManager::new(path.clone()).unwrap();
        manager
            .create_entities(vec![Entity {
                name: "Alice".to_string(),
                entity_type: "person".to_string(),
                observations: vec!["Test".to_string()],
            }])
            .await
            .unwrap();
    }

    // Reopen and verify
    {
        let manager = KnowledgeGraphManager::new(path).unwrap();
        let graph = manager.read_graph().await.unwrap();
        assert_eq!(graph.entities.len(), 1);
        assert_eq!(graph.entities[0].name, "Alice");
    }
}

// ============================================================================
// VALIDATION TESTS (for new security requirements)
// ============================================================================

#[tokio::test]
async fn test_validation_empty_entity_name() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    let result = manager.create_entities(vec![Entity {
        name: "".to_string(), // Empty name
        entity_type: "person".to_string(),
        observations: vec![],
    }]).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty"));
}

#[tokio::test]
async fn test_validation_entity_name_too_long() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    let long_name = "A".repeat(257); // Max is 256
    let result = manager.create_entities(vec![Entity {
        name: long_name,
        entity_type: "person".to_string(),
        observations: vec![],
    }]).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("too long"));
}

#[tokio::test]
async fn test_validation_entity_name_invalid_chars() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    // Control character (null byte)
    let result = manager.create_entities(vec![Entity {
        name: "Alice\0Bob".to_string(),
        entity_type: "person".to_string(),
        observations: vec![],
    }]).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("invalid characters"));
}

#[tokio::test]
async fn test_validation_entity_type_invalid_chars() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    // Space is not allowed in types
    let result = manager.create_entities(vec![Entity {
        name: "Alice".to_string(),
        entity_type: "per son".to_string(), // Space not allowed
        observations: vec![],
    }]).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("invalid characters"));
}

#[tokio::test]
async fn test_validation_observation_too_long() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    let long_obs = "A".repeat(4097); // Max is 4096
    let result = manager.create_entities(vec![Entity {
        name: "Alice".to_string(),
        entity_type: "person".to_string(),
        observations: vec![long_obs],
    }]).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("too long"));
}

#[tokio::test]
async fn test_validation_relation_type_valid() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    // Valid type with allowed characters
    manager.create_entities(vec![
        Entity {
            name: "Alice".to_string(),
            entity_type: "person".to_string(),
            observations: vec![],
        },
        Entity {
            name: "Bob".to_string(),
            entity_type: "person".to_string(),
            observations: vec![],
        },
    ]).await.unwrap();

    let result = manager.create_relations(vec![Relation {
        from: "Alice".to_string(),
        to: "Bob".to_string(),
        relation_type: "work-relation:knows_v1.0".to_string(), // Valid: alphanumeric + -_.:
    }]).await;

    assert!(result.is_ok());
}

// ============================================================================
// FTS5 SEARCH TESTS
// ============================================================================

#[tokio::test]
async fn test_fts5_phrase_search() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    manager.create_entities(vec![
        Entity {
            name: "Alice".to_string(),
            entity_type: "person".to_string(),
            observations: vec!["Works at Acme Corporation".to_string()],
        },
        Entity {
            name: "Bob".to_string(),
            entity_type: "person".to_string(),
            observations: vec!["Works for different company".to_string()],
        },
    ]).await.unwrap();

    // FTS5 phrase search with quotes
    let result = manager.search_nodes(Some("\"Acme Corporation\"".to_string())).await.unwrap();
    assert_eq!(result.entities.len(), 1);
    assert_eq!(result.entities[0].name, "Alice");
}

#[tokio::test]
async fn test_fts5_multi_word_search() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    manager.create_entities(vec![
        Entity {
            name: "Alice".to_string(),
            entity_type: "person".to_string(),
            observations: vec!["Senior software engineer at Google".to_string()],
        },
        Entity {
            name: "Bob".to_string(),
            entity_type: "person".to_string(),
            observations: vec!["Junior developer at Microsoft".to_string()],
        },
    ]).await.unwrap();

    // Search for multiple words (FTS5 tokenizes them)
    let result = manager.search_nodes(Some("software engineer".to_string())).await.unwrap();
    assert_eq!(result.entities.len(), 1);
    assert_eq!(result.entities[0].name, "Alice");
}

// ============================================================================
// PATH VALIDATION TESTS
// ============================================================================

#[test]
fn test_path_validation_invalid_extension() {
    let tmp_dir = TempDir::new().unwrap();
    let invalid_path = tmp_dir.path().join("database.txt"); // Wrong extension

    let result = KnowledgeGraphManager::new(invalid_path);
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains(".db"));
    }
}

#[test]
fn test_path_validation_valid_extension() {
    let tmp_dir = TempDir::new().unwrap();
    let valid_path = tmp_dir.path().join("database.db");

    let result = KnowledgeGraphManager::new(valid_path);
    assert!(result.is_ok());
}

// ============================================================================
// ERROR CONTEXT TESTS
// ============================================================================

#[tokio::test]
async fn test_error_context_entity_not_found() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    // Try to add observation to non-existent entity
    let result = manager.add_observations(vec![ObservationInput {
        entity_name: "NonExistent".to_string(),
        contents: vec!["test".to_string()],
    }]).await;

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("NonExistent")); // Entity name in error
    assert!(err_msg.contains("does not exist")); // Clear message
}

#[tokio::test]
async fn test_error_context_relation_missing_entities() {
    let (_dir, path) = create_temp_db();
    let manager = KnowledgeGraphManager::new(path).unwrap();

    // Create only one entity
    manager.create_entities(vec![Entity {
        name: "Alice".to_string(),
        entity_type: "person".to_string(),
        observations: vec![],
    }]).await.unwrap();

    // Try to create relation to non-existent entity
    let result = manager.create_relations(vec![Relation {
        from: "Alice".to_string(),
        to: "Bob".to_string(), // Bob doesn't exist
        relation_type: "knows".to_string(),
    }]).await;

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Alice")); // From entity
    assert!(err_msg.contains("Bob")); // To entity
    assert!(err_msg.contains("does not exist") || err_msg.contains("do not exist"));
}
