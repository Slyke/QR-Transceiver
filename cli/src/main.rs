use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use qrtx_runtime::{Config, Runtime, ipc};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "Offline optical file transfer")]
struct Cli {
    #[arg(long, global = true, default_value_os_t = qrtx_runtime::default_root())]
    state: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        http: Option<u16>,
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
        mcp_port: Option<u16>,
        #[arg(long)]
        enable_mcp: bool,
        #[arg(long)]
        allow_root: Vec<PathBuf>,
        #[arg(long)]
        camera: bool,
    },
    Send {
        paths: Vec<PathBuf>,
        #[arg(long)]
        recursive: bool,
        /// Encrypt using exact UTF-8 bytes from stdin (including any newline).
        #[arg(long)]
        password_stdin: bool,
    },
    Receive {
        #[arg(long)]
        output: Option<PathBuf>,
        /// Set a receive password from exact UTF-8 stdin before starting the camera.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Set a receive password from stdin, or clear it. Omit id to apply before scanning.
    Password {
        id: Option<String>,
        #[arg(long)]
        clear: bool,
    },
    Both {
        #[arg(long)]
        profile: Option<String>,
    },
    Status,
    Queue,
    History,
    Devices,
    Profiles,
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
    Cancel {
        id: String,
    },
    Inspect {
        id: String,
    },
    Next {
        #[arg(long)]
        svg: Option<PathBuf>,
    },
    Ingest {
        text: String,
    },
    Mcp,
    Rpc {
        method: String,
        #[arg(default_value = "{}")]
        params: String,
    },
    Simulate {
        source: PathBuf,
        output: PathBuf,
        #[arg(long, default_value_t = 7)]
        drop_every: usize,
    },
}
#[tokio::main]
async fn main() {
    qrtx_runtime::init_logging();
    if let Err(error) = run(Cli::parse()).await {
        eprintln!(
            "{}",
            json!({"ok":false,"error":{"code":"QRTX_CLI_FAILED","message":error.to_string()}})
        );
        std::process::exit(1);
    }
}
async fn run(cli: Cli) -> Result<()> {
    let result = match cli.command {
        Command::Serve {
            http,
            mcp_port,
            enable_mcp,
            allow_root,
            camera,
        } => {
            std::fs::create_dir_all(&cli.state)?;
            let mut cfg = Config::load(&cli.state)?;
            if http.is_some() {
                cfg.http_port = http;
            }
            if mcp_port.is_some() {
                cfg.mcp_port = mcp_port;
            }
            for root in allow_root {
                let root = root.canonicalize()?;
                if !cfg.allowed_roots.contains(&root) {
                    cfg.allowed_roots.push(root);
                }
            }
            cfg.validate()?;
            std::fs::write(
                cli.state.join("config.json"),
                serde_json::to_vec_pretty(&cfg)?,
            )?;
            let r = Runtime::open(&cli.state)?;
            let mcp_listener = r
                .configuration()?
                .mcp_listener_port(enable_mcp, mcp_port)
                .map(qrtx_runtime::mcp::bind_http)
                .transpose()?;
            r.start_background();
            if let Some(listener) = mcp_listener {
                let mcp = r.clone();
                tokio::spawn(async move {
                    if let Err(e) = qrtx_runtime::mcp::serve_http(mcp, listener).await {
                        eprintln!("MCP server failed: {e}");
                    }
                });
            }
            if camera {
                r.camera.start(r.clone())?;
            }
            if let Some(port) = r.configuration()?.http_port {
                let api = r.clone();
                tokio::spawn(async move {
                    if let Err(e) = qrtx_runtime::api::serve(api, port).await {
                        eprintln!("HTTP API failed: {e}");
                    }
                });
            }
            tokio::select! {result=ipc::serve(r.clone())=>result?,_=tokio::signal::ctrl_c()=>{r.camera.stop();}}
            return Ok(());
        }
        Command::Mcp => {
            qrtx_runtime::mcp::serve(cli.state, std::env::var("QRTX_MCP_TOKEN").ok()).await?;
            return Ok(());
        }
        Command::Send {
            paths,
            recursive,
            password_stdin,
        } => {
            let password = if password_stdin {
                Some(read_password()?)
            } else {
                None
            };
            ipc::call(&cli.state,"enqueue",json!({"paths":paths.into_iter().map(|p|p.canonicalize()).collect::<std::io::Result<Vec<_>>>()?,"recursive":recursive,"password":password}),None).await?
        }
        Command::Password { id, clear } => {
            let password = if clear { None } else { Some(read_password()?) };
            ipc::call(
                &cli.state,
                "receive_password",
                json!({"id":id,"password":password}),
                None,
            )
            .await?
        }
        Command::Receive {
            output,
            password_stdin,
        } => {
            if password_stdin {
                ipc::call(
                    &cli.state,
                    "receive_password",
                    json!({"password":read_password()?}),
                    None,
                )
                .await?;
            }
            if let Some(output) = output {
                let status = ipc::call(&cli.state, "status", Value::Null, None).await?;
                let mut p = status["profile"].clone();
                p["destination"] = json!(std::path::absolute(output)?);
                p["mode"] = json!("receive");
                ipc::call(&cli.state, "set_profile", p, None).await?;
            }
            ipc::call(&cli.state, "receive_start", Value::Null, None).await?
        }
        Command::Both { profile } => {
            if let Some(name) = profile {
                ipc::call(&cli.state, "select_profile", json!({"name":name}), None).await?;
            }
            let status = ipc::call(&cli.state, "status", Value::Null, None).await?;
            let mut p = status["profile"].clone();
            p["mode"] = json!("both");
            ipc::call(&cli.state, "set_profile", p, None).await?
        }
        Command::Status => ipc::call(&cli.state, "status", Value::Null, None).await?,
        Command::Queue => ipc::call(&cli.state, "transfers", Value::Null, None).await?,
        Command::History => ipc::call(&cli.state, "history", Value::Null, None).await?,
        Command::Devices => ipc::call(&cli.state, "devices", Value::Null, None).await?,
        Command::Profiles => ipc::call(&cli.state, "profiles", Value::Null, None).await?,
        Command::Pause { id } => {
            ipc::call(
                &cli.state,
                "action",
                json!({"id":id,"action":"pause"}),
                None,
            )
            .await?
        }
        Command::Resume { id } => {
            ipc::call(
                &cli.state,
                "action",
                json!({"id":id,"action":"resume"}),
                None,
            )
            .await?
        }
        Command::Cancel { id } => {
            ipc::call(
                &cli.state,
                "action",
                json!({"id":id,"action":"cancel"}),
                None,
            )
            .await?
        }
        Command::Inspect { id } => {
            ipc::call(&cli.state, "transfer", json!({"id":id}), None).await?
        }
        Command::Next { svg } => {
            let f = ipc::call(&cli.state, "next_frame", Value::Null, None).await?;
            if let Some(path) = svg {
                std::fs::write(path, f["svg"].as_str().context("no frame available")?)?;
            }
            f
        }
        Command::Ingest { text } => {
            ipc::call(&cli.state, "ingest", json!({"text":text}), None).await?
        }
        Command::Rpc { method, params } => {
            ipc::call(&cli.state, &method, serde_json::from_str(&params)?, None).await?
        }
        Command::Simulate {
            source,
            output,
            drop_every,
        } => simulate(source, output, drop_every)?,
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
fn read_password() -> Result<String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin().take(4097).read_to_end(&mut bytes)?;
    let password = String::from_utf8(bytes).context("password must be UTF-8")?;
    qrtx_core::password::validate_password(&password)?;
    Ok(password)
}
fn simulate(source: PathBuf, output: PathBuf, drop_every: usize) -> Result<Value> {
    use qrtx_core::{Engine, Profile, protocol::Kind};
    std::fs::create_dir_all(&output)?;
    let mut a = Engine::open(output.join("sender"))?;
    let mut b = Engine::open(output.join("receiver"))?;
    let mut p = Profile::default();
    p.trusted_peers.push(b.fingerprint());
    let job = a.enqueue(vec![source.canonicalize()?], true, Some(p))?;
    let mut frame_count = 0;
    let mut controls = 0;
    for tick in 0..1_000_000 {
        if let Some(f) = a.next_frame()? {
            frame_count += 1;
            if drop_every == 0 || frame_count % drop_every != 0 || f.frame.kind == Kind::Manifest {
                let _ = b.ingest(&f.text);
            }
        }
        if let Some(f) = b.next_frame()? {
            controls += 1;
            let _ = a.ingest(&f.text);
        }
        if tick % 50 == 0
            && let Ok(j) = b.job(job.id)
        {
            for f in &j.manifest.files {
                if !f.directory && !j.delivered_files.contains(&f.id) {
                    b.request_missing(j.id, f.id)?;
                }
            }
        }
        if a.job(job.id)?.status == "confirmed" {
            return Ok(
                json!({"ok":true,"transfer":job.id,"dataAndManifestFrames":frame_count,"controlFrames":controls,"destination":b.profile.destination,"delivery":"confirmed"}),
            );
        }
    }
    anyhow::bail!("simulation exceeded frame budget")
}
