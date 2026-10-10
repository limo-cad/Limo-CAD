//! Detect agent clients and migrate their CAD entry to `limo-cad`.
//!
//! Explicit clients and backups keep configuration changes reviewable. Packaged
//! desktops can run in place, retaining their adjacent runtime libraries.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DEFAULT_SERVER_NAME: &str = "limo-cad";
const RETIRED_SERVER_NAMES: &[&str] = &["nobs-cad", "noBS-CAD", "nbcad"];

#[derive(Debug, Clone)]
pub struct Options {
    pub dry_run: bool,
    pub build: bool,
    pub binary: Option<PathBuf>,
    pub in_place: bool,
    pub server_args: Vec<String>,
    pub desktop: Option<PathBuf>,
    pub clients: Option<Vec<ClientKind>>,
    pub server_name: String,
}

impl Options {
    pub fn parse(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut options = Self {
            dry_run: false,
            build: true,
            binary: None,
            in_place: false,
            server_args: Vec::new(),
            desktop: None,
            clients: None,
            server_name: DEFAULT_SERVER_NAME.to_string(),
        };

        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--dry-run" => options.dry_run = true,
                "--no-build" => options.build = false,
                "--in-place" => options.in_place = true,
                "--server-arg" => options.server_args.push(
                    args.next()
                        .ok_or_else(|| anyhow!("--server-arg requires a literal argument"))?,
                ),
                "--desktop" => {
                    options.desktop = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| anyhow!("--desktop requires a path"))?,
                    ))
                }
                "--binary" => {
                    let path = args
                        .next()
                        .ok_or_else(|| anyhow!("--binary requires a path"))?;
                    options.binary = Some(PathBuf::from(path));
                }
                "--clients" => {
                    let list = args
                        .next()
                        .ok_or_else(|| anyhow!("--clients requires a comma-separated list"))?;
                    options.clients = Some(parse_clients(&list)?);
                }
                "--server-name" => {
                    options.server_name = args
                        .next()
                        .ok_or_else(|| anyhow!("--server-name requires a value"))?;
                }
                "--help" | "-h" => bail!("help requested"),
                other => bail!("unknown install-mcp option '{other}'"),
            }
        }
        options.validate()?;
        Ok(options)
    }

    /// Real installs require an explicit `--clients` list. Dry-run may omit it
    /// to discover/print detected clients only.
    pub fn validate(&self) -> Result<()> {
        if self.binary.is_some() && !self.in_place {
            bail!("--binary requires --in-place; MCP uses the installed CAD executable without creating another copy");
        }
        if self.in_place && self.binary.is_none() {
            bail!("--in-place requires --binary pointing to an installed executable");
        }
        if !self.dry_run && self.clients.is_none() {
            bail!(
                "--clients is required for install writes \
                 (use --dry-run to discover detected clients without writing)"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKind {
    Codex,
    Cursor,
    VsCode,
    Claude,
    OpenCode,
}

impl ClientKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::VsCode => "vscode",
            Self::Claude => "claude",
            Self::OpenCode => "opencode",
        }
    }

    fn parse(name: &str) -> Result<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "codex" => Ok(Self::Codex),
            "cursor" => Ok(Self::Cursor),
            "vscode" | "code" | "vs-code" => Ok(Self::VsCode),
            "claude" => Ok(Self::Claude),
            "opencode" | "open-code" => Ok(Self::OpenCode),
            "grok" | "xai" => {
                bail!("client '{name}' is not supported (no Grok until an official MCP contract)")
            }
            other => {
                bail!("unknown client '{other}' (supported: codex,cursor,vscode,claude,opencode)")
            }
        }
    }
}

fn parse_clients(list: &str) -> Result<Vec<ClientKind>> {
    let mut out = Vec::new();
    for part in list.split(',') {
        if part.trim().is_empty() {
            continue;
        }
        let kind = ClientKind::parse(part)?;
        if !out.contains(&kind) {
            out.push(kind);
        }
    }
    if out.is_empty() {
        bail!("--clients list is empty");
    }
    Ok(out)
}

#[derive(Debug, Clone)]
struct ServerLaunch {
    command: PathBuf,
    args: Vec<String>,
    env: Map<String, Value>,
}

