# Verification

Automated suites cover binary reconstruction, empty and one-byte files, self-contained static QR, QR image encode/decode, CRC corruption, Unicode/unsafe paths, shuffled and duplicated chunks, interrupted multi-megabyte reception, modified staging, interrupted completion transactions, quotas, failure events, authenticated and unauthenticated receipts, replay rejection, selective retransmission, restart identities, bidirectional scheduling, conflict handling and destination symlink confinement.

Runtime tests exercise permission scopes, outgoing roots, verified-event gating, command stdin, duplicate-action suppression, restart deduplication, webhook retries and stable event IDs. Browser tests use the real Rust HTTP service through the development proxy; they cover queueing, QR display, payload inspection, settings, themes and a narrow viewport.

Run:
```sh
cargo test --locked
cargo test --locked -p qrtx-runtime --features camera
cargo clippy --locked --all-targets -- -D warnings
npm run check
npm run build
npm test
```

## Physical release gate

Physical validation requires two devices or equivalent camera/display equipment and is not replaced by simulation:

1. Linux x86-64 to Linux x86-64, Windows to Linux, and ARM64 Raspberry Pi to a desktop.
2. Send-only optical loop with delayed receiver startup.
3. Both-mode simultaneous transfer, authenticated pairing and ACK-driven frame removal.
4. Unplug/replug each USB webcam; verify reconnect and retained progress.
5. Change exposure/focus where the driver supports them, test mirror correction, rotation, crop and zoom.
6. Test fullscreen/detached output on a second monitor at several pixel sizes and QR correction levels.
7. Restart sender/receiver midway through a multi-file batch and verify every final hash.
8. Test low light, motion, glare, dropped frames, stale receipts, changed source files and disk-full errors.
9. Verify a large file does not expand browser memory proportionally to file size.
10. Check packaged installation and transfer with networking disabled.

Record actual hardware, OS, camera mode, QR profile, pass/fail and observed useful throughput. The repository must not claim these checks passed without physical evidence.

See [the local verification record](verification-record.md) for checks actually run. scripts/smoke.py tests real IPC/REST/MCP processes; scripts/native-smoke.py checks startup of an existing native release build on a graphical Linux session.
