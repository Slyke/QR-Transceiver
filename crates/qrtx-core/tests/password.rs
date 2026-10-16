use qrtx_core::{DisplaySettings, Engine, Job, Profile, password, protocol::*};
use std::fs;
use tempfile::TempDir;

fn vector() -> (String, Vec<u8>) {
    let v: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/qrtxenc1.json")).unwrap();
    (
        v["password"].as_str().unwrap().into(),
        hex::decode(v["envelope_hex"].as_str().unwrap()).unwrap(),
    )
}
fn frames(sender: &Engine, j: &Job) -> Vec<Frame> {
    let mut frames = manifest_frames(&j.manifest, j.session);
    for entry in j.manifest.files.iter().filter(|f| !f.directory) {
        for index in 0..entry.chunks {
            let offset = index as u64 * j.manifest.chunk_size as u64;
            frames.push(Frame {
                session: j.session,
                transfer: j.id,
                kind: Kind::Data,
                file: entry.id,
                index,
                total: entry.chunks,
                offset,
                payload: sender
                    .payload(j.id, entry.id, offset, j.manifest.chunk_size as usize)
                    .unwrap()
                    .bytes
                    .into_iter()
                    .map(Option::unwrap)
                    .collect(),
            });
        }
    }
    frames
}
fn manifest_frames(manifest: &Manifest, session: uuid::Uuid) -> Vec<Frame> {
    let bytes = pack(manifest).unwrap();
    bytes
        .chunks(600)
        .enumerate()
        .map(|(index, part)| {
            let mut payload = hex::decode(hash(&bytes)).unwrap();
            payload.extend(part);
            Frame {
                session,
                transfer: manifest.transfer,
                kind: Kind::Manifest,
                file: 0,
                index: index as u32,
                total: bytes.len().div_ceil(600) as u32,
                offset: (index * 600) as u64,
                payload,
            }
        })
        .collect()
}

