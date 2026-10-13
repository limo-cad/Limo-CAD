//! Reuse the desktop's Git identity without linking its compiled build identity
//! into xtask or invalidating the engine graph when a source revision changes.
use anyhow::{bail, ensure, Result};
use std::{env, path::Path};

#[allow(dead_code)]
#[path = "../../../crates/build-info/build.rs"]
mod build_source;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Source {
    pub revision: String,
    pub modified: bool,
}

impl Source {
    pub fn stamp(&self) -> String {
        format!(
            "{}{}",
            self.revision,
            if self.modified { ".modified" } else { "" }
        )
    }
}

fn claim(name: &str) -> Result<Option<String>> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => bail!("{name} is not valid UTF-8"),
    }
}

pub(super) fn read(root: &Path) -> Result<Source> {
    read_with_claims(
        root,
        claim("LIMO_CAD_BUILD_REVISION")?.as_deref(),
        claim("GITHUB_SHA")?.as_deref(),
    )
}

fn read_with_claims(
    root: &Path,
    build_revision: Option<&str>,
    github_sha: Option<&str>,
) -> Result<Source> {
    let actual = build_source::identity(root, None).map_err(anyhow::Error::msg)?;
    ensure!(
        actual.revision != "unknown",
        "Windows packaging requires its own Git checkout for source provenance"
    );
    for (name, expected) in [
        ("LIMO_CAD_BUILD_REVISION", build_revision),
        ("GITHUB_SHA", github_sha),
    ] {
        if let Some(expected) = expected {
            ensure!(
                actual.revision.eq_ignore_ascii_case(expected),
                "{name} does not match the checked-out HEAD"
            );
        }
    }
    Ok(Source {
        revision: actual.revision,
        modified: actual.modified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command};

    fn git(root: &Path, args: &[&str]) {
        assert!(Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn local_package_provenance_preserves_full_head_and_dirty_state_and_refuses_stale_claims() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        git(root, &["init", "--initial-branch=main"]);
        fs::write(root.join("source.txt"), "original").unwrap();
        fs::write(root.join(".gitignore"), "target/\n").unwrap();
        git(root, &["add", "."]);
        git(
            root,
            &[
                "-c",
                "user.name=Package provenance",
                "-c",
                "user.email=package@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "fixture",
            ],
        );
        let clean = read_with_claims(root, None, None).unwrap();
        assert_eq!(clean.revision.len(), 40);
        assert!(!clean.modified);
        assert_eq!(clean.stamp(), clean.revision);
        assert_eq!(
            read_with_claims(root, Some(&clean.revision), Some(&clean.revision)).unwrap(),
            clean
        );
        assert!(read_with_claims(root, Some("stale"), None).is_err());
        assert!(read_with_claims(root, None, Some(&"0".repeat(40))).is_err());
        fs::write(root.join("source.txt"), "edited").unwrap();
        let dirty = read_with_claims(root, None, None).unwrap();
        assert!(dirty.modified);
        assert_ne!(dirty, clean);
        assert_eq!(dirty.stamp(), format!("{}.modified", clean.revision));
        git(root, &["checkout", "--", "source.txt"]);
        fs::write(root.join("untracked.txt"), "new").unwrap();
        assert!(read_with_claims(root, None, None).unwrap().modified);
        fs::remove_file(root.join("untracked.txt")).unwrap();
        fs::create_dir(root.join("target")).unwrap();
        fs::write(root.join("target/ignored-output"), "generated").unwrap();
        assert_eq!(read_with_claims(root, None, None).unwrap(), clean);
        git(
            root,
            &[
                "-c",
                "user.name=Package provenance",
                "-c",
                "user.email=package@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-m",
                "next revision",
            ],
        );
        assert_ne!(read_with_claims(root, None, None).unwrap(), clean);
        assert!(read_with_claims(root, Some(&clean.revision), None).is_err());
        let nested = root.join("target/source-archive");
        fs::create_dir(&nested).unwrap();
        assert!(read_with_claims(&nested, Some(&clean.revision), Some(&clean.revision)).is_err());
    }
}