pub fn run(mut options: Options) -> Result<()> {
    options.validate()?;

    let repo_root = repo_root()?;
    let managed = crate::deploy_native::managed_install_exists()?;
    if options.binary.is_none() {
        let binary = if options.dry_run {
            crate::deploy_native::installed_executable()?
        } else if options.build {
            let source = if managed {
                crate::deploy_native::managed_source_checkout()?
            } else {
                repo_root.clone()
            };
            crate::deploy_native::prepare_runtime(
                &source,
                &crate::deploy_native::BuildOptions::default(),
            )?
        } else {
            crate::deploy_native::verify_installed_runtime()?
        };
        options.desktop = Some(binary.clone());
        options.binary = Some(binary);
        options.in_place = true;
        if !options.server_args.iter().any(|arg| arg == "--headless") {
            options.server_args.push("--headless".into());
        }
    }
    if managed && !options.dry_run {
        let canonical = crate::deploy_native::verify_installed_runtime()?;
        if options
            .binary
            .as_ref()
            .map(|path| normalize_path(path.clone()))
            != Some(normalize_path(canonical.clone()))
        {
            bail!("This machine uses one managed CAD runtime; run cargo xtask deploy-native to select another source checkout");
        }
        if options.server_args.iter().any(|arg| arg != "--headless") {
            bail!("The managed MCP runtime accepts only --headless");
        }
        options.binary = Some(canonical.clone());
        options.desktop = Some(canonical);
        options.server_args = vec!["--headless".into()];
    }
    let binary = resolve_binary(&repo_root, &options)?;
    let mut environment = Map::new();
    if let Some(desktop) = &options.desktop {
        if !options.dry_run && !desktop.is_file() {
            bail!("--desktop executable does not exist: {}", desktop.display());
        }
        environment.insert(
            "LIMO_CAD_DESKTOP_BIN".into(),
            Value::String(path_string(&normalize_path(desktop.clone()))),
        );
        if normalize_path(desktop.clone()) == normalize_path(binary.clone()) {
            environment.insert(
                "LIMO_CAD_LOCAL_RUNTIME".into(),
                Value::String(path_string(&normalize_path(binary.clone()))),
            );
        }
    }
    let launch = ServerLaunch {
        command: binary,
        args: options.server_args.clone(),
        env: environment,
    };

    println!("MCP binary: {}", launch.command.display());
    if options.dry_run {
        println!("mode: dry-run (zero build/copy/write)");
    }

    let discover_all = options.clients.is_none();
    let wanted = options.clients.unwrap_or_else(|| {
        vec![
            ClientKind::Codex,
            ClientKind::Cursor,
            ClientKind::VsCode,
            ClientKind::Claude,
            ClientKind::OpenCode,
        ]
    });

    if discover_all {
        println!("discover: scanning all supported clients (no --clients)");
    }

    let mut installed = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for kind in wanted {
        let targets = discover_targets(kind);
        if targets.is_empty() {
            println!(
                "skip  {:<10} (not detected — no user config/marker present)",
                kind.as_str()
            );
            skipped += 1;
            continue;
        }
        for target in targets {
            if options.dry_run {
                println!(
                    "plan  {:<10} {}  ({})",
                    kind.as_str(),
                    target.path.display(),
                    target.format.label()
                );
                installed += 1;
                continue;
            }
            match upsert_target(&target, &options.server_name, &launch) {
                Ok(action) => {
                    println!(
                        "{:<5} {:<10} {}  ({})",
                        action.verb(),
                        kind.as_str(),
                        target.path.display(),
                        target.format.label()
                    );
                    installed += 1;
                }
                Err(error) => {
                    eprintln!(
                        "error {:<10} {}: {error:#}",
                        kind.as_str(),
                        target.path.display()
                    );
                    failed += 1;
                }
            }
        }
    }

    println!(
        "done: {installed} upsert(s), {skipped} client family(ies) skipped, {failed} error(s)"
    );
    if installed == 0 && failed == 0 {
        println!(
            "hint: install/open a client once so its user config directory exists, then re-run"
        );
    } else if !options.dry_run && installed > 0 {
        println!("restart each client (or reload MCP) to pick up the new server");
    }
    if failed > 0 {
        bail!("{failed} client config(s) failed; see errors above");
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum ConfigFormat {
    CodexToml,
    /// `{ "mcpServers": { "name": { command, args, env } } }`
    McpServers,
    /// VS Code `{ "servers": { "name": { type, command, args, env } } }`
    VsCodeServers,
    /// OpenCode v2 `{ "mcp": { "servers": { "name": { type, command, ... } } } }`
    OpenCodeMcp,
}

impl ConfigFormat {
    fn label(self) -> &'static str {
        match self {
            Self::CodexToml => "mcp_servers TOML",
            Self::McpServers => "mcpServers",
            Self::VsCodeServers => "servers",
            Self::OpenCodeMcp => "opencode mcp",
        }
    }
}

#[derive(Debug, Clone)]
struct Target {
    path: PathBuf,
    format: ConfigFormat,
}

#[derive(Debug, Clone, Copy)]
enum Action {
    Created,
    Updated,
}

impl Action {
    fn verb(self) -> &'static str {
        match self {
            Self::Created => "add",
            Self::Updated => "update",
        }
    }
}

