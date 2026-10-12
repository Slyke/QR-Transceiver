# Automation and crash semantics

Config.json accepts JSON5 on input and is persisted as canonical JSON. Exact whole-value environment references such as "${WEBHOOK_TOKEN}" resolve before validation; partial interpolation is not performed. Missing variables become null. Tokens and webhook credentials never appear in public summaries or logs.

The desktop Automation screen can configure:
- permitted outgoing roots;
- directory watches, recursion and stable-file delay;
- event-filtered loopback HTTP webhooks;
- trusted executable paths, argument arrays, working directories, environment, timeout, retry count and accepted exit codes.

Enabling a rule requires the explicit enable control. Commands and webhooks are absent/disabled initially. Received metadata cannot enable or change them. Watchers remember queued size/mtime versions across restarts and reject watching the current receive folder to reduce feedback loops.

Events include transfer.queued, started, paused, resumed, cancelled, loop_completed; file.received, verified, failed; batch.received, verified; delivery.acknowledged and confirmed. A sender loop event is never a receipt. Completion commands cannot subscribe to loop_completed, file.received or unauthenticated delivery.acknowledged. Prefer file.verified, batch.verified and delivery.confirmed.

Commands use direct process execution. Arguments are fixed trusted strings. The event object is JSON on standard input. Received filenames never pass through a shell. Executables under the state or receive directories are refused. A timeout covers writing stdin and waiting for exit. Output is discarded to prevent unbounded capture.

Each (event ID, rule ID) is durably claimed before execution. Duplicates and restarts do not rerun it. If the process crashes after claiming a command, its outcome may be unknown; the recorded running state is preserved for operator review. Exactly-once external side effects cannot be guaranteed across a crash. Automatic retries within an uninterrupted run are explicit and bounded; configure only when the command's effects are idempotent.

Webhooks use stable event IDs in the Idempotency-Key header, explicit literal loopback destinations, bounded timeout/retries and no redirects or environment proxy. Recipients should deduplicate event IDs. Network destinations and commands are always local trusted configuration.

Changing rule IDs creates new action identities and can cause old persistent events to be processed with the new rule on restart; use event-aware handlers and review configuration before enabling.
