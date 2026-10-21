//! SBOMs describe final release archives; lockfile inventories are separately labeled.
mod documents;
pub(crate) mod native;
#[cfg(test)]
mod tests;
mod vcpkg;

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) const SYFT_VERSION: &str = "1.54.1";
pub(crate) const AUDITABLE_VERSION: &str = "0.7.7";
const REPOSITORY: &str = "https://github.com/limo-cad/Limo-CAD";
const LOCKFILES: &[&str] = &[
    "Cargo.lock",
    "desktop/Cargo.lock",
    "mcp-server/Cargo.lock",
    "experiments/rust-simulation/Cargo.lock",
];

#[derive(Debug, Default)]
struct Options {
    artifact: Option<PathBuf>,
    sdk: Option<PathBuf>,
    target: Option<String>,
    directory: Option<PathBuf>,
    github_output: bool,
    require_release_evidence: bool,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut options = Self::default();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--artifact" if options.artifact.is_none() => {
                    options.artifact =
                        Some(args.next().context("--artifact requires a path")?.into())
                }
                "--sdk" if options.sdk.is_none() => {
                    options.sdk = Some(args.next().context("--sdk requires a prefix")?.into())
                }
                "--target" if options.target.is_none() => {
                    options.target = Some(args.next().context("--target requires a triple")?)
                }
                "--directory" if options.directory.is_none() => {
                    options.directory =
                        Some(args.next().context("--directory requires a path")?.into())
                }
                "--github-output" if !options.github_output => options.github_output = true,
                "--require-release-evidence" if !options.require_release_evidence => {
                    options.require_release_evidence = true
                }
                _ => bail!("unknown or duplicate SBOM option {arg}"),
            }
        }
        Ok(options)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Evidence {
    schema_version: u32,
    generation_context: String,
    version: String,
    source_revision: String,
    source_modified: bool,
    target: String,
    artifact: String,
    artifact_sha256: String,
    syft_version: String,
    cargo_auditable_version: String,
    input_sha256: BTreeMap<String, String>,
    report_sha256: BTreeMap<String, String>,
    native_sdk: Value,
    native_libraries: Vec<native::Library>,
    unknown_information: Vec<String>,
}

fn json_file(path: &Path) -> Result<Value> {
    serde_json::from_slice(&fs::read(path)?).with_context(|| format!("read {}", path.display()))
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().context("report directory")?)?;
    serde_json::to_writer_pretty(file.as_file_mut(), value)?;
    writeln!(file)?;
    file.persist(path)?;
    Ok(())
}

fn output(command: &mut Command) -> Result<String> {
    let result = command
        .output()
        .with_context(|| format!("start {command:?}"))?;
    ensure!(
        result.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?.trim().into())
}

fn execute(command: &mut Command) -> Result<()> {
    ensure!(command.status()?.success(), "{command:?} failed");
    Ok(())
}

fn syft() -> Result<Command> {
    let program = env::var_os("SYFT_BINARY").unwrap_or_else(|| "syft".into());
    let version: Value = serde_json::from_str(&output(
        Command::new(&program).args(["version", "--output", "json"]),
    )?)?;
    ensure!(
        version["version"] == SYFT_VERSION,
        "SBOMs require Syft {SYFT_VERSION}"
    );
    Ok(Command::new(program))
}

fn scan(payload: &Path, work: &Path, catalogers: &str, name: &str, version: &str) -> Result<Value> {
    let config = work.join("syft.yaml");
    fs::write(&config, "file:\n  metadata:\n    selection: none\n")?;
    execute(
        syft()?
            .current_dir(payload)
            .args([
                "scan",
                "dir:.",
                "--source-name",
                name,
                "--source-version",
                version,
                "--override-default-catalogers",
                catalogers,
                "--select-catalogers=-file",
            ])
            .arg("--config")
            .arg(&config)
            .arg("--output")
            .arg(format!(
                "cyclonedx-json@1.6={}",
                work.join("scan.cdx.json").display()
            ))
            .arg("--output")
            .arg(format!(
                "syft-json={}",
                work.join("scan.syft.json").display()
            ))
            .env("SYFT_CACHE_DIR", work.join("cache")),
    )?;
    json_file(&work.join("scan.cdx.json"))
}

