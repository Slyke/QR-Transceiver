use qrtx_android::{MobileEngine, scan_luma};
use qrtx_core::{Engine, Mode, Profile, protocol};
use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;

fn call(e: &mut MobileEngine, method: &str, p: Value) -> Value {
    e.dispatch(method, p).unwrap()
}

#[test]
fn password_bundle_exports_validated_inner_files_and_keeps_scanning_on_retry() {
    let temp = TempDir::new().unwrap();
    let mut phone = MobileEngine::open(&temp.path().join("phone/engine")).unwrap();
    let mut peer = MobileEngine::open(&temp.path().join("peer/engine")).unwrap();
    let inputs = temp.path().join("phone/imports/batch");
    fs::create_dir_all(&inputs).unwrap();
    let source = inputs.join("example.cqs.json.gz");
    fs::write(&source, [0x1f, 0x8b, 0, 255]).unwrap();
    let job = call(
        &mut phone,
        "enqueue",
        json!({"paths":[source],"password":" mobile 🔒 "}),
    );
    for _ in 0..20 {
        let frame = call(&mut phone, "next_frame", json!({}));
        if let Some(text) = frame["text"].as_str() {
            assert_eq!(peer.scan(vec![text.into()])["rejected"], 0);
        }
        if call(&mut peer, "status", json!({}))["transfers"][0]["status"] == "locked" {
            break;
        }
    }
    assert_eq!(
        call(&mut peer, "status", json!({}))["transfers"][0]["passwordNeeded"],
        true
    );
    assert!(
        peer.dispatch("export", json!({"id":job["id"],"file":1}))
            .is_err()
    );
    assert!(
        peer.dispatch(
            "receive_password",
            json!({"id":job["id"],"password":"wrong"})
        )
        .is_err()
    );
    call(
        &mut peer,
        "receive_password",
        json!({"id":job["id"],"password":" mobile 🔒 "}),
    );
    let status = call(&mut peer, "status", json!({}));
    assert_eq!(
        status["transfers"][0]["files"][0]["name"],
        "example.cqs.json.gz"
    );
    let exported = call(&mut peer, "export_batch", json!({"id":job["id"]}));
    assert_eq!(exported.as_array().unwrap().len(), 1);
    assert_eq!(
        fs::read(exported[0]["path"].as_str().unwrap()).unwrap(),
        [0x1f, 0x8b, 0, 255]
    );
    assert!(
        peer.dispatch("export", json!({"id":job["id"],"file":1}))
            .is_err()
    );
}

#[test]
fn android_sends_to_desktop_and_authenticates_receipts_after_restart() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("phone/engine");
    let mut phone = MobileEngine::open(&root).unwrap();
    let mut desktop = Engine::open(temp.path().join("desktop")).unwrap();
    call(
        &mut phone,
        "configure",
        json!({"trusted_peers":[desktop.fingerprint()]}),
    );
    let inputs = temp.path().join("phone/imports/batch");
    fs::create_dir_all(&inputs).unwrap();
    let source = inputs.join("binary.bin");
    let expected: Vec<_> = (0..5700).map(|i| (i % 256) as u8).collect();
    fs::write(&source, &expected).unwrap();
    let job = call(&mut phone, "enqueue", json!({"paths":[source]}));
    let id = job["id"].as_str().unwrap().parse().unwrap();
    for _ in 0..200 {
        let frame = call(&mut phone, "next_frame", json!({}));
        if let Some(text) = frame["text"].as_str() {
            let _ = desktop.ingest(text);
        }
        if desktop.job(id).is_ok_and(|j| j.status == "complete") {
            break;
        }
    }
    assert_eq!(
        fs::read(temp.path().join("desktop/received/binary.bin")).unwrap(),
        expected
    );
    drop(phone);
    let mut phone = MobileEngine::open(&root).unwrap();
    for _ in 0..100 {
        if let Some(frame) = desktop.next_frame().unwrap() {
            let _ = phone.dispatch("ingest", json!({"text":frame.text}));
        }
        if call(&mut phone, "status", json!({}))["transfers"][0]["status"] == "confirmed" {
            break;
        }
    }
    assert_eq!(
        call(&mut phone, "status", json!({}))["transfers"][0]["status"],
        "confirmed"
    );
}

