//! Scoped build management without another task runner or shell command strings.
use anyhow::{bail, ensure, Context, Result};
use std::{env, fs, path::Path, process::Command};

#[path = "../../crates/occt/sdk.rs"]
pub(super) mod sdk;

pub fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is a repository workspace member")
}

pub fn cargo() -> Command {
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(root());
    command
}

pub fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("start {command:?}"))?;
    ensure!(status.success(), "{command:?} failed ({status})");
    Ok(())
}

fn output(command: &mut Command) -> Result<String> {
    let output = command
        .output()
        .with_context(|| format!("start {command:?}"))?;
    ensure!(
        output.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

pub fn tool_version(name: &str) -> Result<String> {
    let path = root().join(".cargo/tools.toml");
    let policy: toml_edit::DocumentMut = fs::read_to_string(path)?.parse()?;
    let version = policy
        .get("tools")
        .and_then(toml_edit::Item::as_table)
        .and_then(|tools| tools.get(name))
        .and_then(toml_edit::Item::as_str)
        .with_context(|| format!("unmanaged tool {name}"))?;
    semver::Version::parse(version)?;
    Ok(version.into())
}

pub fn require_tool(name: &str) -> Result<()> {
    let expected = tool_version(name)?;
    let actual = output(Command::new(name).arg("--version")).with_context(|| {
        format!("Install {name} {expected}: cargo xtask bootstrap --tool {name}")
    })?;
    let actual_version = installed_tool_version(name, &actual)?;
    ensure!(
        actual_version == semver::Version::parse(&expected)?,
        "{actual}; require {name} {expected}. Run cargo xtask bootstrap --tool {name}"
    );
    Ok(())
}

fn installed_tool_version(name: &str, actual: &str) -> Result<semver::Version> {
    let mut fields = actual.split_whitespace();
    let first = fields.next().context("missing tool version")?;
    let version = match (first, fields.next(), fields.next()) {
        (version, None, None) if name == "cargo-machete" => version,
        (tool, Some(version), None) if tool == name => version,
        _ => bail!("unexpected {name} --version output: {actual}"),
    };
    semver::Version::parse(version).with_context(|| format!("invalid {name} version: {version}"))
}

fn toolchain() -> Result<String> {
    let policy: toml_edit::DocumentMut =
        fs::read_to_string(root().join("rust-toolchain.toml"))?.parse()?;
    let version = policy["toolchain"]["channel"]
        .as_str()
        .context("missing pinned Rust channel")?;
    semver::Version::parse(version).context("rust-toolchain.toml must pin a Rust release")?;
    Ok(version.into())
}

#[derive(Clone, Copy)]
enum Scope {
    Engine,
    Desktop,
    Mcp,
    Wasm,
}
impl Scope {
    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "engine" => Self::Engine,
            "desktop" => Self::Desktop,
            "mcp" => Self::Mcp,
            "wasm" => Self::Wasm,
            _ => bail!("unknown scope {value}; use engine, desktop, mcp, or wasm"),
        })
    }
    fn manifest(self) -> &'static str {
        match self {
            Self::Desktop => "desktop/Cargo.toml",
            Self::Mcp => "mcp-server/Cargo.toml",
            Self::Wasm => "crates/wasm/Cargo.toml",
            _ => "Cargo.toml",
        }
    }
    fn arguments(self, command: &mut Command) {
        command.args(["--manifest-path", self.manifest()]);
        match self {
            Self::Engine => {
                command.arg("--workspace");
            }
            Self::Desktop => {
                command.args(["--bin", "limo-cad"]);
            }
            Self::Mcp => {
                command.args(["--bin", "limo-cad-mcp"]);
            }
            Self::Wasm => {
                command.args([
                    "-p",
                    "limo-cad-wasm",
                    "--lib",
                    "--target",
                    "wasm32-unknown-unknown",
                ]);
            }
        }
    }
}

pub fn check(mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut scope = Scope::Engine;
    let mut format = false;
    let mut clippy = false;
    let mut timings = false;
    let mut cache = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scope" => scope = Scope::parse(&args.next().context("missing --scope")?)?,
            "--fmt" => format = true,
            "--clippy" => clippy = true,
            "--timings" => timings = true,
            "--sccache" => cache = true,
            "--help" | "-h" => {
                println!("cargo xtask check [--scope engine|desktop|mcp|wasm] [--fmt] [--clippy] [--timings] [--sccache]\nCompiles only the selected scope; --fmt also checks its workspace formatting. Does not run tests.");
                return Ok(());
            }
            _ => bail!("unknown check argument {arg}"),
        }
    }
    if format {
        run(cargo().args([
            "fmt",
            "--manifest-path",
            scope.manifest(),
            "--all",
            "--",
            "--check",
        ]))?;
    }
    let mut command = cargo();
    command.args([if clippy { "clippy" } else { "check" }, "--locked"]);
    scope.arguments(&mut command);
    if timings {
        command.arg("--timings");
    }
    if cache {
        require_tool("sccache")?;
        command
            .env("RUSTC_WRAPPER", "sccache")
            .env("CARGO_INCREMENTAL", "0");
    }
    run(&mut command)?;
    if cache {
        run(Command::new("sccache").arg("--show-stats"))?;
    }
    Ok(())
}

