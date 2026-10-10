//! Platform-independent CI orchestration. Cargo failures and empty shards fail closed.
use crate::hash::hex;
use anyhow::{bail, ensure, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};

const FLAGSHIPS: [(&str, &str); 2] = [
    (
        "turbine",
        "turbine_replays_edits_restores_prints_and_drives_native_geometry",
    ),
    ("vise", "vise::d_screw_vise_builds_editable_native_geometry"),
];
const PROJECTS: [(&str, &str); 3] = [
    ("garden-bench", "bench.limo"),
    ("d-screw-vise", "vise.limo"),
    ("vertical-axis-turbine", "turbine.limo"),
];

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let task = args
        .next()
        .context("use ci mcp-shard SHARD, stage-demo-projects, or require-platform")?;
    let root = crate::release_tooling::root();
    match task.as_str() {
        "desktop-changes" => return crate::desktop_changes::run(args),
        "native-ignored-test" => {
            let suite = args
                .next()
                .context("missing native ignored suite (fonts or sketch-visual)")?;
            ensure!(
                args.next().is_none(),
                "unexpected native ignored suite argument"
            );
            let (filter, _) = native_ignored_suite(&suite)?;
            let arguments = native_ignored_arguments(filter);
            let mut listing = arguments.clone();
            listing.pop(); // Replace --nocapture with --list; keep the exact filter and --ignored.
            listing.push("--list".into());
            let inventory = crate::build_tools::cargo()
                .current_dir(root)
                .args(listing)
                .stderr(std::process::Stdio::inherit())
                .output()
                .context("list compiled native diagnostic tests")?;
            ensure!(
                inventory.status.success(),
                "native diagnostic inventory failed ({})",
                inventory.status
            );
            verify_native_ignored_inventory(&suite, std::str::from_utf8(&inventory.stdout)?)?;
            let status = crate::build_tools::cargo()
                .current_dir(root)
                .args(arguments)
                .status()?;
            ensure!(
                status.success(),
                "native {suite} diagnostics failed ({status})"
            );
        }
        "mcp-shard" => {
            let shard = args
                .next()
                .context("missing MCP shard (core, turbine, vise)")?;
            ensure!(args.next().is_none(), "unexpected MCP shard argument");
            let arguments = shard_arguments(&shard)?;
            let inventory = crate::build_tools::cargo()
                .current_dir(root)
                .args(mcp_inventory_arguments(false))
                .stderr(std::process::Stdio::inherit())
                .output()
                .context("list compiled MCP recipe tests")?;
            ensure!(
                inventory.status.success(),
                "Cargo inventory failed ({})",
                inventory.status
            );
            // libtest's ordinary --list includes ignored tests. A required
            // flagship can therefore be listed while its exact shard runs 0.
            let ignored = crate::build_tools::cargo()
                .current_dir(root)
                .args(mcp_inventory_arguments(true))
                .stderr(std::process::Stdio::inherit())
                .output()
                .context("list ignored MCP recipe tests")?;
            ensure!(
                ignored.status.success(),
                "ignored recipe inventory failed ({})",
                ignored.status
            );
            verify_inventory(
                std::str::from_utf8(&inventory.stdout)?,
                std::str::from_utf8(&ignored.stdout)?,
            )?;
            let status = crate::build_tools::cargo()
                .current_dir(root)
                .args(arguments)
                .status()?;
            ensure!(status.success(), "MCP {shard} shard failed ({status})");
        }
        "stage-demo-projects" => {
            ensure!(args.next().is_none(), "unexpected staging argument");
            stage_projects(
                &root.join("target/mcp-recipe-evidence"),
                &root.join("target/demo-projects"),
                &env::var("GITHUB_SHA").context("missing GITHUB_SHA")?,
                &fs::read_to_string(root.join("VERSION"))?,
            )?;
        }
        "require-platform" => {
            ensure!(args.next().is_none(), "unexpected platform gate argument");
            require_platform(
                &env::var("MCP_PLATFORM").unwrap_or_default(),
                &env::var("NATIVE_RESULT").unwrap_or_default(),
            )?;
        }
        _ => bail!("unknown CI task '{task}'"),
    }
    Ok(())
}

