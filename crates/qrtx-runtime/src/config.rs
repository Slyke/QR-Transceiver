use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub struct ClientToken {
    pub name: String,
    pub token: String,
    pub scopes: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Webhook {
    pub id: String,
    pub enabled: bool,
    pub events: Vec<String>,
    pub url: String,
    pub bearer: Option<String>,
    pub timeout_ms: u64,
    pub retries: u32,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct CommandRule {
    pub id: String,
    pub enabled: bool,
    pub events: Vec<String>,
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub working_directory: PathBuf,
    pub timeout_ms: u64,
    #[serde(default)]
    pub retries: u32,
    #[serde(default = "success_codes")]
    pub success_codes: Vec<i32>,
    pub environment: std::collections::BTreeMap<String, String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Watch {
    pub path: PathBuf,
    pub recursive: bool,
    pub stable_ms: u64,
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub http_port: Option<u16>,
    pub mcp_port: Option<u16>,
    pub mcp_enabled: bool,
    pub allowed_roots: Vec<PathBuf>,
    pub tokens: Vec<ClientToken>,
    pub webhooks: Vec<Webhook>,
    pub commands: Vec<CommandRule>,
    pub watches: Vec<Watch>,
}
impl Config {
    pub fn mcp_listener_port(&self, enable_flag: bool, port: Option<u16>) -> Option<u16> {
        (self.mcp_enabled || enable_flag).then_some(port.or(self.mcp_port).unwrap_or(38476))
    }
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("config.json");
        if !path.exists() {
            let config = Self {
                tokens: vec![
                    ClientToken {
                        name: "local-cli".into(),
                        token: format!(
                            "{}{}",
                            uuid::Uuid::new_v4().simple(),
                            uuid::Uuid::new_v4().simple()
                        ),
                        scopes: vec![
                            "read".into(),
                            "send".into(),
                            "receive".into(),
                            "configure".into(),
                        ],
                    },
                    ClientToken {
                        name: "local-mcp".into(),
                        token: format!(
                            "{}{}",
                            uuid::Uuid::new_v4().simple(),
                            uuid::Uuid::new_v4().simple()
                        ),
                        scopes: vec!["read".into()],
                    },
                ],
                ..Self::default()
            };
            std::fs::write(&path, serde_json::to_vec_pretty(&config)?)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            }
            return Ok(config);
        }
        let mut value: serde_json::Value = json5::from_str(&std::fs::read_to_string(path)?)?;
        expand_environment(&mut value);
        let config: Self = serde_json::from_value(value)?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.mcp_port != Some(0),
            "MCP port must be between 1 and 65535"
        );
        let mut identities = std::collections::HashSet::new();
        for token in &self.tokens {
            ensure!(
                identities.insert(token.name.clone()),
                "duplicate token name"
            );
            ensure!(
                token.token.len() >= 32 && token.token.len() <= 1024,
                "API tokens must have 32–1024 characters"
            );
        }
        for hook in &self.webhooks {
            let u = reqwest::Url::parse(&hook.url)?;
            ensure!(
                u.scheme() == "http"
                    && matches!(u.host_str(), Some("127.0.0.1" | "[::1]"))
                    && u.username().is_empty()
                    && u.password().is_none(),
                "webhooks require an explicit loopback IP and HTTP"
            );
            ensure!(
                (100..=60_000).contains(&hook.timeout_ms) && hook.retries <= 5,
                "invalid webhook retry/timeout limits"
            );
        }
        let mut ids = std::collections::HashSet::new();
        for rule in &self.webhooks {
            ensure!(
                ids.insert(format!("webhook:{}", rule.id)),
                "duplicate webhook id"
            );
        }
        for command in &self.commands {
            ensure!(
                ids.insert(format!("command:{}", command.id)),
                "duplicate command id"
            );
            ensure!(
                command.executable.is_absolute() && command.working_directory.is_absolute(),
                "commands require absolute executable and working directory"
            );
            ensure!(
                (100..=3_600_000).contains(&command.timeout_ms)
                    && command.args.len() <= 128
                    && command.retries <= 5,
                "invalid command limits"
            );
            ensure!(
                ![
                    "file.received",
                    "transfer.loop_completed",
                    "delivery.acknowledged"
                ]
                .iter()
                .any(|e| command.events.iter().any(|x| x == e)),
                "completion commands require verified or authenticated completion events"
            );
        }
        Ok(())
    }
    pub fn permitted(&self, path: &Path) -> Result<bool> {
        let canonical = path.canonicalize()?;
        Ok(self
            .allowed_roots
            .iter()
            .filter_map(|r| r.canonicalize().ok())
            .any(|r| canonical.starts_with(r)))
    }
}
fn expand_environment(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::String(s) => {
            if let Some(name) = s.strip_prefix("${").and_then(|s| s.strip_suffix('}'))
                && !name.is_empty()
                && name.chars().enumerate().all(|(i, c)| {
                    c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
                })
            {
                *v = std::env::var(name)
                    .ok()
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null);
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(expand_environment),
        serde_json::Value::Object(o) => o.values_mut().for_each(expand_environment),
        _ => {}
    }
}

fn success_codes() -> Vec<i32> {
    vec![0]
}
