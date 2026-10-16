# Optional password encryption

Ordinary QRTX transfers remain the default. Enable **Encrypt new transfer with a password**, enter a password, then add files or a folder. Desktop and Android support this option. A folder contributes its files and relative paths; empty directories are omitted because the bundle format has no directory records. A bundle must contain at least one file.

The sender snapshots and encrypts once. QR size, timing, inversion, pause/resume and loops reuse that envelope. Encrypted jobs do not automatically follow source changes. To send changed files or restart as a new encrypted bundle, add the files with a password again. Every new bundle gets fresh randomness.

The receiver offers password entry before scanning and an inline prompt when a manifest names a `.qrtxenc` file. Scanning continues while the prompt is visible. Renamed files are recognized by their `QRTXENC1` prefix. The normal outer file SHA-256 must pass before decryption. Wrong passwords leave received ciphertext/chunks intact for retry, including across process restarts. Passwords stay only in process memory; they are never put in profiles, settings, jobs, events or saved UI state. A preentered password lasts until cleared or the engine exits. The sender clears its entry after queueing.

After authentication and complete inner validation, files use existing destination confinement, conflict handling, verification events and Android export confirmation. The outer manifest, file SHA-256 and receipt identities always describe the encrypted file. Nested `.qrtxenc` files or `QRTXENC1` contents are passed through as ordinary inner files, never recursively decrypted. An inner `.cqs.json.gz` file is ordinary binary data; this generic transceiver does not interpret or decompress CQS content.

## Format

Exactly one outer file is created, named `transfer.qrtxenc`. This file format uses **unchanged QRTX/1**; QR framing, manifests, CRC32C, SHA-256, signed receipts and ACK structures are unchanged.

| Offset | Bytes | Meaning |
| --- | --- | --- |
| 0 | 8 | ASCII `QRTXENC1` |
| 8 | 16 | Random salt |
| 24 | 12 | Random nonce |
| 36 | remainder | AES-256-GCM ciphertext followed by its 16-byte tag |

The exact first 36 bytes are GCM associated data. The complete envelope must not exceed **256,000,000 bytes**.

Encode the password as exact UTF-8, without trimming or normalization; accept **1–4096 bytes**. Derive 32 bytes with **Argon2id version 0x13**, memory **65536 KiB**, iterations **3**, parallelism **1**, the 16-byte envelope salt, and no secret or Argon2 associated data. Use these raw bytes directly as the AES-256 key.

The authenticated plaintext is:

1. Eight ASCII bytes `QRTXFIL1`.
2. A big-endian u32 file count, from 1 through 4096.
3. For each file: big-endian u32 filename byte length (1–1024), UTF-8 filename bytes, big-endian u64 data byte length, then exactly that many raw bytes.

Apply QRTX `validate_path` to every filename. Reject duplicates (using QRTX's case-insensitive comparison), invalid UTF-8, truncation, overflowing lengths and trailing bytes. Authenticate the full GCM tag and validate every inner record before extracting any file. Password envelopes cannot share an outer manifest with any other entry. To catch renamed envelopes even after corrupt-chunk retransmission, the receiver verifies all outer file hashes before promoting files in a multi-file transfer. Frame ACKs continue as chunks arrive; file/batch ACKs retain their outer identities and hashes.

## API and CLI

`enqueue` accepts an optional string `password`; omission or `null` keeps ordinary transfers. An empty string is an error. The permission remains `send`, with the same outgoing-root restrictions.

`receive_password` accepts `{ "password": "exact UTF-8", "id": "optional-transfer-uuid" }` and requires the `receive` scope. Omit `id` to set a password before scanning or retry pending transfers. Use `password: null` to clear it; without `id`, this clears all in-memory receive passwords. Neither method echoes the password.

Jobs expose `password_bundle`, `password_needed`, and authenticated `bundle_files`. A fully received envelope awaiting a usable password has status `locked`. Its transport manifest stays unchanged. Local inner file IDs start at 4097 and skip the outer file ID to avoid collisions. After completion, `transfer.files` includes the inner output paths. Inner verification events use inner IDs; optical ACKs always use outer IDs and hashes. Ordinary transfer responses remain compatible with these additive fields.

CLI `send --password-stdin FILE...` and `receive --password-stdin` read exact UTF-8 stdin bytes, **including any newline**. Supply bytes from a secret provider through a pipe rather than putting passwords in command arguments or shell history. `password [TRANSFER_ID]` sets/retries the receive password from stdin; `password --clear` clears it. Plain `send` and `receive` retain their previous behavior.

The independent fixture is `tests/fixtures/qrtxenc1.json`, generated with libargon2 and Python cryptography by the adjacent script. Its fixed salt, nonce and public password are only test data.
