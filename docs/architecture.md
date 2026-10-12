# Architecture

The workspace separates transport from platform integration:

- `qrtx-protocol`: bounded binary framing, Base45, CRC32C, CBOR manifests/controls, SHA-256, Ed25519 receipts, QR encoding and image decoding.
- `qrtx-core`: persistent queues, source snapshots, staging files, reconstruction, safe destination commits, ACK scheduling and inspection.
- `qrtx-runtime`: USB-camera adapters, local IPC, optional HTTP API, event dispatch, watchers and the official Rust MCP SDK.
- `qrtransceiver`: service/CLI/MCP executable and a lossy transport simulator.
- `qrtx-desktop`: Tauri host in `desktop/src-tauri`; Svelte renders state and a bounded window of file bytes.
- `qrtx-android`: JNI bridge over the shared offline engine, called by the Kotlin/Compose app in `android/`.

SQLite is in WAL mode with FULL synchronous commits. File payloads live on disk. An exclusive state-directory lock prevents two engines owning a queue. A frame ACK is emitted only after staging data is synced and its chunk record is committed. Restart verifies every persisted chunk against its stored hash, then checks committed output hashes before retaining verified status.

Outgoing files are immutable disk snapshots. Snapshot creation checks source length, modification time and a second content hash. Background maintenance detects changed source metadata and queues a new transfer identity. Incoming file names have a portable strict syntax; capability-relative destination operations prevent path escape, including symlinks.

The display scheduler reserves at least three out of four available slots for data when a send job exists. The remaining slots cycle pending controls. Idle Both-mode endpoints dedicate their display to control traffic. Manifests repeat periodically and remain available when frame ACKs have suppressed all data.

## Platform boundary

ARM64 Linux uses the same Rust code as x86-64 Linux. USB cameras use Nokhwa's V4L2 backend on Linux and Media Foundation on Windows. MJPEG is decoded with the Rust image library; no FFmpeg or OpenCV is required. Capture and preview run independently from QR scanning, with a single queued frame bounding memory and delay. The frontend polls bounded preview images separately from transfer state and shows measured capture/scan rates.

Android uses CameraX image analysis and preview, Kotlin document-provider streams, and a Compose UI. Only the shared core/protocol are linked into its JNI library. The bridge owns safe numeric engine handles, validates camera buffer bounds, confines imports to private storage, and exports only verified private outputs. Desktop IPC, automation, USB capture, and MCP are excluded. See `android.md` for Android behavior and validation. iOS remains future work.

## Style-guide adaptations

- Strict versioned JSON is persisted as required by the application specification. The local service configuration accepts JSON5 and whole-value environment references.
- MCP uses local stdio and the Rust SDK. The guide's HTTP sidecar, Kubernetes and LAN-server defaults do not apply to this offline desktop application.
- Structured JSON logs use tracing, stable event/error keys, version/build metadata and stderr. No network logging sinks are enabled.
- Screen layout follows the application's transfer workflow instead of reproducing the style-guide demonstration page.
- Rust uses rustfmt and Clippy; TypeScript uses object-shaped arguments and arrow functions. Shared CSS variables preserve the guide's semantic tones.


## Presentation preferences

Send controls apply timing, quiet-zone and color-inversion changes to active and paused outgoing jobs without changing chunk boundaries, transfer IDs or manifest hashes. The detached display fits whole QR modules to its window and keeps the remaining area dark. Its initial frame is synchronized when the window opens, including while transmission is paused.

Payload rows use compact fixed byte cells. Auto mode chooses multiples of 16 that fit the available width; users may select 16–128 bytes per row and hide ASCII. Text view uses the same byte-addressed rows, with visible markers for line breaks/tabs. Manual scrolling disengages Follow active bytes. Width, ASCII visibility and section ordering persist in local WebView storage, separately for each state directory. Section order is independent for Dashboard, Send, Receive, Settings and Automation.

History selects recent significant events before applying its limit, excluding routine loop completion entries. The complete event stream remains available to automation consumers.
