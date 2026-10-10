# Native desktop index

The `desktop` Cargo workspace hosts the native Bevy app and its MCP interface.

| Path | Role |
|------|------|
| [src/lib.rs](src/lib.rs) | Native host and engine dispatch |
| [src/session_bridge.rs](src/session_bridge.rs) | Per-window UUID + reload-safe atomic snapshot publish for MCP |
| [Cargo.toml](Cargo.toml) | Native shell crate |

Session layout: `<LIMO_CAD_SESSION_DIR>/<uuid>/{model,focus,heartbeat}.json`.
See [../docs/agentic/MAINTENANCE.md](../docs/agentic/MAINTENANCE.md).
