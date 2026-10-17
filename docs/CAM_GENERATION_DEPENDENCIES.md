# CAM generation dependencies

Updated 2026-09-11. Planner revision 9; ordering-rule revision 1.

## Generation and sequence verification are different

Reordering is manufacturing intent, but an operation is not stale merely
because its row index or immediate predecessor changed. The Rust rules in
`crates/cam/src/dependencies.rs` identify the earlier material-removal evidence
each strategy consumes. The host compares those inputs with the last successful
generation, without generating paths or voxelizing stock for this check.

| Edit | Generation consequence | Sequence consequence |
|---|---|---|
| Move an independent drill after ordinary contour/roughing paths, keeping their facing predecessor | All affected paths retain generation | Program, stock stages and NC use the new order |
| Move/suppress a drilling source after a path with a predrilled entry | The predrill consumer becomes stale | Regeneration fails unless suitable earlier hole clearance exists |
| Move/suppress/edit an earlier facing operation | Its incoming-stock-height consumers become stale | Depth/engagement and rapid-entry assumptions must be rechecked |
| Edit a path, its tool, referenced CAD, setup or WCS | The corresponding input fingerprint changes | Existing posting gates remain active |
| Change a source setup for rest stock | Its downstream rest-setup fingerprint changes | Rest replay/verification uses the new source sequence |

The browser distinguishes changed own settings, earlier facing evidence,
required predrill evidence and upstream rest stock. Reordering never
automatically regenerates or stamps a path as newly generated.

## Rules

For operation `i`, `before(i)` contains the enabled operations earlier in the
same setup. The planner currently has two intra-setup evidence types:

- **Incoming stock height:** every Face candidate in `before(i)`. The planner
  lowers its conservative billet top only after proving whole-stock facing.
  Every current strategy reads that top for depths, engagement/flute reach or
  rapid-entry safety. Partial Face candidates remain conservatively included:
  a cutter/bounds edit can make one start or stop providing coverage.
- **Predrilled entry:** earlier Drill/ChipBreaking/DeepHole operations, only
  when the consumer uses their cylindrical-clearance evidence. A contour with
  selected predrill positions consumes it even with Ramp off. High Speed
  Roughing consumes it when its entry mode is Predrill. Roughing's seed scoring
  reads the entire earlier-hole set, so this version does not narrow the
  dependency set to only the selected hole centers.

Producer sets are sorted by stable operation ID. Each producer's operation,
tool, associative heights and linking intent are fingerprinted. Producer order
within an unchanged set is not evidence: facing's minimum height and the set of
drilled cylinders are order-independent. Each producer retains its own gate.

Conceptually, each generation key is:

```
own_i   = H(operation_i, height_intent_i, linking_i)
face_i  = H(rule_revision, sorted(facing producer inputs before i))
holes_i = H(rule_revision, sorted(predrill producer inputs before i))
key_i   = (planner_revision, model, setup, own_i, tool_i, rest_sources,
           rule_revision, face_i, holes_i)
```

Reuse requires equality of every component. Unknown rule revisions are stale;
malformed fingerprints lose their generation stamp. The strategy-policy match
is exhaustive. A new strategy or stock-aware linking behavior must declare any
additional earlier facts it reads and add the corresponding rule and tests.

## Tool edits and invalid paths

Compatibility and freshness are separate. Before comparing generation
fingerprints, Rust validates each enabled operation against its assigned
project tool. Unsupported kinds, missing plunge capability and tool-dependent
parameter mismatches produce an **Invalid** state with the validation reason.
The browser shows an Invalid badge and repair instructions, not just a
regeneration suggestion. Compatible changes still use the normal Stale state.

The edit transport validates IDs, geometry, finite/positive parameters and
tool definitions strictly, but preserves operation/tool mismatches for repair.
This lets a valid project-library edit commit without silently substituting
a different cutter, changing operation parameters, suppressing operations,
or rewriting successful-generation stamps. Invalid assignments remain enabled
and Invalid after save/reopen. Malformed legacy operation records retain the
existing load-time parking behavior; they are not confused with a tool edit.

Execution remains strict: regeneration, planning, stock simulation and NC
posting reject invalid consumers. Earlier valid operation previews are scoped
before the invalid operation; whole-setup output never skips it. Repairing
the assignment resumes ordinary fingerprint checking, and only successful
explicit regeneration writes new stamps. This is a configuration check, not
a replacement for geometric clearance or ordered-stock verification.

