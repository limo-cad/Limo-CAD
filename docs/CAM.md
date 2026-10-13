# 3-axis CAM foundation

This branch starts a host-neutral, fixed-axis CAM module for common 3-axis
milling workflows. It is an engineering foundation, not yet a claim of
production-safe CAM.

Start with the [CAM documentation index](cam/README.md) and
[linking contract](CAM_LINKING.md) for controls and clearance limits. The
[High Speed Roughing guide](CAM_ADAPTIVE.md) describes continuous exterior
passes, editable height references and explicitly limited fallback/stock support.

## Operating model: nothing is created automatically

Entering the manufacturing workspace never creates a setup, a tool, or an
operation. The operator programs the job explicitly, in this order:

1. **Tool library first.** Every cutter is a library entry with its geometry
   (kind, diameter, flute length/count, tip angle where relevant) and its
   cutting data. Operations reference library tools by an internal id and
   inherit their cutting data as editable defaults, so renumbering or
   renaming a tool never breaks an operation. The machine-facing identity is
   deliberately dual: a tool number is optional — number-calling posts
   (GRBL/LinuxCNC/Fanuc style) fail closed with a clear error when it is
   missing. Automatic output uses library numbers; name-capable Siemens/TNC
   posts use exact library names when numbers are absent, and offer explicit
   Number/Name mode. No separate manual mapping is required. Tool kinds cover flat/ball/bull-nose end mills, face (shell)
   mills, drills, chamfer mills, taps, reamers, boring bars, and thread
   mills (turning kinds are reserved for the planned turning workspace).
   Flat, bull-nose, and face mills carry an optional corner radius. The
   editor is tabbed (General / Cutter / Cutting data) — new tools start
   from a type-picker page, and every field stays editable in any order
   later. Cutting
   data is a default profile plus any number of named profiles (e.g. per
   material), with a two-way chip-load calculator: each linked pair
   (rpm ↔ surface speed, feed ↔ feed-per-tooth, plunge ↔ plunge-per-rev)
   follows an edit on either side, and the side last touched wins at save
   time. Holemaking tools (drills, reamers, boring bars) drop the lateral
   feed pair entirely — a drill only plunges — so their profiles are driven
   by surface speed and plunge feed per revolution, and the editor hides the
   milling feed fields for them. The calculator works on the *effective*
   cutting diameter: engaged
   shallower than the corner radius, De = D − 2R + 2√(2R·ap − ap²) at the
   entered depth of cut — the correction that matters most on high-feed
   tooling. Operation creation can copy any profile instead of the default.

   The library lives in two scopes. The CENTRAL library belongs to the OS
   user, not any project: it is a single per-user file
   (`cam-tool-library.json` by default in the app's platform config directory —
   `~/Library/Application Support` on macOS, `%APPDATA%` on Windows,
   `~/.config` on Linux). **Settings → CAM → Central tool library** now shows
   the exact path and offers explicit folder selection/copy/reset; the Tool
   Library header links to that panel. See [storage behavior and safeguards](CAM_TOOL_LIBRARY.md).
   Each central collection owns its tool-id allocation; separate collections
   are not automatically merged. The
   PROJECT library lives inside the machining document (and the .limo
   file): full-data snapshots of exactly the tools this project uses, which
   is what operations reference — a project file is self-contained and
   portable, and editing the central library never silently rewrites an
   existing project. Synchronisation is always explicit, never a background
   merge: import copies a central tool into the project (or refreshes the
   same-id snapshot), publish pushes a project snapshot back into the
   collection. Creating a tool inside a project also registers it centrally
   so it stays importable everywhere. The Tool Library dialog (ribbon)
   opens on the central scope with a header switch to the project scope and
   a free-text filter over name/number/type. Operation dialogs pick tools
   through that same dialog stacked on top as a picker (the store keeps a
   one-deep dialog stack): the project scope shows first, compatible rows
   highlight, and a double-click or the confirm button chooses — central
   picks are copied into the project on the way in. When the project scope
   holds no compatible tool but the central library does, the picker switches
   scopes on its own instead of showing an empty table. Setups, operations,
   units, and post defaults stay project data.
