//! Android's local bridge. Deliberately depends only on the offline engine.
use anyhow::{Context, Result, bail, ensure};
use qrtx_core::{DisplaySettings, Engine, Mode, Profile, protocol};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

mod jni_bridge;

pub struct MobileEngine {
    engine: Engine,
    imports: PathBuf,
    received: PathBuf,
}

impl MobileEngine {
    pub fn open(root: &Path) -> Result<Self> {
        let mut engine = Engine::open(root)?;
        let received = root.canonicalize()?.join("received");
        let imports = root
            .canonicalize()?
            .parent()
            .context("state parent missing")?
            .join("imports");
        std::fs::create_dir_all(&imports)?;
        if engine.setting("android_initialized")?.is_none() {
            engine.set_profile(Profile {
                name: "Android".into(),
                allow_unpaired_ack: false,
                chunk_size: 400,
                frame_ms: 400,
                max_receive_bytes: 1024 * 1024 * 1024,
                ..engine.profile.clone()
            })?;
            engine.store_setting("android_initialized", "1")?;
        }
        // A process killed during a provider copy may leave an unqueued import batch.
        let sources: Vec<_> = engine
            .jobs()?
            .into_iter()
            .flat_map(|j| j.source_paths)
            .filter_map(|p| p.canonicalize().ok())
            .collect();
        for entry in std::fs::read_dir(&imports)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() && !sources.iter().any(|p| p.starts_with(entry.path())) {
                std::fs::remove_dir_all(entry.path())?;
            }
        }
        // SAF exports are explicit copies out of private storage.
        let mut profile = engine.profile.clone();
        profile.destination = received.clone();
        engine.set_profile(profile)?;
        Ok(Self {
            engine,
            imports,
            received,
        })
    }

    pub fn dispatch(&mut self, method: &str, p: Value) -> Result<Value> {
        let id = || -> Result<uuid::Uuid> {
            Ok(p["id"].as_str().context("transfer id required")?.parse()?)
        };
        let file =
            || -> Result<u32> { Ok(p["file"].as_u64().context("file id required")?.try_into()?) };
        match method {
            "status" => {
                let mut transfers = Vec::new();
                for job in self.engine.jobs()? {
                    let view = self.engine.inspect(job.id)?;
                    let files: Vec<_> = job
                        .manifest
                        .files
                        .iter()
                        .map(|entry| {
                            let progress = view.files.iter().find(|f| f.file == entry.id);
                            json!({
                                "id":entry.id, "name":entry.path, "size":entry.size,
                                "directory":entry.directory, "sha256":entry.sha256,
                                "verified":progress.is_some_and(|p| p.verified),
                                "receivedBytes":progress.map_or(0, |p| p.received_bytes),
                                "receivedChunks":progress.map_or(0, |p| p.received_chunks),
                                "totalChunks":entry.chunks
                            })
                        })
                        .collect();
                    transfers.push(json!({
                        "id":job.id, "direction":job.direction, "status":job.status,
                        "files":files, "cycles":job.cycle, "frames":job.frames,
                        "error":job.error, "bytesPerSecond":view.useful_bytes_per_second,
                        "etaSeconds":view.eta_seconds, "created":job.created
                    }));
                }
                Ok(json!({
                    "fingerprint":self.engine.fingerprint(),
                    "profile":self.engine.profile,
                    "transfers":transfers
                }))
            }
            "configure" => {
                let mut profile = self.engine.profile.clone();
                if let Some(mode) = p.get("mode") {
                    profile.mode = serde_json::from_value(mode.clone())?;
                }
                if let Some(ms) = p.get("frame_ms") {
                    profile.frame_ms = ms.as_u64().context("frame duration required")?;
                }
                if let Some(chunk) = p.get("chunk_size") {
                    profile.chunk_size =
                        chunk.as_u64().context("chunk size required")?.try_into()?;
                }
                if let Some(correction) = p.get("correction") {
                    profile.correction = correction.as_str().context("correction required")?.into();
                }
                if let Some(peers) = p.get("trusted_peers") {
                    profile.trusted_peers = serde_json::from_value(peers.clone())?;
                    for peer in &mut profile.trusted_peers {
                        *peer = peer.to_ascii_lowercase();
                    }
                }
                profile.allow_unpaired_ack = false;
                profile.validate()?;
                self.engine.set_profile(profile.clone())?;
                self.engine.update_display_settings(DisplaySettings {
                    frame_ms: Some(profile.frame_ms),
                    ..Default::default()
                })?;
                Ok(json!(profile))
            }
            "enqueue" => {
                ensure!(
                    self.engine.profile.mode != Mode::Receive,
                    "Switch to Send or Both before adding files"
                );
                let inputs: Vec<PathBuf> = serde_json::from_value(p["paths"].clone())?;
                let paths = inputs
                    .iter()
                    .map(|p| p.canonicalize())
                    .collect::<std::io::Result<Vec<_>>>()?;
                for path in &paths {
                    ensure!(
                        path.canonicalize()?.starts_with(&self.imports),
                        "input must be an imported document"
                    );
                }
                Ok(
                    json!({"id":self.engine.enqueue(paths, p["recursive"].as_bool().unwrap_or(false), None)?.id}),
                )
            }
            "action" => {
                let job = self.engine.job(id()?)?;
                let action = p["action"].as_str().context("action required")?;
                let outputs = if action == "remove" && job.direction == "receive" {
                    self.engine
                        .inspect(job.id)?
                        .files
                        .into_iter()
                        .filter_map(|f| f.path)
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                self.engine.change_job(job.id, action, None)?;
                if action == "remove" && job.direction == "send" {
                    let remaining = self.engine.jobs()?;
                    for source in job.source_paths {
                        if let Some(parent) = source.parent()
                            && parent.parent() == Some(self.imports.as_path())
                            && parent.exists()
                            && !remaining
                                .iter()
                                .flat_map(|j| &j.source_paths)
                                .any(|p| p.starts_with(parent))
                        {
                            std::fs::remove_dir_all(parent)?;
                        }
                    }
                }
                for output in outputs {
                    let path = Path::new(&output);
                    if path.is_file() && path.canonicalize()?.starts_with(&self.received) {
                        std::fs::remove_file(path)?;
                    }
                }
                Ok(json!({}))
            }
            "next_frame" => {
                let Some(frame) = self.engine.next_frame()? else {
                    return Ok(Value::Null);
                };
                let correction = if matches!(
                    frame.frame.kind,
                    protocol::Kind::Data | protocol::Kind::Manifest | protocol::Kind::Bundle
                ) {
                    self.engine.job(frame.frame.transfer)?.profile.correction
                } else {
                    self.engine.profile.correction.clone()
                };
                let code = protocol::qr(&frame.text, &correction, 40)?;
                let dark = code[(0, 0)];
                let modules: Vec<bool> = code.to_colors().iter().map(|v| *v == dark).collect();
                // Finder (0,0) is always dark; use color conversion in the shared renderer.
                Ok(json!({
                    "text":frame.text, "width":code.width(), "modules":modules,
                    "quietZone":4, "durationMs":frame.duration_ms,
                    "kind":format!("{:?}", frame.frame.kind), "index":frame.frame.index,
                    "total":frame.frame.total, "cycle":frame.cycle
                }))
            }
            "ingest" => {
                self.engine
                    .ingest(p["text"].as_str().context("frame text required")?)?;
                Ok(json!({}))
            }
            "maintain" => {
                self.engine.housekeeping()?;
                Ok(json!({}))
            }
            "missing" => Ok(json!(self.engine.request_missing(id()?, file()?)?)),
            "payload" => Ok(json!(self.engine.payload(
                id()?,
                file()?,
                p["offset"].as_u64().unwrap_or(0),
                256
            )?)),
            "export_batch" => {
                let transfer = id()?;
                let view = self.engine.inspect(transfer)?;
                ensure!(
                    view.job.direction == "receive" && view.job.status == "complete",
                    "Wait for the complete transfer before saving a folder"
                );
                let mut files = Vec::new();
                for entry in view.job.manifest.files {
                    let path = if entry.directory {
                        Value::Null
                    } else {
                        self.dispatch("export", json!({"id":transfer,"file":entry.id}))?["path"]
                            .clone()
                    };
                    files.push(json!({"name":entry.path,"directory":entry.directory,"path":path}));
                }
                Ok(json!(files))
            }
            "export" => {
                let view = self.engine.inspect(id()?)?;
                ensure!(
                    view.job.direction == "receive",
                    "Only received files can be exported"
                );
                let requested_file = file()?;
                let entry = view
                    .files
                    .iter()
                    .find(|f| f.file == requested_file)
                    .context("unknown file")?;
                ensure!(entry.verified, "File has not passed SHA-256 verification");
                let path = Path::new(
                    entry
                        .path
                        .as_ref()
                        .context("file has no committed output")?,
                )
                .canonicalize()?;
                ensure!(
                    path.starts_with(&self.received),
                    "Output is outside private receive storage"
                );
                Ok(json!({"path":path}))
            }
            _ => bail!("Unknown Android engine method"),
        }
    }

    pub fn scan(&mut self, texts: Vec<String>) -> Value {
        let mut accepted = 0;
        let mut rejected = 0;
        let mut last_error = None;
        for text in texts {
            match self.engine.ingest(&text) {
                Ok(()) => accepted += 1,
                Err(error) => {
                    rejected += 1;
                    last_error = Some(format!("{error:#}"));
                }
            }
        }
        json!({"accepted":accepted, "rejected":rejected, "lastError":last_error})
    }
}

pub fn scan_luma(bytes: Vec<u8>, width: u32, height: u32) -> Result<Vec<String>> {
    ensure!(
        width > 0
            && height > 0
            && width <= 4096
            && height <= 4096
            && u64::from(width) * u64::from(height) <= 8_847_360,
        "camera frame dimensions exceed bounds"
    );
    let image = protocol::image::GrayImage::from_raw(width, height, bytes)
        .context("invalid camera frame length")?;
    Ok(protocol::scan(image))
}
