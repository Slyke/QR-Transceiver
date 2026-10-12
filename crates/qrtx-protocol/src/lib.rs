//! QRTX/1: bounded optical framing, manifests, QR encoding and signed receipts.
use anyhow::{Result, bail, ensure};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const PREFIX: &str = "QRTX/1:";
pub const HEADER: usize = 56;
pub const MAX_PAYLOAD: usize = 1200;
pub const MAX_TEXT: usize = 4096;
pub const MAX_MANIFEST: usize = 1_048_576;
pub const MAX_FILES: usize = 4096;
pub const MAX_CHUNKS: u32 = 4_000_000;
pub const MAX_FILE_SIZE: u64 = 64 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Manifest = 1,
    Data = 2,
    Control = 3,
    Bundle = 4,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Frame {
    pub session: Uuid,
    pub transfer: Uuid,
    pub kind: Kind,
    pub file: u32,
    pub index: u32,
    pub total: u32,
    pub offset: u64,
    pub payload: Vec<u8>,
}
impl Frame {
    pub fn encode(&self) -> Result<String> {
        ensure!(
            self.payload.len() <= MAX_PAYLOAD,
            "frame payload exceeds limit"
        );
        let mut b = Vec::with_capacity(HEADER + self.payload.len() + 4);
        b.extend([1, self.kind as u8]);
        b.extend(self.session.as_bytes());
        b.extend(self.transfer.as_bytes());
        b.extend(self.file.to_be_bytes());
        b.extend(self.index.to_be_bytes());
        b.extend(self.total.to_be_bytes());
        b.extend(self.offset.to_be_bytes());
        b.extend((self.payload.len() as u16).to_be_bytes());
        b.extend(&self.payload);
        b.extend(crc32c::crc32c(&b).to_be_bytes());
        Ok(format!("{PREFIX}{}", base45::encode(&b)))
    }
    pub fn decode(text: &str) -> Result<Self> {
        ensure!(text.len() <= MAX_TEXT, "frame text exceeds limit");
        let encoded = text
            .strip_prefix(PREFIX)
            .ok_or_else(|| anyhow::anyhow!("unsupported protocol"))?;
        let b = base45::decode(encoded).map_err(|e| anyhow::anyhow!("invalid Base45: {e:?}"))?;
        ensure!(
            b.len() >= HEADER + 4 && b.len() <= HEADER + MAX_PAYLOAD + 4,
            "invalid frame length"
        );
        ensure!(b[0] == 1, "unsupported version");
        let kind = match b[1] {
            1 => Kind::Manifest,
            2 => Kind::Data,
            3 => Kind::Control,
            4 => Kind::Bundle,
            _ => bail!("unknown frame type"),
        };
        let n = u16::from_be_bytes(b[54..56].try_into()?) as usize;
        ensure!(b.len() == HEADER + n + 4, "payload length mismatch");
        ensure!(
            crc32c::crc32c(&b[..b.len() - 4]) == u32::from_be_bytes(b[b.len() - 4..].try_into()?),
            "CRC32C mismatch"
        );
        let f = Self {
            session: Uuid::from_slice(&b[2..18])?,
            transfer: Uuid::from_slice(&b[18..34])?,
            kind,
            file: u32::from_be_bytes(b[34..38].try_into()?),
            index: u32::from_be_bytes(b[38..42].try_into()?),
            total: u32::from_be_bytes(b[42..46].try_into()?),
            offset: u64::from_be_bytes(b[46..54].try_into()?),
            payload: b[56..56 + n].to_vec(),
        };
        ensure!(
            f.total > 0 && f.total <= MAX_CHUNKS && f.index < f.total && f.offset <= MAX_FILE_SIZE,
            "frame bounds invalid"
        );
        if kind == Kind::Control {
            ensure!(
                f.index == 0 && f.total == 1 && f.offset == 0,
                "invalid control header"
            );
        }
        Ok(f)
    }
}
pub fn pack<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut v = Vec::new();
    ciborium::into_writer(value, &mut v)?;
    Ok(v)
}
pub fn unpack<T: DeserializeOwned>(b: &[u8]) -> Result<T> {
    let mut reader = std::io::Cursor::new(b);
    let value = ciborium::from_reader(&mut reader)?;
    ensure!(reader.position() == b.len() as u64, "trailing CBOR bytes");
    Ok(value)
}
pub fn hash(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub id: u32,
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub chunks: u32,
    pub directory: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u8,
    pub transfer: Uuid,
    pub chunk_size: u32,
    pub files: Vec<FileEntry>,
}
impl Manifest {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && (64..=MAX_PAYLOAD as u32).contains(&self.chunk_size),
            "invalid manifest settings"
        );
        ensure!(
            !self.files.is_empty() && self.files.len() <= MAX_FILES,
            "invalid file count"
        );
        let mut paths = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        let mut bytes = 0u64;
        for f in &self.files {
            validate_path(&f.path)?;
            ensure!(
                paths.insert(f.path.to_lowercase()) && ids.insert(f.id),
                "duplicate path or file identity"
            );
            ensure!(
                f.size <= MAX_FILE_SIZE && f.chunks <= MAX_CHUNKS,
                "file exceeds limits"
            );
            ensure!(
                f.chunks
                    == if f.directory {
                        0
                    } else {
                        f.size.div_ceil(self.chunk_size as u64).max(1) as u32
                    },
                "incorrect chunk count"
            );
            ensure!(
                f.sha256.len() == 64 && hex::decode(&f.sha256)?.len() == 32,
                "invalid SHA-256"
            );
            ensure!(!f.directory || f.size == 0, "directory contains data");
            bytes = bytes
                .checked_add(f.size)
                .ok_or_else(|| anyhow::anyhow!("manifest overflow"))?;
        }
        ensure!(bytes <= MAX_FILE_SIZE, "batch exceeds storage limit");
        Ok(())
    }
}
pub fn validate_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty() && path.len() <= 1024 && !path.starts_with('/') && !path.contains('\\'),
        "unsafe path"
    );
    for part in path.split('/') {
        ensure!(
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !part
                    .chars()
                    .any(|c| c.is_control() || "<>:\"|?*".contains(c)),
            "unsafe path component"
        );
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        ensure!(
            !["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.as_bytes()[3].is_ascii_digit()),
            "reserved filename"
        );
        ensure!(!part.starts_with(".qrtx"), "reserved staging path");
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Control {
    Hello,
    Ready,
    AckFrame { file: u32, ranges: Vec<(u32, u32)> },
    AckFile { file: u32, sha256: String },
    AckBatch { manifest_hash: String },
    MissingChunks { file: u32, ranges: Vec<(u32, u32)> },
    Pause,
    Resume,
    Cancel,
    Error { message: String },
    Complete { manifest_hash: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub sequence: u64,
    pub body: Control,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}
impl Receipt {
    pub fn new(
        session: Uuid,
        transfer: Uuid,
        sequence: u64,
        body: Control,
        key: &SigningKey,
    ) -> Result<Self> {
        let signature = key
            .sign(&pack(&(
                "QRTX/1 receipt",
                session,
                transfer,
                sequence,
                &body,
            ))?)
            .to_bytes()
            .to_vec();
        Ok(Self {
            sequence,
            body,
            public_key: key.verifying_key().to_bytes().to_vec(),
            signature,
        })
    }
    pub fn verify(&self, session: Uuid, transfer: Uuid) -> Result<String> {
        ensure!(self.sequence > 0, "invalid receipt sequence");
        let key = VerifyingKey::from_bytes(&self.public_key.as_slice().try_into()?)?;
        let sig = Signature::from_slice(&self.signature)?;
        key.verify_strict(
            &pack(&(
                "QRTX/1 receipt",
                session,
                transfer,
                self.sequence,
                &self.body,
            ))?,
            &sig,
        )?;
        Ok(hash(&self.public_key))
    }
}
pub fn new_identity() -> SigningKey {
    SigningKey::generate(&mut rand::rngs::OsRng)
}
pub use ed25519_dalek::SigningKey as Identity;
pub fn qr_svg(text: &str, correction: &str, max_version: i16) -> Result<String> {
    let code = qr(text, correction, max_version)?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(512, 512)
        .quiet_zone(true)
        .build())
}
pub fn qr(text: &str, correction: &str, max_version: i16) -> Result<qrcode::QrCode> {
    let ec = match correction {
        "L" => qrcode::EcLevel::L,
        "M" => qrcode::EcLevel::M,
        "Q" => qrcode::EcLevel::Q,
        "H" => qrcode::EcLevel::H,
        _ => bail!("unknown QR correction level"),
    };
    ensure!((1..=40).contains(&max_version), "QR version must be 1–40");
    let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), ec)?;
    ensure!(
        code.width() <= (17 + 4 * max_version) as usize,
        "payload exceeds selected QR density; reduce chunk size or correction level"
    );
    Ok(code)
}
pub fn scan(image: image::GrayImage) -> Vec<String> {
    let mut prepared = rqrr::PreparedImage::prepare(image);
    prepared
        .detect_grids()
        .iter()
        .filter_map(|g| g.decode().ok().map(|(_, s)| s))
        .filter(|s| s.starts_with(PREFIX) && s.len() <= MAX_TEXT)
        .collect()
}
pub use image;
pub fn wire_checksum(text: &str) -> Result<u32> {
    let b = base45::decode(
        text.strip_prefix(PREFIX)
            .ok_or_else(|| anyhow::anyhow!("invalid prefix"))?,
    )
    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    ensure!(b.len() >= 4, "short frame");
    Ok(u32::from_be_bytes(b[b.len() - 4..].try_into()?))
}