2. **Manual setup.** The operator chooses the part bodies, defines the stock,
   and picks the WCS origin on the geometry. Stock has four shapes — box,
   cylinder, hex bar, or a modeled body — defined in one of three ways: a
   fixed size with the model centered inside or parked against a chosen face
   with a gap; grown from the model bounding box with per-face allowances; or
   the remaining stock of an earlier setup (rest machining, which inherits the
   source setup's WCS). The WCS origin is picked in the viewport on a lattice
   of 27 stock/model box points (corners, edge midpoints, face centers,
   volume center) or on any point sketched earlier; explicit coordinates and
   equivalent anchor dropdowns remain available. Orientation (Z flip,
   rotation about Z) is explicit. Work offsets name a first offset (G54…G59)
   plus a duplicate-part count: posting one program repeats the toolpaths
   under that many consecutive offsets. A setup can remain Generic 3-axis
   during programming, or carry a machine/controller/post snapshot. Neutral
   paths remain reusable, but NC output requires a matching explicit target;
   see [machine-aware workflow](CAM_MACHINES.md).
3. **One operation at a time.** Each operation is programmed against geometry
   the operator selects — model/sketch edges for 2D contour and chamfer,
   with one-click closed face boundaries or manually assembled open/closed
   chains, a whole-loop hover preview and a reversible selection arrow.
   Open chains never receive an implicit closing cut; disconnected edges within
   one chain and ambiguous branches fail. Chamfer can collect multiple separate
   chains in one operation, with clearance retracts between them.
   Modeled chamfer upper rims derive their width,
   top plane and material side, while sharp edges use explicit width; see
   [shared chains and chamfer](CAM_EDGE_CHAINS.md). Closed sketch loops for pockets
   (clicked directly in the viewport: every closed loop highlights on hover,
   clicking a segment or inside a profile commits it, never a list of feature
   labels); holes clicked directly on the model's cylindrical faces for
   drilling and
   thread milling (hover highlights the face, a click toggles the hole, only
   faces whose axis is parallel to setup Z are pickable under fixed-axis
   planning — the pick still records the hole axis in setup coordinates as
   the seam for a future indexed/5-axis tool orientation — and explicit
   coordinates remain available); the stock top or an explicit region for
   facing. Every operation dialog is the same five-tab scaffold
   (Tool / Geometry / Heights / Passes / Linking); the kind only switches on
   the geometry shape and the fields it needs, so a shared tab is edited once
   for all operation kinds. Creating and editing use the identical dialog —
   editing just seeds the same drafts from the stored operation and saves
   through the same validation. Facing and the other milling dialogs expose
   editable **Spindle speed / Surface speed**, **Cutting feedrate / Feed per
   tooth**, and **Plunge feedrate / Feed per revolution** pairs. Editing either
   side makes it the driver while the dialog is open; dependent fields update
   immediately. Drilling-family cycles omit the lateral pair. In metric terms,
   `n = round(1000 Vc / (pi D))`, `F = fz n z`, and `Fz = f_rev n`, where
   `D` is nominal tool diameter and `z` is flute/insert count. These operation
   calculators do not apply the library editor's shallow-engagement effective
   diameter correction. Inch display uses SFM and inch-based feeds/chip loads.
   Save stores the resolved whole-number RPM and canonical mm/min feeds, not
   an expression or a library mutation; reopening derives the partner values
   from those stored numbers. Selecting a preset resets the drivers. Empty,
   non-positive or non-finite driver inputs block saving instead of reusing an
   old result. High Speed Roughing now labels its Z approach rate **Plunge
   feed**; material-entry **Ramp feed** remains separate and its checked motion
   behavior is unchanged. Heights are entered
   as a reference plus a signed
   offset and resolved to absolute setup Z at submit: the reference can be a
   plane (model/stock top or bottom, origin, or picked-hole top/bottom), a LOWER
   height of the same operation (fixed resolution order bottom → top → feed →
   retract → clearance, so chains cannot cycle), or the picked coplanar edges
   or sketch loop's plane Z ('Selection'). These reference identities and signed offsets are
   persisted and re-resolved from current CAD during explicit regeneration;
   malformed associations cannot silently fall back to baked coordinates.
   Facing targets the
   model's top surface: the default bottom height is model top + 0. Facing
   also carries a mandatory **safe distance**
   (default 5 mm): the entry plunge happens one cutter radius plus this
   clearance outside the stock boundary, so the tool always descends in free
   air and the entry never becomes plunge-milling — which is also why
   non-center-cutting face mills are allowed (flat/bull-nose/face mills
   only; ball, chamfer, and thread mills are rejected). Safe heights
   (clearance and retract Z) are set per
   operation, not globally on the setup. The engine validates the input and
   rejects incomplete programs instead of guessing.

The same document is fully scriptable through the MCP `cam_*` tools
(`cam_get_document`, `cam_set_document`, `cam_plan_setup`, `cam_post_setup`,
`cam_simulate_setup`), which run the same validation as the UI.

Loading is deliberately softer than editing: `decode_project` runs a
softening pass (`CamDocumentDto::soften_for_load`) that migrates legacy
fields (e.g. pre-feed-plane documents get their feed height clamped into
the valid band), repairs stale ids, and force-disables any operation that
still fails validation instead of refusing to open the project. Parked
operations surface as amber warning badges in the browser
(`load_warnings`); fixing and re-saving clears them. A project file can
therefore never be made unopenable by CAM content.

## Browser creation, copying and ordering

The Setups header has one 28-pixel **+** target for New Setup. Tool Library
stays at the bottom of the browser and in the ribbon; the duplicate tiny
header shortcut is removed. Right-clicking a setup also offers New Setup.

Right-click **Duplicate setup** copies its stock/WCS, target-body references,
machine/post snapshot, work offsets, enabled states and all operations. The
copy is inserted immediately after the original with a unique copy name and
fresh setup/operation IDs. Associative height and linking records are remapped
to the copied operation IDs. Existing rest-source links retain their source;
other setups are never redirected to the copy. Project tools are shared by ID,
not duplicated in the tool library. Edit the copied setup's WCS/work offsets
explicitly if it represents a different physical part location.

**Duplicate toolpath** inserts a deep copy directly after the original and
selects it. Both copy actions preserve manufacturing intent but do **not**
copy generation or verification evidence. Enabled copies require regeneration
before NC output; changing shared tool geometry can affect originals and copies.

With a toolpath selected, creating any operation from the ribbon inserts it
**before** that selected path. Selecting the setup (or its stock row) instead
appends to its operation list. The destination setup and anchor are captured
when the dialog opens, so viewport selection or tool picking cannot retarget
the insertion. A deleted anchor gives a clear error instead of appending to an
unintended place. A generation retry edits the inserted draft, never another copy.

Dragging and Option + Up/Down still reorder by stable IDs. Only changed
[generation dependencies](CAM_GENERATION_DEPENDENCIES.md) make paths stale;
the ordered stock simulation and NC sequence are always checked separately.

## Units

Persisted geometry, planned motion, and simulation are always canonical
millimetres. The document carries an operator-facing unit switch (mm / inch)
that can be flipped at any time: every field, readout, and point list in the
workspace displays and accepts input in the chosen unit, and posts emit
matching controller words (`G21`/`G20`, `G710`/`G700`). Switching units never
rewrites stored geometry.

## As-built scope

- Persistent manufacturing intent in `.limo`: setups, WCS (with origin
  provenance), G54-G59 work offset plus duplicate-part count, stock
  definitions (box/cylinder/hex/modeled body; fixed size, model-grown
  allowances, or rest from an earlier setup), tool library with cutting data
  and planner-step defaults (default step-down / step-over seed new
  operations),
  operations with per-operation safe heights, unit preference, and post
  defaults used only to pre-fill the export dialog.
- Deterministic controller-neutral motion planning in millimetres, with
  per-offset program duplication (`G54`, `G55`, … blocks in one program).
- Facing, closed 2D contour, 2D pocket (zigzag clear plus boundary finish
  pass), 2D chamfer (90° chamfer mill with tip-offset control and Automatic
  fitting or Manual entry/exit geometry and feeds), internal
  thread milling (thread mill orbiting a helical path one pitch per
  revolution, split into semicircular arcs; right/left-hand threads, climb or
  conventional direction, optional multi-pass radial stepovers finishing at
  the full orbit; the host resolves the designation to explicit pitch and
  major/minor diameters stored on the operation), and hole
  operations with an explicit cycle family: plain drilling (rapid out), chip
  breaking (peck with an in-hole partial retract), deep drilling (peck with
  full retract), right/left-hand tapping (pitch-derived feed with
  spindle reversal, allowed only after explicit confirmation of a suitable
  floating holder and **not established rigid-tapping synchronization**),
  reaming, and boring (dwell and feed out). Every cycle is
  expanded to explicit longhand motion, so posted output never depends on a
  control's canned-cycle dialect. Helical arcs post as plain G2/G3 blocks
  carrying a Z word in every dialect.
- Multiple depth passes, stepover, and stepdown. Contour radius compensation
  runs in one of two explicit modes. In control (the default) posts the part
  contour as-is and activates machine-side cutter radius compensation
  (G41/G42 with the tool's diameter register on controls that take one,
  cancelled with G40) on the lead-in and lead-out moves, so the shop can
  fine-tune size and wear at the machine. Profile coordinates remain the
  part geometry; only the uncompensated link endpoints and programmed lead
  arcs include the nominal radius needed to make controller activation and
  cancellation physically safe. A radically different cutter still needs
  link-clearance verification and may require reposting.
  In software offsets the path in the planner and posts plain tool-center
  coordinates with no machine compensation. GRBL has no cutter radius
  compensation vocabulary, so the GRBL post refuses in-control programs —
  fail closed with an actionable message rather than emit unoffset motion.
  The simulator honors both modes. For in-control programs it models normal
  controller approach/retract behavior: the activation block moves from the
  uncompensated lead point to the first compensated center point, the
  following profile is offset by the nominal tool radius (mitered at corners;
  arcs tessellate into short chords), and cancellation leaves from the final
  compensated center point. The native Siemens profile explicitly emits
  `NORM` with G41/G42 so posted behavior matches that verification model.
- Straight or arc lead-in/lead-out on contour operations, with
  operator-set lengths and an optional horizontal arc radius that rounds
  each straight lead into a tangential 90° meet with the profile, swinging
  in from the non-material side so the entry corner is never scuffed.
  Lead lengths and arc radius describe the physical cutter-center motion.
  With machine-side compensation, entry and exit points are shifted onto
  that compensated centerline; a requested arc is programmed one tool radius
  larger so the controller's inward offset reconstructs the requested radius
  instead of collapsing a small arc under a large cutter. Outside profiles
  and open chains extend their tangents. An inside machine-compensated loop
  starts along a straight wall segment long enough for both leads plus one
  cutter radius at each neighboring corner, keeping the complete transition
  in the wall band and avoiding ambiguous activation at a sharp inside corner;
  otherwise planning fails closed with guidance to shorten the leads, switch
  to software compensation, or use a smaller cutter. Software-compensated
  inside loops keep the interior-bisector lead. Arc leads into a closed inside
  profile remain unsupported. Positive lead values carry no arbitrary tool-
  diameter floor, though a real control can still reject an activation move
  shorter than its machine-specific minimum. The dialog seeds leads at 1.5x
  the tool radius as a comfortable default, never a limit.
  Vertical lead radii are a documented next slice.
- Contour cuts climb or conventional (the planner re-winds the stored path
  around its start; open chains reverse with the physical side preserved),
  and supports radial multi-pass roughing: `roughing_passes` step toward the
  wall at the roughing stepover leaving the finish allowance, an optional
  finishing pass takes the wall to size at its own feed, and a spring pass
  repeats the final lap once (closed loops) so tool deflection relaxes. In
  control mode only the final profile lap carries G41/G42 — roughing laps
  are pre-offset by the planner. Switching compensation modes never touches
  the picked edge chain: the operation stores its `chain_ref` provenance and
  edit sessions re-select the same viewport entities. Maximum stepdown is
  always on for contour (no multiple-depths toggle). Roughing laps currently
  follow the same exact offset polyline as the profile lap. Rounded/rolling
  roughing corners are deliberately deferred to the shared 2D path kernel:
  doing them safely needs concave-corner handling, self-intersection cleanup,
  and engagement-aware smoothing that can also serve later high-speed paths.
- Facing rows are centered on the face: cutter bands extend one radius past
  each row's center, so the minimal row count spans the face and a face one
  band covers gets exactly one pass through the middle — never a row
  hugging the near edge. Rows zigzag both ways by default; climb or
  conventional runs one way, repositioning at the feed plane above the
  stock between rows. Pocket and chamfer travel climb or conventional
  likewise.
- Every operation carries five resolved heights: clearance, retract, feed,
  top, and (for depth-cutting kinds) bottom — each a reference plane plus a
  signed offset, resolved in the fixed order bottom → top → feed → retract →
  clearance so a row may chain off any lower row. Reference planes cover
  model/stock extremes and the absolute origin; drill and thread operations
  may also hang heights off the picked holes' own span (Hole top = highest
  picked top, Hole bottom = lowest picked bottom). Fresh dialogs seed
  per-kind defaults modeled on established CAM workflows (facing starts a
  skin above the stock top and targets the model top; contours run stock
  top to just below the stock bottom; hole kinds ride the model top or the
  picked holes' span); editing always re-opens the stored absolute values.
  Rapids stop at the feed plane; everything below runs at feed rate. Peck
  re-entries rapid only to just above the last depth, capped at the feed
  plane.
- Viewport-picked holes carry their own geometry: each pick records the
  center in setup XY, the cylindrical face's real top/bottom span in setup
  Z (from the face's own mesh vertices, so stepped bosses and blind bores
  machine across exactly their height), and the unit axis — validated
  parallel to setup Z for fixed-axis planning, with the record kept
  seam-ready for a future indexed/5-axis tool orientation. Manual center
  coordinates still machine across the operation's top/bottom planes; the
  two kinds of targets mix freely in one operation. Editing a drill/thread
  operation re-selects the same faces in the viewport (rebuilt from the
  current model, so edited geometry refreshes the spans); a hole whose face
  vanished degrades to a manual center line instead of dropping out.
  Drilling-family cycles offer Drill tip through bottom with a break-through
  depth: the planner drives the point length (from the tool's stored point
  angle, the conventional 118° when unset) plus the allowance past the
  bottom plane so the full diameter clears the hole bottom. Tapping,
  reaming, and boring stop at the bottom plane by definition and reject
  tip-through at validation.
- Built-in conservative posts for GRBL, LinuxCNC, FANUC, Haas, Mitsubishi,
  Mazak EIA, Syntec, Okuma OSP, Heidenhain/Hermle fixed-axis TNC, and a native
  Siemens 828D reference profile with an explicitly
  confirmed machine-coordinate `SUPA` retract. The post configuration
  is bound to the setup machine snapshot. Program name/number and sequence
  numbers are reviewed at export; all posts honor the document unit switch.
  Controller compatibility and actual compensation switch moves are checked
  before NC output. A profile does not certify machine motion or collisions.
- Private native `.nbpost` JSON machine profiles plus non-executing reference
  source storage/analyzer. Script execution is not implemented; reference
  sources do not generate NC. See [post formats and storage](CAM_POSTS.md).
- A bounded, deterministic 3D voxel stock simulator that consumes the same
  neutral motion IR as the post layer, records per-command removal/time,
  detects rapid/tool contact with remaining stock, and greedily extracts a
  renderer-neutral remaining-stock surface mesh. Initial stock is voxelized
  from the setup's resolved shape: box grid, cylinder/hex prisms, a modeled
  body's mesh, or an earlier setup's simulated remainder (rest chains are
  validated against shared WCS/envelope and cycles). The setup's selected
  OCCT body meshes also enter as the intended finished-part union. An exact
  anisotropic distance transform builds a resolution-aware tolerance band;
  final results separate excess material from protected target loss and every
  cutting step records how many protected cells it removed. The prepared
  target grid is cached behind a bounded host-owned key, so playback frames
  reuse voxelization and both distance transforms instead of rebuilding them.
  Bull-nose cutters use their declared corner radius in the removal envelope.
  Mesh ray columns with an odd number of deduplicated surface crossings are
  counted and disclosed; paired crossings still produce a best-effort volume,
  but closure/tessellation damage can no longer pass silently.
- A React manufacturing workspace that shares the modeling viewport and
  browser: the modeling tree stays in place and gains a Setups section
  (operations listed with their tool tag, `[T<n>]`/`[name]`), while the tool
  library lives in its own full dialog (table plus tabbed editor, with a
  central/project scope switch in the header), not in the browser tree. There is no side inspector panel: double-clicking a
  setup, Stock&WCS, or operation row re-opens the SAME dialog that created it
  (seeded from the stored values), exactly like the modeling feature dialogs;
  right-clicking a setup row opens a context menu (edit, delete), an
  operation row adds suppress/resume — suppressed operations are skipped by
  the planner and the post — and the Stock&WCS row offers edit. The workspace opens directly onto the
  modeled parts (setups
  are created from a centered dialog via the ribbon, never implicitly), with
  manual setup and operation dialogs, viewport point picking for WCS origins
  (solid markers that highlight under the pointer),
  distance/time estimates, warnings, NC export via the
  Post NC dialog, and neutral post-event export. The manufacturing tab mounts
  the very same viewport component as modeling — navigation, grid, ViewCube,
  and model presentation are literally identical — and CAM adds its overlays
  through the viewport's native transient channel (`src/cam/overlay.ts`):
  a translucent stock ghost with envelope edges, RGB WCS axes, the selected
  operation's toolpath (dotted rapids / solid cuts, drawn through geometry),
  identical pure-cone direction markers — green where the cutting feed
  starts, red where the tool leaves the work — planted at the exact start of
  the first feed move of every operation kind; the exit cone parks at the
  final actual motion endpoint on the configured retract plane and points up.
  Milling explicitly stops at retract before continuing to clearance, while
  tapping/reaming feed back to the same endpoint. Face, contour, and drill
  defaults resolve Feed and Retract to the same height, so their two markers
  are level unless the operator deliberately separates those settings,
  oriented along the exact motion tangent (arc tangents from the circle
  geometry, not display chords; rapids never carry markers). Cone size
  follows the model extent alone — a fixed short marker that grows and
  shrinks with the part, identical at both ends of every operation on the
  same model (a drill's short plunge and a facing pass show the same
  cone), and a translucent ghost of the selected
  operation's tool parked at its START position (the first approach target,
  above the entry point — the horizontal offset from the stock boundary
  still reads as one radius plus facing's safe distance), fluted section
  brighter than the shank. The normal simulated view has one physical surface:
  remaining stock in green. Selecting the setup/Stock & WCS row presents its
  incoming stock before the setup's first motion; for a rest setup this is the
  source setup's completed remainder. Selecting an operation presents the
  cumulative stock immediately after that operation. The complete setup or
  operation timeline remains available for playback and background
  verification. Exact model bodies and modeled raw-stock bodies are hidden
  while that stage surface is current, preventing coplanar depth fighting and
  preventing removed areas from appearing filled by the CAD model. X-Ray is
  an explicit reference toggle that brings the finished model back only as a
  faint translucent shell with see-through wireframe edges.

  Finished-part comparison still calculates excess material, protected target
  loss, responsible motion, and the effective comparison band, but those
  amber/magenta evidence meshes are not painted over the normal stock view.
  The compact card and issue panel report the results in the background.
  Operation-truncated results are labeled “through” that operation and
  explicitly say they are compared with the finished part, so intermediate
  excess stock is not presented as a final verdict. The native triangle
  allowance gives the remaining-stock surface up to 60k triangles; if it is
  simplified, both the card and warning panel disclose it while numerical
  volumes and issue counts remain full-detail. Detail presets change both
  requested cell size and the bounded voxel budget; the displayed actual
  cell size remains authoritative. Rapid-contact markers and
  point-pick candidates (solid discs whose fill density adapts to the pixel
  size, so they read as dots at any zoom). The simulation itself is truncated
  at the selected operation (`through_operation_id` on the simulation
  request): selecting the second of four operations shows the cumulative
  result of operations one and two only — later operations have not cut yet,
  so their removal is not shown. Toolpaths and the simulated stock
  render while the selected stage is current and no operation dialog is open
  — clicking empty space clears the selection and returns the viewport to
  the bare model. A status chip at the viewport's lower right reports
  the selected operation's machining time (`h:mm:ss`, from the program's
  per-operation stats) or the whole setup's total when nothing is selected.
  Setup-space planner output is transformed back to
  model coordinates in that one module.

