# Maintenance — MCP & disclosure

## Setup and tests

Use [DEVELOPMENT.md](../DEVELOPMENT.md) for native SDK/runtime setup and the
sequential MCP test command. Manual end-user client configuration belongs in
[INSTALL.md](../INSTALL.md#connect-an-mcp-agent); the
[standalone development installer](INSTALL_MCP.md) documents source-built servers.

CI: `.github/workflows/mcp-server.yml` (Windows and Ubuntu with native OCCT).
Pinned vcpkg checkout must use `fetch-depth: 0` (versioned port trees fail on shallow clones).

## Adding an MCP tool

1. Register `ToolSpec` in `mcp-server/src/lib.rs` `tool_specs()`.
2. Add pack tags in `disclosure::tags_for_tool` (and `auto_focus_for_tool` if needed).
3. Update `MODELING_TOOL_COUNT` / pack count assertions if it is a modeling tool.
4. Add or extend a focused regression under `#[cfg(test)]` in `mcp-server/src/lib.rs`.
5. Keep the shared operation catalog and `docs/mcp-harness.md` current.
6. Run the test suite with OCCT DLLs on `PATH`.

## Disclosure knobs (defaults)

| Knob | Default |
|------|---------|
| Throttle | 300 ms |
| Soft TTL | 60 s |
| Soft LRU | 2 packs |
| Re-promote | 15 s |
| Default focus | `document` |

## Snapshot bridge sessions

- Env: `LIMO_CAD_SESSION_DIR` (else `%TEMP%/limo-cad-sessions` on Windows; `<temp>/limo-cad-sessions-<effective-user-id>` on Unix, private to that user)
- Layout: `<uuid>/{model.json,active-sketch.json?,focus.json,heartbeat.json}` (UUID v4 ids)
- The native host owns each published document session and reserves publish generations before async export
- `cad_attach`: binds normal operations to the live owner; its snapshot read cache is read-only toward the live model
