---
type: Concept
title: Machine-design taxonomy
description: Topic map — seeded pages vs planned gaps for the open machine-design KB (browse + search door).
status: draft
updated: 2026-09-20
searchable: false
---

# Machine-design taxonomy

Open help for **design-time** decisions in Limo CAD. Product/architecture
concepts stay in [the OKF index](../index.md).

**Browse:** use this page as the topic map (seeded → link; planned → web /
further reading). **Search:** `cad_help` `topics` / `search` / `get` using the
**ids** in the tables (path with `/` → `.`).

**Coverage honesty:** prefer searching **seeded** pages first. For
**planned** rows, prefer linked further-reading / web — leave empty rather than invent from an
empty topic.

Provenance: [SOURCES](SOURCES.md). Browse note:
[How humans find help today](../../docs/agentic/HUMAN_HELP.md).

## A. Geometric product definition

| Topic | Status | Id / title |
|-------|--------|------------|
| GD&T intro, datums, FCF, Rule #1 teaching | **seeded** | `machine-design.concepts.gdt-intro` — [GD&T intro](concepts/gdt-intro.md) |
| Fits & clearances (class-level) | **seeded** | `machine-design.concepts.fits-clearances` — [fits-clearances](concepts/fits-clearances.md) |
| Tolerance stack-up (method) | **seeded (citation-only)** | `machine-design.concepts.tolerance-stackup-intro` — [tolerance-stackup-intro](concepts/tolerance-stackup-intro.md) |
| Cosmetic threads vs clearance / tap / insert | **seeded** | `machine-design.concepts.cosmetic-threads-vs-clearance` — [cosmetic-threads-vs-clearance](concepts/cosmetic-threads-vs-clearance.md) |
| Drawing vs MBD / PMI walkthrough | **seeded** | `machine-design.concepts.drawing-vs-mbd-pmi` — [drawing-vs-mbd-pmi](concepts/drawing-vs-mbd-pmi.md) |
| Inspection / metrology bridge | **seeded** | `machine-design.concepts.inspection-metrology-bridge` — [inspection-metrology-bridge](concepts/inspection-metrology-bridge.md) |

## B. Machine elements

| Topic | Status | Id / title |
|-------|--------|------------|
| Fasteners & joints | **seeded** (preload/torque distill) | `machine-design.concepts.fasteners-joints` — [fasteners-joints](concepts/fasteners-joints.md) preload/clamp/torque-as-install; also [fastener-clearance-counterbore](concepts/fastener-clearance-counterbore.md), [am-heat-set-inserts](concepts/am-heat-set-inserts.md), [captive-nut-hex-trap](concepts/captive-nut-hex-trap.md) |
| Hardware pocket / actuator / bolt circle research | **seeded** | `machine-design.concepts.am-hardware-pocket-research` — [am-hardware-pocket-research](concepts/am-hardware-pocket-research.md) |
| Shafts, keys, retaining rings | **seeded** | `machine-design.concepts.shafts-keys-retaining-rings` — [shafts-keys-retaining-rings](concepts/shafts-keys-retaining-rings.md) |
| Bearings / hubs / seats | **seeded** | `machine-design.concepts.bearings-hubs-seats` — [bearings-hubs-seats](concepts/bearings-hubs-seats.md) (**bearing seat** / bore / hub seat geometry); purchased SKU stacks: [bearing-stacks](../concepts/bearing-stacks.md) |
| Springs, seals, couplings | **seeded** (springs/couplings + lid seal) | `machine-design.concepts.springs-couplings` — [springs-couplings](concepts/springs-couplings.md); enclosure seal: [am-enclosure-lid-gasket-labyrinth](concepts/am-enclosure-lid-gasket-labyrinth.md) |
| Power screws / lead screws | **seeded** | `machine-design.concepts.power-screws-lead-screws` — [power-screws-lead-screws](concepts/power-screws-lead-screws.md) |

## C. Mechanisms

