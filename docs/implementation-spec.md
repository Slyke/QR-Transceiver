# QR Transceiver — Implementation Specification

**Version:** 0.1  
**Status:** Initial implementation specification  
**Technology:** Tauri 2, Svelte 5, TypeScript, Rust  
**Primary platform:** Linux desktop  
**Secondary platform:** Windows  
**Architecture:** One application with sender, receiver, and bidirectional functionality

---

# 1. Project Overview

Build a standalone desktop application capable of transferring arbitrary files between two computers using optical communication.

Each computer runs the same application.

Files are encoded into static or animated QR codes displayed on a monitor and decoded using a webcam attached to the receiving computer.

The application must support fully air-gapped environments without requiring internet connectivity, LAN connectivity, USB storage transfers, cloud services, or external accounts.

The application must also provide local automation capabilities through an API, MCP server, webhooks, command-line interface, and completion-triggered commands.

## Fundamental requirements

- Transfer arbitrary binary files without modification.
- Support individual files, multiple files, and directories.
- Preserve file contents exactly.
- Use QR codes for the communication channel.
- Support operation without any reverse communication channel.
- Optionally support bidirectional optical communication.
- Recover from missing, duplicate, corrupt, and out-of-order frames.
- Display the full transmitted and received payloads.
- Visually indicate which bytes are being transmitted or have been received.
- Provide transfer progress, checksums, and estimated completion times.
- Support unattended and automated transfers.
- Keep the transport protocol independent of file formats and applications.
- Never execute received files automatically.

The same installation must be able to perform all supported roles.

---

# 2. Application Modes

Provide three operating modes, selectable from the main window.

## 2.1 Send Mode

Transmit one or more files through QR codes.

A webcam is not required.

Capabilities:

- Select files or directories.
- Queue multiple transfer jobs.
- Preview the outgoing payload.
- Display static or animated QR codes.
- Loop through transmission frames.
- Configure timing, size, and error correction.
- Pause, resume, restart, or cancel.
- Display transmission statistics.
- Optionally repeat until manually stopped.

In Send Mode, the sender cannot determine whether the remote computer has received the transfer successfully.

Completing a transmission loop must not be reported as confirmed delivery.

## 2.2 Receive Mode

Receive files using a webcam.

A QR output display is not required.

Capabilities:

- Select a webcam.
- Display a live camera preview.
- Detect compatible QR codes.
- Automatically associate incoming frames with transfer sessions.
- Reassemble frames regardless of arrival order.
- Track missing and corrupted chunks.
- Display incoming payload data as it arrives.
- Verify file integrity.
- Save reconstructed files.
- Trigger completion events.

Receive Mode must support multiple incoming files within the same batch.

## 2.3 Both Mode

Enable sending and receiving simultaneously.

Each computer can independently send its own files while also receiving files from the other computer.

Both Mode additionally enables bidirectional protocol control.

If an instance has no outgoing user file, it may use its QR display to transmit protocol-control messages.

Examples:

- Transfer acknowledgment.
- Complete-file acknowledgment.
- Batch acknowledgment.
- Missing-chunk requests.
- Pause/resume signals.
- Receiver readiness.
- Transfer errors.
- Requested retransmission.
- Optional transmission-speed recommendations.

When the receiver has reconstructed and verified 100% of the file, it should display a completion acknowledgment containing the transfer identity and verified hash.

The sender's webcam scans this control QR.

The sender can then:

- Stop transmitting.
- Mark delivery as confirmed, if the acknowledgment is authenticated.
- Proceed to the next file.
- Proceed to the next batch.
- Run a configured completion action.

Automatic stopping must be configurable.

### Simultaneous file and control transmission

Both Mode may need to display outgoing data and control QR frames using the same screen.

Implement a frame scheduler that multiplexes:

1. Outgoing file frames.
2. Acknowledgment frames.
3. Retransmission requests.
4. Other protocol-control messages.

Control frames receive priority where needed, but neither direction should permanently starve the other.

When there is no outgoing data, the QR display may remain dedicated to protocol control.

---

# 3. File and Directory Transfers

## 3.1 Supported inputs

Allow users to send:

- A single file.
- Multiple selected files.
- An entire directory.
- A directory recursively.
- A configured outgoing directory.
- Files submitted through the API, CLI, or MCP server.

All file types must be supported, including binary files, JSON, images, archives, executables, and other arbitrary content.

File extension must not affect transfer compatibility.

## 3.2 File queues

Maintain a persistent transfer queue.

Each entry contains:

- Job ID.
- Source path.
- Transfer type.
- File count.
- Total byte count.
- Current status.
- Priority.
- Created timestamp.
- Progress information.
- Retry count.
- Transfer settings/profile.

Allow users to reorder, remove, pause, and resume queued items.

The queue should survive application restarts.

## 3.3 Directory transmission

When sending a directory:

- Enumerate the selected directory according to configured recursion rules.
- Preserve relative paths.
- Include filenames, sizes, and hashes in a transfer manifest.
- Preserve empty directories where supported.
- Retain the file hierarchy during reconstruction.
- Track completion independently for each file.

Do not follow symbolic links by default.

Never allow incoming paths to escape the configured destination directory.

## 3.4 Transmission policies

Support:

**Once:** Send each file once, then stop. This does not imply successful reception.

**Loop until stopped:** Repeatedly transmit the selected files.

**Loop until acknowledged:** Repeat missing or outstanding data until the receiver confirms successful reconstruction.

**Directory cycle:** Repeatedly transmit the directory contents.

**Watch directory:** Monitor a configured folder and automatically enqueue new or updated files.

Directory watching must wait until a file is stable and no longer being written before transferring it.

Files changed during transfer should be detected and treated as a new version rather than silently producing inconsistent output.

## 3.5 Receive destinations

Support:

- User-selected output folder.
- Default receive directory.
- Per-profile directories.
- Per-transfer destination selection.
- Optional original directory structure.

For filename conflicts, support rename, skip-if-identical, or explicit overwrite policies.

Never overwrite an existing file silently by default.

Write incomplete files to a staging area and atomically promote them to the destination only after successful verification.

---

# 4. QR Transfer Protocol

Create a documented, versioned, transport-only protocol.

Working identifier: `QRTX/1`.

The protocol must not depend on particular application-level file types.

## 4.1 Encoding

Support:

- Static QR codes for payloads that fit into one frame.
- Animated QR sequences for larger transfers.
- Arbitrary binary content.
- Out-of-order frame decoding.
- Duplicate-frame rejection.
- Full-file reconstruction.

Use a QR-compatible binary-to-text encoding such as Base45 for the initial implementation.

A protocol-compatible raw binary QR encoding may be added as an optimization if scanner interoperability is verified.

Use a compact structured packet format, such as canonical CBOR internally, to reduce overhead.

Do not require JSON inside every QR frame.

JSON remains suitable for application configuration, manifests exposed through APIs, and debugging.

## 4.2 Frame structure

Each frame must identify its purpose and corresponding session.

Required logical fields:

| Field | Purpose |
|---|---|
| Protocol version | Decoder compatibility |
| Session ID | Identifies the current communication session |
| Transfer ID | Identifies the transfer job |
| Frame type | Manifest, data, control, acknowledgment, etc. |
| File ID | Identifies the file |
| Chunk index | Placement within the file |
| Total chunks | Total expected chunks |
| Byte offset | Original file offset |
| Payload length | Data size in this frame |
| Payload | Encoded file bytes or control message |
| CRC32C | Detects frame corruption |

Compact numeric field identifiers may be used in the actual wire format.

The decoder must enforce strict bounds before allocating memory or accepting a frame.

## 4.3 Manifest

A transfer manifest describes the files being transmitted.

Example logical representation:

```json
{
  "protocol": "QRTX",
  "version": 1,
  "transferId": "uuid",
  "files": [
    {
      "id": "file-001",
      "path": "requests/transaction.json",
      "size": 2048,
      "sha256": "...",
      "chunks": 5
    },
    {
      "id": "file-002",
      "path": "notes/readme.txt",
      "size": 150,
      "sha256": "...",
      "chunks": 1
    }
  ]
}
```

The actual manifest may be binary-encoded and fragmented across multiple QR frames.

It should be repeated periodically so a receiver joining midway through a transmission can recover the necessary metadata.

The manifest itself must have an integrity hash.

## 4.4 Chunk processing

Files are split into numbered chunks.

The receiver must:

1. Identify the transfer session.
2. Identify the corresponding file.
3. Validate the incoming frame.
4. Determine the chunk's file offset.
5. Write verified chunk data to staging storage.
6. Update the received-chunk bitmap.
7. Update the payload viewer.
8. Recalculate relevant progress and integrity indicators.
9. Ignore identical duplicate chunks.
10. Reject conflicting duplicate chunks.

Chunks may arrive in any order.

The receiver must never assume sequential arrival.

## 4.5 Error recovery

Initial implementation:

- Indexed chunks.
- Repeated transmission cycles.
- Per-frame checksums.
- Receiver-side missing-chunk tracking.
- Selective retransmission when reverse communication is available.

Later enhancement:

- Fountain coding or another proven cross-frame forward-error-correction mechanism.

Fountain coding should not replace ordinary QR error correction; they solve different problems.

If fountain coding is implemented, the payload viewer must still indicate the original byte ranges represented or recovered by each coded frame.

Use an established implementation rather than inventing an untested error-correction algorithm.

---

# 5. Bidirectional Control Protocol

Both Mode requires a control channel using the reverse optical path.

## 5.1 Control messages

Support at least:

- `HELLO`
- `READY`
- `ACK_FRAME`
- `ACK_FILE`
- `ACK_BATCH`
- `MISSING_CHUNKS`
- `PAUSE`
- `RESUME`
- `CANCEL`
- `ERROR`
- `COMPLETE`

An implementation may combine redundant acknowledgment types where appropriate.

Every control message must identify its transfer session and have a monotonically increasing sequence number or other replay-resistant identifier.

## 5.2 Completion acknowledgment

When a file is completely reconstructed:

1. Verify all expected chunks are present.
2. Compute the reconstructed file's SHA-256 hash.
3. Compare it with the expected manifest hash.
4. Commit the file to its destination.
5. Generate a completion acknowledgment.

The acknowledgment contains:

- Session ID.
- Transfer ID.
- File ID.
- Verified file hash.
- Completion status.
- Timestamp or sequence identifier.

The sender must compare the receipt with its original manifest before acting on it.

Unauthenticated receipts are only claims from the optical peer, not cryptographic proof of which device sent them.

Support optional cryptographically authenticated pairing and signed receipts.

For automated delivery-confirmation actions, authenticated receipts should be required by default.

## 5.3 Missing-chunk requests

When the receiver identifies missing chunks, it may request their retransmission.

The sender should prioritize these chunks over repeating the entire file.

Support compact representations of missing chunk indices or index ranges.

Requests must be bounded to prevent unreasonable allocations or processing.

## 5.4 Receiver readiness

The receiver may report:

- Camera ready.
- Transfer accepted.
- Transfer paused.
- Current reception rate.
- Frame decode success rate.
- Suggested transmission speed.
- Completed file count.
- Remaining missing chunks.

The sender may use this feedback for adaptive transmission.

Do not allow incoming control messages to execute arbitrary local commands or change security-sensitive configuration.

---

# 6. Transmission Visualization

This is a major UI requirement.

Both sending and receiving views must provide a full inspection interface for the file payload.

## 6.1 Hexadecimal viewer

Implement a hex-editor-style viewer.

Example:

```text
OFFSET       HEX DATA                          ASCII
00000000     48 65 6C 6C 6F 20 57 6F          Hello Wo
00000008     72 6C 64 21 00 01 02 03          rld!....
00000010     45 78 61 6D 70 6C 65 20          Example
00000018     -- -- -- -- -- -- -- --          ????????
00000020     46 69 6C 65 20 44 61 74          File Dat
```

Required features:

- Byte offsets.
- Hexadecimal bytes.
- ASCII representation.
- UTF-8 text representation where valid.
- Virtualized scrolling for large files.
- Selection and navigation to specific offsets.
- Search within received or local data.
- Copy selected bytes as hex or text.
- Distinct visualization of unavailable bytes.
- Active-frame highlighting.

The full logical file must be accessible regardless of file size; do not attempt to render a large file in memory as one DOM element.

## 6.2 Sending visualization

During transmission:

- Highlight the exact original-file byte range currently being transmitted.
- Update the highlight whenever the QR frame changes.
- Display the active chunk index.
- Display the total chunk count.
- Display the current file and byte offset.
- Display the file's SHA-256 hash.
- Show current transmission cycle and progress.

Optionally auto-scroll to follow the active chunk.

Allow disabling auto-scroll so the user can inspect other file regions.

If compression or fountain coding makes a frame represent multiple original ranges, highlight the actual contributing ranges instead of displaying a misleading contiguous region.

## 6.3 Receiving visualization

As frames arrive:

- Fill in the appropriate bytes.
- Leave missing chunks visibly empty.
- Support out-of-order updates.
- Highlight newly received bytes.
- Distinguish verified, missing, and rejected chunks.
- Display the latest received chunk.
- Update completion statistics.

Display a second zoomed-out coverage map for large files.

Suggested statuses:

- Received and frame-validated.
- Missing.
- Corrupt/rejected.
- Currently receiving.
- Fully file-hash-verified.

## 6.4 Text mode

Offer text and hexadecimal viewing modes.

Text viewing must tolerate arbitrary binary data and invalid character encodings.

Support UTF-8 by default, with an option for alternate interpretations.

Do not alter the underlying bytes while displaying them.

For received files, missing regions should remain visibly marked rather than silently disappearing from the text view.

---

# 7. Checksums and Integrity Display

Display integrity information prominently.

## Sending

Show:

- Expected complete-file SHA-256.
- File size.
- Number of chunks.
- Transfer manifest hash.
- Current frame checksum.
- Current file and batch progress.

## Receiving

Show:

- Expected complete-file SHA-256 from the manifest.
- Current received-data digest.
- Latest received frame checksum.
- Number of valid chunks.
- Number of missing chunks.
- Number of corrupted/rejected chunks.
- Number of duplicate chunks.
- Full-file SHA-256 once reconstruction finishes.
- Verified/unverified status.

### Important distinction

A SHA-256 hash of the original file cannot generally be determined from arbitrary missing and out-of-order chunks.

For incomplete reception, compute a separately labeled **received-set digest** over sorted chunk identifiers, byte offsets, lengths, and received bytes.

Update it as new chunks arrive.

Do not represent this digest as the partially completed original-file hash.

Once the entire file is present, calculate the actual SHA-256 of the reconstructed file and compare it with the manifest.

Never report successful completion until this comparison passes.

---

# 8. Progress and Estimated Transfer Times

Both the sender and receiver should display detailed timing information.

## 8.1 Sender statistics

Display:

- Current file.
- Current chunk.
- Total chunks.
- Current transmission loop number.
- Current loop progress.
- Bytes transmitted in the current cycle.
- Total QR frames displayed.
- Configured frame duration.
- Actual observed frame rate.
- Estimated effective payload throughput.
- Estimated time to complete one full transmission loop.
- Elapsed time.
- Confirmed delivery status, if available.

## 8.2 Loop-time calculation

For fixed indexed frames:

```text
data_frames = ceil(file_size / payload_bytes_per_frame)

total_frames =
    data_frames
    + manifest_frames
    + parity_frames
    + scheduled_control_frames

estimated_loop_seconds =
    total_frames * frame_duration_seconds
```

Account for scheduler overhead and measured actual frame timing.

Example:

- File size: 1 MiB.
- Effective payload: 600 bytes per QR frame.
- Transmission rate: 8 frames/sec.
- Approximately 1,748 data frames.
- Approximately 219 seconds for one data-only pass.

This is roughly 3 minutes 39 seconds, excluding manifest, control, and other protocol overhead.

The actual throughput depends on QR density, camera resolution, exposure, screen refresh, and decoding performance.

## 8.3 Receiver statistics

Display:

- Percentage of original bytes reconstructed.
- Valid frame rate.
- Duplicate frame rate.
- Rejected frame rate.
- Actual useful bytes/sec.
- Total bytes received.
- Remaining missing bytes.
- Estimated completion time.
- Current transmission loop where known.
- Last successfully decoded frame timestamp.

Base completion estimates on actual reception performance.

If estimates are unreliable, display an uncertainty indicator rather than an unjustified precise ETA.

## 8.4 Large-file warnings

QR transfer is comparatively slow.

When selecting a large file or directory, show an estimated transmission duration.

Warn when a transfer may require unusually long periods of uninterrupted camera and display operation.

Allow the user to proceed regardless.

---

# 9. QR Rendering and Camera Settings

Provide configurable settings and reusable device profiles.

## 9.1 QR rendering

Settings:

- Frame duration in milliseconds.
- Target frames per second.
- QR pixel dimensions.
- QR module size in pixels.
- Maximum QR version/density.
- QR error-correction level: L, M, Q, H.
- Chunk size.
- Quiet-zone width.
- Foreground/background colours.
- Optional inverted display.
- Fullscreen QR display.
- Looping mode.
- Manifest repetition interval.
- Control-frame allocation.
- Screen/display selection.

Maintain a valid quiet zone and use nearest-neighbour scaling without smoothing.

Validate that selected chunk sizes actually fit into the requested QR version and error-correction level.

If the configuration is invalid, explain why and calculate the maximum usable payload.

## 9.2 Webcam configuration

Support:

- Webcam selection.
- Camera resolution.
- Frame rate.
- Preview orientation.
- Crop/region of interest.
- Zoom.
- Exposure controls where supported.
- Focus controls where supported.
- Automatic focus and exposure where supported.
- Mirror correction.
- Camera reconnect after disconnection.

