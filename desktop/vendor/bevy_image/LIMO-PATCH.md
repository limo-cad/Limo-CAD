# Use the patched native zstd decoder

This is the published `bevy_image 0.20.0` source from Bevy commit
`b3bdd75becaffe9a16677cf5bae5bd6d198f052b`. The normalized registry manifest and
original MIT/Apache licenses are retained. Original registry archive SHA256:
`bb818dd8d705809d54adea08e346edce81a51ddbf06406ddf12a72ee467a3855`.

The only change is in `Cargo.toml`: the optional `zstd` dependency requires
`0.14.3` instead of `0.13.3`. No source file is changed; `rustfmt.toml`
preserves the published formatting, as in the other vendored Bevy crates.

`bevy_image 0.20.0` requires `zstd ^0.13.3`, and no patched 0.13 release
exists. The locked `zstd 0.13.3`, `zstd-safe 7.3.0` and
`zstd-sys 2.1.1+zstd.1.5.7` are affected by
[RUSTSEC-2026-0391](https://rustsec.org/advisories/RUSTSEC-2026-0391.html),
[RUSTSEC-2026-0398](https://rustsec.org/advisories/RUSTSEC-2026-0398.html),
[RUSTSEC-2026-0403](https://rustsec.org/advisories/RUSTSEC-2026-0403.html) and
[RUSTSEC-2026-0410](https://rustsec.org/advisories/RUSTSEC-2026-0410.html)
(reported 2026-10-16 and issued 2026-10-26), which fail the desktop RustSec gate.
`zstd 0.14.3` with
`zstd-safe 8.2.0` and `zstd-sys 2.1.2` is the patched line.

Bevy's only use of the crate under the desktop's `zstd_c` feature is
`zstd::decode_all` in `src/ktx2.rs`, whose signature is unchanged in 0.14.
Its implementation changes, including capped preallocation; signature
compatibility alone does not establish KTX2 decoding behavior.
The `zstd_rust` backend stays off because `ruzstd 0.8` is affected by
RUSTSEC-2026-0344 (see `desktop/Cargo.toml`).

The patch is selected only by the desktop workspace's `[patch.crates-io]`.
Remove it when the pinned upstream `bevy_image` accepts a patched zstd.
