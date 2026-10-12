use qrtx_runtime::{Config, Runtime};
use serde_json::json;
use tempfile::TempDir;

#[test]
fn mcp_requires_explicit_enable_and_settings_persist() {
    let config: Config = serde_json::from_value(json!({"mcp_port":47631})).unwrap();
    assert_eq!(config.mcp_listener_port(false, None), None);
    assert_eq!(config.mcp_listener_port(false, Some(47632)), None);
    assert_eq!(config.mcp_listener_port(true, None), Some(47631));
    assert_eq!(config.mcp_listener_port(true, Some(47632)), Some(47632));
    assert_eq!(Config::default().mcp_listener_port(true, None), Some(38476));
    let tmp = TempDir::new().unwrap();
    let runtime = Runtime::open(tmp.path()).unwrap();
    assert_eq!(
        runtime.dispatch("mcp_settings", json!({}), None).unwrap()["enabled"],
        false
    );
    assert!(
        runtime
            .dispatch(
                "mcp_settings_save",
                json!({"enabled":true,"port":47631}),
                Some(&["read".into()])
            )
            .is_err()
    );
    runtime
        .dispatch(
            "mcp_settings_save",
            json!({"enabled":true,"port":47631}),
            None,
        )
        .unwrap();
    assert!(
        runtime
            .dispatch("mcp_settings_save", json!({"enabled":false,"port":0}), None)
            .is_err()
    );
    assert_eq!(
        Config::load(tmp.path())
            .unwrap()
            .mcp_listener_port(false, None),
        Some(47631)
    );
    runtime
        .dispatch(
            "mcp_settings_save",
            json!({"enabled":false,"port":47631}),
            None,
        )
        .unwrap();
    assert_eq!(
        Config::load(tmp.path())
            .unwrap()
            .mcp_listener_port(false, None),
        None
    );
}
