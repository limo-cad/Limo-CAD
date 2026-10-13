# Matched switching inputs

Use the committed `part-a.nbcad` and `part-b.nbcad` bytes for every measured
host. A has one 18 x 12 x 8 mm block; B has six separate blocks of that size
on a 30 mm grid. They have distinct document names, fully constrained
sketches, and respectively two and twelve history features. These small
fixtures exercise retained documents with different instance layouts. They
are not a reproduction of the user's reported latency and are not a complex
assembly stress test.

The adjacent scripts use existing sketch/solid commands and explicit final
checks. Each script passed two independent headless runs with identical
models, sketches, and geometry. Every saved archive was reopened in another
fresh headless engine and checked for exact model/body preservation before
writing. `manifest.json` records the committed input hashes, sizes, expected
counts, generator binary hash, and its initialization metadata. The generator
reports a modified build; its exact source was not independently attested.

The separate `sheets.nbcad` input retains B's six solids and adds two sheets:
one A4 sheet with one front view, and one A3 sheet with twelve front/top/right
views. `sheets-manifest.json` records its separately built headless generator
at clean source `8341b850`, exact hashes, two independent 47-step/17-check
replays, fresh-engine archive reopens, real front-projection edges, and four
sheet selections that changed only the expected active sheet ID. The input
is ready for a disposable live measurement; those checks do not measure
latency or verify rendered pixels.

To regenerate into a new directory with a known host, use the existing runner:

```sh
mkdir -p "$RUNNER_TEMP/switching-regenerated"
for part in part-a part-b; do
  cargo xtask run-script "xtask/fixtures/switching/$part.nbcad.jsonc" \
    --server /absolute/build/nbcad --server-arg --headless --repeat 2 \
    --out "$RUNNER_TEMP/switching-regenerated/$part" \
    --save "$RUNNER_TEMP/switching-regenerated/$part.nbcad"
done
```

Compare canonical models and archive hashes before replacing the committed
fixtures; do not silently regenerate different inputs for different hosts.

## Disposable workflow plan

Keep this opt-in and separate from release promotion. On one Ubuntu runner,
use the existing Linux desktop dependency setup and prepare both hosts
before measurement: the pinned Bevy preview and the current Bevy candidate.
Build both directly with Cargo in their isolated checkouts;
the candidate uses the native Cargo build. Use the same Rust build profile. Preserve exact
source SHAs, feature/build commands, lockfiles, host hashes, and build logs.
Build the current xtask driver once; it can drive both executables.

For each host, run two repetitions at scale 1 with the committed input pair,
first one instance, then two. Reverse host order for the second repetition.
Each invocation gets a fresh owned Xvfb display, window manager, registry,
config, and output directory. Reuse the existing 3200x2160x24 screen setup
and record the actual adapter/driver from host logs. Finish all builds before
timing, and run only one measurement invocation at a time. Scale 2 is a
separate follow-up after scale 1 works, not a substitute baseline.

Invoke `test-mcp switching-measurement` as documented in
`docs/native-switching-measurement.md`, with `--cycles 20`, declared shell,
exact source SHA, profile, and the two committed absolute archive paths.
First compare saved loaded-model JSON across all hosts; any model difference
invalidates a timing comparison. Keep raw samples, partial failures, UI
snapshots, CPU/RSS/I/O samples and host stdout/stderr with the summary.

The current driver has been compiled and unit checked. This workflow has not
been run, and no threshold or performance comparison is established. Requests
measure application acknowledgment, not equivalent compositor/GPU timing or
physical Alt+Tab. For the separate Drawing experiment, use
`--scenario drawing-sheets --model-a /absolute/sheets.nbcad` and omit
`--model-b`. Repeat the same host/instance/reversal procedure, keep its results
separate, and compare `instance-*/loaded-0.json` across hosts before timing
interpretation. Sheet selection permits only the requested canonical
`drawings.active_sheet_id` change; every other saved field must remain exact.
