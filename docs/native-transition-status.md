# Native transition status

Current checkpoint, **2026-10-10 UTC**: PR124 remains open. Main advanced to
54dcab7f after PR338 merged; Bevy includes that history through merge 08cdd6b9.
Incoming research/license corrections were preserved. No Bevy merge into main
has occurred. The user approved continued UI-first coverage and repair of all
posted defects, including newly reported defects; passing CI does not close the
remaining UI/MCP qualification gates.

Current direction: **new exploratory UI coverage is paused**. Work is limited to
known-defect implementation and planned targeted acceptance of repaired behavior.
Preserve earlier passes; do not advance to untouched catalog items or rerun passing
cases. The user handed over shared runtime ownership on 2026-10-10. One guarded
Rust GUI operator is saving and closing the open documents before deployment;
saved/recovery data and the other branch's models remain preserved.

The existing automated matrix is green at `f7aacc2c`: native hosts on all three
platforms, MCP acceptance, package targets and CodeQL passed. Those results
qualify that source and their stated automated scopes. They do not qualify the
new repairs below or replace the outstanding human UI receipts. No passing
suite was rerun locally.

Post-checkpoint CI at `521f9e29` exposed two remaining failures: the native
handoff panel borrowed an owned formatted caption (`a7ed584e` corrects that
strict Clippy error), and the native revolved-tube summary returned unknown
through extent on both Windows and Ubuntu. `7f516a04` moves shell evidence
ahead of the extent assertion and includes bounded face/ring/seam context.
The expected proof is unchanged; no unsupported topology is accepted to make
the test pass. Fresh Ubuntu and Windows core diagnostics at `647772c3` proved
that all four faces have valid outer-shell evidence, but the lower annular cap
incorrectly exports a +Y normal. OCCT can represent a plane with an indirect
coordinate frame: its parametric normal is XDirection cross YDirection, rather
than its main axis. `b1fa867a` corrects the shared normal transfer and extends
the existing native revolve fixture with independent cap and mesh-normal checks.
Validation of the correction is pending. Both core jobs ran all 118 native
unit tests and their integration binaries successfully after the MCP failure.

Native geometry CI now retains independent core regressions after an MCP
assertion failure, provided native lint succeeded and the run was not cancelled.
MCP failures still fail their job and aggregate gate; workshop/demo publication
keeps the normal success prerequisite. The two focused workflow contract checks
passed once locally, including rejection of cancelled/unprepared execution and
loss of the prerequisite identity. No application build or geometry replay was
used for that check.

