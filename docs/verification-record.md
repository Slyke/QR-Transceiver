# Local verification record

Run on 2026-10-10 (America/Vancouver) in Ubuntu 22.04 under WSL2, x86-64, using Rust 1.98.1 and Node 24.

| Check | Result |
| --- | --- |
| Rust protocol, transfer, persistence, security and automation suites | 24 tests passed |
| Camera-enabled runtime tests | Passed |
| Clippy across the workspace, desktop and test targets, with warnings denied | Passed |
| Svelte/TypeScript checks | No errors or warnings |
| Production frontend | Built |
| Browser workflow against the real Rust engine | 2 tests passed |
| IPC, REST authentication and official MCP SDK process tests | Passed |
| Native GTK/WebKit GUI startup and shared IPC engine | Passed |
| Native display enumeration under WSLg | Passed; 4 displays reported |
| Linux x86-64 desktop and USB-camera-enabled CLI | Release builds produced |
| Linux Debian installer | Produced |
| ARM64 protocol/core cross-build | Passed |

Coverage includes byte-exact multi-megabyte reconstruction, sparse and reversed delivery, interrupted reception, damaged staging, CRC rejection, static QR image decoding, signed receipts, unpaired claims, replay rejection, frame ACK suppression, selective retransmission, file/batch completion, two-way scheduling, safe destination conflicts, symlink confinement, quota enforcement, failure events, and recovery after file promotion succeeds but the completion transaction fails.

The browser receive test verifies visible missing bytes, out-of-order updates, changing received-set digests, and final hash-verified completion. The automation tests verify no completion commands before verification, literal filename handling, stable webhook retry IDs, and no duplicate action execution after repeated frames or restarts.

Actual artifact hashes are in ../artifacts/SHA256SUMS. Startup logs include a deterministic source fingerprint; it is distinct from each packaged file's SHA-256.

## Still requiring target hardware

Native Raspberry Pi desktop packaging, Windows installer installation, unplug/reconnect recovery, and two-device end-to-end transfers still require target-hardware verification. Native Windows builds and local Logitech webcam capture were subsequently checked below. The checked-in CI workflow supplies native Windows, Linux x86-64 and Linux ARM64 build jobs; it has not been run on a hosted CI service here.

Mobile UI and camera adapters are future work. The protocol and core have no Tauri dependency. Calibration/adaptive transport, fountain coding and encryption remain optional future enhancements.

Follow testing.md's physical release checklist before describing this as hardware-validated.


## Windows and UI follow-up — 2026-10-11

Built the native Windows x64 desktop executable, NSIS installer, and camera-enabled CLI using Rust 1.98.1, Node 24 and Visual Studio 2022. Two desktop instances use the same executable with independent state directories. Port arguments alone leave MCP off; native MCP isolation/authorization tests and persisted enablement tests passed (3 tests).

The Logitech C920e is camera index 1 on this workstation. Capture and scan measurements were approximately 15.1 FPS, with an observed scan duration of 12.4 ms. The previous frontend refreshed the camera only once per second. Preview capture and UI polling now run at up to 12.5 FPS, and QR scanning uses a separate worker with one queued frame. Device-control enumeration is cached rather than repeated on every preview.

The Windows receiver reports the 2,048-byte test transfer complete. The committed `webcam-2k.png` and original both hash to `43e747e1156a89b5f4bdd1d60f74338baf270b0635ae2f287928160b7adbfe02`. This verifies the received file; it is not a throughput or reconnect benchmark.

Four browser regressions pass against the real Rust engine. They cover queued-transfer timing changes while transmitting, uninterrupted loop boundaries, a dark detached display that grows and shrinks with its window, immediate paused-frame redraw, configurable/persistent byte widths and ASCII visibility, individual text-row byte addresses, stable manual scrolling, saved section and tab order, keyboard tab navigation, default-collapsed send controls, and existing send/receive verification workflows. Svelte reports zero warnings/errors; production frontend and Clippy checks pass. The protocol/core still cross-check for aarch64-unknown-linux-gnu.

The Rust suites plus the added display/history and MCP settings regressions passed (28 tests total across full and targeted runs). Linux release artifacts above predate this UI follow-up; the Windows files were rebuilt. Native UI automation was stopped when the user pressed Escape; remaining UI checks used Playwright and engine/CLI interfaces.

Final Windows desktop source fingerprint: b7683437451759f7. Executable/CLI/installer SHA-256 values are recorded in artifacts/windows-x64-SHA256SUMS. Final native capture/scan samples remained approximately 15.15 FPS without a capture error.