| Topic | Status | Id / title |
|-------|--------|------------|
| Mechanisms overview (hub) | **seeded** (hub navigation / roles distill) | `machine-design.concepts.mechanisms-overview` — [mechanisms-overview](concepts/mechanisms-overview.md) motion class → family → navigate children / CAD-vs-catalog |
| Linkages / mobility | **seeded** | `machine-design.concepts.mechanisms-linkages-mobility` — [mechanisms-linkages-mobility](concepts/mechanisms-linkages-mobility.md) |
| Cams (CAD-time) | **seeded** | `machine-design.concepts.mechanisms-cams` — [mechanisms-cams](concepts/mechanisms-cams.md) |
| Belts & pulleys | **seeded** | `machine-design.concepts.mechanisms-belts-pulleys` — [mechanisms-belts-pulleys](concepts/mechanisms-belts-pulleys.md) |
| Chains & sprockets | **seeded** | `machine-design.concepts.mechanisms-chains-sprockets` — [mechanisms-chains-sprockets](concepts/mechanisms-chains-sprockets.md) |
| Gears (product page) | **seeded** | `concepts.gears` — [gears](../concepts/gears.md) |
| Printed gears (FDM DFAM) | **seeded** | `machine-design.concepts.am-printed-gears-dfam` — [am-printed-gears-dfam](concepts/am-printed-gears-dfam.md) |
| Technic-style beam/pin envelope | **seeded (LDraw/open nominals; unofficial)** | `machine-design.concepts.technic-envelope` — [technic-envelope](concepts/technic-envelope.md) |
| Intermittent / Geneva-style | **seeded** | `machine-design.concepts.mechanisms-intermittent-geneva` — [mechanisms-intermittent-geneva](concepts/mechanisms-intermittent-geneva.md) |
| Product joints demo | recipe | `vertical-axis-turbine` |
| Deep tooth / cam-law / belt-tension / Geneva slot charts | **planned** | datasheets + `mit-272` link-out; Help stays roles only |

## D. Materials

| Topic | Status | Id / title |
|-------|--------|------------|
| Property vocabulary | **seeded** (CAD-time vocab / roles distill) | `machine-design.concepts.materials-vocabulary` — [materials-vocabulary](concepts/materials-vocabulary.md) E/Sy/Sut/hardness/anisotropy/print-vs-isotropic; **no** allowables charts |
| Selection / allowables tables | **planned** | (no MatWeb scrape) |

## E. Design for manufacturing

