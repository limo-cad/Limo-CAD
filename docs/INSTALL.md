# Install Limo CAD's Bevy preview

The current Bevy source builds the **Limo CAD** application and registers the
MCP as **`limo-cad`**. The published October 4 preview below predates that rename
and retains its original package filenames. Do not rename downloaded files to
match source-build instructions.

New source builds use `Limo-CAD.exe` on Windows, `limo-cad` on Linux/macOS,
`.limo` projects and `limo-cad://` recipe links. The instructions below match
the published preview's executable, `.nbcad` files and `nbcad://` links.

Download the **[Bevy rc.2 preview](https://github.com/limo-cad/Limo-CAD/releases/tag/bevy-preview-0.2.2-20261004.1)**
for **Windows x64 or Ubuntu 26.04 x64**. It includes the native desktop, Scripts
library and MCP server; no compiler or agent is needed to use it. Choose an
application package, not GitHub's **Source code** archives.

This published preview uses application version **0.2.2**, source **`9b082687`**
and channel **`bevy-preview-0.2.2-20261004.1`**. It is separate from the older
stable `v0.2.2` release. Package and executable names still use **noBS CAD**,
the former product name. See
[transition status](native-transition-status.md) for newer source and package
qualification. The Bevy browser application is still unfinished.

This is pre-alpha software. Keep the original copy of an important CAD
project when trying a new build. The release notes record the source revision
and package checks.

Find the installed version, source revision and build channel under
**File → Settings → About Limo CAD**. Include that build text in a bug report.

## Windows

1. Download `noBS-CAD-0.2.2-windows-x64.zip` for an Intel/AMD PC.
2. Install the [Microsoft Visual C++ v14 x64 Redistributable](https://aka.ms/vc14/vc_redist.x64.exe)
   if needed.
3. Extract **all** files into a folder you can keep, then open `noBS-CAD.exe`.
   Keep its DLLs and notices in that folder. The executable is not yet
   code-signed, so if SmartScreen shows **Windows protected your PC**, choose
   **More info → Run anyway**.

Use **Windows 11**. The native Bevy application needs
a graphics adapter and driver that support **Direct3D 12 or Vulkan**;
see [wgpu's platform support](https://github.com/gfx-rs/wgpu#supported-platforms).
To update CAD, close it and extract the new ZIP to a separate folder;
your saved projects can stay where they are.

<details>
<summary>Windows startup help</summary>

Check the package architecture and Visual C++ runtime if a DLL error appears.
For a blank viewport or graphics-adapter error, [update the display driver](https://support.microsoft.com/en-us/windows/update-drivers-through-device-manager-in-windows-ec62f46c-ff14-c91d-eead-d7126dc1f7b6)
through Windows Update or the GPU manufacturer's support site, then restart CAD.
The published Windows x64 package passed SDK-free headless and desktop MCP
checks on Thunder, including Save and guarded shutdown. An independent hosted
build of the same clean source also passed native-input checks in the
[tagged package run](https://github.com/limo-cad/Limo-CAD/actions/runs/37232261112).
This preview has no verified Windows 10 minimum.
This preview has no setup installer or automatic updater.

See [Windows packaging and troubleshooting](WINDOWS_PACKAGING.md) for details.

</details>

## Ubuntu

On **Ubuntu 26.04 LTS, x86_64**, download the `.deb` and run this from its folder:

```sh
sudo apt install ./noBS.CAD_0.2.2_amd64.deb
```

Open **Limo CAD** from the application launcher. Vulkan support is required.
The package is checked on X11 and Ubuntu's Wayland desktop through XWayland.

To update CAD, close it, download the new `.deb`, then run this from the new
download's folder:

```sh
sudo apt install --reinstall ./noBS.CAD_0.2.2_amd64.deb
```

[`--reinstall`](https://manpages.ubuntu.com/manpages/resolute/man8/apt-get.8.html)
asks APT to replace the installed package even when the downloaded file and the
installed one share the same version number.
Reopen CAD and check **File → Settings → About Limo CAD** against the release's
source revision. Your saved project files can stay where they are.

See [Linux dependencies and troubleshooting](LINUX_PACKAGING.md).

## Targets still withheld

This preview has no macOS, Windows ARM64 or AppImage download. macOS
notarization needs the Apple account owner to resolve a team-agreement error;
ARM64 native-input checks and AppImage package qualification remain open.
An older package with the same **0.2.2** application version is not an equivalent
Bevy preview. Developers can [build from source](DEVELOPMENT.md); those local
builds do not establish published platform qualification.

## Make your first part

Start with the short **Sketch, extrude, ease the edges** lesson (`fillet-basics`).
The flagship assemblies have much longer construction sequences; the video edits
on the README are accelerated.

1. Open CAD, select **Scripts**, choose **Sketch, extrude, ease the edges**, and
   click **Run in new design**. Your existing design stays in its own tab.
2. Let the lesson and its final checks finish. The reference result is a
   **60 × 30 × 12 mm** block with **2 mm** rounds on its four top edges.
3. In the feature history, double-click the **Extrude** feature. Change
   **Distance** from **12** to **18 mm** and confirm the edit. The block becomes
   taller while retaining its sketch and rounded top edges.
4. Use **File → Save As** to save `first-part.nbcad` in a folder you can find.
   Close that design tab, then use **File → Open** to reopen the saved file.
   Double-click the extrusion again and confirm that its distance is **18 mm**.

**Save script as…** saves the construction recipe (`.limo.jsonc`). **File → Save**
saves the editable CAD project (`.limo`). Keep the project when you want to continue
modeling; keep the recipe when you want to replay its construction.

To inspect a flagship without waiting for construction, download its `.limo`
from the [showcase media release](https://github.com/limo-cad/Limo-CAD/releases/tag/showcase-v0.2.0)
and use **File → Open**. Browser **Open recipe** links load source into Scripts;
review it before choosing **Run in new design**. Launch the installed app once
before using those browser links, so it can register its recipe handler. New
source builds register `limo-cad`; this published preview registers `nbcad`.

![The completed 12 mm lesson and its editable feature history](assets/showcase/first-part.png)

## Connect an MCP agent

Local **stdio MCP is always available** in the application. A normal launch opens
CAD with the same interface available to an agent; `--headless` suppresses the
window. No separate MCP mode or server installation is needed.

Agent setup is optional. Set your agent's server command to the installed
application executable. The configurations below use `--headless` to start an
independent worker without opening a window every time the agent connects.
That worker can attach to an existing CAD document for live work. Closing its
stdio input ends a headless worker; closing the same input on a visible app
leaves the CAD window and its documents open.

To have the agent open and drive its own visible CAD window, omit `args` instead.
Once that window is ready, ordinary tools target its visible document. They do
not silently create a separate headless model. `cad_attach` can explicitly select
another document; after `cad_detach`, the agent must select a target again.

### Cursor

Edit your user MCP configuration: `%USERPROFILE%/.cursor/mcp.json` on Windows,
or `~/.cursor/mcp.json` on Linux. Add `limo-cad` under `mcpServers`, keeping
any existing servers. This Windows example uses the extracted application:

```json
{
  "mcpServers": {
    "limo-cad": {
      "command": "C:/YOUR/EXTRACTED/FOLDER/noBS-CAD.exe",
      "args": ["--headless"]
    }
  }
}
```

### VS Code

Run **MCP: Open User Configuration** from the Command Palette to open the active
profile's `mcp.json`. For the default profile, the file is
`%APPDATA%/Code/User/mcp.json` on Windows or
`~/.config/Code/User/mcp.json` on Linux. A workspace configuration instead belongs
in `.vscode/mcp.json`.

VS Code uses **`servers`**, with a `stdio` entry:

```json
{
  "servers": {
    "limo-cad": {
      "type": "stdio",
      "command": "C:/YOUR/EXTRACTED/FOLDER/noBS-CAD.exe",
      "args": ["--headless"]
    }
  }
}
```

Merge the entry into the chosen file; retain its other settings. See
[VS Code's MCP configuration reference](https://code.visualstudio.com/docs/agents/reference/mcp-configuration)
for custom profiles and configuration options.

### Choose the executable and try it

Use the absolute path to your installed executable. On other platforms, keep
`"args": ["--headless"]` and change `command`:

- **Ubuntu DEB:** `/usr/bin/nbcad`

Reload the client's MCP servers and confirm that **limo-cad** is available. Open
CAD normally, then ask your agent:

> Use Limo CAD to run the fillet-basics lesson in a new design in the open CAD
> window. Preserve my existing documents. After the final checks pass, change
> the stock extrusion from 12 to 18 mm, inspect the result and keep it open.

The expected reference and save/reopen steps are in
[Make your first part](#make-your-first-part). Discover and
attach to the intended live design; an unattached MCP server owns a separate
headless document. Keep the application and any separate worker on the same release
when updating.

The server runs locally and needs no cloud account. Your agent/model provider
has its own setup and data-handling choices.

<details>
<summary>Other clients and developer MCP setup</summary>

Some clients use a different configuration container; the executable and
arguments stay the same. Keep the complete Windows folder together.
Packaged MCP does not require an OCCT SDK or developer `PATH` setup.
In Windows JSON paths, use forward slashes or escape backslashes.

Stdout carries MCP JSON-RPC; diagnostics go to stderr. See the
[server guide](../mcp-server/README.md) and [live-control contract](mcp-harness.md)
for the interface. Developers building a separate server should use the
[developer guide](DEVELOPMENT.md#standalone-mcp-server), not a second application
installation.

</details>

<details>
<summary>Verify a download's SHA-256 checksum</summary>

Download the package's adjacent `.sha256` asset into the same folder.
On Windows, compare the following values (hash letter case does not matter):

```powershell
Get-FileHash .\noBS-CAD-0.2.2-windows-x64.zip -Algorithm SHA256
Get-Content .\noBS-CAD-0.2.2-windows-x64.zip.sha256
```

On Ubuntu use `sha256sum -c PACKAGE.sha256`, substituting the downloaded
checksum filename.

</details>