The Rust crate at `crates/cam/` owns validation, path generation, motion IR,
posting, and post-event projection. The React code under `src/cam/` and
`src/components/cam/` edits intent and visualizes generated motion. As with
drawings, generated tool motion is not persisted; it is regenerated from the
saved CAM intent.

## Coordinate and safety contract

- All persisted dimensions and motion coordinates are canonical millimetres;
  the document unit switch changes display and posted output only.
- Each setup has an orthonormal, right-handed WCS derived from the origin the
  operator picked (stock box point, model box point, sketch point, or explicit
  coordinates). Setup `Z+` points away from the stock and remains parallel to
  the spindle axis.
- Safe heights live on each operation: `clearance_z` must be above the stock
  top, and `retract_z` must sit between the operation's cut top and its
  clearance plane.
- Rapid moves (`G0`) are always full-speed; there is no configurable "rapid
  feed". The simulator's time estimate uses a fixed internal rapid constant.
- Operations must remain inside the stock range, reference a compatible tool,
  and stay within that tool's flute length.
- Posted programs start in absolute metric XY-plane mode and explicitly cancel
  common modal compensation/cycles. Each duplicated part block re-emits its
  work offset word (`G54`, `G55`, …) before its toolpaths.

The simulator models volumetric remaining stock, flags rapid contact with that
stock, and compares it with the intended part for excess material and protected
target loss. That comparison is voxel-accurate, not B-rep metrology: its
effective tolerance can never be tighter than one cell diagonal and is shown
to the operator. It does **not** yet check fixtures, clamps, shanks, holders,
machine travel, tool-change positions, or spindle envelopes.
Built-in posts retract Z to the operation's clearance plane before their
first XY move, but that still assumes a correctly set WCS and machine-safe
starting position.
Every output must be simulated, inspected, and dry-run above the workpiece
before machining.