The automated `native-exact-thread-precision` blocker is superseded by existing
hosted evidence at `43c20c30c66c3cdadc34256747376222333ca5ba`, run
[38055144113](https://github.com/limo-cad/Limo-CAD/actions/runs/38055144113).
The exact `external_threads_reject_wrong_faces_validate_fits_and_edit_original_cylinder_atomically`
case passed Ubuntu job114221952756 (line2619), Windows job114221952850
(line1609), and macOS job114221952854 (line1451). Receipt:
`D:/limo-cad-maintenance/defect-closure-20261010/native-exact-thread-superseding.json`.
No new tests were run for this reconciliation. This supersedes only that automated
blocker, not human GUI export, Medix STEP, or later-source qualification. Earlier
failed receipts remain historical evidence.

Known #348 source repair `ae63529b` preserves ambiguous cylindrical surfaces with
reasons and body/face provenance without increasing bore counts. It checks wall
orientation, requires topology connectivity before merging coaxial candidates,
and exposes geometry candidates separately when authored holes exist. Simple
blind depth requires two analytic rings and one disk cap; `8820f4fa` locates that
candidate at its mouth and points its normal from cap toward mouth. These are
conservative geometry candidates, not a manufacturing certificate. `57b7cec3`
requires an oriented, coplanar bottom; `e2397070` derives recess depth from two
unsplit walls and a proven annular shoulder, plus total blind depth when a terminal
disk proves it. `b0b01d0f` publishes exact face-owned analytic linear-seam evidence
from OCCT; `e3a19f89` accepts that evidence with strict ownership and axial endpoint
checks, retaining unknown depth for unsupported topology. Independent source
reviews found no remaining blocker in that earlier patch. `3ae8a846` now derives
through status only from two exact annular openings on a proven native outer
shell. Validity, closed-shell membership and outward solid orientation are
required; legacy data, sealed cavities, compounds, open surfaces and unsupported
opening topology retain unknown status. `83559707` bounds this optional analysis
to small analytic topology, skips disposable section/export meshes, and keeps it
out of UI frames and borrowed summary reads. Native positive and negative
regressions are committed but not run locally. Real imported assembly/touching
acceptance and new-head hosted validation remain pending. No GUI deployment is
claimed.

The concrete exact-plane hatch failure posted on #350 is distinct from that
issue's deferred wall-thickness feature. The section repair constructs each
solid's planar material region, subtracts coplanar exterior termination patches
per solid, and unions material before extracting complete oriented boundaries.
It does not move the section plane or change material based on the retained
3D side. Pure contact and imported open-surface outlines remain visible drawing
geometry without becoming hatch area. Shared topology/comparison allowances
and cooperative OCCT progress checks bound the added work; this is not hard
process containment. Focused native regressions cover the exact pocket endpoint,
both cutaway volumes, overlapping compound regions, signed drawing normals,
contact/open-surface compatibility and source preservation. Those focused native
regressions passed on Windows and Ubuntu at `647772c3`. The hosted vise exposed
a separate efficiency regression: querying the whole boundary compound charged
near-whole-solid pair work twice. The repair distributes exact plane Common
over each candidate boundary face under the same shared allowance and deadline,
then collects only contact faces for the existing per-solid subtraction. Planar
and unknown/general surfaces retain exact evaluation; only proven nondegenerate
curved analytic supports are excluded. Source review is complete; corrected
vise execution and actual-file/runtime acceptance remain pending.

For #363, `a48a2f33` strengthens the existing tangent-arch/circular-hole native
fixture with exact volume and default/fine closed-mesh export checks. The fixture
exercises circular-boundary comparison; it does not prove inserted-sample or
exhaustion paths execute. Existing budget-helper evidence covers exhaustion and
shared allowances. A deeper audit found that junction recovery reset a local
comparison allowance and swallowed resource exhaustion. `f2f07958` charges
comparisons and recovery storage to the shared budget and preserves the typed
exhaustion error through rollback. Its standalone C++ budget-helper check passed
once with strict warnings; this is not native geometry or GUI acceptance. See
`qualification/refinement-budget-helper-20261010.json`. New-head hosted native
acceptance remains pending.

For #316, `a74bc03c` permits read-only inspection of native process patterns
outside the managed writer's typed capability. Raw layer, width, shell and
support settings retain process/object/volume provenance in the handoff report;
unsupported patterns still prevent managed qualification. `5dba77aa` bounds the
report before per-part copies, and `2f0c6ae6` requires complete managed scalar
readback. Hosted checks and one targeted native display acceptance are pending.
Prior successful print workflows remain preserved; failed repeat-identity
receipts are not reclassified as passes.

For #360, `c0e1b3b0` adds focused shared-request policy regressions for rejected
Add/Create Body Through All requests and edits. Exact model/document preservation,
finite recovery and subsequent ID allocation are compared with an untouched
control. These planning-stub checks do not claim native geometry or manual MCP
acceptance. Existing human UI acceptance remains valid for its covered behavior.

Concentric UI coverage is also closed on clean matched `93823f6d`: distinct
radii preserved, shared center, invalid selection/cancel, retained deletion,
exact Undo/Redo and complete native persistence. Receipt:
`qualification/concentric-human-ui-20261010.json`. Current UI totals are47 passed,
332 not run,29 failed,9 needs_recheck,24 in progress,2 restricted. All256 manual
MCP cases remain not run. Symmetry is the next untouched catalog item, currently
paused; only targeted known-repair acceptance remains planned.

Deployment remains pending. Multiple recent Vita GUI windows share the canonical
runtime; the user's later handover supersedes the earlier ownership hold in NATS
notice `0e11d4b0-8393-4b7a-8454-d62b57ba2f47`. The current live GUI/MCP pair was
rediscovered and reports matched clean `93823f6d`; guarded focus succeeded.
Document saving and window closure are underway before rebuilding the corrected
source. Verified preservation copies are outside the repository. Bounded lossless compression of inactive
archived compiler/runtime artifacts restored approximately1.3GiB on C: and3.2GiB
on D:, with before/after hashes and hardlink identity preserved. No document,
recovery or session data was moved/deleted; no native application build has started.

Adversarial review follow-up: `3221fd94` also guards ordinary tab/window close
and Save All/Exit against losing unapplied drawing fields. `51850584` borrows
retained archive JSON during cold-tab restoration; `2bc49000` releases stale
assembly snapshots while the browser is hidden. These remain undeployed.

New Tangent UI inspection found a real branch-roundoff defect: disjoint unequal
circles jump to internal tangency. `d8796bc6` preserves the external branch at
its solved floating-point boundary; a small exact-source arithmetic probe
reproduced the old error and passed the correction. A session regression awaits
CI and targeted live acceptance. Line-circle geometry/history/cancel and full
native persistence passed; the overall command remains failed until repaired
acceptance. Receipt: `qualification/tangent-human-ui-20261010.json`.

Additional implementation checkpoint: `26347ace` shares drawing editor snapshots;
`34ca87bf` protects unapplied drawing drafts across document switches. `720c869c`
avoids native placement-buffer copies on projection cache hits. `a1a384d4` borrows
scene/assembly data during motion evaluation instead of cloning whole models;
`612c6af6` removes unrelated print-document copies during height editing.
`1b14e643` fixes the fixed-size chunk API lint reported by both hosted MCP jobs.
Formatting/source review completed; new desktop regressions await hosted CI.
No repeated local build or passing test sweep was run for this batch.

Equal UI coverage is closed on matched clean `93823f6d`: line lengths, circle
radii, invalid mixed geometry/cancel, retained deletion, exact Undo/Redo and
complete native save/reopen equality pass. See `qualification/equal-human-ui-20261010.json`.
Equal increased UI coverage to46 passed cases; subsequent Concentric brings the current total to47. MCP coverage remains pending after UI.

Parallel implementation checkpoint: `27ebe7f0` checks face-hosted sketch support
at its exact history prefix instead of incorrectly requiring the face to survive
the final Common result. `69238177`, `f3b85652` and `a9d73ca9` share immutable CAM
geometry/toolpath snapshots and release inactive documents while preserving
dirty drafts. `5847c981` retains dimension-editor focus across repeated label
clicks. These source changes are not yet deployed or manually qualified.

Native filename reproduction on matched `93823f6d` now has a preserved
`input_incomplete` receipt: four completed input scalars, three verified, visible
`D:\`. `e46cb42e` requires unchanged acceptable readback samples before the next
scalar, resets after unstable snapshots, and never resends input. Two focused
settling/selection logic tests pass; Windows integration and a targeted live
recheck remain pending. The Save As dialog was cancelled without saving, with
closure confirmed by a fresh Bevy-window observation. An earlier Escape only
dismissed transient dialog state and did not close it.

The two focused history-support regressions pass, including native prefix cache
invalidation and failed replay. The isolated five-DOF collinear drag regression
reproduces nonconvergence. A proposed damping-floor change did not fix it and was
reverted. `0de075d6` instead seeds an isolated free collinear group geometrically;
its new regression passes all twelve endpoint movements and conflicting-pin
atomic rejection. Independent source review found no blocking issue; live
recheck remains pending. Ctrl+Shift+S also
reproduced its no-dialog failure on a fresh foreground empty document. Its cause
remains unattributed; `16f487c0` adds an opt-in bounded file-shortcut decision trace
for the next deployment (`LIMO_CAD_FILE_SHORTCUT_DIAGNOSTICS=1`). No raw typed text
or filenames are recorded. Neither failure reopens passing Parallel or fillet
geometry cases. Broad tests and manual replay of passing cases were not run.

Perpendicular now passes its first representative UI acceptance: two free sloped
lines, invalid point selection/cancel, retained deletion, exact Undo/Redo and
native save/reopen with whole-model equality. The checkpoint and
`docs/qualification/perpendicular-human-ui-20261010.json` retain the evidence.
UI coverage is now 45 passed, 336 not run, 28 failed, nine needing recheck,
23 in progress and two restricted. All 256 manual MCP cases remain not run.
Equal is next; these results do not qualify the newly committed source fixes.

Posted-defect repairs: 18a863f6 addresses #363 with a shared circular-refinement
comparison/sample/insertion budget across both healing stages. The standalone
C++ exhaustion/overflow checks and freshly compiled native rounded-thread mating
and analytic circular-hole regressions pass. Commit 8e951700 addresses #362:
distinct control-enabled Windows package names, capability/provenance manifests,
and a default-release publication guard. Both focused Rust Windows package tests
and adversarial Python identity checks pass. Hosted checks and actual release
publication remain separate gates; neither change is deployed to the GUI yet.

Follow-up source repairs: 75883438 rejects the silently ignored `named_view`
argument on `cad_interface action=view` and points callers to `recall_named_view`
(#347). f11a0785 adds body/face provenance and candidate confidence to inferred
holes, and makes projected-axis overlap warnings explicitly unconfirmed (#348).
All three focused MCP regressions pass, including real OCCT revolved cavities,
fillets/chamfers/holes, and named-camera argument rejection. These are source
regressions, not Phase 2 manual MCP qualification or closure of every issue
acceptance criterion. Local desktop test compilation exhausted host memory;
the new desktop regression awaits hosted CI rather than another local rebuild.

The installed GUI/MCP remains clean 93823f6d. A fresh matched pair qualified
retained fillet R5 to R7 to R3 edits, exact Undo/Redo and native save/reopen model
persistence. R20 consumed the carrier, R21 rejected without geometry changes,
and shrinking to R3 restored the exact baseline. A second upper R2 fillet was
created correctly. See `docs/qualification/fillet-repair-20261010.json`.
The two-fillet shared-carrier boundary now passes: lower R19 rejects safely,
R18 consumes the shared carrier without moving the upper R2 fillet, and shrink
to R3 restores the exact entities/constraints. The saved two-fillet model equals
the live model. Rejected dimension Enter also loses text-field focus;
that separate polish defect has source repair e5afb473, pending hosted checks
and one targeted UI recheck on a clean deployment.
No background pointer input or replacement of the separate Roller window was
attempted. The earlier checkpoints below are retained historical evidence.

Broad UI coverage resumed with Parallel: selection/cancel, two free sloped
lines, incorrect point selection, retained relation deletion, exact Undo/Redo,
and native save/reopen pass on the matched 93823f6d pair. See
`docs/qualification/parallel-human-ui-20261010.json` and the two new UI-built
checkpoints. UI leaf coverage is 44 passed, 337 not run, 28 failed, nine needing
recheck, 23 in progress and two restricted; all 256 manual MCP cases remain
not run. Perpendicular is next. No suite rerun or runtime restart was needed.

The resumed d5af845b native build compiled and linked successfully in 9m16s.
Promotion correctly stopped because canonical MCP workers remained running.
The GUI exited through Save all and close; 78 saved model contents stayed exact,
and 192 session models plus archives are preserved separately. Lossless cache
compression preserves compiler files. Clean 93823f6d compiled and linked in
5m45s, then deployed through the supported
restart/launch path. All 70 payload hashes pass. Fresh GUI PID34320 and MCP
PID25152 report a matched clean pair, SHA256
45a253fce260fd6d7454d12d62cc037ebe8a6695a237e4f559f8d412bec2681a.
The separate Roller GUI PID30300 remains intact. Windows denied foreground
activation even though CAD acknowledged it; no input was sent. Targeted repair
rechecks await actual foreground ownership. Current live UI acceptance is incomplete.

Fresh continuation: the earlier canonical GUI exited between turns for an
unattributed reason; no input preceded its detection and no Windows crash
report was found. Canonical relaunch PID36700/MCP47360 reports matched 93823f6d
and foreground ownership. Rib additive Through All refusal/finite recovery,
Coincident point-only framing/Fit grips, Midpoint mixed framing and H/V framing
passed targeted human UI rechecks with whole-model preservation. Other cases
and the broad UI/MCP gates remain unfinished; Parallel is next in catalog order.

Release qualification checkpoint: **2026-10-06 UTC**, with the local UI walkthrough
updated **2026-10-08 UTC**. The default desktop on the Bevy integration branch
uses **Bevy `=0.20.0-rc.2`**, application version **0.2.2**, one native host and
one shared CAD/CAM command path. The integration is tracked by
[PR #124](https://github.com/limo-cad/Limo-CAD/pull/124) and has not merged
into `main`. Passing required checks and an external approval remain merge gates.

Manual development resumed with the user's approval on **2026-10-09 UTC**.
The durable UI-first/MCP-second policy is in `.cursor/rules/surface-qualification.mdc`;
`docs/qualification/surface-coverage.json` records 443 explicit UI cases and
256 registered MCP tools. This is an active breadth survey, not complete
program qualification. Local test suites, recipe replay and direct model
mutations remain outside this manual session's scope.

The clean matched `a3cebb9c` pair now qualifies the representative rigid joint:
missing/same-component rejection, Clear/repick, preview Cancel, same-ID retained
name/direction editing, Undo/Redo and actual UI save/reopen. The human-built
`surface-ui-rigid-joint-human-ui.limo` checkpoint preserves the two definitions
and solved joint. Other joint kinds and dynamic assembly cases remain untested.
Current `aa0a41db` Windows native CI passed 937 tests with zero failures and ten
ignored; Ubuntu/macOS retain their sole known strict modeled-thread failure.
These hosted receipts compile the newest source fixes, which remain undeployed.

`369164c6` retains active native-host, interface-contract and Linux-engine CI
qualification across new checkpoint pushes. D: exhaustion temporarily truncated
one uncommitted workflow write; it was restored exactly before any commit.
Lossless compression of generated Rust caches recovered about 2.8 GB free space.
CAD documents, recovery and sessions were preserved, and exact model equality
was checked before UI operation resumed. The bounded local build blocker remains
open; storage recovery does not qualify a new executable.

The same matched `a3cebb9c` runtime qualifies ordinary solid Select, including
same-occurrence Ctrl/Shift selection, clearing and pointer-only hover with exact
model preservation. The separate line-chain fixture contains a fully constrained
25 by 10 mm triangular profile after numeric validation, chained line creation,
retained dimension editing, creation/edit Undo/Redo, Cancel and exact UI reopen.
Its functional receipt passes; drawing-tool dimension readability fails.
`6f24e148` preserves dimension ink while construction disables editing, without
changing interaction guards. This source fix remains undeployed, and typed angle
locking is still an unqualified line variant.

Three-point arc creation, endpoint dragging, draft Cancel, creation/edit
Undo/Redo and exact UI save/reopen now have functional receipts on `a3cebb9c`.
Distinct collinear picks commit no geometry, but their validation incorrectly
says that a segment has zero length. `58949424` changes only the determinant
rejection message; the geometric checks and mutation behavior stay unchanged.
The source fix awaits deployment and visible recheck. The survey continues to
center-point arcs. Hosted Windows native CI for `6f24e148` passed 937 tests with
zero failures and ten ignored; it does not qualify the repaired live UI.

The center-point arc fixture also passes safe zero-radius rejection, partial
draft cancellation, retained endpoint drag, creation/edit Undo/Redo and exact
native UI save/reopen. Its toolbar mode identity and stale error feedback fail
the separate polish check. `dc131d00` fixes the active Center arc toolbar binding;
`b200a7a1` retires only superseded construction feedback with exact document,
sketch and message provenance, preserving unrelated runtime errors. These
repairs remain undeployed.

Fit-point spline creation with the explicit Finish Spline button, partial draft
Cancel, whole-curve dragging, creation/edit Undo/Redo and exact native save/open
are retained in a separate checkpoint. The advertised Enter route instead
reopens DRAW after a canvas pick, so the spline case fails interaction despite
valid geometry. The committed canvas-focus repair clears retained toolbar focus
only on a guarded primary viewport press; deployed Enter and text-field rechecks
remain pending. The completion button also occupies visible constraint/select
ribbon space. Both issues remain recorded; individual fit-point editing is
unqualified. `0230d0bc` shares a two-slot completion reserve across Spline toolbar,
header and menu layouts; other modes keep their original allocation. The layout
repair is committed but undeployed.

The two-point rectangle fixture retains a fully constrained 30 by 20 profile after
zero-width rejection, typed width/height/Tab entry, exact draft Cancel, retained
driving-dimension edit, creation/edit Undo/Redo and exact native UI save/reopen.
The existing `6f24e148` dimension readability repair gains only these affected
rectangle cases.

The center rectangle checkpoint retains a fully constrained 20 by 12 profile
around the origin. Zero-height rejection, numeric entry, exact draft Cancel,
retained height editing, creation/edit Undo/Redo and native save/reopen pass.
Active-tool dimension ink and toolbar variant feedback remain polish failures.
`eb1e96bf` consolidates the earlier Center arc repair across the existing Line,
Arc, Rectangle, Circle and Slot primary tool families. Only Center arc and Center
rectangle were observed failing; fifteen presentation dependencies are recorded
for recheck after deployment without changing their geometry acceptance. Circles
are next in the breadth survey. All these source repairs remain undeployed.

The center-circle checkpoint passes zero-diameter rejection, positive typed
diameter, exact draft Cancel, retained 20 to 24 mm diameter editing, creation/edit
Undo/Redo and exact native save/reopen with zero DOF. Its primary Circle selection
is correct; the shared dimension-ink defect remains its polish failure. Two-point
circle remains next. Linux engine CI for `eb1e96bf` passed all 43 nonempty suite
summaries with no failed summaries; live presentation is still unqualified.

Two-point circle also passes zero validation, diameter creation, exact draft
Cancel, retained 20 to 16 mm diameter edit, creation/edit Undo/Redo and exact native
save/reopen. Its center remains 10,0 with two positional DOF; no fully constrained
claim is made. This mode actually reproduces both existing dimension-ink and
active-family toolbar defects, extending observed coverage without opening
duplicate repairs. Slots are next in the UI breadth survey.

Center-to-center slot passes axis/width creation, zero-width validation, exact
draft Cancel, retained 10 to 12 mm width edit, creation/edit Undo/Redo and exact
native save/reopen. Eight entities and eleven tangent/equality/endpoint/diameter
constraints remain stable. Its four DOF leave length and placement free; the
second center moves about 0.00009 mm during the width solve, so no locked-length
claim is made. The shared dimension-ink defect remains; Slot's primary control
is clipped at this ribbon width and its active caption is unqualified. Overall
slot and Center-point slot remain next.

Overall slot now passes outside-endpoint creation, safe too-wide rejection and
recovery, exact draft Cancel, retained width edit, creation/edit Undo/Redo and
exact native save/reopen. Width 30 on outside length 20 wrongly says "segment has
zero length"; `15f6f407` changes only that Overall rejection branch to actionable
width/length guidance. Both reviewers inspected pre-mutation rejection and error
lifecycle compatibility. The fix is committed/pushed, with no local suite/build
or deployed message proof. Shared dimension ink remains a second polish defect.
Center-point slot remains next.

Windows native CI for `230e9dc2` passed 937 tests, zero failures and ten ignored,
including exact thread and both visibility/history contracts. This compiles the
earlier feedback/focus/layout/variant repairs; live presentation remains pending.
Ubuntu has only the retained strict thread failure and no poison/new failure;
macOS subsequently completed 942 passes, the same sole known thread failure and
ten ignored at 16:04:34 UTC, with no poison/other failures. `15f6f407` Linux and
repository/WASM checks passed, but its own native qualification remains pending.

Center-point slot completes the three slot modes' functional receipts: symmetric
center/end creation, safe zero-width rejection, exact draft Cancel, retained width
edit, creation/edit Undo/Redo and whole-model native save/reopen. The four DOF
remain free; symmetry is the initial placement rather than a new retained midpoint
constraint. Shared dimension ink remains its polish defect. Polygons are next.

Multiple distinct saved/reopened scratch documents still display `Untitled` in
tabs and Browser despite exact archive/live equality. `2411613a` adds a shared
display-only saved filename fallback for that default name, including accessible
close captions and confirmation headings. Explicit nondefault names, raw model
names, Rename, archive/recovery, dirty state and geometry remain unchanged. Root
and peer reviewed the source; deployed title and dependent presentation checks
remain pending under the existing bounded build blocker.

Inscribed Polygon passes side-count rejection, regular six-vertex creation,
exact form Cancel, retained vertex drag, creation/edit Undo/Redo and whole-model
native save/reopen. Its twelve DOF and zero constraints allow subsequent shape
changes; no retained regularity claim is made. Zero radius safely rejects but
displays the opaque `polygon: notpositive`. `3f745d96` translates only that shared
polygon error to positive-radius guidance before any mutation; root and peer
reviewed it, with deployed validation still pending. Circumscribed mode is next.

`45ce0569` Windows native CI completed 937 passes, zero failures and ten ignored
at 16:17:46 UTC; macOS has 942 passes and only the known strict thread failure.
`8660ca5e` passed all six automatic Ubuntu/Windows core/vise/turbine MCP acceptance
jobs and their aggregator; the final Ubuntu MCP-tests job remains pending at this
receipt; that final job subsequently completed successfully. These checks do not
complete the manual MCP surface survey.

Circumscribed Polygon also passes side-count rejection, radius/rotation geometry,
exact form Cancel, retained vertex editing, creation/edit Undo/Redo and whole-model
native save/reopen. Four sides with radius 10 and rotation 45 produce a 20 by 20
square whose edges are 10 mm from the center, with eight DOF/zero constraints.
Passing geometry is retained; only the shared radius-message dependency requires
a deployed recheck. Midpoint line and Point are next in the breadth survey.

Midpoint line passes coincident rejection/recovery, symmetric midpoint/horizontal/
origin creation, exact draft Cancel, larger retained endpoint resize, driving
length dimension and whole-model native save/reopen. Its free line has one DOF;
the independently exercised 24 mm dimension fully constrains it. A small unchanged
drag leaves an extra Undo/Redo step, confirmed before creation finally undoes.
`27210873` skips only an idle Single-phase exact unchanged snapped target in an
already-consistent sketch, preserving ongoing gestures and solve/recovery paths.
Root and peer reviewed the source; deployed history qualification is pending.
The existing active-variant toolbar defect is also observed. Point is next.

Point passes pointer preview/creation, exact preview Cancel, cyan retained hover,
free dragging, creation/edit Undo/Redo and whole-model native save/reopen on the
matched `a3cebb9c` runtime. Its two positional DOF remain free. Numeric validation
is not applicable to this pointer-only command. No new Point failure was observed;
only the shared `27210873` idle unchanged-drag history dependency needs a deployed
recheck. Sketch editing tools are next in the breadth survey.

`a579dd31` automatic repository/WASM checks passed; native/MCP/package and Linux
qualification remain pending at this receipt. These hosted checks do not qualify
the undeployed source fixes or complete manual MCP coverage.

Sketch Move/Copy passes selection gating/removal, safe invalid-expression rejection,
exact Cancel, signed Move and Create copy, independent operation Undo/Redo and
whole-model native save/reopen. The two free lines retain eight DOF. Tab from X
distance incorrectly focuses Browser Origin before Y; `148d9f0e` redirects only
eligible adjacent fields in the same current owned form after ordinary text flush.
Boundaries, buttons and offscreen targets keep existing traversal. Root and peer
reviewed the committed/pushed source; deployed focus verification remains pending.
Scale, Offset and Trim follow in catalog order.

`858cc0f8` repository/WASM and Linux engine checks passed. Earlier `45ce0569`
Windows native CI passed 937/0/10; Ubuntu/macOS retain only the known strict modeled
thread failure, with both history contracts passing and no poison/other failures.
Latest native and source-fix qualification remain pending.

Sketch Scale passes selection gating, zero-factor rejection, exact Cancel, positive
scaling about an entered nonzero origin, negative scaling about zero, operation
Undo/Redo and whole-model native save/reopen. Stable point/line IDs retain four DOF.
No new Scale failure was observed; only its shared multi-field Tab dependency needs
a deployed recheck. Offset is next, followed by Trim and Extend.

`a579dd31` Windows native CI passed 937/0/10 at 17:19:22 UTC, including exact thread
and both history contracts, with zero poison. Ubuntu/macOS retain only the known
strict modeled-thread failure; both history contracts pass. `148d9f0e` Linux engine
checks passed; newer native qualification remains pending.

Circular Offset passes selection/side gating, safe zero/collapse rejection, exact
Cancel, outward/inward geometry, retained gap editing, three-step creation/edit
Undo/Redo and whole-model native save/reopen. The fixture retains concentric radii
10/14/8 and two DOF. Collapse feedback is opaque; `b99dfa8e` translates only that
error. Adjacent offset labels concatenate; `bb2324bc` preserves a clear +X default
or chooses a separated cardinal anchor for the new radial label, reading saved
anchors without moving existing/manual labels. The 4 mm heuristic is not an
all-font/zoom collision guarantee. Both repairs were root/peer reviewed and
committed/pushed; deployed proof remains pending. Shared active-form dimension
ink also fails and uses existing `6f24e148`. Trim and Extend are next.

`8471bfb1` Windows native CI passed 937/0/10 at 17:34:02 UTC; Unix hosts retain only
the known strict thread failure, with both history contracts passing and zero
poison/other failures. `1e2d6ed0` repository/WASM and Linux checks passed; newer
Offset source checks remain pending. `3f745d96` full automatic MCP matrix passed
all six core/vise/turbine acceptance jobs plus final Ubuntu tests; manual MCP
qualification remains gated on the unfinished UI surface.

Trim passes the representative line-intersection removal workflow, preview/Cancel,
empty-pick validation, subsequent endpoint editing, exact two-step Undo/Redo and
whole-model native save/reopen. No coincident-to-boundary constraint was added;
free horizontal editing can move its y. Extend is next. A lingering modeling
footer after reopen is tracked independently: `3b079dc0` prevents background
memory retention from introducing a caption its success path preserves. Exact
responsibility for the observed footer remains unproven; deployed acceptance
is pending. Only affected Open/status cases were reopened.

Linux CI caught an old Offset wording assertion on `bb2324bc` and `e3e80ed8`.
`79b0cb00` preserves rejection and geometry checks, requires actionable radius
guidance and verifies exact DTO preservation; corrected hosted CI is pending.
`e3e80ed8` repository/Rust/WASM passed. `1e2d6ed0` native Windows passed937/0/10;
Unix hosts retain only the known strict thread failure, both history contracts
passing and zero poison failures. `858cc0f8` packages passed x64 and all
Linux/macOS jobs; ARM controlled-source-host input was denied foreground by
Windows and sent no input. Default shipping artifact OS input remains
unqualified. No local suites/builds were run for these repairs.

Extend passes a representative finite-line boundary workflow: hover/Cancel and
empty-pick preservation, stable endpoint extension with retained point-on-line
constraint, subsequent endpoint drag, exact two-step Undo/Redo and whole-model
native save/reopen. Three constraints/five DOF remain. Shared background footer
was visible after saving; it remains separately queued for `3b079dc0` deployed
acceptance. Break is next.

Corrected `79b0cb00` Linux CI passed: all29 modification tests and the complete
host-neutral workspace passed; the Offset rejection contract is verified.
`e3e80ed8` Windows native passed937/0/10; both Unix hosts retain only the known
strict thread failure and both history contracts pass without poisoning.
These hosted receipts do not qualify undeployed native UI fixes.

Break passes a representative interior-line split, readable menu/hover/feedback,
Cancel/empty-pick preservation, shared-point edit, exact two-step Undo/Redo and
whole-model native save/reopen. Original outer endpoints remain exact; the two
line pieces share an editable point, two horizontal constraints/four DOF.
Fillet and Chamfer are next; broader arc/circle variants are unqualified.

Sketch Fillet creates the representative radius5 tangent corner correctly,
with exact zero/Cancel preservation, creation Undo/Redo and whole-model native
save/reopen. The case fails retained R5-to7 and R5-to3 edits: solver rejects
both while preserving the whole sketch and history. `aa7d0000` extends the
existing consumed-fillet initial guess only to proven finite shared-corner
topology; equations, rank reporting, convergence, consumption guards and atomic
rollback remain unchanged. `e1928904` separately explains nonpositive radius.
Both repairs were reviewed/formatted/committed/pushed; hosted and deployed
acceptance remain pending. Reported DOF6 is the local first-order rank at
singular trim tangency, not proof of two extra finite motions. Chamfer is next.

`3b079dc0` Windows native CI passed937/0/10; Unix hosts retain only the known
strict thread failure, both history contracts passing and zero poisoning.
`cf0e97c3` repository/Rust/WASM and Linux checks passed. These hosted receipts
do not qualify the new Fillet fixes or repaired native captions.

Sketch Chamfer passes zero/Cancel preservation, equal5 creation, exact creation/
edit Undo/Redo and native persistence, but fails accessible caption-center
editing and stable free-carrier edits. `27a244e7` places only new captions6mm
outside the corner; existing/manual placements and picking remain unchanged.
A5-to7 edit satisfies equal7 cutbacks but stretches far ends to almost4m and
y230.8mm. The normal5 baseline is checkpointed for reproduction; bounded
`71fedf1d` adds conservative isolated retained-edit seeding and temporary
corner/far-end/direction preferences, keeping wider graphs and persistent
constraints on the generic path. No repaired UI acceptance or all-font/zoom
spacing guarantee is claimed.

Sketch Mirror passes source/axis order, missing-axis/Cancel exact preservation,
reflection, independent endpoint editing, exact two-step Undo/Redo and native
save/reopen. One diagonal and vertical axis are qualified; curve/multi-entity/
oblique variants remain untested. Rectangular Pattern is next.

`5a9b982a` full repository/Rust/WASM checks passed. `ff4658c3` native Windows
passed937/0/10; Ubuntu937/1/10 and macOS942/1/10 retain only the known strict
thread failure with both history contracts passing and no poisoning. New
`71fedf1d` repository-contract passed; full latest CI is pending.

`ff4658c3` full repository/Rust/WASM checks passed. `e1928904` native Windows
passed937/0/10; Ubuntu937/1/10 and macOS942/1/10 retain only the known strict
thread failure, with both history contracts passing and zero poisoning. Full
automatic MCP workflow `8471bfb1` passed all six host/model jobs plus final
Ubuntu and aggregate checks; the manual256-tool catalog remains untested and
gated on the incomplete UI survey.

Sketch Rectangular Pattern passes zero-count safe feedback, Cancel exact
preservation, positive3x2 spacing20 geometry, independent copied-endpoint edit,
exact Undo/Redo and native save/reopen. Source horizontal constraint remains;
copies are independent/free. Keyboard traversal stays needs_recheck for
undeployed `148d9f0e`; no array preview/oblique/negative/curve claim is made.
Circular Pattern is next. `8c7484cc` full repository/Rust/WASM passed.

Sketch Circular Pattern passes zero-count/Cancel preservation, full360/count4
and clockwise partial180/count3 circle placements about the origin. Independent
copied-center editing preserves radius and other circles; exact history and
native persistence pass. Tab traversal stays needs_recheck for `148d9f0e`;
shifted centers/other primitive variants remain unqualified.

`71fedf1d` native Windows passed937/0/10; Ubuntu937/1/10 and macOS942/1/10
retain only known strict thread failure, with both history contracts passing
and no poisoning. `8d59076a` full repository/Rust/WASM and Linux workspace
checks passed. These do not qualify the undeployed Chamfer repair.

Sketch Coincident passes point-point selection/Cancel, shared drag, retained
inspector/delete, exact history and whole-model native persistence. Existing
point-only re-edit loses visible geometry; degenerate Fit magnifies point grips.
Source repair `d766df9c` is committed, reviewed and not yet deployed. A later
fresh pointer move highlights correctly; no hover defect is attributed.

Sketch Midpoint passes point-line placement, constrained translation, Cancel,
readable inspector/delete/history and whole-model persistence. Saved re-edit
hides the right line endpoint behind the palette; mixed-sketch framing is
being investigated separately from the relation. `9b70a83f` repository/WASM
and Linux workspace passed; native Windows937/0/10 and Unix only known thread
failure. Hosted `aa7d0000` automatic MCP passed; manual MCP stays not-run.

Sketch Collinear passes wrong-type guidance/Cancel, aligned free-line creation,
readable inspector, exact relation history and whole-model save/reopen. Two
endpoint drags silently leave geometry unchanged and consume Undo entries.
`983dbdb9` repairs failed discrete-drag feedback/history only; the Collinear
solve remains failed and queued. `ea169f81` extends Begin framing to authored
sketch bounds outside the palette; source acceptance awaits a matched runtime.
`b0734d79` makes cache-lock test release explicit after intermittent hosted
reacquire failure; production lock behavior is unchanged.

Sketch Horizontal/Vertical passes both closest-axis line choices, Cancel,
duplicate guidance, readable inspectors, independent constrained endpoint edits,
exact four-step history and whole-model native persistence/re-edit. Only the
shared undeployed Begin framing dependency requires a camera recheck.
`9be977e6` repository/WASM and full Linux passed, including both changed failed
discrete-drag contracts. `b0734d79` cache release contract passed hosted Linux.
`9b70a83f` complete automatic MCP passed; manual MCP remains not-run.

User confirmed finite additive Rib policy. Issue[#360](https://github.com/limo-cad/Limo-CAD/issues/360)
is assigned to Jack. Existing UI guard was already qualified; `0166a9cc` now
applies that policy to shared native/MCP add/edit before ID allocation.
Existing archives and Subtract/Common replay remain unchanged; the request
guard passed hosted Linux workspace and repository checks. Full WASM was
cancelled; isolated manual requests and the changed native form wiring remain
pending acceptance on a newly deployed matched runtime.

The clean `b71665d7` GUI/MCP pair verifies all 70 payload hashes. Extrude ToFace
now respects its selected signed stop plane despite retained Flip metadata. Preview,
same-plane validation, Cancel, Apply, Undo/Redo and actual UI save/reopen pass;
all four saved feature statuses are OK. The separate Common final-status defect
remains queued. Revolve's four axis modes and signed half/full turns produce
expected geometry. The clean `5b10cdc3` pair additionally qualifies Add/Subtract/
Common targets, retained Cancel/Undo/Redo and exact save/UI-reopen with four OK
features. Its narrow fix preserves new Revolve drafts across browser visibility
changes and removes duplicated inline validation; both have actual UI receipts.
Two human-built Revolve checkpoints retain the axis and boolean fixtures.

The clean `8fdd5fd7` GUI/MCP pair verified all 70 payload hashes. Settings scrolling
and translated document units pass the exercised normal-window flows. A full
71-character Unicode path saved and reopened a model matching the live state;
a later new-document Save As stopped partway through another path before submission.
That native text case remains open. Ctrl+Shift+S also remains unattributed:
the menu opens Save As but the guarded shortcut did not.

The small human-built fixtures under `examples/checkpoints/surface-ui-*` retain
the block, origin-plane sketches and a Common result whose supporting face is
valid before its consuming extrusion. Editing that extrusion again falsely
rejected its sketch support in the completed scene. `d9719134` uses the existing
isolated pre-feature editor. The broad accompanying status exemption violated
an existing broken-reference CI contract and was fully reverted at `0a8915e8`.
That final clean GUI/MCP pair passes the reported edit, exact Cancel preservation,
Undo/Redo with hidden Sketch2, and save/UI-reopen equality. The resulting
two-body model is retained as `surface-ui-retained-extrusion-fixed-human-ui.limo`.
The Linux contract and Ubuntu/macOS retained-edit checks pass. False error status
on the valid Common result remains separately queued; genuine missing support
continues to be reported.

The next meaningful mode exposed a separate To Face direction error: top Z=8
to selected bottom Z=0 with retained Flip produced Z=8..16 while reporting no
errors. `b41645b1` preserves the selected plane's signed endpoint in geometry
and preview and hides Flip only for that mode; runtime qualification passes on
`b71665d7`, including same-plane validation, editing and exact save/UI-reopen.
The saved human fixture is `surface-ui-to-face-flip-human-ui.limo`. Analogous
Rib logic remains unqualified and will be checked at its normal catalog position.

Hosted native CI on `0bb6257b` confirms the test-only isolation repair: Windows
937 passed/zero failed; Ubuntu 937 passed/one failed; macOS 942 passed/one failed.
All three pass the Browser visibility Undo/Redo regressions with no poisoned-lock
cascade. The exact thread export passes Windows but remains the sole native
failure on Ubuntu/macOS. It is queued after the bounded investigation; source
precision and native identity guards remain unchanged. Other current CI jobs
are not qualified by these native-host receipts.

The earlier stopping receipt is preserved below as historical evidence.
Manual development had stopped at the user's request on **2026-10-09 UTC**.
The final code revision is `289aef988d9c33c1805ea2be5f1de187737b9089`;
the stopping checkpoint is documentation only. Both working checkouts and remote
integration branches were clean and synchronized before recording this receipt.
The installed clean runtime's 70 payload hashes were verified, and its fresh
GUI/MCP pair matched. The pending repeat annulus export dialog was cancelled
without changing the saved model. No additional UI walkthrough, builds or
local suites are authorized by this stopping receipt.

Automatic CI on that code was still in progress at the stopping check: 17 jobs
passed, eight were running and six were skipped, with no reported failures yet.
This is a point-in-time observation, not final CI success. The last completed
native-host run rejected the exact thread case; the follow-up fix remains
pending hosted qualification. Actual human inspection was detailed for selected
bench, Medix and annulus operations, saved-model equality and export integrity.
It was not a systematic application-wide usability or polish audit, and the
full bench/vise/turbine human walkthrough remains unfinished. Retained artifact
identities and limitations are recorded in
`docs/qualification/manual-ui-stop-20261009.json`.

October 9 UTC thread investigation: the actual human UI built and saved the
annular stock (20/10 mm diameters, 20 mm extrusion) and a modeled right-hand
M20 x 2.5 external thread of length 8 mm at the opposite end. The saved archive
matches the live model exactly and is retained at
`examples/checkpoints/threaded-annulus-human-ui.limo`. An incomplete native
Save As filename produced `D:/li.limo`; that original is preserved, and its
byte-identical archive was copied to the named UI-model path and checkpoint.
This is construction/save evidence, not successful thread-export qualification.

Hosted CI identified two distinct native boundary stations merged into one
planar UV node. The bounded repair candidate separates retained native stations
on copied meshes and certifies every affected owner's complete trim, source
precision and incidence before acceptance. It preserves native identity and
the final oriented-closure guard. Native dialog text now pins the observed
writable edit and checks ownership between paced Unicode scalars and after
insertion. Foreground activation has bounded stability sampling; fresh
observation and visible filename verification remain necessary. The native
layout rejection reports a bounded first difference without relaxing its guard.
These changes require a fresh native build and manual UI qualification.

The clean `8e0a0bda` deployment and fresh matched GUI/MCP pair passed the actual
UI-built annulus 3MF and STL exports. The 3MF contains 863 vertices and 1,726
triangles with zero invalid directed edges or zero-area facets; its millimetre
bounds are (-10, -10, approximately 0) to (10, 10, 20). The ASCII STL has 1,726
finite, nondegenerate facets and exactly matches the ordered 3MF coordinates.
The UI completed both export filenames and a full-path native Save As; fresh
captures preceded Enter, and the saved model again matched the live model.
The original accidental-path archive remains preserved. This receipt does not
establish universal foreground stability or prove that the human sketch's
machine-rounded inner radius exercises CI's exact native topology.

Hosted Ubuntu CI on that source still correctly rejected a pre-existing curved
owner facet for source angular precision after the boundary separation, rolling
back the full transaction. The follow-up candidate tries independently restored,
bounded source-chart refinements of that owner at the unchanged requested
precision, then requires the complete all-owner certificates again. The exact
hosted case remains unqualified until CI passes. No local suites were run.

The MCP recipe export assertion now accepts complete, finite ASCII STL as well
as binary STL; precision-preserving ASCII output exposed its binary-only
assumption in hosted CI. No local suites or recipe replay were run for this
manual session. The complete three-model human walkthrough remains unfinished.

The public [Bevy preview](https://github.com/limo-cad/Limo-CAD/releases/tag/bevy-preview-0.2.2-20261004.1)
contains **Windows x64 ZIP and Ubuntu 26.04 x64 DEB from `9b082687`**.
Windows passed SDK-free headless/desktop MCP
checks on Thunder; Ubuntu passed its hosted MCP, X11 and Wayland-desktop checks.
The independently built hosted Windows x64 package also passed native-input
checks on this clean source. The AppImage build and Ubuntu 26.04 qualification
passed in the [tagged package run](https://github.com/limo-cad/Limo-CAD/actions/runs/37232261112),
but that artifact has not been added to the public preview. Windows ARM64 failed;
macOS built and signed but remains blocked by Apple's team-agreement HTTP 403.
The superseded October 2 preview release was removed; its source tag remains.
Application version alone does not identify which source was built. Published
packages retain the former noBS-CAD name while the repository and public project
name are Limo CAD.

Thunder uses one canonical **Limo CAD** executable at
`%LOCALAPPDATA%/limo-cad/bevy/Limo-CAD.exe` for the GUI and `--headless` MCP.
Managed local commands verify the source checkout, SDK, feature mode and installed
payload before reuse. The adjacent `runtime-manifest.json` records the deployed
revision and SHA-256; inspect `build_pair.status` and require `matched` before
qualifying a live GUI/MCP pair. Application version alone is insufficient.

Current October 8 Medix qualification: clean matched `51cb7479` opens the complete
saved import with zero scene errors and zero display warnings. Its live model
matches the saved archive: one compound body, 8,908 face identities and dimensions
536 × 327.47 × 48 mm. Actual GUI 3MF and STL exports now pass the native
source-precision and oriented-closure checks across 33 native shells. Restoring
the original split policy recovers the 37 repaired faces lost by the earlier
lookahead regression to 72 links. Connector caps and
independent pole proposals now qualify. A bounded fallback to the earlier
split strategy retains its primary trajectory and defers alternate trials.
FIFO processing of the alternate queue repairs one further face, reducing 49
links to 45; a last longest-interior-edge strategy repairs seven more faces and
reduces the count to 16 on `0528a7db`. The next trial reaches shared-curve owner
qualification but rolls it back for source-angle failures; inserting flips into
the longest strategy regressed the result to 24. Restoring the exact no-flip
trajectory recovers 16 links. The bounded constrained-ear alternative also leaves
16 links. Source inspection identifies two thin, three-edge strips whose native
rails use different U stations. Their measured source widths exceed the retained
native endpoint offsets. The station synchronization trial initially preserves
16 links because two endpoint checks mixed exact source evaluation with retained
UV coordinates. Using retained station coordinates for range checks and pairing
the shared apex after native vertex/world identity, coordinate-roundoff and
source-precision certificates qualifies both rail repairs and all affected
owners: two accepted transactions insert 22 native stations, reducing 16 links
to 8. A complete constrained native-boundary seed then repairs the two remaining
six-edge rational B-spline patches, without artificial star spokes. Every native
station and final source/pole/domain/closure check remains.
All 70 installed payload hashes match the clean manifest. The actual portable
3MF has 227,034 vertices, 454,572 triangles, zero zero-area triangles and zero
invalid directed-edge links. The actual ASCII STL has the same 454,572 facets,
zero nonfinite or degenerate facets and zero opposing normals. Every ordered
vertex coordinate matches exactly between the exported formats. The original
STEP hash and live/saved CAD model are unchanged. See the
[manual export evidence](qualification/medix-kw22-native-human-ui-20261008.json).
Three default-bed layout issues remain because the preserved 536 mm import is
below and larger than that bed; portable export deliberately retains its original
placement. This does not qualify physical printing. These results qualify the
local Windows GUI path; the complete three-model walkthrough remains unfinished.
The Medix notes below record earlier trials and do not supersede this result.

A fresh blank-window import of the original Downloads STEP on clean matched
`d8b29c3f` independently reproduces that result. Its saved model exactly matches
the prior import, its native export check passes, and its actual GUI 3MF is
byte-identical to the qualified package above. Read-only inspection again finds
zero invalid directed-edge links and exact agreement with the qualified STL.
The managed GUI and operator now both use the supported `LIMO_CAD_SESSION_DIR`
and `LIMO_CAD_PRINT_CACHE` overrides on D: to reduce pressure on C:. The original
session, document, settings and recovery files remain preserved.

Actual guarded bench clicks on clean matched `d515b39a` qualify the footer-race
fix. Appearance Apply and Make component had both refused unchanged Body8 with
**Native interface transition is pending**. Queued dispatch now permits only
`document/status` text changes while retaining the original control stamp,
owner/revision, layout, visibility and modal fences; fresh pointer/MCP observation
remains strict. Both retries passed. The saved
[center-bearer checkpoint](../examples/checkpoints/garden-bench-center-bearer-human-ui.limo)
has its independent timber-colored 28 × 415 × 90 mm bearer named and placed at
586/30/325 mm with identity rotation. Exact live/archive comparison passed with
twenty-nine features, eight definitions, eleven occurrences, five named views,
zero scene errors and zero display warnings. Support blocks and the rest of the
three-model walkthrough remain unfinished. The newly observed threaded-part
export identity conflict remains under investigation in automatic native CI.

The subsequent [front support stock checkpoint](../examples/checkpoints/garden-bench-front-support-stock-human-ui.limo)
adds its origin-constrained 65 × 28 mm sketch and independent 65 mm extrusion
through the same guarded UI. Both features are named, all nine bodies restored,
and the finished sketch hidden. Exact live/archive comparison passed at
thirty-one features, nine definitions and twelve occurrences. This block's
appearance and placement remain unfinished.

The subsequent clean matched `35722a50` GUI export still reports 86 unmatched
links. Three continuous source intersections are now qualified, but the thin
three-edge strips are refused as nonsimple or unoriented. The next candidate
replaces the scale-dependent UV segment classifier with conservative certified
separation; ambiguous contacts and every existing source/domain/closure guard
still reject. It is not yet qualified through the GUI.

The subsequent clean matched `8ee3a909` GUI export gets past that simplicity
check but still reports 86 unmatched links. Its leading thin spheres now stop
after meshing at **connector cap lacks a positive source fan**. A bounded
constrained cap triangulation now preserves every connector and tail station,
checks positive disjoint ears and seven source precision witnesses, and bounds
backtracking to 512 states. It awaits GUI qualification; no successful
native-precision export is claimed.

Actual guarded GUI export on clean matched `0226ec9f` qualifies those connector
caps and reduces invalid boundary links from 86 to 53 across the same 33 native
shells, with no multiply used links. Remaining groups include a source-angle
refinement failure on face 2449, curved trim crossings on 8033/8014, and two-pole
faces 7236/7146. A bounded two-pole export extension now certifies each independent
native vertex and original wedge against the final composed quotient, with at
most four representative choices; it awaits GUI qualification. Export is still
refused, and the saved Medix model remains unchanged.

On clean matched `cb464867`, actual GUI export reduces invalid links from 53 to
49, still with no multiply used links across 33 native shells. The independent
pole proposals now reach refinement; face 7236 fails its source-angle check
instead of the earlier one-pole eligibility gate. Medix reopens with zero scene
errors and zero display warnings and still matches its saved archive exactly.
The next bounded refinement proposal evaluates all four children of both owners
at seven source witnesses before selecting an interior split, and each child
inherits its own parent's depth. Original precision, work/depth/node limits,
native boundaries and final closure checks remain unchanged. GUI qualification
is pending; native-precision export remains refused.

The actual clean matched `64472748` lookahead trial regresses from 49 to 72
invalid links, with 35 repaired faces instead of the previous 37. The next
candidate restores the exact original metric and max-owner depth policy before
any alternate is attempted. Deferred lookahead begins from fully restored mesh,
wire/face status and boundary-index snapshots within the same cumulative caps.
Bounded stderr diagnostics include failed triangle coordinates and source
normals, plus exact trimmed source-PCurve intersection status/counts and quarter
chord errors; they change no precision or topology acceptance checks.

Actual guarded GUI export on clean matched `2cccc3d8` restores 49 invalid links,
37 repaired faces and 41 added triangles, with 99 attempts and no multiply used
links. Its saved model still matches exactly and read-only inspection reports
zero scene errors and zero display warnings. The failing face 2449 alternate
repeatedly shrinks a tiny unconstrained interior edge while leaving two long
interior edges coarse. FIFO processing of that alternate queue is the next
bounded candidate; the primary LIFO trajectory remains unchanged.

Exact interval diagnostics on faces 8033/8014 report a completed native trimmed
PCurve intersection calculation with zero points and overlaps for the two
reported crossing chord pairs. This establishes separation only for those
specific source intervals. Their sampled chords cross despite native curve
separation; a shared native edge sampling repair is being designed independently.
No successful native-precision STL or 3MF is claimed.

On clean matched `fe195308`, guarded GUI export reports 45 invalid links across
33 native shells, 98 attempts, 38 repaired faces and 71 added triangles, with no
multiply used links. Read-only inspection still reports zero scene errors and
zero display warnings, and the saved archive matches exactly. The next candidate
adds source-certified native midpoint samples to crossing chords on all shared
owners in one export-only transaction, then tries FIFO longest-interior-edge
refinement from fresh snapshots. All source accuracy, domain, work/depth limits,
native ownership and final closure checks remain mandatory; GUI qualification
is pending.

Actual guarded GUI export on clean matched `0528a7db` reports 16 invalid links
across 33 native shells, with no multiply used links. Its longest-edge fallback
repairs seven faces in 18 attempts and adds 491 triangles, including resolving
the previously leading face 2449. The shared-curve sampler makes zero mutation
attempts because an owner eligibility gate rejects both crossing faces. The
remaining spherical refinement cells are curved UV slivers: consistent source
normals but nearly reversed straight facet normals. The next candidate preserves
a unique whole-period source chart branch for periodic owners and permits at
most 32 conforming flips of unconstrained interior diagonals, only when both new
facets qualify outright at all seven source accuracy witnesses. Bounds, native
constraints, final domain and closure checks remain unchanged. Read-only scene
inspection remains at zero errors and zero display warnings; the saved archive
matches exactly. Export is still refused; no successful STL or 3MF is claimed.

Actual guarded GUI export on clean matched `31c9a6ba` regresses from 16 to 24
invalid links. Both periodic sampler transactions reach owner meshing but fail
source-angle qualification and restore their pre-insertion baselines; zero
sampler transactions are accepted. Interleaved flips change the previous
successful longest-edge trajectory: five repaired faces instead of seven.
The next candidate restores the exact no-flip strategy before all later work,
then uses independent post-insertion owner snapshots for sampler alternatives
and separately deferred flip trials. The original 16-link qualification remains
the best result. Scene inspection remains at zero errors and zero display
warnings and the live model still matches the saved archive exactly.

Actual guarded GUI export on clean matched `e7e0a775` recovers the exact 16-link
baseline: original repairs 98/38/71 and longest repairs 18/7/491. Two sampler
transactions and two deferred flip attempts are rejected; neither changes the
saved model or loosens final accuracy/closure checks. Both sampler targets now
fail on nearly collinear same-rail triangles. The next candidate tries a bounded
complete constrained-ear triangulation, preserving every native station and
qualifying every triangle at all seven source witnesses before installation.
Bounded native rail diagnostics measure actual rail separation and source/native
normal offsets; they do not certify or alter geometry. Read-only inspection
remains at zero scene errors and zero display warnings; no accepted STL or 3MF
is claimed.

The actual guarded GUI export on clean matched `7c235990` still reports 86
unmatched links. Native curve diagnostics rule out zero-area incidence loss on
the leading sphere faces: all ten existing facets have positive area, but native
trim samples are missing from their mesh. Their crossing chords correspond to
nearly coincident source curves, with approximately 1.407e-7 mm native separation
and recorded tolerances of approximately 0.0483 mm. This observation alone does
not prove continuous curves are disjoint. `96b567a7` adds export-only continuous
intersection qualification and f64 strip witnesses. Its actual guarded GUI
qualification at `d2ba9017` exposed the source-normal failure described above;
the initial three proposed strip repairs were all rejected. Matching strip
boundary stations and facet connectivity are being investigated while retaining
the seven source witnesses. Export is still blocked; no mesh guard is waived.

The clean matched `7c235990` bench session now has a separate preserved
[lower-clearance checkpoint](../examples/checkpoints/garden-bench-lower-clearances-human-ui.limo)
with nineteen features. Four lower-rail bores were created through the physical
Hole dialog at U/V `(32.5, 40)`, `(32.5, 75)`, `(382.5, 40)`, `(382.5, 75)`, using
5.5 mm diameter, 28 mm distance, simple style, flat bottoms and no flip. The
feature is named; physical Undo/Redo and Ctrl+S restored the same feature and
positions, and the saved model matches the live document. The original bench
checkpoint remains unchanged. The lower rail remains unplaced source stock,
with no appearance or separate component definition yet; upper and lower rail
definitions must remain separate. This does not complete the bench walkthrough.

On clean matched `d2ba9017`, physical File/Open reopened the saved four-bore
bench. Its lower stock now has the upper rail's same deep-green timber appearance
and a separate named **Lower side rail** definition, with one named **Left lower
side rail** instance at the source origin. Ctrl+S and read-only archive comparison
qualified the saved [lower-component checkpoint](../examples/checkpoints/garden-bench-lower-component-human-ui.limo):
seven definitions and nine occurrences, with existing upper/apron shared
definitions and placements preserved. Lower-rail placements, its shared right
instance and the remaining bench geometry are unfinished.

On clean matched `19ca11c7`, the physical Assembly UI placed the left lower rail
at `(37, 30, 130)` mm, duplicated it, and named/placed the right lower rail at
`(1135, 30, 130)` mm. Both rotations are zero. Ctrl+S and read-only comparison
qualified the [lower-placement checkpoint](../examples/checkpoints/garden-bench-lower-placements-human-ui.limo):
nineteen features, seven definitions and ten occurrences. The two lower rails
share definition 14 and its four clearance bores, separately from upper-rail
definition 12. Remaining pilots, geometry, joints and drawing/layout work are
unfinished.

On clean matched `e7e0a775`, physical Hole input added six Ø3.5 mm, 39 mm deep,
flat-bottom left-rear rail/arm pilots at stock X 0 mm, Y 32.5 mm and Z
365/400/170/205/590/615 mm. The named
[left-rear pilot checkpoint](../examples/checkpoints/garden-bench-left-rear-pilots-human-ui.limo)
has twenty-six features, seven definitions and ten occurrences with all Browser
eyes restored. Saved parameters and exact live/archive comparison passed.
On clean matched `7a51d2e0`, physical Hole input completed the right-rear rail/arm
pattern on the outward +X face: stock X 65 mm, Y 32.5 mm and the same six Z
positions, with Ø3.5 mm, 39 mm deep, flat-bottom pilots. The named
[post-pilot checkpoint](../examples/checkpoints/garden-bench-post-pilots-human-ui.limo)
has twenty-seven features, seven definitions, ten occurrences and five named
views. All Browser eyes are restored; saved parameters and exact live/archive
comparison passed. The four post pilot patterns are complete. Remaining bench
geometry, joints, drawings and print layouts are unfinished.

On clean matched `fc275793`, physical Rectangle input added the center seat
bearer's 28 × 415 mm XY profile. Its saved constraints retain an origin-coincident
point, horizontal/vertical edges and driving dimensions. Physical Extrude input
created independent 90 mm stock with zero taper and no flip. Both features were
named through the UI; all eight Browser eyes were restored. The saved
[center-stock checkpoint](../examples/checkpoints/garden-bench-center-stock-human-ui.limo)
has twenty-nine features, eight definitions, eleven occurrences and five views,
and matches the live model exactly. The center stock remains at the source
origin; its appearance, assembly placement and support blocks are unfinished.

On clean matched `08efc3e7`, physical top-face selection and the Hole dialog
added the lower rail's two Ø3.5 mm, 32 mm deep, flat-bottom stretcher pilots at
stock points `(14, 195, 90)` and `(14, 235, 90)` mm. The named feature and Ctrl+S
produced the [lower-pilot checkpoint](../examples/checkpoints/garden-bench-lower-pilots-human-ui.limo):
twenty features, seven definitions and ten occurrences, with both lower rails
sharing the four side clearances and two top pilots. Read-only inspection of
the saved parameters and exact live/archive comparison passed. Upper-rail top
pilots and remaining post pilots, geometry, joints and drawings are unfinished.

On clean matched `35722a50`, the physical Hole dialog added the upper rail's five
Ø3.5 mm, 32 mm deep, flat-bottom seat-slat pilots at stock X 14 mm, Y
12.5/102.5/192.5/282.5/372.5 mm, Z 90 mm. The named feature, all-body Browser
visibility restoration and Ctrl+S produced the
[rail-pilot checkpoint](../examples/checkpoints/garden-bench-rail-pilots-human-ui.limo):
twenty-one features, seven definitions and ten occurrences. Upper and lower
rails retain separate definitions and their correct shared pilot patterns.
Saved parameters and exact live/archive comparison passed; the full frame was
captured. Remaining post pilots, geometry, joints and drawings are unfinished.

On clean matched `0226ec9f`, guarded physical Hole input added the left-front
post's six Ø3.5 mm, 39 mm deep, flat-bottom rail/arm pilots at stock
X 0 mm, Y 32.5 mm, Z 365/400/170/205/590/615 mm. The named feature, restored
all-body visibility, Isometric/Fit and Ctrl+S produced the
[left-front pilot checkpoint](../examples/checkpoints/garden-bench-left-front-pilots-human-ui.limo):
twenty-two features, seven definitions and ten occurrences. Saved parameters,
previous hole patterns and assembly preservation, and exact live/archive
comparison passed. Right-front apron and both rear-post pilot patterns,
remaining geometry, joints and drawings are unfinished.

On clean matched `cb464867`, physical Hole input added the right-front post's
two Ø3.5 mm, 39 mm deep, flat-bottom apron pilots at stock X 32.5 mm, Y 0 mm,
Z 350/390 mm. Naming, restoring all Browser eyes and Ctrl+S produced the
[front pilot checkpoint](../examples/checkpoints/garden-bench-front-pilots-human-ui.limo):
twenty-three features, seven definitions and ten occurrences. Both front posts
now retain their apron and six-position rail/arm pilots. Saved parameters and
exact live/archive comparison passed. Both rear-post patterns, remaining
geometry, joints and drawings are unfinished.

On clean matched `fe195308`, physical Hole input added the left-rear post's two
Ø3.5 mm, 39 mm deep, flat-bottom apron pilots at stock X 32.5 mm, Y 0 mm,
Z 350/390 mm. Naming, restoring all Browser eyes and Ctrl+S produced the
[left-rear apron checkpoint](../examples/checkpoints/garden-bench-left-rear-apron-human-ui.limo):
twenty-four features, seven definitions and ten occurrences. Its saved hole
parameters and exact live/archive comparison passed. Rear rail/arm pilots,
right-rear apron pilots, remaining geometry, joints and drawings are unfinished.

On clean matched `0528a7db`, physical Hole input added the right-rear post's two
Ø3.5 mm, 39 mm deep, flat-bottom apron pilots at stock X 32.5 mm, Y 0 mm,
Z 350/390 mm. The named [rear apron checkpoint](../examples/checkpoints/garden-bench-rear-aprons-human-ui.limo)
has twenty-five features, seven definitions and ten occurrences with all Browser
eyes restored. Saved parameters and exact live/archive comparison passed.
Both rear rail/arm pilot patterns, remaining geometry, joints and drawings remain
unfinished.

The October 7 manual reconnect now has an explicit `cad-call --installed
--interactive` path. It verifies the recorded clean deployed identity, enabled
native control and all installed payload hashes without rebuilding, promoting or
closing CAD when the source checkout advances. Live reconnect to the unchanged
`0db4c009` payload returned a matched GUI/MCP pair and rejected an inactive bench
session after a second tab became active. Rendered bench capture succeeded. A
foreground request was denied and a fresh observation confirmed CAD was still
in the background; physical input qualification after this reconnect remains
pending foreground ownership.

The Windows focus source now synchronizes with the window queue for at most one
second after an accepted activation request, then rechecks process/document/window
ownership and actual foreground state. Its native-control build check and full
native desktop build pass. The fixes were deployed from clean `2e98c2f5`; the
reopened bench reports a matched GUI/MCP pair. Genuine Windows foreground denials
remain correctly rejected. On October 8, an accepted guarded activation on clean
`75a04561` was followed by a fresh foreground=true observation and a physical
click selecting the bench tab. This qualifies that accepted activation and
pointer sequence; it does not establish universal focus or gesture timing.

The user-reported `Medix_KW22_v4.STEP` import reached OCCT transfer, then failed
with "could not discretize tangential face boundaries without crossing chords".
The complete 42 MB SolidWorks AP214 file contains 24 solid definitions and 8,478
faces. The custom mesher now recognizes crossings inside a real shared vertex's
OCCT vertex/edge tolerance instead of refining those tolerated junctions until
failure. Crossings outside that tolerance retain refinement and rejection;
remaining errors identify the face, wire, edges, curve types and tolerance.
The STEP source is unchanged. The native desktop build passed and the correction
is installed in `2e98c2f5`; physical UI import qualification still requires
foreground ownership. This does not yet establish successful Medix import or
broader geometry qualification. Before deployment, the prior bench's published
model was compared semantically with its saved archive and matched exactly; its
second tab contained no features after the failed import. Recovery and session
data were preserved.

On October 8, foreground ownership was confirmed and guarded physical input
opened a new tab, selected File > Import STEP, entered the Downloads path in the
owned native dialog and accepted it. The original crossing-chord error no longer
occurred, but import still failed at body 1 face 2225 (area 0.14921575060521969,
aggregate mesh status 6); the tab remained empty at generation 1. The mesher now
runs OCCT's standard healer before its custom circular-boundary sampling, then
checks affected boundaries before clearing intersection-specific failure flags.
Generic failures and strict nonzero-face triangulation checks remain. Additional
diagnostics report the failed face's own status, surface and bounded wire/sample
counts. Clean `75a04561` built and was retried through guarded physical input;
the same face still failed. Its own status was 6, its surface was B-spline, and
its three boundary edges had 22, 11 and 18 samples. The next correction refines
only edges reported by OCCT's boundary-intersection checker, evaluates added
points on the exact curves, preserves healed UV endpoints and rechecks adjacent
faces. It is bounded to eight passes, 4,096 points per edge and 65,536 added
points per model. Strict rejection of unresolved nonzero faces remains. This
candidate built and was physically retried from clean `ef4c3422`. The same face
still failed with status 70 and edge sample counts 2,689, 11 and 2,177. Refinement
alone did not resolve it; successful Medix import is not yet established.
Clean `15d127a0` built, deployed and was physically retried on a matched GUI/MCP
pair. The crossing is between internal samples of two edges sharing a vertex:
its surface point is 0.0347300908856 mm from that vertex, whose tolerance is
0.0346996401522 mm. One source pcurve differs from its native 3D curve by
0.0000319350385369 mm there. The standard healer shifted that edge's endpoint
by 0.0262846534818 mm on the surface. Dense sampling reached its per-edge limit
without removing this crossing. The next investigation targets a transactional,
tolerance-bounded repair of the sampled junction while retaining shared-edge
sample consistency, exact imported topology and every nonzero face.
Clean `a1e7f5da` built, deployed and was physically retried on a matched GUI/MCP
pair. Seven sampled junction repairs passed their adjacent-face boundary checks;
the first unresolved face moved from 2225 to 2281. Import still failed and the
new tab remained empty. The next crossing is 0.0314075013296 mm from its shared
vertex, whose tolerance is 0.0313735171429 mm; the measured source pcurve/3D
discrepancy there is 0.0000174693969219 mm. The candidate preserves imported
topology and rejects unresolved nonzero faces. Successful Medix import remains
unqualified while this next junction is investigated.
Clean `afe296af` built, deployed and was physically retried on a matched GUI/MCP
pair. The repair now includes measured original pcurve/3D discrepancies at the
shared endpoint, each bounded by its incident edge's recorded tolerance. All
eight sampled junction repairs passed. Import progressed to body 1 face 3779,
a spherical face with signed area -0.000015055593139196334 mm², status 4 and no
reported boundary intersections. That nonzero face was rejected rather than
omitted; the import tab stayed empty. Its meshing failure is the next unresolved
issue, and successful Medix import is still not established.
Clean `dee3319c` built, deployed and was physically retried with additional
read-only failure diagnostics. Face 3779 is a radius-1.2 mm spherical strip with
U span 1.560393444807 radians and V span 0.00001503703578442 radians. Its sampled
wire has positive UV area 0.00000278218692873, and the sphere range splitter is
valid. The default Watson triangulator produced no triangulation despite the
accepted boundary. Face tolerance is 0.0000001 mm; both circular edges have
approximately 0.04823 mm tolerance. This distinguishes the remaining failure
from the resolved boundary intersections. A bounded alternate triangulation of
the same shared samples is being investigated; complete import remains open.
Clean `6cb350e4` built, deployed and was physically retried on a matched GUI/MCP
pair. The bounded spherical Delabella retry was attempted on two freshly failed
faces, but neither yielded an accepted triangulation. Face 3779 still failed and
the import tab remained empty. No failed face was omitted. Retry progress-range
ownership and more precise trial diagnostics are the next investigation.
The diagnostic subclass in `000a546d` failed Windows linking because it exposed
unexported OCCT constraint helpers; deployment preserved the installed runtime.
The correction uses the exported factory API and bounded boundary diagnostics.

Clean `2ae55e47` built and was manually retried. Native curve measurements
confirmed an interior self-crossing in the thin spherical boundary, away from
either shared endpoint's tolerance envelope. Neither Watson nor the bounded
Delabella retry accepted the two affected faces. Their exact geometry was not
changed to invent a triangulation.

Clean `4d3cf633` built, deployed and passed the actual Windows UI import and
save/reopen walkthrough on a freshly observed matched GUI/MCP pair. Original
imported STEP bodies may now open with explicit display warnings for unavailable
face triangles. Exact STEP/BRep geometry, stable face slots and boundary edges
remain retained. The persistent amber banner and read-only MCP summary expose
these omissions; derived modeling, cutaway and mesh exports remain strict.
Imports without any usable triangles still fail.

The Medix import has one compound body, 8,908 face identities, 453,400 display
triangles, no feature errors and two warnings (`face:3779`, `face:3792`). It was
saved through the native picker as
`D:/limo-cad-maintenance/ui-models/Medix KW22 v4 - human UI import.limo`.
The archive's embedded STEP is byte-for-byte identical to the 42,065,951-byte
Downloads source (SHA-256
`17d7473c73db7bdf302aa118fe377bae10578f1c72fe03238f8448545d26d8d2`).
Closing and reopening through File > Open reproduced the same body metrics,
warning keys and saved model. A physical File > Export All Bodies as STL attempt
was explicitly refused for incomplete display tessellation; no STL was created.
Evidence is retained outside source under `D:/limo-cad-maintenance/ui-models/medix-4d3-*`.
This qualifies opening and document preservation, not complete triangulation or
printability of Medix. No test suites or recipe replay ran. The separate
Roller-300 scratch runtime was not restarted or operated. A foreground change
during the export check was recovered by guarded focus, fresh observation and
retry; universal Windows focus behavior remains unqualified.

The complete native-picker capture guard at `cbe74672` was manually exercised
on matched clean `0dbd0fdf`: a fresh Downloads STEP import and Save As used
full observed dialogs before each input. The saved archive retains identical
source bytes; the additional checkpoint is
`D:/limo-cad-maintenance/ui-models/Medix KW22 v4 - fresh UI import 0dbd.limo`.
The mesh-only spherical repair at `aeac1cd1` passed its geometric preparation
checks but refused nonanalytic neighboring charts. Clean `15c67f8c` retains
their healed UV trim polygons and bounds shared-point residuals instead of
performing ambiguous inverse projection. Both trials reach final adjacent
coverage/incidence validation, which rejects them and rolls back. A fresh
matched GUI/MCP pair still reports both original warnings;
complete triangulation and mesh export qualification remain unfinished.

Further matched runs through `6c6d5c5b` preserved and restored omitted native
trim corners only after coverage and precision checks, but still rolled back
both trials. The clean `feb57db3` run includes the repository transfer to
`limo-cad/Limo-CAD` and the `2252074f` diagnostics. Those identify native
degenerate edges on neighboring faces 5925 and 5497: their native endpoints
coincide, while the imported surface-coordinate endpoints differ by about
0.031 mm. The original model remains unchanged and both display warnings
remain visible. A tolerance-bounded treatment of those collapsed native edges
is still under development; successful complete mesh exports are not claimed.

Clean matched `367dfbfb` qualifies the native pole's localized chart crossing
and its sampled source-to-mesh precision without changing the STEP geometry.
Both reopening the prior checkpoint and a fresh physical File > Import STEP
from Downloads report zero scene errors and zero display warnings, with all
8,908 face identities and 453,491 triangles. Escape dismissed the import's
appearance panel; a physical Fit and native Save As created
`D:/limo-cad-maintenance/ui-models/Medix KW22 v4 - complete human UI.limo`.
Its embedded 42,065,951-byte STEP has the original SHA256 and the live model
matches its saved archive. This is complete display qualification, not complete
export qualification: actual STL export contains 27 triangles collapsed during
epsilon welding, and actual 3MF export refuses a degenerate welded triangle.
The initial STL is retained as diagnostic evidence; it is not a qualified mesh.
Source-aware export precision work remains open. The 536 mm imported model
also exceeds the default printer envelope; portable export does not qualify
physical printing or its unchanged source placement.
The matched `00603de3` read-only native tessellation inspection returns all
453,491 triangles without a float-precision failure. Its actual UI STL retry
is then refused by the strengthened writer at triangle 22,934, with no new
file created. The export placement layer was found to weld nearby distinct
vertices before the format writer. Preserving raw STL placement and trying
exact-coordinate indexing before 3MF tolerance welding are awaiting UI
qualification; both formats retain strict geometry checks.

Clean matched `f062b45a` now passes actual UI STL export. The new
`D:/limo-cad-maintenance/ui-models/Medix KW22 v4 - verified human UI.stl`
contains all 453,491 facets, with zero nonfinite coordinates, zero-area facets,
zero normals or opposing normals; its SHA256 is
`0edc2caa21296b927e18278c12b8084dc47828dfaab44aee48fbfd006fe76a49`.
Actual tab close and native File > Open reopen the complete checkpoint with
zero scene errors and display warnings, and the live model still matches the
saved archive. Actual UI 3MF still refuses a degenerate triangle after tolerance
welding. Read-only inspection of exact-coordinate STL topology finds 94 edges
with four incident facets and 113 single-use edges: coincident native solids
must retain separate identities, while small shared-boundary discrepancies need
native topology correspondence. Raising a global tolerance would collapse valid
small facets. Native topology-indexed export remains under development; no
successful 3MF or physical-print qualification is claimed.

The next clean matched `26a7ea22` uses native shell/vertex/edge identities for
export indexing and checks native winding before writing. Actual UI 3MF
preflight exposes a further precision defect at face 3794, triangle 1: ordinary
nearest-f32 rounding alone reverses this approximately 14-micrometre edge facet
by 1.649 radians. The same ordered rounded vertices are present at facet 160,565
in the preceding STL. Its earlier normal check verified stored normals against
serialized vertices, so that file is now retained only as diagnostic evidence;
it is not a native-winding-qualified export. A separate f64 export channel and
precision-preserving text STL path are under development. Display remains at
zero errors/warnings and the saved source geometry remains unchanged.

Clean matched `280e1f64` installs the separate double-precision native export
channel and retains every positive-area native facet. All 70 installed payload
hashes match the clean manifest. Reopening the complete checkpoint still reports
zero scene errors and display warnings; its live model matches the saved archive.
Actual UI 3MF preflight now reaches native closure validation but refuses 115
unmatched links across 33 native shells. Retaining positive tiny facets did not
change that count. Source-face diagnostics identify boundary shortcuts, including
faces 2449 and 1491, whose neighboring faces retain intervening native edge
samples. A native-topology-proven repair remains open; no precision-qualified
STL or successful 3MF export is claimed. Canceling preflight leaves the document
unchanged and the CAD window active.

Matched `b3d5d5fc` adds export-only, source-checked boundary recovery and corrects
its false candidates on ordinary reversed boundary links. Actual UI preflight
still refuses the same 115 links: five attempted face repairs were rejected,
with none installed. The ranked diagnostics identify genuine native degenerate
edges at the largest failing face groups, with coincident native endpoints but
distinct surface-coordinate representatives. Pole-aware recovery remains open;
the native precision, domain and closure guards have not been relaxed.

Matched `04cc231f` reduces the actual UI export refusal to 107 invalid links:
105 unmatched boundary links and two links used four times. Native edge ownership
is valid at the largest failing groups; the remaining gaps belong to the copied
export triangulation. Four positive boundary facets were restored with source
checks. The two multiply used links are internal diagonals on curved native
faces, whose incident UV regions classify inside their source trims.

Clean `175f571f` builds and installs without errors; all 70 payload hashes match
its manifest. Actual GUI 3MF preflight still refuses those 107 links. Both tested
surface-point diagonal refinements were rolled back after the complete face
winding guard rejected them. The live Medix document matches the saved complete
checkpoint and reports zero scene errors and display warnings, with 8,908 faces
and dimensions 536 × 327.47 × 48 mm. No native-precision-qualified STL or 3MF is
claimed. The foreground GUI/MCP pair is matched; unique GUI output/error logs
are retained for the previously unexplained process exit.

The export-only minimum-size trial at `3b61ad80` increases the actual refusal
to 136 links. Its diagnostics identify a genuinely inverted original native
facet, with a normalized source-normal angle of 3.009 radians; the derivatives
are finite and nonzero. `6d916088` reverts that minimum-size setting and trials
a larger source UV patch. Actual UI preflight returns to 107 links, rejecting
its replacement child at 0.810 radians against the unchanged 0.700 interior
angle budget. No export artifact is accepted by either trial. Source geometry
and the complete Medix checkpoint remain preserved.

Matched `d724917f` resolves both multiply used links through the certified larger
UV patch and centre search: actual GUI 3MF preflight now reports 105 unmatched
boundary links, with no multiply used links. One internal patch is accepted;
the boundary recovery count remains unchanged. Export is still refused and no
successful 3MF is claimed. The remaining boundary gaps are under investigation.

The bench Hole dialog was reopened through physical input on matched `3b61ad80`
without reproducing the earlier unexpected process exit. Enter in a position
field accepted the dialog; physical Undo removed that premature hole and restored
eighteen features. A separate local **Crown garden bench - lower stock isolated
human UI.limo** preserves the isolated lower-stock view. Its sketches, extrudes,
holes and assembly match the original bench archive, and its live model matched
the separate saved checkpoint before restart. This adds no lower-rail bores;
the original bench document and committed checkpoint remain intact.

The October 7 [human-operated bench checkpoint](../examples/checkpoints/garden-bench-human-ui.limo)
was built through real OS mouse/keyboard input on matched clean GUI/MCP builds
through `0db4c009`. It contains eighteen named features, four separate posts,
two shared aprons, two shared upper side rails, eight post pilots, six shared apron clearance positions,
four shared upper-rail clearance positions, a separate 28 × 415 × 90 mm lower-rail
stock and five named views. The lower stock is fully constrained but still has no
bores, component definition, appearance or assembly placement. UI checks cover fully
constrained stock, translated/rotated shared editing, precise multi-position Hole
editing, instance removal and Undo/Redo. The [walkthrough](../examples/scripts/README.md)
states its unfinished geometry, joints, drawings and print layouts. The vise and
turbine have not yet been rebuilt in this human-operated pass. No test suites or
recipe replay were run for this pass; read-only MCP inspection verifies the result.

Live sketch checks on `f055cc02` confirmed that Dimension can pick an edge through
its constraint glyph without hiding annotations. Select still opens the glyph
inspector and the stored dimension editor; cancellation retains zero degrees of
freedom. Wheel zoom worked over a glyph after ordinary Select interactions. The
first wheel after Fit did not move the camera; its cause remains unattributed,
so this does not qualify every move/wheel timing sequence or physical pinch input.

Native computer control is opt-in through `native-computer-control`, with empty
default features. The Windows backend reuses Enigo for input, Windows Capture for
owned native dialogs and that crate's in-memory PNG encoder. The generic Windows
qualification driver uses Rust and this same control path behind xtask's opt-in
`native-control-harness` feature; specialized IME/print probes and Linux/macOS
drivers still include platform scripting. These source changes do not establish
cross-platform input qualification.

The historical `7137887f` payload passed ten SDK-free MCP checks and 27 steps,
including Save, disconnect survival and guarded close. That evidence remains
specific to that revision and does not qualify the newer walkthrough or replace
the public Windows/Linux package qualification.

Cursor and Codex now register **`limo-cad`**. Their retired CAD entries were
removed; the retired Grok entry was also removed. The Start menu, project/recipe
associations and previous launch paths route to the new payload. The three retired
physical payloads were deleted after the user's manual cleanup. Documents,
recovery saves and the session registry remain intact. See
[runtime identities and migration](limo-cad-runtime.md).

The October 5 cleanup removed 24 local branches only after confirming their
commits were reachable from Bevy and they had no open PR or active worktree.
Unique retired work remains in the verified 104-head Git bundle at
`%LOCALAPPDATA%/limo-cad/archives/Limo-CAD-retired-branches-20261003-051405.bundle`.
Three inactive runtime trees moved to
`D:/limo-cad-maintenance/retired-runtime-payloads`; all 229 files retained their
hashes, recovering about 1.6 GiB on C:. The subsequent manual deletion removed
all 229 runtime files and recovered about 2.4 GiB on D:. Their separate hash
manifests remain. Active CAD windows,
documents, recovery saves, SDKs and session data were preserved.

The complete identity migration moves native configuration to
`org.limocad.desktop` and new leases/inboxes to `limo-cad-sessions`.
The real previous profile moved intact; every existing file retained its SHA-256.
Fresh MCP attach, rendered inspection and read-only assembly execution passed
against the normal installed desktop without advancing its model generation.
The installed executable SHA-256 is
`8EDC25D99BACF40EC7B3887805AE4F39E36DB8D6B7A8F8F37738E155B36AFD02`.

Ordinary Rust/C++/JSONC comments and auxiliary workflow/probe comments were
removed while preserving Rust documentation and interpreter directives.
Stale TypeScript/React descriptions were corrected. Focused migration, archive,
CAM-header/replay and workflow checks passed; strict scoped tooling Clippy passed.
The comment-removal spacing issue in CAM documentation is corrected. Strict
all-target, all-feature Windows desktop Clippy and scoped native-engine/sketch
Clippy pass on the current integration source. The October 6 follow-up scopes
printer-only SVG resources to Windows and fixes macOS 6DoF lints; its hosted
Windows and Ubuntu native-host jobs pass. This does not establish a globally
warning-free build or qualify every platform. No large validation sweep was run.

## Implemented desktop

Bevy owns modeling/sketching, feature forms and history, assemblies and joint
motion, drawing authoring/reference repair/output, CAM, Scripts and lessons,
preferences/localization, file/window lifecycle, printing, accessibility, IME
and 6DoF input. Document units retain the shared engine's read-only contract.
The native document/OCCT host lives in `crates/native-engine`; the desktop uses
a small adapter. Its optional `native-occt` feature owns transactions, exports,
geometry revisions and tab retention without a Bevy/window dependency.
Ordinary engine builds leave that feature disabled and require no native SDK.

The conversion includes:

- Persisted interface sizes from 90% to 175%, independent cross-window refresh,
  matching layout/scale publication, and guarded pointer/text/composition state.
  The follow-up audit fixes large-size sketch menus, origin controls, drawing
  menus and CAM report bounds. Current main's normalization and Ctrl/Cmd
  plus/minus/zero shortcuts are preserved (#211, #213, #215, #251).
- Independent CAM height references for planar faces, level edges, vertices,
  sketch points and level sketch lines. The shared resolver supplies stable
  identities through the existing bounded picker and draft/Apply path (#212).
- Associative drawing exports, placed views and reference repair; installed-font
  outlines for Unicode DXF labels and native printing. Unsupported glyphs fail
  before output. Saved drawing DTOs, placement and paper size remain authoritative.
- Production AccessKit bindings with current control/document/modal guards,
  Windows UI Automation text editing, and read-only/writable field distinctions.
  IME caret placement follows visible field bounds and scale changes; provisional
  composition retains the existing editor checkpoint and committed text.
- Restored inactive-tab eviction, including finished-sketch Undo/Redo (#222, #249).
- Cached authored viewport metadata per native document, independently of
  assembly placement. Immutable engine queries share this data with UI and MCP
  and skip mutation evidence observation. Sketch/solid edits refresh metadata;
  drawing edits, placement changes and revisiting retained tabs reuse it.
  Eviction releases the cache. Browser/history actions, units/name reads and
  synchronous exports borrow their source data under the existing engine guard.
  Drawing panels and hit testing borrow retained annotation marks. Paper/cache
  keys and saved drag receipts share immutable view/style metadata; revision
  stamps remain independent so sheet selection cannot retag a saved receipt.
  The title-block cache borrows only rendered metadata during lookup and retains
  an owned snapshot after successful rendering. Annotation edits reuse frame
  artwork; rejected frames move the existing sheet snapshot into the error receipt.
  Pose-vector copies, per-body replay invalidation and presentation resource
  granularity remain tracked in [#333](https://github.com/limo-cad/Limo-CAD/issues/333).
- An explicit Winit window-icon binding (#259). The deployed Windows small-icon
  handle and native chrome capture confirm the title-bar fix. The packaged MCP
  passed schema-7 attach, rendered inspect and a read-only assembly query.
- In-place sketch editing of a selected shared occurrence (#94). The native UI
  and `sketch_edit` MCP operation use the same validated occurrence frame for
  rendering, picking and dimensions. Surrounding occurrences fade without
  replacing shared meshes. Saved sketches retain definition coordinates;
  recompute updates shared bodies and joint references. Focused tests cover an
  offset component coordinate system, translated/rotated repeats, driving edits,
  save/reopen and rejection of unrelated targets. Linked external files remain
  a separate design decision; physical bench qualification remains outstanding.
- Stateless multi-document MCP routing (#12) through `cad_route`, preserving
  the default attachment and never loading routed models. Per-document inboxes
  and control queues enforce captured owners and generation conflicts. Tickets
  retain completion receipts across tab changes, replacement and close.
- Typed shared drawing commands (#93) add/update/delete specialized annotations
  and views, preserve aligned groups, manage templates and append revisions.
  Release commands validate current topology and occurrence selection. Accepted
  model/assembly edits return affected issued sheets to Draft while retaining
  revision history; unchanged recompute and save/reopen preserve issued metadata.
  Native view and annotation editing use these shared operations. Idle annotation
  preview borrows its sheet, and point/radial pick targets reuse projection stamps.
  Drawing topology capture borrows the solid scene instead of copying its meshes.
- The component-edit recovery lesson (#16) uses the shared in-place operation,
  demonstrates a wrong driving value and Undo, updates rotated repeats and
  saves/reopens. Native lesson and Help catalogs expose the same authored source.
  Teaching and physical-input review remain open.
- The bench recipe creates 22 editable review sheets, including part dimensions,
  machining coordinates, an assembly sheet and cut list. The focused native check
  reproduces every SVG/DXF after reopen and updates the picket-height dimension.
  Its notes retain authored machining inputs; manufacturing review and physical
  timber/hardware qualification remain required.
- System appearance following (#272). Bevy 0.20 moved Winit windows out of the
  World; querying the obsolete resource always selected Light. The main-thread
  capture now reads initial OS appearance and primary-window theme-change events.
  A focused regression covers Dark/Light changes and ignores other windows;
  explicit appearance preferences still take precedence.

Quick-win fixes retain viewport/input/accessibility state, warm drawing-sheet
projections and CPU rasters, reduce document/scene copies, and use exact completion
revisions and document-specific mesh-cache incarnations. Incoming parent commits,
main's naming/translations/drawing changes and the recovered mechanism-dragging
implementation are preserved. No runtime latency improvement is claimed without
measurement.

## Inactive-tab retention

The former desktop's low-memory eviction was lost when its memory-status caller
was retired. The native watcher now probes physical memory every 30 seconds.
The ordered worker makes eligible inactive tabs cold after 60 minutes; constrained
memory evicts the oldest eligible tab and critical memory evicts all eligible
inactive tabs. Active tabs, unfinished sketches and saves in progress are protected.

Cold tabs retain their parametric model, geometry revision, replay baseline,
file/archive ownership, saved receipts and history while releasing the OCCT
engine, Bevy model meshes and drawing caches. Activation rebuilds transactionally
and verifies body identities and feature errors. Failure preserves the snapshot
and previous active tab. The 128-tab bound includes cold tabs.

The October 3 audit also found that serialized reconstruction discarded finished
sketch command stacks. Finished sessions now move into a separate in-memory
retention record, preserving Undo/Redo, runtime editing state and entity identity
high-water marks without retaining an OCCT kernel or solid scene. Rebuilt sketch
states must match before ownership moves; mismatch leaves the snapshot available
for retry. Normal project serialization/schema and file-reopen history policy
are unchanged.

Ten focused Windows tests passed, including real-OCCT reconstruction, repeated
eviction, actual sketch Undo/Redo, rejected restoration and retry, mismatched
sketch-state rejection, file/archive history, protected states, pressure/idle/LRU
policy and drawing-cache isolation. They were isolated in-process checks and did
not change a live document. **The public preview includes both eviction
restoration and the finished-sketch history fix.**

## Dependencies and build tooling

Rust `1.99.0`, rustfmt and Clippy are pinned together. Bevy stays at rc.2; OCCT
stays on the 7.9 ABI. AccessKit remains on Bevy's `0.24` types, Windows bindings
on wgpu/gpu-allocator's shared `0.62.0` types, and usvg/resvg on `0.45.1` because
svg2pdf `0.13` consumes those trees. These are compatibility constraints.

Repository maintenance uses Rust `cargo xtask`: scoped checks/Clippy, dependency
inventory, deterministic archives, version/tag/repository/icon/knowledge guards,
OCCT SDK orchestration, native fixtures, WASM build/smoke, packaged MCP setup and
Windows ZIP/Linux DEB/AppImage/macOS app/DMG packaging. Builders retain runtime
library and license staging, audits, checksums and platform signing/notarization.

Tauri, embedded WebViews, the `dev-bevy-host` switch, React/Three.js source, npm
manifests/lockfiles, Vite/Tailwind configuration, Node drivers, legacy bundlers
and obsolete browser/desktop IPC harnesses are removed. Native SDK containers
and packaging workflows do not provision Node/npm. Embedded vectors and locale
dictionaries remain in `assets/`; viewport colors live in Rust.

Remaining native OS qualification helpers use shell, PowerShell, Python, C# or
Swift for platform APIs and input. The repository is not entirely Rust.
Retired harness ownership is recorded in [scripts/README.md](../scripts/README.md);
that reassignment does not establish equivalent coverage or passing native cases.
Unused Feathers/scene support, redundant widget declarations, unused icon
variants and GTK/Rsvg AppImage development inputs were removed. Winit's actual
X11/XCB/cursor/input runtime libraries and Linux desktop portals remain required.
`sysinfo` is required again for the portable physical-memory probe.

The incoming standalone SDK-cache repair in main #245 is also ported to Bevy
(#302). Reused extracted sources are verified against the pinned archive before
reuse and receipt publication. Install-prefix locks exclude concurrent installers
using different cache directories; receipts reject external/dangling SDK links
and track internal targets. Eight focused Windows cache/SDK checks and strict
all-target xtask Clippy passed. The two Unix symlink fixtures remain for hosted
Linux qualification; no real SDK was downloaded, rebuilt or modified locally.

Focused checks cover Windows desktop/MCP compilation, Rust/wasm32 compilation,
Clippy, repository/version/icon/knowledge guards, package staging/deletion guards,
archive determinism and a fresh engine-facade build. Rust task-runner compilation
also passed for Linux x64 and macOS ARM64; compile checks do not qualify native
packages or signing. No broad validation sweep is being run.

The October 6 drawing follow-up passed strict Desktop/MCP all-target, all-feature
release Clippy and all three workspace format checks. Eleven focused native checks
cover exact view/release Undo/Redo, borrowed idle preview, retained paper navigation,
lesson catalogs, guarded hole authoring and profile export. Native MCP checks cover
specialized annotations, stale-edit rejection, template/revision commands and
release preservation/revocation. The bench check reproduces 22 SVG/DXF sheets after
reopen and updates an associative dimension after a height edit. Its Rust author is
idempotent and preserves all 883 original construction steps and original checks.
The opt-in turbine recipe reproduction binds both placed assembly views to the
production Bevy paper image and reuses the document/image on idle repaint. Its
linework pixels were independently inspected; this CPU image proof does not qualify
GPU scanout or OS-window capture. These additions postdate the historical
`7137887f` payload and public `9b082687` packages above. Consult the current local
runtime manifest for installed source identity; public package qualification is
still recorded separately.

## CI and security review

The October 4 cleanup landed full native-workspace formatting, narrow Clippy
fixes and compiler API updates, plus persistent fmt/Clippy jobs in the existing
required workflows (#284, #286-#288). Local native all-target/all-feature release
Clippy, root tooling Clippy and all three workspace fmt checks passed. The next
pass cleared the 20 xtask warnings and added a strict all-target xtask gate
without replacing workspace-wide Clippy (#298). Four core default warnings are
also cleared with unchanged defaults and serialization (#300; main #299).
Strict all-target checks pass for those packages; other workspace warnings remain
visible. Focused existing regressions and workflow contracts passed. Actionlint
uses an explicit inventory of the verified Ubuntu 26.04 and Windows 11 VS2026 ARM
runner labels; unknown-label errors are not ignored.

The `b2e5e241` and `985392f2` integrations passed Windows, Ubuntu and macOS native
host CI. Later integration source requires fresh checks. The `985392f2` package
and MCP runs exposed three separate failures now repaired below.
No broad local validation sweep or new live-model test was run.

CodeQL 2.27.1 now scans Rust, C++, Actions, JavaScript and Python (#289; standalone
main counterpart #285). Matching Rust compiler, sources and proc-macro server
bindings restore usable extraction for current Bevy dependencies; scan checkout
uses LF to avoid the extractor's escaped-newline CRLF parsing defect. The local
Rust scan extracted 775 files, with one platform-only macro warning and no
extraction-error query results. Its 146 logging alerts were source-audited:
138 are test diagnostics and eight are production diagnostics carrying public
CAD routing UUIDs. The IDs are discoverable through session tools and do not
authenticate callers. The matching GitHub alerts were dismissed as false
positives and three bot threads resolved; the logging query remains enabled.
The corrected Actions scan reports zero findings. This historical scan does not
qualify the current head. Main #285's Rust and C++ jobs hit the 90-minute job
deadline: Rust spent about 69 minutes preparing the cold SDK, and C++ was still
building it at cancellation. The neutral SARIF check came from unsuccessful-run
diagnostics, not a completed Rust scan. Actions and JavaScript uploads succeeded
on that exact merge checkout; native coverage remains pending on the corrected
workflow. SDK caches were already published after successful setup, so cache
publication and security coverage were retained. #301 applies the same bounded
repair to Bevy: a 240-minute job, 150-minute SDK setup and 60-minute native analysis
phase. Fresh completed scans must still qualify the new heads. Main's earlier
Tauri macro warnings also need review after its scan completes; the clean Bevy
extraction does not establish main coverage.

The audit also confirmed Unix session snapshots could be readable by other local
users under permissive default filesystem modes. The shared Rust storage policy
is merged into Bevy (#292), with a separate main PR (#291). Unix uses a per-user
registry with private directories and snapshots. Reads and writes validate
ownership and ancestry, and reject symbolic links or non-regular payload files.
Existing registries must already be private and are never made acceptable by
changing their permissions. Windows keeps its existing default discovery
location. No live registry was moved or modified during this work, and the
installed package was not replaced.

Both privacy branches passed their focused hosted Windows and Ubuntu checks:
fmt, strict all-target storage Clippy, four Windows or 15 Unix storage tests and
ten actual MCP inbox tests. Ubuntu also passed all 15 storage tests under `sudo`
on disposable fixtures. The Bevy results qualify `bac76ce3`, merged at
`34cf84e7`; the main results qualify `03a15973`. They do not replace the remaining
required native/package checks or external main review.

The turbine acceptance reader now expands actual 3MF component/build transforms
and repeated occurrences into world coordinates (#293). It retains unit, finite
vertex, index, positive-volume, closed-edge orientation and solved-placement
checks. Four pure regressions and recipe-target compilation passed on both the
Bevy child and the corresponding main feature fix (#257). The preceding main
head `f2a9f396` passed hosted Windows/Ubuntu acceptance and desktop packaging.
The October 4 follow-up integrates hierarchical 3MF and named print layouts
into the Bevy native host and controls, using the existing CAD hierarchy and
one saved-view model. It includes printer selection, diagnostics, whole-group
corrections, deliberate export, repeated instances, and source-edit guards.
The actual Windows Bevy walkthrough and Bambu Studio 2.8.2.61/OrcaSlicer 2.4.1
native export round trips passed locally. Fresh pinned printer fetching matches
the embedded catalog. The material catalog remains the existing unified surface.
See [Bevy print-layout integration](bevy-print-layout-integration.md) for usage,
qualification and limits. This branch implementation does not change previously
published packages; current hosted/package checks and independent review remain
required.

MCP aggregate CI jobs now run on shard failures and skip whole-run cancellation
(#295; standalone main PR #294). Their required names, success-only gates and
artifact provenance remain intact. Focused workflow contracts and actionlint
passed. Superseded owned runs were canceled after source-identity checks; checks
for the latest open PR heads were preserved. A separate aggregate failure came
from cached empty generated-demo directories. Registry-only aggregate caches now
exclude build targets (#296); fresh-directory reservation, artifact identity and
fail-closed validation remain unchanged. Four focused workflow contracts passed.

The material PR (#263) merged normally at `59195010`, retaining both dependencies
in its sole lockfile conflict and all feature work. It also repaired the new
Linux package privacy failure: live profiles and session fixtures use an owned
private `/tmp` directory, with diagnostics copied to `RUNNER_TEMP` on exit.
Unsafe shared-runner ancestry is not accepted. Focused Linux success/failure
fixtures accompany the change; current hosted package qualification is pending.

The Windows ARM package input guard correctly refused hosted Start/Search
occluders. The preflight now dismisses only identity-verified foreground or
observed shell windows on disposable hosted ARM runners (#297). The native input
guard remains unchanged. Eight account-window and 22 shell-window managed checks
passed; actual hosted ARM input qualification remains pending.

Jack's draft GPU-stock PR #268 has separate fixes for finite-flute eligibility,
deferred grid visibility during paused playback, and missing tool-change timing.
Multi-tool or ambiguous timelines now retain CPU stock; a default-false producer
flag and unanimous matching-layer proof limit GPU removal to known single-tool
timelines. The existing CPU simulation remains authoritative. Native compilation,
frontend typechecking, the focused path producer check and actual Rust/ECS
regressions passed. The new commits do not have fresh interactive GPU evidence.
This main draft still targets the earlier viewport and is not integrated into
the Bevy rc.2 application.

CodeQL alert [#147](https://github.com/limo-cad/Limo-CAD/security/code-scanning/147)
identified unsafe matrix-copy arithmetic in the packaged OpenCASCADE 7.9.3 header;
that SDK remains unpatched. Its size calculation can overflow or narrow before
`memmove`. A large-matrix application trigger has not been established. The
upstream 8.0.1 repair changes class layout and cannot be copied into the 7.9 SDK.
This finding is retained for an ABI-compatible SDK repair or a separately
reviewed SDK migration; it was not suppressed to clear CI.

## Deployment and preserved data

Codex/Cursor MCP settings use the installed Windows runtime above with
`--headless` and `LIMO_CAD_DESKTOP_BIN`. The Rust installer supports in-place
packaged runtimes and preserves Codex TOML comments (#258; main PR #262).
Start-menu, recipe URL, `.limo` file association, PATH and App Paths entries
select Bevy. Projects,
session inboxes, heartbeats and recovery snapshots survive runtime replacement.

The packaged MCP repair (#303) preserves absolute Windows UNC paths in every
client serializer. All 21 installer checks, formatting and strict all-target
xtask Clippy passed; the regression does not require a network share.

The October 4 Windows rebuild uses clean source `9b082687`. Candidate and
installed packages passed SDK-free headless and desktop MCP verification: ten
checks and 27 command steps, live-document binding, real geometry/export, Save,
retained unsaved work, disconnect survival and guarded shutdown. The checks used
private fixture documents. All five installed launch aliases have the same
executable checksum; older compatibility directories are junctions to that
runtime. A missed `.limo` association to a removed 0.1.0 download was repaired.
Three live designs were saved through MCP before the previous runtime closed.
Their recovery documents, session snapshots and deployment receipts are outside
Git under `D:/noBS-CAD-builds/bevy-prerelease-20261004`; earlier maintenance
receipts remain under `%LOCALAPPDATA%/nbcad/maintenance`.

Two audited purges remain outstanding: the 57 retired runtime binaries/DLLs in
`Roller-300/.local/cad-runtime-retired-20261003`, and
`C:/Users/jeffg/dev/noBS-CAD/target/debug/incremental`. Automatic approval review
rejected deletion with "blocked by policy", including the incremental-cache
request after explicit operator approval. No files were deleted in either purge.
The inactive cache was compressed on October 4; current build and temporary
outputs use D:. A subsequent purge of the October 4 retired installation and
inactive development executables was also rejected before execution. Those
copies remain; normal launch routes use the new installed runtime. Source
worktrees, CAD documents and live-session data are preserved.

Main PRs #308 and #309 were consolidated into #262 through merge `80e6bc3`,
preserving their original commits. They are closed as consolidated work; #262
still requires Jack's approval before main. No main merge was performed.

## Release qualification still open

The public Windows x64 ZIP passed SDK-free headless and desktop MCP checks on
Thunder, both before and after installation. The Ubuntu DEB passed headless and
desktop MCP, X11 input/rendering and Wayland-desktop lifecycle/URI checks in the
[tagged package run](https://github.com/limo-cad/Limo-CAD/actions/runs/37232261112/job/111525874975).
The restored-window and Unicode-field X11 captures were reviewed. Checksums and
embedded metadata identify clean `9b082687` source; a machine-readable build
receipt accompanies the packages. The hosted Windows x64 build also passed
MCP and native input/render checks on this source; its restored-window capture
was reviewed. The public Windows ZIP remains the verified local rebuild.

Other preview targets remain withheld:

- **macOS:** compiled and Developer ID signed; Apple notarization returned
  HTTP 403 for a missing/expired team agreement. The account owner must resolve
  that agreement before notarized distribution. No Intel Mac package is qualified.
- **Windows ARM64:** compiled and passed headless checks; the owned input fixture
  refused a click through hosted-runner Start/Search windows. #297 repairs the
  hosted preflight; fresh ARM native-input qualification remains open.
- **AppImage:** preceding-source build/glibc/headless checks passed and X11 startup
  was reached; input stopped because the host lacked `xclip`/`xdotool`. #223 restores
  those prerequisites. The later shared-runner session-fixture privacy failure
  is repaired in #263. This does not establish current-source package success.

Historical source-specific checks also cover Windows UI Automation, drawings and
Unicode output, CAM, Scripts, mechanisms, preferences and lessons. They do not
establish current-head package/device qualification. Outstanding limits include:

- Remaining platform packages,
  required PR checks and external review. Stable `v0.2.2` is a separate legacy
  release; its presence cannot qualify the Bevy branch.
- Fresh Windows/macOS Japanese IME evidence for the latest field implementation,
  candidate-popup placement and physical monitor/DPI transitions.
- Physical printing, macOS/Linux OS print dialogs, screen-reader speech,
  actual 6DoF hardware/driver behavior and macOS OS GetURL delivery.
- Broader real-input annotation/joint/gesture workflows. The joint fixture's
  read-only settlement correction has not been rerun; Scripts chooser gestures
  and physical multiline-editor IME are not established by source-level checks.
- Switching sputter attribution. The observed Windows tab/sheet irregularity
  has no matched current-source reproduction or latency benchmark. The optional
  comparison uses native Bevy builds; see [measurement scope](native-switching-measurement.md).

A build, ignored check, stale-source pass or synthetic geometry assertion is not
a current-device runtime pass. Evidence and generated captures are retained
outside product source under `D:/noBS-CAD-builds/finish-bevy-rc2`.

## Browser work still open

The replacement must reuse the desktop Bevy UI. The current Rust WASM engine
facade builds and has focused binding checks; it is not a browser CAD app.
The complete Bevy WASM host, file/storage/dialog services and geometry-service
transport remain unfinished. The planned first browser host offloads geometry
to native Rust/OCCT. The extracted native-engine host is its service-side
foundation. An optional in-browser OCCT WASM backend is separate work; the
native-service approach does not require that port. See [web/README.md](../web/README.md).

## Audited deletions and history

The snapshot [`6394fb44`](https://github.com/limo-cad/Limo-CAD/commit/6394fb449f12e17dededd76dc702081ff7c277eb)
was previously mislabeled as an unfinished UI rewrite. Independently formatting
all 81 changed Rust files and their parent versions produced identical output:
it was formatting, not an upcoming feature. Its explicit revert removed no
functional implementation; the source remains reachable from
`feat/bevy-switch-timing`. The experimental accessibility tree at `8986fd77`
remains preserved; the production adapter supersedes its disconnected tree.
Recovered mechanism work remains implemented and documented in
[native-mechanism-drag.md](native-mechanism-drag.md).

Redundant integrated branches/worktrees and backup refs were retired only after
checking source representation and archiving unique history. Active work,
projects/session data and verified Git archives remain protected. Obsolete
September checkpoint prose and duplicated old release/validation narratives
are removed from this active status document; Git history retains them.

The October 4 comment cleanup at `ee7b07ce` changed 356 Rust files. Tokenizing
each file and its parent with Rust's `proc_macro2` produced identical token streams,
including documentation attributes. The source audit and all three workspace
formatting checks passed; this evidence does not qualify a new runtime package.
