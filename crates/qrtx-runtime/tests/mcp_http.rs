use qrtx_runtime::{Runtime, ipc, mcp};
use serde_json::{Value, json};

async fn call(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    name: &str,
    arguments: Value,
) -> Value {
    client.post(url)
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-03-26")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap()
}

#[tokio::test]
async fn http_mcp_keeps_instances_and_request_permissions_separate() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let a = Runtime::open(first.path()).unwrap();
    let b = Runtime::open(second.path()).unwrap();
    let mut servers = Vec::new();
    let mut urls = Vec::new();
    for runtime in [a.clone(), b.clone()] {
        let ipc_runtime = runtime.clone();
        servers.push(tokio::spawn(async move {
            ipc::serve(ipc_runtime).await.unwrap()
        }));
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        urls.push(format!(
            "http://127.0.0.1:{}/mcp",
            listener.local_addr().unwrap().port()
        ));
        servers.push(tokio::spawn(async move {
            mcp::serve_http(runtime, listener).await.unwrap()
        }));
    }
    for _ in 0..100 {
        if ipc::call(first.path(), "status", Value::Null, None)
            .await
            .is_ok()
            && ipc::call(second.path(), "status", Value::Null, None)
                .await
                .is_ok()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let client = reqwest::Client::new();
    let token_a = a
        .configuration()
        .unwrap()
        .tokens
        .into_iter()
        .find(|t| t.name == "local-mcp")
        .unwrap()
        .token;
    let token_b = b
        .configuration()
        .unwrap()
        .tokens
        .into_iter()
        .find(|t| t.name == "local-mcp")
        .unwrap()
        .token;
    let unauth = client.post(&urls[0]).json(&json!({})).send().await.unwrap();
    assert_eq!(unauth.status(), reqwest::StatusCode::UNAUTHORIZED);
    let cross_instance = client
        .post(&urls[1])
        .bearer_auth(&token_a)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(cross_instance.status(), reqwest::StatusCode::UNAUTHORIZED);
    let origin = client
        .post(&urls[0])
        .bearer_auth(&token_a)
        .header("origin", "http://localhost:1234")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(origin.status(), reqwest::StatusCode::FORBIDDEN);
    let initialized: Value = client.post(&urls[0]).bearer_auth(&token_a)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"qrtx-test","version":"1"}}}))
        .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
    assert!(initialized["result"]["capabilities"]["tools"].is_object());
    let status_a = call(&client, &urls[0], &token_a, "qr_get_status", json!({})).await;
    let status_b = call(&client, &urls[1], &token_b, "qr_get_status", json!({})).await;
    let payload_a: Value =
        serde_json::from_str(status_a["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let payload_b: Value =
        serde_json::from_str(status_b["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_ne!(payload_a["fingerprint"], payload_b["fingerprint"]);
    let denied = call(
        &client,
        &urls[0],
        &token_a,
        "qr_start_receiver",
        json!({"apply":true}),
    )
    .await;
    assert_eq!(denied["result"]["isError"], true);
    assert!(
        denied["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("permission denied")
    );
    let preview = call(&client, &urls[0], &token_a, "qr_start_receiver", json!({})).await;
    assert_eq!(preview["result"]["isError"], false);
    assert_eq!(a.camera.snapshot().unwrap(), Value::Null);
    for task in servers {
        task.abort();
        let _ = task.await;
    }
}

#[test]
fn mcp_ports_must_be_explicit_and_available() {
    assert!(mcp::bind_http(0).is_err());
    let occupied = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    assert!(mcp::bind_http(occupied.local_addr().unwrap().port()).is_err());
}
