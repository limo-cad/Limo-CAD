//! Reproducible OCCT 7.9 SDK build through a portable Rust entry point.
use crate::build_tools::run as run_command;
use anyhow::{bail, ensure, Context, Result};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
    time::Duration,
};
#[cfg(any(windows, test))]
#[path = "occt_runtime.rs"]
mod runtime;

const VERSION: &str = "7_9_3";
const SHA256: &str = "5ecf094ec6b12d5413dfb851d8c3590c354058aee556e32e408bdfbf8c357d57";
const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
type SourceOverride<'a> = (&'a str, &'a str, &'a [u8]);
const SOURCE_OVERRIDES: &[SourceOverride<'_>] = &[
    (
        "src/math/math_DoubleTab.cxx",
        "bc046ea106812a0f2c37d47a5f54cd58099837dcb6a68c1c86749db4077d5b43",
        include_bytes!("../../native/occt-overlay/opencascade/math_DoubleTab.cxx"),
    ),
    (
        "src/math/math_DoubleTab.lxx",
        "2cf1b4d1c9f855e04b00edaae56d5fb700769db50a3d7000341fd3c269116b54",
        include_bytes!("../../native/occt-overlay/opencascade/math_DoubleTab.lxx"),
    ),
];

fn override_digest() -> Result<String> {
    let mut inputs = Vec::new();
    for (path, original, replacement) in SOURCE_OVERRIDES {
        writeln!(
            inputs,
            "{path}:{original}:{}",
            crate::hash::reader(*replacement)?
        )?;
    }
    crate::hash::reader(inputs.as_slice())
}
const SETTINGS: &[&str] = &[
    "CMAKE_BUILD_TYPE=Release",
    "BUILD_LIBRARY_TYPE=Shared",
    "BUILD_MODULE_FoundationClasses=ON",
    "BUILD_MODULE_ModelingData=ON",
    "BUILD_MODULE_ModelingAlgorithms=ON",
    "BUILD_MODULE_Visualization=ON",
    "BUILD_MODULE_ApplicationFramework=ON",
    "BUILD_MODULE_DataExchange=ON",
    "BUILD_MODULE_DETools=OFF",
    "BUILD_MODULE_Draw=OFF",
    "BUILD_DOC_Overview=OFF",
    "USE_TCL=OFF",
    "USE_TK=OFF",
    "USE_OPENGL=OFF",
    "USE_GLES2=OFF",
    "USE_FREETYPE=ON",
    "USE_FREEIMAGE=OFF",
    "USE_RAPIDJSON=OFF",
    "USE_DRACO=OFF",
    "USE_TBB=OFF",
    "USE_VTK=OFF",
];

struct Options {
    prefix: PathBuf,
    jobs: usize,
    dry_run: bool,
    cache: PathBuf,
    sccache: bool,
    github_key: bool,
    repair_runtime: bool,
    freetype: Vec<String>,
}
impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut prefix = None;
        let mut jobs = None;
        let mut dry_run = false;
        let mut cache = None;
        let mut sccache = false;
        let mut github_key = false;
        let mut repair_runtime = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--prefix" => {
                    ensure!(prefix.is_none(), "duplicate --prefix");
                    prefix = Some(PathBuf::from(args.next().context("missing --prefix path")?));
                }
                "--jobs" => {
                    ensure!(jobs.is_none(), "duplicate --jobs");
                    jobs = Some(args.next().context("missing --jobs")?.parse::<usize>()?);
                }
                "--dry-run" => dry_run = true,
                "--cache-dir" => {
                    ensure!(cache.is_none(), "duplicate --cache-dir");
                    cache = Some(PathBuf::from(args.next().context("missing --cache-dir")?));
                }
                "--sccache" => sccache = true,
                "--github-key" => github_key = true,
                "--repair-runtime" => repair_runtime = true,
                _ => bail!("use build-occt --prefix PATH [--jobs N] [--cache-dir PATH] [--sccache] [--dry-run] [--repair-runtime]"),
            }
        }
        let prefix = prefix.context("missing --prefix PATH")?;
        ensure!(!prefix.as_os_str().is_empty(), "empty install prefix");
        let prefix = if prefix.is_absolute() {
            prefix
        } else {
            env::current_dir()?.join(prefix)
        };
        let jobs = match jobs {
            Some(jobs) => jobs,
            None => env::var("CMAKE_BUILD_PARALLEL_LEVEL")
                .ok()
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<usize>())
                .transpose()?
                .unwrap_or(std::thread::available_parallelism().map_or(1, usize::from)),
        };
        ensure!(jobs > 0, "--jobs must be greater than zero");
        let cache = cache
            .or_else(|| env::var_os("LIMO_CAD_BUILD_CACHE").map(PathBuf::from))
            .unwrap_or_else(|| crate::build_tools::root().join("target/limo-cad-build-cache"));
        ensure!(!cache.as_os_str().is_empty(), "empty cache directory");
        let cache = if cache.is_absolute() {
            cache
        } else {
            env::current_dir()?.join(cache)
        };
        Ok(Self {
            prefix,
            jobs,
            dry_run,
            cache,
            sccache,
            github_key,
            repair_runtime,
            freetype: Vec::new(),
        })
    }
}