#[test]
fn desktop_to_android_recovers_partial_files_and_exports_only_verified_output() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("phone/engine");
    let mut phone = MobileEngine::open(&root).unwrap();
    let mut desktop = Engine::open(temp.path().join("desktop")).unwrap();
    let input = temp.path().join("desktop-input.bin");
    fs::write(&input, vec![27; 9200]).unwrap();
    let job = desktop.enqueue(vec![input], false, None).unwrap();
    for index in 0..15 {
        if let Some(frame) = desktop.next_frame().unwrap()
            && index % 3 != 0
        {
            let _ = phone.dispatch("ingest", json!({"text":frame.text}));
        }
    }
    let p = json!({"id":job.id,"file":1});
    assert!(phone.dispatch("export", p.clone()).is_err());
    drop(phone);
    let mut phone = MobileEngine::open(&root).unwrap();
    for _ in 0..300 {
        if let Some(frame) = desktop.next_frame().unwrap() {
            let _ = phone.dispatch("ingest", json!({"text":frame.text}));
        }
        if phone.dispatch("export", p.clone()).is_ok() {
            break;
        }
    }
    let path = call(&mut phone, "export", p)["path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(fs::read(path).unwrap(), vec![27; 9200]);
    assert_eq!(
        call(&mut phone, "status", json!({}))["transfers"][0]["status"],
        "complete"
    );
}

#[test]
fn displayed_loops_never_confirm_and_imports_are_confined() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("phone/engine");
    let mut phone = MobileEngine::open(&root).unwrap();
    let outside = temp.path().join("outside");
    fs::write(&outside, b"secret").unwrap();
    assert!(
        phone
            .dispatch("enqueue", json!({"paths":[outside]}))
            .is_err()
    );
    call(&mut phone, "configure", json!({"mode":"send"}));
    let input = temp.path().join("phone/imports/batch");
    fs::create_dir_all(&input).unwrap();
    let source = input.join("a");
    fs::write(&source, b"hello").unwrap();
    call(&mut phone, "enqueue", json!({"paths":[source]}));
    for _ in 0..12 {
        call(&mut phone, "next_frame", json!({}));
    }
    let status = call(&mut phone, "status", json!({}));
    assert_eq!(status["transfers"][0]["status"], "active");
    assert_eq!(status["profile"]["allow_unpaired_ack"], false);
    assert!(
        phone
            .dispatch("configure", json!({"trusted_peers":["bad"]}))
            .is_err()
    );
}

#[test]
fn rendered_android_qr_roundtrips_through_camera_decoder() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("phone/engine");
    let mut phone = MobileEngine::open(&root).unwrap();
    let inputs = temp.path().join("phone/imports/batch");
    fs::create_dir_all(&inputs).unwrap();
    let input = inputs.join("hello.txt");
    fs::write(&input, b"test the native QR raster").unwrap();
    call(&mut phone, "enqueue", json!({"paths":[input]}));
    let frame = call(&mut phone, "next_frame", json!({}));
    let width = frame["width"].as_u64().unwrap() as usize;
    let side = (width + 8) * 5;
    let mut pixels = vec![255; side * side];
    for y in 0..width {
        for x in 0..width {
            if frame["modules"][y * width + x].as_bool().unwrap() {
                for yy in 0..5 {
                    for xx in 0..5 {
                        pixels[((y + 4) * 5 + yy) * side + (x + 4) * 5 + xx] = 0;
                    }
                }
            }
        }
    }
    let texts = scan_luma(pixels, side as u32, side as u32).unwrap();
    assert_eq!(texts, vec![frame["text"].as_str().unwrap()]);
    assert!(scan_luma(vec![0], 4096, 4096).is_err());
    assert!(scan_luma(vec![0], 2, 2).is_err());
}

