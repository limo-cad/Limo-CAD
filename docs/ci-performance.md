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

## Shared Windows ARM OpenCASCADE cache

`.github/actions/setup-windows-occt` is used by Windows packaging, Windows MCP
acceptance and the SDK cache warmer. It retains the existing installed-tree and
binary-cache paths. Each key includes the runner/target ABI,
detected MSVC toolset, exact vcpkg revision, manifest and configuration hashes,
and the checked OCCT overlay sources. There is no
partial-key fallback across compiler or SDK versions.

The **Warm Windows ARM SDK cache** workflow runs only on the default branch:

- After SDK manifest, setup-action or warmer-workflow changes merge to main.
- Daily at 06:17 UTC, to refresh after runner/compiler updates and retain the cache.
- Manually when needed, after the workflow is on main:

  ```sh
  gh workflow run windows-occt-cache.yml --ref main
  ```

It builds/restores only the ARM64 SDK, never application packages. Default-branch
caches can be reused by new PRs; caches created by a PR cannot be reused by sibling
PRs. See [GitHub's cache scope rules](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache).
No PR source is checked out or executed in the default-branch warmer, and there
is no `pull_request_target` trigger or privileged cache-sharing workaround.

The first main run may still take about an hour on a cold SDK/compiler key. The
benefit starts after that run saves the installed tree. A new compiler or cache
eviction must build a matching SDK again rather than reuse an incompatible one.

## Independent native acceptance runners

Each native platform now has three runners:

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

Sharding reduces elapsed time, not necessarily total billed runner minutes: it
duplicates a small amount of compilation/SDK setup and needs additional runner
capacity. The serial recipe assertions still do the same work.

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
