use super::*;
use std::io::{Seek, SeekFrom};

impl Engine {
    pub fn set_destination(&mut self, id: Uuid, path: &Path) -> Result<()> {
        let mut job = self.job(id)?;
        ensure!(
            job.direction == "receive" && job.delivered_files.is_empty(),
            "destination can only change before the first file commits"
        );
        fs::create_dir_all(path)?;
        job.profile.destination = path.canonicalize()?;
        self.save(&job)
    }
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }
    pub fn store_setting(&self, key: &str, value: &str) -> Result<()> {
        ensure!(
            key.len() <= 2048 && value.len() <= 1_048_576,
            "setting exceeds limit"
        );
        self.db.execute(
            "INSERT OR REPLACE INTO settings VALUES (?1,?2)",
            params![key, value],
        )?;
        Ok(())
    }
    pub(crate) fn static_frame(&self, j: &Job) -> Result<Option<Frame>> {
        if j.manifest.files.len() != 1 {
            return Ok(None);
        }
        let file = &j.manifest.files[0];
        if file.directory || file.size > 256 || file.chunks != 1 {
            return Ok(None);
        }
        let acked: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM acked WHERE job=?1 AND file=?2 AND idx=0)",
            params![j.id.to_string(), file.id],
            |r| r.get(0),
        )?;
        if acked {
            return Ok(None);
        }
        let metadata = pack(&j.manifest)?;
        let data = fs::read(self.payload_path(j, file.id))?;
        let payload = pack(&(hash(&metadata), metadata, data))?;
        if payload.len() > j.profile.chunk_size as usize {
            return Ok(None);
        }
        Ok(Some(Frame {
            session: j.session,
            transfer: j.id,
            kind: Kind::Bundle,
            file: file.id,
            index: 0,
            total: 1,
            offset: 0,
            payload,
        }))
    }
    pub(crate) fn accept_bundle(&mut self, frame: &Frame) -> Result<()> {
        ensure!(
            frame.index == 0 && frame.total == 1 && frame.offset == 0,
            "invalid bundle header"
        );
        let (digest, metadata, data): (String, Vec<u8>, Vec<u8>) = unpack(&frame.payload)?;
        ensure!(hash(&metadata) == digest, "bundle manifest hash mismatch");
        let m: Manifest = unpack(&metadata)?;
        m.validate()?;
        ensure!(
            m.files.len() == 1
                && m.files[0].id == frame.file
                && m.files[0].chunks == 1
                && m.files[0].size == data.len() as u64,
            "invalid static bundle"
        );
        let part_size = 512;
        for (index, part) in metadata.chunks(part_size).enumerate() {
            let mut payload = hex::decode(&digest)?;
            payload.extend(part);
            self.accept_manifest(&Frame {
                session: frame.session,
                transfer: frame.transfer,
                kind: Kind::Manifest,
                file: 0,
                index: index as u32,
                total: metadata.len().div_ceil(part_size) as u32,
                offset: (index * part_size) as u64,
                payload,
            })?;
        }
        self.accept_data(&Frame {
            payload: data,
            kind: Kind::Data,
            ..frame.clone()
        })
    }
    pub fn housekeeping(&mut self) -> Result<()> {
        let jobs = self.jobs()?;
        for j in jobs {
            if j.direction == "receive"
                && j.status == "active"
                && now().saturating_sub(j.last_activity) > 2000
            {
                for file in j
                    .manifest
                    .files
                    .iter()
                    .filter(|f| !f.directory && !j.delivered_files.contains(&f.id))
                    .take(4)
                {
                    self.request_missing(j.id, file.id)?;
                }
            }
            if j.direction == "send" && j.status == "active" && !j.password_bundle {
                let stamp = self.source_stamp(&j)?;
                let previous = self.setting(&format!("sources:{}", j.id))?;
                if previous.as_deref() != Some(&stamp) {
                    let replacement =
                        self.enqueue(j.source_paths.clone(), j.recursive, Some(j.profile.clone()))?;
                    let mut old = j.clone();
                    old.status = "superseded".into();
                    old.error = Some(format!("Source changed; new version {}", replacement.id));
                    self.save(&old)?;
                }
            }
        }
        Ok(())
    }
    pub(crate) fn source_stamp(&self, j: &Job) -> Result<String> {
        let mut h = Sha256::new();
        for source in &j.source_paths {
            let depth = if source.is_dir() && j.recursive {
                usize::MAX
            } else if source.is_dir() {
                1
            } else {
                0
            };
            for entry in walkdir::WalkDir::new(source)
                .follow_links(false)
                .max_depth(depth)
                .sort_by_file_name()
            {
                let entry = entry?;
                if entry.file_type().is_symlink() {
                    continue;
                }
                let m = entry.metadata()?;
                h.update(entry.path().to_string_lossy().as_bytes());
                h.update(m.len().to_be_bytes());
                h.update(format!("{:?}", m.modified()?).as_bytes());
            }
        }
        Ok(hex::encode(h.finalize()))
    }
    pub fn digest_snapshot(&self, id: Uuid, file: u32) -> Result<DigestSnapshot> {
        let j = self.job(id)?;
        let mut s = self.db.prepare(
            "SELECT idx,offset,length FROM chunks WHERE job=?1 AND file=?2 ORDER BY idx",
        )?;
        let chunks = s
            .query_map(params![id.to_string(), file], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(DigestSnapshot {
            path: self.payload_path(&j, file),
            chunks,
        })
    }
    pub fn action_executable_allowed(&self, exe: &Path) -> Result<bool> {
        let exe = exe.canonicalize()?;
        if exe.starts_with(&self.root) {
            return Ok(false);
        }
        for j in self.jobs()?.iter().filter(|j| j.direction == "receive") {
            if exe.starts_with(&j.profile.destination) {
                return Ok(false);
            }
        }
        Ok(!exe.starts_with(std::path::absolute(&self.profile.destination)?))
    }
}
pub struct DigestSnapshot {
    path: PathBuf,
    chunks: Vec<(u32, u64, u32)>,
}
impl DigestSnapshot {
    pub fn calculate(self) -> Result<String> {
        let mut source = File::open(self.path)?;
        let mut h = Sha256::new();
        h.update(b"QRTX/1 received-set\0");
        for (idx, offset, length) in self.chunks {
            h.update(idx.to_be_bytes());
            h.update(offset.to_be_bytes());
            h.update(length.to_be_bytes());
            let mut b = vec![0; length as usize];
            source.seek(SeekFrom::Start(offset))?;
            source.read_exact(&mut b)?;
            h.update(b);
        }
        Ok(hex::encode(h.finalize()))
    }
}
