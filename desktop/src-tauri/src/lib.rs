use clap::Parser;
use qrtx_runtime::Runtime;
use serde_json::Value;
use tauri::{Manager, State};
#[tauri::command]
async fn rpc(runtime: State<'_, Runtime>, method: String, params: Value) -> Result<Value, String> {
    let r = runtime.inner().clone();
    tokio::task::spawn_blocking(move || r.dispatch(&method, params, None))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn detach(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("qr-display") {
        window.set_focus().map_err(|e| e.to_string())?;
    } else {
        tauri::WebviewWindowBuilder::new(
            &app,
            "qr-display",
            tauri::WebviewUrl::App("index.html?display=1".into()),
        )
        .title("QR Transceiver — Optical display")
        .inner_size(900.0, 900.0)
        .data_directory(app.state::<Runtime>().root.join("webview"))
        .build()
        .map_err(|e| e.to_string())?;
    }
    let profile = app
        .state::<Runtime>()
        .engine
        .lock()
        .map_err(|e| e.to_string())?
        .profile
        .clone();
    if let Some(name) = profile.display_name
        && let Some(monitor) = app
            .available_monitors()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|m| m.name() == Some(&name))
        && let Some(window) = app.get_webview_window("qr-display")
    {
        window
            .set_position(*monitor.position())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(clap::Parser, Default)]
#[command(version, about = "QR Transceiver desktop")]
struct LaunchOptions {
    /// Independent state, identity, browser cache and window settings.
    #[arg(long)]
    state: Option<std::path::PathBuf>,
    /// Enable the local MCP listener for this launch.
    #[arg(long)]
    enable_mcp: bool,
    /// MCP port; specifying a port alone does not enable MCP.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
    mcp_port: Option<u16>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    qrtx_runtime::init_logging();
    let launch = if cfg!(mobile) {
        LaunchOptions::default()
    } else {
        LaunchOptions::parse()
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let root = launch.state.clone().or_else(|| std::env::var_os("QRTX_STATE").map(std::path::PathBuf::from))
                .unwrap_or(app.path().app_data_dir()?);
            let runtime = Runtime::open(&root)?;
            let mcp_listener = runtime.configuration()?.mcp_listener_port(launch.enable_mcp, launch.mcp_port)
                .map(qrtx_runtime::mcp::bind_http).transpose()?;
            app.handle().plugin(
                tauri_plugin_window_state::Builder::default()
                    .with_filename(runtime.root.join("window-state.json").to_string_lossy())
                    .build(),
            )?;
            let profile_name = runtime
                .engine
                .lock()
                .map_err(|e| std::io::Error::other(e.to_string()))?
                .profile
                .name
                .clone();
            tauri::WebviewWindowBuilder::from_config(app, &app.config().app.windows[0])?
                .title(format!("QR Transceiver — {profile_name}"))
                .data_directory(runtime.root.join("webview"))
                .build()?;
            *runtime.displays.lock().map_err(|e| std::io::Error::other(e.to_string()))? = serde_json::json!(app.available_monitors()?.into_iter().map(|m|serde_json::json!({"name":m.name(),"width":m.size().width,"height":m.size().height,"x":m.position().x,"y":m.position().y,"scale":m.scale_factor()})).collect::<Vec<_>>());
            let background = runtime.clone();
            tauri::async_runtime::spawn(async move {
                background.start_background();
                if let Some(listener) = mcp_listener {
                    let r = background.clone();
                    tokio::spawn(async move {
                        if let Err(e) = qrtx_runtime::mcp::serve_http(r, listener).await {
                            eprintln!("MCP server failed: {e}");
                        }
                    });
                }
                if let Some(port) = background.configuration().ok().and_then(|c| c.http_port) {
                    let r = background.clone();
                    tokio::spawn(async move {
                        if let Err(e) = qrtx_runtime::api::serve(r, port).await {
                            eprintln!("HTTP API failed: {e}");
                        }
                    });
                }
                if let Err(e) = qrtx_runtime::ipc::serve(background).await {
                    eprintln!("Local IPC failed: {e}");
                }
            });
            app.manage(runtime);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![rpc, detach])
        .run(tauri::generate_context!())
        .expect("desktop runtime failed");
}
