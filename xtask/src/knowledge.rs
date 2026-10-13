//! Offline knowledge validation and deterministic Pages interchange index.
use anyhow::{bail, ensure, Context, Result};
use regex::Regex;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let task = args
        .next()
        .context("use knowledge check, index [--check], media [--verify], or site")?;
    let root = crate::release_tooling::root();
    match task.as_str() {
        "check" => {
            ensure!(args.next().is_none(), "unexpected check argument");
            check(root)?;
        }
        "index" => {
            let check = match args.next().as_deref() {
                None => false,
                Some("--check") => true,
                _ => bail!("use index [--check]"),
            };
            ensure!(args.next().is_none(), "unexpected index argument");
            let bytes = build_index(root)?;
            let path = root.join("knowledge/machine-design/search-index.json");
            if check {
                ensure!(
                    fs::read_to_string(&path)?.replace("\r\n", "\n") == bytes,
                    "help index is stale; run cargo xtask knowledge index"
                );
            } else {
                fs::write(path, bytes)?;
            }
        }
        "media" => {
            let verify = match args.next().as_deref() {
                None => false,
                Some("--verify") => true,
                _ => bail!("use media [--verify]"),
            };
            ensure!(args.next().is_none(), "unexpected media argument");
            crate::showcase_media::run(root, verify)?;
        }
        "site" => {
            ensure!(args.next().is_none(), "unexpected site argument");
            build_site(root)?;
        }
        _ => bail!("unknown knowledge task '{task}'"),
    }
    println!("Knowledge {task} passed");
    Ok(())
}

pub(super) fn markdown_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            paths.extend(markdown_files(&entry.path())?);
        } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "md") {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn frontmatter(raw: &str) -> Option<(BTreeMap<String, String>, &str)> {
    let rest = raw.strip_prefix("---\n")?;
    let (header, body) = rest.split_once("\n---\n")?;
    let mut fields = BTreeMap::new();
    for line in header.lines() {
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            if !key.is_empty() {
                fields.insert(
                    key.trim().to_owned(),
                    value.trim().trim_matches(['\'', '"']).to_owned(),
                );
            }
        }
    }
    Some((fields, body))
}

/// Check each component against its actual spelling even on case-insensitive hosts.
fn exact_file(root: &Path, absolute: &Path) -> bool {
    let Ok(relative) = absolute.strip_prefix(root) else {
        return false;
    };
    let mut current = root.to_owned();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return false;
        };
        let Ok(entries) = fs::read_dir(&current) else {
            return false;
        };
        if !entries
            .filter_map(Result::ok)
            .any(|entry| entry.file_name() == name)
        {
            return false;
        }
        current.push(name);
    }
    current.is_file()
        && current
            .canonicalize()
            .is_ok_and(|path| root.canonicalize().is_ok_and(|base| path.starts_with(base)))
}

fn resolve(base: &Path, target: &str) -> PathBuf {
    let mut output = base.to_owned();
    for part in Path::new(target).components() {
        match part {
            Component::ParentDir => {
                output.pop();
            }
            Component::CurDir => {}
            other => output.push(other.as_os_str()),
        }
    }
    output
}

/// Validate source URLs against their Git revision, including refs containing `/`.
struct RepositoryLinks<'a> {
    root: &'a Path,
    refs: BTreeMap<String, String>,
    trees: BTreeMap<String, BTreeMap<String, String>>,
}

impl<'a> RepositoryLinks<'a> {
    fn new(root: &'a Path) -> Result<Self> {
        let output = Command::new("git")
            .current_dir(root)
            .args(["for-each-ref", "--format=%(refname)"])
            .output()?;
        ensure!(
            output.status.success(),
            "cannot read repository source refs"
        );
        let names = String::from_utf8(output.stdout)?;
        let mut refs = BTreeMap::new();
        for prefix in ["refs/tags/", "refs/heads/", "refs/remotes/origin/"] {
            for name in names.lines() {
                if let Some(short) = name.strip_prefix(prefix) {
                    refs.insert(short.to_owned(), name.to_owned());
                }
            }
        }
        refs.insert("HEAD".into(), "HEAD".into());
        Ok(Self {
            root,
            refs,
            trees: BTreeMap::new(),
        })
    }

