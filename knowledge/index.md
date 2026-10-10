---
okf_version: "0.2"
---

# Limo CAD — Open Knowledge Format (OKF) index

Portable knowledge bundle for **humans and agents**. Specification:
[Open Knowledge Format v0.2](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md).

Keep concepts **thin**. Longer material lives in the repository’s
[goals](../docs/goals.md), [MCP harness notes](../docs/mcp-harness.md),
[proposed architecture](../docs/proposed-architecture.md), and
[machine-design help search](../docs/machine-design-help-search.md).

## How to browse (humans)

1. **This index** — product doctrine + machine-design door list (below).
2. **[Machine-design taxonomy](machine-design/taxonomy.md)** — seeded vs planned
   topic map with **page ids** and titles (best map for “what exists?”).
3. **Search** — MCP tool `cad_help` → `search` → `get` by id; `topics` is an
   alpha label door (not a curated browse map — use taxonomy/index for that);
   or full-text in your editor over `knowledge/**`.
4. **Full page** — open the Markdown file, or MCP `resources/read` on
   `limo-cad://knowledge/...` (same text).
5. **Demos** — recipe chips / `related_recipes` deep-link **Scripts** (unchanged);
   no Bevy viewport inside Help yet.

Operating note: [How humans find help today](../docs/agentic/HUMAN_HELP.md).

## Topic map → page ids (quick)

Ids are path-derived (`knowledge/` stripped, `/` → `.`). Use these with
`cad_help` `get` (id-only).

### Doctrine & validation

| Topic | Id | Title |
|-------|----|-------|
| MCP workflow (help / focus / inspect / edit / sessions) | `concepts.agent-mcp-workflow` | MCP workflow |
| VERIFY before freeze | `concepts.research-before-commit` | Research before commit |
| Review shot pack | `concepts.validate-before-show` | Validate before show |
| Mesh / wall probe audit | `concepts.adversarial-mesh-audit` | Adversarial mesh audit |
| Assembly overlap @ pose | `concepts.assembly-interference` | Assembly interference check |
| FEM evidence / mesh convergence / solver agreement | `machine-design.concepts.simulation-verification` | Structural simulation evidence and verification |
| Export / print / 3MF vs STL | `concepts.export-print` | Export and print |
| Design VERSION / JSONC scripts | `concepts.design-version-scripts` | Design VERSION and JSONC script naming |
| Geometry naming (bodies / faces / scripts / STEP) | `concepts.geometry-naming` | Geometry naming — bodies, features, faces, scripts, STEP |
| Shared reference geometry | `concepts.shared-reference-geometry` | Shared reference geometry — surfaces that follow param changes |

### Machine design (seeded highlights)

