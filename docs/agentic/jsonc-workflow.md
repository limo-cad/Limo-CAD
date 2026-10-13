# Agent design workflow: MCP first

The saved `.limo` project is the working design. Drive construction, feature
edits, inspection, review views, and persistence through MCP from start to finish.
Continue editing existing history; agents do not need a parallel presentation copy.

## Working loop

1. Check `cad_session_status`. Select the user's desktop explicitly with
   `cad_list_sessions` and `cad_attach`, or use an independent headless document.
   Preserve existing work; create a new project only when a new design is intended.
2. Discover with `cad_interface` `catalog`, `cad_help`, or `cad_list_all_tools`.
   Call `cad_interface` `execute` with the operation's group and arguments, or
   use the individual MCP tool. Attached execution submits and awaits internally.
3. Begin, constrain, and finish sketches; build features with returned IDs.
   Inspect `solid_scene` and `cad_document` between edits. Use `solid_edit_*`
   for changes and re-read topology IDs after recomputation.
4. Use `upsert_named_view`, `rename_named_view`, `delete_named_view`,
   `recall_named_view`, and `clear_named_view` in `document/appearance`.
   Attached `cad_interface` `inspect` returns `view_state`: copy its `camera`,
   `visible_body_ids`, and `part_offsets` plus a chosen `name` into upsert.
   Do not pass the extra inspection fields. A null camera means no modeling
   viewport is available; headless clients supply camera coordinates directly.
5. Save via desktop `cad_interface` with `action: file`, `command: save`, and
   an absolute `.limo` `path` (set `overwrite: true` only to replace that file),
   or retain the exact headless `cad_project_model` string and restore with
   `cad_load_project_model`.
   Reopen/restore and inspect the model and `named_views` before handoff.

For live document Undo/Redo, use `cad_interface` `action: history` with
`command: undo` or `redo`. Inspect `state.history` for availability. This uses
the same history controller as the keyboard and Edit menu.
Check live UI replies for `status: applied`; failed or timed-out receipts are
not completed actions. Inspect the current state before retrying.

View offsets affect display only. Metadata edits clear the active view; recall
it to display the updated configuration. Clear preserves current visibility.

## Optional recipe rebuild loop

Use this only for explicitly requested teaching, reusable recipes, or replay.
Scripts do not supersede the saved project after interactive or MCP edits.

1. Edit the root `.limo.jsonc` (and optional `collections/*.collection.jsonc`).
2. Replay on a **blank** document:

```json
{
  "action": "script",
  "path": "/absolute/path/design.limo.jsonc",
  "mode": "fast",
  "validate": true
}
```

3. Inspect the result. Continue design iteration through MCP feature edits.
4. For an authored recipe revision, prefer hand-authored `$select` / `$project`
   over literal entity IDs. Normal design edits do not require updating a recipe.

`mode: "fast"` (the default) skips `note` / `view` presentation while keeping
every modeling call and check — including steps contributed by collections.

## Collections

Root scripts may declare `includes` so each part lives in its own fragment:

```jsonc
"includes": ["collections/base.collection.jsonc", "collections/lid.collection.jsonc"]
```

Fragments supply `steps` (and optional `checks`). An included `.limo.jsonc`
contributes the same steps and checks; its version, starting state, verification,
and exports are ignored. Ids share one namespace — prefix them per part.

Include paths are relative to the file that contains the `includes` entry. A
fragment in `collections/` that includes `b.collection.jsonc` loads
`collections/b.collection.jsonc`. Paths cannot contain `..`. Loading still
requires a **path**-loaded root, or inline `source` with an absolute
`include_base`. Nested includes are allowed with cycle detection.

## Export vs `cad_script`

| Tool / action | Output | Use |
|---------------|--------|-----|
| `cad_interface` → `export_script` | Version-1 `.limo.jsonc` `source` string | Scratch round-trip / retain last authored script |
| `cad_script` | `{ calls: [{ name, arguments }] }` | Debug the forward MCP mutate stream |

`export_script` fidelity:

- **`lossless_authored`** — last successful `action: script` source in this process
  (commands/refs preserved; comments may be missing if includes were flattened).
  Pretty-printed. `stale: true` means later tools ran; the text is still that script.
- **`lossy_session_trace`** — rebuilt from `tool_trace` with literal arguments; no
  `$select`, no teaching notes; not for published recipes.

`from: auto` returns the authored script only while no modeling tool has
succeeded since that run; live desktop edits count as well. After later tools
(for example `solid_box`), `auto` exports the session trace instead.
`from: last_script` still returns the authored source, with `stale: true`.

UI-only edits and attach baselines (`cad_load_project_model`) are **not** turned
into faithful JSONC in this MVP. Document gaps rather than inventing history.

## Do not

- Treat `cad_script` as a recipe.
- Run scripts over a non-empty document.
- Rely on presentation mode for agent CI rebuilds.
- Treat a stale `lossless_authored` export as the current model. Use `from: auto`
  after further tool calls.