    fn check(&mut self, target: &str, anchor: &str) -> Result<()> {
        let (source, path) = self
            .refs
            .iter()
            .filter_map(|(short, full)| {
                target
                    .strip_prefix(short)
                    .and_then(|rest| rest.strip_prefix('/'))
                    .map(|path| (short.len(), full.as_str(), path))
            })
            .max_by_key(|(length, _, _)| *length)
            .map(|(_, full, path)| (full, path))
            .or_else(|| {
                let (source, path) = target.split_once('/')?;
                ((7..=64).contains(&source.len())
                    && source.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then_some((source, path))
            })
            .with_context(|| {
                format!("source revision unavailable for {target}; fetch the linked Git ref (CI uses fetch-depth: 0)")
            })?;
        ensure!(
            !path.contains('\\') && path.split('/').all(|part| !matches!(part, "" | "." | "..")),
            "invalid repository source path: {path}"
        );
        if !self.trees.contains_key(source) {
            let output = Command::new("git")
                .current_dir(self.root)
                .args(["ls-tree", "-r", "-z", "--full-tree", source])
                .output()?;
            ensure!(
                output.status.success(),
                "cannot read source revision {source}"
            );
            let mut files = BTreeMap::new();
            for entry in String::from_utf8(output.stdout)?.split('\0') {
                let Some((metadata, path)) = entry.split_once('\t') else {
                    continue;
                };
                let fields: Vec<_> = metadata.split_whitespace().collect();
                if fields.len() == 3
                    && matches!(fields[0], "100644" | "100755")
                    && fields[1] == "blob"
                {
                    files.insert(path.to_owned(), fields[2].to_owned());
                }
            }
            self.trees.insert(source.to_owned(), files);
        }
        let blob = self.trees[source]
            .get(path)
            .with_context(|| format!("missing or incorrectly cased target at {source}: {path}"))?;
        if !anchor.is_empty() && path.ends_with(".html") {
            let output = Command::new("git")
                .current_dir(self.root)
                .args(["cat-file", "blob", blob])
                .output()?;
            ensure!(output.status.success(), "cannot read source file {path}");
            let linked = String::from_utf8(output.stdout)?;
            ensure!(
                linked.contains(&format!("id=\"{anchor}\""))
                    || linked.contains(&format!("id='{anchor}'")),
                "missing page anchor: {target}#{anchor}"
            );
        }
        Ok(())
    }
}

fn ids(fields: &BTreeMap<String, String>, key: &str) -> Result<Vec<String>> {
    let Some(value) = fields
        .get(key)
        .filter(|value| !value.is_empty() && *value != "[]")
    else {
        return Ok(Vec::new());
    };
    let values: Vec<_> = value.split(',').map(|s| s.trim().to_owned()).collect();
    let valid = Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")?;
    ensure!(
        values.iter().all(|id| valid.is_match(id)),
        "{key} must contain comma-separated ids or []"
    );
    ensure!(
        values.iter().collect::<BTreeSet<_>>().len() == values.len(),
        "duplicate {key} reference"
    );
    Ok(values)
}

fn check(root: &Path) -> Result<()> {
    let bundle = root.join("knowledge");
    let files = markdown_files(&bundle)?;
    let index = bundle.join("index.md");
    let log = bundle.join("log.md");
    let sources = bundle.join("machine-design/SOURCES.md");
    let mut failures = Vec::new();
    let mut fail = |path: &Path, message: String| {
        failures.push(format!(
            "{}: {message}",
            path.strip_prefix(root).unwrap_or(path).display()
        ))
    };
    let row_id = Regex::new(r"^`([a-z0-9]+(?:-[a-z0-9]+)*)`$")?;
    let link = Regex::new(r"\[[^\]]+\]\(https://[^)]+\)")?;
    let mut source_ids = BTreeSet::new();
    for line in fs::read_to_string(&sources)?
        .lines()
        .filter(|line| line.starts_with("| `"))
    {
        let cols: Vec<_> = line.split('|').map(str::trim).collect();
        let Some(id) = row_id
            .captures(cols.get(1).copied().unwrap_or_default())
            .map(|c| c[1].to_owned())
        else {
            fail(&sources, "invalid source id".into());
            continue;
        };
        if !source_ids.insert(id.clone()) {
            fail(&sources, format!("duplicate source id: {id}"));
        }
        if !link.is_match(cols.get(2).copied().unwrap_or_default())
            || cols.get(3).is_none_or(|s| s.is_empty())
            || !link.is_match(cols.get(4).copied().unwrap_or_default())
        {
            fail(
                &sources,
                format!("source {id} requires a primary reference, author and license link"),
            );
        }
    }
    if source_ids.is_empty() {
        fail(&sources, "source inventory is empty".into());
    }
    let index_raw = fs::read_to_string(&index)?.replace("\r\n", "\n");
    match frontmatter(&index_raw) {
        Some((fields, _)) if fields.get("okf_version").is_some_and(|v| v == "0.2") => {
            if fields.keys().any(|key| key != "okf_version") {
                fail(&index, "unexpected index frontmatter".into());
            }
        }
        _ => fail(
            &index,
            "root index must declare okf_version: \"0.2\"".into(),
        ),
    }
    let log_raw = fs::read_to_string(&log)?.replace("\r\n", "\n");
    if frontmatter(&log_raw).is_some() {
        fail(
            &log,
            "reserved log.md must not use concept frontmatter".into(),
        );
    }
    let dates: Vec<_> = Regex::new(r"(?m)^## (\d{4}-\d{2}-\d{2})$")?
        .captures_iter(&log_raw)
        .map(|c| c[1].to_owned())
        .collect();
    if dates.is_empty() {
        fail(&log, "must contain ISO-date section headings".into());
    } else if dates.windows(2).any(|pair| pair[0] < pair[1]) {
        fail(&log, "date sections must be newest first".into());
    }
    let mut used = BTreeSet::new();
    let markdown_links = Regex::new(r"\[[^\]]*\]\(([^)]+)\)")?;
    for file in &files {
        let raw = fs::read_to_string(file)?.replace("\r\n", "\n");
        if *file != index && *file != log {
            if let Some((fields, _)) = frontmatter(&raw) {
                if fields.get("type").is_none_or(|v| v.is_empty()) {
                    fail(file, "concept frontmatter requires type".into());
                }
                if fields
                    .get("status")
                    .is_some_and(|s| !["draft", "stable", "deprecated"].contains(&s.as_str()))
                {
                    fail(file, "unsupported OKF lifecycle status".into());
                }
                if file.starts_with(bundle.join("machine-design/concepts")) {
                    match ids(&fields, "sources") {
                        Ok(references) => {
                            if references.is_empty() {
                                fail(file, "mechanical-design article requires sources".into());
                            }
                            for id in references {
                                if !source_ids.contains(&id) {
                                    fail(file, format!("unknown source id: {id}"));
                                }
                                used.insert(id);
                            }
                        }
                        Err(error) => fail(file, error.to_string()),
                    }
                }
                match ids(&fields, "related_recipes") {
                    Ok(recipes) => {
                        for id in recipes {
                            if !exact_file(
                                root,
                                &root.join(format!("examples/scripts/{id}.limo.jsonc")),
                            ) {
                                fail(file, format!("missing recipe: {id}"));
                            }
                        }
                    }
                    Err(error) => fail(file, error.to_string()),
                }
            } else {
                fail(file, "concept must start with YAML frontmatter".into());
            }
        }
        for capture in markdown_links.captures_iter(&raw) {
            if capture
                .get(0)
                .is_some_and(|m| m.start() > 0 && raw.as_bytes()[m.start() - 1] == b'!')
            {
                continue;
            }
            let target = capture[1].split('#').next().unwrap_or_default();
            if target.is_empty()
                || ["http://", "https://", "mailto:"]
                    .iter()
                    .any(|s| target.starts_with(s))
            {
                continue;
            }
            if !exact_file(root, &resolve(file.parent().unwrap(), target)) {
                fail(
                    file,
                    format!("missing or incorrectly cased local link: {target}"),
                );
            }
        }
    }
    for id in source_ids.difference(&used) {
        fail(&sources, format!("unused source id: {id}"));
    }
    let attributes = Regex::new(r#"(?:href|src|poster)=["']([^"']+)["']"#)?;
    let repository = regex::escape(crate::repository::slug());
    let repo_link = Regex::new(&format!(
        r"^https://(?:github\.com/{repository}/blob/|raw\.githubusercontent\.com/{repository}/)(.+)$",
    ))?;
    let any_repository =
        Regex::new(r"(?i)^https://(?:github\.com/[^/]+/[^/]+/blob/|raw\.githubusercontent\.com/)")?;
    let scheme = Regex::new(r"(?i)^[a-z][a-z0-9+.-]*:")?;
    let mut repository_links = None;
    for entry in fs::read_dir(&bundle)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() || entry.path().extension().is_none_or(|ext| ext != "html")
        {
            continue;
        }
        let path = entry.path();
        let raw = fs::read_to_string(&path)?;
        let generated: BTreeSet<String> = if entry.file_name() == "showcase.html" {
            match crate::showcase_media::inputs(&raw) {
                Ok(inputs) => inputs.into_iter().map(|input| input.src).collect(),
                Err(error) => {
                    fail(&path, error.to_string());
                    BTreeSet::new()
                }
            }
        } else {
            BTreeSet::new()
        };
        for capture in attributes.captures_iter(&raw) {
            let (target, anchor) = capture[1].split_once('#').unwrap_or((&capture[1], ""));
            if anchor.is_empty() && generated.contains(target) {
                continue;
            }
            let absolute = if target.is_empty() {
                path.clone()
            } else if !scheme.is_match(target) {
                resolve(&bundle, target)
            } else if let Some(c) = repo_link.captures(target) {
                if repository_links.is_none() {
                    repository_links = Some(RepositoryLinks::new(root)?);
                }
                if let Err(error) = repository_links.as_mut().unwrap().check(&c[1], anchor) {
                    fail(&path, format!("{target}: {error}"));
                }
                continue;
            } else {
                if any_repository.is_match(target) {
                    fail(
                        &path,
                        format!(
                            "repository link must use {}: {target}",
                            crate::repository::slug()
                        ),
                    );
                }
                continue;
            };
            if !exact_file(root, &absolute) {
                fail(
                    &path,
                    format!("missing or incorrectly cased target: {target}"),
                );
                continue;
            }
            if !anchor.is_empty() && absolute.extension().is_some_and(|ext| ext == "html") {
                let linked = fs::read_to_string(absolute)?;
                if !linked.contains(&format!("id=\"{anchor}\""))
                    && !linked.contains(&format!("id='{anchor}'"))
                {
                    fail(&path, format!("missing page anchor: {}", &capture[1]));
                }
            }
        }
    }
    ensure!(
        failures.is_empty(),
        "Knowledge bundle validation failed:\n{}",
        failures.join("\n")
    );
    Ok(())
}

#[derive(Serialize)]
struct Entry {
    id: String,
    path: String,
    title: String,
    description: String,
    status: String,
    topics: Vec<String>,
    keywords: Vec<String>,
    related_recipes: Vec<String>,
    sources: Vec<String>,
    snippet: String,
}
#[derive(Serialize)]
struct Index {
    version: u32,
    count: usize,
    entries: Vec<Entry>,
}
fn list(value: Option<&String>) -> Vec<String> {
    value
        .into_iter()
        .flat_map(|s| s.trim_matches(['[', ']']).split(','))
        .map(|s| s.trim().trim_matches('`').to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}
fn snippet(body: &str) -> Result<String> {
    let body = Regex::new(r"(?m)^#.+$|^>.+$")?.replace_all(body, "");
    let body = Regex::new(r"\[([^\]]+)\]\([^)]+\)")?.replace_all(&body, "$1");
    let body = Regex::new(r"[*_`]")?.replace_all(&body, "");
    let body = Regex::new(r"\s+")?
        .replace_all(&body, " ")
        .trim()
        .to_owned();
    if body.encode_utf16().count() <= 220 {
        return Ok(body);
    }
    let prefix: Vec<u16> = body.encode_utf16().take(219).collect();
    Ok(format!("{}…", String::from_utf16_lossy(&prefix)))
}
fn build_index(root: &Path) -> Result<String> {
    let mut entries = Vec::new();
    let title_pattern = Regex::new(r"(?m)^#\s+(.+)$")?;
    for path in markdown_files(&root.join("knowledge/machine-design"))? {
        let raw = fs::read_to_string(&path)?.replace("\r\n", "\n");
        let Some((fields, body)) = frontmatter(&raw) else {
            continue;
        };
        let relative = path
            .strip_prefix(root.join("knowledge"))?
            .to_string_lossy()
            .replace('\\', "/");
        if fields.get("searchable").is_some_and(|v| v == "false")
            || fields.get("type").is_none_or(|v| v != "Concept")
            || !relative.contains("/concepts/")
        {
            continue;
        }
        let get = |key: &str| fields.get(key).cloned().unwrap_or_default();
        let title = fields
            .get("title")
            .filter(|s| !s.is_empty())
            .cloned()
            .or_else(|| title_pattern.captures(body).map(|c| c[1].to_owned()))
            .unwrap_or_else(|| path.file_stem().unwrap().to_string_lossy().into_owned());
        entries.push(Entry {
            id: relative.trim_end_matches(".md").replace('/', "."),
            path: format!("knowledge/{relative}"),
            title,
            description: get("description"),
            status: get("status"),
            topics: list(fields.get("topics")),
            keywords: list(fields.get("keywords")),
            related_recipes: list(fields.get("related_recipes")),
            sources: list(fields.get("sources")),
            snippet: snippet(body)?,
        });
    }
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&Index {
            version: 1,
            count: entries.len(),
            entries
        })?
    ))
}

