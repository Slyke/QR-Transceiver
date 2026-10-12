# Building and offline deployment

Use the checked-in Cargo.lock and desktop/package-lock.json. Root npm commands forward to the desktop package. Android has a separate Gradle build described in [android.md](./android.md). Rust dependencies include qrcode, rqrr, image, Nokhwa, rusqlite with bundled SQLite, cap-std, Ed25519-Dalek, Axum, Tokio and the official rmcp SDK.

Native requirements:
- Linux: GTK3, WebKitGTK 4.1, Ayatana appindicator, librsvg, libv4l, libclang, a C compiler and pkg-config.
- Windows: Visual Studio C++ Build Tools and WebView2.
- Raspberry Pi: 64-bit Raspberry Pi OS with a graphical desktop and WebKitGTK 4.1. USB webcams use V4L2. No 32-bit ARM target is planned.

Build the native desktop package on the target platform. ARM AppImage cross-packaging is not assumed; use a native ARM64 runner for release packages. The core can be cross-built with the AArch64 GNU target and compiler.

```sh
npm ci
npm run check
npm run build
cargo test --locked
cargo build --locked --release -p qrtransceiver --features camera
npm run tauri build -- --bundles deb
```

Offline installation should use prebuilt packages, their required OS libraries and recorded hashes. Neither transfer endpoint requires an internet connection at runtime. For offline source builds, prepare dependencies on a connected build machine using cargo vendor and the npm cache, then carry the source, lockfiles, vendored crates, npm cache and system packages through your approved process. Use scripts/offline-prepare.sh.

scripts/build-hashes.sh records SHA-256 for built artifacts. Release hashes must be generated from the actual final binaries; no unbuilt platform artifact is represented by a hash.

Use --bundles nsis for Windows, or --bundles appimage for a Linux AppImage. The Debian package requires its declared distribution libraries; include these packages in an offline installation medium.

Platform CI is defined in .github/workflows/ci.yml for native Linux x86-64, Linux ARM64 and Windows. Local cross compilation confirms the core's ARM64 target; it does not replace physical webcam/display validation.

Optional fountain coding, encryption and adaptive calibration remain future optimizations. The indexed protocol, QR error correction, signed receipts and selective retransmission work independently of them.

## Windows native build

Run from a normal Windows directory (for example, C:\Users\you\qrtx-windows), not a UNC checkout or a packaged app's virtualized AppData directory. Install Visual Studio C++ tools, a Windows SDK, the pinned Rust toolchain and Node 24.

```powershell
npm ci
npm run check
npm run tauri build -- --bundles nsis --ci
cargo build --locked --release -p qrtransceiver --features camera
```

The desktop executable is target\release\qrtx-desktop.exe and the installer is under target\release\bundle\nsis. Use the Tauri build command for distributable binaries: it enables the custom protocol and embeds dist. A bare cargo build of the desktop without tauri/custom-protocol loads the development URL.

See mcp.md for launching the same desktop executable twice using --state, optional --enable-mcp, and --mcp-port. MCP is disabled by default; selecting a port does not enable it. The deterministic tests\fixtures\webcam-2k.png is exactly 2,048 bytes; scripts\make-demo-image.py regenerates it. A physical test must capture the sender's visible QR sequence with the receiver's webcam, then compare the committed file's SHA-256 with the original.