#[test]
fn optional_encryption_roundtrip_retry_restart_and_unchanged_outer_receipts() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("export.cqs.json.gz");
    fs::write(&source, [0x1f, 0x8b, 0, 255, 1]).unwrap();
    let mut sender = Engine::open(temp.path().join("send")).unwrap();
    let state = temp.path().join("receive");
    let mut receiver = Engine::open(&state).unwrap();
    let profile = Profile {
        trusted_peers: vec![receiver.fingerprint()],
        ..Default::default()
    };
    let ordinary = sender.enqueue(vec![source.clone()], false, None).unwrap();
    assert!(!ordinary.password_bundle);
    assert_eq!(ordinary.manifest.files[0].path, "export.cqs.json.gz");
    let j = sender
        .enqueue_with_password(
            vec![source.clone()],
            false,
            Some(profile),
            Some(" exact 🔒 "),
        )
        .unwrap();
    assert_eq!(j.manifest.files.len(), 1);
    assert_eq!(j.manifest.files[0].path, password::FILENAME);
    let before = sender.payload(j.id, 1, 0, 65536).unwrap().bytes;
    sender
        .update_display_settings(DisplaySettings {
            qr_pixels: Some(900),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(before, sender.payload(j.id, 1, 0, 65536).unwrap().bytes);
    assert_eq!(sender.job(j.id).unwrap().manifest_hash, j.manifest_hash);
    assert!(sender.change_job(j.id, "restart", None).is_err());
    let wire = frames(&sender, &j);
    for f in wire.iter().filter(|f| f.kind == Kind::Manifest) {
        receiver.ingest(&f.encode().unwrap()).unwrap();
    }
    assert!(receiver.job(j.id).unwrap().password_needed);
    for f in wire.iter().filter(|f| f.kind == Kind::Data) {
        receiver.ingest(&f.encode().unwrap()).unwrap();
    }
    assert_eq!(receiver.job(j.id).unwrap().status, "locked");
    assert!(!state.join("received/export.cqs.json.gz").exists());
    let ciphertext = fs::read(state.join(format!("staging/{}/1", j.id))).unwrap();
    assert!(
        receiver
            .set_receive_password(Some(j.id), Some("wrong"))
            .is_err()
    );
    assert_eq!(
        fs::read(state.join(format!("staging/{}/1", j.id))).unwrap(),
        ciphertext
    );
    assert!(!state.join("received/export.cqs.json.gz").exists());
    drop(receiver);
    let mut receiver = Engine::open(&state).unwrap();
    assert!(receiver.job(j.id).unwrap().password_needed);
    receiver
        .set_receive_password(Some(j.id), Some(" exact 🔒 "))
        .unwrap();
    assert_eq!(receiver.job(j.id).unwrap().status, "complete");
    assert_eq!(
        fs::read(state.join("received/export.cqs.json.gz")).unwrap(),
        fs::read(&source).unwrap()
    );
    assert!(!state.join("received/transfer.qrtxenc").exists());
    assert_eq!(receiver.job(j.id).unwrap().manifest_hash, j.manifest_hash);
    for _ in 0..30 {
        if let Some(f) = receiver.next_frame().unwrap() {
            let _ = sender.ingest(&f.text);
        }
        if sender.job(j.id).unwrap().status == "confirmed" {
            break;
        }
    }
    assert_eq!(sender.job(j.id).unwrap().status, "confirmed");
    assert!(
        !serde_json::to_string(&receiver.jobs().unwrap())
            .unwrap()
            .contains(" exact")
    );
    assert!(
        !serde_json::to_string(&receiver.events(0, 100).unwrap())
            .unwrap()
            .contains(" exact")
    );
    drop(receiver);
    let receiver = Engine::open(&state).unwrap();
    assert_eq!(receiver.job(j.id).unwrap().status, "complete");
    assert!(
        !state
            .join("received/export.cqs.json.gz.received-1")
            .exists()
    );
}

#[test]
fn preentered_password_renamed_magic_nested_binary_and_source_changes() {
    let temp = TempDir::new().unwrap();
    let (password, envelope) = vector();
    let source = temp.path().join("renamed.bin");
    fs::write(&source, &envelope).unwrap();
    let mut sender = Engine::open(temp.path().join("a")).unwrap();
    let mut receiver = Engine::open(temp.path().join("b")).unwrap();
    receiver
        .set_receive_password(None, Some(&password))
        .unwrap();
    let j = sender.enqueue(vec![source.clone()], false, None).unwrap();
    for f in frames(&sender, &j) {
        receiver.ingest(&f.encode().unwrap()).unwrap();
    }
    assert_eq!(receiver.job(j.id).unwrap().status, "complete");
    assert!(temp.path().join("b/received/export.cqs.json.gz").exists());
    assert!(temp.path().join("b/received/目录/empty.txt").exists());
    // An envelope INSIDE an encrypted bundle must be passed through as bytes.
    let nested = temp.path().join("nested.qrtxenc");
    fs::write(&nested, &envelope).unwrap();
    let j = sender
        .enqueue_with_password(vec![nested.clone()], false, None, Some("outer"))
        .unwrap();
    receiver.set_receive_password(None, Some("outer")).unwrap();
    for f in frames(&sender, &j) {
        receiver.ingest(&f.encode().unwrap()).unwrap();
    }
    assert_eq!(
        fs::read(temp.path().join("b/received/nested.qrtxenc")).unwrap(),
        envelope
    );
    fs::write(&nested, "modified source").unwrap();
    sender.housekeeping().unwrap();
    assert_eq!(sender.jobs().unwrap().len(), 2);
    assert_eq!(sender.job(j.id).unwrap().status, "active");
}

#[test]
fn sha256_is_checked_before_password_and_corrupt_chunks_can_be_replaced() {
    let temp = TempDir::new().unwrap();
    let (password, envelope) = vector();
    let path = temp.path().join("transfer.qrtxenc");
    fs::write(&path, envelope).unwrap();
    let mut sender = Engine::open(temp.path().join("a")).unwrap();
    let mut receiver = Engine::open(temp.path().join("b")).unwrap();
    receiver
        .set_receive_password(None, Some(&password))
        .unwrap();
    let j = sender.enqueue(vec![path], false, None).unwrap();
    let mut wire = frames(&sender, &j);
    let good = wire.last().unwrap().clone();
    wire.last_mut().unwrap().payload[40] ^= 1;
    for f in &wire[..wire.len() - 1] {
        receiver.ingest(&f.encode().unwrap()).unwrap();
    }
    assert!(
        receiver
            .ingest(&wire.last().unwrap().encode().unwrap())
            .unwrap_err()
            .to_string()
            .contains("SHA-256")
    );
    assert_eq!(receiver.job(j.id).unwrap().bytes, 0);
    assert!(receiver.job(j.id).unwrap().bundle_files.is_empty());
    receiver.ingest(&good.encode().unwrap()).unwrap();
    assert_eq!(receiver.job(j.id).unwrap().status, "complete");
}

#[test]
fn rejects_named_and_renamed_mixed_outer_files_before_committing_anything() {
    let temp = TempDir::new().unwrap();
    let (_, envelope) = vector();
    let mut receiver = Engine::open(temp.path().join("b")).unwrap();
    for name in ["transfer.qrtxenc", "renamed.bin"] {
        let manifest = Manifest {
            version: 1,
            transfer: uuid::Uuid::new_v4(),
            chunk_size: 600,
            files: vec![
                FileEntry {
                    id: 1,
                    path: "ordinary.txt".into(),
                    size: 8,
                    sha256: hash(b"ordinary"),
                    chunks: 1,
                    directory: false,
                },
                FileEntry {
                    id: 2,
                    path: name.into(),
                    size: envelope.len() as u64,
                    sha256: hash(&envelope),
                    chunks: 1,
                    directory: false,
                },
            ],
        };
        let session = uuid::Uuid::new_v4();
        let results: Vec<_> = manifest_frames(&manifest, session)
            .into_iter()
            .map(|f| receiver.ingest(&f.encode().unwrap()))
            .collect();
        if name.ends_with(".qrtxenc") {
            assert!(results.last().unwrap().is_err());
            continue;
        }
        assert!(results.iter().all(Result::is_ok));
        let mut f = Frame {
            session,
            transfer: manifest.transfer,
            kind: Kind::Data,
            file: 1,
            index: 0,
            total: 1,
            offset: 0,
            payload: b"ordinary".to_vec(),
        };
        receiver.ingest(&f.encode().unwrap()).unwrap();
        assert!(!temp.path().join("b/received/ordinary.txt").exists());
        f.file = 2;
        f.payload = envelope.clone();
        let mut corrupt = f.clone();
        corrupt.payload[0] ^= 1;
        assert!(
            receiver
                .ingest(&corrupt.encode().unwrap())
                .unwrap_err()
                .to_string()
                .contains("SHA-256")
        );
        assert!(!temp.path().join("b/received/ordinary.txt").exists());
        assert!(
            receiver
                .ingest(&f.encode().unwrap())
                .unwrap_err()
                .to_string()
                .contains("mixed")
        );
        assert!(!temp.path().join("b/received/ordinary.txt").exists());
    }
}

#[test]
fn corrupt_magic_is_not_persisted_as_a_password_bundle() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("ordinary.bin");
    fs::write(&source, b"ordinary bytes").unwrap();
    let mut sender = Engine::open(temp.path().join("a")).unwrap();
    let mut receiver = Engine::open(temp.path().join("b")).unwrap();
    let j = sender.enqueue(vec![source], false, None).unwrap();
    let mut wire = frames(&sender, &j);
    let good = wire.last().unwrap().clone();
    wire.last_mut().unwrap().payload[..8].copy_from_slice(password::MAGIC);
    for f in &wire[..wire.len() - 1] {
        receiver.ingest(&f.encode().unwrap()).unwrap();
    }
    assert!(
        receiver
            .ingest(&wire.last().unwrap().encode().unwrap())
            .unwrap_err()
            .to_string()
            .contains("SHA-256")
    );
    assert!(!receiver.job(j.id).unwrap().password_bundle);
    receiver.ingest(&good.encode().unwrap()).unwrap();
    assert_eq!(receiver.job(j.id).unwrap().status, "complete");
    assert_eq!(
        fs::read(temp.path().join("b/received/ordinary.bin")).unwrap(),
        b"ordinary bytes"
    );
}