fn discover_targets(kind: ClientKind) -> Vec<Target> {
    match kind {
        ClientKind::Codex => {
            let root = env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home_path(&[".codex"]));
            detect_json(root.join("config.toml"), &[root], ConfigFormat::CodexToml)
        }
        ClientKind::Cursor => detect_json(
            home_path(&[".cursor", "mcp.json"]),
            &[home_path(&[".cursor"])],
            ConfigFormat::McpServers,
        ),
        ClientKind::VsCode => {
            let mut targets = Vec::new();
            for (user_dir, markers) in vscode_user_dirs() {
                targets.extend(detect_json(
                    user_dir.join("mcp.json"),
                    &markers,
                    ConfigFormat::VsCodeServers,
                ));
            }
            targets
        }
        ClientKind::Claude => {
            let mut targets = Vec::new();
            targets.extend(detect_json(
                home_path(&[".claude.json"]),
                &[home_path(&[".claude.json"]), home_path(&[".claude"])],
                ConfigFormat::McpServers,
            ));

            if let Some(appdata) = env::var_os("APPDATA") {
                let desktop = PathBuf::from(appdata)
                    .join("Claude")
                    .join("claude_desktop_config.json");
                let marker = desktop
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| desktop.clone());
                targets.extend(detect_json(desktop, &[marker], ConfigFormat::McpServers));
            } else {
                let desktop = home_path(&[
                    "Library",
                    "Application Support",
                    "Claude",
                    "claude_desktop_config.json",
                ]);
                let marker = desktop
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| desktop.clone());
                targets.extend(detect_json(desktop, &[marker], ConfigFormat::McpServers));
            }
            targets
        }
        ClientKind::OpenCode => {
            let mut markers = vec![
                xdg_config_path(&["opencode"]),
                home_path(&[".config", "opencode"]),
            ];
            if let Some(appdata) = env::var_os("APPDATA") {
                markers.push(PathBuf::from(appdata).join("opencode"));
            }
            let path = xdg_config_path(&["opencode", "opencode.json"]);
            let alt = home_path(&[".config", "opencode", "opencode.json"]);
            let mut targets = detect_json(path, &markers, ConfigFormat::OpenCodeMcp);
            if targets.is_empty() {
                targets = detect_json(alt, &markers, ConfigFormat::OpenCodeMcp);
            }
            targets
        }
    }
}

fn detect_json(path: PathBuf, markers: &[PathBuf], format: ConfigFormat) -> Vec<Target> {
    if path.exists() || markers.iter().any(|marker| marker.exists()) {
        vec![Target { path, format }]
    } else {
        Vec::new()
    }
}

fn vscode_user_dirs() -> Vec<(PathBuf, Vec<PathBuf>)> {
    let mut dirs = Vec::new();
    if let Some(appdata) = env::var_os("APPDATA") {
        let appdata = PathBuf::from(appdata);
        for product in ["Code", "Code - Insiders"] {
            let user = appdata.join(product).join("User");
            dirs.push((user.clone(), vec![user, appdata.join(product)]));
        }
    } else {
        dirs.push((
            home_path(&["Library", "Application Support", "Code", "User"]),
            vec![home_path(&["Library", "Application Support", "Code"])],
        ));
        dirs.push((
            home_path(&[".config", "Code", "User"]),
            vec![home_path(&[".config", "Code"])],
        ));
    }
    dirs
}

fn home_path(parts: &[&str]) -> PathBuf {
    let home = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    parts.iter().fold(home, |acc, part| acc.join(part))
}

fn xdg_config_path(parts: &[&str]) -> PathBuf {
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
        return parts.iter().fold(PathBuf::from(xdg), |acc, p| acc.join(p));
    }
    let mut base = home_path(&[".config"]);
    for part in parts {
        base = base.join(part);
    }
    base
}

fn upsert_target(target: &Target, server_name: &str, launch: &ServerLaunch) -> Result<Action> {
    if let Some(parent) = target.path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
    }

    let existed = target.path.exists();
    let original = if existed {
        fs::read_to_string(&target.path)
            .with_context(|| format!("read {}", target.path.display()))?
    } else {
        String::new()
    };
    if !matches!(target.format, ConfigFormat::CodexToml) {
        refuse_jsonc_rewrite(&target.path, &original)?;
    }

    let next = match target.format {
        ConfigFormat::CodexToml => upsert_codex_toml(&original, server_name, launch)?,
        ConfigFormat::McpServers => upsert_mcp_servers_json(&original, server_name, launch)?,
        ConfigFormat::VsCodeServers => upsert_vscode_servers_json(&original, server_name, launch)?,
        ConfigFormat::OpenCodeMcp => upsert_opencode_json(&original, server_name, launch)?,
    };

    let mut out = next;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    atomic_write_with_backup(&target.path, &out)?;
    Ok(if existed {
        Action::Updated
    } else {
        Action::Created
    })
}