## 3D simulator architecture: planner, OCCT, and Bevy

The simulator is a headless Rust subsystem, not a feature of either OCCT or a
renderer. Two deliberately separate inputs converge on one physical contract:

```text
CAM intent -> planner ---------------------+
                                            +-> neutral 3-axis motion -> voxel removal/collision -> mesh + timeline
final NC text -> strict dialect interpreter+
```

CAM prediction therefore checks the exact neutral `CamProgram` consumed by
every built-in post and by the host-neutral event projection. NC-program mode
instead interprets the final controller text into that same neutral motion
before stock removal. It is not a machine emulator: machine-coordinate
positioning, PLC/macros, tool-length offsets, fixtures, holders, machine
kinematics, and travel limits remain explicit warnings or fail-closed gaps.

The stock engine starts from the setup's resolved shape: a fully occupied
voxel grid for box stock, shape-masked grids for cylinder and hex bar stock, a
mesh-voxelized grid for modeled-body stock, or the remainder voxel grid of the
source setup for rest stock. It
tracks occupancy in a bitset, sweeps the active cutter along linear and
XY/XZ/YZ arc motions, removes intersected cells on feed moves, and checks rapid
moves for tool contact with remaining stock. Each motion produces a timeline
record containing duration, cumulative time, removed-cell count, active tool,
arc geometry, removed protected-target cells, and (for NC input) source line.
A bounded greedy
surface mesher returns renderer-neutral triangle soup and supplies
occupancy-gradient normals on internal vertical cut walls while preserving
flat floors and the stock envelope. Default previews are
limited to 8,000,000 voxels; hard limits are 8,000,000 voxels, 2,000,000 sweep
samples, and 65,536 stock-surface triangles so malformed jobs fail closed
instead of exhausting the host.

