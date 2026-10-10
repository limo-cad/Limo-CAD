<table align="center"><tr>
<td align="center" width="120"><b>English</b></td>
<td align="center" width="120"><a href="README.zh-CN.md" hreflang="zh-CN">简体中文</a></td>
<td align="center" width="120"><a href="README.es.md" hreflang="es">Español</a></td>
<td align="center" width="120"><a href="README.de.md" hreflang="de">Deutsch</a></td>
</tr></table>

<p align="center"><img src="docs/assets/branding/limo-proposal-concept-en.png" alt="Limo CAD. Design with understanding." width="720"></p>

# Limo CAD

> **Design with understanding.**

**Easy-to-use parametric CAD that is free and open source, and always will be.**
Design mechanical parts, assemblies and drawings on your own machine, by hand
or with your AI agent, and keep every sketch and feature editable.

[![Bevy preview](https://img.shields.io/badge/Bevy-0.20.0--rc.2-blue)](https://github.com/limo-cad/Limo-CAD/releases/tag/bevy-preview-0.2.2-20261004.1)
[![License: LGPL 2.1+](https://img.shields.io/badge/license-LGPL%202.1%2B-blue)](LICENSE)
[![Discussions](https://img.shields.io/github/discussions/limo-cad/Limo-CAD?label=discussions)](https://github.com/limo-cad/Limo-CAD/discussions)

**Pre-alpha · Bevy rc.2 preview · Application version 0.2.2**
· [Preview notes, source and checks](https://github.com/limo-cad/Limo-CAD/releases/tag/bevy-preview-0.2.2-20261004.1)
· [Installation help](docs/INSTALL.md)

| Platform | Download |
|---|---|
| Windows 11 | [x64 ZIP](https://github.com/limo-cad/Limo-CAD/releases/download/bevy-preview-0.2.2-20261004.1/noBS-CAD-0.2.2-windows-x64.zip) |
| Linux | [Ubuntu 26.04 x64 DEB](https://github.com/limo-cad/Limo-CAD/releases/download/bevy-preview-0.2.2-20261004.1/noBS.CAD_0.2.2_amd64.deb) |

These rebuilt packages use clean source `9b082687`, including System appearance,
the Windows window icon and finished-sketch history across memory eviction.
Windows ARM64, macOS and AppImage remain withheld pending qualification. The Bevy
browser UI is still under development. Published filenames retain the former
product name. See [transition status](docs/native-transition-status.md).

Windows packages are unsigned. SmartScreen may warn on first launch; choose
**More info → Run anyway**. Keep backups of important pre-alpha projects.

## Why Limo CAD

- **Free and open source, for good.** No paid tier, no feature gates. The code
  is [LGPL 2.1 or later](LICENSE), so it stays open.
- **Easy to use.** Ease of use is one of the project's three priorities, next to
  reliability and performance. The [first-part lesson](#make-your-first-part)
  takes a few minutes.
- **Local and yours.** No account, subscription or cloud service. A whole
  project (parts, assemblies and drawings) lives in one `.limo` file.
- **Real parametric history.** Constrained sketches drive solid features;
  change a dimension and everything downstream rebuilds.
- **Agent-ready.** A built-in MCP server lets any MCP-compatible agent build and
  edit models, and what it makes is the same editable history you would make by hand.
- **Open formats.** STEP, STL and 3MF export; drawings to DXF and print-to-PDF.

## Made in Limo CAD

Each design was built from a blank document through MCP, and its sketches,
features and assembly relationships remain editable. **Watch** plays the build
recording; **Build loop** is a short accelerated excerpt.

<table>
<tr>
<td align="center" valign="top" width="33%">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#garden-bench"><img src="docs/assets/showcase/bench.png" alt="Garden bench with crowned back slats and rounded armrests"></a><br>
<b>Garden bench</b><br>
Change one picket dimension and the whole back updates. Frame, arms and joints stay editable.
</td>
<td align="center" valign="top" width="33%">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#d-screw-vise"><img src="docs/assets/showcase/vise.png" alt="Vise with a captured sliding jaw and compact D-screw handle"></a><br>
<b>Vise</b><br>
Turn the screw and the jaw follows. 100 mm jaws, 90 mm travel, six printed parts plus hardware.
</td>
<td align="center" valign="top" width="33%">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#vertical-axis-turbine"><img src="docs/assets/showcase/turbine.png" alt="Two-stage vertical-axis turbine with its bearing-supported shaft and generator drive"></a><br>
<b>Vertical-axis turbine</b><br>
Two Savonius stages on a bearing-supported shaft, driving a generator through a 4:1 drive.
</td>
</tr>
<tr>
<td align="center">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#garden-bench"><b>Watch</b></a>
· <a href="https://limo-cad.github.io/Limo-CAD/open.html#garden-bench">Open recipe</a><br>
<a href="examples/scripts/garden-bench.limo.jsonc">Source</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/bench.nbcad">.nbcad</a>
· <a href="docs/assets/showcase/bench-loop.gif">Build loop</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/bench-build-full.mp4">MP4</a>
</td>
<td align="center">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#d-screw-vise"><b>Watch</b></a>
· <a href="https://limo-cad.github.io/Limo-CAD/open.html#d-screw-vise">Open recipe</a><br>
<a href="examples/scripts/d-screw-vise.limo.jsonc">Source</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/vise.nbcad">.nbcad</a>
· <a href="docs/assets/showcase/vise-loop.gif">Build loop</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/vise-build-full.mp4">MP4</a>
</td>
<td align="center">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#vertical-axis-turbine"><b>Watch</b></a>
· <a href="https://limo-cad.github.io/Limo-CAD/open.html#vertical-axis-turbine">Open recipe</a><br>
<a href="examples/scripts/vertical-axis-turbine.limo.jsonc">Source</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/turbine.nbcad">.nbcad</a>
· <a href="docs/assets/showcase/turbine-loop.gif">Build loop</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/turbine-build-full.mp4">MP4</a>
</td>
</tr>
</table>

<!-- Print photos: add docs/assets/showcase/<design>-printed.jpg when supplied. -->

Recipe links load source into **Scripts** for review before running. To inspect
a completed design immediately, download its `.limo` and use **File → Open**.
These are development examples; physical fit and load qualification remain open.
[Designs, drawings and validation](docs/flagship-examples.md) · [All recipes](examples/scripts/README.md)

## Make your first part

After [installing CAD](docs/INSTALL.md), open **Scripts**, choose
**Sketch, extrude, ease the edges**, and select **Run in new design**.
The lesson builds a 60 × 30 × 12 mm block with rounded top edges.

When it finishes, double-click the extrusion in the feature history and change
its **Distance** from **12 to 18 mm**. Save the result as `first-part.limo`,
then reopen it to continue editing. [Step-by-step instructions](docs/INSTALL.md#make-your-first-part)

## Design, assemble, draw

Constrained sketches and reference geometry drive editable solid features.
Reuse parts in assemblies, define joints and check their motion and interference.
Keep the parts, assemblies and drawings together in one `.limo` project.

Assign per-body materials and colors, then export **3MF** for your slicer.
Material labels and color metadata assist the handoff; choose the actual print
profile in the slicer. **STEP** carries exact geometry and **STL** supplies mesh
export. Drawing sheets export to **DXF** and print/PDF.
[Assemblies](docs/ASSEMBLIES.md) · [Drawings and export coverage](docs/2D_DRAWINGS.md)

## Work with an agent

Local stdio MCP is always available in the installed application. Bring your preferred
MCP-compatible agent and model to build a part, edit an existing feature, inspect
an assembly or replay a demonstration. CAD keeps the same editable project
whether you use the tools yourself or ask an agent to use them.

[Connect your agent](docs/INSTALL.md#connect-an-mcp-agent), then try:

> Use Limo CAD to run the fillet-basics lesson in a new design in the open CAD
> window. Preserve my existing documents. After the final checks pass, change
> the stock extrusion from 12 to 18 mm, inspect the result and keep it open.

An agent is optional. **Scripts** can build a bundled example, explain its
chapters and show the construction with captions, camera moves and playback
controls. You can inspect and edit the recipe before running it.
[MCP interface](mcp-server/README.md) · [Recipes and playback](docs/native-scripts.md)
· [Engineering knowledge](knowledge/index.md)

## Help build it

Contributions are welcome. Our priorities are **reliability, performance and
ease of use**, in that order. Bring a part, a reproducible bug or a focused
improvement. [Contributing](CONTRIBUTING.md) · [Developer setup](docs/DEVELOPMENT.md)
· [Documentation](docs/INDEX.md)

Questions, ideas, or something you made? Start a thread in
[Discussions](https://github.com/limo-cad/Limo-CAD/discussions). If Limo CAD
is useful to you, a star helps other people find it.

We are working toward guided design lessons and conversational wizards, and
developing an early **3-axis CAM** foundation with toolpath generation, stock
simulation and machine-aware posts. It is not production-safe CAM; review the
[CAM guides and safety limits](docs/cam/README.md). Strength analysis remains a
future capability. [Project direction](docs/goals.md)

## Open-source foundations

- **[Open CASCADE Technology](https://github.com/Open-Cascade-SAS/OCCT)** — geometry and CAD interchange.
- **[Bevy](https://bevy.org/) and [wgpu](https://wgpu.rs/)** — native desktop interface and rendering.
- **[Rust](https://rust-lang.org/)** — modeling, assemblies and recipe execution.

Thanks also to [FreeCAD](https://www.freecad.org/) and the wider open-source CAD community.

## License

[GNU LGPL 2.1 or later](LICENSE). Free to use, inspect and improve.

<details>
<summary>Third-party notices and 3D mouse support</summary>

Dependency licenses and attribution live in [Third-party notices](THIRD_PARTY_NOTICES.md).
Icon sources are recorded in [Icon provenance](docs/ICON_PROVENANCE.md).
Peer CAD projects have their own licenses; see [contribution guidance](CONTRIBUTING.md#license--borrow).

The Bevy desktop supports 3Dconnexion SpaceMouse devices through native HID input.
Limo CAD is independent and is not affiliated with, endorsed by or certified by
3Dconnexion. 3Dconnexion and SpaceMouse are trademarks or registered trademarks
of 3Dconnexion. 3D input device development tools and related technology are
provided under license from 3Dconnexion. © 3Dconnexion 1992–2020. All rights reserved.

</details>