/// Backup existing file as `path.bak.<pid>`, then write via temp + rename.
///
/// The existing portable permissions (Unix mode bits or the Windows read-only
/// flag) are applied to the staging file before any config content is written.
pub fn atomic_write_with_backup(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }

    let original_permissions = if path.exists() {
        Some(
            fs::metadata(path)
                .with_context(|| format!("read metadata {}", path.display()))?
                .permissions(),
        )
    } else {
        None
    };

    if original_permissions.is_some() {
        let backup = path.with_file_name(format!(
            "{}.bak.{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| anyhow!("invalid config path {}", path.display()))?,
            std::process::id()
        ));
        fs::copy(path, &backup)
            .with_context(|| format!("backup {} → {}", path.display(), backup.display()))?;
    }

    let parent = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let staging = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("config"),
        std::process::id()
    ));

    {
        let mut file = fs::File::create(&staging)
            .with_context(|| format!("create temp {}", staging.display()))?;
        if let Some(permissions) = original_permissions {
            file.set_permissions(permissions)
                .with_context(|| format!("preserve permissions on {}", staging.display()))?;
        }
        file.write_all(contents.as_bytes())
            .with_context(|| format!("write temp {}", staging.display()))?;
        file.sync_all()
            .with_context(|| format!("sync temp {}", staging.display()))?;
    }

    fs::rename(&staging, path)
        .with_context(|| format!("rename {} → {}", staging.display(), path.display()))?;
    Ok(())
}

fn upsert_codex_toml(original: &str, server_name: &str, launch: &ServerLaunch) -> Result<String> {
    let mut document = original
        .trim_start_matches('\u{feff}')
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| anyhow!("invalid Codex TOML; configuration left unchanged"))?;
    if document.get("mcp_servers").is_none() {
        document["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let servers = document["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| anyhow!("mcp_servers must be a TOML table"))?;
    if server_name == DEFAULT_SERVER_NAME {
        for retired in RETIRED_SERVER_NAMES {
            servers.remove(retired);
        }
    }
    let mut entry = toml_edit::Table::new();
    entry["command"] = toml_edit::value(path_string(&launch.command));
    let args = launch
        .args
        .iter()
        .fold(toml_edit::Array::new(), |mut values, arg| {
            values.push(arg.as_str());
            values
        });
    entry["args"] = toml_edit::value(args);
    if !launch.env.is_empty() {
        let mut environment = toml_edit::Table::new();
        for (key, value) in &launch.env {
            environment[key] = toml_edit::value(
                value
                    .as_str()
                    .ok_or_else(|| anyhow!("MCP environment values must be strings"))?,
            );
        }
        entry["env"] = toml_edit::Item::Table(environment);
    }
    servers.insert(server_name, toml_edit::Item::Table(entry));
    Ok(document.to_string())
}

fn upsert_mcp_servers_json(
    original: &str,
    server_name: &str,
    launch: &ServerLaunch,
) -> Result<String> {
    let mut root = parse_json_object(original, json!({ "mcpServers": {} }))
        .context("parse JSON (mcpServers)")?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("root JSON value must be an object"))?;
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow!("mcpServers must be an object"))?;
    remove_retired_servers(servers, server_name);
    servers.insert(server_name.to_string(), mcp_servers_entry(launch));
    Ok(serde_json::to_string_pretty(&root)?)
}

fn upsert_vscode_servers_json(
    original: &str,
    server_name: &str,
    launch: &ServerLaunch,
) -> Result<String> {
    let mut root = parse_json_object(original, json!({ "servers": {} }))
        .context("parse JSON (VS Code servers)")?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("root JSON value must be an object"))?;
    let servers = obj
        .entry("servers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow!("servers must be an object"))?;
    remove_retired_servers(servers, server_name);
    servers.insert(
        server_name.to_string(),
        json!({
            "type": "stdio",
            "command": path_string(&launch.command),
            "args": launch.args,
            "env": Value::Object(launch.env.clone()),
        }),
    );
    Ok(serde_json::to_string_pretty(&root)?)
}

fn upsert_opencode_json(
    original: &str,
    server_name: &str,
    launch: &ServerLaunch,
) -> Result<String> {
    let mut root = parse_json_object(
        original,
        json!({
            "$schema": "https://opencode.ai/config.json",
            "mcp": {
                "servers": {}
            }
        }),
    )
    .context("parse JSON (OpenCode)")?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("root JSON value must be an object"))?;
    let mcp = obj
        .entry("mcp")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow!("mcp must be an object"))?;

    let mut command = vec![path_string(&launch.command)];
    command.extend(launch.args.iter().cloned());
    let entry = json!({
        "type": "local",
        "command": command,
        "environment": Value::Object(launch.env.clone()),
    });

    mcp.remove(server_name);
    remove_retired_servers(mcp, server_name);
    let servers = mcp
        .entry("servers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow!("mcp.servers must be an object"))?;
    remove_retired_servers(servers, server_name);
    servers.insert(server_name.to_string(), entry);
    Ok(serde_json::to_string_pretty(&root)?)
}

fn remove_retired_servers(servers: &mut Map<String, Value>, server_name: &str) {
    if server_name == DEFAULT_SERVER_NAME {
        for retired in RETIRED_SERVER_NAMES {
            servers.remove(*retired);
        }
    }
}