fn native_ignored_suite(suite: &str) -> Result<(&'static str, &'static [&'static str])> {
    match suite {
        "fonts" => Ok((
            "native_font_fallback_shapes_",
            &[
                "native_font_fallback_shapes_cjk_and_emoji_without_missing_glyphs",
                "native_font_fallback_shapes_drawing_symbols_without_missing_glyphs",
            ],
        )),
        "sketch-visual" => Ok((
            "native_sketch_boundary_visual_matrix",
            &["native_sketch_boundary_visual_matrix"],
        )),
        _ => bail!("unknown native ignored suite; use fonts or sketch-visual"),
    }
}

fn native_ignored_arguments(filter: &str) -> Vec<String> {
    [
        "test",
        "--locked",
        "--manifest-path",
        "desktop/Cargo.toml",
        "--lib",
        filter,
        "--",
        "--ignored",
        "--nocapture",
    ]
    .map(String::from)
    .into()
}

fn verify_native_ignored_inventory(suite: &str, output: &str) -> Result<()> {
    let (_, expected) = native_ignored_suite(suite)?;
    let selected = output
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .map(|test| test.rsplit("::").next().unwrap_or(test))
        .collect::<Vec<_>>();
    ensure!(selected.len() == expected.len(), "native {suite} selected {} tests, expected {}; a filter must not silently run zero or different tests", selected.len(), expected.len());
    for name in expected {
        ensure!(selected.iter().filter(|selected| *selected == name).count() == 1,
            "expected exactly one compiled native diagnostic {name}; update the guarded suite after a rename");
    }
    Ok(())
}

fn shard_arguments(shard: &str) -> Result<Vec<String>> {
    let mut args: Vec<String> = [
        "test",
        "--locked",
        "--manifest-path",
        "mcp-server/Cargo.toml",
    ]
    .map(String::from)
    .into();
    if shard == "core" {
        args.extend(["--", "--test-threads=1", "--exact"].map(String::from));
        for (_, name) in FLAGSHIPS {
            args.extend(["--skip".into(), name.into()]);
        }
    } else {
        let name = FLAGSHIPS
            .iter()
            .find(|(key, _)| *key == shard)
            .context("unknown MCP shard; use core, turbine, or vise")?
            .1;
        args.extend(
            [
                "--test",
                "recipes",
                name,
                "--",
                "--exact",
                "--test-threads=1",
            ]
            .map(String::from),
        );
    }
    Ok(args)
}

fn mcp_inventory_arguments(ignored: bool) -> Vec<String> {
    let mut args = [
        "test",
        "--locked",
        "--manifest-path",
        "mcp-server/Cargo.toml",
        "--test",
        "recipes",
        "--",
        "--list",
    ]
    .map(String::from)
    .to_vec();
    if ignored {
        args.push("--ignored".into());
    }
    args
}

fn verify_inventory(output: &str, ignored: &str) -> Result<()> {
    for (_, name) in FLAGSHIPS {
        ensure!(output.lines().filter_map(|line| line.strip_suffix(": test")).filter(|test| *test == name).count() == 1,
            "expected exactly one compiled recipe test named {name}; update CI sharding after a rename");
        ensure!(!ignored.lines().filter_map(|line| line.strip_suffix(": test")).any(|test| test == name),
            "required recipe test {name} is ignored; its exact acceptance shard would execute no test");
    }
    Ok(())
}

fn require_platform(platform: &str, result: &str) -> Result<()> {
    ensure!(
        ["windows", "linux"].contains(&platform),
        "unknown or missing MCP platform"
    );
    ensure!(
        result == "success",
        "MCP {platform} acceptance shards did not all succeed: {result}"
    );
    Ok(())
}

#[derive(Serialize)]
struct Asset<'a> {
    recipe: &'a str,
    name: &'a str,
    size: usize,
    sha256: String,
}
#[derive(Serialize)]
struct Manifest<'a> {
    schema_version: u32,
    source_commit: &'a str,
    application_version: &'a str,
    assets: Vec<Asset<'a>>,
}

