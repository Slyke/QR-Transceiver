use qrtx_core::{DisplaySettings, Engine, Mode, Policy, Profile};
use tempfile::TempDir;

#[test]
fn live_presentation_preserves_wire_layout_and_has_no_empty_loop_slots() {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("image.bin");
    std::fs::write(&input, vec![42; 2048]).unwrap();
    let root = tmp.path().join("state");
    let mut engine = Engine::open(&root).unwrap();
    engine
        .set_profile(Profile {
            mode: Mode::Send,
            policy: Policy::Loop,
            chunk_size: 256,
            ..Profile::default()
        })
        .unwrap();
    let job = engine.enqueue(vec![input], false, None).unwrap();
    let first = engine.next_frame().unwrap().unwrap();
    engine
        .update_display_settings(DisplaySettings {
            frame_ms: Some(100),
            qr_pixels: Some(900),
            quiet_zone: Some(6),
            inverted: Some(true),
        })
        .unwrap();
    let redrawn = engine.redraw_frame(&first.text, first.cycle).unwrap();
    assert_eq!(redrawn.text, first.text);
    assert_eq!(redrawn.duration_ms, 100);
    assert_ne!(redrawn.svg, first.svg);
    assert_eq!(
        engine.job(job.id).unwrap().frames,
        1,
        "redrawing must not advance transmission"
    );
    for _ in 0..40 {
        let frame = engine
            .next_frame()
            .unwrap()
            .expect("loop must never emit an idle slot");
        assert_eq!(frame.duration_ms, 100);
    }
    let updated = engine.job(job.id).unwrap();
    assert!(updated.cycle >= 2);
    let history = engine.recent_events(1).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].1.event, "transfer.started");
    assert!(
        engine
            .events(0, 100)
            .unwrap()
            .iter()
            .any(|(_, event)| event.event == "transfer.loop_completed"),
        "automation still receives loop events"
    );
    assert_eq!(updated.manifest_hash, job.manifest_hash);
    assert_eq!(updated.manifest.chunk_size, 256);
    assert_eq!(updated.session, job.session);
    assert!(
        engine
            .update_display_settings(DisplaySettings {
                frame_ms: Some(0),
                ..Default::default()
            })
            .is_err()
    );
    assert_eq!(engine.profile.frame_ms, 100);
    assert_eq!(engine.job(job.id).unwrap().profile.frame_ms, 100);
    drop(engine);
    let engine = Engine::open(root).unwrap();
    assert_eq!(engine.profile.frame_ms, 100);
    assert_eq!(engine.job(job.id).unwrap().profile.quiet_zone, 6);
}