fn mcp_servers_entry(launch: &ServerLaunch) -> Value {
    json!({
        "command": path_string(&launch.command),
        "args": launch.args,
        "env": Value::Object(launch.env.clone()),
    })
}

/// Preserve JSONC comments by refusing a lossy pretty-print rewrite.
///
/// Empty files and strict JSON are fine. If the file only parses after comment
/// stripping, refuse and ask the user to convert to plain JSON first.
fn refuse_jsonc_rewrite(path: &Path, original: &str) -> Result<()> {
    let trimmed = original.trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    if serde_json::from_str::<Value>(trimmed).is_ok() {
        return Ok(());
    }
    let stripped = strip_jsonc_comments(trimmed);
    if serde_json::from_str::<Value>(stripped.trim()).is_ok() {
        bail!(
            "refusing to rewrite JSONC with comments at {} — convert to plain JSON first \
             (pretty-print upsert would drop // and /* */ comments)",
            path.display()
        );
    }

    Ok(())
}

/// Parse a JSON object, tolerating empty files, UTF-8 BOM, and light JSONC (`//` / `/* */`).
fn parse_json_object(original: &str, empty_default: Value) -> Result<Value> {
    let trimmed = original.trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return Ok(empty_default);
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => {
            if value.is_object() {
                Ok(value)
            } else {
                bail!("root JSON value must be an object")
            }
        }
        Err(_) => {
            let stripped = strip_jsonc_comments(trimmed);
            let value: Value = serde_json::from_str(stripped.trim())
                .context("invalid JSON (after stripping comments)")?;
            if !value.is_object() {
                bail!("root JSON value must be an object");
            }
            Ok(value)
        }
    }
}

fn strip_jsonc_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push('"');
            continue;
        }
        if ch == '/' {
            if chars.peek() == Some(&'/') {
                chars.next();
                for comment_ch in chars.by_ref() {
                    if comment_ch == '\n' {
                        out.push('\n');
                        break;
                    }
                }
                continue;
            }
            if chars.peek() == Some(&'*') {
                chars.next();
                let mut saw_star = false;
                for comment_ch in chars.by_ref() {
                    if saw_star && comment_ch == '/' {
                        break;
                    }
                    if comment_ch == '\n' {
                        out.push('\n');
                    }
                    saw_star = comment_ch == '*';
                }
                continue;
            }
        }
        out.push(ch);
    }
    out
}

fn path_string(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if let Some(server_path) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{server_path}")
    } else if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        raw.into_owned()
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    path.canonicalize()
        .map(|canonical| {
            let as_string = path_string(&canonical);
            PathBuf::from(as_string)
        })
        .unwrap_or(path)
}

