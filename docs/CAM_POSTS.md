# Posts and private machine profiles

Native Rust output for fixed three-axis milling;
these are starter contracts, **not commissioned machine kits**. CAM stock checks
do not establish controller, fixture, holder, PLC or whole-machine safety.

## Tool identity

Post NC reads the **project tool library** directly. There is no separate
tool-call mapping form or per-tool verification checkbox. Automatic mode uses
the library number when present; Siemens native and TNC can use the exact name
when no number is assigned. Those posts also offer explicit Number/Name mode.
Numeric-only posts require a library number. No internal ID substitution,
uppercasing, punctuation replacement or truncation occurs.

Named calls use a conservative 1–31 ASCII letter/digit/underscore subset and
preserve case. Unsupported names, missing numbers and duplicate executable
calls stop output with a library-edit error. A quoted numeric name is not a
numeric call. Siemens NC replay resolves the same exact library identities,
including tools that appear only in external NC. Old `tool_calls` records are
read for compatibility but ignored; saving post settings clears them.

The ordinary machine-settings/output review remains: this is not a readback
of the machine's tool table. Numeric T selection can denote a magazine
location on some configurations. Confirm the physical tool table, H/D geometry,
work offsets and machine-specific M codes during commissioning.

## Private storage

To select the post, double-click the setup in the CAM browser and use
**Machine & Controller → Setup target**. Built-in targets and validated
private native profiles appear there. **Post NC → Change machine in Setup…**
opens the same editor; there is no independent dialect override in Post NC.

**Settings → CAM → Custom posts** shows the actual folder and provides
**Import post…**, **Open post folder** and **Refresh posts**. Post NC links to
this panel. The directory is `<app-config>/cam-posts`, separate from both app
bundles and project files. On macOS it is:

    ~/Library/Application Support/org.limocad.desktop/cam-posts

It is a sibling of the default central library, not inside it. Changing the
central-library location does not relocate posts. The native platform config
root is used on other operating systems; Settings shows its resolved path.
App replacement and project export do not overwrite or package these sources.

Two kinds of entry are deliberately distinct:

- **Native profile (`.nbpost`, JSON):** `{format: "nbpost", schema_version: 1 | 2,
  machine: <CamMachineAssignmentDto>}`. A validated built-in renderer plus a
  private machine snapshot—not arbitrary executable code. In Setup,
  **Save as private post profile** saves the current machine definition.
  Saved profiles appear under **Your private post profiles** in the target
  selector. Selection copies a snapshot; later disk changes do not mutate jobs.
- **Source reference (`.cps` or legacy JavaScript `.nbpost`):** preserved
  byte-for-byte, including rights notices, but **not executed** or offered as
  a runnable target. Renaming a source does not convert it into a native post.
  Machine-builder behavior requires a separately implemented and tested extension.

Source posts and machine-specific profiles belong only in user storage, not
in this repository or the app bundle. Built-in renderers do not include
private machine-builder behavior.

### Native private spindle-stop extension

The native Siemens configuration accepts an optional `spindle_stop_subprogram`.
It is a single bare identifier: 3–31 ASCII letters/digits/underscores, starting
with a letter and containing an underscore. Arguments, paths and raw NC blocks
are rejected. Each explicit spindle-off event emits that identifier immediately
before M5, including later tool changes and program shutdown. RPM-only changes
do not introduce a stop. An absent value leaves the standard post unchanged.

Such a profile requires **both the `.nbpost` and machine-profile schema version
2**. Ordinary profiles remain version 1. Old readers reject the newer version
instead of silently stripping the private behavior. Setup selection, device
defaults, project snapshots and Post NC edits preserve the value. Post NC shows
the call read-only and includes it in the reviewed output settings.

A native profile configures the supported fixed-three-axis renderer; it is
not a byte-identical translation of a source post. Tool calls come from the
project library, and D, retract, preload and optional-stop settings remain
explicit profile fields. Private call identifiers are not bundled defaults.

The controller-resident subprogram body is unavailable to CAM. Posting discloses
that its motion, time and side effects are not verified; **NC replay refuses
opaque subprogram calls**, including calls on excluded machine-coordinate
blocks, instead of ignoring them. CAM simulation still covers neutral tool
motion only. The machine builder/operator must commission the subprogram and
the machine state around its call before using the output.

Imports are bounded to nonempty UTF-8 regular files, 2 MiB per file, 128 files
per folder. Plain filenames only; no traversal or symlink import. Publication
is atomic and never overwrites an existing different file. An identical import
is idempotent; rename a changed source/profile to retain both versions. Invalid
native profiles are listed with a reason and cannot be selected. Unsupported
motion modes and mismatched controller/post profiles are rejected. Operations
run on native background workers. External syncing/backups are user-managed.

## Built-in controller contracts

Independently authored renderers share neutral tool-tip motion, units and
validation. Source references are never executed.
Common formatting is shared where the supported semantics actually agree.

