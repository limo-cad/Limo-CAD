# Thermal FEM kernel

`limo-cad-fem::solve_steady` solves steady, scalar, isotropic conduction
`-div(k grad T) = Q` using four-node linear tetrahedra. It consumes the shared
`IsotropicConduction` material models; conductivity, source provenance and unit
conversion remain in the existing material catalog.

Coordinates are metres, temperatures are absolute Kelvin, conductivity is
W/(m*K), volumetric sources are W/m^3 and integrated nodal heat inputs are W.
Prescribed temperatures are explicit node/value pairs. Unspecified boundaries
are insulating. Each connected component must have a temperature constraint.
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
the reaction sum plus total applied power is zero within numerical tolerance.

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
flux/convection/radiation conditions, transient/anisotropic/nonlinear physics,
mechanical solving and larger production benchmarks remain to be implemented
and qualified. No native UI or AI/MCP capability is implied by kernel tests.