fn input_hashes(root: &Path) -> Result<BTreeMap<String, String>> {
    let listed = output(Command::new("git").current_dir(root).args([
        "ls-files",
        "-z",
        "--",
        ".gitattributes",
        "VERSION",
        "Cargo.lock",
        "desktop/Cargo.lock",
        "mcp-server/Cargo.lock",
        "experiments/rust-simulation/Cargo.lock",
        "Cargo.toml",
        "**/Cargo.toml",
        "rust-toolchain.toml",
        "native/occt-overlay",
        "vcpkg.json",
        "vcpkg-configuration.json",
        "xtask/src/sbom.rs",
        "xtask/src/sbom",
        "xtask/src/occt_sdk.rs",
        "xtask/src/package",
        ".github/actions/setup-release-sbom",
        ".github/actions/release-sbom",
        ".github/workflows/desktop-packages.yml",
    ]))?;
    listed
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(|path| Ok((path.into(), crate::hash::file(&root.join(path))?)))
        .collect()
}

fn revision(root: &Path) -> Result<(String, bool)> {
    let revision = output(
        Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"]),
    )?;
    ensure!(
        revision.len() == 40 && revision.chars().all(|c| c.is_ascii_hexdigit()),
        "unknown source revision"
    );
    let dirty = !output(Command::new("git").current_dir(root).args([
        "status",
        "--porcelain",
        "--untracked-files=normal",
    ]))?
    .is_empty();
    if let Ok(claim) = env::var("GITHUB_SHA") {
        ensure!(claim == revision, "GITHUB_SHA differs from checkout");
    }
    Ok((revision, dirty))
}

fn unpack(artifact: &Path, directory: &Path) -> Result<Option<PathBuf>> {
    let name = artifact
        .file_name()
        .and_then(|v| v.to_str())
        .context("UTF-8 archive name")?;
    fs::create_dir(directory)?;
    if name.ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(fs::File::open(artifact)?)?;
        for index in 0..archive.len() {
            let mut file = archive.by_index(index)?;
            let relative = file.enclosed_name().context("ZIP path escapes payload")?;
            ensure!(
                file.unix_mode()
                    .is_none_or(|mode| mode & 0o170000 != 0o120000),
                "portable ZIP contains a symlink"
            );
            let target = directory.join(relative);
            if file.is_dir() {
                fs::create_dir_all(&target)?;
            } else {
                fs::create_dir_all(target.parent().unwrap())?;
                std::io::copy(&mut file, &mut fs::File::create(target)?)?;
            }
        }
    } else if name.ends_with(".deb") {
        execute(
            Command::new("dpkg-deb")
                .arg("--extract")
                .arg(artifact)
                .arg(directory),
        )?;
    } else if name.ends_with(".AppImage") {
        execute(
            Command::new(artifact)
                .current_dir(directory)
                .arg("--appimage-extract"),
        )?;
    } else if name.ends_with(".dmg") {
        execute(
            Command::new("hdiutil")
                .args(["attach", "-readonly", "-nobrowse", "-mountpoint"])
                .arg(directory)
                .arg(artifact),
        )?;
        return Ok(Some(directory.into()));
    } else {
        bail!("unsupported release archive: {name}");
    }
    Ok(None)
}

fn convert(cdx: &Path, spdx: &Path) -> Result<()> {
    execute(
        syft()?
            .arg("convert")
            .arg(cdx)
            .arg("--output")
            .arg(format!("spdx-json@2.3={}", spdx.display())),
    )?;
    let cdx = json_file(cdx)?;
    let mut document = json_file(spdx)?;
    documents::preserve_rust_git_sources(&cdx, &mut document)?;
    write_json(spdx, &document)
}