pub fn bootstrap(mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut targets = Vec::new();
    let mut tools = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--target" => targets.push(args.next().context("missing --target")?),
            "--tool" => tools.push(args.next().context("missing --tool")?),
            "--wasm" => {
                targets.push("wasm32-unknown-unknown".into());
                tools.push("wasm-pack".into());
            }
            "--help" | "-h" => {
                println!("cargo xtask bootstrap [--target TRIPLE] [--tool wasm-pack|cargo-machete|cargo-deny|sccache] [--wasm]\nInstalls the repository's pinned toolchain, targets and explicitly requested tools.");
                return Ok(());
            }
            _ => bail!("unknown bootstrap argument {arg}"),
        }
    }
    let versions: Vec<_> = tools
        .iter()
        .map(|name| tool_version(name).map(|version| (name, version)))
        .collect::<Result<_>>()?;
    let supported = output(Command::new("rustc").arg("--print").arg("target-list"))?;
    for target in &targets {
        ensure!(
            supported.lines().any(|line| line == target),
            "unsupported Rust target {target}"
        );
    }
    let channel = toolchain()?;
    run(Command::new("rustup").current_dir(root()).args([
        "toolchain",
        "install",
        &channel,
        "--profile",
        "minimal",
        "--component",
        "rustfmt",
        "--component",
        "clippy",
    ]))?;
    for target in targets {
        run(Command::new("rustup").args(["target", "add", "--toolchain", &channel, &target]))?;
    }
    for (name, version) in versions {
        if require_tool(name).is_ok() {
            println!("{name} {version} already installed");
        } else {
            let mut install = cargo();
            install.args(["install", name, "--version", &version, "--locked"]);
            if name == "sccache" {
                install.arg("--no-default-features");
            }
            run(&mut install)?;
            require_tool(name)?;
        }
    }
    Ok(())
}

pub fn doctor(mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut scope = Scope::Engine;
    match args.next().as_deref() {
        Some("--scope") => scope = Scope::parse(&args.next().context("missing --scope")?)?,
        Some("--help" | "-h") => {
            println!("cargo xtask doctor [--scope engine|desktop|mcp|wasm]");
            return Ok(());
        }
        None => {}
        Some(other) => bail!("unknown doctor argument {other}"),
    }
    ensure!(args.next().is_none(), "unexpected doctor argument");
    let channel = toolchain()?;
    let rust = output(Command::new("rustc").arg("-vV"))?;
    ensure!(
        rust.lines()
            .any(|line| line == format!("release: {channel}")),
        "Active rustc disagrees with rust-toolchain.toml ({channel})"
    );
    println!(
        "Rust {channel}\n{}\nScope manifest: {}\nTarget directory: {}",
        output(cargo().arg("--version"))?,
        scope.manifest(),
        env::var_os("CARGO_TARGET_DIR").map_or_else(
            || "Cargo defaults per workspace".into(),
            |path| path.to_string_lossy().into_owned()
        )
    );
    match scope {
        Scope::Desktop | Scope::Mcp => {
            let roots = sdk::roots(
                env::consts::OS,
                env::consts::ARCH,
                root(),
                env::var_os("OCCT_ROOT").map(Into::into),
                env::var_os("VCPKG_INSTALLED_DIR").map(Into::into),
                env::var("VCPKG_TARGET_TRIPLET").ok(),
            )
            .map_err(anyhow::Error::msg)?;
            let sdk = sdk::resolve(
                &roots,
                env::consts::OS,
                env::consts::ARCH,
                env::var_os("LIMO_CAD_OCCT_LIB_DIR")
                    .as_deref()
                    .map(Path::new),
            )
            .map_err(anyhow::Error::msg)?;
            println!("OCCT 7.9 headers: {}\nOCCT link libraries: {}\nNative compilation also requires the target's C++ compiler/platform SDK.", sdk.include.display(), sdk.lib.display());
        }
        Scope::Wasm => {
            require_tool("wasm-pack")?;
            let targets = output(Command::new("rustup").args([
                "target",
                "list",
                "--installed",
                "--toolchain",
                &channel,
            ]))?;
            ensure!(
                targets.lines().any(|line| line == "wasm32-unknown-unknown"),
                "Run cargo xtask bootstrap --wasm"
            );
            println!(
                "WASM engine prerequisites present; this check does not build the browser UI."
            );
        }
        Scope::Engine => {}
    }
    Ok(())
}

