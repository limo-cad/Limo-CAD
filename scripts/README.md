# Test ownership

The desktop release is the native Bevy application. Repository build and
validation commands run through Rust `cargo xtask`. Start with
`cargo xtask check --scope engine` or `--scope desktop`; add `--clippy` for
linting. These commands do not run tests or drive a CAD window.

The desktop renderer packet and fake desktop IPC harnesses were removed with
their adapters. Their workflow areas now belong to these existing native
fixtures in `xtask/src`:

- Extrude/profile previews and internal datum support: `native-lifecycle`,
  `native-support`, and `native-planes`.
- Revolve, Sweep, Loft, and Rib: `native-build`.
- Multi-position holes and internal threads: `native-hole`; external threads:
  `native-thread`.
- CAM setup/tool/operation editing, playback, and private tool/post libraries:
  `native-cam`, `native-cam-platform`, `native-cam-geometry`, and `native-cam-nc`.
- Native camera, interaction, controls, and OS input: `native-view` and
  `native-platform`. These replace the ownership of the former browser-side
  Bevy interaction harness, not its implementation.

These names identify where validation belongs; they do not claim that every
retired case has identical coverage or that a native fixture has passed.
Native captures and platform runs supply separate evidence. Browser CPU
geometry assertions are not proof of rendered Bevy pixels.

The former browser app and its JavaScript harnesses were retired. The existing
Rust engine facade uses `cargo xtask build-wasm` and `cargo xtask smoke-wasm`;
it does not yet provide the shared Bevy UI or OCCT WASM kernel. See
[browser work remaining](../web/README.md).

Native fixture commands are dispatched by `cargo xtask test-mcp` and require
their documented owned session/output arguments. Platform input fixtures require
an isolated desktop or CI runner, not an operator's active session.
