use super::*;
use std::io::{Seek, SeekFrom};
use walkdir::WalkDir;

impl Engine {
    pub fn enqueue(
        &mut self,
        paths: Vec<PathBuf>,
        recursive: bool,
        profile: Option<Profile>,
    ) -> Result<Job> {
        self.enqueue_with_password(paths, recursive, profile, None)
    }

    pub fn enqueue_with_password(
        &mut self,
        paths: Vec<PathBuf>,
        recursive: bool,
        profile: Option<Profile>,
        password: Option<&str>,
    ) -> Result<Job> {
        if let Some(password) = password {
            password::validate_password(password)?;
        }
        ensure!(
            !paths.is_empty() && paths.len() <= MAX_FILES,
            "select 1–4096 inputs"
        );
        let profile = profile.unwrap_or_else(|| self.profile.clone());
        profile.validate()?;
        let id = Uuid::new_v4();
        let spool = self.root.join("spool").join(id.to_string());
        fs::create_dir(&spool)?;
        // Drop all unfinished snapshots, including plaintext, on any enqueue failure.
        let result = (|| -> Result<Job> {
            let mut files = Vec::new();
            for source in &paths {
                ensure!(
                    !fs::symlink_metadata(source)?.file_type().is_symlink(),
                    "source symlinks are disabled"
                );
                let source = source.canonicalize()?;
                ensure!(
                    !source.is_dir() || !self.root.starts_with(&source),
                    "outgoing directory contains application state; choose a separate outgoing directory"
                );
                let base = source.parent().context("cannot send filesystem root")?;
                if source.is_dir() {
                    let depth = if recursive { usize::MAX } else { 1 };
                    for entry in WalkDir::new(&source)
                        .follow_links(false)
                        .max_depth(depth)
                        .sort_by_file_name()
                    {
                        let entry = entry?;
                        if entry.file_type().is_symlink() {
                            continue;
                        }
                        if entry.file_type().is_file()
                            || (password.is_none() && entry.file_type().is_dir())
                        {
                            if password.is_some() {
                                check_bundle_size(entry.path(), base, &files)?;
                            }
                            snapshot(entry.path(), base, &spool, &mut files, profile.chunk_size)?;
                        }
                    }
                } else {
                    if password.is_some() {
                        check_bundle_size(&source, base, &files)?;
                    }
                    snapshot(&source, base, &spool, &mut files, profile.chunk_size)?;
                }
            }
            if let Some(password) = password {
                let mut bundle = password::BundleBuilder::new();
                for entry in &files {
                    bundle.push(
                        &entry.path,
                        entry.size,
                        File::open(spool.join(entry.id.to_string()))?,
                    )?;
                }
                let envelope = bundle.encrypt(password)?;
                for entry in &files {
                    fs::remove_file(spool.join(entry.id.to_string()))?;
                }
                let mut output = File::create(spool.join("1"))?;
                output.write_all(&envelope)?;
                output.sync_all()?;
                files = vec![FileEntry {
                    id: 1,
                    path: password::FILENAME.into(),
                    size: envelope.len() as u64,
                    sha256: hash(&envelope),
                    chunks: (envelope.len() as u64).div_ceil(profile.chunk_size as u64) as u32,
                    directory: false,
                }];
            }
            let manifest = Manifest {
                version: 1,
                transfer: id,
                chunk_size: profile.chunk_size,
                files,
            };
            manifest.validate()?;
            let mut password_bundle = password_receive::validate_password_manifest(&manifest)?;
            for entry in manifest
                .files
                .iter()
                .filter(|e| !e.directory && e.size >= 8)
            {
                let mut prefix = [0; 8];
                File::open(spool.join(entry.id.to_string()))?.read_exact(&mut prefix)?;
                if &prefix == password::MAGIC {
                    ensure!(
                        manifest.files.len() == 1,
                        "password bundles cannot be mixed with other outer files"
                    );
                    ensure!(
                        entry.size <= password::MAX_ENVELOPE as u64,
                        "encrypted envelope exceeds 256,000,000 bytes"
                    );
                    password_bundle = true;
                }
            }
            let bytes = pack(&manifest)?;
            ensure!(bytes.len() <= MAX_MANIFEST, "manifest exceeds limit");
            let manifest_hash = hash(&bytes);
            let mut f = File::create(spool.join("manifest"))?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            let j = Job {
                id,
                session: Uuid::new_v4(),
                direction: "send".into(),
                status: "active".into(),
                priority: 0,
                created: now(),
                manifest,
                manifest_hash,
                profile,
                source_paths: paths,
                recursive,
                password_bundle,
                password_needed: false,
                bundle_files: vec![],
                cycle: 0,
                frames: 0,
                bytes: 0,
                cursor_file: 0,
                cursor_chunk: 0,
                manifest_cursor: 0,
                since_manifest: 0,
                delivered_files: vec![],
                confirmed_files: vec![],
                duplicates: 0,
                rejected: 0,
                last_frame: None,
                last_activity: now(),
                error: None,
            };
            self.store_setting(&format!("sources:{}", j.id), &self.source_stamp(&j)?)?;
            self.emit("transfer.queued", &j, None, serde_json::json!({}), None)?;
            self.save(&j)?;
            Ok(j)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&spool);
        }
        result
    }