fn stage_projects(source: &Path, destination: &Path, commit: &str, version: &str) -> Result<()> {
    ensure!(
        commit.len() == 40
            && commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid source commit"
    );
    let version = version.trim();
    semver::Version::parse(version).context("invalid application version")?;
    let mut assets = Vec::new();
    let mut inputs = Vec::new();
    for (recipe, name) in PROJECTS {
        let path = source.join(format!("{recipe}.limo"));
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("missing project {}", path.display()))?;
        ensure!(
            metadata.is_file() && metadata.len() > 0,
            "empty or non-regular project {}",
            path.display()
        );
        let bytes = fs::read(&path)?;
        ensure!(
            !bytes.is_empty(),
            "project became empty: {}",
            path.display()
        );
        assets.push(Asset {
            recipe,
            name,
            size: bytes.len(),
            sha256: hex(&Sha256::digest(&bytes)),
        });
        inputs.push((name, bytes));
    }
    fs::create_dir(destination).context("reserve fresh demo-project directory")?;
    let result = (|| -> Result<()> {
        let staging = tempfile::tempdir_in(destination.parent().context("demo output parent")?)?;
        for (name, bytes) in inputs {
            fs::write(staging.path().join(name), bytes)?;
        }
        let manifest = Manifest {
            schema_version: 1,
            source_commit: commit,
            application_version: version,
            assets,
        };
        fs::write(
            staging.path().join("demo-projects.json"),
            format!("{}\n", serde_json::to_string_pretty(&manifest)?),
        )?;
        fs::remove_dir(destination)?;
        fs::rename(staging.path(), destination)?;
        Ok(())
    })();
    if result.is_err() && destination.exists() {
        fs::remove_dir(destination).context("remove this attempt's empty demo reservation")?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_diagnostic_filters_reject_empty_renamed_and_duplicate_tests() {
        for suite in ["fonts", "sketch-visual"] {
            let (filter, expected) = native_ignored_suite(suite).unwrap();
            let inventory = expected
                .iter()
                .map(|name| format!("native::tests::{name}: test\r\n"))
                .collect::<String>();
            verify_native_ignored_inventory(suite, &inventory).unwrap();
            assert!(verify_native_ignored_inventory(suite, "0 tests, 0 benchmarks").is_err());
            assert!(verify_native_ignored_inventory(
                suite,
                &inventory.replace(expected[0], "renamed_test")
            )
            .is_err());
            assert!(
                verify_native_ignored_inventory(suite, &(inventory.clone() + &inventory)).is_err()
            );
            assert!(verify_native_ignored_inventory(
                suite,
                &inventory.replace(": test", ": benchmark")
            )
            .is_err());
            assert_eq!(
                native_ignored_arguments(filter),
                [
                    "test",
                    "--locked",
                    "--manifest-path",
                    "desktop/Cargo.toml",
                    "--lib",
                    filter,
                    "--",
                    "--ignored",
                    "--nocapture"
                ]
            );
        }
        assert!(native_ignored_suite("all").is_err());
    }

    #[test]
    fn compiled_shards_require_exact_unique_names() {
        let inventory = FLAGSHIPS
            .map(|(_, name)| format!("{name}: test\r\n"))
            .concat();
        verify_inventory(&inventory, "0 tests, 0 benchmarks").unwrap();
        assert!(verify_inventory("", "").is_err());
        assert!(verify_inventory(&(inventory.clone() + &inventory), "").is_err());
        assert!(verify_inventory(&inventory.replace(FLAGSHIPS[0].1, "renamed_test"), "").is_err());
        for (_, name) in FLAGSHIPS {
            assert!(verify_inventory(&inventory, &format!("{name}: test\n")).is_err());
        }
        verify_inventory(&inventory, "optional_gpu_example: test\n").unwrap();
        assert_eq!(mcp_inventory_arguments(true).last().unwrap(), "--ignored");
        assert_eq!(mcp_inventory_arguments(false).last().unwrap(), "--list");
        assert!(shard_arguments("toString").is_err());
        let core = shard_arguments("core").unwrap();
        assert!(!core.contains(&"--test".to_owned()));
        assert_eq!(&core[4..7], ["--", "--test-threads=1", "--exact"]);
        for (shard, name) in FLAGSHIPS {
            assert_eq!(
                &shard_arguments(shard).unwrap()[4..],
                [
                    "--test",
                    "recipes",
                    name,
                    "--",
                    "--exact",
                    "--test-threads=1"
                ]
            );
        }
    }
    #[test]
    fn real_libtest_ignored_flagship_is_listed_but_does_not_execute() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("inventory.rs");
        let executable = root.path().join(if cfg!(windows) {
            "inventory.exe"
        } else {
            "inventory"
        });
        fs::write(&source, format!(
            "#[test] #[ignore] fn {}() {{ panic!(\"must not execute\"); }}\nmod vise {{ #[test] fn {}() {{}} }}\n",
            FLAGSHIPS[0].1, FLAGSHIPS[1].1.strip_prefix("vise::").unwrap()
        )).unwrap();
        let compiled =
            std::process::Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
                .args(["--edition=2021", "--test"])
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .status()
                .unwrap();
        assert!(compiled.success());
        let listed = std::process::Command::new(&executable)
            .arg("--list")
            .output()
            .unwrap();
        let ignored = std::process::Command::new(&executable)
            .args(["--list", "--ignored"])
            .output()
            .unwrap();
        assert!(listed.status.success() && ignored.status.success());
        let listed = std::str::from_utf8(&listed.stdout).unwrap();
        let ignored = std::str::from_utf8(&ignored.stdout).unwrap();
        verify_inventory(listed, "").expect("The old inventory would have accepted this fixture");
        assert!(verify_inventory(listed, ignored).is_err());
        let skipped = std::process::Command::new(&executable)
            .args(["--exact", FLAGSHIPS[0].1])
            .output()
            .unwrap();
        assert!(
            skipped.status.success(),
            "libtest reports success for an ignored exact filter"
        );
        let skipped = std::str::from_utf8(&skipped.stdout).unwrap();
        assert!(skipped.contains("0 passed") && skipped.contains("1 ignored"));
    }
    #[test]
    fn platform_gate_rejects_every_non_success_result() {
        for platform in ["windows", "linux"] {
            for result in ["success", "failure", "cancelled", "skipped", ""] {
                assert_eq!(
                    require_platform(platform, result).is_ok(),
                    result == "success"
                );
            }
        }
        assert!(require_platform("", "success").is_err());
    }
    #[test]
    fn staged_projects_have_exact_provenance_and_preserve_stale_output() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("inputs");
        fs::create_dir(&source).unwrap();
        for (recipe, _) in PROJECTS {
            fs::write(source.join(format!("{recipe}.limo")), recipe).unwrap();
        }
        let dest = root.path().join("output");
        assert!(stage_projects(&source, &dest, "main", "0.2.2").is_err());
        assert!(!dest.exists());
        let missing = source.join("vertical-axis-turbine.limo");
        fs::write(&missing, "").unwrap();
        assert!(stage_projects(&source, &dest, &"a".repeat(40), "0.2.2").is_err());
        assert!(!dest.exists());
        fs::write(missing, "turbine").unwrap();
        stage_projects(&source, &dest, &"a".repeat(40), "0.2.2\n").unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(dest.join("demo-projects.json")).unwrap()).unwrap();
        for asset in manifest["assets"].as_array().unwrap() {
            let bytes = fs::read(dest.join(asset["name"].as_str().unwrap())).unwrap();
            assert_eq!(asset["size"].as_u64().unwrap(), bytes.len() as u64);
            assert_eq!(asset["sha256"], hex(&Sha256::digest(bytes)));
        }
        fs::write(dest.join("bench.limo"), "preserve").unwrap();
        assert!(stage_projects(&source, &dest, &"a".repeat(40), "0.2.2").is_err());
        assert_eq!(
            fs::read_to_string(dest.join("bench.limo")).unwrap(),
            "preserve"
        );
    }
}
