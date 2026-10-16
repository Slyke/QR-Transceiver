pub mod api;
pub mod automation;
pub mod camera;
pub mod config;
pub mod ipc;
pub mod mcp;
use anyhow::{Context, Result, ensure};
pub use config::Config;
use qrtx_core::{Engine, Profile};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct Runtime {
    pub engine: Arc<Mutex<Engine>>,
    pub config: Arc<std::sync::RwLock<Config>>,
    pub root: PathBuf,
    pub camera: camera::CameraState,
    pub displays: Arc<Mutex<Value>>,
}
#[derive(Serialize, Deserialize)]
pub struct Request {
    pub token: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}
#[derive(Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub result: Option<Value>,
    pub error: Option<Value>,
}
impl Runtime {
    pub fn open(root: &Path) -> Result<Self> {
        let engine = Engine::open(root)?;
        let root = root.canonicalize()?;
        let config = Config::load(&root)?;
        tracing::info!(
            loggerKey = "SERVICE_BOOT_DIAGNOSTICS",
            version = env!("CARGO_PKG_VERSION"),
            buildHash = option_env!("BUILD_HASH").unwrap_or("unknown"),
            platform = std::env::consts::OS,
            "QR Transceiver engine started"
        );
        Ok(Self {
            engine: Arc::new(Mutex::new(engine)),
            config: Arc::new(std::sync::RwLock::new(config)),
            root,
            camera: camera::CameraState::default(),
            displays: Arc::new(Mutex::new(json!([]))),
        })
    }
    pub fn authenticated(&self, request: &Request) -> Response {
        let config = match self.configuration() {
            Ok(c) => c,
            Err(e) => return response(Err(e)),
        };
        let scopes = config
            .tokens
            .iter()
            .find(|t| token_eq(&t.token, &request.token))
            .map(|t| t.scopes.as_slice());
        let result = if let Some(scopes) = scopes {
            self.dispatch(&request.method, request.params.clone(), Some(scopes))
        } else {
            Err(anyhow::anyhow!("authentication failed"))
        };
        response(result)
    }
    pub fn dispatch(&self, method: &str, p: Value, scopes: Option<&[String]>) -> Result<Value> {
        let permission = match method {
            "enqueue" => "send",
            "receive_start" | "receive_stop" | "ingest" | "missing" | "receive_password" => {
                "receive"
            }
            "destination" | "camera_control" | "automation_save" | "automation_config"
            | "set_profile" | "save_profile" | "select_profile" | "display_settings"
            | "mcp_settings_save" => "configure",
            "action" | "next_frame" => "send",
            _ => "read",
        };
        if let Some(scopes) = scopes {
            ensure!(
                scopes.iter().any(|s| s == permission),
                "permission denied: {permission}"
            );
        }
        if method == "mcp_settings" || method == "mcp_settings_save" {
            let mut config = self
                .config
                .write()
                .map_err(|_| anyhow::anyhow!("configuration unavailable"))?;
            if method == "mcp_settings_save" {
                let mut updated = config.clone();
                updated.mcp_enabled = p["enabled"].as_bool().context("enabled must be boolean")?;
                updated.mcp_port = Some(p["port"].as_u64().context("port required")?.try_into()?);
                updated.validate()?;
                std::fs::write(
                    self.root.join("config.json"),
                    serde_json::to_vec_pretty(&updated)?,
                )?;
                *config = updated;
            }
            return Ok(
                json!({"enabled":config.mcp_enabled,"port":config.mcp_port.unwrap_or(38476)}),
            );
        }
        if method == "camera_control" {
            self.camera.set_control(
                p["name"]
                    .as_str()
                    .context("camera control name required")?
                    .into(),
                p["value"].clone(),
            )?;
            return Ok(json!({"queued":true}));
        }
        if method == "devices" {
            return Ok(
                json!({"cameras":camera::devices()?,"displays":self.displays.lock().map_err(|_| anyhow::anyhow!("displays unavailable"))?.clone()}),
            );
        }
        if method == "receive_start" {
            self.camera.start(self.clone())?;
            return Ok(json!({"receiving":true}));
        }
        if method == "receive_stop" {
            self.camera.stop();
            return Ok(json!({"receiving":false}));
        }
        if method == "camera" {
            return self.camera.snapshot();
        }
        let mut e = self
            .engine
            .lock()
            .map_err(|_| anyhow::anyhow!("engine unavailable"))?;
        let id = || -> Result<uuid::Uuid> { Ok(p["id"].as_str().context("id required")?.parse()?) };
        let file =
            || -> Result<u32> { Ok(p["file"].as_u64().context("file required")?.try_into()?) };
        let config = self.configuration()?;
        match method {
            "status" => Ok(
                json!({"version":env!("CARGO_PKG_VERSION"),"buildHash":option_env!("BUILD_HASH").unwrap_or("unknown"),"mode":e.profile.mode,"fingerprint":e.fingerprint(),"publicKey":e.public_key(),"profile":e.profile,"camera":self.camera.snapshot()?}),
            ),
            "transfers" | "history" => Ok(json!(e.jobs()?)),
            "transfer" => Ok(json!(e.inspect(id()?)?)),
            "events" if p["history"].as_bool().unwrap_or(false) => Ok(json!(
                e.recent_events(p["limit"].as_u64().unwrap_or(100).min(1000) as u32)?
            )),
            "events" => Ok(json!(e.events(
                p["after"].as_u64().unwrap_or(0),
                p["limit"].as_u64().unwrap_or(100).min(1000) as u32
            )?)),
            "received" => Ok(json!(
                e.jobs()?
                    .into_iter()
                    .filter(|j| j.direction == "receive" && j.status == "complete")
                    .collect::<Vec<_>>()
            )),
            "profiles" => Ok(json!(e.profiles()?)),
            "enqueue" => {
                let paths: Vec<PathBuf> = serde_json::from_value(p["paths"].clone())?;
                if scopes.is_some() {
                    for path in &paths {
                        ensure!(
                            self.configuration()?.permitted(path)?,
                            "path outside configured outgoing roots"
                        );
                    }
                }
                let profile = if p["profile"].is_object() {
                    if let Some(scopes) = scopes {
                        ensure!(
                            scopes.iter().any(|s| s == "configure"),
                            "profile overrides require configure permission"
                        );
                    }
                    Some(serde_json::from_value::<Profile>(p["profile"].clone())?)
                } else {
                    None
                };
                let password = p
                    .get("password")
                    .filter(|v| !v.is_null())
                    .map(|v| v.as_str().context("password must be a string"))
                    .transpose()?;
                Ok(json!(e.enqueue_with_password(
                    paths,
                    p["recursive"].as_bool().unwrap_or(false),
                    profile,
                    password
                )?))
            }
            "receive_password" => {
                let transfer = p
                    .get("id")
                    .filter(|v| !v.is_null())
                    .map(|_| id())
                    .transpose()?;
                let password = p
                    .get("password")
                    .filter(|v| !v.is_null())
                    .map(|v| v.as_str().context("password must be a string"))
                    .transpose()?;
                e.set_receive_password(transfer, password)?;
                Ok(json!({"ok":true}))
            }
            "action" => {
                e.change_job(
                    id()?,
                    p["action"].as_str().context("action required")?,
                    p["priority"].as_i64().map(|n| n as i32),
                )?;
                Ok(json!({"ok":true}))
            }
            "next_frame" => Ok(json!(e.next_frame()?)),
            "redraw_frame" => Ok(json!(e.redraw_frame(
                p["text"].as_str().context("text required")?,
                p["cycle"].as_u64().unwrap_or(0)
            )?)),
            "ingest" => {
                e.ingest(p["text"].as_str().context("text required")?)?;
                Ok(json!({"accepted":true}))
            }
            "payload" => Ok(json!(e.payload(
                id()?,
                file()?,
                p["offset"].as_u64().unwrap_or(0),
                p["length"].as_u64().unwrap_or(1024).min(65536) as usize
            )?)),
            "coverage" => Ok(json!(e.coverage_map(
                id()?,
                file()?,
                p["bins"].as_u64().unwrap_or(256).min(1024) as usize
            )?)),
            "digest" => {
                let snapshot = e.digest_snapshot(id()?, file()?)?;
                drop(e);
                Ok(json!(snapshot.calculate()?))
            }
            "search" => {
                let needle = if p["hex"].as_bool().unwrap_or(false) {
                    hex::decode(
                        p["query"]
                            .as_str()
                            .context("query required")?
                            .replace(' ', ""),
                    )?
                } else {
                    p["query"]
                        .as_str()
                        .context("query required")?
                        .as_bytes()
                        .to_vec()
                };
                let snapshot = e.search_snapshot(id()?, file()?)?;
                drop(e);
                Ok(json!(
                    snapshot.calculate(&needle, p["offset"].as_u64().unwrap_or(0))?
                ))
            }
            "destination" => {
                e.set_destination(
                    id()?,
                    Path::new(p["path"].as_str().context("destination path required")?),
                )?;
                Ok(json!({"ok":true}))
            }
            "missing" => Ok(json!(e.request_missing(id()?, file()?)?)),
            "display_settings" => {
                e.update_display_settings(serde_json::from_value(p)?)?;
                Ok(json!(e.profile))
            }
            "set_profile" => {
                e.set_profile(serde_json::from_value(p)?)?;
                Ok(json!({"ok":true}))
            }
            "save_profile" => {
                e.save_profile(&serde_json::from_value(p)?)?;
                Ok(json!({"ok":true}))
            }
            "select_profile" => {
                let selected = e
                    .profiles()?
                    .into_iter()
                    .find(|x| Some(x.name.as_str()) == p["name"].as_str())
                    .context("unknown profile")?;
                e.set_profile(selected)?;
                Ok(json!({"ok":true}))
            }
            "automation_config" => Ok(
                json!({"webhooks":config.webhooks.iter().map(|w|{let mut v=serde_json::to_value(w).unwrap_or(Value::Null);v["bearer"]=Value::Null;v}).collect::<Vec<_>>(),"commands":config.commands,"watches":config.watches,"allowed_roots":config.allowed_roots}),
            ),
            "automation_save" => {
                let mut next = config.clone();
                next.webhooks = serde_json::from_value(p["webhooks"].clone())?;
                next.commands = serde_json::from_value(p["commands"].clone())?;
                next.watches = serde_json::from_value(p["watches"].clone())?;
                next.allowed_roots = serde_json::from_value(p["allowed_roots"].clone())?;
                if !p["enable_actions"].as_bool().unwrap_or(false) {
                    for w in &mut next.webhooks {
                        w.enabled = false;
                    }
                    for c in &mut next.commands {
                        c.enabled = false;
                    }
                }
                next.validate()?;
                let temporary = self.root.join("config.json.tmp");
                std::fs::write(&temporary, serde_json::to_vec_pretty(&next)?)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
                }
                std::fs::rename(temporary, self.root.join("config.json"))?;
                *self
                    .config
                    .write()
                    .map_err(|_| anyhow::anyhow!("configuration unavailable"))? = next;
                Ok(json!({"ok":true}))
            }
            "automation" => Ok(
                json!({"webhooks":config.webhooks.iter().map(|h|json!({"id":h.id,"enabled":h.enabled,"events":h.events})).collect::<Vec<_>>(),"commands":config.commands.iter().map(|c|json!({"id":c.id,"enabled":c.enabled,"events":c.events})).collect::<Vec<_>>(),"results":e.action_results()?}),
            ),
            _ => anyhow::bail!("unknown method"),
        }
    }
    pub fn configuration(&self) -> Result<Config> {
        Ok(self
            .config
            .read()
            .map_err(|_| anyhow::anyhow!("configuration unavailable"))?
            .clone())
    }
    pub fn start_background(&self) {
        let r = self.clone();
        tokio::spawn(async move {
            automation::run(r).await;
        });
    }
}
pub fn response(result: Result<Value>) -> Response {
    match result {
        Ok(value) => Response {
            ok: true,
            result: Some(value),
            error: None,
        },
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(
                json!({"code":"QRTX_REQUEST_FAILED","message":e.to_string(),"correlationId":uuid::Uuid::new_v4()}),
            ),
        },
    }
}
fn token_eq(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    let a = Sha256::digest(a.as_bytes());
    let b = Sha256::digest(b.as_bytes());
    bool::from(a.ct_eq(&b))
}
pub fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .try_init();
}

pub fn default_root() -> PathBuf {
    if let Some(path) = std::env::var_os("QRTX_STATE") {
        return path.into();
    }
    #[cfg(windows)]
    {
        PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("com.qrtx.transceiver")
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
            })
            .join("com.qrtx.transceiver")
    }
}
