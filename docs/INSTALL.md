# Install Limo CAD 0.3.0-rc.2

Use the packages on the **[0.3.0-rc.2 pre-release page](https://github.com/limo-cad/Limo-CAD/releases/tag/v0.3.0-rc.3)**
once publication has completed. That page lists the source revision, checksums,
build evidence and qualification limits. Choose an application package, not
GitHub's **Source code** archives. This candidate does not replace the stable
release marked Latest.

This is pre-alpha software. Keep an original copy of important CAD projects.
The `.limo` container remains version 1; the current model schema is 15. The app
also reads legacy `.nbcad` projects; older builds may reject newer saved models.

| Platform | Package |
| --- | --- |
| Windows 11 x64 | `Limo-CAD-0.3.0-rc.3-windows-x64.zip` |
| Windows 11 ARM64 | `Limo-CAD-0.3.0-rc.3-windows-arm64.zip` |
| macOS Apple silicon | `Limo.CAD_0.3.0-rc.3_aarch64.dmg` |
| Ubuntu 26.04 x64 | `Limo.CAD_0.3.0-rc.3_amd64.deb` |
| Linux x64, glibc 2.35+ | `Limo.CAD_0.3.0-rc.3_amd64.AppImage` |

The release workflow publishes only after all five packages and their checks
succeed. A listed target is not a claim that the unfinished manual UI/MCP
qualification ledger is complete. See the release notes for known gaps.

Find the installed version, source revision and build channel under
**File → Settings → About Limo CAD**. The executable should report
**0.3.0-rc.2** and channel **v0.3.0-rc.2**; include that build text in bug reports.
Apple bundle metadata uses `0.3.0`, and Debian package metadata uses
`0.3.0~rc.2`, while the executable retains the complete candidate version.

## Windows

1. Download the ZIP for your PC's architecture: x64 for Intel/AMD, ARM64 for ARM.
2. Install the matching Microsoft Visual C++ v14 Redistributable if needed:
   [x64](https://aka.ms/vc14/vc_redist.x64.exe) or
   [ARM64](https://aka.ms/vc14/vc_redist.arm64.exe).
3. Extract **all** files into a folder you can keep, then open `Limo-CAD.exe`.
   Keep its DLLs and notices in that folder. The Windows executable is not
   code-signed; verify its checksum and source before accepting a SmartScreen prompt.

Use **Windows 11** and a graphics adapter/driver with **Direct3D 12 or Vulkan**.
To update, close CAD and extract the new ZIP into a separate folder. Your saved
projects can stay where they are. There is no setup installer or automatic updater.
See [Windows packaging and troubleshooting](WINDOWS_PACKAGING.md).

## macOS

On an **Apple silicon Mac**, open the signed and notarized DMG, copy **Limo CAD**
to **Applications**, then launch it from there. Close CAD before replacing an
older application. This release does not provide an Intel Mac package.

## Ubuntu

On **Ubuntu 26.04 LTS, x86_64**, download the DEB and run this from its folder:

```sh
sudo apt install ./Limo.CAD_0.3.0-rc.3_amd64.deb
```

Open **Limo CAD** from the application launcher. Vulkan support is required.
The package is checked on X11 and Wayland through XWayland. The `limo-cad`
package replaces the older `nbcad` package; your saved projects stay separate.

## Portable Linux

On an **x86_64 glibc 2.35+** system with a working Vulkan driver, download the
AppImage, verify its checksum, then run:

```sh
chmod +x Limo.CAD_0.3.0-rc.3_amd64.AppImage
./Limo.CAD_0.3.0-rc.3_amd64.AppImage
```

See [Linux dependencies and troubleshooting](LINUX_PACKAGING.md).

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
4. Use **File → Save As** to save `first-part.limo` in a folder you can find.
   Close that design tab, then use **File → Open** to reopen the saved file.
   Double-click the extrusion again and confirm that its distance is **18 mm**.

**Save script as…** saves the construction recipe (`.limo.jsonc`). **File → Save**
saves the editable CAD project (`.limo`). Keep the project when you want to continue
modeling; keep the recipe when you want to replay its construction.

To inspect a flagship without waiting for construction, download its `.limo`
from the [showcase media release](https://github.com/limo-cad/Limo-CAD/releases/tag/showcase-v0.2.0)
and use **File → Open**. Browser **Open recipe** links load source into Scripts;
review it before choosing **Run in new design**. Launch the installed app once
before using those browser links, so it can register its recipe handler. This
release registers the `limo-cad://` recipe handler and retains legacy `nbcad://` links.

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
      "command": "C:/YOUR/EXTRACTED/FOLDER/Limo-CAD.exe",
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
      "command": "C:/YOUR/EXTRACTED/FOLDER/Limo-CAD.exe",
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

- **Ubuntu DEB:** `/usr/bin/limo-cad`
- **macOS:** `/Applications/Limo CAD.app/Contents/MacOS/limo-cad`
- **Linux AppImage:** the absolute path to your executable AppImage

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
Get-FileHash .\Limo-CAD-0.3.0-rc.3-windows-x64.zip -Algorithm SHA256
Get-Content .\Limo-CAD-0.3.0-rc.3-windows-x64.zip.sha256
```

On Ubuntu use `sha256sum -c PACKAGE.sha256`, substituting the downloaded
checksum filename.

</details>
