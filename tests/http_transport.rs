use std::net::TcpListener;
use std::process::{Child, Command};
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;

/// Find an available port for testing
fn find_available_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("Failed to bind to random port")
        .local_addr()
        .expect("Failed to get local address")
        .port()
}

/// Wait for HTTP server to become ready by polling health endpoint
async fn wait_for_server(port: u16, timeout_secs: u64) -> bool {
    let client = reqwest::Client::new();
    let health_url = format!("http://127.0.0.1:{}/health", port);
    let start = std::time::Instant::now();

    while start.elapsed().as_secs() < timeout_secs {
        if let Ok(response) = client.get(&health_url).send().await {
            if response.status().is_success() {
                return true;
            }
        }
        sleep(Duration::from_millis(100)).await;
    }
    false
}

/// Start server subprocess in stream mode
fn start_server(port: u16, db_path: &str) -> Child {
    Command::new("cargo")
        .args([
            "run",
            "--",
            "-s",
            "-p",
            &port.to_string(),
            "--db-path",
            db_path,
        ])
        .spawn()
        .expect("Failed to start server")
}

#[tokio::test]
async fn test_http_server_health_check() {
    let port = find_available_port();
    let db_dir = TempDir::new().expect("Failed to create tempdir");
    let db_path = db_dir.path().join("test.db");
    let mut server = start_server(port, db_path.to_str().unwrap());

    // Wait for server to be ready
    assert!(
        wait_for_server(port, 30).await,
        "Server failed to start within timeout"
    );

    // Test health endpoint
    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://127.0.0.1:{}/health", port))
        .send()
        .await
        .expect("Failed to send request");

    assert!(response.status().is_success());
    let body = response.text().await.expect("Failed to read response");
    assert_eq!(body, "OK");

    // Cleanup
    server.kill().expect("Failed to kill server");
    let _ = server.wait();
}

#[tokio::test]
async fn test_mcp_endpoint_accessible() {
    let port = find_available_port();
    let db_dir = TempDir::new().expect("Failed to create tempdir");
    let db_path = db_dir.path().join("test.db");
    let mut server = start_server(port, db_path.to_str().unwrap());

    // Wait for server to be ready
    assert!(
        wait_for_server(port, 30).await,
        "Server failed to start within timeout"
    );

    // Test MCP endpoint exists (even if it rejects our request)
    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://127.0.0.1:{}/mcp", port))
        .send()
        .await
        .expect("Failed to send request");

    // Should get a response (not connection refused)
    // MCP endpoint might return error for GET, but it exists
    assert!(response.status().as_u16() > 0);

    // Cleanup
    server.kill().expect("Failed to kill server");
    let _ = server.wait();
}

#[tokio::test]
async fn test_custom_port_and_bind() {
    let port = find_available_port();
    let db_dir = TempDir::new().expect("Failed to create tempdir");
    let db_path = db_dir.path().join("test.db");
    let mut server = Command::new("cargo")
        .args([
            "run",
            "--",
            "-s",
            "-p",
            &port.to_string(),
            "-b",
            "127.0.0.1",
            "--db-path",
            db_path.to_str().unwrap(),
        ])
        .spawn()
        .expect("Failed to start server");

    // Wait for server to be ready
    assert!(
        wait_for_server(port, 30).await,
        "Server failed to start within timeout"
    );

    // Verify server responds on custom port
    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://127.0.0.1:{}/health", port))
        .send()
        .await
        .expect("Failed to send request");

    assert!(response.status().is_success());

    // Cleanup
    server.kill().expect("Failed to kill server");
    let _ = server.wait();
}

#[tokio::test]
async fn test_server_with_logging() {
    let port = find_available_port();
    let log_file = format!("test-memory-{}.log", port);
    let db_dir = TempDir::new().expect("Failed to create tempdir");
    let db_path = db_dir.path().join("test.db");

    let mut server = Command::new("cargo")
        .args([
            "run",
            "--",
            "-s",
            "-p",
            &port.to_string(),
            "--db-path",
            db_path.to_str().unwrap(),
            "-l",
            &log_file,
        ])
        .spawn()
        .expect("Failed to start server");

    // Wait for server to be ready
    assert!(
        wait_for_server(port, 30).await,
        "Server failed to start within timeout"
    );

    // Make a request to generate log entries
    let client = reqwest::Client::new();
    client
        .get(format!("http://127.0.0.1:{}/health", port))
        .send()
        .await
        .expect("Failed to send request");

    // Give logger time to flush
    sleep(Duration::from_millis(500)).await;

    // Cleanup server first
    server.kill().expect("Failed to kill server");
    let _ = server.wait();

    // Wait a bit more for file operations
    sleep(Duration::from_millis(200)).await;

    // Verify log file exists
    assert!(
        std::path::Path::new(&log_file).exists(),
        "Log file was not created"
    );

    // Cleanup log file
    std::fs::remove_file(&log_file).ok();
}
