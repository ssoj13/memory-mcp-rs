use memory_mcp_rs::graph::{Entity, Relation, ObservationInput, ObservationDeletion};
use memory_mcp_rs::manager::KnowledgeGraphManager;
use tempfile::NamedTempFile;

#[tokio::test]
async fn test_create_and_read_entities() {
    let tmp = NamedTempFile::new().unwrap();
    let manager = KnowledgeGraphManager::new(tmp.path().to_path_buf()).unwrap();

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
    let tmp = NamedTempFile::new().unwrap();
    let manager = KnowledgeGraphManager::new(tmp.path().to_path_buf()).unwrap();

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
    let tmp = NamedTempFile::new().unwrap();
    let manager = KnowledgeGraphManager::new(tmp.path().to_path_buf()).unwrap();

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
    let tmp = NamedTempFile::new().unwrap();
    let manager = KnowledgeGraphManager::new(tmp.path().to_path_buf()).unwrap();

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
    let tmp = NamedTempFile::new().unwrap();
    let manager = KnowledgeGraphManager::new(tmp.path().to_path_buf()).unwrap();

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
    let tmp = NamedTempFile::new().unwrap();
    let manager = KnowledgeGraphManager::new(tmp.path().to_path_buf()).unwrap();

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
    let tmp = NamedTempFile::new().unwrap();
    let manager = KnowledgeGraphManager::new(tmp.path().to_path_buf()).unwrap();

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
    let tmp = NamedTempFile::new().unwrap();
    let manager = KnowledgeGraphManager::new(tmp.path().to_path_buf()).unwrap();

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
    let tmp = NamedTempFile::new().unwrap();
    let manager = KnowledgeGraphManager::new(tmp.path().to_path_buf()).unwrap();

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
    let tmp = NamedTempFile::new().unwrap();
    let manager = KnowledgeGraphManager::new(tmp.path().to_path_buf()).unwrap();

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
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();

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
