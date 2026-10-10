//! Verify the exact association written by the owned native package fixture.
use super::common;
use anyhow::{ensure, Context, Result};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn exec_argument(path: &str) -> Result<String> {
    ensure!(
        !path.contains('=') && !path.chars().any(char::is_control),
        "executable path cannot be represented in Desktop Entry Exec"
    );
    let mut quoted = String::from("\"");
    for character in path.chars() {
        match character {
            '\\' => quoted.push_str("\\\\\\\\"),
            '"' | '`' | '$' => {
                quoted.push_str("\\\\");
                quoted.push(character);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    Ok(quoted)
}
pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    ensure!(
        env::consts::OS == "linux",
        "recipe association verification requires Linux"
    );
    let mut options = BTreeMap::new();
    while let Some(key) = args.next() {
        ensure!(
            matches!(
                key.as_str(),
                "--evidence" | "--server" | "--artifact" | "--backend"
            ),
            "unknown recipe-handler option {key}"
        );
        let value = args
            .next()
            .context("recipe-handler option requires a value")?;
        ensure!(
            options.insert(key, value).is_none(),
            "duplicate recipe-handler option"
        );
    }
    let get = |key: &str| {
        options
            .get(key)
            .with_context(|| format!("{key} is required"))
    };
    let evidence = Path::new(get("--evidence")?);
    let server = Path::new(get("--server")?);
    let artifact = Path::new(get("--artifact")?);
    let backend = get("--backend")?;
    ensure!(
        matches!(backend.as_str(), "x11" | "wayland"),
        "unknown package backend {backend}"
    );
    let (data, config) = if backend == "wayland" {
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(evidence.join("native-wayland.json"))?)?;
        ensure!(
            report["passed"] == true && report["desktop"]["passed"] == true,
            "Wayland fixture did not pass"
        );
        let profile = Path::new(
            report["desktop"]["native_profile"]
                .as_str()
                .context("missing owned native profile")?,
        );
        let session = Path::new(
            report["desktop"]["session_directory"]
                .as_str()
                .context("missing owned session directory")?,
        );
        ensure!(
            profile.is_absolute() && session.is_absolute(),
            "owned profile and session must be absolute"
        );
        ensure!(
            profile.canonicalize()?.parent() == Some(session.canonicalize()?.as_path()),
            "native profile left its owned session"
        );
        (profile.join("data"), profile.join("config"))
    } else {
        (
            PathBuf::from(env::var_os("XDG_DATA_HOME").context("missing isolated XDG_DATA_HOME")?),
            PathBuf::from(
                env::var_os("XDG_CONFIG_HOME").context("missing isolated XDG_CONFIG_HOME")?,
            ),
        )
    };
    ensure!(
        data.is_absolute() && config.is_absolute(),
        "isolated XDG directories must be absolute"
    );
    let handler = common::output(
        Command::new("xdg-mime")
            .args(["query", "default", "x-scheme-handler/limo-cad"])
            .env("XDG_DATA_HOME", &data)
            .env("XDG_CONFIG_HOME", &config),
    )?;
    ensure!(
        handler == "limo-cad.desktop",
        "owned native recipe handler was not registered: {handler}"
    );
    let registered = data.join("applications/limo-cad.desktop");
    common::run(Command::new("desktop-file-validate").arg(&registered))?;
    let executable = if artifact.extension().is_some_and(|v| v == "AppImage") {
        artifact.to_path_buf()
    } else {
        server.canonicalize()?
    };
    let expected = format!(
        "Exec={} %u",
        exec_argument(executable.to_str().context("UTF-8 executable path")?)?
    );
    let contents = fs::read_to_string(&registered)?;
    ensure!(
        contents.lines().find(|line| line.starts_with("Exec=")) == Some(expected.as_str()),
        "recipe handler did not retain the exact packaged executable and URL argument"
    );
    fs::copy(registered, evidence.join("registered.desktop"))?;
    fs::write(
        evidence.join("recipe-handler.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(
                &serde_json::json!({"passed":true,"data_home":data,"config_home":config,"executable":executable,"desktop_id":handler})
            )?
        ),
    )?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recipe_exec_quotes_both_desktop_escape_layers() {
        assert_eq!(
            exec_argument("/tmp/a b%\\\"`$.AppImage").unwrap(),
            "\"/tmp/a b%%\\\\\\\\\\\\\"\\\\`\\\\$.AppImage\""
        );
        assert!(exec_argument("/tmp/bad=path").is_err());
        assert!(exec_argument("/tmp/bad\npath").is_err());
    }
}
