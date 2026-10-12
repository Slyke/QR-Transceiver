# Android

The Android app is a native Kotlin / Jetpack Compose application in `android/`. It shares `qrtx-core` and `qrtx-protocol` with the desktop app through the small JNI crate `crates/qrtx-android`. It does not link the desktop runtime, run an MCP server, execute commands, watch folders, or send webhooks. The manifest requests camera access only; there is no Internet or broad storage permission.

## Supported devices and behavior

- Android 8.0 (API 26) or newer; ARM64 phones/tablets and x86-64 emulators.
- Send, Receive, and Both modes; front/rear cameras with tap-to-focus.
- Import multiple documents or a directory through Android's Storage Access Framework.
- Persistent queues, pause/resume/cancel/restart, SHA-256 verification, missing-chunk requests, and signed receipts using the existing QRTX/1 protocol.
- Full-screen QR display with whole-pixel modules and a four-module quiet zone. In Both mode the camera remains active in a small preview while the QR display is expanded.
- File details, hashes, progress, and a paginated hex/ASCII inspector.
- Explicit export of verified received files through Android's Save dialog.
- Light/dark system themes. Transfers keep the screen awake and suspend when the app is no longer visible; they resume when it returns. No background camera service is installed.

Select **Both**, start the camera and display on both devices, and use the front camera when the two screens face each other. Select **Send** or **Receive** for a one-way transfer. A displayed loop is never proof of delivery. Compare each device's full fingerprint independently, enter it under Settings, and save before queueing files. Untrusted receipts do not stop Android's sender. Changing trust, chunk size, or error correction affects new transfers; frame duration also updates active and paused outgoing jobs.

Document-provider names are sanitized for the portable wire protocol, and duplicate names receive suffixes. Imports are private snapshots; Android never watches the original provider document for later edits. Private imported inputs remain available for restart until the last transfer referring to them is removed.

The incoming queue is limited to 1 GiB of declared file content. Keep storage available for staged and committed copies; export and remove completed transfers to reclaim space. Removing an incoming queue entry deletes its private file copies, but not documents already exported with Save. Uninstalling the app removes its private identity, queue, inputs, and received files. Android backup is disabled to avoid cloning the signing identity.

Use **Save all to folder** on a completed incoming transfer to recreate its directory tree, including empty folders, inside a new QRTX folder at your selected destination. Individual verified files can also be saved separately before the rest of the transfer finishes. A provider error may leave a partial exported folder; the verified private copy remains available to retry.

Android commits received files with a capability-relative atomic no-overwrite rename. [Android app SELinux policy](https://android.googlesource.com/platform/system/sepolicy/+/refs/heads/main/private/app_neverallows.te) forbids the hard-link promotion used on desktop; both paths preserve collision handling and commit-intent recovery.

## Build

Use Linux/macOS, or WSL2 on Windows. Open `android/` in Android Studio on Linux/macOS; on Windows use the WSL build commands below. The existing Windows desktop build still uses native Windows tools.

Install:
- JDK 17
- Android SDK platform 36, Build Tools 35.0.0, Platform Tools
- Android NDK 28.2.13676358 (or set `ANDROID_NDK_HOME` to a compatible NDK)
- The repository's pinned Rust toolchain and Android targets

```sh
export JAVA_HOME=/path/to/jdk-17
export ANDROID_HOME=/path/to/android-sdk
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"
sdkmanager 'platforms;android-36' 'build-tools;35.0.0' 'platform-tools' 'ndk;28.2.13676358'
rustup target add aarch64-linux-android x86_64-linux-android
bash scripts/build-android.sh
```

The Gradle wrapper is pinned to 8.13 with a distribution checksum. Gradle calls `scripts/build-android-rust.sh` before packaging and builds both supported ABIs. Native libraries are linked with 16 KiB page alignment. Dependencies are needed during the initial build; the installed app transfers files offline.

For this workspace, locally downloaded tools can also be used automatically from `.android-tools/jdk` and `.android-tools/sdk`; these are ignored build dependencies, not source files.

The installable development APK is `android/app/build/outputs/apk/debug/app-debug.apk`.

```sh
"$ANDROID_HOME/platform-tools/adb" install -r android/app/build/outputs/apk/debug/app-debug.apk
```

Use `bash scripts/build-android.sh assembleRelease` for a release build. Release signing is intentionally not configured: provide your own release key through the normal Android signing process. Do not distribute the debug signing key as a production identity.

## Verification

```sh
cargo test --locked -p qrtx-android
bash scripts/build-android.sh testDebugUnitTest lintDebug assembleDebug
# With an emulator/device connected:
bash scripts/build-android.sh connectedDebugAndroidTest
```

Host interoperability tests send files in both directions between the Android bridge and desktop engine, recover a partially received file, verify trusted/untrusted receipt behavior, and decode rendered Android QR modules. JVM tests exercise camera-plane stride handling and provider filename handling. Instrumentation tests load the packaged JNI library on Android, transfer byte arrays through QR raster decoding, reopen the receiver after dropped frames, verify saved bytes, and exchange signed receipts. Compose tests exercise navigation and settings.

A real phone-to-screen optical test is still needed to establish camera focus, thermal behavior, lighting tolerance, and sustainable frame rate on a particular device.

Implementation references: [CameraX image analysis](https://developer.android.com/media/camera/camerax/analyze), [Storage Access Framework](https://developer.android.com/guide/topics/providers/document-provider), [16 KiB page support](https://developer.android.com/guide/practices/page-sizes), [AGP 8.13 compatibility](https://developer.android.com/build/releases/agp-8-13-0-release-notes).

See [the Android verification record](./android-verification.md) for the checks performed on this build.