fn configure(options: &Options, source: &Path, build: &Path) -> Command {
    let mut command = Command::new("cmake");
    command
        .arg("-S")
        .arg(source)
        .arg("-B")
        .arg(build)
        .args(["-G", "Ninja"])
        .arg(format!("-DINSTALL_DIR={}", options.prefix.display()))
        .arg("-DINSTALL_DIR_LAYOUT=Unix");
    for setting in SETTINGS {
        command.arg(format!("-D{setting}"));
    }
    if options.sccache {
        command.args([
            "-DCMAKE_C_COMPILER_LAUNCHER=sccache",
            "-DCMAKE_CXX_COMPILER_LAUNCHER=sccache",
        ]);
    }
    command.args(&options.freetype);
    command
}
pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut options = Options::parse(args)?;
    ensure!(
        !options.repair_runtime || cfg!(windows),
        "--repair-runtime is supported on Windows only"
    );
    let overrides = override_digest()?;
    let url =
        format!("https://github.com/Open-Cascade-SAS/OCCT/archive/refs/tags/V{VERSION}.tar.gz");
    if options.dry_run {
        println!(
            "OCCT {} source {url}\nSHA256 {SHA256}\nChecked storage overrides {overrides}\n{:?}\nJobs: {}\nCache: {}",
            VERSION.replace('_', "."),
            configure(
                &options,
                std::path::Path::new("SOURCE"),
                std::path::Path::new("BUILD")
            ),
            options.jobs,
            options.cache.display()
        );
        return Ok(());
    }
    fs::create_dir_all(&options.cache)?;
    if options.sccache {
        crate::build_tools::require_tool("sccache")?;
    }
    let compiler = crate::occt_cache::compiler_identity(&options.cache)?;
    options.freetype = crate::occt_cache::freetype_arguments(&compiler)?;
    let recipe = configure(
        &options,
        std::path::Path::new("SOURCE"),
        std::path::Path::new("BUILD"),
    )
    .get_args()
    .map(|arg| arg.to_string_lossy().into_owned())
    .collect::<Vec<_>>()
    .join("\n");
    let key = crate::occt_cache::key(&format!("{SHA256}:{overrides}"), &compiler, &recipe)?;
    if options.github_key {
        let mut output = fs::OpenOptions::new()
            .append(true)
            .open(env::var_os("GITHUB_OUTPUT").context("--github-key requires GITHUB_OUTPUT")?)?;
        writeln!(output, "sdk_key={key}")?;
        return Ok(());
    }
    let source_cache = options.cache.join("sources").join(SHA256);
    let source_lock = crate::occt_cache::lock(&source_cache)?;
    let archive = source_cache.join("occt.tar.gz");
    if !archive.exists() {
        download(&url, &archive)?;
    }
    ensure!(
        crate::hash::file(&archive)? == SHA256,
        "cached OCCT archive checksum differs; refusing reuse"
    );
    let source_variant = source_cache.join(&overrides);
    fs::create_dir_all(&source_variant)?;
    let source = source_variant.join(format!("OCCT-{VERSION}"));
    if !source.exists() {
        let staging = tempfile::tempdir_in(&source_cache)?;
        tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&archive)?))
            .unpack(staging.path())?;
        let extracted = staging.path().join(format!("OCCT-{VERSION}"));
        ensure!(
            extracted.join("CMakeLists.txt").is_file(),
            "missing OCCT source tree"
        );
        verify_source(&archive, &extracted, SHA256)?;
        apply_overrides(&extracted, SOURCE_OVERRIDES)?;
        fs::rename(extracted, &source)?;
    }
    let source_inventory =
        verify_source_with_overrides(&archive, &source, SHA256, SOURCE_OVERRIDES)?;
    drop(source_lock);
    let work = options.cache.join("builds").join(&key);
    let build = work.join("build");
    #[cfg(windows)]
    verify_windows_compiler_paths(None, Some(&build), &source_inventory)?;
    #[cfg(not(windows))]
    drop(source_inventory);
    let _build_lock = crate::occt_cache::lock(&work)?;
    let _prefix_lock = crate::occt_cache::prepare_locked(&options.prefix, &key)?;
    if crate::occt_cache::complete(&options.prefix, &key)? {
        #[cfg(windows)]
        let repaired =
            runtime::verify_or_repair(&options.prefix, &key, &compiler, options.repair_runtime)?;
        crate::occt_storage::verify(&options.prefix)?;
        #[cfg(windows)]
        if repaired {
            crate::occt_cache::publish(&options.prefix, &key)?;
        }
        println!(
            "Verified installed OCCT SDK cache hit: {}",
            options.prefix.display()
        );
        return Ok(());
    }
    // Validate and capture the exact dependency inputs before an expensive
    // native build; assembly rechecks this snapshot before/after adding files.
    #[cfg(windows)]
    let runtime_plan = runtime::Plan::from_compiler_identity(&compiler)?;
    run_command(&mut configure(&options, &source, &build))?;
    run_command(
        Command::new("cmake")
            .arg("--build")
            .arg(&build)
            .arg("--parallel")
            .arg(options.jobs.to_string()),
    )?;
    run_command(Command::new("cmake").arg("--install").arg(&build))?;
    #[cfg(windows)]
    runtime_plan.apply(&options.prefix, &key)?;
    let doc = options.prefix.join("share/doc/opencascade");
    fs::create_dir_all(&doc)?;
    let mut copyright = fs::File::create(doc.join("copyright"))?;
    writeln!(copyright,"Open CASCADE Technology {}\nhttps://github.com/Open-Cascade-SAS/OCCT\nCopyright (c) Open CASCADE SAS\n\nOCCT is distributed under the GNU Lesser General Public License version 2.1\nwith the following additional exception.\n",VERSION.replace('_',"."))?;
    std::io::copy(
        &mut fs::File::open(source.join("OCCT_LGPL_EXCEPTION.txt"))?,
        &mut copyright,
    )?;
    fs::copy(source.join("LICENSE_LGPL_21.txt"), doc.join("LGPL-2.1.txt"))?;
    for name in ["LICENSE_LGPL_21.txt", "OCCT_LGPL_EXCEPTION.txt"] {
        fs::copy(source.join(name), doc.join(name))?;
    }
    crate::build_tools::sdk::resolve(
        std::slice::from_ref(&options.prefix),
        env::consts::OS,
        env::consts::ARCH,
        None,
    )
    .map_err(anyhow::Error::msg)
    .context("validate the installed OCCT SDK before publishing its receipt")?;
    verify_source_with_overrides(&archive, &source, SHA256, SOURCE_OVERRIDES)?;
    crate::occt_storage::verify(&options.prefix)?;
    crate::occt_cache::publish(&options.prefix, &key)?;
    println!(
        "Installed OCCT {} into {}\nBuild cache: {}",
        VERSION.replace('_', "."),
        options.prefix.display(),
        work.display()
    );
    if options.sccache {
        crate::build_tools::run(Command::new("sccache").arg("--show-stats"))?;
    }
    Ok(())
}

