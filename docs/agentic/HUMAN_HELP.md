---
title: How humans find help today
status: draft
updated: 2026-09-20
---

# How humans find help today

Short operating note for Design Ops and contributors. **No full Tauri Help
panel yet** — humans and agents share one Markdown corpus; doors differ.

## Doors (same corpus)

| Door | What a human does | Same crate / data |
|------|-------------------|-------------------|
| **OKF index** | Open [`knowledge/index.md`](../../knowledge/index.md) — topic list → page titles | Source of truth |
| **Taxonomy** | Open [`knowledge/machine-design/taxonomy.md`](../../knowledge/machine-design/taxonomy.md) — seeded vs planned map with links | Source of truth |
| **MCP `cad_help`** | In an MCP client: `topics` → labels; `search` → snippets; `get` by id | `limo-cad-help` |
| **MCP resources** | `resources/list` / `resources/read` on `limo-cad://knowledge/...` (start at `limo-cad://knowledge/index.md`) | Embedded at MCP build |
| **GitHub Pages** | Browse hosted knowledge HTML (when published) | Same markdown; prefer `cad_help` / resources for automation |
| **Checkout** | Read files under `knowledge/**` in a clone | Same files |

Future **desktop Help** should call the **same** `limo-cad-help` crate (search /
get / topics) and render the same pages — not a second corpus. Until that UI
lands, the index + taxonomy + MCP/Pages are the human browse path.

## Scripts / demos (unchanged)

Recipe chips and presentation deep-links still open **Scripts** /
`.limo.jsonc` demos. Help does **not** embed a Bevy viewport. Agents and
humans use the same recipe ids from page frontmatter (`related_recipes`).

## Shared corpus, different doors

- **Browse:** start at index or taxonomy; follow links; optional Pages.
- **Search / automation:** prefer `cad_help` `search` → `get` (id-only); use
  `topics` to discover labels; `resources/read` only for the chosen full page.
- One shared corpus; every door points at the same pages.

## MCP prompts

As of 2026-09-20, `limo-cad-mcp` advertises `prompts` and ships one template:
**`help_search`** (optional argument `query` / alias `context`). It steers
agents to form a contextual query and call `cad_help` (`search` → `get` 1–2
ids; optional `resources/read` for the full OKF page). Design-flow prompts
(validate-before-show, research-before-commit) remain **knowledge pages +
agent skills** for now — deferred as separate MCP prompts. Doctrine lives in
the knowledge corpus (e.g. agent MCP workflow concepts).

## Rebuild reminder

Knowledge embeds at MCP **build** time. After corpus changes, rebuild and
reinstall from the repository root:

```bash
cargo xtask install-mcp --clients cursor
```

See [`INSTALL_MCP.md`](INSTALL_MCP.md). Reload/re-Add the MCP client after a
binary replace; Restart alone can leave a stale server.

## Worked example (human via `cad_help`)

Goal: find a **fit coupon** demo after reading enclosure join guidance.

1. **`topics`** (optional) — note labels such as `enclosures`, `dfam`, `recipes`,
   `joints`.
2. **`search`** — query e.g. `when not to snap glue screw` or
   `fit coupons recipes map`. Expect near-top hits:
   - `machine-design.concepts.am-assembly-join-choice`
   - `machine-design.concepts.fit-coupons-recipes-map`
3. **`get`** — fetch `machine-design.concepts.fit-coupons-recipes-map` (id-only).
   Read the table: Concept → recipe ids (`turbine-fit-coupons`, `mounting-plate`, …).
4. **Scripts** — open the chip / recipe id from frontmatter `related_recipes`
   (same ids as on the Concept page). Help does not show Bevy; Scripts is the
   geometry door.
5. Optional: `get` a teaching page first (e.g.
   `machine-design.concepts.am-enclosure-lid-gasket-labyrinth`), then follow its
   `related_recipes` the same way.
6. CAD-program ops: search `inspect between mutates`, `edit history not delete-rebuild`,
   `3MF vs STL`, `unit systems mm`, or `datum sketch plane` for agent-ops Concepts.

Automation uses the identical `search` → `get` → recipe path; browse may
instead click links from [knowledge/index.md](../../knowledge/index.md) or
[taxonomy](../../knowledge/machine-design/taxonomy.md).