The cutter envelope and displayed tool share one Rust profile. Flat and face
mills use their declared sharp/radiused/chamfered corners; ball and bull-nose
mills use their revolved tip profiles; drills use the declared
point angle (or a disclosed 118-degree fallback), and chamfer mills use the
declared conical profile capped at the tool diameter. The chamfer tool model is
currently sharp-tip; tip diameter is not yet a library field. See
[cutter geometry, UI and performance](CAM_CUTTER_GEOMETRY.md).

Simulation density is model-relative, never camera-relative. Auto targets 352
cells along the stock's longest side; that is 10.65 times the former Auto
volumetric sample density (352 cubed versus 160 cubed), or about 2.2 times
finer on each axis. Fine/Balanced/Fast target 512/256/128 respectively under
8M/4M/1M bounded voxel budgets. A budget may coarsen the requested edge, and
the workspace always shows the actual result. Camera zoom and viewport pixel
count do not alter the removal grid or verification values.

OCCT remains the authority for exact CAD B-reps, feature replay, and target
part tessellation. The selected setup bodies from `SolidSceneDto` are
transformed from model coordinates into the setup WCS and voxelized as a union
on the same grid as stock. Rust computes Euclidean distance bands, classifies
remaining cells outside the accepted target region, protects target core cells
from unreported removal, and emits presentation meshes and motion-attributed
findings. A complete simulation prepares this invariant target grid in a
bounded cache; block-boundary playback requests send only its opaque key.
Geometry, WCS, grid, or tolerance changes create or validate a different
prepared input. Odd ray-crossing parity is retained as target-quality metadata
and reappears on every cached playback result. OCCT is not in the frame-by-frame removal loop: fixture/holder
collisions and exact metrology still need explicit geometry contracts.
Avoiding repeated topology-changing OCCT booleans keeps interactive simulation
deterministic and bounded.

Bevy remains a presentation layer. On desktop, remaining-stock triangles move
directly from the Rust simulator into a dedicated retained Bevy resource; they
never serialize through React or share the transient command-preview channel.
Browser/WASM keeps the renderer-neutral mesh fallback. Remaining stock uses an
opaque, depth-writing, studio-lit material with occupancy-gradient normals;
command/profile fills remain alpha-blended overlays. A bounded dual-cell
relaxation moves only internal display vertices toward the binary isosurface,
softening circular/conical voxel stairs without changing one occupied cell or
one verification measurement. Both Bevy camera passes use 8x MSAA.

