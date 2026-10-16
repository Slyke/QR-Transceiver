use super::*;

impl Engine {
    /// Set/clear a password in memory, either before scanning or for one transfer.
    /// A retry uses the original staged ciphertext and never clears received chunks.
    pub fn set_receive_password(&mut self, id: Option<Uuid>, password: Option<&str>) -> Result<()> {
        if let Some(password) = password {
            password::validate_password(password)?;
        }
        if let Some(id) = id {
            let j = self.job(id)?;
            ensure!(
                j.direction == "receive",
                "password requires a receiving transfer"
            );
            if let Some(password) = password {
                self.transfer_passwords
                    .insert(id, zeroize::Zeroizing::new(password.to_owned()));
            } else {
                self.transfer_passwords.remove(&id);
            }
        } else {
            self.receive_password = password.map(|p| zeroize::Zeroizing::new(p.to_owned()));
            if password.is_none() {
                self.transfer_passwords.clear();
            }
        }
        let mut errors = Vec::new();
        for mut j in self.jobs()?.into_iter().filter(|j| {
            j.direction == "receive"
                && j.password_bundle
                && id.is_none_or(|id| j.id == id)
                && matches!(j.status.as_str(), "active" | "locked")
        }) {
            j.password_needed = self.password_for(j.id).is_none();
            j.error = None;
            self.save(&j)?;
            let id = j.id;
            self.finish_ready_files(j)?;
            if let Some(error) = self.job(id)?.error {
                errors.push(error);
            }
        }
        ensure!(errors.is_empty(), "{}", errors.join("; "));
        Ok(())
    }

    pub(crate) fn password_for(&self, id: Uuid) -> Option<&str> {
        self.transfer_passwords
            .get(&id)
            .or(self.receive_password.as_ref())
            .map(|s| s.as_str())
    }

    /// Inspect only SHA-256-verified files, before any file in the batch commits.
    pub(crate) fn check_password_outer(&self, j: &mut Job) -> Result<()> {
        for entry in j.manifest.files.iter().filter(|e| !e.directory) {
            if entry.size < 8 {
                continue;
            }
            let mut prefix = [0; 8];
            File::open(self.payload_path(j, entry.id))?.read_exact(&mut prefix)?;
            if &prefix == password::MAGIC {
                ensure!(
                    j.manifest.files.len() == 1,
                    "password bundles cannot be mixed with other outer files"
                );
                ensure!(
                    entry.size <= password::MAX_ENVELOPE as u64,
                    "encrypted envelope exceeds 256,000,000 bytes"
                );
                if !j.password_bundle {
                    j.password_bundle = true;
                    j.password_needed = self.password_for(j.id).is_none();
                    self.save(j)?;
                }
            }
        }
        Ok(())
    }

    /// Called only after the normal outer SHA-256 check has passed.
    pub(crate) fn finish_password_bundle(
        &mut self,
        j: &mut Job,
        outer: &FileEntry,
    ) -> Result<bool> {
        if j.password_needed || self.password_for(j.id).is_none() {
            j.status = "locked".into();
            j.password_needed = true;
            self.save(j)?;
            return Ok(false);
        }
        let mut source = File::open(self.payload_path(j, outer.id))?;
        ensure!(
            source.metadata()?.len() <= password::MAX_ENVELOPE as u64,
            "encrypted envelope exceeds 256,000,000 bytes"
        );
        let mut envelope = Vec::new();
        (&mut source)
            .take(password::MAX_ENVELOPE as u64 + 1)
            .read_to_end(&mut envelope)?;
        let bundle = match password::decrypt(
            &envelope,
            self.password_for(j.id).context("password required")?,
        ) {
            Ok(bundle) => bundle,
            Err(error) => {
                j.status = "locked".into();
                j.password_needed = true;
                j.error = Some(error.to_string());
                self.transfer_passwords.remove(&j.id);
                self.save(j)?;
                return Ok(false);
            }
        };
        // The entire tag and every inner path/length are valid before any write.
        j.bundle_files = bundle
            .files()
            // QRTX permits arbitrary u32 outer IDs, not just 1..=MAX_FILES.
            .zip((MAX_FILES as u32 + 1..).filter(|id| *id != outer.id))
            .map(|((name, bytes), id)| FileEntry {
                id,
                path: name.to_owned(),
                size: bytes.len() as u64,
                sha256: hash(bytes),
                chunks: 0,
                directory: false,
            })
            .collect();
        self.save(j)?;
        for (entry, (_, bytes)) in j.bundle_files.iter().zip(bundle.files()) {
            let mut staged = File::create(self.payload_path(j, entry.id))?;
            staged.write_all(bytes)?;
            staged.sync_all()?;
        }
        for entry in &j.bundle_files {
            let target = self.commit_file(j, entry)?;
            self.record_received_file(j, entry, &target)?;
        }
        j.error = None;
        j.password_needed = false;
        j.delivered_files.push(outer.id);
        self.save(j)?;
        self.transfer_passwords.remove(&j.id);
        self.ack_file(j, outer)?;
        Ok(true)
    }

    pub(crate) fn record_received_file(
        &self,
        j: &Job,
        entry: &FileEntry,
        target: &str,
    ) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO outputs VALUES (?1,?2,?3)",
            params![j.id.to_string(), entry.id, target],
        )?;
        self.emit(
            "file.received",
            j,
            Some(entry.id),
            serde_json::json!({"path":target,"size":entry.size}),
            None,
        )?;
        self.emit(
            "file.verified",
            j,
            Some(entry.id),
            serde_json::json!({"path":target,"sha256":entry.sha256,"size":entry.size}),
            None,
        )?;
        self.db.execute(
            "DELETE FROM commits WHERE job=?1 AND file=?2",
            params![j.id.to_string(), entry.id],
        )?;
        Ok(())
    }

    pub(crate) fn ack_file(&self, j: &Job, entry: &FileEntry) -> Result<()> {
        if self.profile.ack_files {
            self.queue_control(
                j,
                Control::AckFile {
                    file: entry.id,
                    sha256: entry.sha256.clone(),
                },
                &format!("file:{}", entry.id),
            )?;
        }
        Ok(())
    }

    pub(crate) fn bundle_outputs_valid(&self, j: &Job) -> Result<bool> {
        if j.bundle_files.is_empty() {
            return Ok(false);
        }
        for entry in &j.bundle_files {
            if !file_hash(&self.payload_path(j, entry.id)).is_ok_and(|h| h == entry.sha256) {
                return Ok(false);
            }
            let path: Option<String> = self
                .db
                .query_row(
                    "SELECT path FROM outputs WHERE job=?1 AND file=?2",
                    params![j.id.to_string(), entry.id],
                    |r| r.get(0),
                )
                .optional()?;
            if !path
                .is_some_and(|p| file_hash(Path::new(&p)).is_ok_and(|hash| hash == entry.sha256))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

pub(crate) fn validate_password_manifest(manifest: &Manifest) -> Result<bool> {
    let encrypted = manifest
        .files
        .iter()
        .any(|f| !f.directory && password::is_password_filename(&f.path));
    if encrypted {
        ensure!(
            manifest.files.len() == 1 && !manifest.files[0].directory,
            "password bundles cannot be mixed with other outer files"
        );
        ensure!(
            manifest.files[0].size <= password::MAX_ENVELOPE as u64,
            "encrypted envelope exceeds 256,000,000 bytes"
        );
    }
    Ok(encrypted)
}