fn release(root: &Path, options: Options) -> Result<()> {
    ensure!(
        options.directory.is_none() && !options.require_release_evidence,
        "release only accepts --artifact, --sdk, --target and --github-output"
    );
    ensure!(env::var("LIMO_CAD_AUDITABLE_VERSION").as_deref() == Ok(AUDITABLE_VERSION), "record the pinned cargo-auditable installation with LIMO_CAD_AUDITABLE_VERSION={AUDITABLE_VERSION}");
    let artifact = options
        .artifact
        .context("release requires --artifact")?
        .canonicalize()?;
    let sdk = options
        .sdk
        .context("release requires --sdk")?
        .canonicalize()?;
    let target = options.target.context("release requires --target")?;
    ensure!(
        [
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin"
        ]
        .contains(&target.as_str()),
        "unsupported SBOM target"
    );
    let version = crate::release_tooling::version::read(root)?;
    let (source_revision, source_modified) = revision(root)?;
    let input_sha256 = input_hashes(root)?;
    let artifact_sha256 = crate::hash::file(&artifact)?;
    let name = artifact
        .file_name()
        .and_then(|v| v.to_str())
        .context("UTF-8 artifact")?
        .to_owned();
    ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)),
        "unsafe artifact name"
    );
    let temporary = tempfile::tempdir()?;
    let payload = temporary.path().join("payload");
    let mount = unpack(&artifact, &payload)?;
    let result = (|| {
        let (native_libraries, native_sdk) = native::inventory(&payload, &sdk, &target, &version)?;
        let mut cdx = scan(&payload, temporary.path(), "cargo-auditable-binary-cataloger,elf-binary-package-cataloger,pe-binary-package-cataloger", &name, &version)?;
        let executable = documents::auditable_root(&cdx, &version)?;
        documents::enrich_crate_licenses(root, &target, &mut cdx)?;
        documents::add_release(
            &mut cdx,
            &native_libraries,
            &native_sdk,
            &version,
            &source_revision,
            &target,
            &artifact_sha256,
            &executable,
        )?;
        documents::validate(&cdx)?;
        let cdx_name = format!("{name}.sbom.cdx.json");
        let spdx_name = format!("{name}.sbom.spdx.json");
        let metadata_name = format!("{name}.sbom.metadata.json");
        let cdx_path = temporary.path().join(&cdx_name);
        let spdx_path = temporary.path().join(&spdx_name);
        write_json(&cdx_path, &cdx)?;
        convert(&cdx_path, &spdx_path)?;
        let mut spdx = json_file(&spdx_path)?;
        documents::enrich_spdx(
            &mut spdx,
            &native_libraries,
            &name,
            &artifact_sha256,
            &version,
            &source_revision,
            &target,
        )?;
        documents::preserve_rust_graph(&cdx, &mut spdx)?;
        documents::verify_spdx(&spdx, &native_libraries, &name, &artifact_sha256)?;
        write_json(&spdx_path, &spdx)?;
        let report_sha256 = BTreeMap::from([
            (cdx_name.clone(), crate::hash::file(&cdx_path)?),
            (spdx_name.clone(), crate::hash::file(&spdx_path)?),
        ]);
        let unknown_information = native_libraries.iter().filter(|library| library.version == "unknown" || library.producer == "unknown" || library.license == "NOASSERTION")
            .map(|library| format!("{}: version={}, producer={}, license={}", library.path, library.version, library.producer, library.license))
            .chain(std::iter::once("Host OS, graphics drivers and external runtime versions are supplied by the consumer; they are outside this archive's inventory.".into()))
            .chain(std::iter::once("Rust component producers are unknown when not identified by upstream metadata. Component inclusion and signatures do not establish vulnerability exploitability.".into()))
            .collect();
        let evidence = Evidence {
            schema_version: 1,
            generation_context: "after build; extracted release archive".into(),
            version,
            source_revision,
            source_modified,
            target,
            artifact: name.clone(),
            artifact_sha256,
            syft_version: SYFT_VERSION.into(),
            cargo_auditable_version: AUDITABLE_VERSION.into(),
            input_sha256,
            report_sha256,
            native_sdk,
            native_libraries,
            unknown_information,
        };
        write_json(&temporary.path().join(&metadata_name), &evidence)?;
        let reports = [&cdx_name, &spdx_name, &metadata_name];
        let sums = reports
            .iter()
            .map(|file| {
                Ok(format!(
                    "{}  {file}\n",
                    crate::hash::file(&temporary.path().join(file))?
                ))
            })
            .collect::<Result<String>>()?;
        let parent = artifact.parent().unwrap();
        for file in reports {
            fs::copy(temporary.path().join(file), parent.join(file))?;
        }
        let checksum_name = format!("{name}.sbom.SHA256SUMS.txt");
        fs::write(parent.join(&checksum_name), sums)?;
        verify_one(root, parent, &evidence)?;
        if options.github_output {
            let path = env::var_os("GITHUB_OUTPUT").context("GITHUB_OUTPUT is not set")?;
            let mut output = fs::OpenOptions::new().append(true).open(path)?;
            for (key, file) in [
                ("artifact", name.as_str()),
                ("cdx", &cdx_name),
                ("spdx", &spdx_name),
                ("metadata", &metadata_name),
                ("checksums", &checksum_name),
            ] {
                let path = parent.join(file);
                let text = path
                    .to_str()
                    .context("UTF-8 report path")?
                    .replace('\\', "/");
                ensure!(!text.contains(['\n', '\r']), "unsafe workflow output");
                writeln!(output, "{key}={text}")?;
            }
        }
        println!(
            "Release SBOMs verified: {name} ({} native library files)",
            evidence.native_libraries.len()
        );
        Ok(())
    })();
    if let Some(mount) = mount {
        let detached = crate::package::detach_dmg(&mount);
        if result.is_ok() {
            detached?;
        } else if let Err(error) = detached {
            eprintln!("DMG cleanup also failed: {error:#}");
        }
    }
    result
}