The scanner should operate continuously without requiring a button press for every QR code.

Repeated video frames containing the same QR must not cause duplicated file writes.

## 9.3 Calibration mode

Recommended feature.

Display a QR calibration sequence and measure:

- Successful decoding rate.
- Camera decoding latency.
- Maximum reliable QR density.
- Maximum reliable frame rate.
- Exposure and focus stability.
- Usable screen resolution.

Suggest a reliable transmission configuration and save it as a profile.

## 9.4 Adaptive mode

Recommended for Both Mode.

Using receiver feedback, the sender may adjust:

- Frame duration.
- QR density.
- Payload size.
- Redundancy.
- Error-correction level.

Changes must be coordinated so the receiver can continue decoding without losing the session.

Manual settings must remain available.

---

# 10. API Support

Expose a local application API.

The API must work without external network connectivity.

Prefer a Unix-domain socket on Linux and an appropriate local IPC mechanism on Windows.

An optional HTTP API may bind to loopback only unless deliberately enabled and authorized.

## 10.1 API capabilities

Allow authorized local clients to:

- Query application status.
- List cameras and displays.
- Start receiving.
- Queue files for transmission.
- Queue directories.
- Inspect transfers.
- Pause/resume/cancel jobs.
- Read progress.
- Read transfer history.
- Select transmission profiles.
- Subscribe to events.
- Retrieve completed-file metadata.

Example HTTP-style interface:

```text
GET    /v1/status
GET    /v1/devices
GET    /v1/transfers
GET    /v1/transfers/{id}
POST   /v1/transfers/files
POST   /v1/transfers/directory
POST   /v1/receive/start
POST   /v1/transfers/{id}/pause
POST   /v1/transfers/{id}/resume
POST   /v1/transfers/{id}/cancel
GET    /v1/events
GET    /v1/settings/profiles
```

Document the API using OpenAPI where applicable.

Support event streaming through an authenticated local event stream.

API requests must not grant unrestricted filesystem access.

---

# 11. MCP Support

Implement an MCP server for integration with local AI assistants.

Primary transport: local stdio.

An optional local socket-based MCP service may be provided.

No internet connection or external MCP registry should be necessary.

Suggested MCP tools:

```text
qr_get_status
qr_list_transfers
qr_get_transfer
qr_send_file
qr_send_directory
qr_start_receiver
qr_pause_transfer
qr_resume_transfer
qr_cancel_transfer
qr_get_received_files
qr_get_transfer_history
qr_list_profiles
qr_select_profile
```

These names are illustrative and should be registered using the selected MCP SDK's standard tool schema.

All tools must expose structured inputs and outputs.

### Local LLM integration

The user may run a small local language model on the air-gapped computer.

The model should be able to:

- Discover newly received files.
- Receive completion notifications.
- Queue permitted outgoing files.
- Query transfer status.
- Retrieve transfer metadata.
- Request file-processing workflows.

The MCP integration must not automatically grant an AI model arbitrary command execution, unrestricted filesystem access, or permission to disclose sensitive files.

Provide separate permissions for reading status, receiving, sending, and changing configuration.

Any application consuming these APIs is responsible for deciding how to interpret the transferred file contents.

---

# 12. Webhooks and Completion Actions

Support event-driven automation on both sender and receiver.

## 12.1 Events

At minimum:

```text
transfer.queued
transfer.started
transfer.paused
transfer.resumed
transfer.loop_completed
transfer.failed
transfer.cancelled

file.received
file.verified
file.failed

batch.received
batch.verified

delivery.acknowledged
delivery.confirmed
delivery.failed
```

`transfer.loop_completed` means the sender displayed all frames in one cycle.

`file.verified` means the receiver reconstructed the file and verified its hash.

`delivery.acknowledged` means a matching optical receipt was received.

`delivery.confirmed` must be reserved for an authenticated receipt matching the expected file or batch hash.

## 12.2 Webhook support

Configure outgoing HTTP POST callbacks on matching events.

Webhook payload example:

```json
{
  "event": "file.verified",
  "eventId": "uuid",
  "transferId": "uuid",
  "fileId": "file-001",
  "path": "/received/transaction.json",
  "size": 2048,
  "sha256": "...",
  "timestamp": "2026-10-10T19:00:00Z"
}
```

Features:

- Configurable event subscriptions.
- Multiple destinations.
- Retry policy.
- Timeout.
- Structured error logging.
- Stable event IDs for deduplication.
- Optional authenticated delivery.