The animated cutter is a retained semantic Bevy primitive whose transform
changes without rebuilding static stock or toolpath layers. React advances a
renderer-neutral timeline clock; Bevy continues to own camera input, so
orbit/pan/zoom remain live while the cutter plays. Cutter pose is interpolated
continuously, including helical arcs in all three principal planes. The desktop
kernel runs on a cancellable blocking Rust worker outside the project engine
lock. A full run keeps bounded one-bit-per-voxel checkpoints at operation
boundaries: stage selection uses them directly, and playback inside a later
operation resumes after the closest completed operation rather than recutting
all earlier paths. The cache holds at most four setup entries / 32 MiB and
falls back to a fresh deterministic run after eviction. Remaining stock still
commits at completed physical motion-block boundaries; a future narrow-band
checkpoint stream can make long individual blocks update more continuously.

The workspace displays the actual maximum cell edge as **3D detail** and
renders the simulator's own accuracy/coverage warnings (deduplicated from
planner warnings), so a blocky boundary cannot silently be mistaken for exact
stock. Bevy must not own material removal or safety decisions.

### NC-program input currently accepted

The strict workpiece interpreter accepts metric/inch modal coordinates,
G0/G1/G2/G3, G17/G18/G19, G40/G41/G42, G54-G59, spindle/coolant/tool changes,
`R`/`CR=` and IJK arcs, and standard feed-per-minute programs. Siemens 828D
mode additionally understands G70/G71/G700/G710, G505/G506 fixture aliases,
`SUPA` exclusion, plane
selection by the two programmed endpoint axes, named or numbered tools,
preload calls that do not become active until M6, and modal CYCLE81 drilling.
CYCLE81 is expanded to its documented rapid-to-safety-plane, feed-to-depth,
rapid-to-retract sequence. After excluded `SUPA`/G53 or tool-change movement,
an explicit zero-motion position record re-establishes the workpiece pose; the
simulator never invents a connecting sweep across hidden machine travel.
Changed work offsets also re-establish XYZ without joining different frames.
G700/G710 set geometry and feed units. G70/G71 change geometry only: existing
physical modal feed is retained, but new F words require unknown machine-base
units and fail closed. Verify and convert external feeds before choosing an
extended unit mode; do not blindly replace the unit word.
Generic ISO input reads `G4 P...` in seconds; explicit Fanuc-style input reads
the same P word in milliseconds, matching the built-in post. Siemens accepts
its native `G4 F...` seconds as well as P seconds. Compensation cancellation
on a rapid block consumes the real compensated tool-center endpoint before any
later reposition or plunge, so a stale offset cannot create a phantom cut.
Haas distinguishes integer P milliseconds from decimal P seconds. New built-in
ISO post headers let Auto select the correct dwell interpretation. OSP/TNC
replay is explicitly unsupported, even though those formats can be generated.
Unsupported canned cycles, rotary axes, inverse-
time/feed-per-revolution motion, and controller flow control fail closed rather
than being guessed.

## Post-processor ecosystem decision

### We still need a small Limo CAD post boundary

There is no broadly adopted open plug-in ABI that lets a CAM system hand the
same rich job object to proprietary CAM posts, LinuxCNC, Mach, Fanuc, and
SINUMERIK posts.
RS274/NGC is a useful public G-code dialect and the NIST interpreter exposes a
well-documented set of canonical machining functions, but those are a
controller language and interpreter contract rather than a portable CAM post
plug-in API. ISO 14649 / STEP-NC defines a higher-level CNC data model, but it
is not the post ecosystem deployed on the machines targeted here.

Limo CAD therefore owns a deliberately small internal boundary:

`persistent CAM intent -> neutral motion IR -> simulator + post adapters`

That boundary is not intended to invent another public standard. It keeps the
planner and simulator independent of controller syntax, lets built-in posts
remain testable, and gives third-party adapters one stable input.

Public references:

- [NIST RS274/NGC Interpreter](https://www.nist.gov/publications/nist-rs274ngc-interpreter-version-3)
- [LinuxCNC G-code overview](https://linuxcnc.org/docs/html/gcode/overview.html)
- [ISO 14649-1 / STEP-NC overview](https://www.iso.org/standard/34743.html)

### `.nbpost`: the noBS file association for user-supplied posts

The noBS extension is `.nbpost`. A user may deliberately rename a compatible
post they are entitled to use to `.nbpost`; Limo CAD itself does not copy,
convert, redistribute, or silently import third-party post files. Renaming
changes only the local file association. It does not change copyright, license
terms, source syntax, or the script's dependence on its original host runtime.

A callback post is not necessarily standalone JavaScript. It may expect a
host-provided API with section, tool, machine, cycle, property, formatting,
and file-system objects. Renaming a file therefore does not imply that it can
run in Limo CAD.

The implemented v1 `.nbpost` slice is deliberately non-executing:

- accepts UTF-8 `.nbpost` files up to 2 MiB;
- lexes function declarations without evaluating JavaScript;
- detects lifecycle and motion callbacks in the supported shape;
- reports callbacks outside the planned fixed 3-axis v1 surface;
- detects the presence of rights/license notices; and
- keeps the source in memory only—it is not persisted in `.limo`.

The UI says **analysis only** and the engine always returns `runnable: false`.
Actual execution must wait for a resource-bounded sandbox, a versioned host
object API, deterministic output capture, and fail-closed handling for every
unsupported callback and controller feature.

The existing adapter also exports a versioned `limo-cad-post-events` JSON stream
using the callback names recognized by the analyzer. It is an integration seam,
not a third-party intermediate file and not a `.nbpost` runner.

### Third-party post legal and provenance guardrails

Third-party post files and their host runtimes may be proprietary even when a
user is entitled to run or customize them locally. The native Siemens
implementation here was written from operator-provided NC behavior and public
controller programming documentation, not by copying a third-party post
implementation. This is an engineering policy, not legal advice:

- Do not copy a third-party runtime, intermediate format, or post
  implementation into Limo CAD without a compatible license.
- Treat every supplied post as third-party source until its header, author,
  and license are reviewed. Keep ambiguous files outside the repository.
- Known-good NC output and user-authored behavioral requirements may be stored
  as golden fixtures with the provider's permission; implement resulting post
  behavior independently against public controller documentation.
- If a user owns a custom post and can relicense it, record that provenance in
  the contribution before publishing derived code.
- Do not upload, bundle, publish, or serve a user's post by default. Local
  selection and in-memory execution are materially narrower than distribution.
- Preserve notices. Make the user confirm that they are entitled to use the
  post and that machine output remains their responsibility.
- Use descriptive compatibility wording only after the compatibility suite
  passes—for example, “supports a documented subset of user-supplied callback
  posts.” Do not imply third-party sponsorship or certification and do not use
  third-party logos without permission.

Recommended phases:

1. Validate the new native Siemens 828D reference profile on the actual machine
   using progressively richer known-good programs. It already fails closed
   without an explicit `SUPA` retract profile.
2. Expand tested controller-specific starters (see [current posts](CAM_POSTS.md)).
   Mach3/Mach4 and Siemens ISO remain future profiles, not aliases for FANUC.
3. Grow the neutral motion IR and post event/metadata projection alongside
   new operations and golden NC fixtures.
4. Implement the `.nbpost` sandbox and minimum callback host API independently
   from public specifications, with per-post compatibility reports and no
   proprietary posts in the repository.
5. Claim compatibility only for fixtures that demonstrate the exact supported
   subset; fail closed on unsupported callbacks, cycles, machine kinematics,
   or file/runtime APIs.

### Siemens 828D native mode versus ISO mode

Treat these as two controller profiles, not aliases. SINUMERIK uses `G290` to
select the native Siemens language and `G291` to select its ISO dialect. The
828D manuals document compatibility differences and state that controller
state such as the active tool, offsets, and work coordinate system survives a
dialect switch. In practice the ISO profile is deliberately Fanuc-like and
will cover many common blocks, but Siemens-specific modal behavior, cycles,
tool calls, and machine-builder M-codes still require an 828D golden test set.

The first production target is native Siemens mode because it can be checked
on the user's real 828D. The ISO/Fanuc-style profile follows as a separate
compatibility target rather than reusing the generic Fanuc post unchanged.

### Current native Siemens 828D contract

The reference profile is calibrated from the supplied known-good MPF and emits
the following independently implemented fixed 3-axis subset:

- `; %_N_<NAME>_MPF` program envelope;
- selected `G54`-`G59`, then `G17 G710 G90 G94` and `G64`;
- profile-controlled tool-change positioning: `SUPA Z`, controller-managed
  `M6`, or `SUPA Z` followed by a verified fixed machine `X/Y` station;
- unnumbered `MSG ("operation")` records;
- tool calls directly from project-library numbers or exact, case-sensitive
  names (`T="NAME"`); no separate mapping form and no rewriting of names;
- separate `T...`, `M6`, and configured `D...` blocks;
- optional `M1` before the second and later tool changes (off by default;
  explicit saved choices are retained);
- optional, explicitly enabled next-tool `T...` preload immediately after
  `M6`/`D...`;
- `S... M3/M4`, `M5`, and `M7/M8/M9`;
- absolute XYZ rapid/linear motion, XY-plane `G2/G3` with `I/J`, and native
  `G4 F...` dwell seconds; and
- final `SUPA` retract, tool edge restore, and `M30`.

Sequence numbers, when selected, start at `N10` and advance by one like the
reference MPF. Coordinates use up to five decimal places. Next-tool preload is
safe-off by default; when explicitly enabled, it follows the reference MPF by
staging the next program tool after each `M6`/`D...` and wrapping the last
preload to the first tool when they differ. The post currently omits
`WORKPIECE`, canned `MCALL` cycles, non-XY arcs and all machine-builder/shop
macros. Cutter compensation uses explicit NORM/G451 with the conservative
intersection contract described in [machine-aware CAM](CAM_MACHINES.md).
The operator's custom spindle-slowdown behavior remains in private reference
storage, not the standard 828D implementation. Drilling is expanded into explicit
motions until canned-cycle semantics are separately validated.

Other controller starters and per-user post storage are documented in
[Posts and private machine profiles](CAM_POSTS.md). Source scripts are not
executable plug-ins; a native profile selects one of our built-in Rust renderers.

#### ATC style is not a motion contract

`SUPA Z0` is a common and attractive convention on 3-axis VMCs whose machine
zero is the fully retracted spindle position, including many double-arm
installations. It is not guaranteed by the 828D or by the visual style of the
changer. Siemens defines tool and pallet change points in the machine
coordinate system, whose zero and physical layout are set by the machine-tool
builder. `SUPA` suppresses frames and offsets for only that block; therefore
`Z0` means the builder's machine-coordinate zero, not a controller-wide ATC
standard.

Siemens documents the same `T...` selection plus `M6` activation contract for
milling machines with chain, rotary-plate, or box magazines and repeatedly
defers configuration details to the machine manufacturer. Manufacturer
documentation reaches the same practical conclusion: Haas publishes a
calibrated Z-axis tool-change offset for both umbrella and side-mount/double-arm
changers. Some controls put that motion inside `M6`; others require or permit a
post/macro to position first.

The Siemens post therefore stores two independent settings:

- **Changer style**: double arm, umbrella/shuttle, carousel/chain/wheel, or
  custom. This changes guidance and the displayed example only; a regression
  test guarantees that it cannot silently change emitted motion.
- **Positioning before `M6`**: `SUPA Z -> M6`, controller/PLC managed, or
  `SUPA Z -> fixed machine X/Y -> M6`. Fixed-station posting fails closed until
  both coordinates are entered. Z always moves first.
- **Allow next-tool T preload**: disabled by default. When disabled, every
  executable `T...` belongs to the `M6` immediately following it. Enabling it
  reproduces the supplied program's `T current -> M6 -> D -> T next` pattern,
  but only for a machine whose manual or known-good output confirms that an
  early `T` call can safely stage the magazine. Carousel/chain/wheel machines
  must not inherit this from their visual style.

The Post NC dialog renders the exact later-tool-change example for the
selected strategy and includes the following `T...` preload only when enabled.
Examples are templates for comparison with the machine manual and a known-good
program, not universal snippets. The same dialog also carries the `.nbpost`
compatibility inspector (moved here when the side inspector was retired in
favor of the shared create/edit dialogs).

- [Siemens Fundamentals programming guide (G710 metric mode)](https://support.industry.siemens.com/cs/attachments/48013055/PG_0710_en_en-US.pdf)
- [Siemens Fundamentals programming guide (G4 dwell with `F` seconds)](https://support.industry.siemens.com/cs/attachments/108679566/PG_1102_en.pdf)
- [Siemens 828D functions manual (WORKPIECE and CYCLE81 examples)](https://support.industry.siemens.com/cs/attachments/109977633/828D_smte_fct_man_1224_en-US.pdf)
- [Siemens Fundamentals: machine coordinates contain tool/pallet change points](https://support.industry.siemens.com/cs/attachments/57038573/PG_0911_en_en-US.pdf)
- [Siemens 828D NC programming: `T`, `M6`, and machine-builder configuration](https://support.industry.siemens.com/cs/attachments/109823259/828D_ncprogramming_progr_man_0723_en-US.pdf)
- [Haas umbrella changer Z-axis tool-change offset](https://www.haascnc.com/service/online-manuals/mill-tool-changer---service-manual/umbrella-tool-changer---alignment-.html)
- [Haas side-mount/double-arm Z-axis tool-change offset](https://www.haascnc.com/service/online-manuals/mill-tool-changer---service-manual/side-mount-tool-changer---alignment.html)

- [SINUMERIK 828D ISO Milling Programming Manual](https://support.industry.siemens.com/cs/attachments/download/109801226/ONE_840Dsl_828D_iso_milling_progr_man_0721_en-US.pdf)
- [SINUMERIK ISO Dialects Function Manual](https://support.industry.siemens.com/cs/attachments/109813912/ONE_iso_dialects_fct_man_0722_en-US.pdf)

### Why not NX first

Siemens NX Post Configurator and Post Hub are strong industrial systems, but
they are tightly integrated with NX CAM's Machine Output Manager data model and
NX deployment workflow:

- [Siemens Post Hub overview](https://blogs.sw.siemens.com/nx-manufacturing/post-hub-a-cloud-based-postprocessor-solution-for-nx-cam-software/)
- [Siemens Post Configurator introduction](https://blogs.sw.siemens.com/nx-manufacturing/wp-content/uploads/sites/15/2019/09/01_Post-Configurator-Enablement_Introduction.pdf)

That coupling and target audience make NX compatibility substantially more
expensive than the current callback-adapter work. Defer it until the core CAM
model, machine definitions, cycles, and verification layer are mature.

## Next engineering slices

1. **CAM stabilization gate (landed 2026-08-28).** A representative
   face/contour/drill job crosses save/reopen, controller-neutral planning,
   native Siemens posting, and stock simulation in one regression. The gate
   also pins retract-before-clearance, normal cutter-compensation transitions,
   operator-visible simulator warnings, and actual preview resolution.
2. **Dual-input simulator/playback foundation (landed 2026-08-29).** CAM
   prediction and strict final-NC interpretation converge on one physical
   timeline and voxel kernel. Both play in the shared 3D viewport with live
   orbiting, smooth tool pose, time/speed/scrub controls, and deterministic
   block-boundary stock frames. Siemens 828D is the first controller language;
   the supplied production MPF parses through its multi-plane arcs,
   compensation, tool preloads, and modal CYCLE81 drilling.
3. **Target-aware CAM verification (landed 2026-08-29).** Selected setup
   bodies now define the intended part. The shared kernel reports excess stock,
   protected target loss, effective tolerance, and the responsible physical
   motion. Bevy presents the selected stage's remaining stock as one green
   surface; comparison evidence stays in the compact card and issue stream so
   it cannot obscure or depth-fight the physical stock. The same contract
   works for CAM prediction and final-NC input without duplicating removal
   behavior.
4. **Simulator confidence hardening (active).** Bull-nose corner-radius
   removal, compensation cancellation on rapid blocks, target-grid playback
   caching, bounded operation-stage stock checkpoints/resume, cancellable
   background Rust execution, direct Rust-to-Bevy stock delivery, modeled-stock
   rest chains, dual-cell display relaxation, gradient normals, and opaque lit
   presentation are landed. Add accurate envelopes for the remaining supported
   cutters; shaft/holder and fixture collisions; a true bounded narrow-band
   signed-distance display shell; and finer checkpoints for continuous removal
   within long individual motion blocks. The occupied voxel bitset remains the
   deterministic verification authority. Keep OCCT as exact
   target/fixture geometry, the headless Rust subsystem as the removal/safety
   authority, and Bevy as presentation. Machine envelopes remain a later,
   explicitly machine-aware layer.
5. **Shared robust 2D geometry kernel.** Implement polygon offsets with
   miter/arc joins and self-intersection cleanup, clipping/booleans,
   tangent-arc fitting, and engagement-aware smoothing. This is the
   prerequisite for rounded contour roughing, safe concave transitions,
   rest-boundary reasoning, and later high-speed/3D path families.
6. **Remaining 2.5D linking and operations.** Ramp/helical entry,
   keep-tool-down linking, vertical lead radii, tabs, bore milling, and the
   remaining thread-milling variants belong after the shared geometry base.
7. **High Speed Roughing (experimental preview added 2026-09-06).** An
   original Rust circular-patch rougher now uses a conservative target upper
   envelope, engagement bounds at sampled lap stations, helical entry, and
   certified cleared-stock links. It has its own bounded circle/distance
   geometry rather than depending on unfinished polygon offsets. This does
   not close the production gate: continuous engagement bounds, efficient
   ordering, fine step-ups, rest inputs, and holder/fixture checks remain.
   See [CAM_ADAPTIVE.md](CAM_ADAPTIVE.md) and the clearance scope in
   [CAM_LINKING.md](CAM_LINKING.md).
8. **Controller equivalence and machine verification.** Compare interpreted
   final NC with planner motion, broaden the strict ISO/LinuxCNC/Mach dialect
   suites, and add golden controller programs. Tool-length compensation,
   fixtures, limits, safe home/tool-change policies, and machine geometry join
   only in this explicitly machine-aware layer.
