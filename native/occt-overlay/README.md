# Checked OCCT 7.9.3 storage

The replacement `math_DoubleTab.cxx` and `.lxx` preserve the upstream class layout
and exported signatures. They check dimensions before allocation, retain `size_t`
byte counts, reject undersized copy destinations, and check rebased index bounds.
The small-buffer threshold remains 16 elements; empty storage and overlapping
copies retain their behavior. Valid matrix indexing retains its existing hot path.

The Windows port is derived from microsoft/vcpkg tree
`9a763d6711b422dea8fd72603ef8f89ff2386983` (OCCT 7.9.3, port revision 1).
The overlay adds revision 2. Both that port and `cargo xtask build-occt` verify
the exact upstream input hashes before applying these replacements. Rust build
cache identities include the replacement content; source and installed SDK
receipts are checked before reuse.

The changed OCCT files retain their LGPL-2.1 license and OCCT exception notices.
The vcpkg port and original packaging patches retain Microsoft's MIT license in
`VCPKG-LICENSE.txt`. Upstream OCCT source is
https://github.com/Open-Cascade-SAS/OCCT/tree/V7_9_3.

```sh
cargo xtask build-occt --prefix /absolute/path/to/a/fresh/sdk --jobs 2
cargo xtask verify-occt-storage --prefix /absolute/path/to/a/fresh/sdk
```

Windows manifest installs select the overlay through `vcpkg-configuration.json`.
Unix builds use the Rust builder and a C++ compiler, CMake/Ninja and FreeType.
Verification compiles a bounded probe against the installed `TKMath` and
`TKernel`; it checks allocation, copy, overlap, empty ranges and rebase behavior.
Replacing only the header is insufficient: the allocator and constructor live
in the native runtime and must be rebuilt too.