#[test]
fn untrusted_receipt_cannot_stop_android_sender() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("phone/engine");
    let mut phone = MobileEngine::open(&root).unwrap();
    let mut receiver = Engine::open(temp.path().join("receiver")).unwrap();
    receiver
        .set_profile(Profile {
            mode: Mode::Both,
            ..receiver.profile.clone()
        })
        .unwrap();
    let inputs = temp.path().join("phone/imports/batch");
    fs::create_dir_all(&inputs).unwrap();
    let input = inputs.join("a");
    fs::write(&input, b"hello").unwrap();
    call(&mut phone, "enqueue", json!({"paths":[input]}));
    for _ in 0..12 {
        let frame = call(&mut phone, "next_frame", json!({}));
        if let Some(text) = frame["text"].as_str() {
            assert!(protocol::Frame::decode(text).is_ok());
            let _ = receiver.ingest(text);
        }
    }
    for _ in 0..20 {
        if let Some(frame) = receiver.next_frame().unwrap() {
            let _ = phone.dispatch("ingest", json!({"text":frame.text}));
        }
    }
    assert_ne!(
        call(&mut phone, "status", json!({}))["transfers"][0]["status"],
        "confirmed"
    );
    assert_eq!(
        call(&mut phone, "status", json!({}))["transfers"][0]["status"],
        "active"
    );
}

#[test]
fn removing_old_transfer_keeps_inputs_used_by_restart() {
    let temp = TempDir::new().unwrap();
    let mut phone = MobileEngine::open(&temp.path().join("phone/engine")).unwrap();
    let batch = temp.path().join("phone/imports/batch");
    fs::create_dir_all(&batch).unwrap();
    let source = batch.join("file.txt");
    fs::write(&source, b"Keep me for the replacement").unwrap();
    let id = call(&mut phone, "enqueue", json!({"paths":[source]}))["id"]
        .as_str()
        .unwrap()
        .to_string();
    call(&mut phone, "action", json!({"id":id,"action":"pause"}));
    call(&mut phone, "action", json!({"id":id,"action":"restart"}));
    call(&mut phone, "action", json!({"id":id,"action":"remove"}));
    assert!(source.exists());
    call(&mut phone, "maintain", json!({}));
    let status = call(&mut phone, "status", json!({}));
    let remaining = status["transfers"][0]["id"].as_str().unwrap();
    call(
        &mut phone,
        "action",
        json!({"id":remaining,"action":"cancel"}),
    );
    call(
        &mut phone,
        "action",
        json!({"id":remaining,"action":"remove"}),
    );
    assert!(!batch.exists());
}

#[test]
fn folder_export_includes_empty_directories_and_only_verified_files() {
    let temp = TempDir::new().unwrap();
    let mut phone = MobileEngine::open(&temp.path().join("phone/engine")).unwrap();
    let mut desktop = Engine::open(temp.path().join("desktop")).unwrap();
    let input = temp.path().join("example");
    fs::create_dir_all(input.join("empty")).unwrap();
    fs::write(input.join("file"), b"folder export").unwrap();
    let job = desktop.enqueue(vec![input], true, None).unwrap();
    assert!(
        phone
            .dispatch("export_batch", json!({"id":job.id}))
            .is_err()
    );
    let mut result = None;
    for _ in 0..100 {
        if let Some(frame) = desktop.next_frame().unwrap() {
            let _ = phone.dispatch("ingest", json!({"text":frame.text}));
        }
        if let Ok(batch) = phone.dispatch("export_batch", json!({"id":job.id})) {
            result = Some(batch);
            break;
        }
    }
    let result = result.expect("folder should finish");
    let entries = result.as_array().unwrap();
    assert!(
        entries
            .iter()
            .any(|e| e["name"] == "example/empty" && e["directory"] == true)
    );
    let file = entries
        .iter()
        .find(|e| e["name"] == "example/file")
        .unwrap();
    assert_eq!(
        fs::read(file["path"].as_str().unwrap()).unwrap(),
        b"folder export"
    );
}

#[cfg(unix)]
#[test]
fn android_style_data_directory_alias_does_not_delete_queued_inputs() {
    let temp = TempDir::new().unwrap();
    let private = temp.path().join("data");
    let alias = temp.path().join("user-zero");
    fs::create_dir(&private).unwrap();
    std::os::unix::fs::symlink(&private, &alias).unwrap();
    let mut phone = MobileEngine::open(&alias.join("engine")).unwrap();
    let source = alias.join("imports/batch/hello");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, b"survives an Android data-directory alias").unwrap();
    call(&mut phone, "enqueue", json!({"paths":[source]}));
    drop(phone);
    let mut phone = MobileEngine::open(&alias.join("engine")).unwrap();
    assert!(source.exists());
    call(&mut phone, "maintain", json!({}));
    assert!(call(&mut phone, "next_frame", json!({})).is_object());
}