| Topic | Id | Title |
|-------|----|-------|
| Fits / clearance class | `machine-design.concepts.fits-clearances` | Fits and clearances |
| GD&T intro | `machine-design.concepts.gdt-intro` | GD&T intro |
| Snap-fits / living hinges | `machine-design.concepts.am-snap-fit` | AM snap-fits and living hinges |
| Thin walls / anisotropy | `machine-design.concepts.am-thin-walls` | AM thin walls and print orientation |
| Hardware pocket / servo / PCD | `machine-design.concepts.am-hardware-pocket-research` | Hardware pocket research |
| Cable exits / strain relief | `machine-design.concepts.am-cable-exits-strain-relief` | AM cable exits… |
| Captive / hex nut trap | `machine-design.concepts.captive-nut-hex-trap` | Captive nut and hex nut trap |
| Cosmetic vs clearance threads | `machine-design.concepts.cosmetic-threads-vs-clearance` | Cosmetic threads vs modeled clearance |
| Heat-set inserts | `machine-design.concepts.am-heat-set-inserts` | AM heat-set inserts… |
| Fastener preload / torque | `machine-design.concepts.fasteners-joints` | Fasteners and joints |
| Fastener hole roles | `machine-design.concepts.fastener-clearance-counterbore` | Fastener clearance & counterbore |
| Lid / gasket / labyrinth | `machine-design.concepts.am-enclosure-lid-gasket-labyrinth` | AM enclosure lid… |
| Vent / finger-trap grille | `machine-design.concepts.am-ventilation-grille-finger-trap` | AM ventilation grille… |
| Boss / standoff patterns | `machine-design.concepts.am-boss-standoff-patterns` | AM boss-to-boss… |
| Glue / screw / snap choice | `machine-design.concepts.am-assembly-join-choice` | AM assembly join choice |
| Warpage / flatness | `machine-design.concepts.am-warpage-cooling-flatness` | AM warpage… |
| Fit coupons / recipes hub | `machine-design.concepts.fit-coupons-recipes-map` | Fit coupons and recipes map |
| Datum / sketch plane (MCP) | `machine-design.concepts.datum-sketch-plane-choice` | Datum / sketch plane choice |
| Hole feature vs modeled | `machine-design.concepts.hole-wizard-vs-modeled` | Hole wizard vs modeled hole |
| Bearings / hubs / seats | `machine-design.concepts.bearings-hubs-seats` | Bearing seat geometry — hubs / seats |
| Shafts / keys / retaining rings | `machine-design.concepts.shafts-keys-retaining-rings` | Shafts, keys, and retaining rings |
| Springs / couplings | `machine-design.concepts.springs-couplings` | Springs and couplings |
| Drawing vs MBD / PMI | `machine-design.concepts.drawing-vs-mbd-pmi` | Drawing vs MBD / PMI |
| Inspection / metrology bridge | `machine-design.concepts.inspection-metrology-bridge` | Inspection / metrology bridge |
| DFAM / FDM overview | `machine-design.concepts.dfam-fdm-overview` | DFAM for FDM overview |
| FDM holes / printed fits | `machine-design.concepts.am-fdm-holes-fit-allowances` | FDM holes and printed-fit allowances |
| FDM load / layers / infill | `machine-design.concepts.am-fdm-load-layers-infill` | FDM load path vs layer orientation / infill roles |
| Materials vocabulary | `machine-design.concepts.materials-vocabulary` | E / Sy / Sut / hardness / anisotropy / print vs isotropic (vocab only) |
| Mechanisms overview | `machine-design.concepts.mechanisms-overview` | Motion class → family → navigate children / CAD-vs-catalog |
| Linkages / mobility | `machine-design.concepts.mechanisms-linkages-mobility` | Linkages and mobility |
| Cams | `machine-design.concepts.mechanisms-cams` | Cams (CAD-time) |
| Belts & pulleys | `machine-design.concepts.mechanisms-belts-pulleys` | Belts and pulleys |
| Chains & sprockets | `machine-design.concepts.mechanisms-chains-sprockets` | Chains and sprockets |
| Printed gears (FDM DFAM) | `machine-design.concepts.am-printed-gears-dfam` | Printed gears — FDM DFAM |
| Intermittent / Geneva | `machine-design.concepts.mechanisms-intermittent-geneva` | Intermittent motion / Geneva |
| Requirements → BOM hygiene | `machine-design.concepts.design-hygiene-requirements-bom` | Requirements → embodiment → make-vs-buy → BOM roles |

Full seeded/planned table: **[taxonomy](machine-design/taxonomy.md)**.

## Concepts

- [Product stance](concepts/product-stance.md) - Local-first mechanical CAD priorities.
- [Architecture](concepts/architecture.md) - Kernel, shell, and project-file boundaries.
- [MCP harness](concepts/mcp-harness.md) - Headless/live routing and engineering resources.
- [MCP workflow](concepts/agent-mcp-workflow.md) - Help-first, soft focus, inspect, edit, sessions, units.
- [Design VERSION / JSONC scripts](concepts/design-version-scripts.md) - One VERSION → design_vM_N.limo.jsonc (filename + embed); prune prior; prefer JSONC chunks over gen_*.py.
- [Geometry naming](concepts/geometry-naming.md) - Role-noun bodies/features/faces; align script ↔ browser ↔ STEP; VERIFY names after mutate.
- [Shared reference geometry](concepts/shared-reference-geometry.md) - Named shared planes/axes/faces so related surfaces follow param changes; VERIFY after edits.
- [Research before commit](concepts/research-before-commit.md) - VERIFY table and local help before freezing geometry.
- [Assembly interference check](concepts/assembly-interference.md) - Geometric overlap/clearance at solved poses vs fit classes.
- [Validate before show](concepts/validate-before-show.md) - Mandatory review shot pack; prefer recapture when frames are blank or inside-solid.
- [Adversarial mesh audit](concepts/adversarial-mesh-audit.md) - Manifold, wall probes, shards; printable-solid gate.
- [Export & print](concepts/export-print.md) - Interchange, 3MF vs STL, mesh preflight, qualification boundaries.
- [Contribution process](concepts/process.md) - Lightweight contribution and review expectations.
- [Gear identification and compatible pairs](concepts/gears.md) - Module/DP, OD limits, pressure angle, ratio changes and mounting.
- [Additive workholding](concepts/additive-workholding.md) - Captured guides, assembly access, D-flat roots and qualification.
- [Small wind rotors and low-speed generators](concepts/small-wind-generators.md) - Power, startup, gearing, motor dimensions and measured loads.
- [Bearing supports, hubs, and axial retention](concepts/bearing-stacks.md) - Purchased SKU seats, press-fit hubs, lead-in, shoulders, spacer stacks.