fn build_site(root: &Path) -> Result<()> {
    let site = root.join("_site");
    ensure!(
        !site.exists(),
        "refusing stale _site; move the previous output before rebuilding"
    );
    let staging = tempfile::tempdir_in(root)?;
    fn copy(source: &Path, dest: &Path) -> Result<()> {
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                fs::create_dir(dest.join(entry.file_name()))?;
                copy(&entry.path(), &dest.join(entry.file_name()))?;
            } else {
                ensure!(kind.is_file(), "non-regular knowledge asset");
                fs::copy(entry.path(), dest.join(entry.file_name()))?;
            }
        }
        Ok(())
    }
    copy(&root.join("knowledge"), staging.path())?;
    fs::copy(
        staging.path().join("home.html"),
        staging.path().join("index.html"),
    )?;
    fs::write(staging.path().join(".nojekyll"), "")?;
    ensure!(
        staging.path().join("index.md").is_file(),
        "missing knowledge index"
    );
    fs::rename(staging.path(), site)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(root)
            .args([
                "-c",
                "user.name=Knowledge Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=unused-fixture-hooks",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn source_repository() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "--quiet", "--initial-branch=main"]);
        fs::write(root.path().join("part.nbcad.jsonc"), "{}").unwrap();
        fs::write(root.path().join("page.html"), "<div id='old'></div>").unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "--quiet", "-m", "Before rename"]);
        git(root.path(), &["tag", "v0.2.0"]);
        git(
            root.path(),
            &["checkout", "--quiet", "-b", "feat/bevy-interface"],
        );
        fs::rename(
            root.path().join("part.nbcad.jsonc"),
            root.path().join("part.limo.jsonc"),
        )
        .unwrap();
        fs::write(root.path().join("page.html"), "<div id='new'></div>").unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "--quiet", "-m", "After rename"]);
        root
    }

    #[test]
    fn repository_links_use_tagged_files_and_slashed_branch_refs() {
        let root = source_repository();
        let mut links = RepositoryLinks::new(root.path()).unwrap();
        links.check("v0.2.0/part.nbcad.jsonc", "").unwrap();
        links
            .check("feat/bevy-interface/part.limo.jsonc", "")
            .unwrap();
        links.check("v0.2.0/page.html", "old").unwrap();
        links.check("feat/bevy-interface/page.html", "new").unwrap();
        for (target, anchor) in [
            ("v0.2.0/part.limo.jsonc", ""),
            ("feat/bevy-interface/part.nbcad.jsonc", ""),
            ("feat/bevy-interface/Part.limo.jsonc", ""),
            ("feat/bevy-interface/../part.limo.jsonc", ""),
            ("feat/bevy-interface/dir\\part.limo.jsonc", ""),
            ("v0.2.0/page.html", "new"),
            ("feat/bevy-interface/page.html", "old"),
        ] {
            assert!(links.check(target, anchor).is_err(), "{target}#{anchor}");
        }
        let revision = git(root.path(), &["rev-parse", "HEAD"]);
        links
            .check(&format!("{}/part.limo.jsonc", revision.trim()), "")
            .unwrap();
        assert_eq!(links.trees.len(), 3);
    }

    #[test]
    fn shallow_source_links_require_the_linked_revision() {
        let source = source_repository();
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(
            root.path(),
            &[
                "fetch",
                "--quiet",
                "--no-tags",
                "--depth=1",
                source.path().to_str().unwrap(),
                "refs/heads/feat/bevy-interface",
            ],
        );
        git(
            root.path(),
            &["checkout", "--quiet", "--detach", "FETCH_HEAD"],
        );
        let mut links = RepositoryLinks::new(root.path()).unwrap();
        links.check("HEAD/part.limo.jsonc", "").unwrap();
        assert!(links
            .check("v0.2.0/part.nbcad.jsonc", "")
            .unwrap_err()
            .to_string()
            .contains("source revision unavailable"));
        git(
            root.path(),
            &[
                "fetch",
                "--quiet",
                "--depth=1",
                source.path().to_str().unwrap(),
                "refs/tags/v0.2.0:refs/tags/v0.2.0",
            ],
        );
        RepositoryLinks::new(root.path())
            .unwrap()
            .check("v0.2.0/part.nbcad.jsonc", "")
            .unwrap();
    }

    #[test]
    fn real_index_is_byte_identical_and_bundle_valid() {
        let root = crate::release_tooling::root();
        check(root).unwrap();
        assert_eq!(
            build_index(root).unwrap(),
            fs::read_to_string(root.join("knowledge/machine-design/search-index.json"))
                .unwrap()
                .replace("\r\n", "\n")
        );
    }
    #[test]
    fn references_reject_paths_duplicates_and_malformed_ids() {
        let mut fields = BTreeMap::new();
        for bad in ["../part", "part, part", "[part]"] {
            fields.insert("related_recipes".into(), bad.into());
            assert!(ids(&fields, "related_recipes").is_err());
        }
        fields.insert("related_recipes".into(), "[]".into());
        assert!(ids(&fields, "related_recipes").unwrap().is_empty());
    }

    #[test]
    fn authored_bundle_gate_preserves_attribution_recipe_and_lifecycle_rejections() {
        let source_row = "| `example-source` | [Original](https://example.org/source) | Example Author | [License](https://example.org/license) |";
        let article = "---\ntype: Concept\nstatus: stable\nsources: example-source\nrelated_recipes: example-part\n---\n# Example\n[Sources](../SOURCES.md)\n";
        for (target, before, after, expected) in [
            (
                "article",
                "sources: example-source",
                "sources: missing-source",
                "unknown source id",
            ),
            (
                "article",
                "sources: example-source",
                "sources: []",
                "article requires sources",
            ),
            ("article", "example-part", "missing-part", "missing recipe"),
            (
                "article",
                "example-part",
                "../example-part",
                "comma-separated ids",
            ),
            (
                "article",
                "example-part",
                "example-part, example-part",
                "duplicate related_recipes",
            ),
            (
                "article",
                "../SOURCES.md",
                "../sources.md",
                "incorrectly cased local link",
            ),
            (
                "article",
                "status: stable",
                "status: nonsense",
                "unsupported OKF lifecycle",
            ),
            (
                "sources",
                "| Example Author |",
                "|  |",
                "requires a primary reference",
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir_all(root.path().join("knowledge/machine-design/concepts")).unwrap();
            fs::create_dir_all(root.path().join("examples/scripts")).unwrap();
            fs::write(
                root.path().join("examples/scripts/example-part.limo.jsonc"),
                "{}",
            )
            .unwrap();
            fs::write(
                root.path().join("knowledge/index.md"),
                "---\nokf_version: \"0.2\"\n---\n# Knowledge\n",
            )
            .unwrap();
            fs::write(
                root.path().join("knowledge/log.md"),
                "# Updates\n\n## 2026-09-13\n",
            )
            .unwrap();
            let sources =
                format!("---\ntype: Concept\nstatus: stable\n---\n# Sources\n{source_row}\n");
            let article_path = root
                .path()
                .join("knowledge/machine-design/concepts/example.md");
            let sources_path = root.path().join("knowledge/machine-design/SOURCES.md");
            fs::write(&article_path, article.replace('\n', "\r\n")).unwrap();
            fs::write(&sources_path, &sources).unwrap();
            check(root.path()).unwrap();
            if target == "article" {
                fs::write(article_path, article.replace(before, after)).unwrap();
            } else {
                fs::write(sources_path, sources.replace(before, after)).unwrap();
            }
            assert!(
                check(root.path())
                    .unwrap_err()
                    .to_string()
                    .contains(expected),
                "{expected}"
            );
        }
    }
    #[test]
    fn links_check_case_and_stay_inside_repository() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("SOURCES.md"), "source").unwrap();
        assert!(exact_file(root.path(), &root.path().join("SOURCES.md")));
        assert!(!exact_file(root.path(), &root.path().join("sources.md")));
        assert!(!exact_file(root.path(), &root.path().join("../SOURCES.md")));
    }
}
