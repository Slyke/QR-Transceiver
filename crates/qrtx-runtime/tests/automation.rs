#![cfg(unix)]
use qrtx_core::{Engine, protocol::Kind};
use qrtx_runtime::{
    Config, Runtime,
    config::{CommandRule, Webhook},
};
use serde_json::{Value, json};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::TempDir;

#[tokio::test]
async fn verified_events_run_commands_once_and_webhooks_retry_with_stable_ids() {
    let tmp = TempDir::new().unwrap();
    let state = tmp.path().join("receiver");
    fs::create_dir(&state).unwrap();
    let mut cfg = Config::load(&state).unwrap();
    let output = tmp.path().join("events.jsonl");
    let script = "import json,sys,pathlib; event=json.load(sys.stdin); p=pathlib.Path(sys.argv[1]); p.open('a').write(json.dumps(event)+'\\n')";
    cfg.commands.push(CommandRule {
        id: "record".into(),
        enabled: true,
        events: vec!["file.verified".into()],
        executable: "/usr/bin/python3".into(),
        args: vec!["-c".into(), script.into(), output.to_string_lossy().into()],
        working_directory: tmp.path().into(),
        timeout_ms: 5000,
        environment: Default::default(),
        retries: 0,
        success_codes: vec![0],
    });
    let hits = Arc::new(AtomicUsize::new(0));
    let ids = Arc::new(std::sync::Mutex::new(Vec::new()));
    let h = hits.clone();
    let i = ids.clone();
    let router = axum::Router::new().route(
        "/",
        axum::routing::post(move |headers: axum::http::HeaderMap| {
            let h = h.clone();
            let i = i.clone();
            async move {
                i.lock()
                    .unwrap()
                    .push(headers["Idempotency-Key"].to_str().unwrap().to_string());
                if h.fetch_add(1, Ordering::SeqCst) == 0 {
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR
                } else {
                    axum::http::StatusCode::OK
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    cfg.webhooks.push(Webhook {
        id: "test".into(),
        enabled: true,
        events: vec!["file.verified".into()],
        url: format!("http://127.0.0.1:{port}/"),
        bearer: None,
        timeout_ms: 2000,
        retries: 1,
    });
    fs::write(state.join("config.json"), serde_json::to_vec(&cfg).unwrap()).unwrap();
    let runtime = Runtime::open(&state).unwrap();
    let worker = tokio::spawn(qrtx_runtime::automation::run(runtime.clone()));
    let input = tmp.path().join("literal$(touch owned).bin");
    fs::write(&input, vec![123; 1800]).unwrap();
    let mut sender = Engine::open(tmp.path().join("sender")).unwrap();
    let job = sender.enqueue(vec![input], false, None).unwrap();
    let mut data = Vec::new();
    for _ in 0..10 {
        if let Some(frame) = sender.next_frame().unwrap() {
            if frame.frame.kind == Kind::Manifest {
                runtime.engine.lock().unwrap().ingest(&frame.text).unwrap();
            } else if frame.frame.kind == Kind::Data {
                data.push(frame.text);
            }
        }
        if sender.job(job.id).unwrap().cycle > 0 {
            break;
        }
    }
    runtime.engine.lock().unwrap().ingest(&data[0]).unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert!(!output.exists());
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    for text in &data[1..] {
        runtime.engine.lock().unwrap().ingest(text).unwrap();
    }
    tokio::time::timeout(Duration::from_secs(8), async {
        while !output.exists() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    for text in &data {
        runtime.engine.lock().unwrap().ingest(text).unwrap();
    }
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    {
        let captured = ids.lock().unwrap();
        assert_eq!(captured[0], captured[1]);
    }
    assert!(!tmp.path().join("owned").exists());
    let text = fs::read_to_string(&output).unwrap();
    let event: Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(event["event"], "file.verified");
    worker.abort();
    let _ = worker.await;
    drop(runtime);
    let runtime = Runtime::open(&state).unwrap();
    let worker = tokio::spawn(qrtx_runtime::automation::run(runtime));
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(fs::read_to_string(output).unwrap(), text);
    worker.abort();
    let _ = worker.await;
    server.abort();
}
#[test]
fn auth_scopes_and_outgoing_roots_are_enforced() {
    let tmp = TempDir::new().unwrap();
    let runtime = Runtime::open(tmp.path()).unwrap();
    let config = runtime.configuration().unwrap();
    let token = config
        .tokens
        .iter()
        .find(|t| t.name == "local-mcp")
        .unwrap()
        .token
        .clone();
    let send = qrtx_runtime::Request {
        token: token.clone(),
        method: "enqueue".into(),
        params: json!({"paths":["/etc/passwd"]}),
    };
    assert!(!runtime.authenticated(&send).ok);
    assert!(
        runtime
            .authenticated(&qrtx_runtime::Request {
                token: token.clone(),
                method: "status".into(),
                params: Value::Null
            })
            .ok
    );
    assert!(
        !runtime
            .authenticated(&qrtx_runtime::Request {
                token: "bad".into(),
                method: "status".into(),
                params: Value::Null
            })
            .ok
    );
    let request = qrtx_runtime::Request {
        token: config.tokens[0].token.clone(),
        method: "enqueue".into(),
        params: json!({"paths":["/etc/passwd"]}),
    };
    assert!(!runtime.authenticated(&request).ok);
    let config:Config=serde_json::from_value(json!({"webhooks":[{"id":"x","enabled":true,"events":["file.verified"],"url":"http://example.com","bearer":null,"timeout_ms":1000,"retries":0}]})).unwrap();
    assert!(config.validate().is_err());
}