## Lossless document transport

An operation edit sends the complete CAM document back to Rust. Roughing
geometry captures model-mesh `f32` coordinates as `f64`; the default JSON
reader could move one of those coordinates by one ULP on a round trip.
For example, `0.013000000268220901` decoded to the adjacent floating-point
value. The earlier path's own-geometry fingerprint then changed even though
only a later operation was edited.

The shared CAM dependency enables `serde_json/float_roundtrip` so native,
browser and project reads preserve these values. We do not loosen fingerprint
tolerances or discard geometry evidence to hide drift. Regression tests cover
19,999 mesh coordinates and an actual later-operation edit/regeneration after
JSON transport. A browser test also edits a contour after roughing and verifies
the earlier captured geometry and generation records remain identical.

Records already stale from earlier numerical drift are not automatically
certified: regenerate them once with the corrected build. Actual model, tool,
setup and consumed-predecessor changes retain their normal regeneration gates.

## Safety and compatibility

This engine answers **“does generation depend on the changed order?”**, not
**“is this machining sequence safe?”** Ordered stock simulation and production
post clearance/target gates remain separate and mandatory. For example, moving
a bore after thread milling may leave the thread path unchanged, but NC export
still rejects its uncleared tool entry.

Setup-plan and stock caches retain order-sensitive identities. Tool changes,
connecting motion, progress and intermediate stock cannot reuse a previous
sequence just because individual generation stamps remain current. Rest-source
fingerprints remain conservative over the whole source sequence.

Old projects without `order_dependencies` retain the previous full-prefix
comparison. On a CAM edit, an old stamp is translated only if it exactly
matches the **pre-edit** model/setup/tool/operation/prefix and current planner
revision. Translation does not plan, resolve geometry or certify changed
inputs. Those inputs are compared against the translated old evidence normally.
Already-stale legacy records still require explicit regeneration.

## Regression coverage

- Independent drilling moved last: unchanged generation stamps and contour
  motion, current statuses, NC assembly and save/load retention.
- Independent drill tool edits/suppression leave unrelated milling current.
- Facing moved behind consumers gives a specific stock-height reason.
- Selected contour predrill with Ramp off remains order-dependent and refuses
  generation after its drill moves later.
- Verified old stamps upgrade before reorder; stale legacy stamps remain stale.
  An edit made together with a reorder is compared against pre-edit evidence,
  not silently certified by migration. Unknown/malformed rules cannot be current.
- Unchanged thread generation cannot bypass actual pre-entry verification.
- Browser reordering moves drilling past contour and roughing without
  regeneration; roughing moved ahead of facing becomes stale.
- Later-operation edits and regeneration preserve earlier operation values and
  generation stamps after JSON transport, including captured roughing meshes.

Validation on 2026-09-10: 196 CAM and 126 host unit tests pass (one opt-in CAM
display capture remains ignored). The browser editing, icon-consistency,
linking/reorder, playback, responsive-ribbon and machine-workflow suites pass
against rebuilt WASM. Pure editing tests cover immutable copies, rest-source
references and frozen insertion anchors; the pure reorder test confirms
order-sensitive stock cache identity. TypeScript, icon provenance, desktop assets and the arm64 app
bundle pass; the bundle's signature, 30 libraries and five notices verify.

## Transitive rest-stock height intent

Rest-stock freshness includes height-expression records for every operation in
every upstream source setup, as well as setup, operation, tool and linking
intent. These side records matter before regeneration updates cached numerical
heights. Changing or removing source clearance, retract, feed, top or bottom
intent immediately marks all dependent rest results stale. Records are ordered
by operation ID, so moving records in storage does not invalidate evidence.

The upstream fingerprint uses a new version for rest setups. Older rest stamps
lack height-intent evidence and require explicit regeneration; no migration
blesses them as current. Independent setup fingerprints remain unchanged.
Existing source-first regeneration and NC posting gates remain in force.

Host regressions cover a three-setup rest chain plus an independent setup, each
height role and removal, failed-regeneration atomicity, save/reopen, source-first
regeneration, unchanged-intent restoration, legacy evidence and unrelated edits.
These are engine checks, not a new native UI qualification result.
