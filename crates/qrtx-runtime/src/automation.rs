use crate::*;
use qrtx_core::Event;
use std::{collections::HashMap, time::Duration};
pub async fn run(runtime: Runtime) {
    let mut cursor = 0;
    let mut watch_state: HashMap<PathBuf, (u64, std::time::SystemTime, u64, bool)> = HashMap::new();
    loop {
        let Ok(config) = runtime.configuration() else {
            break;
        };
        let r = runtime.clone();
        let maintenance = tokio::task::spawn_blocking(move || {
            r.engine
                .lock()
                .map_err(|_| anyhow::anyhow!("engine unavailable"))?
                .housekeeping()
        })
        .await;
        match maintenance {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(errorKey="HOUSEKEEPING_FAILED",error=%error,"Transfer maintenance failed")
            }
            Err(error) => {
                tracing::warn!(errorKey="HOUSEKEEPING_TASK_FAILED",error=%error,"Transfer maintenance task failed")
            }
        }
        let events = runtime
            .engine
            .lock()
            .ok()
            .and_then(|e| e.events(cursor, 100).ok())
            .unwrap_or_default();
        for (seq, event) in events {
            process(&runtime, &config, &event).await;
            cursor = seq;
        }
        for watch in &config.watches {
            let walker = walkdir::WalkDir::new(&watch.path)
                .follow_links(false)
                .max_depth(if watch.recursive { usize::MAX } else { 1 });
            for entry in walker
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                let Ok(meta) = entry.metadata() else { continue };
                let Ok(modified) = meta.modified() else {
                    continue;
                };
                let path = entry.path().to_path_buf();
                let now = qrtx_core::now();
                let item =
                    watch_state
                        .entry(path.clone())
                        .or_insert((meta.len(), modified, now, false));
                if item.0 != meta.len() || item.1 != modified {
                    *item = (meta.len(), modified, now, false);
                }
                if !item.3 && now.saturating_sub(item.2) >= watch.stable_ms.max(1000) {
                    let r = runtime.clone();
                    let p = path.clone();
                    let cfg = config.clone();
                    let stamp = format!("{}:{:?}", meta.len(), modified);
                    let result = tokio::task::spawn_blocking(move || -> Result<()> {
                        ensure!(cfg.permitted(&p)?, "watch path outside outgoing roots");
                        let key = format!(
                            "watch:{}",
                            qrtx_protocol::hash(p.to_string_lossy().as_bytes())
                        );
                        {
                            let e = r
                                .engine
                                .lock()
                                .map_err(|_| anyhow::anyhow!("engine unavailable"))?;
                            ensure!(
                                e.action_executable_allowed(&p)?,
                                "cannot watch application state or any receive destination"
                            );
                            if e.setting(&key)?.as_deref() == Some(&stamp) {
                                return Ok(());
                            }
                        }
                        r.dispatch("enqueue", json!({"paths":[p],"recursive":false}), None)?;
                        r.engine
                            .lock()
                            .map_err(|_| anyhow::anyhow!("engine unavailable"))?
                            .store_setting(&key, &stamp)?;
                        Ok(())
                    })
                    .await;
                    match result {
                        Ok(Ok(())) => item.3 = true,
                        _ => tracing::warn!(
                            errorKey = "WATCH_ENQUEUE_FAILED",
                            "Stable file could not be queued"
                        ),
                    }
                }
            }
        }
        watch_state.retain(|path, _| path.exists());
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
async fn process(runtime: &Runtime, config: &Config, event: &Event) {
    for rule in &config.webhooks {
        if !rule.enabled || !rule.events.contains(&event.event) {
            continue;
        }
        let id = format!("webhook:{}", rule.id);
        if !runtime
            .engine
            .lock()
            .ok()
            .and_then(|e| e.claim_action(&event.id, &id).ok())
            .unwrap_or(false)
        {
            continue;
        }
        let result = async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_millis(rule.timeout_ms))
                .build()?;
            let mut last = String::new();
            for attempt in 0..=rule.retries {
                let mut request = client
                    .post(&rule.url)
                    .header("Idempotency-Key", &event.id)
                    .json(event);
                if let Some(token) = &rule.bearer {
                    request = request.bearer_auth(token);
                }
                match request.send().await.and_then(|r| r.error_for_status()) {
                    Ok(_) => return Ok::<(), anyhow::Error>(()),
                    Err(e) => last = e.without_url().to_string(),
                }
                if attempt < rule.retries {
                    tokio::time::sleep(Duration::from_millis(250 * (1 << attempt))).await;
                }
            }
            anyhow::bail!("{last}")
        }
        .await;
        finish(runtime, event, &id, result);
    }
    for rule in &config.commands {
        if !rule.enabled || !rule.events.contains(&event.event) {
            continue;
        }
        let id = format!("command:{}", rule.id);
        if !runtime
            .engine
            .lock()
            .ok()
            .and_then(|e| e.claim_action(&event.id, &id).ok())
            .unwrap_or(false)
        {
            continue;
        }
        let result = async {
            ensure!(
                runtime
                    .engine
                    .lock()
                    .map_err(|_| anyhow::anyhow!("engine unavailable"))?
                    .action_executable_allowed(&rule.executable)?,
                "received files and staging files cannot be executed"
            );
            let mut last = String::new();
            for attempt in 0..=rule.retries {
                let outcome = tokio::time::timeout(Duration::from_millis(rule.timeout_ms), async {
                    // Fixed trusted argv; the complete event is JSON on stdin. No shell expansion.
                    let mut child = tokio::process::Command::new(&rule.executable)
                        .args(&rule.args)
                        .current_dir(&rule.working_directory)
                        .envs(&rule.environment)
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .kill_on_drop(true)
                        .spawn()?;
                    if let Some(mut stdin) = child.stdin.take() {
                        use tokio::io::AsyncWriteExt;
                        stdin.write_all(&serde_json::to_vec(event)?).await?;
                    }
                    let status = child.wait().await?;
                    ensure!(
                        rule.success_codes.contains(&status.code().unwrap_or(-1)),
                        "command exited with {status}"
                    );
                    Ok::<(), anyhow::Error>(())
                })
                .await;
                match outcome {
                    Ok(Ok(())) => return Ok(()),
                    Ok(Err(e)) => last = e.to_string(),
                    Err(_) => last = "command timed out".into(),
                }
                if attempt < rule.retries {
                    tokio::time::sleep(Duration::from_millis(250 * (1 << attempt))).await;
                }
            }
            anyhow::bail!("{last}")
        }
        .await;
        finish(runtime, event, &id, result);
    }
}
fn finish(runtime: &Runtime, event: &Event, rule: &str, result: Result<()>) {
    if let Ok(e) = runtime.engine.lock() {
        let _ = e.finish_action(
            &event.id,
            rule,
            result.is_ok(),
            result.as_ref().err().map(|e| e.to_string()).as_deref(),
        );
    }
    if let Err(error) = result {
        tracing::error!(errorKey="AUTOMATION_ACTION_FAILED",event_id=%event.id,rule,error=%error,"Automation failed");
    }
}
