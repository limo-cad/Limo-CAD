# Limo CAD knowledge update log

## 2026-10-07

- Challenged the session audit directly: separated artifact permissions from
  repository-level evidence, recorded inspected source hashes and the unresolved
  faer notice, and removed unsupported manual/figure licensing implications.
- Tightened detent guidance for peak insertion/removal travel, minimum holding
  force, constant-stiffness assumptions and tensile stress versus layer planes.
- Added precise benchmark conditions/result links; reconciled copying policy,
  the DOE link-only classification and stale branch maintenance wording.

## 2026-10-06

- Promoted general simulation verification, installed preload/tolerance,
  bending-axis, slicer-path and material-revision lessons from the preserved
  research into searchable Concepts. Numerical and private part evidence
  remain in their repositories; no geometry was changed.
- Added primary research/library/material references with artifact-specific
  reuse decisions. Copying licensed content is permitted with its notices;
  citations alone do not authorize copying proprietary material.
- Corrected blanket public-domain labels for the mixed-source DOE deck.
- See the repository's `docs/analysis/SESSION-KNOWLEDGE-AUDIT.md` for the audit,
  priority recommendations and remaining rights-verification gaps.

## 2026-09-21

### critical: clamshell teeth↔cutouts + heat stake insert

- Deepened `am-clamshell-retainer` mating-half CAD roles (tooth↔receptacle, tooth
  count, envelope span); keywords lock `clamshell teeth cutout` / `matching cutouts clip`.
