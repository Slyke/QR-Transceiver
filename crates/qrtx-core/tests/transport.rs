use qrtx_core::{Engine, Mode, Policy, Profile, protocol::*};
use std::fs;
use tempfile::TempDir;

fn setup() -> (TempDir, Engine, Engine, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("input");
    fs::create_dir(&input).unwrap();
    let a = Engine::open(tmp.path().join("a")).unwrap();
    let b = Engine::open(tmp.path().join("b")).unwrap();
    (tmp, a, b, input)
}
fn frames(a: &mut Engine) -> Vec<Frame> {
    let mut out = Vec::new();
    for _ in 0..10000 {
        if let Some(f) = a.next_frame().unwrap() {
            out.push(f.frame);
        }
        if a.jobs().unwrap().iter().all(|j| j.cycle > 0) {
            break;
        }
    }
    out
}
#[test]
fn out_of_order_duplicate_restart_and_ack() {
    let (tmp, mut a, mut b, input) = setup();
    fs::write(
        input.join("binary.bin"),
        (0..12500).map(|i| (i % 256) as u8).collect::<Vec<_>>(),
    )
    .unwrap();
    fs::write(input.join("empty"), []).unwrap();
    fs::create_dir(input.join("empty-dir")).unwrap();
    let profile = Profile {
        policy: Policy::Once,
        trusted_peers: vec![b.fingerprint()],
        ..Profile::default()
    };
    let job = a.enqueue(vec![input.clone()], true, Some(profile)).unwrap();
    let all = frames(&mut a);
    for f in all.iter().filter(|f| f.kind == Kind::Manifest) {
        b.ingest(&f.encode().unwrap()).unwrap();
    }
    let data: Vec<_> = all.iter().filter(|f| f.kind == Kind::Data).rev().collect();
    for f in data.iter().take(data.len() / 2) {
        b.ingest(&f.encode().unwrap()).unwrap();
        b.ingest(&f.encode().unwrap()).unwrap();
    }
    drop(b);
    let mut b = Engine::open(tmp.path().join("b")).unwrap();
    for f in data.iter().skip(data.len() / 2) {
        b.ingest(&f.encode().unwrap()).unwrap();
    }
    assert_eq!(b.job(job.id).unwrap().status, "complete");
    let out = tmp.path().join("b/received/input/binary.bin");
    assert_eq!(
        fs::read(out).unwrap(),
        fs::read(input.join("binary.bin")).unwrap()
    );
    assert!(tmp.path().join("b/received/input/empty-dir").is_dir());
    for _ in 0..200 {
        if let Some(f) = b.next_frame().unwrap() {
            let _ = a.ingest(&f.text);
        }
    }
    assert_eq!(a.job(job.id).unwrap().status, "confirmed");
}
#[test]
fn frame_ack_drops_rotation_and_missing_restores() {
    let (_tmp, mut a, mut b, input) = setup();
    fs::write(input.join("x"), vec![7; 1800]).unwrap();
    let job = a.enqueue(vec![input.join("x")], false, None).unwrap();
    let all = frames(&mut a);
    for f in all.iter().filter(|f| f.kind == Kind::Manifest) {
        b.ingest(&f.encode().unwrap()).unwrap();
    }
    let first = all
        .iter()
        .find(|f| f.kind == Kind::Data && f.index == 0)
        .unwrap();
    b.ingest(&first.encode().unwrap()).unwrap();
    for _ in 0..8 {
        if let Some(f) = b.next_frame().unwrap() {
            let _ = a.ingest(&f.text);
        }
    }
    for _ in 0..20 {
        if let Some(f) = a.next_frame().unwrap() {
            assert!(!(f.frame.kind == Kind::Data && f.frame.index == 0));
        }
    }
    assert_eq!(b.request_missing(job.id, 1).unwrap(), vec![(1, 2)]);
}
#[test]
fn corrupt_staging_cannot_keep_verified_bitmap() {
    let (tmp, mut a, mut b, input) = setup();
    fs::write(input.join("x"), vec![7; 1800]).unwrap();
    let j = a.enqueue(vec![input.join("x")], false, None).unwrap();
    let all = frames(&mut a);
    for f in all.iter().filter(|f| f.kind == Kind::Manifest) {
        b.ingest(&f.encode().unwrap()).unwrap();
    }
    b.ingest(
        &all.iter()
            .find(|f| f.kind == Kind::Data)
            .unwrap()
            .encode()
            .unwrap(),
    )
    .unwrap();
    drop(b);
    fs::write(
        tmp.path()
            .join("b/staging")
            .join(j.id.to_string())
            .join("1"),
        vec![9; 1800],
    )
    .unwrap();
    let b = Engine::open(tmp.path().join("b")).unwrap();
    assert_eq!(b.inspect(j.id).unwrap().files[0].received_chunks, 0);
}
#[test]
fn conflicting_duplicates_and_stale_receipts_rejected() {
    let (_tmp, mut a, mut b, input) = setup();
    fs::write(input.join("x"), vec![7; 1200]).unwrap();
    a.enqueue(vec![input.join("x")], false, None).unwrap();
    let all = frames(&mut a);
    for f in all.iter().filter(|f| f.kind == Kind::Manifest) {
        b.ingest(&f.encode().unwrap()).unwrap();
    }
    let f = all.iter().find(|f| f.kind == Kind::Data).unwrap();
    b.ingest(&f.encode().unwrap()).unwrap();
    let mut bad = f.clone();
    bad.payload[0] = 8;
    assert!(b.ingest(&bad.encode().unwrap()).is_err());
    let mut receipt = None;
    for _ in 0..8 {
        if let Some(f) = b.next_frame().unwrap()
            && a.ingest(&f.text).is_ok()
        {
            receipt = Some(f.text);
            break;
        }
    }
    assert!(a.ingest(&receipt.unwrap()).is_err());
}
#[test]
fn send_only_loop_is_never_delivery() {
    let (_tmp, mut a, _b, input) = setup();
    fs::write(input.join("x"), [42]).unwrap();
    let p = Profile {
        mode: Mode::Send,
        policy: Policy::Once,
        ..Profile::default()
    };
    let j = a.enqueue(vec![input.join("x")], false, Some(p)).unwrap();
    frames(&mut a);
    assert_eq!(a.job(j.id).unwrap().status, "displayed");
    assert!(
        !a.events(0, 100)
            .unwrap()
            .iter()
            .any(|(_, e)| e.event == "delivery.confirmed")
    );
}
