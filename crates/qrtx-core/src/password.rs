//! QRTXENC1 is an application file format, independent of QRTX/1 transport.
use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::AeadInPlace};
use anyhow::{Context, Result, ensure};
use argon2::{Algorithm, Argon2, Params, Version};
use qrtx_protocol::validate_path;
use rand::{RngCore, rngs::OsRng};
use std::collections::HashSet;
use zeroize::Zeroizing;

pub const MAGIC: &[u8; 8] = b"QRTXENC1";
pub const FILENAME: &str = "transfer.qrtxenc";
pub const MAX_ENVELOPE: usize = 256_000_000;
const HEADER: usize = 36;
const TAG: usize = 16;
const FILE_MAGIC: &[u8; 8] = b"QRTXFIL1";

pub fn is_password_filename(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".qrtxenc")
}

pub fn validate_password(password: &str) -> Result<()> {
    ensure!(
        (1..=4096).contains(&password.len()),
        "password must contain 1–4096 UTF-8 bytes"
    );
    Ok(())
}

fn key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    validate_password(password)?;
    let params = Params::new(65536, 3, 1, Some(32)).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut key = Zeroizing::new([0; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut *key)
        .map_err(|e| anyhow::anyhow!("key derivation failed: {e}"))?;
    Ok(key)
}

/// A bounded builder; callers can append snapshots without holding a second file copy.
pub(crate) struct BundleBuilder {
    pub(crate) bytes: Zeroizing<Vec<u8>>,
    names: HashSet<String>,
}
impl BundleBuilder {
    pub(crate) fn new() -> Self {
        let mut bytes = Zeroizing::new(FILE_MAGIC.to_vec());
        bytes.extend(0u32.to_be_bytes());
        Self {
            bytes,
            names: HashSet::new(),
        }
    }
    pub(crate) fn push(&mut self, name: &str, size: u64, reader: impl std::io::Read) -> Result<()> {
        validate_path(name)?;
        ensure!(self.names.len() < 4096, "bundle must contain 1–4096 files");
        ensure!(
            self.names.insert(name.to_lowercase()),
            "duplicate bundle filename"
        );
        let size: usize = size.try_into().context("file length overflow")?;
        let end = self
            .bytes
            .len()
            .checked_add(12)
            .and_then(|n| n.checked_add(name.len()))
            .and_then(|n| n.checked_add(size))
            .context("bundle length overflow")?;
        ensure!(
            end <= MAX_ENVELOPE - HEADER - TAG,
            "encrypted envelope exceeds 256,000,000 bytes"
        );
        self.bytes.extend((name.len() as u32).to_be_bytes());
        self.bytes.extend(name.as_bytes());
        self.bytes.extend((size as u64).to_be_bytes());
        let start = self.bytes.len();
        self.bytes.resize(end, 0);
        let mut reader = reader;
        reader.read_exact(&mut self.bytes[start..])?;
        let mut extra = [0];
        ensure!(reader.read(&mut extra)? == 0, "file changed while bundling");
        self.bytes[8..12].copy_from_slice(&(self.names.len() as u32).to_be_bytes());
        Ok(())
    }
    pub(crate) fn encrypt(mut self, password: &str) -> Result<Vec<u8>> {
        ensure!(!self.names.is_empty(), "bundle must contain 1–4096 files");
        let mut header = [0; HEADER];
        header[..8].copy_from_slice(MAGIC);
        OsRng
            .try_fill_bytes(&mut header[8..])
            .context("secure randomness unavailable")?;
        encrypt(&mut self.bytes, password, header)
    }
}

fn encrypt(plaintext: &mut Vec<u8>, password: &str, header: [u8; HEADER]) -> Result<Vec<u8>> {
    let key = key(password, &header[8..24])?;
    let cipher = Aes256Gcm::new_from_slice(&*key).expect("32-byte key");
    cipher
        .encrypt_in_place(Nonce::from_slice(&header[24..]), &header, plaintext)
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;
    let mut envelope = Vec::with_capacity(HEADER + plaintext.len());
    envelope.extend(header);
    envelope.extend_from_slice(plaintext);
    Ok(envelope)
}