    /// Presentation settings may change without modifying a transfer's wire layout or identity.
    pub fn update_display_settings(&mut self, settings: DisplaySettings) -> Result<()> {
        let mut profile = self.profile.clone();
        settings.apply(&mut profile)?;
        let mut jobs = self.jobs()?;
        jobs.retain(|job| {
            job.direction == "send" && matches!(job.status.as_str(), "active" | "paused")
        });
        for job in &mut jobs {
            settings.apply(&mut job.profile)?;
        }
        let transaction = self.db.transaction()?;
        transaction.execute(
            "INSERT OR REPLACE INTO settings VALUES ('profile',?1)",
            [serde_json::to_string(&profile)?],
        )?;
        for job in jobs {
            transaction.execute(
                "UPDATE jobs SET body=?2 WHERE id=?1",
                rusqlite::params![job.id.to_string(), serde_json::to_string(&job)?],
            )?;
        }
        transaction.commit()?;
        self.profile = profile;
        Ok(())
    }

    pub fn redraw_frame(&self, text: &str, cycle: u64) -> Result<DisplayFrame> {
        let frame = Frame::decode(text)?;
        let mut profile = self.job(frame.transfer)?.profile;
        DisplaySettings {
            frame_ms: Some(self.profile.frame_ms),
            qr_pixels: Some(self.profile.qr_pixels),
            quiet_zone: Some(self.profile.quiet_zone),
            inverted: Some(self.profile.inverted),
        }
        .apply(&mut profile)?;
        display(frame, cycle, &profile)
    }

