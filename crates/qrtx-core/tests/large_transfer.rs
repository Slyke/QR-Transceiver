use qrtx_core::{Engine, protocol::*};
use std::fs;
use tempfile::TempDir;

#[test]
fn multi_megabyte_transfer_resumes_sparse_data_and_searches_across_pages() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("large.bin");
    let mut bytes: Vec<u8> = (0..2 * 1024 * 1024 + 37)
        .map(|n| ((n * 31 + n / 251) % 256) as u8)
        .collect();
    let needle = b"search across the 64 KiB page boundary";
    bytes[65530..65530 + needle.len()].copy_from_slice(needle);
    fs::write(&source, &bytes).unwrap();
    let mut sender = Engine::open(temp.path().join("sender")).unwrap();
    let job = sender.enqueue(vec![source], false, None).unwrap();
    assert_eq!(sender.search(job.id, 1, needle, 0).unwrap(), Some(65530));
    let state = temp.path().join("receiver");
    let mut receiver = Engine::open(&state).unwrap();
    let metadata = pack(&job.manifest).unwrap();
    for (index, part) in metadata.chunks(600).enumerate() {
        let mut payload = hex::decode(&job.manifest_hash).unwrap();
        payload.extend(part);
        let frame = Frame {
            session: job.session,
            transfer: job.id,
            kind: Kind::Manifest,
            file: 0,
            index: index as u32,
            total: metadata.len().div_ceil(600) as u32,
            offset: (index * 600) as u64,
            payload,
        };
        receiver.ingest(&frame.encode().unwrap()).unwrap();
    }
    let chunks = job.manifest.files[0].chunks;
    let data = |index: u32| {
        let offset = index as usize * 600;
        Frame {
            session: job.session,
            transfer: job.id,
            kind: Kind::Data,
            file: 1,
            index,
            total: chunks,
            offset: offset as u64,
            payload: bytes[offset..(offset + 600).min(bytes.len())].to_vec(),
        }
        .encode()
        .unwrap()
    };
    for index in (0..chunks).rev().filter(|i| i % 3 != 0) {
        receiver.ingest(&data(index)).unwrap();
    }
    assert!(
        receiver
            .payload(job.id, 1, 0, 16)
            .unwrap()
            .bytes
            .iter()
            .all(Option::is_none)
    );
    assert_eq!(
        receiver.payload(job.id, 1, 600, 16).unwrap().bytes,
        bytes[600..616]
            .iter()
            .copied()
            .map(Some)
            .collect::<Vec<_>>()
    );
    assert_ne!(receiver.job(job.id).unwrap().status, "complete");
    drop(receiver);
    let mut receiver = Engine::open(&state).unwrap();
    for index in (0..chunks).rev().filter(|i| i % 3 == 0) {
        receiver.ingest(&data(index)).unwrap();
    }
    assert_eq!(receiver.job(job.id).unwrap().status, "complete");
    assert_eq!(receiver.search(job.id, 1, needle, 0).unwrap(), Some(65530));
    assert_eq!(fs::read(state.join("received/large.bin")).unwrap(), bytes);
}
