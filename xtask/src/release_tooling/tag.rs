use anyhow::{bail, ensure, Context, Result};
use std::{
    path::Path,
    process::{Command, Output},
};

fn git(root: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .context("run git")
}

fn output(root: &Path, args: &[&str]) -> Result<String> {
    let result = git(root, args)?;
    ensure!(
        result.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

pub fn fetch_main(root: &Path) -> Result<&'static str> {
    let shallow = output(root, &["rev-parse", "--is-shallow-repository"])? == "true";
    let mut args = vec!["fetch", "--quiet", "--no-tags"];
    if shallow {
        args.push("--unshallow");
    }
    args.extend(["origin", "+refs/heads/main:refs/remotes/origin/main"]);
    output(root, &args)?;
    Ok("refs/remotes/origin/main")
}

pub fn check(root: &Path, tag: &str, sha: &str, main: &str) -> Result<()> {
    let commit = output(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{sha}^{{commit}}"),
        ],
    )?;
    let version = output(root, &["show", &format!("{commit}:VERSION")])?;
    super::version::validate(&version)?;
    ensure!(
        tag == format!("v{version}"),
        "{tag} does not name the VERSION on its commit ({version}); release tags are v<VERSION>"
    );
    let ancestry = git(root, &["merge-base", "--is-ancestor", &commit, main])?;
    match ancestry.status.code() {
        Some(0) => Ok(()),
        Some(1) => bail!(
            "{commit} is not on main ({main}); tag the merge commit after the bump has landed"
        ),
        _ => bail!(
            "could not decide whether {commit} is on main: {}",
            String::from_utf8_lossy(&ancestry.stderr)
        ),
    }
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let args: Vec<_> = args.collect();
    ensure!(
        args.len() == 2,
        "usage: cargo xtask check-release-tag TAG SHA"
    );
    let root = super::root();
    let main = fetch_main(root)?;
    check(root, &args[0], &args[1], main)?;
    println!(
        "{} names VERSION at {}, which is on main; the tag may publish.",
        args[0], args[1]
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn command(root: &Path, args: &[&str]) -> String {
        let result = Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_AUTHOR_NAME", "ci")
            .env("GIT_AUTHOR_EMAIL", "ci@example.invalid")
            .env("GIT_COMMITTER_NAME", "ci")
            .env("GIT_COMMITTER_EMAIL", "ci@example.invalid")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    }

    fn commit(root: &Path, version: &str) -> String {
        fs::write(root.join("VERSION"), format!("{version}\n")).unwrap();
        command(root, &["add", "VERSION"]);
        command(root, &["commit", "-qm", version]);
        command(root, &["rev-parse", "HEAD"])
    }

    #[test]
    fn shallow_tag_check_rejects_wrong_version_and_unmerged_commit() {
        let temp = tempfile::tempdir().unwrap();
        let origin = temp.path().join("origin");
        command(
            temp.path(),
            &["init", "-q", "-b", "main", origin.to_str().unwrap()],
        );
        let released = commit(&origin, "0.9.0");
        command(&origin, &["tag", "v0.9.0"]);
        commit(&origin, "0.9.1");
        command(&origin, &["checkout", "-qb", "feature"]);
        let stray = commit(&origin, "0.9.2");
        command(&origin, &["tag", "v0.9.2"]);
        command(&origin, &["checkout", "-q", "main"]);
        let url = format!(
            "file:///{}",
            origin
                .to_str()
                .unwrap()
                .replace('\\', "/")
                .trim_start_matches('/')
        );
        for (tag, sha, accepted) in [
            ("v0.9.0", released.as_str(), true),
            ("v0.9.2", stray.as_str(), false),
        ] {
            let clone = temp.path().join(tag);
            command(
                temp.path(),
                &[
                    "clone",
                    "-q",
                    "--depth",
                    "1",
                    "--branch",
                    tag,
                    &url,
                    clone.to_str().unwrap(),
                ],
            );
            assert_eq!(
                output(&clone, &["rev-parse", "--is-shallow-repository"]).unwrap(),
                "true"
            );
            let main = fetch_main(&clone).unwrap();
            assert_eq!(check(&clone, tag, sha, main).is_ok(), accepted);
            assert!(check(&clone, "v9.9.9", sha, main).is_err());
            assert!(check(&clone, "showcase-v0.9.0", sha, main).is_err());
        }
    }
}
