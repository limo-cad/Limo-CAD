//! Desktop CI input classification, including both sides of renamed assets.
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{
    env, fs,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Builds {
    windows: bool,
    macos: bool,
    linux: bool,
}
impl Builds {
    fn all() -> Self {
        Self {
            windows: true,
            macos: true,
            linux: true,
        }
    }
    fn path(&mut self, path: &str) {
        let common = [
            "desktop/",
            "crates/",
            "native/occt-overlay/",
            "assets/",
            "mcp-server/",
            "knowledge/",
            "interface/",
            "examples/scripts/",
            "xtask/",
            ".github/actions/",
            ".cargo/",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix))
            || [
                "Cargo.toml",
                "Cargo.lock",
                "VERSION",
                "REPOSITORY",
                "vcpkg.json",
                "vcpkg-configuration.json",
                "rust-toolchain",
                "rust-toolchain.toml",
                "LICENSE",
                "THIRD_PARTY_NOTICES.md",
                ".github/workflows/desktop-packages.yml",
            ]
            .contains(&path);
        if common {
            *self = Self::all();
        }
        self.windows |= [
            "scripts/verify-windows-viewport.ps1",
            "scripts/prepare-hosted-arm-desktop.ps1",
            "scripts/ci/arm-runner-shell-preflight.test.ps1",
        ]
        .contains(&path);
        self.linux |= [
            "scripts/verify-linux-viewport.sh",
            "scripts/verify-linux-native-package.sh",
            "scripts/docker/ubuntu-26.04.Dockerfile",
            "scripts/docker/appimage-ubuntu-22.04.Dockerfile",
        ]
        .contains(&path);
    }
    fn files(&mut self, files: &[Value]) -> Result<()> {
        for file in files {
            self.path(
                file["filename"]
                    .as_str()
                    .context("changed file missing filename")?,
            );
            if let Some(previous) = file["previous_filename"].as_str() {
                self.path(previous);
            }
        }
        Ok(())
    }
    fn write(&self, path: &Path) -> Result<()> {
        let mut output = fs::OpenOptions::new().append(true).open(path)?;
        writeln!(
            output,
            "windows_should_build={}\nmacos_should_build={}\nlinux_should_build={}",
            self.windows, self.macos, self.linux
        )?;
        Ok(())
    }
}

fn api_json(agent: &ureq::Agent, url: &str, token: &str) -> Result<Value> {
    let mut response = agent
        .get(url)
        .header("User-Agent", "Limo-CAD-desktop-inputs")
        .header("Accept", "application/vnd.github+json")
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .context("read PR build inputs")?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 16 * 1024 * 1024,
        "PR inventory response too large"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    ensure!(args.next().is_none(), "use cargo xtask ci desktop-changes");
    let mut builds = Builds::all();
    if env::var("GITHUB_EVENT_NAME")?.as_str() == "pull_request" {
        let event: Value = serde_json::from_slice(&fs::read(
            env::var_os("GITHUB_EVENT_PATH").context("missing GitHub event")?,
        )?)?;
        let changed = event["pull_request"]["changed_files"]
            .as_u64()
            .context("missing changed_files")?;
        if changed <= 3000 {
            builds = Builds::default();
            let number = event["pull_request"]["number"]
                .as_u64()
                .context("missing PR number")?;
            let repo = env::var("GITHUB_REPOSITORY")?;
            ensure!(
                repo.split('/').count() == 2
                    && repo
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_./".contains(&byte)),
                "invalid repository"
            );
            let endpoint =
                env::var("GITHUB_API_URL").unwrap_or_else(|_| "https://api.github.com".into());
            ensure!(
                endpoint.starts_with("https://"),
                "GitHub API must use HTTPS"
            );
            let token = env::var("GITHUB_TOKEN").context("missing read-only GitHub token")?;
            let config = ureq::Agent::config_builder()
                .https_only(true)
                .timeout_global(Some(Duration::from_secs(60)))
                .build();
            let agent: ureq::Agent = config.into();
            let pull_url = format!("{endpoint}/repos/{repo}/pulls/{number}");
            let head = event["pull_request"]["head"]["sha"]
                .as_str()
                .context("missing PR head")?;
            ensure!(
                api_json(&agent, &pull_url, &token)?["head"]["sha"].as_str() == Some(head),
                "PR head changed before classification; retry"
            );
            let mut count = 0;
            for page in 1..=30 {
                let page = api_json(
                    &agent,
                    &format!("{pull_url}/files?per_page=100&page={page}"),
                    &token,
                )?;
                let files = page
                    .as_array()
                    .context("GitHub file inventory must be an array")?;
                builds.files(files)?;
                count += files.len();
                if files.len() < 100 {
                    break;
                }
            }
            ensure!(
                api_json(&agent, &pull_url, &token)?["head"]["sha"].as_str() == Some(head),
                "PR head changed during classification; retry"
            );
            ensure!(count as u64 == changed, "PR changed during classification or file inventory incomplete ({count}/{changed}); retry");
            println!("Classified {count} changed files: {builds:?}");
        } else {
            println!("PR exceeds GitHub's file inventory limit; build every platform");
        }
    } else {
        println!("Tag/manual event: build every platform");
    }
    builds.write(Path::new(
        &env::var_os("GITHUB_OUTPUT").context("missing GITHUB_OUTPUT")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_assets_and_cargo_policy_build_every_platform() {
        for path in [
            "assets/i18n/en.json",
            "assets/ribbon-icons/save.svg",
            ".cargo/config.toml",
            ".cargo/tools.toml",
            "crates/occt/sdk.rs",
            "native/occt-overlay/opencascade/math_DoubleTab.lxx",
            "vcpkg-configuration.json",
            "VERSION",
            "REPOSITORY",
        ] {
            let mut builds = Builds::default();
            builds.path(path);
            assert_eq!(builds, Builds::all(), "{path}");
        }
    }
    #[test]
    fn docs_skip_and_platform_verifiers_are_scoped() {
        let mut builds = Builds::default();
        builds.path("docs/DEVELOPMENT.md");
        assert_eq!(builds, Builds::default());
        builds.path("scripts/verify-linux-native-package.sh");
        assert_eq!(
            builds,
            Builds {
                linux: true,
                ..Builds::default()
            }
        );
        builds.path("scripts/verify-windows-viewport.ps1");
        assert!(builds.windows && builds.linux && !builds.macos);
    }
    #[test]
    fn removing_an_asset_by_rename_still_builds() {
        let mut builds = Builds::default();
        builds.files(&[serde_json::json!({"filename": "docs/moved.svg", "previous_filename": "assets/ribbon-icons/save.svg"})]).unwrap();
        assert_eq!(builds, Builds::all());
        assert!(builds.files(&[serde_json::json!({})]).is_err());
    }

    #[test]
    fn executed_windows_preflight_inputs_and_renames_qualify_windows_packages() {
        for path in [
            "scripts/verify-windows-viewport.ps1",
            "scripts/prepare-hosted-arm-desktop.ps1",
            "scripts/ci/arm-runner-shell-preflight.test.ps1",
        ] {
            assert!(crate::release_tooling::root().join(path).is_file());
            let expected = Builds {
                windows: true,
                ..Builds::default()
            };
            let mut direct = Builds::default();
            direct.path(path);
            assert_eq!(direct, expected, "{path}");
            let mut renamed = Builds::default();
            renamed
                .files(&[serde_json::json!({
                    "filename": "docs/retired-preflight.ps1",
                    "previous_filename": path,
                })])
                .unwrap();
            assert_eq!(renamed, expected, "renamed {path}");
        }
    }
}
