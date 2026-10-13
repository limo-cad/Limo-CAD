use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Library {
    pub path: String,
    pub sha256: String,
    pub name: String,
    pub package_name: String,
    pub purl: Option<String>,
    pub kind: String,
    pub version: String,
    pub producer: String,
    pub license: String,
    pub identification: String,
    pub imports: Vec<String>,
    pub occt: bool,
}

pub(super) fn is_library(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".dll")
        || name.ends_with(".dylib")
        || name.ends_with(".so")
        || name.contains(".so.")
}

fn is_binary(path: &Path) -> Result<bool> {
    let mut magic = [0u8; 4];
    let count = fs::File::open(path)?.read(&mut magic)?;
    Ok(count >= 2
        && (&magic[..2] == b"MZ"
            || (count == 4
                && (magic == *b"\x7fELF"
                    || matches!(
                        magic,
                        [0xcf, 0xfa, 0xed, 0xfe]
                            | [0xce, 0xfa, 0xed, 0xfe]
                            | [0xfe, 0xed, 0xfa, 0xcf]
                            | [0xfe, 0xed, 0xfa, 0xce]
                            | [0xca, 0xfe, 0xba, 0xbe]
                    )))))
}

fn walk(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk(root, &entry.path(), files)?;
        } else if (kind.is_file() || kind.is_symlink())
            && (is_library(&entry.file_name().to_string_lossy())
                || kind.is_file() && is_binary(&entry.path())?)
        {
            let path = entry.path();
            ensure!(
                path.canonicalize()?.starts_with(root),
                "native library link leaves the package: {}",
                path.display()
            );
            ensure!(path.is_file(), "native library is not a file");
            files.push(path);
        }
    }
    Ok(())
}

fn imports(path: &Path, target: &str) -> Result<Vec<String>> {
    if target.contains("windows") {
        let image = crate::windows_runtime::inspect_imports(path)?;
        let machine = if target.starts_with("aarch64") {
            0xaa64
        } else {
            0x8664
        };
        ensure!(
            image.machine == machine,
            "native library has the wrong target architecture"
        );
        Ok(image.imports)
    } else if target.contains("apple") {
        let architecture = if target.starts_with("aarch64") {
            "arm64"
        } else {
            "x86_64"
        };
        ensure!(
            super::output(Command::new("lipo").arg("-archs").arg(path))?
                .split_whitespace()
                .any(|arch| arch == architecture),
            "native binary lacks the SBOM target architecture"
        );
        let own_id = super::output(Command::new("otool").arg("-D").arg(path))?
            .lines()
            .nth(1)
            .map(str::trim)
            .map(str::to_owned);
        Ok(super::output(Command::new("otool").arg("-L").arg(path))?
            .lines()
            .skip(1)
            .map(|line| {
                line.trim()
                    .split(" (compatibility version")
                    .next()
                    .unwrap()
                    .to_owned()
            })
            .filter(|import| own_id.as_ref() != Some(import))
            .collect())
    } else {
        ensure!(
            super::output(Command::new("readelf").arg("--file-header").arg(path))?
                .lines()
                .any(|line| line.contains("Machine:") && line.contains("X86-64")),
            "native binary is not Linux x86-64"
        );
        let text = super::output(
            Command::new("readelf")
                .args(["--dynamic", "--wide"])
                .arg(path),
        )?;
        Ok(text
            .lines()
            .filter(|line| line.contains("(NEEDED)"))
            .filter_map(|line| {
                line.split_once('[')
                    .and_then(|(_, rest)| rest.split_once(']'))
                    .map(|(name, _)| name.to_owned())
            })
            .collect())
    }
}

/// Loader edits and code signing preserve these native build identities.
/// Never attribute a final file to an installed package by filename alone.
pub(crate) fn binary_identity(path: &Path) -> Option<String> {
    if cfg!(target_os = "macos") {
        let text = super::output(Command::new("otool").arg("-l").arg(path)).ok()?;
        let ids = text
            .lines()
            .filter_map(|line| line.trim().strip_prefix("uuid "))
            .collect::<Vec<_>>();
        (!ids.is_empty()).then(|| ids.join(","))
    } else if cfg!(target_os = "linux") {
        let text = super::output(Command::new("readelf").arg("--notes").arg(path)).ok()?;
        text.lines()
            .find_map(|line| line.trim().strip_prefix("Build ID: ").map(str::to_owned))
    } else {
        crate::hash::file(path).ok()
    }
}

fn header(sdk: &Path) -> Result<PathBuf> {
    [
        sdk.join("include/opencascade"),
        sdk.join("include"),
        sdk.join("inc"),
    ]
    .into_iter()
    .find(|path| path.join("Standard_Version.hxx").is_file())
    .context("SDK version header is missing")
}

