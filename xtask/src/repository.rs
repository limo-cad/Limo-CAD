//! GitHub link ownership and a reviewed, opt-in repository move.
use anyhow::{bail, ensure, Context, Result};
use regex::{Captures, Regex};
use std::{fs, path::Path, process::Command};

pub fn slug() -> &'static str {
    include_str!("../../REPOSITORY").trim()
}

pub fn id() -> &'static str {
    include_str!("../../REPOSITORY_ID").trim()
}

pub fn api() -> String {
    format!("https://api.github.com/repos/{}", slug())
}

pub fn releases() -> String {
    format!("https://github.com/{}/releases/download", slug())
}

fn pages(slug: &str) -> String {
    let (owner, repo) = slug.split_once('/').expect("validated repository slug");
    format!("{}.github.io/{repo}", owner.to_ascii_lowercase())
}

fn valid_slug(slug: &str) -> bool {
    slug.split_once('/').is_some_and(|(owner, repo)| {
        [owner, repo].into_iter().all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
    })
}

fn retarget(text: &str, from: &str, to: &str, pages_to: Option<&str>) -> Result<String> {
    let links = Regex::new(&format!(
        r"(https://(?:github\.com|raw\.githubusercontent\.com|api\.github\.com/repos)/|https://img\.shields\.io/github/(?:[A-Za-z0-9_.-]+/)*?|git@github\.com:|`){}((?:\.git)?(?:$|[^A-Za-z0-9_.-]))",
        regex::escape(from)
    ))?;
    let text = links.replace_all(text, |c: &Captures<'_>| format!("{}{to}{}", &c[1], &c[2]));
    let ci_guards = Regex::new(&format!(
        r#"(?m)(\bGITHUB_REPOSITORY\b[^A-Za-z0-9_\r\n]*?(?:==|!=|-eq|-ne|=)\s*["']?){}($|["'\s;,)])"#,
        regex::escape(from)
    ))?;
    let text = ci_guards.replace_all(&text, |c: &Captures<'_>| format!("{}{to}{}", &c[1], &c[2]));
    let page_links = Regex::new(&format!(
        r"(?i)(https://){}((?:\.git)?(?:$|[^A-Za-z0-9_.-]))",
        regex::escape(&pages(from))
    ))?;
    let destination = pages_to.map(str::to_owned).unwrap_or_else(|| pages(to));
    Ok(page_links
        .replace_all(&text, |c: &Captures<'_>| {
            format!("{}{destination}{}", &c[1], &c[2])
        })
        .into_owned())
}

fn skipped(file: &str) -> bool {
    file.starts_with("docs/release-notes/")
        || file == "xtask/src/repository/tests.rs"
        || matches!(
            Path::new(file).file_name().and_then(|s| s.to_str()),
            Some("Cargo.lock" | "package-lock.json")
        )
        || Path::new(file)
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "png"
                        | "jpg"
                        | "jpeg"
                        | "gif"
                        | "webp"
                        | "ico"
                        | "icns"
                        | "svg"
                        | "mp4"
                        | "zip"
                        | "nbcad"
                        | "woff"
                        | "woff2"
                        | "wasm"
                        | "dmg"
                        | "deb"
                        | "pdf"
                )
            })
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut args = args.peekable();
    let (mut to, mut pages_to, mut write) = (None, None, false);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--to" if to.is_none() => {
                to = Some(
                    args.next()
                        .filter(|s| !s.starts_with("--"))
                        .context("--to needs owner/repo")?,
                )
            }
            "--pages-url" if pages_to.is_none() => {
                let value = args
                    .next()
                    .filter(|s| !s.starts_with("--"))
                    .context("--pages-url needs a host/path")?;
                pages_to = Some(
                    value
                        .trim_start_matches("https://")
                        .trim_start_matches("http://")
                        .trim_end_matches('/')
                        .to_owned(),
                );
            }
            "--write" if !write => write = true,
            "--help" | "-h" => {
                println!("cargo xtask retarget-repository --to owner/repo [--pages-url host/path] [--write]\nDefaults to a dry run; release history, lockfiles and binaries are preserved.");
                return Ok(());
            }
            _ => bail!("Unknown or duplicate retarget-repository option {arg}"),
        }
    }
    let to = to.context("Use cargo xtask retarget-repository --to owner/repo [--write]")?;
    ensure!(
        valid_slug(&to) && valid_slug(slug()),
        "Invalid repository slug"
    );
    ensure!(
        pages_to.as_ref().is_none_or(|s| !s.is_empty()
            && !s.chars().any(char::is_whitespace)
            && !s.contains(['?', '#'])),
        "Invalid Pages host/path"
    );
    let root = crate::build_tools::root();
    let files = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z"])
        .output()?;
    ensure!(
        files.status.success(),
        "Cannot enumerate tracked repository files"
    );
    let mut changes = Vec::new();
    let leftovers = Regex::new(&format!(
        r"(?i){}(?:$|[^A-Za-z0-9_-])",
        regex::escape(slug())
    ))?;
    for file in String::from_utf8(files.stdout)?
        .split('\0')
        .filter(|s| !s.is_empty() && !skipped(s))
    {
        let path = root.join(file);
        if !path.is_file() {
            continue;
        }
        ensure!(
            !fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "Tracked symlink must be handled separately: {file}"
        );
        let Ok(before) = fs::read_to_string(&path) else {
            continue;
        };
        if before.contains('\0') {
            continue;
        }
        let after = if file == "REPOSITORY" {
            format!("{to}\n")
        } else {
            retarget(&before, slug(), &to, pages_to.as_deref())?
        };
        if after != before {
            println!("{} {file}", if write { "updated" } else { "would update" });
            changes.push((path, after.clone()));
        }
        for (index, _) in after
            .lines()
            .enumerate()
            .filter(|(_, line)| leftovers.is_match(line))
        {
            println!("Review remaining mention: {file}:{}", index + 1);
        }
    }
    if write {
        for (path, after) in &changes {
            fs::write(path, after)?;
        }
    }
    println!(
        "{} files: {} -> {to}{}",
        changes.len(),
        slug(),
        if write {
            ""
        } else {
            " (dry run; use --write to apply)"
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests;
