# Review corrections and verification — September 10, 2026

The 21-layer stack starts at upstream main `39eb862`. Each layer retains an
incremental diff against its predecessor. All previous heads remain recoverable
in local backup refs; the recipe sources and geometry are unchanged from the
previous review candidate. See [stack order](development-stack.md).

## Corrections

- **PR97:** exit waits for pending control acknowledgements and checks for edits
  arriving during Save before closing. Cancel, explicit Discard and failed Save
  keep their intended behavior.
- **PR99:** source loading and running are mutually exclusive, a dismissed panel
  stays dismissed during loading, and observational session reads cannot advance
  an unpublished model fence. Test-only MCP helpers stay out of production.
- **PR108:** a proven non-mutating Open rejection permits fresh Save and export
  attempts. Uncertain replacement and failed publication remain guarded. Save,
  Rename, recovery, STEP and mesh exports retain the document, selection and
  model they captured before asynchronous file operations. Save As serializes a
  renamed copy and adopts its target only after the write succeeds.
- **PR108:** temporary history stages preserve materials and hidden-body choices
  for retained feature outputs, including consumed and suppressed bodies. Actual
  deletion or removal of an output still removes obsolete metadata. History
  editing restores presentation state, preserves newer eye/material choices,
  and rejects delayed editor effects on another document. The shared document
  revision and visibility prerequisites live in this layer.
- **PR115:** replacement retires the old native session. Script startup,
  publication, queued mutations, controls and completion retain their owner.
  Receipts are identified by session and sequence; an old receipt cannot select
  an unrelated attachment. Direct MCP history navigation restores retained
  metadata through the existing refresh path. Ordinary solid operations keep
  their existing fast update path.

## Tested source and scope

The Windows desktop, MCP server and Rust xtask were rebuilt at `5406bdf`.
The final prerequisite restack at `ce8fa3f` has a byte-for-byte identical Git
source tree. Subsequent evidence edits are documentation only.

Validation passed:

- 526 Rust workspace tests; the two ignored cases are fixture generators.
- 162 native MCP library tests and 73 Windows desktop library tests.
- Frontend tests, desktop frontend build and formatting checks.
- 38 smoke checks against freshly generated WASM, including captured-model
  export, copied-name export and guarded name adoption.
- Browser contracts for 155 product controls, plus production Open/export,
  Save/recovery, history/editor, inbox, publication, script and delayed-control
  behavior. Ordinary polling, same-document edits and document replacement are
  tested separately. Deliberately restored regressions failed these tests.

Standalone PR108 at `2049bdb` passed TypeScript and the complete browser contract
suite, including history metadata and deferred editor callbacks. PR99's earlier
standalone validation is recorded under `layer99`; later integration results
are not presented as independent validation of every intermediate commit.

## Live file verification

The original live bench completed 595 steps and 47 checks at `bac4c2b`, matching
two independent native references. File/Open then exposed a real bug: temporary
datum-frame restoration deleted materials from bodies 25, 26 and 27, the arm
support rail and armrests. Geometry and all other saved fields were unchanged.

The rebuilt correction reopened that same archive through MCP File/Open with
all project values equal and all 20 material assignments retained. The archive
was preserved unchanged; its SHA-256 is
`8387ac281dc3f250740c8a7587ca288033d28926e4426408b6278514ee92f29d`.
JSON formatting and key order are not model identity.

The final rebuilt desktop then repeated the complete bench from a blank
document: 595 steps and 47 final checks in 160.920 seconds at presentation speed
16. Model, sketches, geometry and solved assembly matched the independent
reference exactly. The completed presentation retained 595/595 and the final
isometric camera label. This is a presentation timing, not a comparative
performance measurement.

Direct MCP rollback to stage zero and forward to stage 95 both applied and
published. The restored project matched its saved model exactly, including all
20 material assignments. Final 3MF export and Save succeeded, and MCP closed
the owned validation window; no CAD process remained.

Earlier live checks at `bac4c2b` rejected malformed native model input without losing
the active session, exported a valid 417,078-byte 3MF afterward, retired the
session on native New, and rejected a write addressed to that retired session.
Other applications and user documents were preserved throughout validation.

## Refreshed vise and turbine verification

The same rebuilt binaries also completed two independent headless runs of each
manufacturing candidate. Both runs matched each other and the earlier saved
reference model, sketches, geometry and solved assembly exactly:

- Vise: 692 steps and 55 final checks per run; 93.287 and 90.158 seconds.
- Turbine: 1,622 steps and 11 final checks per run; 232.508 and 198.761 seconds.

Both then completed attached desktop runs in fast mode, using one owned window
in the background. The vise completed in 254.526 seconds and the turbine in
860.710 seconds, with the same checks and exact reference comparison passing.
These runs overlapped other verification and are not controlled performance
benchmarks. Their purpose is to qualify the shared history/session correction
against the other two complete recipes, including their drawings.

MCP Save and File/Open preserved every saved project value, including all five
vise and 16 turbine material assignments. Saved archive SHA-256 values:

- Vise: `4349d78aa21280861aa777fe6c41e2cfdae9f53e551c97b7b99c3e7097f2f99d`
- Turbine: `a7ba4f197738722b53a1c954c5757e002911560591f164d2f0dccf14f5645965`

MCP Close acknowledged the request and the owned process then exited; no CAD
process remained. A close acknowledgement is not an instantaneous process-exit
observation. The local evidence is retained under `flagship-refresh` alongside
the bench verification. Earlier manufacturing manifests remain historical;
these new runs do not add physical fit, load, wear or generator-output evidence.

## Published CI follow-up

The first new [PR100 macOS packaging job](https://github.com/jackControls/noBS-CAD/actions/runs/34554969245/job/103125612038)
compiled and ad-hoc signed the app, then failed in `bundle_dmg.sh`. Its log did
not expose the underlying subprocess failure. Successful packages on other
layers do not establish the cause of that failure.

Independent main-based [PR117](https://github.com/jackControls/noBS-CAD/pull/117)
enables the locked Tauri CLI's debug output in the existing ad-hoc DMG step.
Production signing and validation are unchanged. This repairs missing diagnostic
output; the original disk-image failure still needs a confirmed outcome.

## Binary identification

- Desktop SHA-256: `341E41E7EFCC4AADE18C0BA9878850371C3B997157DF837A8A99459029906D61`
- MCP SHA-256: `51FFEAC9010D5EFD2F2603ED3AE7F3F9FCE98A9DE331664F507B94202E91BFDB`
- xtask SHA-256: `A64B0BA43942CFF3664CA8D032A446FAA22251A3875868BDE57AFC3975F73299`

## Remaining review boundaries

PR99 remains draft for the native miniature preview, load/run/inspect layout,
chapter staging and presentation/performance work described in the
[interface review](script-interface-review.md). The added history metadata reads
are scoped to history navigation; these tests do not establish a maximum-rate
performance improvement.

Bench drafting remains in issue93, selected-occurrence editing in issue94, and
the lesson collection in issue16. Physical print/fit, wear/load and generator
output qualification remain unperformed. The manufacturing candidates and their
earlier tested revisions remain documented in the existing validation manifests.
Changed PR heads require renewed review; historical approvals and local tests
do not establish GitHub approval or CI status for a newly pushed head.
