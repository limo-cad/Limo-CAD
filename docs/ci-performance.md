# CI performance without reducing coverage

## Cheap checks before packages

Desktop packages classifies paths and runs the reusable Version guard before
starting platform jobs. Each platform job qualifies its checked OCCT SDK before
compiling and staging packages. These gates apply to PR, tag and manual runs.
Packaged viewport, stdio, signing and portability checks remain in place.

Version guard includes the caller workflow in its concurrency key, so its
standalone required PR check and Desktop packages' preflight cannot cancel each
other. The required check names in [branch protection](branch-protection.md) stay
unchanged. The independent Rust interface workflow runs CI helper/contract tests.

## Compiled dependencies for Windows and macOS packages

Windows x64, Windows ARM64 and macOS Apple silicon packaging restore Rust
dependencies for both the root tooling workspace and the desktop workspace.
Restoration follows toolchain setup and precedes SDK preparation, so macOS can
also reuse the dependencies of the Rust SDK builder. The existing pinned Rust
cache action keys by compiler, Cargo manifests/lockfiles and build environment;
the additional key separates the runner and target architecture.

These caches exclude workspace crates and installed Cargo tools. Application
code is rebuilt and packages are staged, signed and verified by the existing
steps. Dependency caches can be saved when a later package check fails; the
check still fails normally. SDK verification, package tests, release gates,
triggers and the policy that lets SDK owners finish are unchanged.

The first compatible build populates the cache. Later updates to the same PR can
reuse it; sibling PRs cannot read each other's merge-ref caches. After merge,
an existing manual **Desktop packages** run on `main` can seed caches visible to
new PRs. No extra scheduled or push-triggered builds are added. Compare cache
restore/save time, compilation time and cache storage after rollout before
claiming a measured saving; repository cache limits and eviction still apply.

## Shared Windows OpenCASCADE caches

`.github/actions/setup-windows-occt` is used by Windows packaging, Windows MCP
acceptance and the SDK cache warmer. It retains the existing installed-tree and
binary-cache paths. Each key includes the runner/target ABI,
detected MSVC toolset, exact vcpkg revision, manifest and configuration hashes,
and the checked OCCT overlay sources. There is no
partial-key fallback across compiler or SDK versions.

The **Warm Windows SDK caches** workflow runs only on the default branch:

- After SDK manifest, checkout-attribute, setup-action or warmer-workflow changes merge to main.
- Daily at 06:17 UTC, to refresh after runner/compiler updates and retain the cache.
- Manually when needed, after the workflow is on main:

  ```sh
  gh workflow run windows-occt-cache.yml --ref main
  ```