fn verify_one(root: &Path, directory: &Path, evidence: &Evidence) -> Result<()> {
    ensure!(
        evidence.schema_version == 1,
        "unknown release SBOM metadata version"
    );
    let (revision, _) = revision(root)?;
    ensure!(
        evidence.source_revision == revision
            && evidence.version == crate::release_tooling::version::read(root)?,
        "SBOM source identity differs from checkout"
    );
    ensure!(
        evidence.input_sha256 == input_hashes(root)?,
        "SBOM dependency/SDK inputs have changed"
    );
    ensure!(
        evidence
            .artifact
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)),
        "unsafe artifact path"
    );
    ensure!(
        evidence.artifact_sha256 == crate::hash::file(&directory.join(&evidence.artifact))?,
        "SBOM archive hash mismatch"
    );
    let mut expected = String::new();
    for suffix in ["cdx.json", "spdx.json", "metadata.json"] {
        let name = format!("{}.sbom.{suffix}", evidence.artifact);
        let hash = crate::hash::file(&directory.join(&name))?;
        if suffix != "metadata.json" {
            ensure!(
                evidence.report_sha256.get(&name) == Some(&hash),
                "SBOM report hash mismatch"
            );
        }
        expected.push_str(&format!("{hash}  {name}\n"));
    }
    ensure!(
        fs::read_to_string(directory.join(format!("{}.sbom.SHA256SUMS.txt", evidence.artifact)))?
            == expected,
        "SBOM checksum manifest mismatch"
    );
    let cdx = json_file(&directory.join(format!("{}.sbom.cdx.json", evidence.artifact)))?;
    documents::validate(&cdx)?;
    documents::verify_native(&cdx, &evidence.native_libraries)?;
    documents::auditable_root(&cdx, &evidence.version)?;
    ensure!(
        cdx["metadata"]["component"]["version"] == evidence.version,
        "SBOM product version mismatch"
    );
    for (name, value) in [
        ("limo:source:revision", &evidence.source_revision),
        ("limo:build:target", &evidence.target),
    ] {
        ensure!(
            cdx["metadata"]["properties"]
                .as_array()
                .is_some_and(|properties| properties
                    .iter()
                    .any(|property| property["name"] == name && property["value"] == *value)),
            "SBOM source/target metadata mismatch"
        );
    }
    ensure!(
        cdx["metadata"]["component"]["hashes"][0]["content"] == evidence.artifact_sha256,
        "SBOM subject differs from archive"
    );
    let spdx = json_file(&directory.join(format!("{}.sbom.spdx.json", evidence.artifact)))?;
    documents::verify_spdx(
        &spdx,
        &evidence.native_libraries,
        &evidence.artifact,
        &evidence.artifact_sha256,
    )?;
    documents::verify_rust_graph(&cdx, &spdx)?;
    ensure!(
        !evidence.native_libraries.is_empty(),
        "native inventory is empty"
    );
    Ok(())
}

fn verify_directory(
    root: &Path,
    directory: &Path,
    strict: bool,
    sets: &mut Vec<(String, String)>,
    archives: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            verify_directory(root, &entry.path(), strict, sets, archives)?;
        } else if entry
            .file_name()
            .to_string_lossy()
            .ends_with(".sbom.metadata.json")
        {
            let evidence: Evidence = serde_json::from_slice(&fs::read(entry.path())?)?;
            verify_one(root, directory, &evidence)?;
            if strict {
                ensure!(
                    !evidence.source_modified,
                    "release SBOM was generated from modified source"
                );
                ensure!(
                    evidence.syft_version == SYFT_VERSION
                        && evidence.cargo_auditable_version == AUDITABLE_VERSION,
                    "release SBOM used unexpected tools"
                );
                for suffix in ["sbom.sigstore.json", "provenance.sigstore.json"] {
                    ensure!(
                        directory
                            .join(format!("{}.{suffix}", evidence.artifact))
                            .is_file(),
                        "release attestation bundle missing"
                    );
                }
            }
            sets.push((evidence.artifact, evidence.target));
        } else if [".zip", ".deb", ".AppImage", ".dmg"]
            .iter()
            .any(|suffix| entry.file_name().to_string_lossy().ends_with(suffix))
        {
            archives.push(
                entry
                    .file_name()
                    .to_str()
                    .context("UTF-8 archive name")?
                    .into(),
            );
        }
    }
    Ok(())
}

