# Rust simulation experiment and project plan

Decision record, 2026-10-06. Tracking: [structural analysis #336](https://github.com/limo-cad/Limo-CAD/issues/336),
[section hardening #339](https://github.com/limo-cad/Limo-CAD/issues/339).

## Decision

Evaluate **Fenris + fenris-solid + faer** for engineering structural calculations
and **Rapier** for the simulation world, including its new FEM soft-body path.
Evaluate **conspire** for constitutive models and nonlinear mechanics. A project's
experimental status is acceptable: we own version pinning, regression benchmarks,
diagnostics, patches and physical validation. Do not infer accuracy from maturity
labels or attractive deformation animations.

The executable experiments live in `experiments/rust-simulation`, with their own
Cargo workspace and lockfile. No simulation dependency is added to the desktop or
engine. Structural implementation remains an experiment reported in #336, until
the study/result contracts and qualification gates below have evidence.

Measured first evidence: Fenris/faer quadratic cantilever tip displacement
0.2627746 mm versus a 0.2666667 mm beam reference, with 0.196% final refinement
change and successful equilibrium/reaction/energy checks. Rapier's rigid-body
free-fall error halves with the timestep and its ball contact settles. Its stiff
FEM beam is not qualified: 250-iteration cases are nonstationary and mesh-sensitive;
a 2000-iteration control settles at 0.1310795 mm, still 50.85% below beam theory.
The fresh identical-mesh static probe gives 0.1310809759 mm, agreeing with that
settled Rapier case within 0.001114%. The large beam-theory deficit is almost
entirely shared coarse Tet4 spatial stiffness error in this narrow case; solver
agreement and continuum accuracy are separate gates. Full 3D refinement remains
unconverged, and iteration diagnostics are still needed. See the checked-in JSON
reports, experiment README and [research index](analysis/RESEARCH-INDEX.md) for
parameters, historical evidence, sources and limits.

## Review of the initial section contributions

Three independent reviewers inspected PR #337 against `origin/feat/bevy-interface`:
correctness, security, and Rust/OCCT architecture. Specialized Bugbot and security
runners were unavailable; these were separate general review agents. Findings
below describe the reviewed commit `1dfc6b66`, not claimed completed repairs.

1. **P1: OCCT exception translation.** `section_mesh` in `crates/occt/src/shim.cpp`
   does not catch `Standard_Failure`; the generated CXX wrapper catches
   `std::exception` and is `noexcept`. An OCCT exception can terminate the process.
   Define a shared bridge exception policy, retain operation/body/plane context,
   and return a typed kernel-stage failure. Cover the reused projection entry too.
2. **P2: idle viewport asset churn.** `section_view::clear` mutates Bevy's `State`
   even when no cutaway exists. Its changed tick invalidates presentation/face
   overlay caches each frame. Read before mutating; prove idle synchronization
   preserves resource ticks and selected/hovered overlay asset handles.
3. **P2: tangency appears valid.** Real-kernel reproduction on
   `xtask/fixtures/switching/part-a.nbcad`: the 18x12x8 block at XY offset 0,
   below retention, probe 6 returns an 18 mm span and SVG but no cutaway triangles.
   Offset 8 with above retention behaves the same. The 3D inspector hides sources
   and displays nothing. Represent boundary-only/empty retention explicitly;
   reject these under the documented interior-section contract.
4. **P2: kernel failure appears empty.** The projection implementation silently
   continues on failed `BRepAlgoAPI_Section::IsDone`. Separate empty success from
   algorithm failure; inspect `HasErrors` and retain OCCT diagnostics.
5. **P2: limits apply after allocation.** Rust checks mesh and contour budgets
   after native meshing/sampling. Add limits before native output growth and
   OCCT progress cancellation. Those limits still do not bound all internal OCCT
   allocations: hard deadline/memory containment needs a separate worker process.
   Diagram mode currently asks for a cutaway too; avoid unnecessary meshing.
6. **P2: redundant native pipeline.** Inspection computes full hidden-line
   projection it discards, clips once there, then clips again for a cutaway.
   Introduce a focused native section query returning contours and an optional
   retained half-solid from one operation context. Persistent drawings keep their
   complete HLR pipeline. Share exact section primitives, not an entire exporter.
7. **P3: Rust ownership and typing.** Replace JSON construction of
   `DrawingViewDto` with its typed initializer; replace a one-shot
   `Arc<Mutex<Option<P>>>` handoff with a bounded channel/typed completion;
   deserialize owned query values and borrow hatch input instead of copying large
   geometry payloads. Keep `InvalidRequest`, `KernelStage`, `BudgetExceeded`,
   `AmbiguousContour`, `BoundaryOnly` and `Empty` distinct until the UI/MCP boundary.

The reviewed Linux CI failure was separate: OCCT's Font_FontMgr could not find
`fontconfig/fontconfig.h`. Upstream `03717f03` now provisions/fingerprints Linux
Fontconfig. Upstream `67905138` consumes preview JSON, removes projection copying
and reuses cached bounds; those allocation improvements need no duplicate repair.

PR #337 merged during the original review. The six findings were rechecked at
Bevy tip `e1b6f4f` and recorded in #339. During the fresh analytical pass,
upstream `74a8f8df` implemented their source repairs: shared CXX exception
translation, explicit boundary outcomes, focused section primitives, native
sampling/output limits and cooperative deadlines, transition-only Bevy mutation
and a typed bounded completion channel. Source/diff inspection confirms those
changes are present; their implementation work is not duplicated in this PR.

[The upstream qualification record](https://github.com/limo-cad/Limo-CAD/issues/339#issuecomment-6016455742)
reports 14 focused native/desktop/MCP cases and strict Clippy.
[Packaged acceptance recorded on 2026-10-11](https://github.com/limo-cad/Limo-CAD/issues/339#issuecomment-6106333435)
completed #339's targeted boundary/source-restoration and idle checks at matched
`a3145e2c`, with retained native desktop host evidence. #339 is closed. Section
inspection remains on the Bevy integration branch; PR #124 is still open, so
this acceptance does not establish delivery to main or complete application
qualification.
Cooperative cancellation still does not provide hard process memory/deadline
containment. This experiment does not claim to have independently rerun that
entire app suite or performed those packaged visual checks itself.

## Existing application capabilities to preserve

- `crates/assembly`: component/occurrence/joint ownership, forward and inverse
  kinematics, gear relations, named positions, multi-driver motor/keyframe studies,
  velocity/acceleration motion CSV. Reuse their IDs and joint intent.
- `crates/occt`: exact pair clearance, closest points and common/overlap volume;
  broad-phase sweep/prune; sampled motion interference and contact-stop bisection;
  BRep geometry, mass-property groundwork, face references, drawing sections and
  exports. Keep OCCT authoritative for source geometry and final geometric checks.
- `crates/cam`: bounded deterministic voxel stock removal, target comparison,
  gouge/collision reports, playback caches and cooperative cancellation. This is
  manufacturing simulation, not a dynamics world; retain it rather than duplicate it.
- Bevy: modeling and drawing presentation, retained controls, camera interaction,
  screenshots, worker receipts and owner/revision guards. Reuse presentation
  patterns after fixing the section lifecycle issues.

The useful new gaps are dynamic forces/inertia/friction, deformation/stress,
validated analysis meshing, persistent study definitions, result provenance and
engineering postprocessing. Another animation timeline is not the starting point.

## Rust candidates and responsibility

**Fenris 0.0.33 + fenris-solid 0.0.33 (MIT/Apache-2.0).** Generic finite-element
spaces, quadrature and sparse assembly; solid material operators include linear
elasticity, Neo-Hookean and St. Venant–Kirchhoff. Evaluate first for transparent
static engineering calculations. Fenris does not supply the entire solver, contact
algorithm, CAD-conforming mesher or study application. Its own README warns about
API stability/testing; we accept that and qualify exactly the paths used.
[Primary repository](https://github.com/InteractiveComputerGraphics/fenris).

**faer 0.24.4 (MIT), plus nalgebra as provided by Fenris.** faer supplies sparse
factorization/solves, with dense and eigenvalue building blocks; nalgebra fits
small local element tensors. Linear algebra alone is not structural analysis.
Evaluate factorization diagnostics, unsupported/singular models, residuals and
memory growth. No explicit matrix inversion in the solve path.
[Primary API](https://docs.rs/faer/0.24.4/faer/).

**conspire 0.8.0 (GPL-3.0).** Rust FEM, continuum/constitutive mechanics,
hyperelastic, viscous/plastic and thermal building blocks are present in the
published source. Evaluate material response/tangent consistency and nonlinear
benchmarks as a second structural implementation, especially polymers. Not added
to this harness yet. Its GPL license is a distribution/integration decision for
this LGPL application, not a reason to reject its technical experiment.
[Primary repository](https://github.com/mrbuche/conspire.rs).

**rapier3d-f64 0.36.0 (Apache-2.0).** Rigid bodies, colliders, joints/motors,
forces/contact and deterministic stepping suit a mechanism world. The current
release also has volumetric soft bodies, corotational/Neo-Hookean elastic cells
and a `fem` solver; test it rather than repeat outdated claims that Rapier only
supports rigid bodies. Defaults aimed at animation need explicit physical units,
material settings, mesh/timestep sweeps and equilibrium/result diagnostics.
[Primary soft-body guide](https://rapier.rs/docs/user_guides/rust/soft_bodies/).

**Parry 0.31.1 (Rapier's geometry dependency).** Collision proxies, queries and
new volumetric meshing are worth a dedicated geometry experiment. Test thin
walls, holes, disconnected components and face correspondence before using its
cells for structural calculations. OCCT tessellation is a surface mesh; it is not
automatically a conforming analysis volume mesh. Convex decomposition can fill
critical pockets, so compare proxies to the OCCT source at detent scale.
[Primary source](https://github.com/dimforge/parry).

Published `bevy_rapier3d` 0.36.0 depends on Bevy 0.19.0 and Rapier
`=0.35.0-glamx0.2`; this application's branch is Bevy 0.20.0. That is a real
compatibility mismatch, verified from the downloaded crate manifests. Use direct
Rapier behind a small adapter initially; investigate updating the official plugin
once its versions fit. Do not pull a second Bevy runtime into the app.
[Primary plugin repository](https://github.com/dimforge/bevy_rapier).

## What the commercial CAD systems teach us

Compare product families and licensed analysis modules, not every base CAD seat.

**CATIA + SIMULIA:** design-integrated part/assembly studies, associative meshing,
linear/nonlinear structural response, implicit/explicit dynamics, advanced contact,
and the wider durability/optimization portfolio. The transferable pattern is
associativity between design, study definition and solver evidence.
[SIMULIA structural portfolio](https://www.3ds.com/products/simulia/structural-simulation).

**NX + Simcenter 3D:** a shared pre/post environment spanning structures, dynamics,
thermal, motion, acoustics, durability and other disciplines; geometry changes
flow into analysis models. Motion loads can feed structural dynamics; correlation
to physical tests is an explicit part of the workflow. The transferable pattern is
one study/result platform with specialized solvers and reusable load mapping.
[Siemens capabilities](https://www.siemens.com/en-gb/products/simcenter/mechanical-simulation/simcenter-3d/),
[structural dynamics workflow](https://static.sw.cdn.siemens.com/siemens-disw-assets/public/2w2HxTC4C8JJ6ATdKOIOMH/en-US/simcenter-3d-for-structural-dynamics-simulation-sg-77918-d24.pdf).

**SOLIDWORKS Simulation:** embedded design workflow; Standard includes linear
static, fatigue and rigid motion; Professional adds frequency, buckling, thermal,
drop and other studies; Premium adds nonlinear/dynamic analysis. The transferable
pattern is a study tree with materials, fixtures, loads, mesh, solve and results,
plus clear limits on the analysis type being performed.
[Official capability/tier overview](https://www.solidworks.com/product/solidworks-simulation).

Common capability families are (1) associative preparation, (2) materials and
loads/constraints/connections, (3) mesh quality and convergence, (4) static and
modal response, (5) nonlinear/contact response, (6) mechanism dynamics, (7) thermal
and other coupled disciplines, (8) probes, deformed/undeformed plots, reactions,
history graphs and reports, and (9) parameter studies/optimization/test correlation.
Our first target is the common reliable workflow, not the breadth of all three suites.

## Architecture alongside Bevy and OCCT

1. **Study definition:** host-neutral Rust types for source body/occurrence IDs,
   geometry revision, material parameters/provenance, supports, loads, contacts,
   mesh policy and solver settings. Use newtypes for units/IDs at boundaries and
   explicit arrays of f64 in numerical loops. Convert mm to SI exactly once for
   Rapier; a structural adapter may use a documented consistent mm/N/MPa system.
2. **Geometry preparation:** resolve references against a frozen OCCT snapshot;
   export exact mass/inertia and source face identity; build separate collision,
   analysis and display meshes. Preserve face-to-element provenance for boundary
   conditions. Never assign fixture nodes by screen coordinates or unstable
   imported surface numbering. Reject unresolved references after edits.
3. **Simulation jobs:** owned immutable inputs sent to a bounded worker process.
   Typed messages/progress/errors; memory/time/mesh limits; cancel kills the owned
   job when cooperative solver cancellation cannot stop it. Never hold the native
   document mutex through the solve. Deterministic IDs/settings/seed/version hashes.
4. **Adapters:** `StructuralBackend` (Fenris/faer, conspire experiments),
   `DynamicsBackend` (Rapier), `GeometryVerifier` (OCCT). Keep mechanics intent
   independent of solver-specific handles. Explicitly map existing joints; reject
   unsupported gear/cam/closed-loop constraints rather than approximating silently.
5. **Results:** node/element fields, units, stress definition/location, reactions,
   contact histories, convergence/quality diagnostics and source/config hashes.
   Result status distinguishes completed, unconverged, invalid mesh, unsupported
   model, cancelled, failed and stale. A computed frame is not proof of equilibrium.
6. **Bevy presentation:** study panel/tree, fixture/load arrows, mesh/quality mode,
   undeformed ghost plus displacement overlay, stress legend/probes, history plots,
   section clipping and named report views. Orbit remains interactive. Display
   deformation scale prominently; result geometry never modifies source features.

## Experiments, ordered by useful gaps

**E0 — review hardening and reproducible qualification (first).** Resolve the
review blockers; add a focused section query and typed Rust results. Qualify the
isolated harness on beam elasticity, unsupported supports, motion timestep and
contact settling. Preserve measured failures. Current executable work is here.

**E1 — geometry and study contract.** One source part and one small assembly.
Verify mesh volume against OCCT, positive Jacobians/element quality, disconnected
parts and pockets, face-reference persistence, units and load/support coverage.
Add job snapshot/hash/cancellation/stale-result tests. Compare Parry volume mesh
to a checked mesh fixture; retain the existing Gmsh/CalculiX local experiment as
an independent numerical reference, not the application implementation strategy.

**E2 — world dynamics beside existing kinematics.** Reuse a jointed mechanism;
map fixed/revolute/prismatic joints, motors, density-derived inertia and gravity.
Bevy play/pause/single-step/reset at a fixed simulation timestep. Benchmarks:
free fall, pendulum period, joint constraint drift, force balance, friction ramp,
contact settling, CCD at detent scale and repeated deterministic runs. Validate
collision approximation with OCCT at saved key frames. Proposed gates: <1% period
error, documented drift/penetration below the mechanism's clearance budget,
and convergent timestep results. These are project acceptance criteria, not claims
that the current smoke test meets every one.

**E3 — linear structural study and useful results.** Fenris/faer first, conspire
as a comparison. Patch test, cantilever, rigid modes, missing supports, pressure
resultants, mesh convergence, global reactions/moments and energy. Initial gates:
free residual <1e-7, resultant/reaction balance within 1e-6 relative, <1% change
under last refinement and agreement with an independent 3D reference accounting
for boundary differences. Show displacement and reactions before promising peak
stress at singular clamp corners. Then add stress integration-point provenance,
averaging controls and mesh-adaptive probes. Modal/frequency is the next reusable
extension after reliable stiffness/mass assembly.

**E4 — detent mechanics and nonlinear/contact comparison.** Use the actual four
flex corners and installed housing pocket: inward flex perpendicular to the broad
clip face, slight residual preload, largely relieved installed deflection. Sweep
insertion depth, clearance/preload, friction, corner wall thickness and print
orientation. Compare Fenris/conspire nonlinear continuation to Rapier FEM; measure
force-displacement/hysteresis, residual energy, reaction balance, penetration and
mesh/timestep sensitivity. Separate global snap behavior from local stress.
Printed Bambu PETG Basic/PLA coupons supply directional stiffness, strength and
time-dependent response; the synthetic E=2000 MPa benchmark is not that data.
Measure insertion/retention loads and preload relaxation on real prints before
using the model to predict reliability. Fork/patch libraries when these tests
expose errors; experimental status is acceptable, unmeasured behavior is not.

**E5 — breadth after the mechanical workflow works.** Frequency/modal, buckling,
steady/transient thermal and thermal-stress studies; repeated load cases and
parameter studies. Fatigue/creep/lifetime, topology optimization, CFD and broader
multiphysics need their own validated models and are later work. Do not ship an
uncalibrated fatigue-life number because a general solver has a plasticity switch.

## Definition of a successful experiment

A reproducible Rust run, source/config/library hashes, quantitative benchmark
results, diagnosed failures and a small usable Bevy study workflow. Retain a
backend only when its measured strengths fit the task. The first decision is
between verified numerical paths and the integration work each requires; no
library is dismissed solely for describing itself as experimental.