pub fn deps(mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut scope = Scope::Engine;
    let mut action = "tree".to_string();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scope" => scope = Scope::parse(&args.next().context("missing --scope")?)?,
            "--unused" => action = "unused".into(),
            "--advisories" => action = "advisories".into(),
            "--help" | "-h" => {
                println!("cargo xtask deps [--scope engine|desktop|mcp|wasm] [--unused|--advisories]\nDefaults to duplicate-version Cargo tree. Optional tools must be installed through bootstrap.");
                return Ok(());
            }
            _ => bail!("unknown deps argument {arg}"),
        }
    }
    match action.as_str() {
        "unused" => {
            require_tool("cargo-machete")?;
            let root = root();
            let directories = match scope {
                Scope::Engine => vec![root.join("crates"), root.join("xtask")],
                Scope::Wasm => vec![root.join("crates/wasm")],
                _ => vec![root.join(scope.manifest()).parent().unwrap().to_owned()],
            };
            run(Command::new("cargo-machete")
                .current_dir(root)
                .args(directories))?;
        }
        "advisories" => {
            require_tool("cargo-deny")?;
            run(cargo().args([
                "deny",
                "--manifest-path",
                scope.manifest(),
                "--locked",
                "check",
                "advisories",
            ]))?;
        }
        _ => {
            run(cargo().args([
                "tree",
                "--locked",
                "--manifest-path",
                scope.manifest(),
                "--duplicates",
            ]))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_versions_accept_known_bare_and_prefixed_outputs() {
        for (name, actual, expected) in [
            ("cargo-machete", "0.9.2\n", "0.9.2"),
            ("cargo-machete", "cargo-machete 0.9.2", "0.9.2"),
            ("wasm-pack", "wasm-pack 0.15.0", "0.15.0"),
            ("cargo-deny", "cargo-deny 0.20.2", "0.20.2"),
            ("sccache", "sccache 0.18.0", "0.18.0"),
        ] {
            assert_eq!(
                installed_tool_version(name, actual).unwrap(),
                semver::Version::parse(expected).unwrap(),
            );
        }
    }

    #[test]
    fn tool_versions_reject_wrong_tools_malformed_versions_and_extra_output() {
        for (name, actual) in [
            ("cargo-machete", ""),
            ("cargo-machete", "cargo-machete"),
            ("cargo-machete", "other-tool 0.9.2"),
            ("cargo-machete", "0.9"),
            ("cargo-machete", "v0.9.2"),
            ("cargo-machete", "cargo-machete latest"),
            ("cargo-machete", "cargo-machete 0.9.2 extra"),
            ("cargo-machete", "0.9.2\n0.9.3"),
            ("wasm-pack", "0.15.0"),
        ] {
            assert!(
                installed_tool_version(name, actual).is_err(),
                "{name}: {actual}"
            );
        }
        assert_ne!(
            installed_tool_version("cargo-machete", "0.9.3").unwrap(),
            semver::Version::parse("0.9.2").unwrap(),
        );
    }

    #[test]
    fn native_and_wasm_scopes_preserve_workspace_boundaries() {
        for (scope, manifest, target) in [
            (Scope::Engine, "Cargo.toml", None),
            (Scope::Desktop, "desktop/Cargo.toml", None),
            (Scope::Mcp, "mcp-server/Cargo.toml", None),
            (
                Scope::Wasm,
                "crates/wasm/Cargo.toml",
                Some("wasm32-unknown-unknown"),
            ),
        ] {
            let mut command = cargo();
            scope.arguments(&mut command);
            let arguments: Vec<_> = command.get_args().filter_map(|arg| arg.to_str()).collect();
            assert!(arguments
                .windows(2)
                .any(|pair| pair == ["--manifest-path", manifest]));
            assert_eq!(arguments.contains(&"--target"), target.is_some());
            if let Some(target) = target {
                assert!(arguments.contains(&target));
            }
            assert!(!arguments.contains(&"--all-features"));
        }
    }
    #[test]
    fn pins_are_exact_and_unknown_tools_are_errors() {
        for name in ["wasm-pack", "cargo-machete", "cargo-deny", "sccache"] {
            semver::Version::parse(&tool_version(name).unwrap()).unwrap();
        }
        assert!(tool_version("unmanaged").is_err());
        assert!(Scope::parse("bevy-wasm").is_err());
    }
}
