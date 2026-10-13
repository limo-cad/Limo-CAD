# Steerable MCP — operating rules

## Invariants (do not break)

1. **Disclosure is guidance, not a jail.** Never reject `tools/call` with “not in focus.”
2. **Hard errors** = missing IDs, invalid sketch state, kernel failure only.
3. **Notification name** must stay exactly `notifications/tools/list_changed`.
4. **Stdout** = JSON-RPC only; logs on **stderr**.
5. **Headless goldens** must work without `cad_attach`.
6. **Offline/local** invariant; stdio is current transport, not forever law.
7. When adding a modeling tool: update `tags_for_tool` in `disclosure.rs` **and** the pack matrix test.

## Modes

| Mode | Who | Behavior |
|------|-----|----------|
| `dynamic` (default) | Main agent / human | Spine ∪ active ∪ soft packs |
| `full_static` | Subagents / broken clients | Advertise all tools |

Prefer `cad_list_all_tools` for planners over leaving the main session in `full_static`.

## Focus packs

`document | assembly | sketch | solid | modify | body_ops | datums | history | inspect | print | cam`

Keep `disclosure::tags_for_tool` aligned when adding dialogs or export tools.

## Snapshot bridge (honest scope)

`cad_list_sessions` / `cad_attach` / `cad_refresh` / `cad_detach` implement a
**read-only snapshot bridge** under `LIMO_CAD_SESSION_DIR`:

- session ids are **UUID v4** (document names rejected);
- The native host owns one UUID per desktop window and publishes
  `<uuid>/{model.json,active-sketch.json?,focus.json,heartbeat.json}` with pre-export generation reservations;
- attach **fails** if `model.json` is missing or invalid;
- MCP **never** writes the session files back after editing in memory;
- refresh is explicit (no filesystem watcher);
- this is **not** a live UI co-link / LWW writeback.

Revisioned MCP→UI sync remains future work. Installer / UI launch: [#32](https://github.com/limo-cad/Limo-CAD/pull/32).


## Print export

Prefer `solid_export_3mf` for slicers. `standard`, `bambu_studio`, and
`orca_slicer` write the same portable model (mesh, material name, display
color). Prusa and Cura add that slicer's metadata. None of these is a full
pre-sliced project. STL is geometry-only.

## Related reading

- [mcp-harness.md](../mcp-harness.md)
- Issues [#10](https://github.com/limo-cad/Limo-CAD/issues/10), [#11](https://github.com/limo-cad/Limo-CAD/issues/11)
- MCP tools / listChanged: https://modelcontextprotocol.io/specification/2025-06-18/server/tools
