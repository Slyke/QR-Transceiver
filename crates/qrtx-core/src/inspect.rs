use super::*;
use std::io::{Seek, SeekFrom};

impl Engine {
    pub fn inspect(&self, id: Uuid) -> Result<TransferView> {
        let j = self.job(id)?;
        let mut files = Vec::new();
        for f in &j.manifest.files {
            let (chunks, bytes) = if j.direction == "receive" {
                self.db.query_row(
                    "SELECT COUNT(*),COALESCE(SUM(length),0) FROM chunks WHERE job=?1 AND file=?2",
                    params![id.to_string(), f.id],
                    |r| Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?)),
                )?
            } else {
                let c: u64 = self.db.query_row(
                    "SELECT COUNT(*) FROM acked WHERE job=?1 AND file=?2",
                    params![id.to_string(), f.id],
                    |r| r.get(0),
                )?;
                (c, 0)
            };
            let path = self
                .db
                .query_row(
                    "SELECT path FROM outputs WHERE job=?1 AND file=?2",
                    params![id.to_string(), f.id],
                    |r| r.get(0),
                )
                .optional()?;
            files.push(FileProgress {
                file: f.id,
                received_chunks: chunks,
                total_chunks: f.chunks,
                received_bytes: bytes,
                verified: j.delivered_files.contains(&f.id),
                path,
                received_set_digest: None,
            });
        }
        let data: u64 = j.manifest.files.iter().map(|f| f.chunks as u64).sum();
        let mf = pack(&j.manifest)?
            .len()
            .div_ceil(j.manifest.chunk_size as usize) as u64;
        let frames = data + mf * (1 + data / j.profile.manifest_interval as u64);
        let overhead = if j.profile.mode == Mode::Both && j.profile.feedback {
            j.profile.control_interval as f64 / (j.profile.control_interval - 1) as f64
        } else {
            1.0
        };
        let elapsed = now().saturating_sub(j.created) as f64 / 1000.0;
        let useful = if j.direction == "receive" && elapsed > 0.0 {
            j.bytes as f64 / elapsed
        } else {
            0.0
        };
        let remaining = j
            .manifest
            .files
            .iter()
            .map(|f| f.size)
            .sum::<u64>()
            .saturating_sub(j.bytes);
        let eta = if useful > 0.0 && j.frames >= 8 && now().saturating_sub(j.last_activity) < 5000 {
            Some(remaining as f64 / useful)
        } else {
            None
        };
        Ok(TransferView {
            job: j.clone(),
            files,
            estimated_loop_seconds: frames as f64 * j.profile.frame_ms as f64 / 1000.0 * overhead,
            useful_bytes_per_second: useful,
            eta_seconds: eta,
        })
    }
    pub fn payload(&self, id: Uuid, file: u32, offset: u64, length: usize) -> Result<PayloadPage> {
        ensure!(length <= 65536, "payload page too large");
        let j = self.job(id)?;
        let f = j
            .manifest
            .files
            .iter()
            .find(|f| f.id == file && !f.directory)
            .context("unknown payload file")?;
        ensure!(offset <= f.size, "offset exceeds file");
        let count = length.min(f.size.saturating_sub(offset) as usize);
        let mut b = vec![0; count];
        let mut source = File::open(self.payload_path(&j, file))?;
        source.seek(SeekFrom::Start(offset))?;
        source.read_exact(&mut b)?;
        let ranges = self.coverage(id, file, offset, offset + count as u64)?;
        let bytes = b
            .into_iter()
            .enumerate()
            .map(|(i, b)| {
                let at = offset + i as u64;
                if j.direction == "send" || ranges.iter().any(|(s, e)| at >= *s && at < *e) {
                    Some(b)
                } else {
                    None
                }
            })
            .collect();
        Ok(PayloadPage {
            offset,
            size: f.size,
            bytes,
            verified: j.delivered_files.contains(&file),
            ranges,
        })
    }
    pub fn coverage(&self, id: Uuid, file: u32, start: u64, end: u64) -> Result<Vec<(u64, u64)>> {
        let j = self.job(id)?;
        if j.direction == "send" {
            return Ok(vec![(start, end)]);
        }
        let mut s=self.db.prepare("SELECT offset,length FROM chunks WHERE job=?1 AND file=?2 AND offset<?4 AND offset+length>=?3 ORDER BY idx")?;
        let rows = s.query_map(params![id.to_string(), file, start, end], |r| {
            Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?))
        })?;
        let mut out: Vec<(u64, u64)> = Vec::new();
        for row in rows {
            let (a, len) = row?;
            let b = a + len;
            if let Some(last) = out.last_mut()
                && last.1 == a
            {
                last.1 = b;
                continue;
            }
            if out.len() >= 4096 {
                break;
            }
            out.push((a, b));
        }
        Ok(out)
    }
    pub fn coverage_map(&self, id: Uuid, file: u32, bins: usize) -> Result<Vec<f64>> {
        ensure!(bins > 0 && bins <= 1024, "invalid coverage bins");
        let j = self.job(id)?;
        let f = j
            .manifest
            .files
            .iter()
            .find(|f| f.id == file)
            .context("unknown file")?;
        if j.direction == "send" {
            return Ok(vec![1.0; bins]);
        }
        let width = f.size.max(1).div_ceil(bins as u64);
        let mut map = vec![0.0; bins];
        let mut s = self
            .db
            .prepare("SELECT offset,length FROM chunks WHERE job=?1 AND file=?2")?;
        for row in s.query_map(params![id.to_string(), file], |r| {
            Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?))
        })? {
            let (start, len) = row?;
            if len == 0 {
                continue;
            }
            let end = start + len;
            for i in start / width..=(end - 1) / width {
                if (i as usize) < bins {
                    let a = start.max(i * width);
                    let b = end.min((i + 1) * width);
                    let denom = (f.size - i * width).min(width).max(1);
                    map[i as usize] += (b - a) as f64 / denom as f64;
                }
            }
        }
        Ok(map)
    }
    pub fn received_digest(&self, id: Uuid, file: u32) -> Result<String> {
        let j = self.job(id)?;
        let mut source = File::open(self.payload_path(&j, file))?;
        let mut h = Sha256::new();
        h.update(b"QRTX/1 received-set\0");
        let mut s = self.db.prepare(
            "SELECT idx,offset,length FROM chunks WHERE job=?1 AND file=?2 ORDER BY idx",
        )?;
        for row in s.query_map(params![id.to_string(), file], |r| {
            Ok((
                r.get::<_, u32>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, u32>(2)?,
            ))
        })? {
            let (idx, off, len) = row?;
            h.update(idx.to_be_bytes());
            h.update(off.to_be_bytes());
            h.update(len.to_be_bytes());
            let mut b = vec![0; len as usize];
            source.seek(SeekFrom::Start(off))?;
            source.read_exact(&mut b)?;
            h.update(b);
        }
        Ok(hex::encode(h.finalize()))
    }
    pub fn search_snapshot(&self, id: Uuid, file: u32) -> Result<SearchSnapshot> {
        let job = self.job(id)?;
        let entry = job
            .manifest
            .files
            .iter()
            .find(|f| f.id == file && !f.directory)
            .context("unknown payload file")?;
        let mut ranges: Vec<(u64, u64)> = Vec::new();
        if job.direction == "send" {
            ranges.push((0, entry.size));
        } else {
            let mut statement = self.db.prepare(
                "SELECT offset,length FROM chunks WHERE job=?1 AND file=?2 ORDER BY idx",
            )?;
            for row in statement.query_map(params![id.to_string(), file], |r| {
                Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?))
            })? {
                let (start, length) = row?;
                if let Some(last) = ranges.last_mut()
                    && last.1 == start
                {
                    last.1 = start + length;
                } else {
                    ranges.push((start, start + length));
                }
            }
        }
        Ok(SearchSnapshot {
            path: self.payload_path(&job, file),
            ranges,
        })
    }
    pub fn search(&self, id: Uuid, file: u32, needle: &[u8], start: u64) -> Result<Option<u64>> {
        self.search_snapshot(id, file)?.calculate(needle, start)
    }
}
pub struct SearchSnapshot {
    path: PathBuf,
    ranges: Vec<(u64, u64)>,
}
impl SearchSnapshot {
    pub fn calculate(self, needle: &[u8], start: u64) -> Result<Option<u64>> {
        ensure!(
            !needle.is_empty() && needle.len() <= 1024,
            "search must contain 1–1024 bytes"
        );
        let mut source = File::open(self.path)?;
        for (a, b) in self.ranges {
            let mut offset = a.max(start);
            let mut carry = Vec::<u8>::new();
            while offset < b {
                let length = (b - offset).min(65536) as usize;
                let base = offset.saturating_sub(carry.len() as u64);
                let old = carry.len();
                carry.resize(old + length, 0);
                source.seek(SeekFrom::Start(offset))?;
                source.read_exact(&mut carry[old..])?;
                if let Some(index) = carry.windows(needle.len()).position(|w| w == needle) {
                    return Ok(Some(base + index as u64));
                }
                let keep = (needle.len() - 1).min(carry.len());
                carry.drain(..carry.len() - keep);
                offset += length as u64;
            }
        }
        Ok(None)
    }
}
