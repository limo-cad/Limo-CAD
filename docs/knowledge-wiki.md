# Knowledge bundle (OKF)

The `knowledge/` directory is an
[Open Knowledge Format v0.2](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
bundle: Markdown concepts with YAML frontmatter for humans and agents.

- Browse in-repo: start at [`knowledge/index.md`](../knowledge/index.md)
- Hosted: [knowledge site](https://limo-cad.github.io/Limo-CAD/) via `.github/workflows/pages-knowledge.yml`
- Prefer **`cad_help`** (`search` → `get` / `topics`) for discovery; then
  `resources/read` on `limo-cad://knowledge/...` when the full page is needed
  (start at `limo-cad://knowledge/index.md`). Prefer bundled markdown over scraping Pages HTML.
- Keep concepts **thin**; longer factual and proposed design stays in
  [`mcp-harness.md`](mcp-harness.md) and
  [`proposed-architecture.md`](proposed-architecture.md)
- Tracking epic: [#9](https://github.com/limo-cad/Limo-CAD/issues/9)

Validate the bundle locally with:

```sh
cargo xtask knowledge check
```

The Pages site is intentionally a thin landing page over the source bundle.
A richer viewer can be added later without changing `knowledge/` as the source
of truth.

The MCP inventory is derived from every `knowledge/**/*.md` file during compilation
(`crates/help/build.rs`), including the index and update log; `cad_help` search
covers the Concept pages of that same bundle. Reads return the compiled Markdown
unchanged, without
network requests, arbitrary file access, document mutation or an additional tool
surface. The immutable bundle does not advertise subscriptions or list changes;
rebuild the server when updating it. Links outside `knowledge/` point to supporting
repository documents and are not separately served as MCP resources.

## Mechanical-design guidance

Start at the [OKF index](../knowledge/index.md) and
[machine-design taxonomy](../knowledge/machine-design/taxonomy.md) for the seeded
Concept set (fits, GD&T, DFM/DFAM, fasteners, materials vocabulary, CAD-program
ops). Product concepts (gears, workholding, bearings, wind) live under
`knowledge/concepts/`.

Prefer `cad_help` search → get by id; use `resources/read` on a chosen
`limo-cad://knowledge/...` URI for the full page. Use `cad_interface` with
`action: recipes` to inspect the recipe catalog before selecting a referenced
example. Articles do not run commands or replace the current document.

## Maintaining the content

Write guidance that helps a reader make a concrete design decision. Include an
example or check where useful. Keep assumptions, units and the boundary between
digital validation and physical evidence explicit. Remove empty topic pages;
the index should describe content a reader can use now.

For the mechanical-design articles, `sources` and `related_recipes` are
comma-separated stable IDs (`[]` means no related recipes). Source IDs resolve
through [SOURCES.md](../knowledge/machine-design/SOURCES.md); recipe IDs resolve
through the committed recipe catalog. Cite primary references beside the material
they support, with author, title, link, license and an adaptation notice where
applicable. Record only references the articles actually use.

Check each source's reuse terms before adapting it. Keep required credit and
license notices; the repository license does not erase a source's attribution
requirements. Link to proprietary standards and material with incompatible or
unclear reuse terms rather than copying it. Do not reproduce standards tables,
vendor datasets or third-party figures without the necessary permission.

`cargo xtask knowledge check` validates structure, source metadata, recipe paths and
case-sensitive local links. Repository source URLs are checked against their linked
Git revision, including historical tags and branch names containing slashes. Fetch
the linked refs before checking a shallow checkout; Pages and the Windows MCP core
job fetch all refs because both run the real bundle test. The Linux workspace job
also fetches all refs. Other MCP shards retain shallow checkouts.
The checker stays offline and caches each source tree during a run.
Its fixture tests run in the existing Pages job;
native MCP tests check that every Markdown file is served unchanged and recipe
references name published recipes. These checks do not establish factual accuracy,
license compatibility or physical fitness; review the article and cited source.

The Markdown corpus, MCP `cad_help` (BM25 via `limo-cad-help`), and
`limo-cad://knowledge/...` resources are one surface. Future desktop Help should
call the same crate — not a second corpus or ranker. Prefer Scripts deep-links
over a Bevy viewport inside Help.
