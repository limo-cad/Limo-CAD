# Material constitutive response

The shared material catalog now supports explicit constant isotropic elasticity
and scalar Fourier conduction through `MaterialDetails::isotropic_elasticity`
and `MaterialDetails::isotropic_conduction`. These evaluate local material laws;
they do not mesh geometry, assemble equations, solve a study or qualify a part.

Each model resolves existing named properties using optional exact context and
source filters. Missing, ambiguous, nonnumeric or dimensionally incompatible
inputs reject. The resulting models borrow the original property records, so
units, provenance and context remain inspectable. They neither persist derived
constants nor create a separate analysis material catalog. Selecting the model
is explicit: kind, alloy name or a successful computation does not establish
isotropy, linearity, temperature independence or suitability for a load regime.

Elasticity requires positive Young's modulus and a Poisson ratio strictly between
-1 and 0.5. It admits auxetic values and rejects singular or nonfinite derived
moduli. Stress uses SI Pa and engineering strain ordered xx, yy, zz, yz, xz, xy;
the last three strain components are engineering shear, twice tensor shear.
There is no implicit plane-stress reduction, finite-strain update or thermal
expansion. See the primary [MOOSE constitutive model documentation](https://mooseframework.inl.gov/source/materials/ComputeIsotropicElasticityTensor.html)
and its [parameter checks](https://mooseframework.inl.gov/docs/doxygen/modules/ComputeIsotropicElasticityTensor_8C_source.html).

Conduction accepts nonnegative scalar conductivity in W/(m*K); zero represents
an ideal insulator. Temperature gradients in K/m produce heat flux in W/m^2
with the opposite direction. No absolute-temperature offset belongs in that
gradient. See the [MOOSE heat-conduction formulation](https://mooseframework.inl.gov/source/kernels/HeatConduction.html).

Validation covers axial free contraction, hydrostatic response, engineering
shear, a rotated strain tensor, positive energy for auxetic and nearly
incompressible values, slab heat flux, insulation, equivalent units, saved
material round-trip, provenance, ambiguity, invalid inputs and overflow.
An export-catalog integration test consumes the actual Aluminum 6061-T6 record
without copying or rewriting its properties. This is mathematical and catalog
integration evidence, not solver convergence or native analysis UI qualification.
