# PLAN: JSONC script collections + round-trip export MVP

## Architecture constraints (do not violate)

1. **`limo-cad-script` is host-neutral** — no filesystem, no MCP session, no window. It only
   strips JSONC, validates, resolves `$ref`/`$select`/`$project`/`$count`, and sequences
   host calls. Filesystem loading stays in the host (`mcp-server/src/interface.rs` today).
2. **Version 1 scripts remain data + expressions only** — no embedded JS/shell. Includes
   are **composition of JSONC fragments**, not a programming runtime.
3. **`cad_script` stays the forward MCP tool-trace dump** —
   `{ calls: [{ name, arguments }] }`. Docs already say it is not a recipe and does not
   reverse-engineer feature history. **Do not overload or rename it** for JSONC export.
4. **Fast mode already skips presentation** (`RunOptions.presentation == false` skips
   `note`/`view` host calls while retaining modeling + checks). Collections must expand
   *before* run so included presentation steps behave the same.
5. **Scripts require blank document** and forbid nested session/transport ops
   (`cad_interface`, `cad_attach`, `cad_detach`, `cad_submit`, `cad_await_apply`).
6. Existing golden pattern already converts `tool_trace` → v1 steps in
   `command_script_replays_parametric_model_and_refuses_existing_work`
   (`mcp-server/src/lib.rs` ~4600). Export MVP should productize that path, not invent
   a second encoding.

## Goals for this PR slice

| # | Goal | MVP shape |
|---|------|-----------|
| 1 | Script collections / part isolation | Top-level `includes` → flatten fragment `steps`/`checks` into one document |
| 2 | Round-trip export distinct from `cad_script` | New `cad_interface` action `export_script` |
| 3 | Agent / fast path | Document + test that `mode: "fast"` + includes skips presentation |
| 4 | Tests + docs | Unit tests in `limo-cad-script`; MCP test for export→replay; docs |

Non-goals (explicit): full bi-directional UI feature-history → commented JSONC with
`$select`; rewriting INJS2065; Python factories; changing `cad_script` semantics.

---

## Part A — Script collections (`includes`)

### Schema (additive, still `version: 1`)

Root script may declare:

```jsonc
{
  "version": 1,
  "name": "Assembly root",
  "includes": [
    "collections/base.collection.jsonc",
    { "path": "collections/lid.collection.jsonc" }
  ],
  "steps": [ /* root-only mating / assembly after fragments */ ],
  "checks": [],
  "exports": {}
}
```

**Collection fragment** (not a full script — no `version` required):

```jsonc
{
  "name": "Base plate (optional, for error messages)",
  "steps": [ /* call | let | assert | note | view */ ],
  "checks": [ /* optional */ ]
}
```

Recommended filename suffix: `.collection.jsonc`. An included `.limo.jsonc` may be a
full script; only its `steps` and `checks` are composed. `version`,
`starting_state`, `verification`, `exports`, and `$schema` are ignored.

### Flatten semantics

1. Expand includes **in array order**, depth-first if nested (MVP depth cap **8**).
2. Concatenate:  
   `steps = concat(include₁.steps, …, includeₙ.steps, root.steps)`  
   `checks = concat(include₁.checks, …, includeₙ.checks, root.checks)`
3. Root owns `version`, `name`, `starting_state`, `verification`, `exports`.
4. Drop `includes` from the flattened document before `Script::parse` validation.
5. **One shared binding / step-id namespace** across all fragments + root. Duplicate ids
   fail preflight (existing check). Authors prefix ids (`base_stock`, `lid_sketch`, …).
6. Nested includes allowed in fragments with the same rules; **cycle detection** via
   stack of normalized relative paths.

### Path safety (host loader)

When expanding from a root **file path**:

- Include paths must be **relative** (reject absolute / drive letters / `\\`).
- Reject empty segments, `.`, and `..`.
- Resolve each include relative to the file that declares it, then join that
  root-relative path against the root file’s parent directory. After
  `canonicalize`, the resolved file must stay under that base directory.
- Extension must be `.limo.jsonc` or `.collection.jsonc` (case-insensitive).
- Reject once the running total of loaded source bytes exceeds `MAX_SCRIPT_BYTES`
  (16 MiB), not only after every fragment has been read.

Inline `source` with `includes` requires an absolute `include_base` directory.
Bundled recipes still cannot carry unresolved includes.

### Crate API (`crates/script`)

Add `crates/script/src/includes.rs` (module from `lib.rs`):