#[test]
fn rejects_oversize_envelope_in_manifest_before_allocating_staging() {
    let temp = TempDir::new().unwrap();
    let mut receiver = Engine::open(temp.path().join("b")).unwrap();
    let size = password::MAX_ENVELOPE as u64 + 1;
    let manifest = Manifest {
        version: 1,
        transfer: uuid::Uuid::new_v4(),
        chunk_size: 600,
        files: vec![FileEntry {
            id: 1,
            path: "transfer.qrtxenc".into(),
            size,
            sha256: hash(&[]),
            chunks: size.div_ceil(600) as u32,
            directory: false,
        }],
    };
    for f in manifest_frames(&manifest, uuid::Uuid::new_v4()) {
        assert!(receiver.ingest(&f.encode().unwrap()).is_err());
    }
    assert!(receiver.jobs().unwrap().is_empty());
    assert!(
        !temp
            .path()
            .join(format!("b/staging/{}", manifest.transfer))
            .exists()
    );
}

#[test]
fn arbitrary_outer_ids_do_not_alias_inner_staging_or_receipts() {
    let temp = TempDir::new().unwrap();
    let (password, envelope) = vector();
    let state = temp.path().join("b");
    let mut receiver = Engine::open(&state).unwrap();
    receiver
        .set_receive_password(None, Some(&password))
        .unwrap();
    let manifest = Manifest {
        version: 1,
        transfer: uuid::Uuid::new_v4(),
        chunk_size: 600,
        files: vec![FileEntry {
            id: 4097,
            path: "renamed.bin".into(),
            size: envelope.len() as u64,
            sha256: hash(&envelope),
            chunks: 1,
            directory: false,
        }],
    };
    let session = uuid::Uuid::new_v4();
    for f in manifest_frames(&manifest, session) {
        receiver.ingest(&f.encode().unwrap()).unwrap();
    }
    let data = Frame {
        session,
        transfer: manifest.transfer,
        kind: Kind::Data,
        file: 4097,
        index: 0,
        total: 1,
        offset: 0,
        payload: envelope.clone(),
    };
    receiver.ingest(&data.encode().unwrap()).unwrap();
    let job = receiver.job(manifest.transfer).unwrap();
    assert_eq!(job.status, "complete");
    assert!(job.bundle_files.iter().all(|f| f.id != 4097));
    assert_eq!(job.delivered_files, vec![4097]);
    assert_eq!(
        fs::read(state.join(format!("staging/{}/4097", job.id))).unwrap(),
        envelope
    );
    // Repeated optical frames still address the original encrypted bytes.
    receiver.ingest(&data.encode().unwrap()).unwrap();
    drop(receiver);
    assert_eq!(
        Engine::open(&state).unwrap().job(job.id).unwrap().status,
        "complete"
    );
    let inner = job.bundle_files[0].id;
    fs::write(
        state.join(format!("staging/{}/{inner}", job.id)),
        b"tampered",
    )
    .unwrap();
    let mut recovered = Engine::open(&state).unwrap();
    assert_eq!(recovered.job(job.id).unwrap().status, "locked");
    assert!(recovered.payload(job.id, inner, 0, 100).is_err());
    recovered
        .set_receive_password(Some(job.id), Some(&password))
        .unwrap();
    assert_eq!(recovered.job(job.id).unwrap().status, "complete");
    assert!(
        !state
            .join("received/export.cqs.json.gz.received-1")
            .exists()
    );
}

