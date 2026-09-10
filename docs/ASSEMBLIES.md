# Assemblies, components, and joints

## Production boundary

Part design and assembly placement are separate persisted model layers.

- `nbcad-solid` owns sketches, feature history, source bodies, and stable OCCT
  face/edge topology.
- `nbcad-assembly` owns reusable component definitions, occurrence hierarchy,
  local coordinate systems, grounding, joint definitions, and kinematics.
- OCCT remains the source of exact connector topology. Assembly transforms do
  not rewrite, duplicate, or recompute the part feature history.
- Bevy and the browser development renderer consume the same solved occurrence
  poses for display, picking, highlighting, and animation. Neither renderer is
  an assembly authority.

This boundary keeps the model deterministic in native desktop, browser
development, CAM, file export, and headless tests.

## Persisted component structure

`AssemblyDocumentDto.component_structure` contains two intentionally different
records:

- A **component definition** owns a stable set of source `BodyId` values and a
  component-local coordinate system. A definition may contain one body,
  multiple rigid bodies, or no direct bodies when it is used as a subassembly
  container.
- A **component occurrence** references one definition and owns only assembly
  state: its parent occurrence, parent-local pose, visibility, name, and
  grounding state.

One definition may therefore have any number of occurrences without copying
OCCT geometry or part history. The same multi-body definition moves rigidly as
one occurrence. Occurrences form a validated acyclic tree, so each child pose is
expressed in its parent subassembly coordinate system and nested transforms are
composed recursively.

The final transform for a source body is:

```text
occurrence world pose × inverse(component local coordinate system)
```

Changing an occurrence placement moves only that instance. Changing the local
coordinate system changes the shared definition origin used by every instance;
it still does not edit the source body or its features.

The Assembly panel supports:

- grouping selected source bodies into one authored multi-body component;
- creating an empty subassembly container;
- adding a reusable definition at the document root or under a selected
  occurrence;
- reparenting and renaming occurrences;
- editing parent-local occurrence placement and the shared component origin;
- grounding one occurrence independently within each sibling group;
- hiding an occurrence without hiding the source body definition;
- duplicating a complete occurrence subtree, including internal joints, while
  continuing to share its underlying component definitions.

Subassembly membership is explicit occurrence structure. Duplicating a subtree
creates independent placement nodes (and cloned internal joint intent), so a
later placement edit in one copy does not move the other copy.

## Compatibility and identity

Older projects without component data migrate deterministically: every live
body receives one promoted one-body definition and one root occurrence. The
migration is idempotent and additive to the existing project schema.

Every occurrence and component has a stable persisted id. Joints bind their two
exact topology connectors to exact occurrence ids, not merely to source body
ids. This disambiguates two instances of the same source part and prevents an
edit from silently retargeting a different instance.

For a picked planar attachment point, supply `source_surface_frame` with the
selected face's `plane.origin`, `plane.normal`, and `plane.u`, and put the
attachment origin and axes in `frame`. The connector preserves that local
offset through recomputation and save/load, as cylindrical connectors do.
Omitting `source_surface_frame` retains the legacy face-centroid behavior.

Renderers retain one tessellation per source body and instance it for every
visible occurrence. Native and browser picking transform the ray into each
instance and return the exact occurrence id, so selection and hover do not
collapse reused parts onto the first instance.

## Joint completeness

Assembly is a Solid Modeling sub-function. Bodies that have not been organized
explicitly are promoted automatically, preserving the simple single-part flow
while still giving every moving item a component identity. The Browser and
Assembly panel can explicitly fix one occurrence in each assembly level.

The production joint set accepts exact connectors on different components:

- rigid;
- revolute and slider;
- cylindrical (independent slide + rotate);
- planar (two in-plane translations plus rotation);
- ball and universal;
- pin-slot;
- screw (rotation coupled to axial travel by pitch).

Every joint kind accepts the same connector sources:

- planar faces;
- cylindrical faces and their virtual circular openings;
- closed circular OCCT edges, including both rims of a chamfer or countersink.

Face-backed connectors store stable `BodyId`, `FaceId`, and topology-key values.
Circular-edge connectors store stable `BodyId`, `EdgeId`, and edge topology-key
values. Every connector also stores a model-space origin, orthogonal
primary/secondary axes, and a radius when the topology is circular. Per-side
twist values define an editable zero orientation without weakening topology
identity.