```rust
pub fn flatten_includes(
    source: &str,
    mut load: impl FnMut(&str) -> Result<String, String>,
) -> Result<String, String>;

pub fn parse_with_includes(
    source: &str,
    load: impl FnMut(&str) -> Result<String, String>,
) -> Result<Script, String>;
```

- `Script::parse`: if top-level `includes` is present and non-empty →  
  an error that tells the caller to open the script from its file path. No Rust
  API name belongs in that message.
- Export `strip_jsonc` as `pub(crate)` or keep private and reuse inside includes.
- No filesystem in the crate — `load` is caller-supplied.

### Host wiring (`mcp-server/src/interface.rs`)

Update `script_source` (or add `script_source_expanded`):

1. Load recipe / source / path as today.
2. If parsed skeleton has `includes`, require the selector was **`path`**, build loader
   rooted at `Path::parent(path)`, call `flatten_includes`, return flattened text.
3. `inspect_script` / `execute_script` / `preview_script` all see expanded source
   (preview still enforces 80-step / 2 MiB limits **after** flatten).

Do **not** allow a script step to `call` another script (`execute_script` already
rejects recursive script runs). Includes are compile-time composition only.

### Fast / agent path

- No new mode. Agents keep `"mode": "fast"` (default).
- Included `note`/`view` steps are still validated at parse time, then skipped at run
  when `presentation: false` — identical to monolithic scripts (existing test
  `fast_and_present_modes_execute_identical_modeling_and_checks`).
- Document: prefer putting teaching captions in collections; agents always use fast.

---

## Part B — Round-trip export (`export_script`)

### Surface (distinct from `cad_script`)

Extend **`cad_interface`** with action **`export_script`** (same home as `script` /
`recipes`). Do **not** add a parallel top-level tool in MVP unless catalog noise
requires it later.

```json
{
  "action": "export_script",
  "name": "Optional human title",
  "from": "auto"          // "auto" | "last_script" | "session_trace"
}
```

Response:

```json
{
  "format": "limo.jsonc",
  "version": 1,
  "fidelity": "lossless_authored" | "lossy_session_trace",
  "source": "{\n  \"version\": 1, ...\n}\n",
  "name": "...",
  "step_count": 12,
  "notes": [
    "lossy_session_trace embeds literal IDs/arguments from this process; prefer authored $select for durable recipes.",
    "cad_script remains the forward MCP call dump; this action emits version-1 JSONC."
  ]
}
```

### Fidelity rules (document honestly)

| Source | Fidelity | When |
|--------|----------|------|
| Retained last successful `action:script` source (post-includes flatten **or** original path text — prefer **flattened** for self-contained export; optional later flag to keep includes) | **lossless_authored** for geometry/commands/refs that were in that source | Document was built (or last rebuilt) from a script in this process |
| `tool_trace` → call steps via `group_for(name)` (same as existing unit test) | **lossy_session_trace** | Headless MCP mutates from blank; or no retained script |
| UI-only edits / attach baseline `cad_load_project_model` | **Not reconstructed** into feature-faithful JSONC in this PR | Skip `cad_load_project_model` entries; if trace is *only* baseline, return error asking for blank-session construction or last_script |

**Lossless** means: replaying exported `source` on a blank doc with `mode:fast`
reproduces the same modeling command sequence and authored refs. Comments may be
absent if export re-serializes JSON (MVP may emit compact JSON without comments —
document as comment-lossy even for `lossless_authored` if we re-serialize; better:
retain original flattened text string for last_script).

**Lossy** means: may use literal entity IDs; no `$select`/`$project`; no notes/views;
may fail to replay after topology changes; not a substitute for hand-authored recipes.

### Implementation sketch

1. `CadServer` fields:
   - `last_script_source: Option<String>` — set when `execute_script` completes **Ok**
     (store the expanded source actually executed).
   - Clear on `cad_load_project_model` / attach baseline seed? Prefer: clear
     `last_script_source` when `seed_script_baseline_from_model` runs (model no longer
     matches that script), and when a non-script mutating path succeeds after script
     (optional strictness: MVP clears only on attach/load baseline).
2. `fn export_script(&mut self, arguments: &Value) -> Result<Value, String>`
3. Helper (mcp-server or small fn in interface):

