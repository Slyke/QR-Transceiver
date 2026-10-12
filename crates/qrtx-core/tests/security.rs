use qrtx_core::{Conflict, Engine, Policy, Profile, protocol::*};
use std::fs;
use tempfile::TempDir;

#[test]
fn static_qr_is_self_contained_and_recovers_empty_binary_files() {
    for bytes in [vec![], vec![0], vec![255, 0, 128, 42], b"hello".to_vec()] {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("tiny.bin");
        fs::write(&path, &bytes).unwrap();
        let mut a = Engine::open(tmp.path().join("a")).unwrap();
        let mut b = Engine::open(tmp.path().join("b")).unwrap();
        let j = a.enqueue(vec![path], false, None).unwrap();
        let f = a.next_frame().unwrap().unwrap();
        assert_eq!(f.frame.kind, Kind::Bundle);
        b.ingest(&f.text).unwrap();
        assert_eq!(b.job(j.id).unwrap().status, "complete");
        assert_eq!(
            fs::read(tmp.path().join("b/received/tiny.bin")).unwrap(),
            bytes
        );
    }
}
#[test]
fn unpaired_claim_never_confirms_and_replays_do_not_repeat_events() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("x");
    fs::write(&path, b"data").unwrap();
    let mut a = Engine::open(tmp.path().join("a")).unwrap();
    let mut b = Engine::open(tmp.path().join("b")).unwrap();
    let j = a.enqueue(vec![path], false, None).unwrap();
    b.ingest(&a.next_frame().unwrap().unwrap().text).unwrap();
    for _ in 0..8 {
        if let Some(f) = b.next_frame().unwrap() {
            let _ = a.ingest(&f.text);
        }
    }
    assert_eq!(a.job(j.id).unwrap().status, "acknowledged");
    assert!(
        !a.events(0, 100)
            .unwrap()
            .iter()
            .any(|(_, e)| e.event == "delivery.confirmed")
    );
    let n = a.events(0, 100).unwrap().len();
    for _ in 0..8 {
        if let Some(f) = b.next_frame().unwrap() {
            let _ = a.ingest(&f.text);
        }
    }
    assert_eq!(n, a.events(0, 100).unwrap().len());
}
#[test]
fn default_conflict_renames_and_identical_policy_skips() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("x");
    fs::write(&path, b"new").unwrap();
    let mut a = Engine::open(tmp.path().join("a")).unwrap();
    let mut b = Engine::open(tmp.path().join("b")).unwrap();
    fs::create_dir_all(tmp.path().join("b/received")).unwrap();
    fs::write(tmp.path().join("b/received/x"), b"old").unwrap();
    a.enqueue(vec![path.clone()], false, None).unwrap();
    b.ingest(&a.next_frame().unwrap().unwrap().text).unwrap();
    assert_eq!(fs::read(tmp.path().join("b/received/x")).unwrap(), b"old");
    assert_eq!(
        fs::read(tmp.path().join("b/received/x.received-1")).unwrap(),
        b"new"
    );
    let mut p = b.profile.clone();
    p.conflict = Conflict::SkipIdentical;
    b.set_profile(p).unwrap();
    fs::write(tmp.path().join("b/received/x"), b"new").unwrap();
    let prior = a.jobs().unwrap()[0].id;
    a.change_job(prior, "cancel", None).unwrap();
    let j = a.enqueue(vec![path], false, None).unwrap();
    b.ingest(&a.next_frame().unwrap().unwrap().text).unwrap();
    assert_eq!(b.job(j.id).unwrap().status, "complete");
    assert!(!tmp.path().join("b/received/x.received-2").exists());
}
#[cfg(unix)]
#[test]
fn symlink_destination_cannot_escape_capability() {
    use std::os::unix::fs::symlink;
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("input");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("x"), b"protected").unwrap();
    let mut a = Engine::open(tmp.path().join("a")).unwrap();
    let mut b = Engine::open(tmp.path().join("b")).unwrap();
    let outside = tmp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::create_dir_all(tmp.path().join("b/received")).unwrap();
    symlink(&outside, tmp.path().join("b/received/input")).unwrap();
    a.enqueue(vec![input], true, None).unwrap();
    let mut rejected = false;
    for _ in 0..12 {
        if let Some(f) = a.next_frame().unwrap()
            && b.ingest(&f.text).is_err()
        {
            rejected = true;
        }
    }
    assert!(rejected);
    assert!(!outside.join("x").exists());
}
#[test]
fn missing_request_restores_an_acknowledged_frame() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("x");
    fs::write(&path, vec![1; 1800]).unwrap();
    let mut a = Engine::open(tmp.path().join("a")).unwrap();
    let j = a.enqueue(vec![path], false, None).unwrap();
    let key = new_identity();
    let control = |seq, body| {
        Frame {
            session: j.session,
            transfer: j.id,
            kind: Kind::Control,
            file: 0,
            index: 0,
            total: 1,
            offset: 0,
            payload: pack(&Receipt::new(j.session, j.id, seq, body, &key).unwrap()).unwrap(),
        }
        .encode()
        .unwrap()
    };
    a.ingest(&control(
        1,
        Control::AckFrame {
            file: 1,
            ranges: vec![(0, 0)],
        },
    ))
    .unwrap();
    a.ingest(&control(
        2,
        Control::MissingChunks {
            file: 1,
            ranges: vec![(0, 0)],
        },
    ))
    .unwrap();
    let mut restored = false;
    for _ in 0..5 {
        if let Some(f) = a.next_frame().unwrap()
            && f.frame.kind == Kind::Data
            && f.frame.index == 0
        {
            restored = true;
            break;
        }
    }
    assert!(restored);
    assert!(
        a.ingest(&control(
            3,
            Control::AckFrame {
                file: 1,
                ranges: vec![(0, u32::MAX)]
            }
        ))
        .is_err()
    );
}
#[test]
fn restart_uses_new_identity_and_old_receipt_cannot_complete_it() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("x");
    fs::write(&path, b"first").unwrap();
    let mut a = Engine::open(tmp.path().join("a")).unwrap();
    let j = a.enqueue(vec![path.clone()], false, None).unwrap();
    fs::write(path, b"second").unwrap();
    a.change_job(j.id, "restart", None).unwrap();
    let jobs = a.jobs().unwrap();
    let new = jobs.iter().find(|n| n.id != j.id).unwrap();
    assert_ne!(new.session, j.session);
    let key = new_identity();
    let payload = pack(
        &Receipt::new(
            j.session,
            j.id,
            1,
            Control::AckBatch {
                manifest_hash: j.manifest_hash,
            },
            &key,
        )
        .unwrap(),
    )
    .unwrap();
    let bad = Frame {
        session: j.session,
        transfer: new.id,
        kind: Kind::Control,
        file: 0,
        index: 0,
        total: 1,
        offset: 0,
        payload,
    };
    assert!(a.ingest(&bad.encode().unwrap()).is_err());
    assert_eq!(a.job(new.id).unwrap().status, "active");
}
#[test]
fn both_directions_make_progress_and_control_does_not_starve_data() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("x");
    fs::write(&path, vec![6; 2400]).unwrap();
    let mut a = Engine::open(tmp.path().join("a")).unwrap();
    let mut b = Engine::open(tmp.path().join("b")).unwrap();
    let pa = Profile {
        trusted_peers: vec![b.fingerprint()],
        policy: Policy::UntilAcknowledged,
        ..Profile::default()
    };
    let pb = Profile {
        trusted_peers: vec![a.fingerprint()],
        policy: Policy::UntilAcknowledged,
        ..Profile::default()
    };
    let ja = a.enqueue(vec![path.clone()], false, Some(pa)).unwrap();
    let jb = b.enqueue(vec![path], false, Some(pb)).unwrap();
    for _ in 0..200 {
        if let Some(f) = a.next_frame().unwrap() {
            let _ = b.ingest(&f.text);
        }
        if let Some(f) = b.next_frame().unwrap() {
            let _ = a.ingest(&f.text);
        }
        if a.job(ja.id).unwrap().status == "confirmed"
            && b.job(jb.id).unwrap().status == "confirmed"
        {
            return;
        }
    }
    panic!("bidirectional transfer stalled");
}
