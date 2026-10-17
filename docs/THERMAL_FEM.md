# Thermal FEM kernel

`limo-cad-fem::solve_steady` solves steady, scalar, isotropic conduction
`-div(k grad T) = Q` using four-node linear tetrahedra. It consumes the shared
`IsotropicConduction` material models; conductivity, source provenance and unit
conversion remain in the existing material catalog.

Coordinates are metres, temperatures are absolute Kelvin, conductivity is
W/(m*K), volumetric sources are W/m^3 and integrated nodal heat inputs are W.
Prescribed temperatures are explicit node/value pairs. Unspecified boundaries
are insulating. Each connected component must have a temperature constraint or positive convection.
The caller supplies a conforming, nonoverlapping tetrahedral mesh; the kernel
checks indices, duplicate elements/nodes, degeneracy, finite coefficients and
anchors, but does not certify geometric conformity or detect all overlaps.
Zero-conductivity cells are rejected; use insulating boundaries for this kernel.

Element matrices are assembled sparsely. A diagonally preconditioned conjugate
gradient solve eliminates prescribed temperatures without changing the source
mesh. Iteration/node/element limits and a cancellation callback bound work.
Default limits are 4,096 nodes, 16,384 elements and 10,000 iterations; these are
work budgets, not claims of production-scale solver performance. The engine
returns no partial result on failure. It recomputes the free-node residual and
checks global heat balance before returning temperatures, element fluxes and
constraint heat reactions. Positive reaction means heat supplied to the domain;
the reaction sum plus volumetric/nodal power plus net surface heat is zero
within numerical tolerance.

The formulation follows the usual diffusion weak form described in the primary
[MFEM finite-element tutorial](https://mfem.org/tutorial/fem/). Analytical
manufactured-solution verification follows the approach in the
[FEniCS tutorial](https://pub.fenicsproject.org/tutorial/sphinx1/index.html).
The implementation is original Rust; it does not embed either external solver.

Tests cover affine temperature and flux, a slab with insulated sides, a
piecewise-conductivity interface, nodal power/reaction balance, coordinate
scaling and element winding, a quadratic manufactured solution under three
refinements, cancellation, exhausted budgets, invalid loads and unanchored or
degenerate meshes. For the quadratic fixture, halving spacing gives roughly a
fourfold reduction in element-centroid RMS temperature error. This metric is
not a mesh-wide integrated error norm or a general convergence guarantee.

This is an analysis kernel, not the completed FEM product. Native study
persistence/editing, CAD meshing and stable boundary association, task/result
provenance and stale-result integration, field visualization, face-based
radiation and spatially varying surface conditions, transient/anisotropic/nonlinear physics,
mechanical solving and larger production benchmarks remain to be implemented
and qualified. No native UI or AI/MCP capability is implied by kernel tests.


## Surface flux and convection

The extended entry point, solve_steady_with_surfaces, accepts one ThermalSurface
record per exterior mesh triangle. Positive heat flux enters the domain,
independent of node winding. A constant convective coefficient h (W/m^2/K) and
ambient temperature (K) add the consistent linear-triangle boundary matrix and
ambient load. Flux and convection can coexist in one record; duplicate records
for the same triangle are rejected. Nodes with prescribed temperature may also
belong to a loaded surface; their reaction accounts for that load.

The surface must belong to exactly one tetrahedron. Interior, nonexistent,
duplicate and nonmanifold faces reject; this is still not a general geometric
overlap/conformity check. A zero convection coefficient adds no anchor.
Unspecified faces retain zero normal flux. The result includes net surface heat
in watts in the same order as the input records. That heat includes both the
prescribed flux and h*(ambient - mean face temperature), so global balance does
not mistake the ambient forcing term alone for the physical convective heat.

The consistent triangle matrix is h*A/12 times a matrix with diagonal 2 and
off-diagonal 1; constant flux and ambient loads use A/3 per node. The weak-form
sign convention is documented in the primary [FEniCSx mixed-boundary tutorial](https://jsdokken.com/dolfinx-tutorial/chapter3/robin_neumann_dirichlet.html).
Tests verify slab resistance, convection-only anchoring, combined flux/convection,
winding independence and a nonuniform-temperature triangle matrix.

Residual tolerances measure equation imbalance in watts; they do not bound
absolute temperature error in an ill-conditioned problem. Scaling, conditioning
and production-range qualification remain necessary, especially with very weak
convection or extreme conductivity contrasts. No adaptive accuracy guarantee is
claimed. Positive coefficients that underflow during boundary assembly reject.
