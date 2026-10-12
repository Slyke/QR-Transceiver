use qrtx_core::{Engine, protocol::Kind};
use std::fs;
use tempfile::TempDir;

#[test]
fn promoted_file_recovers_after_completion_transaction_fails() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("crash.bin");
    fs::write(&source, b"durable").unwrap();
    let mut sender = Engine::open(temp.path().join("sender")).unwrap();
    let state = temp.path().join("receiver");
    let mut receiver = Engine::open(&state).unwrap();
    let job = sender.enqueue(vec![source], false, None).unwrap();
    let database = rusqlite::Connection::open(state.join("state.sqlite3")).unwrap();
    database.execute_batch("CREATE TRIGGER fail_verified BEFORE INSERT ON events WHEN NEW.body LIKE '%file.verified%' BEGIN SELECT RAISE(FAIL,'simulated storage failure'); END;").unwrap();
    let frame = sender.next_frame().unwrap().unwrap();
    assert_eq!(frame.frame.kind, Kind::Bundle);
    assert!(receiver.ingest(&frame.text).is_err());
    assert_eq!(
        fs::read(state.join("received/crash.bin")).unwrap(),
        b"durable"
    );
    assert!(receiver.job(job.id).unwrap().delivered_files.is_empty());
    assert!(
        !receiver
            .events(0, 100)
            .unwrap()
            .iter()
            .any(|(_, e)| e.event == "file.verified")
    );
    drop(receiver);
    database
        .execute_batch("DROP TRIGGER fail_verified")
        .unwrap();
    let receiver = Engine::open(&state).unwrap();
    assert_eq!(receiver.job(job.id).unwrap().status, "complete");
    assert!(!state.join("received/crash.bin.received-1").exists());
    assert_eq!(
        receiver
            .events(0, 100)
            .unwrap()
            .iter()
            .filter(|(_, e)| e.event == "file.verified")
            .count(),
        1
    );
    drop(receiver);
    let receiver = Engine::open(&state).unwrap();
    assert_eq!(
        receiver
            .events(0, 100)
            .unwrap()
            .iter()
            .filter(|(_, e)| e.event == "file.verified")
            .count(),
        1
    );
}

#[test]
fn receive_quota_counts_completed_staging_and_removal_releases_it() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("quota.bin");
    fs::write(&source, vec![7; 60]).unwrap();
    let mut sender = Engine::open(temp.path().join("sender")).unwrap();
    let state = temp.path().join("receiver");
    let mut receiver = Engine::open(&state).unwrap();
    let mut profile = receiver.profile.clone();
    profile.max_receive_bytes = 100;
    receiver.set_profile(profile).unwrap();
    let first = sender.enqueue(vec![source.clone()], false, None).unwrap();
    receiver
        .ingest(&sender.next_frame().unwrap().unwrap().text)
        .unwrap();
    sender.change_job(first.id, "cancel", None).unwrap();
    let second = sender.enqueue(vec![source], false, None).unwrap();
    let frame = sender.next_frame().unwrap().unwrap();
    assert!(
        receiver
            .ingest(&frame.text)
            .unwrap_err()
            .to_string()
            .contains("quota")
    );
    receiver.change_job(first.id, "remove", None).unwrap();
    assert!(!state.join("staging").join(first.id.to_string()).exists());
    assert!(state.join("received/quota.bin").exists());
    receiver.ingest(&frame.text).unwrap();
    assert_eq!(receiver.job(second.id).unwrap().status, "complete");
}

#[test]
fn corrupt_file_hash_resets_progress_before_retransmission() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("retry.bin");
    fs::write(&source, vec![7; 1200]).unwrap();
    let mut sender = Engine::open(temp.path().join("sender")).unwrap();
    let mut receiver = Engine::open(temp.path().join("receiver")).unwrap();
    let job = sender.enqueue(vec![source], false, None).unwrap();
    let mut frames = Vec::new();
    while frames.len() < 2 {
        if let Some(frame) = sender.next_frame().unwrap() {
            if frame.frame.kind == Kind::Data {
                frames.push(frame.frame);
            } else {
                receiver.ingest(&frame.text).unwrap();
            }
        }
    }
    let mut wrong = frames[0].clone();
    wrong.payload[0] ^= 1;
    receiver.ingest(&wrong.encode().unwrap()).unwrap();
    assert!(receiver.ingest(&frames[1].encode().unwrap()).is_err());
    assert_eq!(receiver.job(job.id).unwrap().bytes, 0);
    for frame in frames {
        receiver.ingest(&frame.encode().unwrap()).unwrap();
    }
    assert_eq!(receiver.job(job.id).unwrap().bytes, 1200);
    assert_eq!(receiver.job(job.id).unwrap().status, "complete");
}

#[test]
fn local_transmission_failure_emits_failure_events() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("gone.bin");
    fs::write(&source, b"snapshot").unwrap();
    let state = temp.path().join("sender");
    let mut sender = Engine::open(&state).unwrap();
    let job = sender.enqueue(vec![source], false, None).unwrap();
    fs::remove_file(state.join("spool").join(job.id.to_string()).join("1")).unwrap();
    assert!(sender.next_frame().is_err());
    assert_eq!(sender.job(job.id).unwrap().status, "failed");
    let events = sender.events(0, 100).unwrap();
    assert!(events.iter().any(|(_, e)| e.event == "transfer.failed"));
    assert!(events.iter().any(|(_, e)| e.event == "delivery.failed"));
    assert!(!events.iter().any(|(_, e)| e.event == "delivery.confirmed"));
}

#[test]
fn receiver_pause_resume_and_cancel_control_the_paired_sender() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("control.bin");
    fs::write(&source, vec![9; 1800]).unwrap();
    let mut sender = Engine::open(temp.path().join("sender")).unwrap();
    let mut receiver = Engine::open(temp.path().join("receiver")).unwrap();
    let mut profile = sender.profile.clone();
    profile.trusted_peers.push(receiver.fingerprint());
    let job = sender.enqueue(vec![source], false, Some(profile)).unwrap();
    while receiver.job(job.id).is_err() {
        if let Some(frame) = sender.next_frame().unwrap() {
            receiver.ingest(&frame.text).unwrap();
        }
    }
    for (action, expected) in [
        ("pause", "paused"),
        ("resume", "active"),
        ("cancel", "cancelled"),
    ] {
        receiver.change_job(job.id, action, None).unwrap();
        for _ in 0..10 {
            if let Some(frame) = receiver.next_frame().unwrap() {
                let _ = sender.ingest(&frame.text);
            }
            if sender.job(job.id).unwrap().status == expected {
                break;
            }
        }
        assert_eq!(sender.job(job.id).unwrap().status, expected);
    }
}
