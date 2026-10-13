# Simulation research and evidence index

Updated 2026-10-11. This index preserves successful experiments, negative results,
review findings, rejected interpretations, sources and remaining qualification
gaps. Evidence for the private part stays in its private repository; no part STEP,
STL or full source model is copied into this public application repository.

## Application and library research

- [Knowledge preservation and copying/reference audit](SESSION-KNOWLEDGE-AUDIT.md):
  which session lessons are unique and reusable, searchable Help promotion,
  artifact-specific license evidence, source priorities and remaining rights gaps.

- [Simulation decision and phased project plan](../SIMULATION-EXPERIMENT.md):
  existing capability audit, OCCT/Bevy integration boundaries, Rust library and
  license evaluation, vendor capability research, independent adversarial review,
  failure semantics and qualification gates. Sources are linked at the claims.
- [Rust experiment methods and results](../../experiments/rust-simulation/README.md):
  pinned independent Cargo workspace; Fenris/fenris-solid/faer static elasticity;
  Rapier rigid motion and soft-body timestep/iteration diagnostics. Source code,
  lockfile and original Windows JSON reports are tracked beside the README.
- [Fresh same-topology probe](../../experiments/rust-simulation/src/bin/same_mesh.rs)
  and [its report](../../experiments/rust-simulation/same-mesh-results.json):
  identical Rapier Tet4 rest mesh, supports and consistent tip loads in a Fenris
  static solve; independent refinement in X and across the section; current-run
  Rapier comparison. Equilibrium/reactions/energy and same-mesh agreement are
  checked separately from continuum accuracy.
- [Hosted Linux evidence](../../experiments/rust-simulation/evidence/linux-37461574608/README.md):
  actual downloaded runner JSON and hash manifest, compared with Windows.
- [Historical Gmsh/CalculiX reference](../../experiments/reference-structural/README.md):
  exact original Python script, synthetic STEP, result JSON, solver logs and
  hashes. Its load integration differs from the Rust benchmark; no app dependency
  or current external-solver recommendation is inferred.

## Review record and open work

- [PR 337 initial review](https://github.com/limo-cad/Limo-CAD/pull/337#issuecomment-6015409330)
  records correctness, security and Rust/OCCT reviewers. Specialized review
  runners were unavailable; independent general agents performed those reviews.
- [Section hardening #339](https://github.com/limo-cad/Limo-CAD/issues/339)
  tracks the kernel exception, empty/tangent outcome, native budgeting, pipeline
  and Bevy lifecycle findings after PR 337 merged. Upstream `74a8f8df` implemented
  their source fixes. [Targeted packaged acceptance on 2026-10-11](https://github.com/limo-cad/Limo-CAD/issues/339#issuecomment-6106333435)
  completed the boundary/source-restoration and idle checks; #339 is closed.
  Broad application qualification is separate.
- [Structural analysis #336](https://github.com/limo-cad/Limo-CAD/issues/336)
  holds the original external-worker experiment, the Rust experiment and follow-up
  design. Neither experiment installs a structural solver into the CAD app.
- [Dedicated experiment PR 338](https://github.com/limo-cad/Limo-CAD/pull/338)
  holds the development/review history and current hosted qualification.

## Private design research

The [INJS2065 part repository](https://github.com/jeffglousher/INJS2065-Technic-Case)
contains `review/research-20261006/README.md` and
`review/analysis/DETENT-MECHANICS-FRESH-PASS.md`. Its research archive includes:

Committed private evidence:
[archive and fresh mechanics at 3a7eb797](https://github.com/jeffglousher/INJS2065-Technic-Case/tree/3a7eb79763dbf19b5ce109530f35f957d6c87175/review/research-20261006),
[fresh mechanics report](https://github.com/jeffglousher/INJS2065-Technic-Case/blob/3a7eb79763dbf19b5ce109530f35f957d6c87175/review/analysis/DETENT-MECHANICS-FRESH-PASS.md).

- Original PETG linear-FEM scripts, four mesh-level reports, material assumptions,
  field summaries, relief-junction measurements, diagram and exact STEP input.
- Original clip high-order volume-meshing script/report and STEP input, including
  invalid elements before optimization and low residual quality afterward.
- Earlier slicer challenge scripts, resolved settings, all case/metric reports,
  recommendations and actual toolpath images; newer local-corner-flex evidence
  was already committed under `review/local-corner-flex/`.
- Native section queries/SVGs and Bevy main-CAD/drawing screenshots, with the
  model left unchanged.
- Fresh mechanics interpretation and independently loaded corner compliance
  investigation. Limitations are retained alongside the numerical conclusions.

Each original evidence bundle carries an exact-byte SHA-256 manifest. Large
regenerable solver fields, meshes, sliced 3MF/G-code and third-party binaries stay
outside Git with their location/hash inventory in the private archive. This is
not a claim that all local binary artifacts or historical application experiments
have been imported into this PR.

## Fresh conclusions that change the next experiment

The coarse Tet4 static answer is 0.1310809759 mm versus settled Rapier
0.1310795160 mm: about 0.001114% difference. Thus the earlier 50.85% deficit from
beam theory is almost entirely shared spatial discretization error for this
particular low-load beam; it is not evidence of a stationary Rapier modulus/unit
defect. The 250-iteration nonstationarity remains a separate observed issue.
The finest Tet4 result is still 7.60% below the beam screen, so no converged Tet4
force/stress prediction is claimed. This does not uniquely identify shear locking.

For the detent, define geometry-driven installed travel and the actual contact
direction before assigning a preload force. A coupled full-wall plate/frame
cannot be reduced to four independent free cantilevers without verification.
Printed material version, test conditions, local strain convergence and creep
remain explicit gaps. Manufacturer break elongation is not a repeated-snap strain
allowable. These generic lessons drive the study/input/provenance requirements;
the detailed part-specific calculations remain private.

The fresh private four-load-case experiment found weak inter-corner coupling
under its selected floor supports, with a maximum absolute row coupling ratio
of 0.124%. Six support/mesh studies passed positive quality, geometric volume,
equilibrium/reactions/moments, energy, reciprocity and independent combined-load
checks; final illustrative force changes were below 1.9%. This is useful
support-specific confirmation, not proof of actual installed contact or preload.