| Target | File | Tools / length offsets | Fixture / retract | Dwell |
| --- | --- | --- | --- | --- |
| Siemens 828D native | `.mpf` | Numeric or exact named T, M6, configured D | G54–G59; configured SUPA Z D0 | G4 F seconds |
| FANUC | `.nc` | Tn M6; G43 Hn | G54–G59; G90 G53 G0 Z | G4 P milliseconds |
| Haas NGC | `.nc` | Tn M6; explicit G43 Hn | G54–G59; G90 G53 G0 Z | G4 P decimal seconds |
| Mitsubishi M80/M800 | `.nc` | Tn M6; G43 Hn | G54–G59; G90 G53 G0 Z | G4 P milliseconds |
| Mazak EIA milling | `.eia` | Tn M6; G43 Hn | G54–G59; G90 G53 G0 Z | G4 P milliseconds |
| Syntec milling | `.nc` | Tn M6; G43 Hn | G54–G59; G90 G53 G0 Z | G4 P milliseconds |
| Okuma OSP milling | `.min` | Tn M6; G56 Hn; G53 cancels length | G15 H1–H6; G16 H0 G0 Z | G4 F seconds |
| Heidenhain TNC | `.h` | Numeric/exact named TOOL CALL | Cycle 247 preset 1–6; L Z R0 FMAX M91 | Cycle 9 seconds |
| Hermle / Heidenhain | `.h` | Same fixed-axis TNC subset | Same; no builder/rotary macros | Cycle 9 seconds |

LinuxCNC and GRBL retain their existing output policies. Siemens ISO, turning,
indexed/simultaneous rotary, TCP, probing, smoothing cycles, subprograms,
thread/tapping synchronization and machine-specific macros are not enabled by
selecting a brand. Hermle naming does not claim a complete machine package.

FANUC and the new starters require **Machine retract Z** to be entered before
posting; blank means unknown, never an assumed safe zero. Retract before tool
changes and changed fixture offsets. After the tool call, traverse XY while
retracted, then approach Z with the tool-length register. Preserve intentional
high-feed G1 approaches. Length cancellation follows retract, never in stock.
The operator must establish a reviewed fixed-axis/reset state and confirm that
M6/TOOL CALL leaves sufficient traversing height. Arbitrary restart is unverified.

The new profiles assume flood/off coolant only. Unknown mist/builder M codes
are rejected, not guessed. Drilling/peck/dwell cycles remain explicit neutral
motion. Optional stops and preloading are off; the existing Siemens profile
retains explicit opt-ins. H/D use the library number; independent register
remapping is not yet implemented.

Conservative numeric bounds: tool calls up to 99999 for the new starters;
Haas H/D 1–200; Mazak H/D 1–512; OSP cutter-radius D1–D999. Program numbers are 1–9999,
or 1–99999 on Haas. These bounds do not prove actual machine table capacity.
Sequence numbering never wraps: Syntec stops above N8999, Haas above N99999,
other new ISO starters above N99999999. Disable numbering for longer files.
There is a one-million-block output cap. FANUC/Mitsubishi P dwell is at most
9999 ms; other new dwell output is bounded to 99999 seconds. No silent clamping.

## Arc and compensation handling

Only XY-plane arcs are accepted. ISO/OSP use incremental I/J; TNC uses absolute
CC centers and C endpoints with DR direction. Inch/metric coordinates and feed
are converted once from canonical millimetres. Arc-bearing programs retain
6 metric / 8 inch decimal places to avoid rounded radius inconsistencies.

TNC helices are linearized independently in Rust. For radius `r` and allowed
chord deviation `ε = 0.002 mm`, choose

    Δθ ≤ min(5°, 2 acos(1 − min(ε/r, 1)))
    n = ceil(|sweep| / Δθ)
    p(t) = (cx + r cos(θ0 + t·sweep), cy + r sin(θ0 + t·sweep), z0 + t·Δz)

Thus radial chord deviation `r(1−cos(Δθ/2)) ≤ ε`; depth interpolation is linear,
CW/CCW is preserved, and the final endpoint is exact before output rounding.
Reject nonfinite/inconsistent radii or more than 100000 chords per helix.
This is an output approximation, not a machine accuracy claim.

Cutter compensation uses the existing generated-move gate: linear XY switches
at least one tool radius for the new starters, with no rapid/arc activation or
uncanceled state at tool change/end. TNC maps to RL/RR/R0, not ISO G41/G42/G40.
The full physical tool radius is assumed; wear-only and 3D compensation are
unsupported. See [machine compensation policy](CAM_MACHINES.md).

## Replay and validation scope

Our new ISO post headers explicitly identify dwell semantics. Auto replay
recognizes those headers and rejects a conflicting manually selected dialect.
Haas replay distinguishes `P10` (10 ms) from `P10.` (10 seconds). An arbitrary
unmarked program still needs the appropriate explicit dialect; target brand
does not silently select it. OSP and TNC have no NC interpreter yet: they are
refused, not replayed as ISO. CAM simulation remains available but is not proof
of those emitted programs. Machine commissioning/control-side checks are still
required for every starter.

Regression coverage includes per-brand headers/units/fixture and tool changes,
library identity, invalid profiles/registers, dwell units, compensated motion,
metric/inch stock round trips for ISO brands, bounded TNC helix chords, and
native storage integrity. Browser tests exercise actual WASM posting plus
isolated storage IPC; they do not alter the operator's job or central library.

Private-extension tests cover numbered/unnumbered output, tool changes,
program end, RPM-only changes, malformed identifiers, reader-version rejection,
preserved settings and refused opaque NC replay. Optional private-file tests
require an explicit local path and are not part of the portable fixture suite.
