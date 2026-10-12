# Local API

The desktop and background service own the same Rust engine. Linux IPC is a mode-0600 Unix socket at STATE/engine.sock; Windows IPC is a local named pipe whose name derives from the state path. Clients authenticate with named tokens stored in private config.json. Tokens are compared through SHA-256 and constant-time comparison.

IPC is one newline-delimited JSON request and response per connection:

```json
{"token":"local-token","method":"status","params":{}}
```

Responses contain `ok`, `result`, and a structured `error` when unsuccessful. Requests are bounded to 1 MiB. Send paths must lie within configured outgoing roots. Received file contents cannot supply configuration or commands.

Devices returns cameras and displays; display metadata is populated when the desktop hosts the engine. A standalone daemon returns an empty display list.

Methods: status, devices, transfers, transfer, history, received, enqueue, action, receive_start, receive_stop, next_frame, ingest, payload, coverage, digest, search, missing, profiles, save_profile, select_profile, set_profile, camera, camera_control, destination, automation, automation_config, automation_save, events.

`action` accepts pause, resume, cancel, restart, priority and remove. Restart snapshots the current source under a new transfer/session identity. Remove requires a non-active job and frees its snapshots/staging; committed received files remain intact. Payload reads are at most 64 KiB and carry null values for unavailable bytes. Events accept an after cursor and a limit up to 1000.

Optional HTTP: `qrtransceiver serve --http 38475`. Every /v1 route requires `Authorization: Bearer TOKEN`. Browser Origin headers are rejected; CORS is not enabled. It listens on 127.0.0.1. Liveness/readiness are /livez and /readyz. No external binding is implemented.

The OpenAPI document lists the principal REST routes. POST /v1/rpc exposes the same method/params dispatch. GET /v1/events?after=N is an authenticated SSE stream of persistent event batches; use the event cursor to recover after disconnecting.

Scopes:
- read: status, metadata, event/history and bounded payload inspection.
- receive: camera receiving, ingesting frames and missing-chunk requests.
- send: queueing, transfer actions and requesting output frames.
- configure: profiles, camera controls and trusted automation rules.

The locally generated CLI token has all scopes. The separately generated MCP token initially has read only. Do not give the CLI token to an assistant when a narrower MCP identity is sufficient.

The configure-scoped destination method takes {id, path} and changes a receiving transfer's output directory before its first file is committed. Enqueue profile overrides also require configure permission. Each next_frame response includes duration_ms for its saved transfer profile.
