# Checked OCCT 8.0.1 storage

The SDK uses upstream tag `V8_0_1`, commit
`b8f597c677811d1f9f4d8a97f5ae2825c0353a42`. The upstream matrix implementation
is now header-only and uses `NCollection_Array2`; the old 7.9 `.cxx`/`.lxx`
replacements are retired.

The replacement `math_DoubleTab.hxx` preserves the 8.0 class layout and
64-element small buffer. It checks dimensions and byte counts before selecting
storage, uses zero-based array constructors to avoid signed offset arithmetic,
checks rebasing, and computes upper bounds in 64 bits. Copy requires equal
dimensions, as upstream 8.0 does, and uses `memmove` for overlapping storage.
Small, external and heap storage retain copy and move behavior.

The Windows port and five packaging patches come from microsoft/vcpkg commit
`f89a4a1da4e3176a8d1a14c1825b9b2f98e48843`. Overlay port revision 1 adds the
checked matrix header. Both this port and `cargo xtask build-occt` verify the
exact upstream archive and header hashes before replacement. Cache identities
include the replacement content; source and installed SDK receipts are checked
before reuse. SDK discovery requires the 8.0 ABI and rejects 7.9 headers.

The OCCT header retains its LGPL-2.1 license and OCCT exception notice.
The vcpkg files retain Microsoft's MIT license in `VCPKG-LICENSE.txt`.
Upstream source: https://github.com/Open-Cascade-SAS/OCCT/tree/V8_0_1.

```sh
cargo xtask build-occt --prefix /absolute/path/to/a/fresh/sdk --jobs 2
cargo xtask verify-occt-storage --prefix /absolute/path/to/a/fresh/sdk
```

Build a separate 8.0 SDK; preserve existing SDKs, application sessions and
recovery data. Verification compiles a bounded probe against the selected
`TKMath` and `TKernel`, checking overflow, null storage, copy, overlap, empty
ranges, extreme bounds, rebasing and moves. The entire SDK must use the checked
header; replacing only an installed header does not rebuild existing libraries.
