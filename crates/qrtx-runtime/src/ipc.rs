use crate::*;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
pub const MAX_REQUEST: usize = 1_048_576;
async fn line<R: AsyncBufRead + Unpin>(reader: &mut R, limit: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let buf = reader.fill_buf().await?;
        if buf.is_empty() {
            break;
        }
        let n = buf
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(buf.len());
        ensure!(out.len() + n <= limit, "IPC message exceeds limit");
        let done = buf[n - 1] == b'\n';
        out.extend_from_slice(&buf[..n]);
        reader.consume(n);
        if done {
            break;
        }
    }
    Ok(out)
}
async fn connection<S: AsyncRead + AsyncWrite + Unpin>(stream: S, runtime: Runtime) -> Result<()> {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = BufReader::new(read);
    let bytes = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        line(&mut reader, MAX_REQUEST),
    )
    .await??;
    let request: Request = serde_json::from_slice(&bytes)?;
    let response = tokio::task::spawn_blocking(move || runtime.authenticated(&request)).await?;
    let mut bytes = serde_json::to_vec(&response)?;
    bytes.push(b'\n');
    write.write_all(&bytes).await?;
    Ok(())
}
pub async fn serve(runtime: Runtime) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = runtime.root.join("engine.sock");
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        let listener = tokio::net::UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        loop {
            let (stream, _) = listener.accept().await?;
            let r = runtime.clone();
            tokio::spawn(async move {
                if let Err(e) = connection(stream, r).await {
                    tracing::warn!(errorKey="IPC_CONNECTION_FAILED",error=%e,"IPC request failed");
                }
            });
        }
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ServerOptions;
        let name = pipe_name(&runtime.root);
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&name)?;
        loop {
            server.connect().await?;
            let next = ServerOptions::new()
                .reject_remote_clients(true)
                .create(&name)?;
            let r = runtime.clone();
            tokio::spawn(async move {
                if let Err(e) = connection(server, r).await {
                    tracing::warn!(errorKey="PIPE_CONNECTION_FAILED",error=%e,"IPC request failed");
                }
            });
            server = next;
        }
    }
}
#[cfg(windows)]
fn pipe_name(root: &Path) -> String {
    format!(
        r"\\.\pipe\qrtx-{}",
        &hex::encode(Sha256::digest(root.to_string_lossy().as_bytes()))[..24]
    )
}
pub async fn call(root: &Path, method: &str, params: Value, token: Option<&str>) -> Result<Value> {
    let root = root.canonicalize()?;
    let token = if let Some(t) = token {
        t.to_string()
    } else {
        Config::load(&root)?
            .tokens
            .first()
            .context("no local token configured")?
            .token
            .clone()
    };
    let req = Request {
        token,
        method: method.into(),
        params,
    };
    let mut bytes = serde_json::to_vec(&req)?;
    bytes.push(b'\n');
    #[cfg(unix)]
    let stream = tokio::net::UnixStream::connect(root.join("engine.sock"))
        .await
        .context("engine is not running; start qrtransceiver serve or the desktop application")?;
    #[cfg(windows)]
    let stream = tokio::net::windows::named_pipe::ClientOptions::new().open(pipe_name(&root))?;
    let (read, mut write) = tokio::io::split(stream);
    write.write_all(&bytes).await?;
    let bytes = line(&mut BufReader::new(read), 16 * 1024 * 1024).await?;
    let response: Response = serde_json::from_slice(&bytes)?;
    ensure!(
        response.ok,
        "{}",
        response.error.map(|e| e.to_string()).unwrap_or_default()
    );
    Ok(response.result.unwrap_or(Value::Null))
}