Creation validates the live solid scene and rejects missing, incompatible, or
retargeted topology. Saved projects structurally validate joint ids, counters,
frames, offsets, and limits. An older project without an `assembly` member opens
with an empty assembly document because the field is additive to schema v2.

## Driving a mechanism and geared rotation

`assembly_set_joint_motion` prescribes the selected joint's primary coordinates
and solves its passive coordinates with the same damped least-squares closure
solver used by component dragging. A frame/screw/retained-jaw/slider loop can
therefore be driven by its screw alone. The screw field
`advanced.screw_pitch_mm_per_revolution` is axial travel per full revolution
(the lead). Failed closure or a dependent coordinate outside its limits rejects
the whole edit; previews and motion-study samples remain transient. Inactive
history intent is retained for redo.

`assembly_create_gear_relation`, `assembly_update_gear_relation`, and
`assembly_delete_gear_relation` share the Assembly / Joints interface group.
A relation links two revolute coordinates in one assembly level:

```text
angle_b = phase_deg + (reverse ? -1 : 1) * teeth_a / teeth_b * angle_a
```

Angles stay unwrapped through multiple turns. `reverse: true` describes an
external pair; `false` describes equal-direction coupling. For an 80-tooth
wheel driving a 20-tooth pinion, a +90 degree input gives a -360 degree output
when phase is zero. Drive either endpoint and the other follows. Connector
frames establish angular zero, so the model's tooth placement determines the
required phase. The relation records kinematics; it does not generate teeth,
prove meshing or contact clearance, or calculate torque and load capacity.

Relations persist in `assembly.gear_relations`. Deleting a joint deletes its
relations; duplicating a mechanism subtree clones its internal relations with
independent ids. Editing a related joint to a non-revolute kind is rejected.
Project schema 4 prevents older readers from silently dropping this coupling;
older projects migrate with an empty relation list.

## Solved behavior

1. one solved pose per occurrence, recursively composed through its parent
   subassembly, plus a compatibility pose per live `BodyId`;
2. connector re-resolution and explicit broken-reference diagnostics after
   model recompute;
3. a deterministic constraint graph propagates rigid poses from the grounded
   body through every supported joint kind;
4. closed loops are solved as one connected mechanism: damped least-squares
   optimization enforces every closure constraint instead of silently choosing
   one propagation path, and disconnected components are identified as free;
5. Bevy and the browser renderer apply the same occurrence-specific GPU pose to
   model pixels, highlights, orbit framing, and ray picking;
6. motion values and optional limits persist in the project;
7. a new non-rigid joint gets a short, non-persistent motion demonstration;
8. every free coordinate can be driven independently from the Assembly panel;
9. direct single-joint dragging remains available for revolute, slider, and
   cylindrical joints;
10. dragging any movable component with no direct manipulator selected runs a
    damped least-squares inverse-kinematics solve across the complete grounded
    joint path;
11. component drag constrains position by default and allows the mechanism to
    rotate naturally; callers can opt into a full target orientation;
12. every transient solve respects per-coordinate limits and reports explicit
    unreachable or over-constrained diagnostics;
13. **Save position** records a named, reusable coordinate set without rewriting
    joint defaults; applying a saved position is an explicit model edit;
14. motion studies evaluate the complete multi-joint mechanism at a deterministic
    timeline time and remain transient until a named position is applied.

The solver re-resolves every connector against its exact live body plus face or
edge identity, topology key, and occurrence binding. Geometry is never
retargeted by ordinal position. Editing a joint may explicitly repair a stale
connector, but saving the edit re-canonicalizes the new topology and rejects a
silent ordinal or instance replacement. Joints can also be
suppressed/unsuppressed without deletion. The viewport only translates a
pointer gesture into a candidate joint coordinate or occurrence pose; the
host-neutral assembly solver remains authoritative for the displayed pose.

## Named positions and motion studies

Named positions store all joint coordinates by stable `JointId`. They are
independent records: capturing a second position never overwrites the first,
and deleting a joint removes only that joint's stale coordinate from every
position. Positions round-trip in the project file and can be renamed, applied,
or deleted from the Motion panel.