fn source(root: &Path, options: Options) -> Result<()> {
    ensure!(
        options.artifact.is_none()
            && options.sdk.is_none()
            && options.target.is_none()
            && !options.github_output
            && !options.require_release_evidence,
        "source only accepts --directory"
    );
    let directory = options
        .directory
        .unwrap_or_else(|| root.join("target/sbom"));
    fs::create_dir_all(&directory)?;
    let temporary = tempfile::tempdir()?;
    let payload = temporary.path().join("source");
    fs::create_dir(&payload)?;
    for file in LOCKFILES {
        let destination = payload.join(file);
        fs::create_dir_all(destination.parent().unwrap())?;
        fs::copy(root.join(file), destination)?;
    }
    let version = crate::release_tooling::version::read(root)?;
    let mut cdx = scan(
        &payload,
        temporary.path(),
        "rust-cargo-lock-cataloger",
        "Limo CAD source inventory",
        &version,
    )?;
    let raw = json_file(&temporary.path().join("scan.syft.json"))?;
    for file in LOCKFILES {
        ensure!(
            raw["artifacts"]
                .as_array()
                .context("source inventory")?
                .iter()
                .any(
                    |artifact| artifact["locations"]
                        .as_array()
                        .is_some_and(|locations| locations.iter().any(|location| location["path"]
                            .as_str()
                            .is_some_and(|path| path.trim_start_matches('/') == *file)))
                ),
            "missing source lockfile {file}"
        );
    }
    cdx["metadata"]["lifecycles"] = json!([{"phase": "pre-build"}]);
    cdx["metadata"]["properties"] = json!([{"name":"limo:source:revision","value":revision(root)?.0}, {"name":"limo:sbom:scope","value":"Source lockfiles; includes build and optional targets. Native release libraries are in the per-archive SBOMs."}]);
    cdx["compositions"] = json!([{"aggregate":"incomplete"}]);
    let cdx_path = directory.join("Limo-CAD-source.sbom.cdx.json");
    write_json(&cdx_path, &cdx)?;
    convert(&cdx_path, &directory.join("Limo-CAD-source.sbom.spdx.json"))?;
    println!("Source inventories: {}", directory.display());
    Ok(())
}

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let command = args.next().context("use sbom source, release or verify")?;
    let options = Options::parse(args)?;
    let root = crate::release_tooling::root();
    match command.as_str() {
        "source" => source(root, options),
        "release" => release(root, options),
        "verify" => {
            ensure!(
                options.artifact.is_none()
                    && options.sdk.is_none()
                    && options.target.is_none()
                    && !options.github_output,
                "verify only accepts --directory and --require-release-evidence"
            );
            let directory = options.directory.context("verify requires --directory")?;
            let mut sets = Vec::new();
            let mut archives = Vec::new();
            verify_directory(
                root,
                &directory,
                options.require_release_evidence,
                &mut sets,
                &mut archives,
            )?;
            ensure!(!sets.is_empty(), "no release SBOMs found");
            let mut names = sets
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            names.sort();
            archives.sort();
            ensure!(
                names == archives && names.windows(2).all(|names| names[0] != names[1]),
                "every archive needs exactly one matching SBOM set"
            );
            if options.require_release_evidence {
                let expected = [
                    (".zip", "x86_64-pc-windows-msvc"),
                    (".zip", "aarch64-pc-windows-msvc"),
                    (".deb", "x86_64-unknown-linux-gnu"),
                    (".AppImage", "x86_64-unknown-linux-gnu"),
                    (".dmg", "aarch64-apple-darwin"),
                ];
                ensure!(
                    sets.len() == expected.len()
                        && expected.iter().all(|(suffix, target)| sets
                            .iter()
                            .filter(|(name, actual)| name.ends_with(suffix) && actual == target)
                            .count()
                            == 1),
                    "release requires all five platform SBOMs"
                );
            }
            println!("Verified {} release SBOM sets", sets.len());
            Ok(())
        }
        _ => bail!("use sbom source, release or verify"),
    }
}
