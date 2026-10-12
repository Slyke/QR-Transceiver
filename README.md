# QR Transceiver

A local optical file-transfer application built with Rust, Tauri 2, Svelte 5 and TypeScript. Two installations exchange files using screens and USB webcams. The transfer engine does not require a network, cloud account, telemetry or an update service.

## Android

The native Android app supports offline Send/Receive/Both, front/rear cameras, file and folder import, verified file export, resumable queues, and trusted receipts. It has no MCP or command/webhook automation. Build an APK with `bash scripts/build-android.sh`; see [Android setup and usage](./docs/android.md) for prerequisites and validation.

## Repository layout

- `desktop/`: Tauri host and Svelte UI. Root npm commands forward to this package.
- `android/`: native Android application and Gradle wrapper.
- `crates/`: shared protocol, engine, desktop runtime, and Android JNI bridge.
- `cli/`: existing CLI/service and simulator.

## Run the desktop application

On Debian/Ubuntu or 64-bit Raspberry Pi OS:

```sh
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev patchelf libclang-dev libv4l-dev
npm ci
npm run tauri dev
```

Use Node 24 and the pinned Rust toolchain. On Windows install the Microsoft C++ Build Tools and WebView2, then use the same npm commands in a native Windows checkout. Raspberry Pi targets **ARM64**, with USB webcams. Android has a native Kotlin/Compose app sharing the Rust protocol and file engine. iOS adapters remain future work.

Production desktop build: `npm run tauri build -- --bundles deb` on Linux or `npm run tauri build -- --bundles nsis` on Windows. Build the CLI and background service with `cargo build --release -p qrtransceiver --features camera`. The desktop and CLI must point to the same state directory to share an engine. By default they use the platform application-data directory named `com.qrtx.transceiver`; override with `QRTX_STATE` or the CLI's `--state`.

## Transfer files

1. Select Send, Receive, or Both. A sender needs a display; a receiver needs a camera.
2. Add files or a directory. The engine snapshots sources to disk and calculates SHA-256.
3. Select **Start display** and **Start camera** on the appropriate endpoints.
4. In Both mode, enable feedback and start the displays on both endpoints. Frame and file ACKs remove accepted data from rotation; a missing-chunk request restores the requested frames.
5. To authenticate receipts, compare each device's fingerprint independently and enter it in the other device's trusted peers **before queueing**.

A displayed loop is never delivery confirmation. Unpaired receipts are labeled acknowledgment claims. Only a matching receipt signed by a configured trusted identity produces `delivery.confirmed`.

The payload inspector supports byte offsets, missing-byte markers, UTF-8 and alternate encodings, search, selection, copying, coverage maps, and active-frame highlighting. Use Ctrl/Cmd+O to add files, Ctrl/Cmd+Shift+O for a directory, and Space to pause/resume display.

## Local service and automation

```sh
qrtransceiver serve --allow-root /absolute/outbox
qrtransceiver send /absolute/outbox/example.bin
qrtransceiver send /absolute/outbox --recursive
qrtransceiver queue
qrtransceiver receive
qrtransceiver mcp
```

The GUI already hosts a service; run a separate daemon only when the GUI is closed or use a different state directory. The engine prevents concurrent owners of one state database.

Linux uses a permission-restricted Unix socket; Windows uses a local named pipe. Optional HTTP is enabled explicitly with `serve --http 38475` and binds to 127.0.0.1. Sending through automation requires permitted outgoing roots. A headless sender still needs a display client that requests and renders frames.

The desktop Automation screen edits trusted rules, with a separate explicit enable control. Commands receive an event JSON object on stdin, use a fixed executable and argument array, and are disabled by default. They cannot execute files from the receive or staging directories.

## Verify without cameras

```sh
cargo test
cargo test -p qrtx-runtime --features camera
npm run check
npm run build
qrtransceiver simulate ./tests/fixtures /tmp/qrtx-test --drop-every 7
```

The simulator uses the same engine at both endpoints, drops frames, exchanges signed control messages, requests retransmissions and verifies byte-perfect reconstruction.

For the browser integration test:

```sh
qrtransceiver --state .qrtx serve --http 38475 --allow-root ./tests/fixtures
# In another terminal:
npm run dev
# Then:
npx playwright install chromium
npm test
```

The Vite-only proxy reads local development credentials on the server. Production assets contain no API token.

## Documentation

- [Android app](./docs/android.md)
- [Architecture](./docs/architecture.md)
- [QRTX/1 wire protocol](./docs/protocol.md), [interoperability vectors](./docs/protocol-vectors.json)
- [Local API](./docs/api.md), [OpenAPI](./schemas/openapi.json)
- [MCP](./docs/mcp.md), [automation and crash semantics](./docs/automation.md)
- [Builds, offline installation and platform validation](./docs/building.md)
- [Testing and release checks](./docs/testing.md), [local verification record](./docs/verification-record.md)
- [Security](./SECURITY.md)
- [Original implementation specification](./docs/implementation-spec.md)

This implementation uses the adjacent Phox style guide's shared tokens, full-page monospace typography, root themes, TypeScript conventions and structured diagnostics. Application-specific deviations are recorded in the architecture document.