## Read through MCP

The native MCP server embeds this Markdown corpus at build time. Prefer **`cad_help`**
(`search` → `get` / `topics`) for discovery — snippet-first with locked caps (search
default 5 / max 10, snippet ~280 chars, get 12 KiB, topics page 50). Use standard
`resources/list` then `resources/read` with a returned URI (for example
`limo-cad://knowledge/index.md`) when the full page is needed. Resources are read-only
and available without a checkout or network connection. They describe the bundled
source revision; rebuild to pick up later knowledge changes.

Resolve links between knowledge pages relative to the current resource URI:
from `limo-cad://knowledge/concepts/gears.md`, `additive-workholding.md` means
`limo-cad://knowledge/concepts/additive-workholding.md`. Links starting `../../docs/`
or `../../mcp-server/` identify supporting paths in a checkout of the same source
revision; they are not additional MCP resources. External HTTPS sources can be
opened separately when network access is available.

## Machine design

Open design-time help (GD&T, elements, mechanisms, materials, DFM).
Prefer **seeded** pages via `cad_help` before web search; see taxonomy for **planned**
gaps. Provenance: [SOURCES](machine-design/SOURCES.md).

- [Taxonomy](machine-design/taxonomy.md) - Topic map for the domain KB (browse map).
- [Sources](machine-design/SOURCES.md) - License and provenance table.
- [GD&T intro](machine-design/concepts/gdt-intro.md) - Datums and feature control frames.
- [Fits & clearances](machine-design/concepts/fits-clearances.md) - Clearance, locational, interference.
- [Fasteners & joints](machine-design/concepts/fasteners-joints.md) - Preload/clamp load, torque as install vs proof, friction/lube; VERIFY datasheet (no invented charts).
- [Power screws / lead screws](machine-design/concepts/power-screws-lead-screws.md) - Lead vs pitch, wear nuts, VERIFY (not load ratings).
- [Materials vocabulary](machine-design/concepts/materials-vocabulary.md) - E/Sy/Sut/hardness/anisotropy/print-vs-isotropic roles; educational ≠ allowable (no MatWeb/allowables tables).
- [DFM overview](machine-design/concepts/dfm-overview.md) - Process families and heuristics.
- [DFAM for FDM overview](machine-design/concepts/dfam-fdm-overview.md) - Additive FDM golden path hub; links seeded AM Concepts.
- [FDM holes / printed-fit allowances](machine-design/concepts/am-fdm-holes-fit-allowances.md) - Role-based printed hole fits; coupons over universal tables.
- [FDM load / layers / infill](machine-design/concepts/am-fdm-load-layers-infill.md) - Load path vs bed face; shells vs infill; coupons, no % strength tables.
- [Mechanisms overview](machine-design/concepts/mechanisms-overview.md) - Motion class → element family → navigate children / CAD-vs-catalog → VERIFY (no tooth/cam/belt charts).
- [Linkages / mobility](machine-design/concepts/mechanisms-linkages-mobility.md) - Joints/DOFs first; four-bar / slider-crank envelopes.
- [Cams (CAD-time)](machine-design/concepts/mechanisms-cams.md) - Rise–dwell–return story; base circle / PA as VERIFY.
- [Belts & pulleys](machine-design/concepts/mechanisms-belts-pulleys.md) - Center distance, wrap, tension path; purchased profiles.
- [Chains & sprockets](machine-design/concepts/mechanisms-chains-sprockets.md) - Center distance, wrap, tension path; purchased pitch.
- [Printed gears (FDM DFAM)](machine-design/concepts/am-printed-gears-dfam.md) - Orientation vs tooth load; min tooth vs nozzle; backlash coupon; no module-strength tables.
- [Intermittent / Geneva](machine-design/concepts/mechanisms-intermittent-geneva.md) - Index/dwell/lock-arc roles; envelopes; VERIFY purchased indexer or analyzed cam.
- [Requirements → BOM hygiene](machine-design/concepts/design-hygiene-requirements-bom.md) - Requirements → embodiment → make-vs-buy → BOM roles / VERIFY freeze (CAD-time; no ERP dumps).

