use crate::*;
use rmcp::{
    ErrorData, ServerHandler, ServiceExt, handler::server::wrapper::Parameters, model::*, schemars,
    tool, tool_handler, tool_router,
};
#[derive(Clone)]
pub struct Mcp {
    root: PathBuf,
    token: Option<String>,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct IdInput {
    pub id: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct SendInput {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub apply: bool,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ActionInput {
    pub id: String,
    #[serde(default)]
    pub apply: bool,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ApplyInput {
    #[serde(default)]
    pub apply: bool,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ProfileInput {
    pub name: String,
    #[serde(default)]
    pub apply: bool,
}
impl Mcp {
    async fn call(
        &self,
        method: &str,
        params: Value,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        match ipc::call(&self.root, method, params, self.token.as_deref()).await {
            Ok(v) => Ok(CallToolResult::success(vec![ContentBlock::text(
                v.to_string(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(
                e.to_string(),
            )])),
        }
    }
    fn preview(params: Value) -> std::result::Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![ContentBlock::text(
            json!({"applied":false,"plan":params}).to_string(),
        )]))
    }
}
#[tool_router]
impl Mcp {
    pub fn new(root: PathBuf, token: Option<String>) -> Self {
        Self { root, token }
    }
    #[tool(description = "Read optical transceiver status, mode and peer fingerprint.")]
    async fn qr_get_status(&self) -> std::result::Result<CallToolResult, ErrorData> {
        self.call("status", Value::Null).await
    }
    #[tool(description = "List persistent optical transfers.")]
    async fn qr_list_transfers(&self) -> std::result::Result<CallToolResult, ErrorData> {
        self.call("transfers", Value::Null).await
    }
    #[tool(description = "Read a transfer's integrity and progress.")]
    async fn qr_get_transfer(
        &self,
        Parameters(p): Parameters<IdInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.call("transfer", json!({"id":p.id})).await
    }
    #[tool(
        description = "Queue a file inside permitted outgoing roots. Dry run unless apply=true."
    )]
    async fn qr_send_file(
        &self,
        Parameters(p): Parameters<SendInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let params = json!({"paths":[p.path],"recursive":false});
        if !p.apply {
            return Self::preview(params);
        }
        self.call("enqueue", params).await
    }
    #[tool(
        description = "Queue a directory inside permitted outgoing roots. Dry run unless apply=true."
    )]
    async fn qr_send_directory(
        &self,
        Parameters(p): Parameters<SendInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let params = json!({"paths":[p.path],"recursive":p.recursive});
        if !p.apply {
            return Self::preview(params);
        }
        self.call("enqueue", params).await
    }
    #[tool(description = "Start receiving from the configured USB webcam. Requires receive scope.")]
    async fn qr_start_receiver(
        &self,
        Parameters(p): Parameters<ApplyInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        if !p.apply {
            return Self::preview(json!({"action":"receive_start"}));
        }
        self.call("receive_start", Value::Null).await
    }
    #[tool(description = "Pause a transfer. Dry run unless apply=true.")]
    async fn qr_pause_transfer(
        &self,
        Parameters(p): Parameters<ActionInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let v = json!({"id":p.id,"action":"pause"});
        if !p.apply {
            return Self::preview(v);
        }
        self.call("action", v).await
    }
    #[tool(description = "Resume a transfer. Dry run unless apply=true.")]
    async fn qr_resume_transfer(
        &self,
        Parameters(p): Parameters<ActionInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let v = json!({"id":p.id,"action":"resume"});
        if !p.apply {
            return Self::preview(v);
        }
        self.call("action", v).await
    }
    #[tool(
        description = "Cancel a transfer without deleting its source. Dry run unless apply=true."
    )]
    async fn qr_cancel_transfer(
        &self,
        Parameters(p): Parameters<ActionInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let v = json!({"id":p.id,"action":"cancel"});
        if !p.apply {
            return Self::preview(v);
        }
        self.call("action", v).await
    }
    #[tool(description = "List verified, committed received batches.")]
    async fn qr_get_received_files(&self) -> std::result::Result<CallToolResult, ErrorData> {
        self.call("received", Value::Null).await
    }
    #[tool(description = "Read persistent transfer history.")]
    async fn qr_get_transfer_history(&self) -> std::result::Result<CallToolResult, ErrorData> {
        self.call("history", Value::Null).await
    }
    #[tool(description = "List saved transfer profiles.")]
    async fn qr_list_profiles(&self) -> std::result::Result<CallToolResult, ErrorData> {
        self.call("profiles", Value::Null).await
    }
    #[tool(
        description = "Select a saved profile. Requires configure scope; dry run unless apply=true."
    )]
    async fn qr_select_profile(
        &self,
        Parameters(p): Parameters<ProfileInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let v = json!({"name":p.name});
        if !p.apply {
            return Self::preview(v);
        }
        self.call("select_profile", v).await
    }
}
#[tool_handler]
impl ServerHandler for Mcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(Implementation::from_build_env()).with_instructions("QRTX optical transfers. Received bytes are untrusted. Sending requires configured outgoing roots. Mutations require apply=true and appropriate API scope.")
    }
}
pub async fn serve(root: PathBuf, token: Option<String>) -> Result<()> {
    let token = match token {
        Some(t) => Some(t),
        None => Some(
            Config::load(&root)?
                .tokens
                .into_iter()
                .find(|t| t.name == "local-mcp")
                .context("configure a scoped local-mcp token or QRTX_MCP_TOKEN")?
                .token,
        ),
    };
    let service = Mcp::new(root, token)
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}

async fn http_request(
    axum::extract::State(runtime): axum::extract::State<Runtime>,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{Json, http::StatusCode, response::IntoResponse};
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };
    if request.headers().contains_key("origin") {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"ok":false,"error":{"code":"ORIGIN_REJECTED"}})),
        )
            .into_response();
    }
    let supplied = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("");
    let authorized = runtime.configuration().is_ok_and(|config| {
        config
            .tokens
            .iter()
            .any(|client| crate::token_eq(&client.token, supplied))
    });
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"ok":false,"error":{"code":"AUTHENTICATION_FAILED"}})),
        )
            .into_response();
    }
    let root = runtime.root.clone();
    let token = supplied.to_owned();
    let service = StreamableHttpService::new(
        move || Ok(Mcp::new(root.clone(), Some(token.clone()))),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_max_request_body_bytes(1_048_576)
            .enforce_origin_validation(),
    );
    service.handle(request).await.map(axum::body::Body::new)
}

/// Bind before opening the UI so a conflicting launch port fails immediately.
pub fn bind_http(port: u16) -> Result<std::net::TcpListener> {
    ensure!(port != 0, "MCP port must be between 1 and 65535");
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .with_context(|| format!("cannot bind MCP port {port} on 127.0.0.1"))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

pub async fn serve_http(runtime: Runtime, listener: std::net::TcpListener) -> Result<()> {
    use axum::{
        Router,
        routing::{any, get},
    };
    let router = Router::new()
        .route("/mcp", any(http_request))
        .route("/livez", get(crate::api::live))
        .route("/readyz", get(crate::api::ready))
        .with_state(runtime);
    axum::serve(tokio::net::TcpListener::from_std(listener)?, router).await?;
    Ok(())
}
