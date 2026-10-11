# Fresh analytical iteration and adversarial review

2026-10-06, dedicated simulation PR 338. Numerical producer, independent mechanics
reviewer and independent adversarial reviewer had separate tasks. General agents
performed these reviews; no specialized Bugbot/security runner is claimed.

The source status was refreshed too: upstream Bevy commit `74a8f8df` has landed
repairs for the previously reported section findings. The project plan now records
that work and the hosted/platform and packaged visual qualification then pending
for #339, rather than treating the old e1b6f4f findings as unchanged current source.
[Targeted packaged acceptance on 2026-10-11](https://github.com/limo-cad/Limo-CAD/issues/339#issuecomment-6106333435)
subsequently closed #339; the Bevy integration into main remains in open PR #124.

## New numerical evidence

The original comparison mixed quadratic Hex27 static elasticity with coarse
linear Tet4 transient elasticity. The exact-topology experiment now isolates
the discretization. For the settled low-load coarse beam, Fenris gives
0.1310809759 mm and Rapier 0.1310795160 mm, approximately 0.001114% difference.
That is a narrow agreement result, not continuum accuracy. The full 3D Tet4 sweep
remains unconverged and differs from refined Hex27 by approximately 6.23%.

The independent reviewer checked the metres-to-mm conversion, modulus/force
units, rest topology, root supports, consistent traction and work-conjugate
area-mean displacement calculation. No additional blocking setup defect was
found. The 250-iteration trajectory failures remain recorded; the new settled
comparison does not erase those outcomes.

One concrete review defect was fixed before publication: an initial version
compared against a frozen displacement constant. The comparison now consumes the
actual supplied world report, validates schema/library/units/material/load,
requires a unique completed matching grid/timestep/iteration case and independently
checks positive finite displacement plus stationarity. The output includes the
matched numerical row. CI runs the comparison after the world step using that
runner's output, rather than using the checked-in baseline.

Adversarial executable checks rejected all of these inputs with exit status 1:

- Changed world-report units.
- Duplicate matching world cases.
- A comparison case marked nonstationary.
- A 20% altered displacement that fails the live 2% agreement gate.

Formatting and strict Clippy across all experiment targets pass. The new static
probe also passed using the archived **actual Linux world report** as input on
Windows. This checks input/result agreement across that evidence pair; it does
not claim the new probe already ran on Linux. The original hosted workflow passed
at e8b733cf; new-head CI is tracked separately.

## Evidence completeness

The old Gmsh/CalculiX synthetic beam script, STEP, JSON, solver logs, input decks
and node/reaction tables are now tracked with SHA-256 manifests. The independent
reviewer explicitly distinguished its equal-node force/arithmetic mean from the
Rust consistent traction/area mean, so cross-method closeness is corroboration
rather than an identical-BC validation gate.

Private part research was inventoried separately: original PETG FEM and slicer
studies, native CAD sections/drawings, clip meshing, source inputs and images.
Large regenerable fields/G-code/3MF remain locally inventoried by path/size/hash;
third-party binaries and dependency environments are excluded. See the
[research index](RESEARCH-INDEX.md) for the private repository evidence locations.

## New mechanics experiment and safeguards

The private fresh pass challenged the old all-corners-at-once load interpretation
and computed four independent load cases. A work-conjugate compliance matrix,
reciprocity, positivity and equilibrium distinguish genuine local flexibility
from an assumed four-spring model. The fresh review also required true load-patch
centroids/areas instead of labeling design reference centers as contact points,
absolute coupling measures, geometric mesh-quality gates, independent combined-load
checks and reaction/moment/energy checks.

These are support-sensitivity screening models. Bilateral floor constraints do
not bound actual unilateral contact, and a linear target-displacement solution
does not establish installed contact forces. The part's fresh report retains
the material-version issue, dimensional-axis distinction, tolerance/relaxation
window and relief-junction continuity concerns. No CAD geometry was changed.

The finalized private study ran three mesh levels with both floor restraints.
Every run passed the strengthened gates. Maximum absolute inter-patch coupling
was 0.124% in the finest normal-floor case; maximum final-refinement illustrative
force changes were below 1.9%. This supports weak coupling under those restraints
but does not validate the real assembly contact. Private scripts and final JSON
are committed at `3a7eb797` in the part repository, with the fresh mechanics report.
