use super::*;
use cap_std::{ambient_authority, fs::Dir};
use std::io::{Seek, SeekFrom};

impl Engine {
    pub fn ingest(&mut self, text: &str) -> Result<()> {
        ensure!(
            self.profile.mode != Mode::Send,
            "receiver is disabled in Send mode"
        );
        let frame = Frame::decode(text)?;
        let result = match frame.kind {
            Kind::Manifest => self.accept_manifest(&frame),
            Kind::Data => self.accept_data(&frame),
            Kind::Control => self.accept_control(&frame),
            Kind::Bundle => self.accept_bundle(&frame),
        };
        if result.is_err()
            && frame.kind != Kind::Control
            && let Ok(mut j) = self.job(frame.transfer)
            && j.session == frame.session
            && j.direction == "receive"
        {
            j.rejected += 1;
            self.save(&j)?;
        }
        result
    }
    pub(crate) fn accept_manifest(&mut self, f: &Frame) -> Result<()> {
        ensure!(
            f.payload.len() > 32 && f.total <= MAX_MANIFEST.div_ceil(64) as u32,
            "manifest fragment bounds"
        );
        ensure!(
            f.offset <= MAX_MANIFEST as u64 && f.file == 0,
            "manifest offset invalid"
        );
        let digest = hex::encode(&f.payload[..32]);
        if let Ok(j) = self.job(f.transfer) {
            ensure!(
                j.direction == "receive" && j.session == f.session && j.manifest_hash == digest,
                "transfer identity conflict"
            );
            if j.status == "complete" {
                self.completion_controls(&j)?;
            }
            return Ok(());
        }
        let count: u64 = self.db.query_row(
            "SELECT COUNT(*) FROM (SELECT DISTINCT session,transfer FROM fragments)",
            [],
            |r| r.get(0),
        )?;
        let exists: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM fragments WHERE session=?1 AND transfer=?2)",
            params![f.session.to_string(), f.transfer.to_string()],
            |r| r.get(0),
        )?;
        ensure!(exists || count < 16, "too many pending manifests");
        let existing=self.db.query_row("SELECT total,hash,payload FROM fragments WHERE session=?1 AND transfer=?2 AND idx=?3",params![f.session.to_string(),f.transfer.to_string(),f.index],|r|Ok((r.get::<_,u32>(0)?,r.get::<_,String>(1)?,r.get::<_,Vec<u8>>(2)?))).optional()?;
        if let Some((total, h, p)) = existing {
            ensure!(
                total == f.total && h == digest && p == f.payload[32..],
                "conflicting manifest fragment"
            );
        } else {
            let size:u64=self.db.query_row("SELECT COALESCE(SUM(length(payload)),0) FROM fragments WHERE session=?1 AND transfer=?2",params![f.session.to_string(),f.transfer.to_string()],|r|r.get(0))?;
            ensure!(
                size + f.payload.len() as u64 - 32 <= MAX_MANIFEST as u64,
                "manifest exceeds limit"
            );
            self.db.execute(
                "INSERT INTO fragments VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    f.session.to_string(),
                    f.transfer.to_string(),
                    f.index,
                    f.total,
                    digest,
                    &f.payload[32..]
                ],
            )?;
        }
        let mut s=self.db.prepare("SELECT idx,total,hash,payload FROM fragments WHERE session=?1 AND transfer=?2 ORDER BY idx")?;
        let rows = s
            .query_map(
                params![f.session.to_string(), f.transfer.to_string()],
                |r| {
                    Ok((
                        r.get::<_, u32>(0)?,
                        r.get::<_, u32>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(s);
        if rows.len() != f.total as usize {
            return Ok(());
        }
        let mut bytes = Vec::new();
        for (idx, (i, t, h, p)) in rows.into_iter().enumerate() {
            ensure!(
                i == idx as u32 && t == f.total && h == digest,
                "inconsistent manifest"
            );
            bytes.extend(p);
        }
        ensure!(hash(&bytes) == digest, "manifest integrity failure");
        let manifest: Manifest = unpack(&bytes)?;
        manifest.validate()?;
        ensure!(
            manifest.transfer == f.transfer,
            "manifest transfer mismatch"
        );
        let jobs = self.jobs()?;
        let active: Vec<_> = jobs
            .iter()
            .filter(|j| {
                j.direction == "receive" && j.status != "complete" && j.status != "cancelled"
            })
            .collect();
        ensure!(active.len() < 16, "too many incoming sessions");
        let used: u64 = jobs
            .iter()
            .filter(|j| j.direction == "receive")
            .flat_map(|j| &j.manifest.files)
            .map(|f| f.size)
            .sum();
        ensure!(
            used + manifest.files.iter().map(|f| f.size).sum::<u64>()
                <= self.profile.max_receive_bytes,
            "receive quota exceeded"
        );
        fs::create_dir_all(self.root.join("staging").join(f.transfer.to_string()))?;
        fs::create_dir_all(&self.profile.destination)?;
        let destination = self.profile.destination.canonicalize()?;
        let mut profile = self.profile.clone();
        profile.destination = destination;
        let j = Job {
            id: f.transfer,
            session: f.session,
            direction: "receive".into(),
            status: "active".into(),
            priority: 0,
            created: now(),
            manifest,
            manifest_hash: digest,
            profile,
            source_paths: vec![],
            recursive: false,
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
        for file in &j.manifest.files {
            if file.directory {
                continue;
            }
            let staged = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(self.payload_path(&j, file.id))?;
            staged.set_len(file.size)?;
            staged.sync_all()?;
        }
        self.save(&j)?;
        self.db.execute(
            "DELETE FROM fragments WHERE session=?1 AND transfer=?2",
            params![j.session.to_string(), j.id.to_string()],
        )?;
        self.emit("transfer.started", &j, None, serde_json::json!({}), None)?;
        self.queue_control(&j, Control::Ready, "ready")?;
        self.finish_ready_files(j)
    }
    pub(crate) fn accept_data(&mut self, f: &Frame) -> Result<()> {
        let mut j = self
            .job(f.transfer)
            .context("waiting for complete manifest")?;
        ensure!(
            j.direction == "receive" && j.session == f.session,
            "data targets different session"
        );
        ensure!(
            j.status == "active" || j.status == "complete",
            "transfer is not accepting data"
        );
        let entry = j
            .manifest
            .files
            .iter()
            .find(|e| e.id == f.file && !e.directory)
            .context("unknown file")?;
        ensure!(
            f.total == entry.chunks
                && f.index < entry.chunks
                && f.offset == f.index as u64 * j.manifest.chunk_size as u64,
            "chunk layout mismatch"
        );
        let expected = entry
            .size
            .saturating_sub(f.offset)
            .min(j.manifest.chunk_size as u64) as usize;
        ensure!(f.payload.len() == expected, "chunk length mismatch");
        let old = self
            .db
            .query_row(
                "SELECT hash FROM chunks WHERE job=?1 AND file=?2 AND idx=?3",
                params![j.id.to_string(), f.file, f.index],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        let digest = hash(&f.payload);
        if let Some(old) = old {
            ensure!(old == digest, "conflicting duplicate chunk");
            j.duplicates += 1;
        } else {
            let mut dest = OpenOptions::new()
                .write(true)
                .open(self.payload_path(&j, f.file))?;
            dest.seek(SeekFrom::Start(f.offset))?;
            dest.write_all(&f.payload)?;
            dest.sync_data()?;
            self.db.execute(
                "INSERT INTO chunks VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    j.id.to_string(),
                    f.file,
                    f.index,
                    f.offset,
                    f.payload.len(),
                    digest
                ],
            )?;
            j.bytes += f.payload.len() as u64;
        }
        j.frames += 1;
        j.last_frame = Some(f.clone());
        j.last_activity = now();
        self.save(&j)?;
        if self.profile.ack_frames {
            self.queue_control(
                &j,
                Control::AckFrame {
                    file: f.file,
                    ranges: vec![(f.index, f.index)],
                },
                &format!("frame:{}", f.file),
            )?;
        }
        self.finish_ready_files(j)
    }
    fn finish_ready_files(&mut self, mut j: Job) -> Result<()> {
        for entry in j.manifest.files.clone() {
            if j.delivered_files.contains(&entry.id) {
                continue;
            }
            let count: u32 = self.db.query_row(
                "SELECT COUNT(*) FROM chunks WHERE job=?1 AND file=?2",
                params![j.id.to_string(), entry.id],
                |r| r.get(0),
            )?;
            if !entry.directory && count != entry.chunks {
                continue;
            }
            if !entry.directory {
                let digest = file_hash(&self.payload_path(&j, entry.id))?;
                if digest != entry.sha256 {
                    j.error = Some(format!("SHA-256 mismatch: {}", entry.path));
                    j.rejected += 1;
                    j.bytes = j.bytes.saturating_sub(entry.size);
                    self.db.execute(
                        "DELETE FROM chunks WHERE job=?1 AND file=?2",
                        params![j.id.to_string(), entry.id],
                    )?;
                    self.emit(
                        "file.failed",
                        &j,
                        Some(entry.id),
                        serde_json::json!({"reason":"hash_mismatch"}),
                        Some(now().to_string()),
                    )?;
                    self.save(&j)?;
                    anyhow::bail!("file SHA-256 mismatch; chunks cleared for retransmission");
                }
            }
            let target = self.commit_file(&j, &entry)?;
            // File promotion can precede SQLite; the persisted intent makes that gap recoverable.
            self.db.execute_batch("SAVEPOINT verified_file")?;
            let completion = (|| -> Result<()> {
                self.db.execute(
                    "INSERT OR REPLACE INTO outputs VALUES (?1,?2,?3)",
                    params![j.id.to_string(), entry.id, target],
                )?;
                j.delivered_files.push(entry.id);
                self.save(&j)?;
                self.emit(
                    "file.received",
                    &j,
                    Some(entry.id),
                    serde_json::json!({"path":target,"size":entry.size}),
                    None,
                )?;
                self.emit(
                    "file.verified",
                    &j,
                    Some(entry.id),
                    serde_json::json!({"path":target,"sha256":entry.sha256,"size":entry.size}),
                    None,
                )?;
                if self.profile.ack_files {
                    self.queue_control(
                        &j,
                        Control::AckFile {
                            file: entry.id,
                            sha256: entry.sha256,
                        },
                        &format!("file:{}", entry.id),
                    )?;
                }
                self.db.execute(
                    "DELETE FROM commits WHERE job=?1 AND file=?2",
                    params![j.id.to_string(), entry.id],
                )?;
                Ok(())
            })();
            match completion {
                Ok(()) => self.db.execute_batch("RELEASE verified_file")?,
                Err(error) => {
                    self.db
                        .execute_batch("ROLLBACK TO verified_file; RELEASE verified_file")?;
                    return Err(error);
                }
            }
        }
        if j.delivered_files.len() == j.manifest.files.len() {
            j.status = "complete".into();
            self.save(&j)?;
            self.emit("batch.received", &j, None, serde_json::json!({}), None)?;
            self.emit(
                "batch.verified",
                &j,
                None,
                serde_json::json!({"manifestHash":j.manifest_hash}),
                None,
            )?;
            self.completion_controls(&j)?;
        }
        Ok(())
    }
    fn commit_file(&self, j: &Job, entry: &FileEntry) -> Result<String> {
        let dir = Dir::open_ambient_dir(&j.profile.destination, ambient_authority())?;
        let relative = if j.profile.preserve_paths {
            entry.path.clone()
        } else {
            entry
                .path
                .rsplit('/')
                .next()
                .context("empty filename")?
                .to_string()
        };
        validate_path(&relative)?;
        if entry.directory {
            dir.create_dir_all(&relative)?;
            return Ok(j
                .profile
                .destination
                .join(relative)
                .to_string_lossy()
                .into());
        }
        if let Some(parent) = Path::new(&relative).parent()
            && !parent.as_os_str().is_empty()
        {
            dir.create_dir_all(parent)?;
        }
        let intent: Option<String> = self
            .db
            .query_row(
                "SELECT path FROM commits WHERE job=?1 AND file=?2",
                params![j.id.to_string(), entry.id],
                |r| r.get(0),
            )
            .optional()?;
        let previous: Option<String> = self
            .db
            .query_row(
                "SELECT path FROM outputs WHERE job=?1 AND file=?2",
                params![j.id.to_string(), entry.id],
                |r| r.get(0),
            )
            .optional()?;
        let previous = previous.and_then(|p| {
            Path::new(&p)
                .strip_prefix(&j.profile.destination)
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        });
        for candidate in intent.iter().chain(previous.iter()) {
            validate_path(candidate)?;
            if let Ok(mut existing) = dir.open(candidate)
                && reader_hash(&mut existing)? == entry.sha256
            {
                return Ok(j
                    .profile
                    .destination
                    .join(candidate)
                    .to_string_lossy()
                    .into());
            }
        }
        let temporary = format!(".qrtx-{}-{}", j.id, entry.id);
        // Capability-relative operations cannot resolve outside the selected receive root.
        let mut options = cap_std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        let mut tmp = match dir.open_with(&temporary, &options) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                dir.remove_file(&temporary)?;
                dir.open_with(&temporary, &options)?
            }
            Err(e) => return Err(e.into()),
        };
        std::io::copy(&mut File::open(self.payload_path(j, entry.id))?, &mut tmp)?;
        tmp.sync_all()?;
        drop(tmp);
        let mut target = intent.unwrap_or_else(|| relative.clone());
        let record_intent = |target: &str| -> Result<()> {
            self.db.execute(
                "INSERT OR REPLACE INTO commits VALUES (?1,?2,?3)",
                params![j.id.to_string(), entry.id, target],
            )?;
            Ok(())
        };
        if j.profile.conflict == Conflict::Overwrite {
            record_intent(&target)?;
            dir.rename(&temporary, &dir, &target)?;
        } else {
            let mut attempt = 0u32;
            loop {
                record_intent(&target)?;
                match promote_without_overwrite(&dir, &temporary, &target) {
                    Ok(()) => break,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        let mut existing = dir.open(&target)?;
                        if reader_hash(&mut existing)? == entry.sha256
                            && j.profile.conflict == Conflict::SkipIdentical
                        {
                            dir.remove_file(&temporary)?;
                            break;
                        }
                        attempt += 1;
                        ensure!(attempt <= 10000, "too many filename collisions");
                        target = format!("{relative}.received-{attempt}");
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Ok(j.profile.destination.join(target).to_string_lossy().into())
    }
    fn completion_controls(&self, j: &Job) -> Result<()> {
        self.queue_control(
            j,
            Control::AckBatch {
                manifest_hash: j.manifest_hash.clone(),
            },
            "batch",
        )
    }
    pub fn request_missing(&self, id: Uuid, file: u32) -> Result<Vec<(u32, u32)>> {
        let j = self.job(id)?;
        ensure!(
            j.direction == "receive",
            "missing requests require receiving job"
        );
        let f = j
            .manifest
            .files
            .iter()
            .find(|f| f.id == file)
            .context("unknown file")?;
        let mut s = self
            .db
            .prepare("SELECT idx FROM chunks WHERE job=?1 AND file=?2 ORDER BY idx")?;
        let got = s
            .query_map(params![id.to_string(), file], |r| r.get::<_, u32>(0))?
            .collect::<rusqlite::Result<std::collections::BTreeSet<_>>>()?;
        let mut ranges = Vec::new();
        let mut start = None;
        let mut count = 0;
        for idx in 0..f.chunks {
            if !got.contains(&idx) {
                if start.is_none() {
                    start = Some(idx);
                }
                count += 1;
            } else if let Some(s) = start.take() {
                ranges.push((s, idx - 1));
            }
            if ranges.len() >= 31 || count >= 2048 {
                if let Some(s) = start.take() {
                    ranges.push((s, idx));
                }
                break;
            }
        }
        if let Some(s) = start {
            ranges.push((s, f.chunks - 1));
        }
        if !ranges.is_empty() {
            self.queue_control(
                &j,
                Control::MissingChunks {
                    file,
                    ranges: ranges.clone(),
                },
                &format!("missing:{file}"),
            )?;
        }
        Ok(ranges)
    }
    pub(crate) fn recover(&mut self) -> Result<()> {
        for mut j in self.jobs()? {
            if j.direction != "receive" {
                continue;
            }
            let mut s = self
                .db
                .prepare("SELECT file,idx,offset,length,hash FROM chunks WHERE job=?1")?;
            let rows = s
                .query_map([j.id.to_string()], |r| {
                    Ok((
                        r.get::<_, u32>(0)?,
                        r.get::<_, u32>(1)?,
                        r.get::<_, u64>(2)?,
                        r.get::<_, usize>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(s);
            for (file, idx, offset, len, expected) in rows {
                let valid = (|| -> Result<bool> {
                    let mut source = File::open(self.payload_path(&j, file))?;
                    source.seek(SeekFrom::Start(offset))?;
                    let mut b = vec![0; len];
                    source.read_exact(&mut b)?;
                    Ok(hash(&b) == expected)
                })();
                if !valid.unwrap_or(false) {
                    self.db.execute(
                        "DELETE FROM chunks WHERE job=?1 AND file=?2 AND idx=?3",
                        params![j.id.to_string(), file, idx],
                    )?;
                    j.delivered_files.retain(|f| *f != file);
                    j.status = "active".into();
                    fs::create_dir_all(
                        self.payload_path(&j, file)
                            .parent()
                            .context("missing staging parent")?,
                    )?;
                    let stage = OpenOptions::new()
                        .create(true)
                        .truncate(false)
                        .write(true)
                        .open(self.payload_path(&j, file))?;
                    if let Some(f) = j.manifest.files.iter().find(|f| f.id == file) {
                        stage.set_len(f.size)?;
                    }
                }
            }
            let mut verified = Vec::new();
            for id in &j.delivered_files {
                let entry = j
                    .manifest
                    .files
                    .iter()
                    .find(|f| f.id == *id)
                    .context("invalid persisted file")?;
                let path = self
                    .db
                    .query_row(
                        "SELECT path FROM outputs WHERE job=?1 AND file=?2",
                        params![j.id.to_string(), id],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?;
                let valid = path
                    .as_ref()
                    .map(|p| {
                        if entry.directory {
                            Path::new(p).is_dir()
                        } else {
                            file_hash(Path::new(p))
                                .map(|h| h == entry.sha256)
                                .unwrap_or(false)
                        }
                    })
                    .unwrap_or(false);
                if valid {
                    verified.push(*id);
                } else {
                    j.status = "active".into();
                }
            }
            j.delivered_files = verified;
            j.bytes = self.db.query_row(
                "SELECT COALESCE(SUM(length),0) FROM chunks WHERE job=?1",
                [j.id.to_string()],
                |r| r.get(0),
            )?;
            self.db.execute(
                "DELETE FROM controls WHERE key LIKE ?1",
                [format!("{}:%", j.id)],
            )?;
            self.save(&j)?;
            if j.status != "cancelled" && j.status != "paused" {
                self.finish_ready_files(j)?;
            }
        }
        Ok(())
    }
}

// Android's SELinux policy prohibits hard links for application processes.
// Keep atomic no-clobber promotion, and resolve the destination parent through
// the capability before passing only a basename to renameat2.
#[cfg(any(target_os = "android", all(test, target_os = "linux")))]
fn rename_without_overwrite(dir: &Dir, temporary: &str, target: &str) -> std::io::Result<()> {
    let target = Path::new(target);
    let basename = target.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "missing destination filename",
        )
    })?;
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = dir.open_dir(parent)?;
    rustix::fs::renameat_with(
        dir,
        temporary,
        &parent,
        basename,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(Into::into)
}

#[cfg(target_os = "android")]
fn promote_without_overwrite(dir: &Dir, temporary: &str, target: &str) -> std::io::Result<()> {
    rename_without_overwrite(dir, temporary, target)
}

#[cfg(not(target_os = "android"))]
fn promote_without_overwrite(dir: &Dir, temporary: &str, target: &str) -> std::io::Result<()> {
    dir.hard_link(temporary, dir, target)?;
    dir.remove_file(temporary)
}

#[cfg(all(test, target_os = "linux"))]
mod android_promotion_tests {
    use super::*;

    #[test]
    fn rename_promotes_atomically_without_overwriting() {
        let temp = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        fs::write(temp.path().join("temporary"), b"new bytes").unwrap();
        fs::write(temp.path().join("existing"), b"keep me").unwrap();
        let error = rename_without_overwrite(&dir, "temporary", "existing").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(temp.path().join("existing")).unwrap(), b"keep me");
        dir.create_dir("folder").unwrap();
        rename_without_overwrite(&dir, "temporary", "folder/received").unwrap();
        assert_eq!(
            fs::read(temp.path().join("folder/received")).unwrap(),
            b"new bytes"
        );
        assert!(!temp.path().join("temporary").exists());
    }

    #[test]
    fn rename_cannot_follow_a_parent_symlink_outside_receive_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("receive");
        let outside = temp.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(root.join("temporary"), b"private").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
        let dir = Dir::open_ambient_dir(&root, ambient_authority()).unwrap();
        assert!(rename_without_overwrite(&dir, "temporary", "escape/file").is_err());
        assert!(!outside.join("file").exists());
        assert!(root.join("temporary").exists());
    }
}
