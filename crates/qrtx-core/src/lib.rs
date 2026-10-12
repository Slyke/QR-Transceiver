//! Shared transfer engine. No GUI, camera, network, shell or JavaScript dependency.
mod extras;
mod inspect;
pub mod model;
mod receive;
mod send;
use anyhow::{Context, Result, ensure};
pub use model::*;
use protocol::*;
pub use qrtx_protocol as protocol;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub struct Engine {
    pub(crate) db: Connection,
    pub(crate) root: PathBuf,
    pub profile: Profile,
    pub(crate) identity: Identity,
    _lock: File,
    pub(crate) ticks: u64,
}
impl Engine {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        let root = root.as_ref().canonicalize()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("engine.lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .context("state directory already belongs to a running engine; use its API")?;
        fs::create_dir_all(root.join("spool"))?;
        fs::create_dir_all(root.join("staging"))?;
        let key_path = root.join("identity.key");
        let identity = if key_path.exists() {
            Identity::from_bytes(&fs::read(&key_path)?.as_slice().try_into()?)
        } else {
            let key = new_identity();
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&key_path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                f.set_permissions(fs::Permissions::from_mode(0o600))?;
            }
            f.write_all(&key.to_bytes())?;
            f.sync_all()?;
            key
        };
        let db = Connection::open(root.join("state.sqlite3"))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS jobs (id TEXT PRIMARY KEY, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS chunks (job TEXT NOT NULL,file INTEGER NOT NULL,idx INTEGER NOT NULL,offset INTEGER NOT NULL,length INTEGER NOT NULL,hash TEXT NOT NULL,PRIMARY KEY(job,file,idx));
            CREATE TABLE IF NOT EXISTS outputs (job TEXT NOT NULL,file INTEGER NOT NULL,path TEXT NOT NULL,PRIMARY KEY(job,file));
            CREATE TABLE IF NOT EXISTS commits (job TEXT NOT NULL,file INTEGER NOT NULL,path TEXT NOT NULL,PRIMARY KEY(job,file));
            CREATE TABLE IF NOT EXISTS fragments (session TEXT,transfer TEXT,idx INTEGER,total INTEGER,hash TEXT,payload BLOB,PRIMARY KEY(session,transfer,idx));
            CREATE TABLE IF NOT EXISTS acked (job TEXT,file INTEGER,idx INTEGER,PRIMARY KEY(job,file,idx));
            CREATE TABLE IF NOT EXISTS resend (job TEXT,file INTEGER,idx INTEGER,PRIMARY KEY(job,file,idx));
            CREATE TABLE IF NOT EXISTS controls (key TEXT PRIMARY KEY,body TEXT NOT NULL,shown INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS sequences (peer TEXT,session TEXT,transfer TEXT,sequence INTEGER,PRIMARY KEY(peer,session,transfer));
            CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS events (seq INTEGER PRIMARY KEY AUTOINCREMENT,key TEXT UNIQUE,body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS actions (event TEXT,rule TEXT,status TEXT,attempts INTEGER DEFAULT 0,next_at INTEGER DEFAULT 0,error TEXT,PRIMARY KEY(event,rule));")?;
        let mut profile: Profile = db
            .query_row("SELECT value FROM settings WHERE key='profile'", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default();
        if profile.destination.is_relative() {
            profile.destination = root.join(&profile.destination);
        }
        let mut engine = Self {
            db,
            root,
            profile,
            identity,
            _lock: lock,
            ticks: 0,
        };
        engine.recover()?;
        Ok(engine)
    }
    pub fn fingerprint(&self) -> String {
        hash(&self.identity.verifying_key().to_bytes())
    }
    pub fn public_key(&self) -> String {
        hex::encode(self.identity.verifying_key().to_bytes())
    }
    pub fn set_profile(&mut self, mut profile: Profile) -> Result<()> {
        profile.validate()?;
        if profile.destination.is_relative() {
            profile.destination = self.root.join(&profile.destination);
        }
        self.db.execute(
            "INSERT OR REPLACE INTO settings VALUES ('profile',?1)",
            [serde_json::to_string(&profile)?],
        )?;
        self.profile = profile;
        Ok(())
    }
    pub fn save_profile(&self, profile: &Profile) -> Result<()> {
        profile.validate()?;
        ensure!(
            !profile.name.is_empty() && profile.name.len() <= 100,
            "invalid profile name"
        );
        self.db.execute(
            "INSERT OR REPLACE INTO settings VALUES (?1,?2)",
            params![
                format!("profile:{}", profile.name),
                serde_json::to_string(profile)?
            ],
        )?;
        Ok(())
    }
    pub fn profiles(&self) -> Result<Vec<Profile>> {
        let mut s = self
            .db
            .prepare("SELECT value FROM settings WHERE key LIKE 'profile:%'")?;
        s.query_map([], |r| r.get::<_, String>(0))?
            .map(|r| Ok(serde_json::from_str(&r?)?))
            .collect()
    }
    pub fn jobs(&self) -> Result<Vec<Job>> {
        let mut s = self.db.prepare("SELECT body FROM jobs")?;
        let mut jobs: Vec<Job> = s
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| Ok(serde_json::from_str(&r?)?))
            .collect::<Result<_>>()?;
        jobs.sort_by_key(|j| (-j.priority, j.created));
        Ok(jobs)
    }
    pub fn job(&self, id: Uuid) -> Result<Job> {
        Ok(serde_json::from_str(&self.db.query_row(
            "SELECT body FROM jobs WHERE id=?1",
            [id.to_string()],
            |r| r.get::<_, String>(0),
        )?)?)
    }
    pub(crate) fn save(&self, j: &Job) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO jobs VALUES (?1,?2)",
            params![j.id.to_string(), serde_json::to_string(j)?],
        )?;
        Ok(())
    }
    pub fn change_job(&mut self, id: Uuid, action: &str, priority: Option<i32>) -> Result<()> {
        let mut j = self.job(id)?;
        match action {
            "pause" => j.status = "paused".into(),
            "resume" => {
                ensure!(
                    j.status != "complete" && j.status != "confirmed",
                    "completed transfer cannot resume"
                );
                j.status = "active".into();
            }
            "cancel" => j.status = "cancelled".into(),
            "restart" => {
                ensure!(j.direction == "send", "only sending jobs can restart");
                let replacement =
                    self.enqueue(j.source_paths.clone(), j.recursive, Some(j.profile.clone()))?;
                j.status = "superseded".into();
                j.error = Some(format!("Restarted as {}", replacement.id));
            }
            "priority" => j.priority = priority.unwrap_or(0).clamp(-10000, 10000),
            "remove" => {
                ensure!(j.status != "active", "pause or cancel before removing");
                let folder = self
                    .root
                    .join(if j.direction == "send" {
                        "spool"
                    } else {
                        "staging"
                    })
                    .join(id.to_string());
                if folder.exists() {
                    fs::remove_dir_all(folder)?;
                }
                let tx = self.db.transaction()?;
                for table in ["chunks", "outputs", "commits", "acked", "resend"] {
                    tx.execute(
                        &format!("DELETE FROM {table} WHERE job=?1"),
                        [id.to_string()],
                    )?;
                }
                tx.execute(
                    "DELETE FROM controls WHERE key LIKE ?1",
                    [format!("{id}:%")],
                )?;
                tx.execute(
                    "DELETE FROM settings WHERE key=?1",
                    [format!("sources:{id}")],
                )?;
                tx.execute("DELETE FROM jobs WHERE id=?1", [id.to_string()])?;
                tx.commit()?;
                return Ok(());
            }
            _ => anyhow::bail!("unknown transfer action"),
        }
        self.save(&j)?;
        if j.direction == "receive" {
            let control = match action {
                "pause" => Some(Control::Pause),
                "resume" => Some(Control::Resume),
                "cancel" => Some(Control::Cancel),
                _ => None,
            };
            if let Some(control) = control {
                self.queue_control(&j, control, "state")?;
            }
        }
        self.emit(
            &format!(
                "transfer.{}",
                match action {
                    "pause" => "paused",
                    "resume" => "resumed",
                    "cancel" => "cancelled",
                    other => other,
                }
            ),
            &j,
            None,
            serde_json::json!({}),
            Some(now().to_string()),
        )?;
        Ok(())
    }
    /// Recent user-facing history excludes repetitive loop bookkeeping before applying the limit.
    pub fn recent_events(&self, limit: u32) -> Result<Vec<(u64, Event)>> {
        let mut statement = self.db.prepare("SELECT seq,body FROM events WHERE json_extract(body,'$.event') != 'transfer.loop_completed' ORDER BY seq DESC LIMIT ?1")?;
        let mut events = statement
            .query_map([limit.min(1000)], |row| {
                Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?))
            })?
            .map(|row| {
                let (sequence, body) = row?;
                Ok((sequence, serde_json::from_str(&body)?))
            })
            .collect::<Result<Vec<_>>>()?;
        events.reverse();
        Ok(events)
    }

    pub fn events(&self, after: u64, limit: u32) -> Result<Vec<(u64, Event)>> {
        let mut s = self
            .db
            .prepare("SELECT seq,body FROM events WHERE seq>?1 ORDER BY seq LIMIT ?2")?;
        s.query_map(params![after, limit.min(1000)], |r| {
            Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?))
        })?
        .map(|r| {
            let (n, v) = r?;
            Ok((n, serde_json::from_str(&v)?))
        })
        .collect()
    }
    pub(crate) fn emit(
        &self,
        event: &str,
        j: &Job,
        file: Option<u32>,
        data: serde_json::Value,
        suffix: Option<String>,
    ) -> Result<()> {
        let key = format!(
            "{}:{}:{event}:{file:?}:{}",
            j.id,
            j.session,
            suffix.unwrap_or_default()
        );
        let e = Event {
            id: hash(key.as_bytes()),
            event: event.into(),
            transfer: j.id,
            file,
            timestamp: now(),
            data,
        };
        self.db.execute(
            "INSERT OR IGNORE INTO events(key,body) VALUES (?1,?2)",
            params![key, serde_json::to_string(&e)?],
        )?;
        Ok(())
    }
    pub(crate) fn payload_path(&self, j: &Job, file: u32) -> PathBuf {
        self.root
            .join(if j.direction == "send" {
                "spool"
            } else {
                "staging"
            })
            .join(j.id.to_string())
            .join(file.to_string())
    }
    pub(crate) fn queue_control(&self, j: &Job, body: Control, key: &str) -> Result<()> {
        if self.profile.mode != Mode::Both || !self.profile.feedback {
            return Ok(());
        }
        let seq: u64 = self
            .db
            .query_row("SELECT value FROM settings WHERE key='sequence'", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .unwrap_or_else(|| "0".into())
            .parse()?;
        let seq = seq.checked_add(1).context("receipt sequence exhausted")?;
        let receipt = Receipt::new(j.session, j.id, seq, body, &self.identity)?;
        let frame = Frame {
            session: j.session,
            transfer: j.id,
            kind: Kind::Control,
            file: 0,
            index: 0,
            total: 1,
            offset: 0,
            payload: pack(&receipt)?,
        };
        frame.encode()?;
        self.db.execute(
            "INSERT OR REPLACE INTO settings VALUES ('sequence',?1)",
            [seq.to_string()],
        )?;
        self.db.execute(
            "INSERT OR REPLACE INTO controls(key,body,shown) VALUES (?1,?2,0)",
            params![format!("{}:{key}", j.id), serde_json::to_string(&frame)?],
        )?;
        self.db.execute("DELETE FROM controls WHERE key IN (SELECT key FROM controls ORDER BY rowid DESC LIMIT -1 OFFSET 8192)",[])?;
        Ok(())
    }
    pub fn claim_action(&self, event: &str, rule: &str) -> Result<bool> {
        Ok(self.db.execute(
            "INSERT OR IGNORE INTO actions(event,rule,status,attempts) VALUES (?1,?2,'running',1)",
            params![event, rule],
        )? > 0)
    }
    pub fn finish_action(
        &self,
        event: &str,
        rule: &str,
        ok: bool,
        error: Option<&str>,
    ) -> Result<()> {
        self.db.execute(
            "UPDATE actions SET status=?3,error=?4 WHERE event=?1 AND rule=?2",
            params![event, rule, if ok { "complete" } else { "failed" }, error],
        )?;
        Ok(())
    }
    pub fn action_results(&self) -> Result<Vec<serde_json::Value>> {
        let mut s = self
            .db
            .prepare("SELECT event,rule,status,error FROM actions ORDER BY rowid DESC LIMIT 100")?;
        Ok(s.query_map([],|r|Ok(serde_json::json!({"event":r.get::<_,String>(0)?,"rule":r.get::<_,String>(1)?,"status":r.get::<_,String>(2)?,"error":r.get::<_,Option<String>>(3)?})))?.collect::<rusqlite::Result<_>>()?)
    }
}
pub(crate) fn file_hash(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    reader_hash(&mut f)
}
pub(crate) fn reader_hash(reader: &mut impl Read) -> Result<String> {
    let mut h = Sha256::new();
    let mut b = [0u8; 65536];
    loop {
        let n = reader.read(&mut b)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(hex::encode(h.finalize()))
}