| Topic | Status | Id / title |
|-------|--------|------------|
| DFM overview | **seeded** | `machine-design.concepts.dfm-overview` — [dfm-overview](concepts/dfm-overview.md) |
| Process guidelines | **seeded** | `machine-design.concepts.dfm-process-guidelines` — [dfm-process-guidelines](concepts/dfm-process-guidelines.md) |
| DFAM (FDM hub + snap / walls / ribs / supports / inserts / clamshell / holes / load-layers / printed gears) | **seeded** | Hub: `machine-design.concepts.dfam-fdm-overview` — [dfam-fdm-overview](concepts/dfam-fdm-overview.md); holes: [am-fdm-holes-fit-allowances](concepts/am-fdm-holes-fit-allowances.md); load/layers/infill: [am-fdm-load-layers-infill](concepts/am-fdm-load-layers-infill.md); printed gears: [am-printed-gears-dfam](concepts/am-printed-gears-dfam.md); also [am-snap-fit](concepts/am-snap-fit.md), [am-thin-walls](concepts/am-thin-walls.md), [am-clamshell-retainer](concepts/am-clamshell-retainer.md), [am-heat-set-inserts](concepts/am-heat-set-inserts.md), [am-ribs-gussets-draft](concepts/am-ribs-gussets-draft.md), [am-supports-overhangs](concepts/am-supports-overhangs.md) |
| Cable exits / wire windows / strain relief | **seeded** | `machine-design.concepts.am-cable-exits-strain-relief` — [am-cable-exits-strain-relief](concepts/am-cable-exits-strain-relief.md) |
| Enclosure lid / gasket / labyrinth | **seeded** | `machine-design.concepts.am-enclosure-lid-gasket-labyrinth` — [am-enclosure-lid-gasket-labyrinth](concepts/am-enclosure-lid-gasket-labyrinth.md) |
| Ventilation grille / finger-trap openings | **seeded** | `machine-design.concepts.am-ventilation-grille-finger-trap` — [am-ventilation-grille-finger-trap](concepts/am-ventilation-grille-finger-trap.md) |
| Boss-to-boss / standoff patterns | **seeded** | `machine-design.concepts.am-boss-standoff-patterns` — [am-boss-standoff-patterns](concepts/am-boss-standoff-patterns.md) |
| AM join choice (glue / weld / screw / snap) | **seeded** | `machine-design.concepts.am-assembly-join-choice` — [am-assembly-join-choice](concepts/am-assembly-join-choice.md) |
| Warpage / cooling / flatness (large plates) | **seeded** | `machine-design.concepts.am-warpage-cooling-flatness` — [am-warpage-cooling-flatness](concepts/am-warpage-cooling-flatness.md) |
| Fit coupons & recipes map (hub) | **seeded** | `machine-design.concepts.fit-coupons-recipes-map` — [fit-coupons-recipes-map](concepts/fit-coupons-recipes-map.md) |
| Fillet vs chamfer | **seeded** | `machine-design.concepts.fillet-chamfer` — [fillet-chamfer](concepts/fillet-chamfer.md) |
| Alignment nubs vs pins | **seeded** | `machine-design.concepts.alignment-nubs-pins` — [alignment-nubs-pins](concepts/alignment-nubs-pins.md) |
| Locating schemes / DOF | **seeded** | `machine-design.concepts.locating-scheme-dof` — [locating-scheme-dof](concepts/locating-scheme-dof.md) |
| Draft vs layer anisotropy | **seeded** | Load/layers/infill: [am-fdm-load-layers-infill](concepts/am-fdm-load-layers-infill.md); also [am-ribs-gussets-draft](concepts/am-ribs-gussets-draft.md), [am-thin-walls](concepts/am-thin-walls.md), [dfm-process-guidelines](concepts/dfm-process-guidelines.md) |
| Living hinge sizing | **deepened on** | [am-snap-fit](concepts/am-snap-fit.md) (HIT; not a separate page) |

## F. Design hygiene

| Topic | Status | Id / title |
|-------|--------|------------|
| Requirements → embodiment → BOM / purchased parts | **seeded** (requirements/BOM distill) | `machine-design.concepts.design-hygiene-requirements-bom` — [design-hygiene-requirements-bom](concepts/design-hygiene-requirements-bom.md) make-vs-buy / BOM roles / VERIFY freeze; also [research-before-commit](../concepts/research-before-commit.md), [am-hardware-pocket-research](concepts/am-hardware-pocket-research.md) |

## G. Assembly validation

| Topic | Status | Id / title |
|-------|--------|------------|
| Interference / clearance at solved poses | **seeded** | `concepts.assembly-interference` — [assembly-interference](../concepts/assembly-interference.md) |
| Fit classes (clearance / transition / interference) | **seeded** | `machine-design.concepts.fits-clearances` — [fits-clearances](concepts/fits-clearances.md) |
| Validate before show (shot pack) | **seeded** | `concepts.validate-before-show` — [validate-before-show](../concepts/validate-before-show.md) |
| Adversarial mesh / wall probe audit | **seeded** | `concepts.adversarial-mesh-audit` — [adversarial-mesh-audit](../concepts/adversarial-mesh-audit.md) |


## H. CAD program / mechanical ops