fn sdk_evidence(sdk: &Path) -> Result<Value> {
    let include = header(sdk)?;
    crate::occt_storage::verify_header(&include)?;
    let version = fs::read_to_string(include.join("Standard_Version.hxx"))?;
    ensure!(
        regex::Regex::new(r#"OCC_VERSION_COMPLETE\s+"7\.9\.3""#)?.is_match(&version),
        "SDK is not the expected OCCT 7.9.3"
    );
    let receipt_path = sdk.join(".limo-sdk-complete.json");
    if !receipt_path.is_file() {
        ensure!(
            sdk.join("share/opencascade/limo-occt-build.json").is_file(),
            "SDK lacks build evidence; rebuild the checked SDK or pinned vcpkg overlay"
        );
        return super::vcpkg::evidence(sdk, crate::occt_sdk::sbom_evidence("")?);
    }
    let receipt = super::json_file(&receipt_path)?;
    let key = receipt["key"].as_str().context("SDK completion key")?;
    ensure!(
        crate::occt_cache::complete(sdk, key)?,
        "SDK files do not match the completed receipt"
    );
    let path = sdk.join(".limo-sdk-sbom.json");
    let mut evidence = super::json_file(&path)
        .context("rebuild the SDK to capture its source/patch/dependency provenance")?;
    let expected = crate::occt_sdk::sbom_evidence("")?;
    ensure!(
        evidence["occt"] == expected["occt"],
        "SDK source or patch evidence differs from this checkout"
    );
    evidence["completionReceiptSha256"] = json!(crate::hash::file(&receipt_path)?);
    evidence["provenanceSha256"] = json!(crate::hash::file(&path)?);
    evidence["cacheKey"] = json!(key);
    Ok(evidence)
}

fn known_dependency(path: &Path, sdk: &Path, evidence: &Value) -> Option<Value> {
    let name = path.file_name()?.to_str()?;
    let lower = name.to_ascii_lowercase();
    let owner = if lower.contains("freetype") {
        "freetype"
    } else if lower.contains("fontconfig") {
        "fontconfig"
    } else {
        return None;
    };
    let package = evidence["dependencies"]
        .as_array()?
        .iter()
        .find(|package| package["name"] == owner && package["version"] != "unknown")
        .cloned()?;
    let identity = binary_identity(path)?;
    let verified = if cfg!(target_os = "windows") {
        let source = sdk.join("bin").join(name);
        source.is_file() && binary_identity(&source).as_deref() == Some(identity.as_str())
    } else {
        package["binaryIdentity"].as_str() == Some(identity.as_str())
    };
    verified.then_some(package)
}

fn vcpkg_package(sdk: &Path, path: &Path) -> Option<Value> {
    let name = path.file_name()?.to_str()?;
    let source = sdk.join("bin").join(name);
    if crate::hash::file(&source).ok()? != crate::hash::file(path).ok()? {
        return None;
    }
    let port = crate::windows_runtime::dependency_port(name)?;
    let spdx = super::json_file(&sdk.join("share").join(port).join("vcpkg.spdx.json")).ok()?;
    let package = spdx["packages"].as_array()?.iter().find(|package| {
        package["name"]
            .as_str()
            .is_some_and(|name| name == port || name.ends_with(&format!("/{port}")))
    })?;
    let recorded_version = package["versionInfo"].as_str().unwrap_or("unknown");
    let license = ["licenseDeclared", "licenseConcluded"]
        .iter()
        .find_map(|key| {
            package[*key]
                .as_str()
                .filter(|value| *value != "NOASSERTION" && *value != "NONE")
        })
        .unwrap_or("NOASSERTION");
    let purl = package["externalRefs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|reference| reference["referenceType"] == "purl")
        .and_then(|reference| reference["referenceLocator"].as_str());
    Some(
        json!({"name":port, "version":recorded_version.split('#').next().unwrap_or("unknown"),"purl":purl,
        "producer":package["supplier"].as_str().filter(|value|*value != "NOASSERTION").unwrap_or("unknown").trim_start_matches("Organization: "),
        "license":license,"identification":format!("checksum-matched DLL; vcpkg port {port} version {recorded_version}; license from its SPDX package metadata")}),
    )
}

fn linux_package(path: &Path) -> Option<Value> {
    let name = path.file_name()?.to_str()?;
    let identity = binary_identity(path)?;
    // The distro record provides an installed-package version, not an inferred
    // ABI/SONAME version. The final bundled file gets its own SHA-256 below.
    let source = [
        "/usr/lib/x86_64-linux-gnu",
        "/lib/x86_64-linux-gnu",
        "/usr/lib",
    ]
    .into_iter()
    .map(|directory| Path::new(directory).join(name))
    .find(|path| path.is_file())?;
    let mut candidates = vec![source.clone()];
    if let Ok(real) = source.canonicalize() {
        candidates.push(real);
    }
    for source in candidates {
        if binary_identity(&source).as_deref() != Some(identity.as_str()) {
            continue;
        }
        let owned = super::output(Command::new("dpkg-query").arg("--search").arg(source)).ok()?;
        let package = owned.lines().next()?.split_once(": ")?.0;
        let details = super::output(Command::new("dpkg-query").args([
            "--show",
            "--showformat=${Package}\t${Version}\t${source:Package}\t${Architecture}",
            package,
        ]))
        .ok()?;
        let mut values = details.split('\t');
        let binary = values.next()?;
        let version = values.next()?;
        let source = values
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or(binary);
        let arch = values.next()?;
        let os = fs::read_to_string("/etc/os-release").ok()?;
        let field = |key: &str| {
            os.lines()
                .find_map(|line| line.strip_prefix(&format!("{key}=")))
                .map(|value| value.trim_matches('"'))
        };
        let id = field("ID")?;
        let release = field("VERSION_ID")?;
        return Some(
            json!({"name":source,"version":version,"producer":"unknown","license":"NOASSERTION",
                "purl":format!("pkg:deb/{}/{}@{}?arch={}&distro={}-{}",purl_encode(id),purl_encode(binary),purl_encode(version),purl_encode(arch),purl_encode(id),purl_encode(release))}),
        );
    }
    None
}

fn purl_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b".-_~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn brew_packages() -> BTreeMap<String, Value> {
    let Ok(text) = super::output(Command::new("brew").args(["info", "--json=v2", "--installed"]))
    else {
        return BTreeMap::new();
    };
    let Ok(data) = serde_json::from_str::<Value>(&text) else {
        return BTreeMap::new();
    };
    data["formulae"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|formula| {
            let name = formula["name"].as_str()?;
            let version = formula["linked_keg"]
                .as_str()
                .filter(|version| !version.is_empty())
                .or_else(|| formula["installed"].as_array()?.first()?["version"].as_str())?;
            Some((
                name.to_owned(),
                json!({"name":name,"version":version,"producer":"unknown",
            "license":formula["license"].as_str().unwrap_or("NOASSERTION")}),
            ))
        })
        .collect()
}

fn brew_package(path: &Path, packages: &BTreeMap<String, Value>) -> Option<Value> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_prefix("lib")?.split('.').next()?;
    let formula = match stem {
        "png16" => "libpng",
        "harfbuzz" => "harfbuzz",
        "brotlicommon" | "brotlidec" | "brotlienc" => "brotli",
        "z" => "zlib",
        "bz2" => "bzip2",
        "tbb" | "tbbmalloc" => "tbb",
        other => other,
    };
    let package = packages.get(formula)?.clone();
    let identity = binary_identity(path)?;
    let files = super::output(Command::new("brew").args(["list", "--formula", formula])).ok()?;
    files
        .lines()
        .map(Path::new)
        .filter(|source| source.file_name() == path.file_name() && source.is_file())
        .any(|source| {
            source.components().any(|part| {
                Some(part.as_os_str().to_string_lossy().as_ref()) == package["version"].as_str()
            }) && binary_identity(source).as_deref() == Some(identity.as_str())
        })
        .then_some(package)
}