    pub fn next_frame(&mut self) -> Result<Option<DisplayFrame>> {
        // A loop boundary or completed once-only job is bookkeeping, not a display slot.
        // One extra attempt starts the next loop; bounding by the job count also skips completed jobs.
        let attempts = self.jobs()?.len() + 1;
        let result: Result<Option<DisplayFrame>> = (|| {
            for _ in 0..attempts {
                if let Some(frame) = self.next_frame_inner()? {
                    return Ok(Some(frame));
                }
            }
            Ok(None)
        })();
        match result {
            Ok(frame) => Ok(frame),
            Err(error) => {
                if let Some(mut job) = self
                    .jobs()?
                    .into_iter()
                    .find(|j| j.direction == "send" && j.status == "active")
                {
                    job.status = "failed".into();
                    job.error = Some(error.to_string());
                    self.save(&job)?;
                    self.emit(
                        "transfer.failed",
                        &job,
                        None,
                        serde_json::json!({"reason":error.to_string()}),
                        None,
                    )?;
                    if job.profile.mode == Mode::Both && job.profile.feedback {
                        self.emit(
                            "delivery.failed",
                            &job,
                            None,
                            serde_json::json!({"reason":"local_transmission_failed"}),
                            None,
                        )?;
                    }
                }
                Err(error)
            }
        }
    }
    fn next_frame_inner(&mut self) -> Result<Option<DisplayFrame>> {
        self.ticks += 1;
        let send = self.profile.mode != Mode::Receive;
        let mut job = if send {
            self.jobs()?
                .into_iter()
                .find(|j| j.direction == "send" && j.status == "active")
        } else {
            None
        };
        if self.profile.mode == Mode::Both
            && self.profile.feedback
            && (job.is_none()
                || self
                    .ticks
                    .is_multiple_of(self.profile.control_interval as u64))
        {
            let row = self
                .db
                .query_row(
                    "SELECT key,body FROM controls ORDER BY shown,rowid LIMIT 1",
                    [],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()?;
            if let Some((key, body)) = row {
                let f: Frame = serde_json::from_str(&body)?;
                self.db.execute(
                    "UPDATE controls SET shown=?2 WHERE key=?1",
                    params![key, now()],
                )?;
                return Ok(Some(display(f, 0, &self.profile)?));
            }
        }
        let Some(ref mut j) = job else {
            return Ok(None);
        };
        if j.frames == 0 {
            self.emit("transfer.started", j, None, serde_json::json!({}), None)?;
        }
        if let Some(frame) = self.static_frame(j)? {
            if !j.delivered_files.is_empty() {
                return Ok(None);
            }
            j.frames += 1;
            j.bytes += j.manifest.files[0].size;
            j.cycle += 1;
            j.last_frame = Some(frame.clone());
            j.last_activity = now();
            self.emit(
                "transfer.loop_completed",
                j,
                None,
                serde_json::json!({"cycle":j.cycle,"deliveryConfirmed":false}),
                Some(j.cycle.to_string()),
            )?;
            if j.profile.policy == Policy::Once {
                j.status = "displayed".into();
            }
            self.save(j)?;
            return Ok(Some(display(frame, j.cycle, &j.profile)?));
        }
        let mf = fs::read(
            self.root
                .join("spool")
                .join(j.id.to_string())
                .join("manifest"),
        )?;
        let part_size = j.profile.chunk_size as usize;
        let parts = mf.len().div_ceil(part_size) as u32;
        let frame = if j.manifest_cursor < parts {
            let idx = j.manifest_cursor;
            let start = idx as usize * part_size;
            let end = (start + part_size).min(mf.len());
            let mut payload = hex::decode(&j.manifest_hash)?;
            payload.extend(&mf[start..end]);
            j.manifest_cursor += 1;
            j.since_manifest = 0;
            Some(Frame {
                session: j.session,
                transfer: j.id,
                kind: Kind::Manifest,
                file: 0,
                index: idx,
                total: parts,
                offset: start as u64,
                payload,
            })
        } else {
            let request = self
                .db
                .query_row(
                    "SELECT file,idx FROM resend WHERE job=?1 ORDER BY rowid LIMIT 1",
                    [j.id.to_string()],
                    |r| Ok((r.get::<_, u32>(0)?, r.get::<_, u32>(1)?)),
                )
                .optional()?;
            if let Some((file, idx)) = request {
                self.db.execute(
                    "DELETE FROM resend WHERE job=?1 AND file=?2 AND idx=?3",
                    params![j.id.to_string(), file, idx],
                )?;
                Some(self.data_frame(j, file, idx)?)
            } else {
                self.next_data(j)?
            }
        };
        if let Some(ref frame) = frame {
            j.frames += 1;
            j.bytes += if frame.kind == Kind::Data {
                frame.payload.len() as u64
            } else {
                0
            };
            if frame.kind == Kind::Data {
                j.since_manifest += 1;
            }
            if j.since_manifest >= j.profile.manifest_interval {
                j.manifest_cursor = 0;
            }
            j.last_activity = now();
            j.last_frame = Some(frame.clone());
        }
        self.save(j)?;
        frame.map(|f| display(f, j.cycle, &j.profile)).transpose()
    }
    fn next_data(&self, j: &mut Job) -> Result<Option<Frame>> {
        while j.cursor_file < j.manifest.files.len() {
            let f = &j.manifest.files[j.cursor_file];
            if f.directory || j.delivered_files.contains(&f.id) {
                j.cursor_file += 1;
                j.cursor_chunk = 0;
                continue;
            }
            let mut statement = self.db.prepare(
                "SELECT idx FROM acked WHERE job=?1 AND file=?2 AND idx>=?3 ORDER BY idx",
            )?;
            let mut rows = statement.query(params![j.id.to_string(), f.id, j.cursor_chunk])?;
            while let Some(row) = rows.next()? {
                let acknowledged: u32 = row.get(0)?;
                if acknowledged != j.cursor_chunk {
                    break;
                }
                j.cursor_chunk += 1;
            }
            if j.cursor_chunk < f.chunks {
                let idx = j.cursor_chunk;
                j.cursor_chunk += 1;
                return Ok(Some(self.data_frame(j, f.id, idx)?));
            }
            j.cursor_file += 1;
            j.cursor_chunk = 0;
        }
        j.cycle += 1;
        self.emit(
            "transfer.loop_completed",
            j,
            None,
            serde_json::json!({"cycle":j.cycle,"deliveryConfirmed":false}),
            Some(j.cycle.to_string()),
        )?;
        if j.profile.policy == Policy::Once {
            j.status = "displayed".into();
        } else {
            j.cursor_file = 0;
            j.cursor_chunk = 0;
            j.manifest_cursor = 0;
        }
        Ok(None)
    }
    fn data_frame(&self, j: &Job, file: u32, index: u32) -> Result<Frame> {
        let entry = j
            .manifest
            .files
            .iter()
            .find(|f| f.id == file && !f.directory)
            .context("unknown file")?;
        ensure!(index < entry.chunks, "invalid chunk index");
        let offset = index as u64 * j.manifest.chunk_size as u64;
        let length = (entry.size.saturating_sub(offset)).min(j.manifest.chunk_size as u64) as usize;
        let mut bytes = vec![0; length];
        let mut source = File::open(self.payload_path(j, file))?;
        source.seek(SeekFrom::Start(offset))?;
        source.read_exact(&mut bytes)?;
        Ok(Frame {
            session: j.session,
            transfer: j.id,
            kind: Kind::Data,
            file,
            index,
            total: entry.chunks,
            offset,
            payload: bytes,
        })
    }
    pub(crate) fn accept_control(&mut self, frame: &Frame) -> Result<()> {
        ensure!(
            self.profile.mode == Mode::Both && self.profile.feedback,
            "optical feedback is disabled"
        );
        let mut j = self.job(frame.transfer)?;
        ensure!(
            j.direction == "send" && j.session == frame.session,
            "receipt targets a different session"
        );
        let receipt: Receipt = unpack(&frame.payload)?;
        let peer = receipt.verify(frame.session, frame.transfer)?;
        ensure!(
            receipt.sequence <= i64::MAX as u64,
            "receipt sequence exceeds storage limit"
        );
        let authenticated = j.profile.trusted_peers.contains(&peer);
        ensure!(
            authenticated || j.profile.allow_unpaired_ack,
            "unpaired receipt rejected"
        );
        let previous: u64 = self
            .db
            .query_row(
                "SELECT sequence FROM sequences WHERE peer=?1 AND session=?2 AND transfer=?3",
                params![peer, j.session.to_string(), j.id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        ensure!(receipt.sequence > previous, "stale receipt");
        match &receipt.body {
            Control::AckFrame { file, ranges } | Control::MissingChunks { file, ranges } => {
                let entry = j
                    .manifest
                    .files
                    .iter()
                    .find(|f| f.id == *file && !f.directory)
                    .context("unknown receipt file")?;
                ensure!(ranges.len() <= 32, "too many ranges");
                let mut count = 0u32;
                for &(start, end) in ranges {
                    ensure!(start <= end && end < entry.chunks, "invalid chunk range");
                    count = count
                        .checked_add(end - start + 1)
                        .context("range overflow")?;
                }
                ensure!(count <= 4096, "request exceeds chunk limit");
                for &(start, end) in ranges {
                    for idx in start..=end {
                        if matches!(&receipt.body, Control::AckFrame { .. }) {
                            if j.profile.ack_frames {
                                self.db.execute(
                                    "INSERT OR IGNORE INTO acked VALUES (?1,?2,?3)",
                                    params![j.id.to_string(), file, idx],
                                )?;
                            }
                        } else {
                            self.db.execute(
                                "DELETE FROM acked WHERE job=?1 AND file=?2 AND idx=?3",
                                params![j.id.to_string(), file, idx],
                            )?;
                            self.db.execute(
                                "INSERT OR IGNORE INTO resend VALUES (?1,?2,?3)",
                                params![j.id.to_string(), file, idx],
                            )?;
                        }
                    }
                }
            }
            Control::AckFile { file, sha256 } => {
                let entry = j
                    .manifest
                    .files
                    .iter()
                    .find(|f| f.id == *file)
                    .context("unknown receipt file")?;
                ensure!(&entry.sha256 == sha256, "receipt hash mismatch");
                if j.profile.ack_files {
                    if !j.delivered_files.contains(file) {
                        j.delivered_files.push(*file);
                    }
                    if authenticated && !j.confirmed_files.contains(file) {
                        j.confirmed_files.push(*file);
                    }
                    self.emit(
                        "delivery.acknowledged",
                        &j,
                        Some(*file),
                        serde_json::json!({"sha256":sha256,"authenticated":authenticated}),
                        None,
                    )?;
                    if authenticated {
                        self.emit(
                            "delivery.confirmed",
                            &j,
                            Some(*file),
                            serde_json::json!({"sha256":sha256,"peer":peer}),
                            None,
                        )?;
                    }
                }
            }
            Control::AckBatch { manifest_hash } | Control::Complete { manifest_hash } => {
                ensure!(
                    manifest_hash == &j.manifest_hash,
                    "receipt manifest hash mismatch"
                );
                j.delivered_files = j.manifest.files.iter().map(|f| f.id).collect();
                self.emit(
                    "delivery.acknowledged",
                    &j,
                    None,
                    serde_json::json!({"manifestHash":manifest_hash,"authenticated":authenticated}),
                    None,
                )?;
                if authenticated {
                    j.confirmed_files = j.delivered_files.clone();
                    self.emit(
                        "delivery.confirmed",
                        &j,
                        None,
                        serde_json::json!({"manifestHash":manifest_hash,"peer":peer}),
                        None,
                    )?;
                }
            }
            Control::Pause => {
                ensure!(authenticated, "pause requires paired peer");
                j.status = "paused".into();
            }
            Control::Resume => {
                ensure!(authenticated, "resume requires paired peer");
                if j.status == "paused" {
                    j.status = "active".into();
                }
            }
            Control::Cancel => {
                ensure!(authenticated, "cancel requires paired peer");
                j.status = "cancelled".into();
            }
            Control::Error { message } => {
                ensure!(message.len() <= 256, "error message exceeds limit");
                j.error = Some(message.clone());
            }
            Control::Hello | Control::Ready => {}
        }
        self.db.execute(
            "INSERT OR REPLACE INTO sequences VALUES (?1,?2,?3,?4)",
            params![
                peer,
                j.session.to_string(),
                j.id.to_string(),
                receipt.sequence
            ],
        )?;
        if j.profile.stop_on_ack && j.delivered_files.len() == j.manifest.files.len() {
            j.status = if j.confirmed_files.len() == j.manifest.files.len() {
                "confirmed"
            } else {
                "acknowledged"
            }
            .into();
        }
        self.save(&j)
    }
}
fn check_bundle_size(source: &Path, base: &Path, files: &[FileEntry]) -> Result<()> {
    let size = fs::metadata(source)?.len();
    let name = source
        .strip_prefix(base)?
        .to_str()
        .context("filename is not valid Unicode")?;
    let used: u64 = files
        .iter()
        .map(|f| 12 + f.path.len() as u64 + f.size)
        .sum();
    ensure!(
        size.checked_add(used)
            .and_then(|n| n.checked_add(36 + 16 + 12 + 12 + name.len() as u64))
            .is_some_and(|n| n <= password::MAX_ENVELOPE as u64),
        "encrypted envelope exceeds 256,000,000 bytes"
    );
    Ok(())
}
fn snapshot(
    source: &Path,
    base: &Path,
    spool: &Path,
    files: &mut Vec<FileEntry>,
    chunk_size: u32,
) -> Result<()> {
    ensure!(files.len() < MAX_FILES, "too many files");
    let path = source
        .strip_prefix(base)?
        .to_str()
        .context("filename is not valid Unicode")?
        .replace('\\', "/");
    validate_path(&path)?;
    let meta = fs::symlink_metadata(source)?;
    let directory = meta.is_dir();
    ensure!(directory || meta.is_file(), "unsupported file type");
    let id = files.len() as u32 + 1;
    ensure!(
        meta.len().div_ceil(chunk_size as u64) <= MAX_CHUNKS as u64,
        "file requires too many chunks for this profile"
    );
    let sha256 = if directory {
        hash(&[])
    } else {
        ensure!(meta.len() <= MAX_FILE_SIZE, "file exceeds size limit");
        let mut src = File::open(source)?.take(meta.len() + 1);
        let mut dest = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(spool.join(id.to_string()))?;
        let copied = std::io::copy(&mut src, &mut dest)?;
        dest.sync_all()?;
        let after = fs::metadata(source)?;
        ensure!(
            copied == meta.len()
                && after.len() == meta.len()
                && after.modified()? == meta.modified()?,
            "source changed while snapshotting; enqueue its new version"
        );
        let digest = file_hash(&spool.join(id.to_string()))?;
        ensure!(
            file_hash(source)? == digest,
            "source changed while snapshotting; retry after writes finish"
        );
        digest
    };
    let size = if directory { 0 } else { meta.len() };
    files.push(FileEntry {
        id,
        path,
        size,
        sha256,
        chunks: if directory {
            0
        } else {
            size.div_ceil(chunk_size as u64).max(1) as u32
        },
        directory,
    });
    Ok(())
}
fn display(frame: Frame, cycle: u64, profile: &Profile) -> Result<DisplayFrame> {
    let text = frame.encode()?;
    let pixels = if profile.module_pixels == 0 {
        profile.qr_pixels
    } else {
        (protocol::qr(&text, &profile.correction, profile.max_version)?.width() as u32
            + profile.quiet_zone * 2)
            * profile.module_pixels
    };
    let svg = protocol::qr_custom_svg(
        &text,
        &profile.correction,
        profile.max_version,
        &protocol::QrStyle {
            pixels,
            quiet: profile.quiet_zone,
            foreground: &profile.foreground,
            background: &profile.background,
            inverted: profile.inverted,
        },
    )?;
    let checksum = format!("{:08x}", crc_from_text(&text)?);
    Ok(DisplayFrame {
        duration_ms: profile.frame_ms,
        text,
        svg,
        frame,
        cycle,
        checksum,
    })
}
fn crc_from_text(text: &str) -> Result<u32> {
    protocol::wire_checksum(text)
}
