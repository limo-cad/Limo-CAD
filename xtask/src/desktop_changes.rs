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
                ".gitattributes",
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
            } else {
                ensure!(
                    file["status"].as_str() != Some("renamed"),
                    "renamed file missing previous filename"
                );
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

fn immutable_inventory(
    event: &Value,
    endpoint: &str,
    repo: &str,
    mut read: impl FnMut(&str) -> Result<Value>,
) -> Result<Builds> {
    let changed = event["pull_request"]["changed_files"]
        .as_u64()
        .context("missing changed_files")?;
    // The immutable compare API returns at most 300 files, on its first page.
    // Never substitute today's mutable PR inventory for this retained run.
    if changed > 300 {
        println!("PR exceeds immutable file inventory limit; build every platform");
        return Ok(Builds::all());
    }
    ensure!(
        repo.split('/').count() == 2
            && repo
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_./".contains(&byte)),
        "invalid repository"
    );
    ensure!(
        endpoint.starts_with("https://"),
        "GitHub API must use HTTPS"
    );
    let base = event["pull_request"]["base"]["sha"]
        .as_str()
        .context("missing PR base")?;
    let head = event["pull_request"]["head"]["sha"]
        .as_str()
        .context("missing PR head")?;
    for sha in [base, head] {
        ensure!(
            sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid immutable PR commit"
        );
    }
    let comparison = read(&format!("{endpoint}/repos/{repo}/compare/{base}...{head}"))?;
    ensure!(
        comparison["base_commit"]["sha"].as_str() == Some(base),
        "comparison base differs from event"
    );
    let merge_base = comparison["merge_base_commit"]["sha"]
        .as_str()
        .context("missing comparison merge base")?;
    ensure!(
        merge_base.len() == 40 && merge_base.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid comparison merge base"
    );
    ensure!(
        matches!(
            comparison["status"].as_str(),
            Some("ahead" | "behind" | "diverged" | "identical")
        ),
        "invalid comparison status"
    );
    let files = comparison["files"]
        .as_array()
        .context("immutable file inventory must be an array")?;
    ensure!(
        files.len() as u64 == changed,
        "immutable file inventory incomplete ({}/{changed})",
        files.len()
    );
    for file in files {
        ensure!(
            matches!(
                file["status"].as_str(),
                Some(
                    "added"
                        | "removed"
                        | "modified"
                        | "renamed"
                        | "copied"
                        | "changed"
                        | "unchanged"
                )
            ),
            "invalid changed file status"
        );
        for key in ["filename", "previous_filename"] {
            if key == "previous_filename" && file[key].is_null() {
                continue;
            }
            let path = file[key].as_str().context("invalid changed file path")?;
            ensure!(
                !path.is_empty()
                    && !path.starts_with('/')
                    && !path.contains('\0')
                    && !path
                        .split('/')
                        .any(|part| part.is_empty() || part == "." || part == ".."),
                "invalid changed file path"
            );
        }
    }
    let mut builds = Builds::default();
    builds.files(files)?;
    println!(
        "Classified {} immutable changed files: {builds:?}",
        files.len()
    );
    Ok(builds)
}

fn conservative_inventory(result: Result<Builds>) -> Builds {
    match result {
        Ok(builds) => builds,
        Err(error) => {
            println!("Exact desktop input inventory unavailable ({error:#}); build every platform");
            Builds::all()
        }
    }
}

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    ensure!(args.next().is_none(), "use cargo xtask ci desktop-changes");
    let builds = if env::var("GITHUB_EVENT_NAME")?.as_str() == "pull_request" {
        conservative_inventory((|| {
            let event: Value = serde_json::from_slice(&fs::read(
                env::var_os("GITHUB_EVENT_PATH").context("missing GitHub event")?,
            )?)?;
            let repo = env::var("GITHUB_REPOSITORY")?;
            let endpoint =
                env::var("GITHUB_API_URL").unwrap_or_else(|_| "https://api.github.com".into());
            let token = env::var("GITHUB_TOKEN").context("missing read-only GitHub token")?;
            let config = ureq::Agent::config_builder()
                .https_only(true)
                .timeout_global(Some(Duration::from_secs(60)))
                .build();
            let agent: ureq::Agent = config.into();
            immutable_inventory(&event, &endpoint, &repo, |url| {
                api_json(&agent, url, &token)
            })
        })())
    } else {
        println!("Tag/manual event: build every platform");
        Builds::all()
    };
    builds.write(Path::new(
        &env::var_os("GITHUB_OUTPUT").context("missing GITHUB_OUTPUT")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "1111111111111111111111111111111111111111";
    const HEAD: &str = "2222222222222222222222222222222222222222";

    fn event(changed: u64) -> Value {
        serde_json::json!({"pull_request": {"changed_files": changed, "base": {"sha": BASE}, "head": {"sha": HEAD}}})
    }

    fn comparison(files: Value) -> Value {
        // A PR need not branch from today's base tip: a distinct valid merge
        // base is ordinary and must not prevent exact selective classification.
        serde_json::json!({"base_commit": {"sha": BASE}, "merge_base_commit": {"sha": "3333333333333333333333333333333333333333"}, "status": "diverged", "files": files})
    }

    #[test]
    fn immutable_event_classification_survives_pr_head_and_base_advancement() {
        for (old_file, current_file, expected) in [
            ("docs/old.md", "desktop/src/main.rs", Builds::default()),
            ("desktop/src/main.rs", "docs/new.md", Builds::all()),
        ] {
            let mut requests = Vec::new();
            let advanced = serde_json::json!({"base": {"sha": "4444444444444444444444444444444444444444"}, "head": {"sha": "5555555555555555555555555555555555555555"}});
            let result = immutable_inventory(
                &event(1),
                "https://api.github.com",
                "limo-cad/Limo-CAD",
                |url| {
                    requests.push(url.to_owned());
                    // Model the live PR having advanced before the request. The
                    // production reader must choose the immutable old comparison,
                    // never these current PR metadata/files with the same count.
                    if url.contains("/pulls/") {
                        return Ok(if url.ends_with("/files") {
                            serde_json::json!([{"filename": current_file, "status": "modified"}])
                        } else {
                            advanced.clone()
                        });
                    }
                    ensure!(
                        url.ends_with(&format!("/compare/{BASE}...{HEAD}")),
                        "wrong immutable source"
                    );
                    Ok(comparison(
                        serde_json::json!([{"filename": old_file, "status": "modified"}]),
                    ))
                },
            );
            assert_eq!(conservative_inventory(result), expected);
            assert_eq!(
                requests,
                [format!(
                    "https://api.github.com/repos/limo-cad/Limo-CAD/compare/{BASE}...{HEAD}"
                )]
            );
        }
        let renamed = immutable_inventory(
            &event(1),
            "https://api.github.com",
            "limo-cad/Limo-CAD",
            |_| {
                Ok(comparison(
                    serde_json::json!([{"filename": "docs/moved.svg", "previous_filename": "assets/ribbon-icons/save.svg", "status": "renamed"}]),
                ))
            },
        );
        assert_eq!(conservative_inventory(renamed), Builds::all());
    }

    #[test]
    fn unknown_immutable_inventory_builds_every_platform_without_skipping() {
        let mut wrong_base = comparison(serde_json::json!([]));
        wrong_base["base_commit"]["sha"] = serde_json::json!(HEAD);
        let mut invalid_merge_base = comparison(serde_json::json!([]));
        invalid_merge_base["merge_base_commit"]["sha"] = serde_json::json!("main");
        for (changed, response) in [
            (1, comparison(serde_json::json!([]))),
            (0, wrong_base),
            (0, invalid_merge_base),
            (
                1,
                comparison(serde_json::json!([{"filename": "docs/a", "status": "renamed"}])),
            ),
            (
                1,
                comparison(serde_json::json!([{"filename": "docs/a", "status": "unknown"}])),
            ),
            (
                1,
                comparison(serde_json::json!([{"filename": null, "status": "modified"}])),
            ),
            (
                1,
                comparison(serde_json::json!([{"filename": "", "status": "modified"}])),
            ),
            (1, comparison(serde_json::json!([{"filename": "docs/a"}]))),
            (
                1,
                comparison(
                    serde_json::json!([{"filename": "docs/a", "previous_filename": "", "status": "renamed"}]),
                ),
            ),
        ] {
            assert_eq!(
                conservative_inventory(immutable_inventory(
                    &event(changed),
                    "https://api.github.com",
                    "limo-cad/Limo-CAD",
                    |_| Ok(response.clone())
                )),
                Builds::all()
            );
        }
        assert_eq!(
            conservative_inventory(immutable_inventory(
                &event(1),
                "https://api.github.com",
                "limo-cad/Limo-CAD",
                |_| anyhow::bail!("immutable fork SHA unavailable")
            )),
            Builds::all()
        );
        assert_eq!(
            immutable_inventory(
                &event(301),
                "https://api.github.com",
                "limo-cad/Limo-CAD",
                |_| panic!("must not read truncated comparison")
            )
            .unwrap(),
            Builds::all()
        );
    }
    #[test]
    fn current_assets_and_cargo_policy_build_every_platform() {
        for path in [
            "assets/i18n/en.json",
            "assets/ribbon-icons/save.svg",
            ".cargo/config.toml",
            ".cargo/tools.toml",
            ".gitattributes",
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