fn source_inventory(archive: &Path) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(archive)?));
    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.header().entry_type().is_pax_global_extensions() {
            continue;
        }
        let path = entry.path()?.into_owned();
        ensure!(
            path.components()
                .all(|part| matches!(part, Component::Normal(_))),
            "invalid OCCT archive path"
        );
        let relative = path.strip_prefix(format!("OCCT-{VERSION}"))?;
        let name = relative
            .to_str()
            .context("non-UTF8 OCCT archive path")?
            .replace('\\', "/");
        let kind = entry.header().entry_type();
        if name.is_empty() {
            ensure!(kind.is_dir(), "invalid OCCT archive root");
            continue;
        }
        for parent in relative
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            let parent = parent
                .to_str()
                .context("non-UTF8 source parent")?
                .replace('\\', "/");
            ensure!(
                files.get(&parent).is_none_or(|value| value == "directory"),
                "OCCT archive traverses a non-directory"
            );
            files.insert(parent, "directory".into());
        }
        let fingerprint = if kind.is_file() {
            format!("file:{}", crate::hash::reader(&mut entry)?)
        } else if kind.is_dir() {
            "directory".into()
        } else if kind.is_symlink() {
            let target = entry
                .link_name()?
                .context("missing source symlink target")?;
            confined_source_link(relative, &target)?;
            format!(
                "symlink:{}",
                target
                    .to_str()
                    .context("non-UTF8 source symlink")?
                    .replace('\\', "/")
            )
        } else {
            bail!("unsupported OCCT archive entry: {name}");
        };
        ensure!(
            files
                .get(&name)
                .is_none_or(|old| old == "directory" && fingerprint == "directory"),
            "duplicate OCCT archive entry: {name}"
        );
        files.insert(name, fingerprint);
    }
    ensure!(
        files
            .get("CMakeLists.txt")
            .is_some_and(|value| value.starts_with("file:")),
        "missing OCCT source tree"
    );
    Ok(files)
}