#[test]
fn accepts_exact_envelope_limit_and_cleans_failed_sender_snapshots() {
    let temp = TempDir::new().unwrap();
    let mut receiver = Engine::open(temp.path().join("b")).unwrap();
    let size = password::MAX_ENVELOPE as u64;
    let manifest = Manifest {
        version: 1,
        transfer: uuid::Uuid::new_v4(),
        chunk_size: 600,
        files: vec![FileEntry {
            id: 1,
            path: "transfer.qrtxenc".into(),
            size,
            sha256: hash(&[]),
            chunks: size.div_ceil(600) as u32,
            directory: false,
        }],
    };
    for f in manifest_frames(&manifest, uuid::Uuid::new_v4()) {
        receiver.ingest(&f.encode().unwrap()).unwrap();
    }
    assert!(receiver.job(manifest.transfer).unwrap().password_needed);
    let source = temp.path().join("duplicate.txt");
    fs::write(&source, "data").unwrap();
    let mut sender = Engine::open(temp.path().join("a")).unwrap();
    assert!(
        sender
            .enqueue_with_password(vec![source.clone(), source], false, None, Some("password"))
            .is_err()
    );
    assert!(sender.jobs().unwrap().is_empty());
    assert_eq!(
        fs::read_dir(temp.path().join("a/spool")).unwrap().count(),
        0
    );
}

#[test]
fn encrypted_import_recovers_commit_intent_without_duplicate_outputs() {
    let temp = TempDir::new().unwrap();
    let (password, envelope) = vector();
    let source = temp.path().join("transfer.qrtxenc");
    fs::write(&source, envelope).unwrap();
    let mut sender = Engine::open(temp.path().join("a")).unwrap();
    let state = temp.path().join("b");
    let mut receiver = Engine::open(&state).unwrap();
    receiver
        .set_receive_password(None, Some(&password))
        .unwrap();
    let job = sender.enqueue(vec![source], false, None).unwrap();
    let db = rusqlite::Connection::open(state.join("state.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_inner BEFORE INSERT ON events WHEN NEW.body LIKE '%file.verified%' BEGIN SELECT RAISE(FAIL,'simulated storage failure'); END;").unwrap();
    let wire = frames(&sender, &job);
    for f in &wire[..wire.len() - 1] {
        receiver.ingest(&f.encode().unwrap()).unwrap();
    }
    assert!(
        receiver
            .ingest(&wire.last().unwrap().encode().unwrap())
            .is_err()
    );
    assert!(receiver.job(job.id).unwrap().delivered_files.is_empty());
    drop(receiver);
    db.execute_batch("DROP TRIGGER fail_inner").unwrap();
    let mut receiver = Engine::open(&state).unwrap();
    assert!(receiver.job(job.id).unwrap().password_needed);
    receiver
        .set_receive_password(Some(job.id), Some(&password))
        .unwrap();
    assert_eq!(receiver.job(job.id).unwrap().status, "complete");
    assert!(
        !state
            .join("received/export.cqs.json.gz.received-1")
            .exists()
    );
    assert_eq!(
        receiver
            .events(0, 100)
            .unwrap()
            .iter()
            .filter(|(_, e)| e.event == "file.verified")
            .count(),
        2
    );
}
