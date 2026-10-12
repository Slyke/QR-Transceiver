# Requested changes and clarifications

Recorded from this conversation on 2026-10-11 (America/Vancouver).

The original specification is preserved in [implementation-spec.md](implementation-spec.md). This document consolidates the user's subsequent requirements; these amendments take precedence where they differ from the original specification. It is a requirements summary, not a verbatim conversation transcript. Implementation details and verification limits are recorded separately in [architecture.md](architecture.md) and [verification-record.md](verification-record.md).

## Platform and coding requirements

- Use the adjacent `../styleguide` coding standards where appropriate.
- Support 64-bit Windows and Linux desktops and ARM64 processors, including Raspberry Pi devices. 32-bit support is not required.
- Keep the shared protocol/core suitable for eventual mobile-phone support. Mobile adapters remain future work.
- USB webcams are acceptable. The local test webcam is the Logitech C920e.
- In Both mode, when optical control is enabled, support acknowledgment of files and/or individual frames and remove acknowledged content from rotation according to the configured ACK options.

## Windows build and local test

- Build a native Windows executable that can be tested with the user's webcam.
- Run the same executable twice, once as sender and once as receiver, with independent instance state.
- Transfer a 2 KiB image and verify the received bytes.
- Provide a launch flag to select each instance's MCP port.
- Keep MCP off by default. Enable it explicitly with `--enable-mcp` or the saved setting. Specifying `--mcp-port` alone must not enable MCP.
- Resolve the observed localhost connection-refused failure in the distributable desktop build.

Current verification: native Windows executable, CLI and NSIS installer were built; two instances were launched; the received 2,048-byte fixture matches the original SHA-256. See the verification record for exact results and remaining hardware checks. MCP settings currently take effect after restarting the instance.

## Sending and QR display

- Scale the detached QR code when its window is resized.
- Keep the area outside the QR code dark/black, retaining the white quiet zone around a normal QR code.
- Do not insert a "Display standing by" frame into a running QR cycle.
- Put common send settings, including frame rate, in an accordion on the Send page.
- Start the Send controls accordion collapsed by default.
- Apply send-control changes immediately to queued transfers and the detached display, including when its current frame is paused.
- Remove the fullscreen button beside Detach display on the Send screen. The detached window keeps its fullscreen control.

## Camera reception

- Investigate and improve the visibly slow receiver preview, which appeared to run at approximately 1 FPS.
- Show meaningful capture and scanning rates separately from sender display rate.

Current implementation separates preview polling from transfer-state polling and scans QR frames on a separate worker. Local measurements were approximately 15 FPS for capture and scanning; preview updates are bounded to at most 12.5 FPS. These are measured/implemented behaviors, not a guarantee for all cameras.

## Payload inspector

- Replace the widely stretched hex cells with compact cells and fit substantially more bytes per row.
- Let the user configure row width and use more of the available panel width.
- Persist the chosen width. The implemented choices are Auto and 16–128 bytes per row in increments of 16, with an optional ASCII column.
- Fix jumping when manually scrolling, especially in Text mode with Follow active bytes disabled.
- Retain the existing Follow active bytes checkbox; the user clarified that a duplicate auto-scroll checkbox was not needed.
- Give text rows their own correct byte offsets. For example, a view starting at `00000000` must also show the subsequent row at `000000c0` when that offset falls within the visible rows.

## Page organization and appearance

- Break Settings into tabs.
- Use recognizable sun/moon icons for the top-right theme switch; the previous sun glyph resembled a settings cog.
- Add up/down controls to reorder page sections/control groups and persist their positions.
- Save ordering independently for Dashboard, Send, Receive, Settings and Automation. In Settings, group ordering determines tab order.
- Remove the sidebar copy "No cloud. No account. Just a screen and a camera."
- Remove the sidebar label "OFFLINE BY DESIGN".
- Remove routine loop-completion entries from user-facing History so they do not overwhelm significant events. The complete automation event stream remains available.

## Related files

- [Original implementation specification](implementation-spec.md)
- [Architecture and presentation behavior](architecture.md)
- [Verification results and remaining limitations](verification-record.md)
- [Windows/native build instructions](building.md)
- [MCP configuration and instance flags](mcp.md)
- [Windows artifact checksums](../artifacts/windows-x64-SHA256SUMS)