For an air-gapped machine, webhooks should target only explicitly permitted local services by default.

No webhook configuration may be supplied or modified by a received file.

## 12.3 Command execution

Allow configured local commands to run when an event occurs.

Examples:

- After receiving a file.
- After verifying a complete batch.
- After confirming delivery on the sending machine.
- After transfer failure.
- After a watched directory file has been sent and acknowledged.

Configuration should include:

- Event type.
- Executable path.
- Explicit argument array.
- Allowed working directory.
- Execution timeout.
- Optional environment variables.
- Retry behavior.
- Exit-code handling.
- Enable/disable state.

Do not pass event data through a shell command string.

Use direct process execution with separately encoded arguments.

Received filenames and metadata must never be interpreted as shell syntax.

Commands must come from preconfigured trusted rules.

Default to command execution disabled until configured.

### Completion semantics

On the receiver, completion-triggered commands run only after verification and successful file commit.

On the sender, remote-delivery commands run only after a valid matching receiver acknowledgment, with authentication required for confirmed-delivery events.

Avoid repeated execution of the same action after duplicate acknowledgments or application restarts.

---

# 13. Command-Line and Headless Operation

Provide a CLI backed by the same Rust transfer engine.

Illustrative commands:

```bash
qrtransceiver send ./request.json

qrtransceiver send ./outbox --recursive

qrtransceiver receive --output ./inbox

qrtransceiver both --profile desktop-pair

qrtransceiver status

qrtransceiver queue

qrtransceiver pause <transfer-id>

qrtransceiver resume <transfer-id>
```

These are example interfaces, not finalized executable syntax.

A background agent/service should be able to manage queues, scan cameras, and publish events without requiring interaction with the primary GUI.

Sending QR codes still requires an available graphical display/output surface. Headless operation must not imply that an optical display is unnecessary.

Provide machine-readable JSON CLI output.

Use the same API and transfer state engine for both graphical and command-line operation.

---

# 14. Profiles and Configuration

Support multiple saved profiles.

Examples:

- Laptop webcam and monitor.
- Desktop webcam and monitor.
- Small-screen receiver.
- High-reliability slow transfer.
- Fast low-redundancy transfer.
- Air-gapped automated mode.

Profiles contain:

- Application operating mode.
- Camera selection.
- Display selection.
- QR dimensions.
- Frame duration.
- QR correction level.
- Chunk size.
- File/directory behavior.
- Destination folders.
- Loop settings.
- Feedback settings.
- API/MCP permissions.
- Automation event configurations.

Use versioned JSON configuration files.

Accept imported JSON5 configuration if convenient, but persist a documented canonical JSON representation.

Configuration import must not silently enable dangerous automation or external network access.

---

# 15. Transfer History and Persistence

Persist:

- Transfer session IDs.
- Job statuses.
- File metadata.
- Checksums.
- Chunk-reception bitmaps.
- Temporary reconstructed data.
- Transfer timestamps.
- Completion receipts.
- Error information.
- Automation results.

Use SQLite or another transactional local database for queue and session state.

Store file payloads on disk rather than inside the database.

The application should recover gracefully from an unexpected shutdown.

## Resuming incomplete files

If the receiver restarts:

1. Load its incomplete transfer state.
2. Verify staging-file consistency.
3. Recover the chunk bitmap.
4. Resume accepting missing chunks.
5. Complete reconstruction when possible.

If a transfer resumes without a reverse channel, repeated transmission loops should eventually supply the missing frames.

Never reuse stale verified status after the underlying staging file has changed.

---

# 16. Application UI

Use Svelte for a clean desktop-oriented interface.

## Main navigation

```text
QR Transceiver
├── Dashboard
│   ├── Send / Receive / Both
│   ├── Active Transfers
│   └── Connection & Device Status
├── Send
│   ├── Files & Directories
│   ├── Queue
│   ├── QR Display
│   └── Payload Viewer
├── Receive
│   ├── Camera Preview
│   ├── Active Reception
│   ├── Payload Viewer
│   └── Received Files
├── History
├── Automation
│   ├── API
│   ├── MCP
│   ├── Webhooks
│   └── Commands
└── Settings
    ├── Cameras & Displays
    ├── QR Encoding
    ├── Timing & Performance
    ├── File Handling
    ├── Transfer Profiles
    └── Security
```

## Layout

The sending interface should show:

- File queue.
- File/transfer details.
- Large active QR code.
- Hex/text payload viewer.
- Active byte-range highlighting.
- Current frame and chunk.
- Loop timing and throughput.
- Transmission controls.

The receiving interface should show:

- Live camera preview.
- Current transfer session.
- Per-file reception progress.
- Hex/text payload viewer.
- Missing-chunk visualization.
- Hash validation state.
- Completion and automation status.

Both Mode should provide a split interface for outgoing and incoming transfers.

Allow QR windows to be detached and displayed fullscreen on another monitor.

## Usability

- Support dark and light themes.
- Remember window positions.
- Permit configurable UI scaling.
- Use readable status labels.
- Avoid animations unrelated to active transfers.
- Support keyboard shortcuts.
- Display detailed errors.
- Keep transfer progress responsive even with large files.

---

# 17. Security Requirements

The application is intended to operate in environments containing sensitive information.

## 17.1 File isolation

- Do not interpret transferred payloads as commands.
- Do not automatically open received files.
- Do not automatically execute received files.
- Do not follow untrusted paths outside the destination directory.
- Reject path traversal.
- Reject unsafe absolute paths.
- Handle symbolic links safely.
- Enforce size and resource limits.
- Verify file hashes before completion events.
- Quarantine invalid transfers where appropriate.

## 17.2 Optical communication

QR transfer provides transport, not inherent confidentiality or sender authentication.

Anyone able to record the displayed QR sequence may be able to reconstruct its contents.

Checksums detect corruption but do not prove who sent a file.

Provide optional peer pairing and authenticated acknowledgment support.

Authenticated pairing may use independently generated application identity keys and an initial user-verified fingerprint exchange.

These keys must be separate from any cryptocurrency wallet keys.

Optional end-to-end encrypted file transfer may be added later.

Do not claim that plain QR transfer provides encryption.

## 17.3 Automation safety

- Commands disabled by default.
- Commands configured only through trusted local configuration or authorized API access.
- No shell interpolation of received metadata.
- No execution of received files.
- Restrict API and MCP permissions.
- Require explicit authorization for sensitive configuration changes.
- Protect local API endpoints.
- Limit execution time and resource consumption.
- Prevent automation-event loops.

For air-gapped deployments, expose only local IPC endpoints by default.

## 17.4 Supply-chain controls

- No telemetry.
- No mandatory online services.
- No mandatory automatic updates.
- Pin dependencies.
- Maintain lockfiles.
- Provide source code and build instructions.
- Document bundled native libraries.
- Support offline installation.
- Provide build hashes.

---

# 18. Suggested Project Structure

```text
qrtransceiver/
├── README.md
├── SECURITY.md
├── docs/
│   ├── architecture.md
│   ├── protocol.md
│   ├── api.md
│   ├── mcp.md
│   ├── automation.md
│   └── testing.md
├── src/
│   ├── lib/
│   │   ├── components/
│   │   ├── stores/
│   │   ├── views/
│   │   └── types/
│   └── routes/
├── src-tauri/
│   ├── src/
│   │   ├── commands/
│   │   └── main.rs
│   └── capabilities/
├── crates/
│   ├── qrtx-core/
│   ├── qrtx-protocol/
│   ├── qrtx-qr/
│   ├── qrtx-camera/
│   ├── qrtx-storage/
│   ├── qrtx-automation/
│   ├── qrtx-api/
│   └── qrtx-mcp/
├── cli/
├── schemas/
├── tests/
│   ├── unit/
│   ├── integration/
│   ├── protocol/
│   └── fixtures/
└── scripts/
```

The Rust engine must be independent of the UI.

The desktop interface, CLI, and MCP server should share the same core implementation rather than duplicating transfer logic.

Use streaming file I/O wherever possible.

Do not read large files into JavaScript memory simply to create QR frames.

---

# 19. Testing Requirements

## Basic correctness

Verify that received files are byte-for-byte identical.

Test:

- Empty files.
- One-byte files.
- Small text files.
- Arbitrary binary files.
- Non-UTF-8 content.
- Files larger than one QR frame.
- Multiple files.
- Nested directories.
- Unicode filenames.
- Files with duplicate names in different directories.
- Large files using simulated optical delivery.

## QR reliability

Simulate:

- Missing frames.
- Duplicate frames.
- Out-of-order frames.
- Corrupt frames.
- Camera capture interruptions.
- Variable frame rates.
- Delayed receiver startup.
- Sender restarts.
- Receiver restarts.
- Malformed QR payloads.
- Incorrect protocol versions.
- Multiple overlapping transfer sessions.
- Stale acknowledgments.