- Heat-set: keyword + opening phrase `heat stake insert` (counterbore stays #2, not top).
- Goldens H107–H109; Rust BM25 top_id; rebuild search-index; MCP reinstall.

## 2026-09-20

### dogfood round 2 (heat-stake boss ranking)

- Proven WEAK: `heat stake boss` / `heat-set boss` ranked `am-boss-standoff-patterns`
  ahead of `am-heat-set-inserts` (stake≠set token; boss page keyword `heat-set boss`
  stole heat+boss queries).
- Fix: heat-set opening leads heat-stake / heat-set insert-in-boss; keywords add
  stake synonyms; boss-standoff drops `heat-set boss` keyword (keeps body link).
- Goldens H105–H106; Rust BM25 top_id asserts; rebuild search-index; MCP reinstall.

### quality pass (snippet honesty + Prefer cruft)

- AGMA/chart queries: openings on `gears`, `am-printed-gears-dfam`, and
  `mechanisms-overview` now lead with **no chart in Help / VERIFY datasheet**.
- `materials-vocabulary`: opening leads with E/Sy/Sut + MatWeb honesty; merged
  duplicate Prefer / Preferred callouts / Open data into one Prefer table +
  shorter allowables honesty.
- `am-snap-fit`: opening leads with class / thickness / orientation before sizing.

### dogfood weak areas (bearing seat ranking + chart honesty)

- **Bearing seat ranking:** retitled/keywords `bearings-hubs-seats` for singular
  bearing seat / bore / hub seat; shifted `bearing-stacks` to purchased SKU /
  collar / shield language so seat geometry queries top hubs-seats.
- **AGMA / tooth-chart phrasing:** gears + printed-gears + mechanisms-overview
  openings lead with roles-only / datasheet VERIFY (no invented charts).
- **topics browse:** index + agent-mcp-workflow point agents to taxonomy/index
  for curated browse (topics stays alpha labels).
- Goldens H100–H104; Rust BM25 queries; rebuild search-index; MCP reinstall.

### dogfood slop purge (prefer tone leftovers)

- Folded leftover `## Anti-patterns` on mechanisms-overview + design-hygiene into
  Prefer tables (golden-path; no negative-bias lecture headers).
- Renamed `## What not to do` on am-printed-gears-dfam + am-fdm-load-layers-infill
  to Prefer-instead tables.
- No page deletes; no MatWeb / tooth charts invented.

### Materials vocabulary deepen (CAD-time props)

- Deepened `machine-design.concepts.materials-vocabulary`: CAD-time roles for
  **E / Sy / Sut / hardness / fatigue / CTE / density / corrosion**, isotropic vs
  anisotropy / print-vs-isotropic, grade/temper/condition; golden path +
  CAD-vs-datasheet ownership; DOE `doe-3d` + NWTC `nwtc-guns-dfm` + KittyCAD /
  Materials Project patterns — **no** allowables / MatWeb scrapes (selection
  tables stay planned).
- Taxonomy D → seeded (vocab / roles distill); Still-thin → allowables planned.
  Index quick-lookup + bullet. Goldens H97–H99; Rust BM25 queries.
- Rebuilt search-index.json; MCP embed via install script (no Cursor Uninstall/Add).

### Mechanisms overview hub deepen

- Deepened `machine-design.concepts.mechanisms-overview`: CAD-time vocabulary
  (motion class / element family / soft sync / CAD-vs-catalog), navigate-by-need
  table to child Concepts, prefer/anti-pattern tables, golden-path step to open
  the right child; MIT OCW `mit-272` + DOE buy-before-invent — **no** tooth /
  cam / belt / Geneva-slot charts.
- Taxonomy C → seeded (hub navigation / roles distill); Still-thin → charts stay
  datasheet. Index quick-lookup + bullet. Goldens H94–H96; Rust BM25 queries.
- Rebuilt search-index.json; MCP embed via install script (no Cursor Uninstall/Add).

### Design hygiene requirements/BOM deepen

- Deepened `machine-design.concepts.design-hygiene-requirements-bom`: make-vs-buy
  per body, BOM roles (purchased / fab / phantom / fastener kit), reference
  designator tree↔BOM↔balloon, VERIFY table + freeze-too-early anti-patterns;
  DOE Module 3D standardize / buy-before-invent distill (`doe-3d`) plus NWTC /
  PALNI DFM–DFA and `nasa-fastener` for purchased hardware lines — **no** ERP /
  Stage-Gate lecture.
- Taxonomy F → seeded (requirements/BOM distill); Still-thin → ERP dumps out.
  Index quick-lookup + bullet. Goldens H91–H93; Rust BM25 queries.
- Rebuilt search-index.json; MCP embed via install script (no Cursor Uninstall/Add).

### Fasteners preload / torque deepen (NASA RP-1228)

- Deepened `machine-design.concepts.fasteners-joints`: preload / clamp load,
  torque as install method vs proof, friction/lube / *K* sensitivity, grip &
  engagement, proof-load and running-torque roles; **no** invented torque charts
  — VERIFY datasheet or NASA RP-1228 (`nasa-fastener`).
- Taxonomy B Fasteners → seeded (preload/torque distill); Still-thin row → deep
  charts stay datasheet. Index quick-lookup + bullet; clearance soft pointer;
  SOURCES note. Goldens H88–H90; Rust BM25 queries.
- Rebuilt search-index.json; MCP embed via install script (no Cursor Uninstall/Add).

### Technic-style envelope (LDraw LDU nominals)

- Deepened [Technic-style envelope](machine-design/concepts/technic-envelope.md) with open-source **LDraw LDU** brick/stud/Technic nominals (1 LDU ≈ 0.4 mm; stud pitch 20 LDU / 8 mm; brick 24 LDU / 9.6 mm; plate 8 LDU / 3.2 mm; stud/hole class 12 LDU / 4.8 mm; axle length modules N×8 mm) plus MIT Technic.scad print-oriented hole bias; kept unofficial / trademark / measure-first tone.
- Added SOURCES ids `ldraw-ffs`, `ldraw-opls`, `technic-scad` (distill) and link-only `stegu-ldu`, `cailliau-lego-dims`.
- Help goldens H85–H87 for LDU / stud pitch / Technic hole diameter discoverability.
- Spec↔MCP align: ADR 0006 index → Accepted; `mcp-harness` cad_help-first.

### Shared reference geometry (followers / named parents)

- Seeded `concepts.shared-reference-geometry`: prefer named shared references
  (planes, axes, sketches, faces) over duplicated numeric offsets; drive
  related surfaces from the same reference; JSONC/history reference-first;
  assembly locating-scheme analog; datum/sketch-plane + geometry-naming
  cross-links; VERIFY after param change via solid_scene / compare / section.
- Soft pointers: geometry-naming, datum-sketch-plane-choice; index + taxonomy H;
  Design Ops guidance; goldens H82+ + Rust BM25 queries.

### Geometry naming (bodies / faces / scripts / STEP)

- Seeded `concepts.geometry-naming`: role-noun bodies, feature/history names
  aligned with script steps, name mating/datum/export-critical faces/edges;
  JSONC args/comments/set_name + VERSION/section headers/chunks; prefer
  name-preserving STEP/export; one vocabulary script ↔ browser ↔ STEP.
- VERIFY: solid_scene / cad_document shows intended names before next write.
- Soft pointers: agent-mcp-workflow, design-version-scripts; index + taxonomy H;
  Design Ops guidance; goldens H79+ + Rust BM25 queries.

### Design VERSION / JSONC script golden path (correction)

- Rewrote `concepts.design-version-scripts`: authoritative artifact is
  versioned / VERSION-embedded `.limo.jsonc` — **not** a Python `gen_v*.py`
  generator. Prefer hand-authored JSONC chunks.
- Working designs (INJS): **version in filename AND inside JSONC**
  (`design_vM_N.limo.jsonc`). Catalog demos may keep stable unversioned ids
  with VERSION only in metadata.
- Cut/prune prior `design_v*.limo.jsonc` (+ leftover `gen_v*.py`). Chunking /
  section headers for edit-tool-sized hunks; chaptered includes preferred
  when/if supported.
- Soft pointers: agent-mcp-workflow, index, taxonomy, docs/agentic INDEX;
  Design Ops guidance; goldens H76–H78 + Rust BM25 queries retargeted.

### Bearings / hubs / seats CAD-time deepen

- Seeded `machine-design.concepts.bearings-hubs-seats` (shaft/housing seats,
  fit roles, preload/spacer stacks; load/speed/life as VERIFY→catalog; **no**
  invented L10 tables). Cite `nasa-bearing` + `mit-272`; cross-link shafts,
  mechanisms hub, fits, product `concepts.bearing-stacks`.
- Taxonomy B Bearings / hubs / seats → **seeded** (primary CAD-time id; product
  SKU page retained). Still-thin: deep L10/capacity charts stay datasheet.
- Index table + bullets; light pointer on bearing-stacks; mechanisms/shafts/
  springs/fits/fit-coupons cross-links.
- Rebuilt search-index.json; Rust embed + BM25/unit tests; wire goldens H73+.

### Design hygiene BOM + intermittent/Geneva

- Seeded `machine-design.concepts.design-hygiene-requirements-bom` (requirements
  → embodiment → purchased vs print → BOM roles; golden-path checklist; link
  research-before-commit + hardware-pocket; no process lecture).
- Seeded `machine-design.concepts.mechanisms-intermittent-geneva` (index/dwell/
  lock-arc roles; envelopes; VERIFY purchased indexer or analyzed cam; cite
  `mit-272`; link mechanisms hub + cams).
- Light thicken `inspection-metrology-bridge` (characteristic/balloon roles +
  handoff checklist; keep short).
- Taxonomy F → **seeded (partial)**; C intermittent/Geneva **seeded**; Still-thin
  + inspection row updated. Index table + bullets; cross-links from overview,
  cams, research-before-commit, hardware-pocket.
- Rebuilt search-index.json; Rust embed + BM25/unit tests; wire goldens H68+.

### Chains/sprockets + printed gears DFAM

- Seeded `machine-design.concepts.mechanisms-chains-sprockets` (center distance,
  wrap, tension path, purchased pitch; cite `mit-272`; link mechanisms hub +
  belts sibling).
- Seeded `machine-design.concepts.am-printed-gears-dfam` (orientation vs tooth
  load; min tooth vs nozzle; backlash as coupon; cross-link gears + DFAM hub +
  load-layers; **no** module strength tables).
- Skipped optional NASA preload Concept — fasteners-joints already carries
  checklist distill; deep numeric tables remain Still-thin.
- Taxonomy C/E + Still-thin; index table + bullets; cross-links from overview,
  belts, gears, DFAM hub, load-layers. Rebuilt search-index.json; Rust embed +
  BM25/unit tests; wire goldens H64+.

### Mechanisms corpus (hub + linkages/cams/belts)

- Seeded `machine-design.concepts.mechanisms-overview` (motion class → element
  family → envelopes/centers/DOFs → VERIFY hub; table to gears + elements).
- Seeded `mechanisms-linkages-mobility` (joints/DOFs first; Gruebler as roles;
  four-bar / slider-crank envelopes).
- Seeded `mechanisms-cams` (follower types; rise–dwell–return story; base
  circle / pressure angle as VERIFY — no invented cam charts).
- Seeded `mechanisms-belts-pulleys` (center distance, wrap, tension, idlers;
  purchased belt profiles).
- Stretch: thin `inspection-metrology-bridge` (characteristic → pack → method
  class → as-built); taxonomy A seeded thin.
- Taxonomy C → **seeded (partial)**; Still-thin updated. Index table + bullets;
  cross-links from gears, power-screws, shafts, springs, technic-envelope,
  drawing-vs-mbd, gdt-intro.
- Cite `mit-272` link-out on mechanisms pages. Rebuilt search-index.json; Rust
  embed + BM25/unit tests; floor ≥50; wire goldens H56–H63.

### FDM load / layers / infill

- Seeded `machine-design.concepts.am-fdm-load-layers-infill` (name primary load
  → bed face so tension in-plane; shells vs infill roles; coupons; no %
  strength tables). Attribution Guns/NWTC + DOE 3D.
- Cross-links: DFAM hub, thin-walls, snap-fit; taxonomy DFAM + draft/anisotropy
  rows updated. Rebuilt `machine-design/search-index.json`; Rust embed +
  BM25/unit tests + wire goldens H54–H55.

### DFAM FDM hub + printed holes

- Seeded `machine-design.concepts.dfam-fdm-overview` (golden-path hub:
  process → orientation → walls → supports → joints → coupons; AM Concept
  table). Attribution Guns/NWTC + DOE 3D.
- Seeded `machine-design.concepts.am-fdm-holes-fit-allowances` (role-based
  clearance/locate/press; coupons over universal XY tables; links fits,
  thin-walls, export-print).
- Taxonomy DFAM row marked **seeded** (hub + holes); SOURCES `prusa-kb`
  link-only; index table + bullets; thin-walls/DFM cross-links.
- Rebuilt `machine-design/search-index.json`; Rust embed + BM25/unit tests +
  wire goldens H50–H53.

### Drawing vs MBD / PMI concept

- Seeded `machine-design.concepts.drawing-vs-mbd-pmi` (CAD-time packs: drawing
  vs MBD vs dual; datum packs sheet/3D; VERIFY PMI↔process). Taxonomy A +
  Still-thin updated; inspection/metrology bridge remains planned.
- Index table + Machine-design bullet; cross-links from gdt-intro, datum,
  hole, fasteners, locating. Rebuilt `machine-design/search-index.json`; Rust
  embed + BM25/unit tests + wire golden H49.

### Springs/couplings concept

- Seeded `machine-design.concepts.springs-couplings` (CAD-time seats, coupling
  misalignment class, VERIFY → datasheets; no rate/angle charts). Taxonomy B +
  Still-thin updated; lid/labyrinth remains the seal page.
- Index table + Machine-design bullet; cross-links from shafts, fasteners,
  bearings. Rebuilt `machine-design/search-index.json`; Rust embed + BM25/unit
  tests + wire golden H48.

### Shafts/keys/rings + browse decruft

- Seeded `machine-design.concepts.shafts-keys-retaining-rings` (CAD-time roles;
  no key/circlip charts). Taxonomy B row + Still-thin note updated.
- Index: doctrine section rename; Machine-design bullets for datum, hole, shafts
  (were table-only). Soft: STEERABLE_MCP drop “jail” wording.
- Rebuilt `machine-design/search-index.json`; wired Rust embed + BM25/unit tests
  + wire golden H47.

### Layout stage + decruft pass

- Decided **not** to move `knowledge/` under `docs/` on this draft PR (stable
  `limo-cad://knowledge/` + embeds).
- Taxonomy: dropped duplicate power-screws rows (section H + stale “Still thin”);
  softened door wording; prompts row notes `help_search` shipped.
- Rebuilt `machine-design/search-index.json` (31 Concepts, includes power-screws).
- Soft: `docs/knowledge-wiki.md` (cad_help-first, drop stale “four articles” /
  “no search engine”), HUMAN_HELP shared-corpus doors, index/taxonomy prefer tone,
  power-screws lead/travel line.

### Ship-clean slop purge (materials + prefer tone)

- Rewrote leftover `Agent anti-patterns` on materials vocabulary to preferred callouts.
- Soft: renamed Agent loop / Agent search tip; prefer-tone on taxonomy, index, INSTALL, INSTALL_MCP.

### Golden-path tone sweep

- Corpus + agentic ADR wording flipped to prefer / golden-path packs (what to do);
  kept engineering content and link-out copyright stance. Soft prefer OK; jail
  language out.
- Doctrine recorded in-repo (one OKF corpus, golden-path authorship):
  `docs/machine-design-help-search.md`.

### Purge MCP ops micro-pages (golden-path rewrite)

Merged MCP ops Concepts into `concepts/agent-mcp-workflow.md` (help, soft focus,
inspect, topology ids, solid_edit_*, units/drivers, headless vs attach). Folded
3MF/STL preflight into `concepts/export-print.md`. Deleted micro-pages and
uncommitted expansion drafts (anti-patterns, AM mirror, draft-injection-vs-AM,
STEP import). Stripped Anti-patterns sections corpus-wide; tone is golden-path.
Wire goldens retargeted; embeds updated (reinstall MCP for live cad_help).

### AM/mechanical Concept wave (searchable KB)

Added heat-set inserts, fastener clearance/counterbore, ribs/gussets/draft,
locating-scheme DOF, tolerance stack-up intro (citation-only), supports/overhangs,
and unofficial Technic envelope. Deepened bearing-stacks (press-fit hub / lead-in /
shoulder). Wired embeds + BM25 unit tests; corpus now 32 Concept pages.

## 2026-09-19

### sync docs/machine-design-kb onto main

Replayed help/KB unique work onto `origin/main` tip (full 139-commit rebase abandoned:
~254 conflicted paths). Added `crates/help` + MCP `cad_help`, expanded machine-design
pages (taxonomy, fasteners, materials, search-index), agent-mcp-workflow doctrine, and
kept main's product concepts (gears, workholding, bearings, wind) plus existing
`resources/*` bundle.

## 2026-09-13

- Added four mechanical-design articles covering datums, fits, manufacturing
  and assembly decisions. The existing native MCP resource inventory embeds
  and serves them with the rest of the knowledge bundle.
- Consolidated useful materials/hardware guidance; removed placeholder pages,
  unused search exports and unimplemented search/UI proposals.
- Extended the knowledge gate to validate source provenance and recipe references.

## 2026-09-11
- **Update**: Added actual G-code footprint, bridge-anchor and preceding-layer checks; zero support paths alone leave printability unproven — prefer bridge/anchor checks.
- **Update**: Added small-generator/rotor/load matching and bearing/axial-retention guidance, with manufacturer and experimental references. Both use the existing automatic MCP resource inventory.
- **Update**: Added sourced gear-identification/pair-design and additive-workholding guidance.
- **Integration**: Native MCP resources expose the same Markdown corpus offline; no separate knowledge store or modeling tool.
- **Maintenance**: Updated MCP and export concepts to distinguish implemented development-branch behavior from physical qualification.
- **Validation**: Existing OKF/link checks and native resource discovery/read/error tests.

## 2026-07-29

- **Update**: Aligned the bundle with OKF v0.2 and current `main`.
- **Validation**: Added automated structure and internal-link checks.

## 2026-07-28

- **Update**: Aligned concepts with maintainer feedback — goals vs proposals,
  co-link first / multi-window deferred, and agent steering files kept internal.

## 2026-07-27

- **Creation**: Seeded the bundle from the README product stance and MCP docs.
