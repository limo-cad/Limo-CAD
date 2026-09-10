# Maintenance — MCP & disclosure

## Prerequisites (Windows)

```powershell
# After vcpkg install (see docs/WINDOWS_PACKAGING.md):
$env:OCCT_ROOT = "$PWD\vcpkg_installed\x64-windows"
$env:Path = "$PWD\vcpkg_installed\x64-windows\bin;$env:Path"
```

Point MCP clients at the release binary after build:

```text
.../mcp-server/target/release/nbcad-mcp
```

Example Cursor / VS Code config:

```json
{
  "mcpServers": {
    "nbcad": {
      "command": "/absolute/path/to/noBS-CAD/mcp-server/target/release/nbcad-mcp"
    }
  }
}
```

## Tests

```powershell
cargo test --manifest-path mcp-server/Cargo.toml
```

CI: `.github/workflows/mcp-server.yml` (Windows + vcpkg OCCT).
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

- Env: `NBCAD_SESSION_DIR` (else `%TEMP%/nbcad-sessions`)
- Layout: `<uuid>/{model.json,active-sketch.json?,focus.json,heartbeat.json}` (UUID v4 ids)
- Tauri owns each published document session and reserves publish generations before async export
- `cad_attach`: binds normal operations to the live owner; its snapshot read cache never writes the live model back