The reconstruction algorithm must pass with arbitrary frame arrival order.

## Bidirectional testing

Verify:

- Both computers transmit simultaneously.
- An idle receiver displays completion control messages.
- Sender detects a matching acknowledgment.
- Sender stops when configured.
- Missing-chunk requests cause selective retransmission.
- Old receipts cannot complete a new session.
- Invalid receipts do not trigger confirmed-delivery events.
- Control traffic does not permanently block file transmission.

## Automation testing

Verify:

- API job submission.
- MCP job submission.
- Completion webhooks.
- Completion commands on both ends.
- Correct file path handling.
- Stable event IDs.
- Idempotent event processing.
- No command execution before hash verification.
- No command injection through filenames.
- No execution of files received from the optical channel.

## UI testing

Verify:

- Sent-byte highlighting.
- Out-of-order received-byte rendering.
- Missing-byte placeholders.
- Live checksum updates.
- Correct file-hash verification status.
- Accurate loop-time estimates.
- Responsive UI during large transfers.
- Correct progress after resuming interrupted sessions.

Provide a simulated camera/frame transport so the full protocol can be tested on a single development VM without two physical computers.

Also perform real webcam-to-screen end-to-end tests before release.

---

# 20. Implementation Priorities

### Stage 1 — Core transfer

Implement:

- Shared Rust engine.
- Basic Tauri/Svelte GUI.
- Send and Receive modes.
- Static and animated QR codes.
- Indexed chunk transfers.
- Manifest support.
- File hash verification.
- Hex/text payload viewer.
- Live transmitted/received byte highlighting.
- Basic timing estimates.

### Stage 2 — Reliability and Both Mode

Implement:

- Bidirectional QR control.
- Acknowledgments.
- Missing-chunk requests.
- Selective retransmission.
- Multiple files and directories.
- Persistent transfer sessions.
- Transfer recovery.
- Queue management.

### Stage 3 — Automation

Implement:

- Local API.
- MCP server.
- CLI.
- Webhooks.
- Completion commands.
- Directory watching.
- Automation permission controls.

### Stage 4 — Optimization

Implement:

- Camera calibration.
- Adaptive frame rates.
- Adaptive density.
- Optional fountain coding.
- Advanced throughput statistics.
- Authenticated device pairing.
- Optional encrypted transfers.

These stages are an implementation sequence, not permission to omit required functionality from the finished application.

---

# 21. Acceptance Criteria

The complete application must satisfy the following:

1. The same application can run on both computers.
2. Either installation can send, receive, or do both.
3. Arbitrary binary files can be transferred without modification.
4. Multiple files and complete directories are supported.
5. Transfers can operate without reverse communication.
6. Both Mode supports optical feedback and completion acknowledgments.
7. Missing chunks can be recovered without retransmitting an entire file when bidirectional feedback is available.
8. Sender highlights the exact data being transmitted.
9. Receiver progressively fills in received data regardless of chunk order.
10. The UI displays expected hashes, current reception integrity information, and final verification results.
11. Full-file SHA-256 verification is required before accepting completion.
12. Frame duration, QR size, payload density, and QR error correction are configurable.
13. The app estimates one complete transmission loop and tracks actual throughput.
14. Multi-file and directory queues are persistent.
15. The API, CLI, and MCP server can control permitted transfers.
16. Webhooks and configured commands are supported on both ends.
17. Completion actions cannot be triggered by merely displaying every QR frame once.
18. The transfer engine operates without internet access.
19. No received file can cause arbitrary code execution.
20. Simulated and physical end-to-end transfer tests pass.

---

# 22. Instructions to Codex

Build a working application, not merely a prototype demonstrating QR encoding.

Start by inspecting currently maintained Rust QR encoding/decoding libraries and camera backends.

Use established libraries whenever possible.

Implement the protocol and file reconstruction engine independently of Svelte and Tauri.

Document the on-wire protocol so another application could implement a compatible sender or receiver.

Use a simulated frame transport to exercise the protocol without physical cameras.

Ensure the program works when both endpoints use the same binary.

Prioritize byte-perfect file reconstruction, out-of-order handling, recovery, and payload visualization.

Do not introduce cryptocurrency-specific features or dependencies.

Do not introduce cloud services, telemetry, or external network requirements.

Implement optional optimizations only after the basic transport and its integrity checks are functioning reliably.

**Final objective:** A self-contained, reliable, visually inspectable, and automation-friendly optical file-transfer application suitable for communication between ordinary computers and permanently air-gapped systems.
