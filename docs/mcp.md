# MCP

Run `qrtransceiver mcp` as a local stdio server alongside the running desktop app or daemon. It uses the official `rmcp` SDK and forwards requests to authenticated local IPC; it does not create a second engine. stdout contains MCP protocol messages only; diagnostics go to stderr.

Example client entry:

```json
{
  "mcpServers": {
    "qr-transceiver": {
      "command": "/absolute/path/qrtransceiver",
      "args": ["--state", "/absolute/app-state", "mcp"]
    }
  }
}
```

Default identity: the local-mcp token in config.json, initially read-only. To use a different scoped token, provide QRTX_MCP_TOKEN to the MCP process. Sending additionally requires permitted outgoing roots.

Read tools:
- qr_get_status
- qr_list_transfers
- qr_get_transfer
- qr_get_received_files
- qr_get_transfer_history
- qr_list_profiles

Mutating tools, all dry-run unless apply=true:
- qr_send_file, qr_send_directory (send)
- qr_start_receiver (receive)
- qr_pause_transfer, qr_resume_transfer, qr_cancel_transfer (send)
- qr_select_profile (configure)

Tool arguments have SDK-generated JSON schemas. Results are structured JSON text. The transfer event stream is available from local IPC/API; consumers can poll qr_get_received_files for newly committed files. The server does not interpret or execute received payloads.

## Desktop instances and HTTP MCP

The desktop executable accepts `--state PATH`, `--enable-mcp`, and `--mcp-port PORT`. Run the same binary twice with independent state directories and ports:

```powershell
Start-Process .\qrtx-desktop.exe -ArgumentList '--state C:\QRTX\sender --enable-mcp --mcp-port 47631'
Start-Process .\qrtx-desktop.exe -ArgumentList '--state C:\QRTX\receiver --enable-mcp --mcp-port 47632'
```

Connect MCP clients to `http://127.0.0.1:47631/mcp` or `http://127.0.0.1:47632/mcp`. Supply an Authorization Bearer token from that instance's `config.json`; the generated `local-mcp` identity is read-only. Tokens from one instance do not authenticate against the other. Grant only the required scopes for mutating tools, which still require `apply: true`.

This is the official SDK's stateless Streamable HTTP transport, with JSON responses and legacy protocol compatibility. Each request retains its own scoped identity when forwarded to the same instance's local IPC. Origin-bearing browser requests are rejected, and the listener binds only to IPv4 loopback. `/livez` and `/readyz` are available on the MCP port.

MCP is off by default. Enable it with `--enable-mcp` or the MCP server switch in Settings (`mcp_enabled: true` in config). A port alone never enables MCP. Settings take effect after restarting the instance. The port flag takes precedence without changing the desktop's saved configuration; the default enabled port is 38476. Port zero and occupied ports fail at startup. `--state` takes precedence over `QRTX_STATE`; the default remains the platform app-data directory. Each state directory also owns its WebView cache and saved window positions.

The daemon supports the same port via `qrtransceiver --state PATH serve --enable-mcp --mcp-port PORT`. The existing stdio bridge remains available. The in-process desktop listener is intentional for this standalone local application: there is no external upstream service or separately privileged sidecar identity.
