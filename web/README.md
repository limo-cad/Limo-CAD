# Bevy browser host

The React application, npm dependencies and its browser drivers are retired.
The browser application will use the same Rust Bevy UI as the desktop.

`cargo xtask build-wasm` builds the shared engine facade into `web/engine/`.
`cargo xtask smoke-wasm` exercises that facade and its generated bindings in
Chrome. Both commands work from Windows, Linux and macOS with wasm-pack installed.
Generated JavaScript is wasm-bindgen runtime glue; it is not a second application.

This engine facade does not yet include the OCCT B-rep kernel or the Bevy UI.
A usable browser application still requires the shared Bevy WASM host, browser
file/storage/dialog services and a geometry-service connection. The planned first
host offloads geometry to native Rust/OCCT through the shared native-engine host.
An optional in-browser kernel requires an OCCT WASM port; that port is separate
from the native-service approach. Neither complete browser path is shipped yet.
The native MCP and inbox/heartbeat bridge remain Rust-owned desktop services and
do not load frontend assets.
