Limo CAD's native Bevy preview, built from clean source **`9b082687fc2db8c676185c988d4edec801c0ec1a`**, using **Bevy 0.20.0-rc.2** and Rust **1.99.0**. Application version remains **0.2.2**; the channel identifies this build as **`bevy-preview-0.2.2-20261004.1`**.

This replaces the October 2 Bevy preview. The newer source includes the System Light/Dark appearance fix, the Windows title-bar icon binding, preserved finished-sketch Undo/Redo across inactive-tab eviction, updated material catalog normalization and temperature units, and the Rust-only desktop/tooling cleanup. UI and MCP actions use the shared native command and document lifecycle paths.

The native desktop uses Bevy and OpenCASCADE's 7.9 ABI. Tauri, desktop WebViews, React/npm applications and their build tooling are removed from this integration. The separate Bevy WebAssembly host and geometry-service transport remain unfinished; this preview is a native desktop release.

## Downloads and checks

- **Windows x64 portable ZIP:** [noBS-CAD-0.2.2-windows-x64.zip](https://github.com/jackControls/Limo-CAD/releases/download/bevy-preview-0.2.2-20261004.1/noBS-CAD-0.2.2-windows-x64.zip)
- **Ubuntu 26.04 x64 DEB:** [noBS.CAD_0.2.2_amd64.deb](https://github.com/jackControls/Limo-CAD/releases/download/bevy-preview-0.2.2-20261004.1/noBS.CAD_0.2.2_amd64.deb)

The Windows x64 package was rebuilt from this exact clean source on Thunder. Both the candidate and installed package passed SDK-free headless and desktop MCP lifecycle checks: automatic live-document binding, real geometry/export, Save, preservation of retained unsaved work, disconnect survival and guarded window shutdown. These checks used private fixture documents.

The Ubuntu DEB passed headless MCP, desktop MCP, X11 native input/render checks and Wayland-desktop lifecycle/recipe checks through XWayland in the [tagged desktop package run](https://github.com/jackControls/Limo-CAD/actions/runs/37232261112/job/111525874975). The restored-window and Unicode-field captures were reviewed. Both package reports identify the exact clean source and preview channel.

Each attached package has a matching SHA-256 file; `SHA256SUMS.txt` contains the combined checksums. `build-receipt.json` records source, package hashes and qualification. The Windows ZIP is the verified local rebuild installed on Thunder. The independently built hosted Windows x64 package also passed headless/desktop MCP and native input/render checks on the same clean source; its restored-window capture was reviewed.

Windows ARM64 and AppImage remain withheld while their [tagged build jobs](https://github.com/jackControls/Limo-CAD/actions/runs/37232261112) run. macOS built and Developer ID signed, but Apple notarization returned HTTP 403 for a missing or expired team agreement. The Apple account owner must resolve that agreement before macOS distribution. No earlier-source or unqualified package is attached.

Windows packages are unsigned. Extract the whole ZIP and keep its DLLs, licenses and notices alongside `noBS-CAD.exe`. Windows 11 and a Direct3D 12 or Vulkan-capable graphics driver are required. The matching Microsoft Visual C++ v14 redistributable may be required.

Ubuntu 26.04 x64 uses the DEB package; run `sudo apt install --reinstall ./noBS.CAD_0.2.2_amd64.deb` when replacing another 0.2.2 installation. Source revision and build channel, rather than version alone, identify the installed build. Package and executable names retain the former noBS CAD name.

## Known limits

The integration is tracked in [PR #124](https://github.com/jackControls/Limo-CAD/pull/124) and has not merged into main. Main changes remain subject to external review and Jack's approval. The broad validation suite was not run locally for this deployment.

Physical printing, screen-reader speech, 6DoF hardware and monitor/DPI transitions have no fresh device qualification on this source. OpenCASCADE 7.9.3's reported matrix-copy arithmetic issue remains tracked separately; the ABI-changing 8.x SDK migration is outside this release.

Keep backups of important pre-alpha `.nbcad` documents. [Installation guide](https://github.com/jackControls/Limo-CAD/blob/feat/bevy-interface/docs/INSTALL.md) and [transition status](https://github.com/jackControls/Limo-CAD/blob/feat/bevy-interface/docs/native-transition-status.md).