- [DFM process guidelines](machine-design/concepts/dfm-process-guidelines.md) - Molding, cast, sheet, weld, EDM, CNC.
- [AM snap-fits](machine-design/concepts/am-snap-fit.md) - Cantilever clips, latches, living hinges (sizing).
- [AM thin walls](machine-design/concepts/am-thin-walls.md) - FDM min wall, anisotropy, print orientation.
- [Fillet vs chamfer](machine-design/concepts/fillet-chamfer.md) - When to blend vs bevel.
- [Alignment nubs vs pins](machine-design/concepts/alignment-nubs-pins.md) - Locator class: short AM nubs/socks vs pins/dowels.
- [AM clamshell retainer](machine-design/concepts/am-clamshell-retainer.md) - Slide-fit first, then optional detents.
- [AM heat-set inserts](machine-design/concepts/am-heat-set-inserts.md) - Bosses, crush ribs, heat-set vs tapped plastic.
- [Fastener clearance & counterbore](machine-design/concepts/fastener-clearance-counterbore.md) - Clearance, counterbore, tap vs insert roles.
- [AM ribs, gussets, and draft](machine-design/concepts/am-ribs-gussets-draft.md) - Stiffen with ribs; draft vs anisotropy cross-link.
- [Locating schemes & DOF](machine-design/concepts/locating-scheme-dof.md) - Primary/secondary locate; avoid overconstraint.
- [Tolerance stack-up intro](machine-design/concepts/tolerance-stackup-intro.md) - Dimensional loops; citation-only, no closed tables.
- [AM supports & overhangs](machine-design/concepts/am-supports-overhangs.md) - Support strategy, bridging, overhang design.
- [Technic-style envelope (unofficial)](machine-design/concepts/technic-envelope.md) - Open LDraw/Technic.scad nominals (LDU→mm); measure + coupon; unofficial.
- [Hardware pocket research](machine-design/concepts/am-hardware-pocket-research.md) - Servo/horn/spline/bolt-circle VERIFY pattern.
- [AM cable exits & strain relief](machine-design/concepts/am-cable-exits-strain-relief.md) - Wire windows, grommets, jacket clamp.
- [Captive nut / hex trap](machine-design/concepts/captive-nut-hex-trap.md) - Anti-rotation nut pockets for AM.
- [Cosmetic threads vs clearance](machine-design/concepts/cosmetic-threads-vs-clearance.md) - Display helix vs real hole roles.
- [AM enclosure lid / gasket / labyrinth](machine-design/concepts/am-enclosure-lid-gasket-labyrinth.md) - Lid seal classes and clamp/locate roles.
- [AM ventilation grille / finger-trap](machine-design/concepts/am-ventilation-grille-finger-trap.md) - Vent openings vs ingress and print bars.
- [AM boss-to-boss / standoff patterns](machine-design/concepts/am-boss-standoff-patterns.md) - PCB/plate standoff grids and screw roles.
- [AM assembly join choice](machine-design/concepts/am-assembly-join-choice.md) - Glue / weld / screw / snap — when not to snap.
- [AM warpage / cooling / flatness](machine-design/concepts/am-warpage-cooling-flatness.md) - Large plate curl and flatness levers.
- [Datum / sketch plane (MCP)](machine-design/concepts/datum-sketch-plane-choice.md) - Origin, CS, and sketch plane choice for MCP edits.
- [Hole feature vs modeled](machine-design/concepts/hole-wizard-vs-modeled.md) - Hole feature vs sketched/patterned holes.
- [Bearings / hubs / seats](machine-design/concepts/bearings-hubs-seats.md) - Shaft/housing seats, fit roles, preload stacks; load/speed/life VERIFY to catalog.
- [Shafts / keys / retaining rings](machine-design/concepts/shafts-keys-retaining-rings.md) - Stepped shafts, keyseats, circlip grooves (CAD-time).
- [Springs / couplings](machine-design/concepts/springs-couplings.md) - Spring seats and shaft couplings (CAD-time; datasheet rates).
- [Drawing vs MBD / PMI](machine-design/concepts/drawing-vs-mbd-pmi.md) - 2D drawing notes vs model PMI; VERIFY process match.
- [Inspection / metrology bridge](machine-design/concepts/inspection-metrology-bridge.md) - Characteristic roles → pack → method class → as-built loop.
- [Fit coupons & recipes map](machine-design/concepts/fit-coupons-recipes-map.md) - Concept → `related_recipes` hub.

Use the listed resources for their stated scope, then consult the cited sources
for more detail. The bundle is guidance for design decisions; it does not supply
certified material allowables, standards tables or physical qualification.

## Hosted page

GitHub Pages builds from this bundle (see `.github/workflows/pages-knowledge.yml`).
Prefer `cad_help` or bundled MCP resources over scraping the hosted HTML.