/// Resolve only an explicitly selected installed executable. Default setup
/// builds and promotes the unified desktop before reaching this boundary.
fn resolve_binary(_repo_root: &Path, options: &Options) -> Result<PathBuf> {
    let path = options
        .binary
        .as_ref()
        .context("No installed runtime selected")?;
    if !options.dry_run && !path.is_file() {
        bail!("Installed executable does not exist: {}", path.display());
    }
    Ok(normalize_path(path.clone()))
}
fn repo_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .ok_or_else(|| anyhow!("xtask crate has no parent directory"))?;
    Ok(root.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn product_rename_removes_retired_servers_and_preserves_other_clients() {
        let launch = launch_fixture();
        for (rewrite, key) in [
            (
                upsert_mcp_servers_json as fn(&str, &str, &ServerLaunch) -> Result<String>,
                "mcpServers",
            ),
            (
                upsert_vscode_servers_json as fn(&str, &str, &ServerLaunch) -> Result<String>,
                "servers",
            ),
        ] {
            let original = json!({key: {"nobs-cad": {"command": "old-cad"}, "nbcad": {}, "other": {"command": "other-tool"}}}).to_string();
            let next: Value =
                serde_json::from_str(&rewrite(&original, DEFAULT_SERVER_NAME, &launch).unwrap())
                    .unwrap();
            assert_eq!(next[key]["other"]["command"], "other-tool");
            assert_eq!(
                next[key][DEFAULT_SERVER_NAME]["command"],
                path_string(&launch.command)
            );
            for retired in RETIRED_SERVER_NAMES {
                assert!(next[key].get(*retired).is_none());
            }
            assert_eq!(
                rewrite(&next.to_string(), DEFAULT_SERVER_NAME, &launch).unwrap(),
                serde_json::to_string_pretty(&next).unwrap()
            );
        }
        let original = "# retained\nmodel='user-model'\n[mcp_servers.nobs-cad]\ncommand='old-cad'\n[mcp_servers.other]\ncommand='other-tool'\n";
        let next = upsert_codex_toml(original, DEFAULT_SERVER_NAME, &launch).unwrap();
        assert!(next.contains("# retained"));
        let document = next.parse::<toml_edit::DocumentMut>().unwrap();
        assert!(document["mcp_servers"].get("nobs-cad").is_none());
        assert_eq!(
            document["mcp_servers"]["other"]["command"].as_str(),
            Some("other-tool")
        );
        assert!(document["mcp_servers"].get(DEFAULT_SERVER_NAME).is_some());

        let original = json!({"mcp": {"nobs-cad": {}, "servers": {"noBS-CAD": {}, "other": {"command": ["other-tool"]}}}}).to_string();
        let next: Value = serde_json::from_str(
            &upsert_opencode_json(&original, DEFAULT_SERVER_NAME, &launch).unwrap(),
        )
        .unwrap();
        assert!(next["mcp"].get("nobs-cad").is_none());
        assert!(next["mcp"]["servers"].get("noBS-CAD").is_none());
        assert_eq!(
            next["mcp"]["servers"]["other"]["command"],
            json!(["other-tool"])
        );
        assert!(next["mcp"]["servers"].get(DEFAULT_SERVER_NAME).is_some());

        let next: Value = serde_json::from_str(
            &upsert_mcp_servers_json(r#"{"mcpServers":{"nobs-cad":{}}}"#, "custom-cad", &launch)
                .unwrap(),
        )
        .unwrap();
        assert!(next["mcpServers"].get("nobs-cad").is_some());
    }

    #[test]
    fn packaged_unc_paths_remain_absolute_in_every_client_configuration() {
        let mut launch = launch_fixture();
        launch.command = PathBuf::from(r"\\?\UNC\cad-server\printed parts\Limo-CAD.exe");
        let expected = r"\\cad-server\printed parts\Limo-CAD.exe";
        for (next, key) in [
            (
                upsert_mcp_servers_json("", "nobs-cad", &launch).unwrap(),
                "mcpServers",
            ),
            (
                upsert_vscode_servers_json("", "nobs-cad", &launch).unwrap(),
                "servers",
            ),
        ] {
            let value: Value = serde_json::from_str(&next).unwrap();
            assert_eq!(value[key]["nobs-cad"]["command"], expected);
        }
        let value: Value =
            serde_json::from_str(&upsert_opencode_json("", "nobs-cad", &launch).unwrap()).unwrap();
        assert_eq!(value["mcp"]["servers"]["nobs-cad"]["command"][0], expected);
        let document = upsert_codex_toml("", "nobs-cad", &launch)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(
            document["mcp_servers"]["nobs-cad"]["command"].as_str(),
            Some(expected)
        );
        assert_eq!(
            path_string(Path::new(r"\\?\C:\printed parts\Limo-CAD.exe")),
            r"C:\printed parts\Limo-CAD.exe"
        );
    }

    #[test]
    fn packaged_options_keep_literal_server_arguments() {
        let options = Options::parse(
            [
                "--clients",
                "codex,cursor,codex",
                "--binary",
                "/installed/Limo-CAD",
                "--in-place",
                "--server-arg",
                "--headless",
                "--desktop",
                "/installed/Limo-CAD",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert!(options.in_place);
        assert_eq!(options.server_args, ["--headless"]);
        assert_eq!(
            options.clients.unwrap(),
            [ClientKind::Codex, ClientKind::Cursor]
        );
        assert!(
            Options::parse(["--dry-run", "--in-place"].into_iter().map(str::to_owned)).is_err()
        );
    }

    #[test]
    fn codex_upsert_preserves_comments_and_other_configuration() {
        let original = "# Keep this preference\nmodel = 'custom-model'\n[mcp_servers.other]\ncommand = 'other-tool'\n[mcp_servers.nobs-cad]\ncommand = 'retired-cad'\n[mcp_servers.nobs-cad.env]\nOCCT_ROOT = '/old-sdk'\n";
        let mut launch = launch_fixture();
        launch.args.push("--headless".into());
        launch.env.clear();
        launch.env.insert(
            "LIMO_CAD_DESKTOP_BIN".into(),
            Value::String("/installed/Limo-CAD".into()),
        );
        let next = upsert_codex_toml(original, "nobs-cad", &launch).unwrap();
        let document = next.parse::<toml_edit::DocumentMut>().unwrap();
        assert!(next.contains("# Keep this preference"));
        assert_eq!(document["model"].as_str(), Some("custom-model"));
        assert_eq!(
            document["mcp_servers"]["other"]["command"].as_str(),
            Some("other-tool")
        );
        assert_eq!(
            document["mcp_servers"]["nobs-cad"]["args"][0].as_str(),
            Some("--headless")
        );
        assert_eq!(
            document["mcp_servers"]["nobs-cad"]["env"]["LIMO_CAD_DESKTOP_BIN"].as_str(),
            Some("/installed/Limo-CAD")
        );
        assert!(document["mcp_servers"]["nobs-cad"]["env"]
            .get("OCCT_ROOT")
            .is_none());
        assert!(upsert_codex_toml("mcp_servers = 'bad-shape'", "nobs-cad", &launch).is_err());
        let error = upsert_codex_toml("secret = 'fixture-sensitive-value", "nobs-cad", &launch)
            .unwrap_err();
        assert!(!error.to_string().contains("fixture-sensitive-value"));
    }

    #[test]
    fn every_json_client_keeps_packaged_headless_arguments() {
        let mut launch = launch_fixture();
        launch.args.push("--headless".into());
        for (next, key) in [
            (
                upsert_mcp_servers_json("", "nobs-cad", &launch).unwrap(),
                "mcpServers",
            ),
            (
                upsert_vscode_servers_json("", "nobs-cad", &launch).unwrap(),
                "servers",
            ),
        ] {
            let value: Value = serde_json::from_str(&next).unwrap();
            assert_eq!(value[key]["nobs-cad"]["args"], json!(["--headless"]));
        }
        let value: Value =
            serde_json::from_str(&upsert_opencode_json("", "nobs-cad", &launch).unwrap()).unwrap();
        assert_eq!(
            value["mcp"]["servers"]["nobs-cad"]["command"],
            json!(["/repo/mcp-server/target/release/limo-cad-mcp", "--headless"])
        );
    }

    #[test]
    fn in_place_resolution_does_not_copy_packaged_executable_or_libraries() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("packaged-cad");
        fs::write(&binary, b"fixture executable").unwrap();
        fs::write(directory.path().join("runtime-library"), b"fixture library").unwrap();
        let options = Options::parse(
            [
                "--clients".to_owned(),
                "codex".to_owned(),
                "--binary".to_owned(),
                binary.to_string_lossy().into_owned(),
                "--in-place".to_owned(),
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(
            resolve_binary(directory.path(), &options).unwrap(),
            normalize_path(binary)
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    fn launch_fixture() -> ServerLaunch {
        let mut env = Map::new();
        env.insert("OCCT_ROOT".to_string(), Value::String("/occt".into()));
        ServerLaunch {
            command: PathBuf::from("/repo/mcp-server/target/release/limo-cad-mcp"),
            args: Vec::new(),
            env,
        }
    }

    #[test]
    fn options_dry_run_allows_omitting_clients() {
        let options = Options::parse(["--dry-run".into()].into_iter()).unwrap();
        assert!(options.dry_run);
        assert!(options.clients.is_none());
    }

    #[test]
    fn options_write_requires_clients() {
        let err = Options::parse(std::iter::empty()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("--clients is required"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn options_write_with_clients_ok() {
        let options = Options::parse(
            [
                "--clients".into(),
                "cursor,vscode".into(),
                "--no-build".into(),
            ]
            .into_iter(),
        )
        .unwrap();
        assert!(!options.dry_run);
        assert!(!options.build);
        assert_eq!(
            options.clients.as_deref(),
            Some(&[ClientKind::Cursor, ClientKind::VsCode][..])
        );
    }

    #[test]
    fn options_duplicate_clients_are_processed_once() {
        let options = Options::parse(
            [
                "--clients".into(),
                "cursor,CURSOR,vscode,cursor".into(),
                "--no-build".into(),
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(
            options.clients.as_deref(),
            Some(&[ClientKind::Cursor, ClientKind::VsCode][..])
        );
    }

    #[test]
    fn reject_grok_and_xai_clients() {
        for name in ["grok", "xai", "Grok", "XAI"] {
            let err = ClientKind::parse(name).unwrap_err();
            let msg = format!("{err:#}");
            assert!(
                msg.contains("not supported") || msg.contains("Grok"),
                "unexpected error for {name}: {msg}"
            );
        }
        let err =
            Options::parse([String::from("--clients"), String::from("cursor,grok")].into_iter())
                .unwrap_err();
        assert!(format!("{err:#}").contains("not supported"));
    }

    #[test]
    fn upsert_preserves_sibling_mcp_servers() {
        let original = r#"{
  "mcpServers": {
    "other": { "command": "echo" }
  }
}"#;
        let next = upsert_mcp_servers_json(original, "nobs-cad", &launch_fixture()).unwrap();
        let value: Value = serde_json::from_str(&next).unwrap();
        assert_eq!(value["mcpServers"]["other"]["command"], "echo");
        assert_eq!(
            value["mcpServers"]["nobs-cad"]["command"],
            "/repo/mcp-server/target/release/limo-cad-mcp"
        );
        assert_eq!(value["mcpServers"]["nobs-cad"]["env"]["OCCT_ROOT"], "/occt");
    }

    #[test]
    fn upsert_vscode_uses_servers_key() {
        let next = upsert_vscode_servers_json("", "nobs-cad", &launch_fixture()).unwrap();
        let value: Value = serde_json::from_str(&next).unwrap();
        assert_eq!(value["servers"]["nobs-cad"]["type"], "stdio");
        assert!(value.get("mcpServers").is_none());
    }

    #[test]
    fn upsert_opencode_respects_servers_nest() {
        let original = r#"{
  "mcp": {
    "servers": {
      "keep": { "type": "local", "command": ["true"] }
    }
  }
}"#;
        let next = upsert_opencode_json(original, "nobs-cad", &launch_fixture()).unwrap();
        let value: Value = serde_json::from_str(&next).unwrap();
        assert!(value["mcp"]["servers"]["keep"].is_object());
        assert_eq!(value["mcp"]["servers"]["nobs-cad"]["type"], "local");
        assert!(value["mcp"]["servers"]["nobs-cad"].get("enabled").is_none());
    }

    #[test]
    fn upsert_opencode_fresh_config_uses_v2_schema() {
        let next = upsert_opencode_json("", "nobs-cad", &launch_fixture()).unwrap();
        let value: Value = serde_json::from_str(&next).unwrap();
        let entry = &value["mcp"]["servers"]["nobs-cad"];
        assert_eq!(value["$schema"], "https://opencode.ai/config.json");
        assert_eq!(entry["type"], "local");
        assert_eq!(
            entry["command"][0],
            "/repo/mcp-server/target/release/limo-cad-mcp"
        );
        assert!(entry.get("enabled").is_none());
        assert!(value["mcp"].get("nobs-cad").is_none());
    }

    #[test]
    fn upsert_opencode_migrates_legacy_flat_entry() {
        let original = r#"{
  "mcp": {
    "nobs-cad": {
      "type": "local",
      "command": ["old"],
      "enabled": true
    }
  }
}"#;
        let next = upsert_opencode_json(original, "nobs-cad", &launch_fixture()).unwrap();
        let value: Value = serde_json::from_str(&next).unwrap();
        assert!(value["mcp"].get("nobs-cad").is_none());
        assert_eq!(
            value["mcp"]["servers"]["nobs-cad"]["command"][0],
            "/repo/mcp-server/target/release/limo-cad-mcp"
        );
        assert!(value["mcp"]["servers"]["nobs-cad"].get("enabled").is_none());
    }

    #[test]
    fn atomic_write_with_backup_creates_bak_and_replaces() {
        let dir = std::env::temp_dir().join(format!(
            "limo-cad-xtask-atomic-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mcp.json");
        fs::write(&path, "{\"old\":true}\n").unwrap();

        atomic_write_with_backup(&path, "{\"new\":true}\n").unwrap();

        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(written, "{\"new\":true}\n");

        let backup = dir.join(format!("mcp.json.bak.{}", std::process::id()));
        assert!(backup.is_file(), "expected backup at {}", backup.display());
        assert_eq!(fs::read_to_string(&backup).unwrap(), "{\"old\":true}\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_with_backup_creates_new_file_without_bak() {
        let dir = std::env::temp_dir().join(format!(
            "limo-cad-xtask-atomic-new-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fresh.json");

        atomic_write_with_backup(&path, "{\"ok\":1}\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"ok\":1}\n");
        assert!(!dir
            .join(format!("fresh.json.bak.{}", std::process::id()))
            .exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_with_backup_preserves_unix_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "limo-cad-xtask-atomic-mode-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("private.json");
        fs::write(&path, "{\"secret\":\"old\"}\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        atomic_write_with_backup(&path, "{\"secret\":\"new\"}\n").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"secret\":\"new\"}\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_json_object_accepts_empty_and_jsonc() {
        let empty = parse_json_object("  \n", json!({ "mcpServers": {} })).unwrap();
        assert!(empty["mcpServers"].is_object());
        let jsonc =
            parse_json_object("{\n  // comment\n  \"mcpServers\": {}\n}\n", json!({})).unwrap();
        assert!(jsonc["mcpServers"].is_object());
    }

    #[test]
    fn refuse_jsonc_rewrite_blocks_commented_configs() {
        let path = PathBuf::from("fake-mcp.json");
        refuse_jsonc_rewrite(&path, "{\n  // keep me\n  \"mcpServers\": {}\n}\n").unwrap_err();
        refuse_jsonc_rewrite(&path, "{\n  \"mcpServers\": {}\n}\n").unwrap();
        refuse_jsonc_rewrite(&path, "").unwrap();
    }

    #[test]
    fn jsonc_detection_preserves_unicode_and_string_slashes() {
        let original = r#"{
  // 保留这个注释
  "name": "José 東京",
  "url": "https://example.com/a/*literal*/",
  "mcpServers": {}
}"#;
        let stripped = strip_jsonc_comments(original);
        let value: Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(value["name"], "José 東京");
        assert_eq!(value["url"], "https://example.com/a/*literal*/");

        let path = PathBuf::from("unicode-mcp.json");
        let error = refuse_jsonc_rewrite(&path, original).unwrap_err();
        assert!(format!("{error:#}").contains("refusing to rewrite JSONC"));
    }
}
