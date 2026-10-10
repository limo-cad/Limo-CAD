# Configure CAD MCP clients

For a downloaded application, follow [Install → Connect an MCP agent](../INSTALL.md#connect-an-mcp-agent).
That setup uses the installed CAD executable with `--headless` and needs no source build.

`cargo xtask install-mcp` updates selected clients' user configurations and
preserves unrelated entries. It can use the installed Bevy application in place
and never creates a separate executable copy. It does not launch CAD or create a live
session.

## Installed Bevy application

Use the canonical installed executable, with its adjacent runtime libraries:

```text
cargo xtask install-mcp --clients codex,cursor,vscode,claude,opencode --no-build --binary ABSOLUTE_CAD_PATH --in-place --server-arg --headless --desktop ABSOLUTE_CAD_PATH
```

Replace `ABSOLUTE_CAD_PATH` with the installed executable on your OS. On this
Windows machine it is `C:/Users/jeffg/AppData/Local/limo-cad/bevy/Limo-CAD.exe`.
`--in-place` leaves the executable and runtime libraries together and adds no
SDK paths. `--server-arg` accepts a literal argument, including `--headless`,
and can be repeated. `--desktop` sets `LIMO_CAD_DESKTOP_BIN` for an explicit
`cad_interface launch`. Reload the client's MCP connection after installing.

GUI and MCP use the same Bevy binary. `--headless` runs an independent document
without a window; explicit attach selects a live document. Without that flag,
the application opens a Bevy window and its stdio MCP controls that window.

## Source iteration

On Windows, build and promote the current checkout into the canonical runtime:

```text
cargo xtask deploy-native --computer-control --restart --launch
cargo xtask install-mcp --clients cursor,codex
```

`deploy-native` also registers detected clients. Its default keeps local Rust
crates at optimization level 1 and dependencies at level 3, using incremental
compilation. `--release` selects full optimization. `--restart` explicitly
terminates GUI/MCP workers without saving; otherwise a changed runtime that is
still running blocks promotion. Build or provenance failures preserve the old
installation and stop before launching or testing it. The command currently
supports Windows runtime promotion; use explicit portable executables on Unix.
The opt-in `--computer-control` enables the Rust OS-input tool for UI audits;
normal builds leave it disabled. Automatic rebuilds preserve the installed mode.

A dry run discovers clients and prints planned configuration without building,
copying or writing:

```text
cargo xtask install-mcp --dry-run
```

A real install requires `--clients`. Supported names are `codex`, `cursor`,
`vscode`, `claude` and `opencode`; absent clients are skipped. Explicit executable
selection always requires `--in-place`, so it cannot create a second copy.

## Configuration destinations

The utility detects the existing user configuration, rather than writing a
committed workspace file:

- **Codex:** `$CODEX_HOME/config.toml`, default `~/.codex/config.toml`, with
  `mcp_servers.limo-cad`. TOML comments and unrelated tables are preserved.
  Malformed TOML is refused without logging its contents.
- **Cursor:** `~/.cursor/mcp.json`, with `mcpServers.limo-cad`.
- **VS Code:** the detected Code or Code Insiders user `mcp.json`, with
  `servers.limo-cad` and `"type": "stdio"`. Default-profile locations are
  `%APPDATA%/Code/User/mcp.json` on Windows,
  `~/Library/Application Support/Code/User/mcp.json` on macOS and
  `~/.config/Code/User/mcp.json` on Linux.
- **Claude Code / Claude Desktop:** detected `~/.claude.json` and/or
  `claude_desktop_config.json`, with `mcpServers.limo-cad`.
- **OpenCode v2:** detected `opencode.json` under its configuration directory,
  with `mcp.servers.limo-cad`.

On Windows, `~` means `%USERPROFILE%`. The
[application setup guide](../INSTALL.md#connect-an-mcp-agent) owns the copyable
manual Cursor and VS Code configurations. Keep those formats separate.

## Binary and runtime resolution

Default installation builds the desktop first and installs the executable Cargo
reports. GUI and MCP share this single runtime and its adjacent libraries.
`--no-build` verifies the managed installation's source and payload hashes,
including an explicit canonical `--binary` path. Managed machines reject other
executable bindings. Without a managed installation, `--binary PATH --in-place`
configures a portable executable without copying it.

The shared-runtime entry includes `LIMO_CAD_LOCAL_RUNTIME` and
`LIMO_CAD_DESKTOP_BIN` pointing to the same executable. Managed desktop launch
always selects that runtime. The executable also sets its own desktop path,
preventing an inherited stale desktop override from splitting GUI and MCP.

The managed installation records its checkout in `runtime-manifest.json`.
Local `test-mcp`, `verify-package-mcp`, `run-script` and `cad-call` commands build
and promote that checkout before selecting the canonical executable. Automatic
commands retain the recorded source binding; only `deploy-native` can change it.
Failed promotion stops the command. Hosted GitHub package checks retain explicit
isolated artifact selection.

## Existing configurations

The default installer removes retired `nobs-cad`, `noBS-CAD` and `nbcad` server entries and writes one `limo-cad` entry. Unrelated servers are preserved. A custom `--server-name` leaves the retired names untouched. It backs up an existing
file to `*.bak.<pid>`, writes through a temporary file and preserves portable
permissions. Repeated client names are processed once.

Empty files, Codex TOML and plain JSON are accepted. JSONC comments are rejected so a
pretty-print rewrite cannot silently discard them. If a client uses commented
configuration, follow the manual setup guide and add the entry yourself.

## Verify and maintain

Confirm **limo-cad** appears in the client's server list and call
`cad_get_focus` or `cad_list_focus_areas`. Then use the
[first-part prompt](../INSTALL.md#choose-the-executable-and-try-it).
For dynamic tool discovery and live document selection, read the
[MCP interface](../../mcp-server/README.md) and [ownership guide](../mcp-harness.md).

Implementation lives in `xtask/src/install_mcp.rs`; `xtask/src/main.rs` routes
the command. New client support should include detection, the correct
configuration writer and focused tests. Run `cargo test --locked -p xtask install_mcp::tests::` for this
installer. Native CAD build and test commands remain in [DEVELOPMENT.md](../DEVELOPMENT.md).

## Local help (`cad_help` + knowledge resources)

After the server is installed, prefer:

1. MCP tool **`cad_help`** with actions `search` → `get` / `topics` (snippet-first;
   caps locked in [`machine-design-help-search.md`](../machine-design-help-search.md)).
2. MCP **`resources/list`** / **`resources/read`** on `limo-cad://knowledge/...` when the
   full markdown page is needed.

Rebuild/reinstall the MCP binary after knowledge or `crates/help` changes so the
embedded corpus matches the checkout:

```sh
cargo xtask install-mcp --clients cursor
```

Then re-Add / reload the Cursor MCP server (restart alone is not enough after a
binary replace).

