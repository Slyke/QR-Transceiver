# Security

QRTX transfers arbitrary bytes without opening or executing received files. It provides integrity, not confidentiality. Anyone able to record the QR sequence may reconstruct it.

Checksums detect corruption; SHA-256 verifies reconstruction. Ed25519 receipts bind a transfer and session to a device key. Pair by independently comparing fingerprints. Unpaired receipts are labeled claims and cannot produce confirmed-delivery actions. Remote pause/resume/cancel require a paired key.

Defenses include bounded packet/manifest/control sizes, per-device receive quota, immutable outgoing snapshots, portable path validation, capability-relative file commits, conflict-safe default renaming, persisted replay counters, exclusive engine ownership, and full staging revalidation on restart.

Local API tokens, identity keys and state should remain private to the OS account. Unix state directories are mode 0700, secrets and sockets mode 0600. Windows relies on the user's application-data ACL and a local-only named pipe. Do not place state in a shared writable directory.

No shell interpolation, auto-opening, telemetry, cloud services or auto-updater is implemented. Trusted completion commands are disabled until explicitly configured. A configured handler is itself trusted software and must safely process untrusted input.

Before release, perform the physical and platform checks in docs/testing.md. Resource exhaustion by repeated new optical sessions remains bounded by configured storage/session limits; clear abandoned sessions through local administration when needed.