pub struct QrStyle<'a> {
    pub pixels: u32,
    pub quiet: u32,
    pub foreground: &'a str,
    pub background: &'a str,
    pub inverted: bool,
}
pub fn qr_custom_svg(
    text: &str,
    correction: &str,
    max_version: i16,
    style: &QrStyle<'_>,
) -> Result<String> {
    let QrStyle {
        pixels,
        quiet,
        foreground,
        background,
        inverted,
    } = *style;
    use std::fmt::Write;
    ensure!((4..=16).contains(&quiet), "invalid quiet zone");
    for c in [foreground, background] {
        ensure!(
            c.len() == 7 && c.starts_with('#') && hex::decode(&c[1..]).is_ok(),
            "invalid QR color"
        );
    }
    let code = qr(text, correction, max_version)?;
    let width = code.width();
    let side = width + 2 * quiet as usize;
    let size = (pixels as usize / side).max(1) * side;
    let (fg, bg) = if inverted {
        (background, foreground)
    } else {
        (foreground, background)
    };
    let mut path = String::new();
    for y in 0..width {
        for x in 0..width {
            if code[(x, y)] == qrcode::Color::Dark {
                write!(
                    path,
                    "M{} {}h1v1h-1z",
                    x + quiet as usize,
                    y + quiet as usize
                )?;
            }
        }
    }
    Ok(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {side} {side}" shape-rendering="crispEdges"><rect width="{side}" height="{side}" fill="{bg}"/><path d="{path}" fill="{fg}"/></svg>"#
    ))
}
pub fn maximum_chunk(correction: &str, max_version: i16) -> usize {
    let mut low = 0usize;
    let mut high = 1168usize;
    while low < high {
        let mid = (low + high).div_ceil(2);
        let f = Frame {
            session: Uuid::nil(),
            transfer: Uuid::nil(),
            kind: Kind::Manifest,
            file: 0,
            index: 0,
            total: 1,
            offset: 0,
            payload: vec![255; mid + 32],
        };
        if f.encode()
            .and_then(|s| qr(&s, correction, max_version))
            .is_ok()
        {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    low
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_roundtrip_and_crc() {
        let f = Frame {
            session: Uuid::new_v4(),
            transfer: Uuid::new_v4(),
            kind: Kind::Data,
            file: 1,
            index: 0,
            total: 1,
            offset: 0,
            payload: (0..=255).collect(),
        };
        assert_eq!(Frame::decode(&f.encode().unwrap()).unwrap(), f);
        let mut wire = base45::decode(f.encode().unwrap().strip_prefix(PREFIX).unwrap()).unwrap();
        wire[60] ^= 1;
        assert!(Frame::decode(&format!("{PREFIX}{}", base45::encode(&wire))).is_err());
    }
    #[test]
    fn paths_are_portable_and_confined() {
        for p in [
            "../a",
            "/a",
            "C:/a",
            "a\\b",
            "a/../../b",
            "NUL.txt",
            "a:",
            "a/",
            ".qrtx-stage/a",
        ] {
            assert!(validate_path(p).is_err(), "{p}");
        }
        assert!(validate_path("nested/日本語.bin").is_ok());
    }
    #[test]
    fn signed_receipts_bind_session() {
        let s = Uuid::new_v4();
        let t = Uuid::new_v4();
        let r = Receipt::new(s, t, 1, Control::Ready, &new_identity()).unwrap();
        assert!(r.verify(s, t).is_ok());
        assert!(r.verify(Uuid::new_v4(), t).is_err());
    }
    #[test]
    fn optical_roundtrip() {
        let text = Frame {
            session: Uuid::new_v4(),
            transfer: Uuid::new_v4(),
            kind: Kind::Data,
            file: 1,
            index: 0,
            total: 1,
            offset: 0,
            payload: vec![42; 400],
        }
        .encode()
        .unwrap();
        let q = qr(&text, "M", 40).unwrap();
        let side = q.width() + 8;
        let im = image::GrayImage::from_fn((side * 5) as u32, (side * 5) as u32, |x, y| {
            let (x, y) = (x as usize / 5, y as usize / 5);
            image::Luma([
                if x >= 4
                    && y >= 4
                    && x < side - 4
                    && y < side - 4
                    && q[(x - 4, y - 4)] == qrcode::Color::Dark
                {
                    0
                } else {
                    255
                },
            ])
        });
        assert_eq!(scan(im), vec![text]);
    }
}