```rust
fn session_trace_to_v1_source(calls: &[Value], name: &str) -> Result<String, String> {
    let mut steps = Vec::new();
    for call in calls {
        let op = call["name"].as_str().ok_or("trace entry missing name")?;
        if op == "cad_load_project_model" { continue; }
        let group = interface::group_for(op).ok_or_else(|| format!("unmapped op {op}"))?;
        steps.push(json!({"call":{"group":group,"operation":op,"arguments":call["arguments"].clone()}}));
    }
    if steps.is_empty() {
        return Err("session_trace has no portable modeling calls to export".into());
    }
    Ok(json!({"version":1,"name":name,"starting_state":"empty","steps":steps}).to_string())
}
```

4. Update `cad_interface` tool description string to mention `export_script`.
5. Keep `cad_script` description unchanged aside from a one-line “see export_script
   for version-1 JSONC”.

### Recommended agent workflow (docs)

1. Author / compose `.limo.jsonc` (+ collections) as SoT.
2. Replay with `cad_interface {action:script, path, mode:fast}`.
3. Tiny live tweaks via MCP execute — **then either** re-edit JSONC by hand **or**
   accept `export_script` lossy_trace for a scratch replay (not for publishing).
4. Prefer updating the authored JSONC; use `cad_script` only for debugging the raw
   MCP call stream.

---

## Part C — Tests

### `limo-cad-script` (no OCCT)

1. `flatten_includes` concatenates steps/checks; root name preserved.
2. Cycle → error; `..` / absolute → error from loader contract tests.
3. Unresolved `includes` in `Script::parse` → error.
4. Fast mode: included `note` does not produce host presentation calls.
5. Duplicate id across root + include → parse error.

Fixtures under `crates/script/tests/fixtures/collections/`.

### `mcp-server` (engine)

1. Productize existing pattern: build via `mcp_box` / small mutates →
   `export_script` → fresh `CadServer` + `action:script` → equal body count /
   `cad_compare_solids` metrics (same spirit as `command_script_replays_…`).
2. `execute_script` success → `export_script` `from:last_script` returns
   `fidelity: lossless_authored` and same step_count.
3. `cad_script` still returns `{calls:…}` shape unchanged (regression assert).
4. Path-based script with includes loads and runs under `mode:fast` (temp dir fixture).

---

## Part D — Docs

1. **`docs/native-scripts.md`** — new sections:
   - Script collections (`includes`)
   - Exporting version-1 JSONC (`export_script` vs `cad_script`)
   - Lossless vs lossy table
   - Agent rebuilds: always `mode:fast`; presentation is optional teaching chrome
2. **Short agentic note** — `docs/agentic/jsonc-workflow.md` (1–2 pages): SoT = JSONC,
   compose parts via includes, replay fast, export caveats.
3. Schema: `examples/scripts/limo-cad-script.schema.json` — add `includes` + fragment
   `$defs/collection`.
4. Optional example: `examples/scripts/collections/` minimal split of a toy part
   (keep fillet-basics monolithic; add `compose-two-boxes` or similar tiny demo).

---

## File touch list (Thunder)

| Path | Change |
|------|--------|
| `crates/script/src/includes.rs` | **NEW** — flatten/parse_with_includes |
| `crates/script/src/lib.rs` | mod includes; reject unresolved includes in `parse` |
| `crates/script/tests/fixtures/collections/*` | **NEW** fixtures |
| `examples/scripts/limo-cad-script.schema.json` | `includes` + collection def |
| `examples/scripts/collections/*` | optional tiny demo |
| `mcp-server/src/interface.rs` | expand includes when `path` load |
| `mcp-server/src/lib.rs` | `export_script` action; retain `last_script_source`; tests |
| `docs/native-scripts.md` | collections + export + fast/agent |
| `docs/agentic/jsonc-workflow.md` | short agent note |
| `crates/script/README.md` | one paragraph on includes |

## Apply order for parent

1. Land `includes.rs` + `lib.rs` reject + unit tests (green `cargo test -p limo-cad-script`).
2. Schema + docs (can parallel).
3. `interface.rs` path expansion.
4. `export_script` in `lib.rs` + MCP tests.
5. Draft PR description quoting lossless/lossy table; link brief.

## Open choices (defaults chosen for MVP)

| Question | MVP default |
|----------|-------------|
| Keep includes in exported last_script? | Export **flattened** self-contained source |
| Nested includes? | Yes, depth ≤ 8 |
| Fragment may include full script metadata? | Ignore fragment `version`/`exports`; only `steps`/`checks`/`name` |
| Clear last_script on any post-script mutate? | Clear on attach/load baseline only in MVP |
| New top-level MCP tool? | No — `cad_interface` action only |

