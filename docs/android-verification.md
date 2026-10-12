# Android validation — 2026-10-11

Implementation: native Kotlin/Compose, CameraX, Android document providers, and the shared Rust engine through JNI. Android 8+; ARM64 and x86-64. No MCP, folder-watch automation, command hooks, or webhooks.

Completed checks:

- Rust Android bridge: 8 integration tests covering both desktop transfer directions, QR raster decoding, recovery after lost frames/restart, trusted/untrusted receipts, verified folder exports, shared import lifetime after restart, and Android data-directory aliases.
- Two additional shared-core tests verify Android atomic no-overwrite promotion and rejection of parent-symlink escapes. All shared-core recovery/security/transport tests pass after the platform adaptation.
- Rust formatting and Clippy with warnings denied.
- Kotlin compilation and 4 JVM tests covering camera-plane stride/bounds and portable document names.
- Android lint: no errors; dependency update notices remain.
- Debug APK assembly with both native ABIs.
- APK has camera permission and AndroidX's internal signature permission, with no Internet or broad storage permission.
- ELF native libraries and APK packaging use 16 KiB alignment.
- Existing desktop check script: Rust tests (including camera runtime), Clippy, Svelte check, and production frontend build.
- Desktop Tauri compilation after relocation.
- All 4 desktop browser tests pass using a separate test state, including sending, payload inspection, receiving, and responsive layout.

On-device validation: both instrumentation tests pass on an Android 11 / API 30 x86-64 emulator (67.4 seconds for the final combined run). The UI test imports an actual document, checks navigation/settings, starts the QR display, enters full screen, and captures the app. The native test decodes QR raster frames, recovers after dropped frames and a receiver restart, verifies the committed bytes, confirms a trusted signed receipt, and removes the private output. This exposed and verified the fixes for Android's hard-link restriction and data-directory aliases.

The final UI refinement was also verified separately: the complete import/display/full-screen/navigation smoke test passed in 43.158 seconds. Controls are positioned above the QR preview, and screenshots were visually inspected.

Artifacts:
- `artifacts/qr-transceiver-android-debug.apk` — installable development build.
- `artifacts/android-SHA256SUMS` — SHA-256 of that exact APK.
- `artifacts/android-home.png` and `artifacts/android-display.png` — real emulator captures.

The Android 16 / API 36 emulator was unstable under this WSL host; runtime validation used API 30 instead. ARM64 was cross-compiled, and both APK architectures passed 16 KiB ELF/package alignment checks. A physical phone-to-screen camera test is still required for focus, lighting, and sustained frame rate.

Build and test commands are in [android.md](./android.md).