fn confined_source_link(relative: &Path, target: &Path) -> Result<()> {
    let mut depth = relative
        .parent()
        .map_or(0, |parent| parent.components().count());
    for part in target.components() {
        match part {
            Component::Normal(_) => depth += 1,
            Component::CurDir => (),
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => bail!("source symlink escapes its tree"),
        }
    }
    Ok(())
}

fn source_files(root: &Path, directory: &Path, files: &mut BTreeMap<String, String>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root)?;
        let name = relative
            .to_str()
            .context("non-UTF8 source path")?
            .replace('\\', "/");
        let kind = entry.file_type()?;
        let fingerprint = if kind.is_symlink() {
            let target = fs::read_link(&path)?;
            confined_source_link(relative, &target)?;
            ensure!(
                fs::canonicalize(&path)?.starts_with(root),
                "source symlink escapes its tree"
            );
            format!(
                "symlink:{}",
                target
                    .to_str()
                    .context("non-UTF8 source symlink")?
                    .replace('\\', "/")
            )
        } else if kind.is_dir() {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                ensure!(
                    entry.metadata()?.file_attributes() & 0x400 == 0,
                    "source junctions are not supported"
                );
            }
            source_files(root, &path, files)?;
            "directory".into()
        } else {
            ensure!(kind.is_file(), "source contains a non-regular file");
            format!("file:{}", crate::hash::file(&path)?)
        };
        files.insert(name, fingerprint);
    }
    Ok(())
}

fn verify_source(archive: &Path, source: &Path, digest: &str) -> Result<()> {
    verify_source_with_overrides(archive, source, digest, &[]).map(|_| ())
}

// The supported Windows compiler still rejects these native paths at MAX_PATH,
// regardless of Rust's ability to inspect an extended-length source tree. This
// guards real source/header paths and generated forwarding headers only; it
// does not certify every future object/PDB path or third-party dependency path.
#[cfg(any(windows, test))]
fn require_windows_compiler_path(path: &Path) -> Result<()> {
    #[cfg(windows)]
    let units = {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().count()
    };
    #[cfg(not(windows))]
    let units = path
        .to_str()
        .context("non-Unicode compiler path")?
        .encode_utf16()
        .count();
    ensure!(
        units < 260,
        "OCCT Windows compiler path has {units} UTF-16 units (must be below 260): {}. Use a short absolute --cache-dir, such as D:/lc; existing SDK/cache ownership is preserved",
        path.display()
    );
    Ok(())
}