A motion study owns a duration, playback speed, loop preference, and any number
of coordinate drivers. One driver targets one free coordinate of one joint and
uses either:

- ordered keyframes with step, linear, or smooth interpolation; or
- a motor law with initial value, speed, and constant acceleration.

Each timeline sample evaluates all enabled drivers together, clamps the result
to joint limits, runs the same host-neutral multi-joint solver as interactive
dragging, and reports the effective value, speed, and acceleration for every
driver. Timeline scrubbing and playback only create a viewport preview; part
feature history and saved assembly placement remain unchanged.

Motion paths export to CSV at a requested sample rate. Each occurrence row
contains the time, world translation, world quaternion, linear velocity and
acceleration, and angular velocity and acceleration. This makes the path useful
for CAM handoff, external analysis, or regression comparison without depending
on Bevy or the DOM.

A later production slice will add reusable, pre-designed motion scripts and
direct video capture/export of simulated motion playback. Both will consume the
same deterministic motion-study timeline so authored demonstrations, recorded
video, and exported paths stay synchronized.

## Interference, clearance, and contact stops

Native desktop checks use the retained OCCT B-reps transformed by the solved
occurrence poses. A static check reports exact minimum distance, closest points,
and Boolean-common overlap volume for every requested occurrence/body pair.
The browser development path exposes the same contract with a clearly marked
transformed-bounds fallback; it is suitable for UI development but is not
presented as exact geometry.

Swept collision evaluates the same exact native pair query at deterministic
timeline samples. The requested sample rate is persisted in the report so a
reviewer can judge temporal resolution; it is not represented as a continuous
closed-form swept solid. Reports include first and last collision times, minimum
clearance, and maximum overlap for every colliding pair. Interactive playback
evaluates only enabled contact pairs, rejects distant pairs with transformed
bounds, and probes each frame interval before refining the first physical stop;
the all-body exact report remains an explicit Inspect operation.

A contact set binds two placed source bodies by stable occurrence and body ids,
plus a nonnegative clearance. When **stop motion** is enabled, playback detects
a clear-to-contact transition, refines the first hit by bisection, and clamps
the whole deterministic mechanism to that time. This is a unilateral physical
stop for motion studies, not a force, friction, bounce, or dynamics simulation.
Contact sets are named, persisted, suppressible, editable, and removable.

## Production hardening

The desktop host retains the last assembly solution and invalidates it only
when solid geometry or assembly intent changes. Interactive inverse kinematics
limits its variables to the connected mechanism being dragged, so unrelated
occurrences do not become optimization variables. The native viewport relies
on Bevy's mesh frustum culling and additionally suppresses default edge
overlays for off-screen or sub-pixel occurrences; selected, hovered, and
explicitly highlighted geometry always remains visible.

Interference and clearance checks use a conservative sweep-and-prune broad
phase over transformed occurrence bounds before invoking exact OCCT distance
and Boolean-common operations. Candidate rejection cannot hide an overlap or a
pair inside the requested clearance, while avoiding quadratic exact tests for
distant parts.

STEP export can consume the solved visible occurrence set. Each exported root
is the exact retained OCCT body transformed by its occurrence world pose, so
reused and nested instances arrive at their assembled locations for downstream
CAM or inspection. The current exchange is flattened placed geometry; an
AP242/XCAF hierarchy with occurrence names, product metadata, and BOM identity
is a later interoperability layer rather than a prerequisite for exact
assembled geometry.

Assembly intent is persisted with the project, including definitions,
occurrences, grounding, joint topology references, named positions, motion
studies, and contacts. Joints are also represented in the design-history strip
as editable, suppressible, deletable assembly steps after the part feature
history. Explicit deletion of a feature-owned body removes joints that refer to
that body only after the OCCT edit succeeds; a failed edit or temporary history
rollback does not destroy assembly intent.

## Deliberately not implemented yet

This is deterministic rigid-body kinematics, not a physics engine. Flexible
bodies, force/torque integration, mass and inertia, friction, bounce, compliant
contact, gears/cams, continuous analytic swept volumes, associative external
component-library links, AP242/XCAF product hierarchy, and direct CAM operation
generation remain later slices. The pose representation, exact placed STEP
geometry, and exported motion paths are host-neutral so those systems can
extend them without moving assembly authority into rendering code.
