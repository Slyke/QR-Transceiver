use qrtx_runtime::Runtime;
use serde_json::json;
use tempfile::TempDir;

#[test]
fn password_requires_receive_scope_and_never_enters_saved_settings() {
    let temp = TempDir::new().unwrap();
    let runtime = Runtime::open(temp.path()).unwrap();
    let params = json!({"password":"memory-only secret"});
    assert!(
        runtime
            .dispatch("receive_password", params.clone(), Some(&["read".into()]))
            .is_err()
    );
    runtime
        .dispatch("receive_password", params, Some(&["receive".into()]))
        .unwrap();
    assert!(
        runtime
            .dispatch("receive_password", json!({"password":""}), None)
            .is_err()
    );
    assert!(
        runtime
            .dispatch("receive_password", json!({"password":42}), None)
            .is_err()
    );
    runtime
        .dispatch("receive_password", json!({"password":null}), None)
        .unwrap();
    drop(runtime);
    for entry in std::fs::read_dir(temp.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            let bytes = std::fs::read(entry.path()).unwrap();
            assert!(
                !bytes
                    .windows(b"memory-only secret".len())
                    .any(|w| w == b"memory-only secret")
            );
        }
    }
}

#[test]
fn enqueue_password_is_optional_and_type_checked() {
    let temp = TempDir::new().unwrap();
    let runtime = Runtime::open(&temp.path().join("state")).unwrap();
    let source = temp.path().join("a.txt");
    std::fs::write(&source, "text").unwrap();
    let ordinary = runtime
        .dispatch("enqueue", json!({"paths":[source]}), None)
        .unwrap();
    assert_eq!(ordinary["password_bundle"], false);
    for bad in [json!(""), json!(false)] {
        assert!(
            runtime
                .dispatch("enqueue", json!({"paths":[source],"password":bad}), None)
                .is_err()
        );
    }
    let encrypted = runtime
        .dispatch("enqueue", json!({"paths":[source],"password":" "}), None)
        .unwrap();
    assert_eq!(encrypted["password_bundle"], true);
    assert_eq!(
        encrypted["manifest"]["files"][0]["path"],
        "transfer.qrtxenc"
    );
}