It builds/restores the x64 and ARM64 SDKs, never application packages. The x64
job matches the Windows2025 packaging and MCP runner namespace. Default-branch
caches can be reused by new PRs; caches created by a PR cannot be reused by sibling
PRs. See [GitHub's cache scope rules](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache).
No PR source is checked out or executed in the default-branch warmer, and there
is no `pull_request_target` trigger or privileged cache-sharing workaround.

The first main run can still take around 100 minutes on the observed cold x64 key. The
benefit starts after that run saves the installed tree. A new compiler or cache
eviction must build a matching SDK again rather than reuse an incompatible one.

## Independent native acceptance runners

Each native platform has three test runners. Both call the same
`mcp-acceptance.yml` suite, so platform-independent commands and failure handling
are defined once. Linux starts independently of the Windows SDK preparation.

Windows prepares its SDK once per workflow run, before starting its three test
shards. The producer saves the existing ABI-keyed caches, then publishes the
installed x64 SDK as a seven-day artifact. Consumers download that exact artifact
ID from the same run and check their MSVC toolset against the producer. They do
not check out vcpkg or rebuild a missing SDK. A failed producer, missing artifact,
compiler mismatch or missing headers prevents acceptance and fails its aggregate
gate. Rerunning failed test jobs can reuse the successful producer's artifact;
after expiry or a compiler-image change, rerun the whole workflow to prepare it
again. No cross-run artifact lookup or privileged fork trigger is introduced.

| Shard | Tests and outputs |
| --- | --- |
| `core` | All normal cargo test targets and doctests except the two exact flagship tests; native OCCT integration regressions; complete feature workshop; drawing, repeated bench/fillet checks; Windows xtask tests; verified bench project |
| `turbine` | Existing complete turbine construction, edits, restoration, printing, mechanics and independent replay test; verified turbine project |
| `vise` | Existing complete vise construction, edits, restoration, printing and mechanics test; verified vise project |

Sharding does not relax correctness assertions, ignored-test settings, production
request deadlines or per-host job timeout limits. `--test-threads=1` remains in every shard: separate runners
avoid the CPU/memory contention and shared-process environment races that made
parallel tests on one runner unreliable. A compiled inventory check rejects a
missing/renamed flagship instead of allowing libtest's zero-test success.

The first Windows core run exposed an existing timing assumption in the inbox
sequence-safety stress test: eight writers perform 128 durable publications,
but the test required every contended lock within five seconds and all reader
I/O within ten seconds. The stress fixture now shares a 120-second budget
across all workers and wakes the reader after publication instead of busy
scanning. It still validates every payload, contiguous unique sequence IDs and
concurrent archiving; it does not retry failed writes or skip assertions.
Normal publishing retains its five-second lock timeout. Separate timeout tests
verify that a blocked publisher fails without writing or consuming a sequence.
The longer budget is only for this safety stress fixture, not application
requests or CI job limits.

The session-level reservation test hit the same assumption a week later:
sixteen threads poll one OS lock for 128 durable publications, and a loaded
Windows runner starved two pollers past production's five-second wait while
the other fourteen kept publishing. It now shares one 120-second budget across
its threads through `write_inbox_op_within`, joins every worker before
reporting, and still requires 128 distinct, durable, pending entries. The test
fixture that serializes `LIMO_CAD_SESSION_DIR` also recovers its lock after a
holder panics, so one failing test reports as one failure instead of a
`PoisonError` in every later test of the same binary.

The stable `mcp-tests` and `MCP tests (Ubuntu)` checks aggregate all three shards
for their respective platform. They fail if any shard fails, cancels or skips;
Ubuntu does not wait for Windows. Only then are that platform's three project
inputs downloaded from the same workflow run and assembled into the existing
`Limo-CAD-demo-projects-{platform}-{sha}` artifact with version, source commit,
sizes and SHA-256 hashes. Partial/empty inputs never produce a final demo bundle.
Rerunning failed jobs can reuse successful shard inputs from the same run; input
artifacts are kept for seven days, after which all shards must be rerun together.

Sharding preserves serial recipe assertions while running independent checks on
separate runners. Windows SDK preparation is shared; the small Cargo compilation
remains per shard so future test targets and doctests retain Cargo's normal
behavior. The producer adds a scheduling and artifact-transfer barrier, whose
cost must be measured separately on cache-hit runs.

## Baseline and verification

In [main run 35241288725](https://github.com/limo-cad/Limo-CAD/actions/runs/35241288725),
recipe acceptance took 58 minutes on Linux and 100 minutes on Windows, while
compilation took 52 seconds and 2 minutes respectively. That makes runner-isolated
flagship tests more useful than adding Rust caches alone. In
[package run 35526256187](https://github.com/limo-cad/Limo-CAD/actions/runs/35526256187),
the cold Windows ARM SDK installation took 71 minutes. These are baselines, not
a guaranteed runtime for every runner.

Run `cargo test --locked -p xtask release_tooling::` and
`cargo test --locked -p xtask workflow_contracts::` for fast
contract and negative-path tests. Also validate workflow YAML/expressions with
actionlint when editing it; its runner-label catalog may lag the existing
`ubuntu-26.04` and `windows-11-vs2026-arm` labels used by this repository. Compare
real Actions job/step timings after rollout before claiming a measured speedup.

## Measured bottlenecks

Observed on 12 October 2026. These are hosted job timings, excluding local compilation.
Queue time and execution time are separate; GitHub reported an Actions incident
affecting queues and checks that day. Runner capacity and incident recovery can
delay jobs independently of repository changes.

- [Windows core acceptance](https://github.com/limo-cad/Limo-CAD/actions/runs/38251064087/job/114817714771):
  2 seconds queued, 122 minutes 47 seconds executing. SDK setup took 98 minutes
  21 seconds, MCP tests 9 minutes 22 seconds, native geometry regressions
  8 minutes 10 seconds, and the workshop 5 minutes 4 seconds.
  Both exact SDK caches missed; compiling OpenCASCADE alone took about 94 minutes.
  All three Windows shards independently prepared the same SDK.
  Their SDK setup steps totaled 17,440 seconds (290m40s). A single preparation
  at the observed slowest duration of 5,901 seconds would avoid 11,539 seconds
  (192m19s), about 66% of that preparation work, before artifact-transfer costs.
  This estimates runner work; it is not a measured reduction in critical-path
  elapsed time. The cold critical path still includes one SDK build, while a
  compatible default-branch cache can remove that compilation on later runs.
- [Linux turbine acceptance](https://github.com/limo-cad/Limo-CAD/actions/runs/38251064087/job/114817714831):
  3 seconds queued, 51 minutes 10 seconds executing. Rust compilation took
  51.75 seconds; the native geometry recipe test took 2,980.34 seconds.
  Reducing compiler work will have little effect on this test's dominant cost.
  Flagship invocations retain their existing slow-RPC output on successful runs,
  making construction, drawing, restore and interference costs visible without
  removing assertions, motion samples or independent replay.
- [Browser smoke](https://github.com/limo-cad/Limo-CAD/actions/runs/38266323719/job/114869293985):
  22 minutes 13 seconds queued, 6 minutes 28 seconds executing. Installing
  wasm-pack took 68 seconds, compiling the browser engine 184 seconds, and
  smoke tests 89 seconds.
- [Host-neutral Rust tests](https://github.com/limo-cad/Limo-CAD/actions/runs/38251063987):
  2 seconds queued, 4 minutes 11 seconds executing.

The cache API snapshot contained 23 caches totaling 10,748,439,918 bytes, all
scoped to PR merge refs. That is around the default 10 GiB repository cache limit.
A cache under one PR merge ref cannot supply an unrelated PR. Default-branch
caches can supply PRs; keeping compatible SDKs there avoids repeated cold builds.

## Manifest identity and browser tool reuse

`vcpkg.json` uses LF on every checkout. Its Windows CRLF hash was
`f0611456cc56445180a7a46a36d542dbbb53c90b45df2f741cd9a15b3eb260d5`, matching
the observed misses. The same manifest with LF hashes to
`2b9d516ae49f1cdf2100ff1ba7a12ef10bd19ea468623ef63247b89cb28165c8`.
This intentionally requires one compatible cold seed after adoption. Cache keys
retain every ABI input and have no broad SDK fallback.

The browser job checks the version of wasm-pack restored by its existing Rust
cache. It installs only when absent or different from pinned 0.13.1. The tool's
installation metadata is not needed to reuse the verified binary. Installation
failure still fails the job. No extra cache or test skip is introduced.

No acceptance tests or package gates are removed. Post-change hosted savings
remain to be measured after the default-branch seed. Avoided tool/SDK work is
not a promise of total wall-clock savings. Standard hosted runners are free for
this public repository; no paid runners are required.

Reproduce the observations without launching more builds:

```sh
gh api repos/limo-cad/Limo-CAD/actions/runs/38251064087/jobs --paginate
gh api repos/limo-cad/Limo-CAD/actions/jobs/114817714771/logs
gh api repos/limo-cad/Limo-CAD/actions/jobs/114817714831/logs
gh api repos/limo-cad/Limo-CAD/actions/cache/usage
gh api repos/limo-cad/Limo-CAD/actions/caches --paginate
```

Local verification: 16 workflow/sharding/release contracts passed using Git Bash
on Windows, actionlint passed excluding only the existing ARM runner label, and
the actual tool guard passed missing/matching/wrong-version and failed-install
cases. Ten executable platform/SDK guard cases passed, including missing inputs,
unknown platform, compiler mismatch and missing SDK headers. An autocrlf-enabled
checkout preserved LF manifest bytes. The unrelated
full-suite symlink fixture needs Windows symlink privileges.

After merge, compare matched x64 jobs with the same manifest/compiler key. Record
queue delay, SDK source/setup duration, and total execution separately. Inspect
the browser tool step and SDK artifact upload/download independently. Investigate the turbine recipe geometry
cost next if it remains dominant, preserving its correctness assertions.

Sources: [runner billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions),
[cache scope and eviction](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching),
[Rust cache contents](https://github.com/Swatinem/rust-cache/tree/v2#cache-details),
[GitHub incident history](https://www.githubstatus.com/).