#[cfg(any(windows, test))]
fn verify_windows_compiler_paths(
    source: Option<&Path>,
    build: Option<&Path>,
    inventory: &BTreeMap<String, String>,
) -> Result<()> {
    let source = source.map(std::path::absolute).transpose()?;
    let build = build.map(std::path::absolute).transpose()?;
    for (relative, fingerprint) in inventory {
        if !relative.starts_with("src/") || !fingerprint.starts_with("file:") {
            continue;
        }
        let relative = Path::new(relative);
        let extension = relative.extension().and_then(|value| value.to_str());
        // OCCT's COLLECT_AND_INSTALL_OCCT_HEADER_FILES uses this extension set
        // and places each selected header basename in build/inc.
        let forwarding_header = matches!(
            extension,
            Some("h" | "g" | "p" | "hpp" | "hxx" | "lxx" | "gxx" | "pxx")
        );
        if !forwarding_header && !matches!(extension, Some("c" | "cpp" | "cxx" | "ixx")) {
            continue;
        }
        if let Some(source) = &source {
            require_windows_compiler_path(&source.join(relative))?;
        }
        if let Some(build) = build.as_ref().filter(|_| forwarding_header) {
            require_windows_compiler_path(&build.join("inc").join(relative.file_name().unwrap()))?;
        }
    }
    Ok(())
}