/// All authentication and structural validation finishes before any file is returned.
/// Nested envelopes are ordinary bytes: callers must not invoke this recursively.
pub fn decrypt(envelope: &[u8], password: &str) -> Result<DecryptedBundle> {
    ensure!(
        (HEADER + TAG..=MAX_ENVELOPE).contains(&envelope.len()),
        "invalid encrypted envelope size"
    );
    ensure!(envelope.starts_with(MAGIC), "invalid QRTXENC1 magic");
    let key = key(password, &envelope[8..24])?;
    let cipher = Aes256Gcm::new_from_slice(&*key).expect("32-byte key");
    let mut bytes = Zeroizing::new(envelope[HEADER..].to_vec());
    cipher
        .decrypt_in_place(
            Nonce::from_slice(&envelope[24..36]),
            &envelope[..HEADER],
            &mut *bytes,
        )
        .map_err(|_| anyhow::anyhow!("incorrect password or damaged encrypted bundle"))?;
    let entries = parse(&bytes)?;
    Ok(DecryptedBundle { bytes, entries })
}

pub struct DecryptedBundle {
    bytes: Zeroizing<Vec<u8>>,
    entries: Vec<(String, std::ops::Range<usize>)>,
}
impl DecryptedBundle {
    pub fn files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.entries
            .iter()
            .map(|(name, range)| (name.as_str(), &self.bytes[range.clone()]))
    }
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize, length: usize) -> Result<&'a [u8]> {
    let end = cursor
        .checked_add(length)
        .context("bundle length overflow")?;
    let value = bytes.get(*cursor..end).context("truncated bundle")?;
    *cursor = end;
    Ok(value)
}
fn parse(bytes: &[u8]) -> Result<Vec<(String, std::ops::Range<usize>)>> {
    let mut cursor = 0;
    ensure!(
        take(bytes, &mut cursor, 8)? == FILE_MAGIC,
        "invalid QRTXFIL1 magic"
    );
    let count = u32::from_be_bytes(take(bytes, &mut cursor, 4)?.try_into()?);
    ensure!(
        (1..=4096).contains(&count),
        "bundle must contain 1–4096 files"
    );
    let mut names = HashSet::new();
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let length = u32::from_be_bytes(take(bytes, &mut cursor, 4)?.try_into()?) as usize;
        ensure!((1..=1024).contains(&length), "invalid filename length");
        let name = std::str::from_utf8(take(bytes, &mut cursor, length)?)?;
        validate_path(name)?;
        ensure!(
            names.insert(name.to_lowercase()),
            "duplicate bundle filename"
        );
        let length = u64::from_be_bytes(take(bytes, &mut cursor, 8)?.try_into()?);
        let length: usize = length.try_into().context("file length overflow")?;
        let start = cursor;
        take(bytes, &mut cursor, length)?;
        entries.push((name.to_owned(), start..cursor));
    }
    ensure!(cursor == bytes.len(), "trailing bundle bytes");
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vector() -> serde_json::Value {
        serde_json::from_str(include_str!("../../../tests/fixtures/qrtxenc1.json")).unwrap()
    }

    #[test]
    fn independent_argon2_and_aes_gcm_vector() {
        let v = vector();
        let password = v["password"].as_str().unwrap();
        let envelope = hex::decode(v["envelope_hex"].as_str().unwrap()).unwrap();
        assert_eq!(
            hex::encode(*key(password, &envelope[8..24]).unwrap()),
            v["key_hex"]
        );
        let mut plaintext = hex::decode(v["plaintext_hex"].as_str().unwrap()).unwrap();
        assert_eq!(
            encrypt(&mut plaintext, password, envelope[..36].try_into().unwrap()).unwrap(),
            envelope
        );
        let bundle = decrypt(&envelope, password).unwrap();
        let files: Vec<_> = bundle.files().collect();
        for (i, (name, data)) in files.iter().enumerate() {
            assert_eq!(*name, v["files"][i]["name"].as_str().unwrap());
            assert_eq!(hex::encode(data), v["files"][i]["hex"]);
        }
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn authenticates_header_ciphertext_and_entire_tag() {
        let v = vector();
        let envelope = hex::decode(v["envelope_hex"].as_str().unwrap()).unwrap();
        for offset in [0, 8, 24, 36, envelope.len() - 16, envelope.len() - 1] {
            let mut bad = envelope.clone();
            bad[offset] ^= 1;
            assert!(decrypt(&bad, v["password"].as_str().unwrap()).is_err());
        }
        assert!(decrypt(&envelope, v["password"].as_str().unwrap().trim()).is_err());
        assert!(decrypt(&envelope, "  pässé🔒  ").is_err());
        assert!(
            decrypt(
                &envelope[..envelope.len() - 1],
                v["password"].as_str().unwrap()
            )
            .is_err()
        );
    }

    #[test]
    fn password_bytes_and_randomness() {
        assert!(validate_password("").is_err());
        assert!(validate_password(&"x".repeat(4097)).is_err());
        assert!(validate_password(&"🔒".repeat(1025)).is_err());
        assert!(validate_password(" ").is_ok());
        assert!(validate_password("\0").is_ok());
        let password = "🔒".repeat(1024);
        let bundle = || {
            let mut b = BundleBuilder::new();
            b.push("a", 0, &b""[..]).unwrap();
            b
        };
        let a = bundle().encrypt(&password).unwrap();
        let b = bundle().encrypt(&password).unwrap();
        assert_ne!(&a[8..24], &b[8..24]);
        assert_ne!(&a[24..36], &b[24..36]);
        assert_eq!(decrypt(&a, &password).unwrap().files().count(), 1);
    }

    #[test]
    fn rejects_every_truncation_trailing_bytes_and_invalid_lengths() {
        let v = vector();
        let plain = hex::decode(v["plaintext_hex"].as_str().unwrap()).unwrap();
        for end in 0..plain.len() {
            assert!(parse(&plain[..end]).is_err(), "length {end}");
        }
        let mut trailing = plain.clone();
        trailing.push(0);
        assert!(parse(&trailing).is_err());
        for count in [0u32, 4097, u32::MAX] {
            let mut bad = plain.clone();
            bad[8..12].copy_from_slice(&count.to_be_bytes());
            assert!(parse(&bad).is_err());
        }
        for length in [0u32, 1025, u32::MAX] {
            let mut bad = plain.clone();
            bad[12..16].copy_from_slice(&length.to_be_bytes());
            assert!(parse(&bad).is_err());
        }
        let mut bad = plain.clone();
        bad[16] = 0xff;
        assert!(parse(&bad).is_err());
        let mut bad = plain;
        let offset = 16 + "export.cqs.json.gz".len();
        bad[offset..offset + 8].copy_from_slice(&u64::MAX.to_be_bytes());
        assert!(parse(&bad).is_err());
        assert!(
            BundleBuilder::new()
                .push("a", u64::MAX, std::io::empty())
                .is_err()
        );
        assert!(
            BundleBuilder::new()
                .push("a", MAX_ENVELOPE as u64, std::io::empty())
                .is_err()
        );
    }

    #[test]
    fn validates_all_paths_duplicates_and_limits_before_extraction() {
        for name in [
            "../escape",
            "/absolute",
            "a\\b",
            "CON",
            "a/../b",
            ".qrtx-private",
            "a.",
            "a ",
            "a//b",
        ] {
            let mut plain = FILE_MAGIC.to_vec();
            plain.extend(1u32.to_be_bytes());
            plain.extend((name.len() as u32).to_be_bytes());
            plain.extend(name.as_bytes());
            plain.extend(0u64.to_be_bytes());
            assert!(parse(&plain).is_err(), "{name}");
        }
        let mut b = BundleBuilder::new();
        b.push(&"a".repeat(1024), 0, std::io::empty()).unwrap();
        assert!(b.push(&"b".repeat(1025), 0, std::io::empty()).is_err());
        for i in 1..4096 {
            b.push(&format!("{i}"), 0, std::io::empty()).unwrap();
        }
        assert_eq!(parse(&b.bytes).unwrap().len(), 4096);
        assert!(b.push("one-too-many", 0, std::io::empty()).is_err());
        let mut b = BundleBuilder::new();
        b.push("same", 0, std::io::empty()).unwrap();
        let record = b.bytes[12..].to_vec();
        b.bytes.extend(record);
        b.bytes[8..12].copy_from_slice(&2u32.to_be_bytes());
        assert!(parse(&b.bytes).is_err());
        let mut b = BundleBuilder::new();
        b.push("same", 0, std::io::empty()).unwrap();
        assert!(b.push("SAME", 0, std::io::empty()).is_err());
        let mut record = b.bytes[12..].to_vec();
        record[4..8].copy_from_slice(b"SAME");
        b.bytes.extend(record);
        b.bytes[8..12].copy_from_slice(&2u32.to_be_bytes());
        assert!(parse(&b.bytes).is_err());
    }

    #[test]
    fn authenticated_malformed_bundle_is_rejected() {
        let v = vector();
        let envelope = hex::decode(v["envelope_hex"].as_str().unwrap()).unwrap();
        let mut plaintext = hex::decode(v["plaintext_hex"].as_str().unwrap()).unwrap();
        plaintext.push(0);
        let bad = encrypt(
            &mut plaintext,
            "password",
            envelope[..36].try_into().unwrap(),
        )
        .unwrap();
        assert!(
            decrypt(&bad, "password")
                .err()
                .unwrap()
                .to_string()
                .contains("trailing")
        );
    }
}
