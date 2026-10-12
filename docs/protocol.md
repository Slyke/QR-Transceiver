# QRTX/1 wire protocol

All integers in the frame header are unsigned big-endian. A QR contains the ASCII prefix `QRTX/1:` followed by RFC 9285 Base45 of the binary packet. Base45 whitespace is significant: do not trim decoded QR text.

| Byte offset | Width | Field |
| --- | --- | --- |
| 0 | 1 | Version, exactly 1 |
| 1 | 1 | Kind: 1 manifest, 2 data, 3 control, 4 static bundle |
| 2 | 16 | Session UUID bytes |
| 18 | 16 | Transfer UUID bytes |
| 34 | 4 | File ID; 0 for manifest/control |
| 38 | 4 | Zero-based chunk index |
| 42 | 4 | Total chunks |
| 46 | 8 | Original file byte offset; manifest fragment offset |
| 54 | 2 | Payload byte length |
| 56 | variable | Payload |
| final 4 | 4 | CRC32C of every preceding packet byte |

Bounds are checked before session lookup or file writes. Text is at most 4096 bytes, payload at most 1200 bytes, chunk count at most 4,000,000, manifest at most 1 MiB, files per manifest at most 4096. A batch is limited to 64 GiB in protocol metadata and a smaller configurable local receive quota (16 GiB default). At most 16 incomplete manifests/reception sessions are accepted. The local receive quota counts completed staging retained for inspection; removing an inactive queue entry releases that staging without deleting committed output. Control ranges have at most 32 inclusive pairs and expand to at most 4096 chunks.

## Manifest and data

Manifest payloads begin with the 32 raw bytes of the complete manifest SHA-256, followed by a CBOR fragment. Concatenate fragments in chunk-index order and verify that hash before parsing the manifest. Repeat manifests so late receivers can join. Data preceding a complete manifest is discarded; subsequent loops recover it.

The CBOR manifest is a map in this serialization order:

`version, transfer, chunk_size, files`

Each file map uses:

`id, path, size, sha256, chunks, directory`

UUIDs are CBOR byte strings (16 bytes); hashes are lower-case hexadecimal text. Other integers use their shortest CBOR representation. Empty files have one empty data chunk and the SHA-256 of empty bytes. Directories have zero chunks and zero size. Chunk indices, totals, offsets and lengths must agree exactly with the manifest. Identical duplicate chunks are ignored; conflicting duplicates are rejected.

A static bundle uses kind 4 and carries a CBOR array `[manifest_sha256, manifest_cbor_bytes, file_bytes]`. The byte vectors in this array are encoded as CBOR arrays of unsigned bytes. It must contain one non-directory file with one chunk. The packet carries that file's ID, index 0, total 1 and offset 0. This permits small transfers with one stationary QR.

## Control and signatures

A control payload is a CBOR map in this order:

`sequence, body, public_key, signature`

The key and signature are CBOR arrays of bytes (32 and 64 bytes). Body variants are externally tagged CBOR maps for variants with fields, or CBOR text for unit variants:

- `Hello`, `Ready`, `Pause`, `Resume`, `Cancel`
- `AckFrame: {file, ranges}`
- `AckFile: {file, sha256}`
- `AckBatch: {manifest_hash}`
- `MissingChunks: {file, ranges}`
- `Error: {message}`
- `Complete: {manifest_hash}`

Ranges are inclusive `[start,end]` arrays. Fields above are serialized in the displayed order. Protocol control names correspond to the specification's HELLO, READY, ACK_FRAME, ACK_FILE, ACK_BATCH, MISSING_CHUNKS, PAUSE, RESUME, CANCEL, ERROR and COMPLETE.

Ed25519 signs the CBOR array:

`["QRTX/1 receipt", session_uuid_bytes, transfer_uuid_bytes, sequence, body]`

The verifier reconstructs this typed serialization before checking the signature. Sequence is a positive monotonically increasing integer no larger than 2^63−1. The receiver persists its sequence counter; the sender persists the highest accepted sequence for each public-key fingerprint/session/transfer. Older or repeated sequences are rejected.

A peer fingerprint is SHA-256 of its 32 public-key bytes. A valid signature from an unknown key remains an unpaired claim. A configured trusted fingerprint is necessary for confirmed-delivery events and remote pause/resume/cancel.

File ACK hashes must match the original manifest. Batch receipts must match the full manifest hash. Data ACKs suppress acknowledged chunks; missing requests reinsert selected chunks. File/batch ACKs are generated only after full hashing and successful commit.

## Received-set digest

This is distinct from full-file SHA-256. Hash the ASCII bytes `QRTX/1 received-set` followed by a NUL byte, then, for each durable received chunk in ascending index order:

- chunk index: 4-byte big-endian
- original offset: 8-byte big-endian
- byte length: 4-byte big-endian
- original chunk bytes

The digest changes as the received set changes. It is never described as a partial original-file hash.

See the checked-in vectors for concrete CBOR, QR text, CRC, key fingerprint and receipt bytes. The vector's fixed private key is only a test fixture.