fn apply_overrides(source: &Path, overrides: &[SourceOverride<'_>]) -> Result<()> {
    for (relative, original, replacement) in overrides {
        let path = source.join(relative);
        ensure!(
            crate::hash::file(&path)? == *original,
            "OCCT override input differs from the pinned source: {relative}"
        );
        fs::write(path, replacement)?;
    }
    Ok(())
}

fn verify_source_with_overrides(
    archive: &Path,
    source: &Path,
    digest: &str,
    overrides: &[SourceOverride<'_>],
) -> Result<BTreeMap<String, String>> {
    ensure!(
        crate::hash::file(archive)? == digest,
        "cached OCCT archive checksum differs; refusing reuse"
    );
    ensure!(
        fs::symlink_metadata(source)?.is_dir(),
        "cached OCCT source must be a directory"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            fs::symlink_metadata(source)?.file_attributes() & 0x400 == 0,
            "cached OCCT source must not be a junction"
        );
    }
    let mut expected = source_inventory(archive)?;
    #[cfg(windows)]
    verify_windows_compiler_paths(Some(source), None, &expected)?;
    for (relative, original, replacement) in overrides {
        ensure!(
            expected.get(*relative) == Some(&format!("file:{original}")),
            "OCCT override is not based on the pinned archive: {relative}"
        );
        expected.insert(
            (*relative).into(),
            format!("file:{}", crate::hash::reader(*replacement)?),
        );
    }
    let root = fs::canonicalize(source)?;
    let mut actual = BTreeMap::new();
    source_files(&root, &root, &mut actual)?;
    ensure!(
        actual == expected,
        "cached OCCT sources differ from the verified archive; select a fresh cache directory"
    );
    Ok(expected)
}

fn download(url: &str, archive: &std::path::Path) -> Result<()> {
    let mut temporary =
        tempfile::NamedTempFile::new_in(archive.parent().context("source cache parent")?)?;
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(120)))
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(url)
        .header("User-Agent", "Limo-CAD-OCCT-SDK")
        .call()?;
    let bytes = std::io::copy(
        &mut response.body_mut().as_reader().take(MAX_SOURCE_BYTES + 1),
        temporary.as_file_mut(),
    )?;
    ensure!(
        bytes <= MAX_SOURCE_BYTES,
        "OCCT source archive exceeds the 256 MB limit"
    );
    ensure!(
        crate::hash::file(temporary.path())? == SHA256,
        "OCCT source checksum differs; refusing extraction"
    );
    temporary.as_file_mut().sync_all()?;
    temporary.persist_noclobber(archive)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_repair_flag_keeps_the_original_occt_configure_recipe() {
        let base = Options::parse(
            ["--prefix", "same-owned-sdk"]
                .map(str::to_owned)
                .into_iter(),
        )
        .unwrap();
        let repair = Options::parse(
            ["--prefix", "same-owned-sdk", "--repair-runtime"]
                .map(str::to_owned)
                .into_iter(),
        )
        .unwrap();
        assert!(repair.repair_runtime);
        assert_eq!(
            configure(&base, Path::new("SOURCE"), Path::new("BUILD"))
                .get_args()
                .collect::<Vec<_>>(),
            configure(&repair, Path::new("SOURCE"), Path::new("BUILD"))
                .get_args()
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn windows_compiler_paths_count_utf16_and_keep_the_259_unit_boundary() {
        assert!(require_windows_compiler_path(Path::new(&"a".repeat(259))).is_ok());
        let error = require_windows_compiler_path(Path::new(&"a".repeat(260)))
            .unwrap_err()
            .to_string();
        assert!(error.contains("260 UTF-16 units"));
        assert!(error.contains("short absolute --cache-dir"));
        assert!(
            require_windows_compiler_path(Path::new(&format!("{}\u{1f600}", "a".repeat(257))))
                .is_ok()
        );
        assert!(
            require_windows_compiler_path(Path::new(&format!("{}\u{1f600}", "a".repeat(258))))
                .is_err()
        );
    }

    #[test]
    fn verified_source_inventory_guards_native_and_forwarding_header_paths() {
        let (temporary, archive, source, digest) = source_fixture(false);
        let inventory = verify_source_with_overrides(&archive, &source, &digest, &[]).unwrap();
        verify_windows_compiler_paths(
            Some(&source),
            Some(&temporary.path().join("short-build")),
            &inventory,
        )
        .unwrap();
        // Resolve the same relative path spelling that Options accepts, without
        // using process-wide cwd changes or invoking CMake/the compiler.
        let long = Path::new(&"a".repeat(180)).join("b".repeat(80));
        let source_error = verify_windows_compiler_paths(Some(&long), None, &inventory)
            .unwrap_err()
            .to_string();
        assert!(source_error.contains("src"));
        assert!(source_error.contains("example.cxx"));
        let header_error = verify_windows_compiler_paths(None, Some(&long), &inventory)
            .unwrap_err()
            .to_string();
        assert!(header_error.contains("inc"));
        assert!(header_error.contains("example.hxx"));
        #[cfg(windows)]
        {
            let long_parent = temporary.path().join(&long);
            fs::create_dir_all(&long_parent).unwrap();
            let long_source = long_parent.join(format!("OCCT-{VERSION}"));
            fs::rename(&source, &long_source).unwrap();
            let error = verify_source(&archive, &long_source, &digest)
                .unwrap_err()
                .to_string();
            assert!(error.contains("OCCT Windows compiler path"));
            assert!(error.contains("short absolute --cache-dir"));
        }
    }
    #[test]
    fn checked_overrides_are_verified_against_both_pristine_and_resulting_sources() {
        let (_temporary, archive, source, digest) = source_fixture(false);
        let original = crate::hash::file(&source.join("src/example.cxx")).unwrap();
        let overrides = [(
            "src/example.cxx",
            original.as_str(),
            b"checked replacement".as_slice(),
        )];
        verify_source(&archive, &source, &digest).unwrap();
        assert!(verify_source_with_overrides(&archive, &source, &digest, &overrides).is_err());
        apply_overrides(&source, &overrides).unwrap();
        verify_source_with_overrides(&archive, &source, &digest, &overrides).unwrap();
        assert!(verify_source(&archive, &source, &digest).is_err());
        fs::write(source.join("src/example.cxx"), "unapproved patch").unwrap();
        assert!(verify_source_with_overrides(&archive, &source, &digest, &overrides).is_err());
        assert!(apply_overrides(&source, &overrides).is_err());
    }

    fn source_fixture(with_link: bool) -> (tempfile::TempDir, PathBuf, PathBuf, String) {
        let temporary = tempfile::tempdir().unwrap();
        let archive = temporary.path().join("occt.tar.gz");
        let encoder = flate2::write::GzEncoder::new(
            fs::File::create(&archive).unwrap(),
            flate2::Compression::default(),
        );
        let mut builder = tar::Builder::new(encoder);
        let mut pax = tar::Header::new_ustar();
        pax.set_entry_type(tar::EntryType::new(b'g'));
        pax.set_size(16);
        pax.set_cksum();
        builder
            .append_data(&mut pax, "pax_global_header", &b"16 comment=test\n"[..])
            .unwrap();
        for (name, content) in [
            ("CMakeLists.txt", "project(OCCT)"),
            ("src/example.cxx", "pinned source"),
            ("src/example.hxx", "pinned header"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(content.len() as u64);
            header.set_cksum();
            builder
                .append_data(
                    &mut header,
                    format!("OCCT-{VERSION}/{name}"),
                    content.as_bytes(),
                )
                .unwrap();
        }
        if with_link {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_link_name("example.cxx").unwrap();
            header.set_cksum();
            builder
                .append_data(
                    &mut header,
                    format!("OCCT-{VERSION}/src/link.cxx"),
                    std::io::empty(),
                )
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
        let source = temporary.path().join(format!("OCCT-{VERSION}"));
        tar::Archive::new(flate2::read::GzDecoder::new(
            fs::File::open(&archive).unwrap(),
        ))
        .unpack(temporary.path())
        .unwrap();
        let digest = crate::hash::file(&archive).unwrap();
        (temporary, archive, source, digest)
    }
    #[test]
    fn changed_missing_or_extra_source_files_are_rejected_against_archive() {
        let (_temporary, archive, source, digest) = source_fixture(false);
        verify_source(&archive, &source, &digest).unwrap();
        fs::write(source.join("src/example.cxx"), "modified source").unwrap();
        assert!(verify_source(&archive, &source, &digest).is_err());
        fs::write(source.join("src/example.cxx"), "pinned source").unwrap();
        fs::write(source.join("extra.cxx"), "untracked source").unwrap();
        assert!(verify_source(&archive, &source, &digest).is_err());
        fs::remove_file(source.join("extra.cxx")).unwrap();
        fs::remove_file(source.join("src/example.cxx")).unwrap();
        assert!(verify_source(&archive, &source, &digest).is_err());
        assert!(verify_source(&archive, &source, "wrong digest").is_err());
    }
    #[test]
    fn source_links_must_remain_inside_the_source_tree() {
        assert!(
            confined_source_link(Path::new("src/link"), Path::new("../CMakeLists.txt")).is_ok()
        );
        assert!(confined_source_link(Path::new("src/link"), Path::new("../../outside")).is_err());
        assert!(confined_source_link(Path::new("link"), Path::new("/outside")).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn source_link_targets_are_checked_against_the_verified_archive() {
        use std::os::unix::fs::symlink;
        let (temporary, archive, source, digest) = source_fixture(true);
        verify_source(&archive, &source, &digest).unwrap();
        fs::remove_file(source.join("src/link.cxx")).unwrap();
        fs::write(temporary.path().join("outside.cxx"), "pinned source").unwrap();
        symlink(
            temporary.path().join("outside.cxx"),
            source.join("src/link.cxx"),
        )
        .unwrap();
        assert!(verify_source(&archive, &source, &digest).is_err());
    }
    #[test]
    fn sdk_recipe_keeps_abi_modules_and_literal_paths_without_a_shell() {
        let options = Options::parse(
            ["--prefix", "SDK path with spaces", "--jobs", "2"]
                .map(str::to_owned)
                .into_iter(),
        )
        .unwrap();
        let command = configure(
            &options,
            std::path::Path::new("source path"),
            std::path::Path::new("build path"),
        );
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"source path".into()) && args.contains(&"build path".into()));
        assert!(args.contains(&format!("-DINSTALL_DIR={}", options.prefix.display())));
        for module in ["DataExchange", "Visualization", "ApplicationFramework"] {
            assert!(args.contains(&format!("-DBUILD_MODULE_{module}=ON")));
        }
        for module in ["Draw", "DETools"] {
            assert!(args.contains(&format!("-DBUILD_MODULE_{module}=OFF")));
        }
        assert_eq!(options.jobs, 2);
        assert!(Options::parse(
            ["--prefix", "test", "--jobs", "0"]
                .map(str::to_owned)
                .into_iter()
        )
        .is_err());
        assert!(Options::parse(
            ["--prefix", "test", "--prefix", "another"]
                .map(str::to_owned)
                .into_iter()
        )
        .is_err());
    }
}