fn verify_occt_file(file: &Path, sdk: &Path, target: &str) -> Result<()> {
    let name = file.file_name().context("OCCT filename")?;
    let source = if target.contains("windows") {
        crate::package::runtime_bin(sdk)?
    } else {
        sdk.join("lib")
    }
    .join(name);
    ensure!(
        source.is_file(),
        "bundled OCCT file is absent from the checked SDK: {}",
        file.display()
    );
    // Linux loader edits and macOS signing change bytes. The build ID/UUID
    // ties the final file to the receipt-verified SDK before those edits.
    if target.contains("windows") {
        ensure!(
            crate::hash::file(file)? == crate::hash::file(&source)?,
            "bundled OCCT file differs from the checked SDK"
        );
    } else {
        let identity = binary_identity(file).context("OCCT file lacks a native build identity")?;
        ensure!(
            binary_identity(&source).as_deref() == Some(identity.as_str()),
            "bundled OCCT build identity differs from the checked SDK"
        );
    }
    Ok(())
}

pub(super) fn inventory(
    payload: &Path,
    sdk: &Path,
    target: &str,
    version: &str,
) -> Result<(Vec<Library>, Value)> {
    let root = payload.canonicalize()?;
    let evidence = sdk_evidence(sdk)?;
    let brew = if target.contains("apple") {
        brew_packages()
    } else {
        BTreeMap::new()
    };
    let mut files = Vec::new();
    walk(&root, &root, &mut files)?;
    files.sort();
    ensure!(!files.is_empty(), "release contains no native libraries");
    let mut libraries = Vec::new();
    for file in files {
        let name = file
            .file_name()
            .and_then(|value| value.to_str())
            .context("UTF-8 native library")?;
        let occt = name.starts_with("TK") || name.starts_with("libTK");
        if occt {
            verify_occt_file(&file, sdk, target)?;
        }
        let package = if name == "limo-cad" || name == "Limo-CAD.exe" {
            Some(
                json!({"name":"limo-cad","version":version,"producer":"Limo CAD project","license":"LGPL-2.1-or-later"}),
            )
        } else if occt {
            Some(evidence["occt"].clone())
        } else {
            known_dependency(&file, sdk, &evidence).or_else(|| {
                if target.contains("windows") {
                    vcpkg_package(sdk, &file)
                } else if target.contains("linux") {
                    linux_package(&file)
                } else {
                    brew_package(&file, &brew)
                }
            })
        };
        let identification = if let Some(details) = package
            .as_ref()
            .and_then(|package| package["identification"].as_str())
        {
            details
        } else if occt {
            "verified checked OCCT SDK"
        } else if package.is_some() {
            "SDK or installed package-manager metadata; final-file hash recorded independently"
        } else {
            "unknown producer/version/license; final-file fingerprint only"
        }
        .to_owned();
        let package = package.unwrap_or_else(|| json!({"name":name,"version":"unknown","producer":"unknown","license":"NOASSERTION"}));
        let package_name = if occt {
            name
        } else {
            package["name"].as_str().unwrap_or(name)
        };
        let purl = package["purl"].as_str().map(str::to_owned).or_else(|| {
            (package["version"] != "unknown" && !occt).then(|| {
                format!(
                    "pkg:generic/{}@{}",
                    purl_encode(package_name),
                    purl_encode(package["version"].as_str().unwrap_or("unknown"))
                )
            })
        });
        libraries.push(Library {
            path: file
                .strip_prefix(&root)?
                .to_str()
                .context("UTF-8 package path")?
                .replace('\\', "/"),
            sha256: crate::hash::file(&file)?,
            name: name.into(),
            package_name: package_name.into(),
            purl,
            kind: if is_library(name) {
                "library"
            } else {
                "application"
            }
            .into(),
            version: package["version"].as_str().unwrap_or("unknown").into(),
            producer: package["producer"].as_str().unwrap_or("unknown").into(),
            license: package["license"].as_str().unwrap_or("NOASSERTION").into(),
            identification,
            imports: imports(&file, target)?,
            occt,
        });
    }
    ensure!(
        libraries
            .iter()
            .any(|library| library.name.contains("TKernel")),
        "OCCT TKernel missing from final package"
    );
    ensure!(
        libraries
            .iter()
            .any(|library| library.name.contains("TKDESTEP")),
        "OCCT STEP translator missing from final package"
    );
    let mut names = BTreeMap::new();
    for library in &libraries {
        if let Some(previous) = names.insert(library.name.to_ascii_lowercase(), &library.sha256) {
            ensure!(
                previous == &library.sha256,
                "conflicting bundled native binaries named {}",
                library.name
            );
        }
    }
    Ok((libraries, evidence))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_includes_versioned_sos_and_aliases_and_refuses_external_native_links() {
        for name in [
            "TKernel.dll",
            "libTKernel.7.9.3.dylib",
            "libfreetype.so.6.20.1",
            "libxcb.so",
        ] {
            assert!(is_library(name));
        }
        for name in ["LICENSE", "shader.wgsl", "README.dll.txt"] {
            assert!(!is_library(name));
        }
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().canonicalize().unwrap();
            fs::write(root.join("libowned.so.1"), "library").unwrap();
            std::os::unix::fs::symlink("libowned.so.1", root.join("libowned.so")).unwrap();
            let mut files = Vec::new();
            walk(&root, &root, &mut files).unwrap();
            assert_eq!(files.len(), 2);
            std::os::unix::fs::symlink("/etc/passwd", root.join("libexternal.so")).unwrap();
            assert!(walk(&root, &root, &mut Vec::new())
                .unwrap_err()
                .to_string()
                .contains("leaves the package"));
        }
    }
}
