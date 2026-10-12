use qrtx_protocol::*;
use uuid::Uuid;
fn main() -> anyhow::Result<()> {
    let session = Uuid::parse_str("11111111-2222-4333-8444-555555555555")?;
    let transfer = Uuid::parse_str("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee")?;
    let data = vec![0, 1, 2, 127, 128, 255];
    let manifest = Manifest {
        version: 1,
        transfer,
        chunk_size: 600,
        files: vec![FileEntry {
            id: 1,
            path: "example.bin".into(),
            size: data.len() as u64,
            sha256: hash(&data),
            chunks: 1,
            directory: false,
        }],
    };
    let manifest_bytes = pack(&manifest)?;
    let frame = Frame {
        session,
        transfer,
        kind: Kind::Data,
        file: 1,
        index: 0,
        total: 1,
        offset: 0,
        payload: data,
    };
    let key = Identity::from_bytes(&[7; 32]);
    let receipt = Receipt::new(
        session,
        transfer,
        1,
        Control::AckFile {
            file: 1,
            sha256: manifest.files[0].sha256.clone(),
        },
        &key,
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"session":session,"transfer":transfer,"manifest":manifest,"manifestCborHex":hex::encode(&manifest_bytes),"manifestSha256":hash(&manifest_bytes),"dataFrameText":frame.encode()?,"frameCrc32c":format!("{:08x}",wire_checksum(&frame.encode()?)?),"receiptCborHex":hex::encode(pack(&receipt)?),"publicKeyHex":hex::encode(key.verifying_key().to_bytes()),"peerFingerprint":hash(&key.verifying_key().to_bytes()),"testPrivateKeyHex":"07 repeated 32 times; TEST ONLY"})
        )?
    );
    Ok(())
}
