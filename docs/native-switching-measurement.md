# Bounded Bevy switching measurement

The optional comparison builds published Bevy preview source
`82cd981eb9d835faf481a021c416e7733cfb9f91` and the candidate using the same
pinned Rust compiler. It has not completed a matched current-source run and
establishes no performance result or cause for reported tab/sheet sputtering.

`cargo xtask switching-comparison --plan` prints the sixteen cases on any OS
without launching an application. Execution is restricted to the explicitly
opted-in, owned GitHub-hosted Linux job in
[native-switching.yml](../.github/workflows/native-switching.yml).
Rust owns builds, hashing, the case matrix, window-manager lifetime, bounded
focus observations and receipt aggregation. Xvfb, Openbox, DBus, xprop and
xdotool supply Linux platform services.

The matrix covers document tabs and drawing sheets, one and two application
instances, and two repeats with baseline/candidate order reversed on the
second repeat. Three frozen archives and two manifests are retained alongside
source revisions, both lockfiles, compiler/system inventory, build commands,
logs and executable hashes. Both builds finish before measuring. Each case
uses its own Xvfb display, registry, preferences and evidence directory. The
process-tree ownership check precedes every GUI launch and X11 focus read.

For an individual invocation, use a fresh private Xvfb desktop and a separately
built Bevy executable:

```sh
cargo xtask test-mcp switching-measurement \
  --server /absolute/build/limo-cad \
  --out /absolute/empty/evidence \
  --model-a /absolute/fixtures/part-a.limo \
  --model-b /absolute/fixtures/part-b.limo \
  --commit FULL_BUILD_SOURCE_SHA --profile release \
  --cycles 20 --instances 1
```

Use distinct document names and real solids for document tabs. Drawing sheets
use `--scenario drawing-sheets` with one `--model-a` archive containing exactly
two named sheets with projected views; omit `--model-b`. The obsolete `--shell`
option is rejected. This driver supports Bevy hosts only.

Each invocation excludes two warm-up cycles from statistics. A semantic click
is timed from request to application acknowledgment; control lookup is outside
that interval. Separate settled-navigation timings include read-only
inspection, attachment, process sampling and exact-model verification. The
only permitted drawing change is the requested active sheet ID. An unrelated
geometry, history, annotation or CAM change fails immediately and is retained
as evidence. Selected-sheet observation also requires the expected sheet name.

Two-instance cases time one foreground request, then require both application
focus and actual X11 active/input window PIDs to match the owned process and
document. Observations never replay the request. RPC requests have a 45-second
deadline, completion observations share a five-second budget, and the measured
loop has a fifteen-minute budget. Read-only X11 commands use bounded output and
an owned child terminated on timeout or error.

Failures retain partial receipts and logs. Aggregation examines exactly the
expected cases and requires every successful report, twelve loaded-model
copies per archive, exact loaded-model equality across hosts, frozen input
hashes and matching executable/source/profile/instance/cycle receipts.
Missing or invalid JSON is reported as incomplete evidence. Process samples
are bounded; summed RSS is not unique physical memory.

Source revision and build profile are declarations recorded by the build
coordinator; hashes are computed from actual files. Application acknowledgment
and GPU submission do not prove compositor presentation, physical input
latency, GPU time or memory, monitor/DPI behavior, a performance threshold, or
the cause of a user's observation. No qualification job is required merely to
inspect the plan or run the pure coordinator regressions.