| Topic | Status | Id / title |
|-------|--------|------------|
| MCP workflow (help / focus / inspect / edit / sessions / units) | **seeded** | `concepts.agent-mcp-workflow` — [agent-mcp-workflow](../concepts/agent-mcp-workflow.md) |
| Datum / CS / sketch plane choice | **seeded** | `machine-design.concepts.datum-sketch-plane-choice` — [datum-sketch-plane-choice](concepts/datum-sketch-plane-choice.md) |
| Hole feature vs modeled / patterns | **seeded** | `machine-design.concepts.hole-wizard-vs-modeled` — [hole-wizard-vs-modeled](concepts/hole-wizard-vs-modeled.md) |
| Export / print / 3MF vs STL | **seeded** | `concepts.export-print` — [export-print](../concepts/export-print.md) (pair with [adversarial-mesh-audit](../concepts/adversarial-mesh-audit.md)) |
| Design VERSION / JSONC script naming | **seeded** | `concepts.design-version-scripts` — [design-version-scripts](../concepts/design-version-scripts.md) |
| Geometry naming (bodies / faces / scripts / STEP) | **seeded** | `concepts.geometry-naming` — [geometry-naming](../concepts/geometry-naming.md) |
| Shared reference geometry (followers / named parents) | **seeded** | `concepts.shared-reference-geometry` — [shared-reference-geometry](../concepts/shared-reference-geometry.md) |

## Still thin / planned (honest)

Structural evidence is now seeded in
[`machine-design.concepts.simulation-verification`](concepts/simulation-verification.md):
CAD/contact interpretation, same-mesh comparison, mesh convergence, numerical
checks and physical qualification. This is experimental verification guidance;
production structural analysis and certified material allowables remain planned.

| Gap | Notes |
|-----|-------|
| Deep Y14.41 / semantic PMI export recipes | drawing-vs-mbd-pmi seeded (CAD-time packs); vendor/tutorial dumps **out**; inspection bridge seeded (roles); deep CMM/GR&R still out |
| Deep CMM / GR&R / gage design | inspection bridge **seeded** (roles + handoff checklist); numeric/procedure dumps **out** |
| Deep bearing L10 / capacity charts | bearings-hubs-seats seeded (VERIFY→catalog); L10 tables stay datasheet |
| Deep key stress / groove charts | shafts page seeded (roles only); numeric charts **TODO** |
| Deep spring-rate / coupling misalignment charts | springs-couplings seeded (roles only); numeric charts **TODO** (datasheets) |
| Deep cam-law / belt-tension / chain-tension / tooth / AGMA / Geneva-slot charts | mechanisms hub **seeded** (navigation + CAD-vs-catalog roles); linkages/cams/belts/chains/Geneva seeded; gears + printed-gears DFAM seeded (**roles only**; no AGMA/tooth/module-strength tables); numeric charts stay datasheet / `mit-272` |
| Deep ERP / PLM / Stage-Gate BOM workflows | design-hygiene **seeded** (CAD-time roles + VERIFY); ERP/indenture dumps **out** |
| Materials allowables tables | **planned** (no MatWeb); vocabulary **seeded** (roles only) |
| Deep fastener torque / proof-load / K-factor charts | fasteners-joints **seeded** (preload/clamp + torque-as-install + friction/lube roles); numeric tables stay NASA RP-1228 / datasheet |
| More MCP `prompts` (beyond `help_search`) | design-flow prompts still pages + skills ([HUMAN_HELP](../../docs/agentic/HUMAN_HELP.md)) |

## `cad_help` topics labels (useful seeds)

Frontmatter `topics:` on Concept pages feed `cad_help` **topics**. Expect labels
such as: `dfam`, `am`, `fdm`, `fasteners`, `joints`, `hardware`, `enclosures`,
`cables`, `seals`, `threads`, `fits`, `gdt`, `locators`, `print`, `snap-fit`,
`recipes`, `manufacturing`, `research`, `validation`, `assembly`, `mcp`, `bearings`,
`mechanisms`, `springs`, `couplings`, `mbd`, `pmi`, `drawings`, `dfa`, `datums`, `sketch`, `holes`, `history`, `export`, `units`,
`modeling`, `workflow`, `disclosure`, `focus`, `sessions`, `parametric`, `selection`, `topology`, `VERSION`, `design_v`, `JSONC`. Use `topics` then `search` with those words; `get` with an id
from the tables above.

## Live examples

Scripted recipes remain the screen source. See
[machine-design help search](../../docs/machine-design-help-search.md),
the [fit coupons & recipes map](concepts/fit-coupons-recipes-map.md) hub, and
recipe ids in page frontmatter (`related_recipes`).
