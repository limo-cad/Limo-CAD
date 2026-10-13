//! SDK discovery shared by the native build script and the Rust task doctor.
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const LIBRARIES: &[&str] = &[
    "TKDESTEP",
    "TKXSBase",
    "TKDE",
    "TKFillet",
    "TKHLR",
    "TKOffset",
    "TKBool",
    "TKBO",
    "TKShHealing",
    "TKPrim",
    "TKTopAlgo",
    "TKMesh",
    "TKBRep",
    "TKGeomAlgo",
    "TKGeomBase",
    "TKG3d",
    "TKG2d",
    "TKMath",
    "TKernel",
];

pub struct Sdk {
    pub include: PathBuf,
    pub lib: PathBuf,
}

#[cfg(windows)]
fn compiler_path(path: PathBuf) -> PathBuf {
    use std::{ffi::OsString, path::Component, path::Prefix};
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path;
    };
    let mut result = match prefix.kind() {
        Prefix::VerbatimDisk(letter) => PathBuf::from(format!("{}:", char::from(letter))),
        Prefix::VerbatimUNC(server, share) => {
            let mut prefix = OsString::from(r"\\");
            prefix.push(server);
            prefix.push(r"\");
            prefix.push(share);
            PathBuf::from(prefix)
        }
        _ => return path,
    };
    result.extend(components);
    result
}

#[cfg(not(windows))]
fn compiler_path(path: PathBuf) -> PathBuf {
    path
}

pub fn roots(
    os: &str,
    arch: &str,
    repository: &Path,
    explicit: Option<PathBuf>,
    vcpkg: Option<PathBuf>,
    triplet: Option<String>,
) -> Result<Vec<PathBuf>, String> {
    if let Some(root) = explicit {
        return Ok(vec![root]);
    }
    let triplet = triplet.unwrap_or_else(|| match arch {
        "aarch64" => "arm64-windows".into(),
        "x86" => "x86-windows".into(),
        _ => "x64-windows".into(),
    });
    if os == "windows" {
        let expected = match arch {
            "x86_64" => "x64-",
            "aarch64" => "arm64-",
            "x86" => "x86-",
            _ => {
                return Err(format!(
                    "Unsupported OCCT Windows target architecture {arch}"
                ))
            }
        };
        if !triplet.starts_with(expected) || !triplet.contains("windows") {
            return Err(format!(
                "VCPKG_TARGET_TRIPLET {triplet} does not match Windows {arch}"
            ));
        }
    }
    if let Some(root) = vcpkg {
        return Ok(vec![root.join(&triplet)]);
    }
    Ok(match os {
        "windows" => vec![repository.join("vcpkg_installed").join(triplet)],
        "linux" => vec!["/usr".into(), "/opt/opencascade".into()],
        "macos" => vec![
            "/opt/homebrew/opt/opencascade".into(),
            "/usr/local/opt/opencascade".into(),
            "/opt/opencascade".into(),
        ],
        _ => {
            return Err(format!(
                "Native OCCT is unsupported on {os}; use the host-neutral engine"
            ))
        }
    })
}

fn version_is_compatible(header: &str) -> bool {
    let value = |name| {
        header.lines().find_map(|line| {
            let mut tokens = line.split_whitespace();
            (tokens.next() == Some("#define") && tokens.next() == Some(name))
                .then(|| tokens.next().and_then(|value| value.parse::<u32>().ok()))
                .flatten()
        })
    };
    value("OCC_VERSION_MAJOR") == Some(7) && value("OCC_VERSION_MINOR") == Some(9)
}

fn missing_libraries(directory: &Path, os: &str) -> Vec<&'static str> {
    LIBRARIES
        .iter()
        .copied()
        .filter(|stem| {
            let name = match os {
                "windows" => format!("{stem}.lib"),
                "macos" => format!("lib{stem}.dylib"),
                _ => format!("lib{stem}.so"),
            };
            !directory.join(name).is_file()
        })
        .collect()
}

pub fn resolve(
    roots: &[PathBuf],
    os: &str,
    arch: &str,
    override_lib: Option<&Path>,
) -> Result<Sdk, String> {
    let mut failures = Vec::new();
    for root in roots {
        let result = (|| {
            let include = ["include/opencascade", "inc", "include"]
                .into_iter()
                .map(|path| root.join(path))
                .find(|path| path.join("Standard_Version.hxx").is_file())
                .ok_or_else(|| "Standard_Version.hxx not found".to_string())?;
            let header = fs::read_to_string(include.join("Standard_Version.hxx"))
                .map_err(|e| e.to_string())?;
            if !version_is_compatible(&header) {
                return Err("SDK headers must use the OCCT 7.9 ABI".into());
            }
            let lib = if let Some(path) = override_lib {
                let missing = missing_libraries(path, os);
                if !missing.is_empty() {
                    return Err(format!(
                        "LIMO_CAD_OCCT_LIB_DIR {} missing {}",
                        path.display(),
                        missing.join(", ")
                    ));
                }
                path.to_owned()
            } else {
                let multiarch = match arch {
                    "aarch64" => "lib/aarch64-linux-gnu",
                    "x86" => "lib/i386-linux-gnu",
                    _ => "lib/x86_64-linux-gnu",
                };
                [
                    "lib",
                    "lib64",
                    multiarch,
                    "win64/vc17/lib",
                    "win64/vc16/lib",
                    "win64/vc15/lib",
                    "win64/vc14/lib",
                ]
                .into_iter()
                .map(|path| root.join(path))
                .find(|path| missing_libraries(path, os).is_empty())
                .ok_or_else(|| format!("complete OCCT link libraries for {os}/{arch} not found"))?
            };
            Ok(Sdk {
                include: compiler_path(include),
                lib: compiler_path(lib),
            })
        })();
        match result {
            Ok(sdk) => return Ok(sdk),
            Err(error) => failures.push(format!("{}: {error}", root.display())),
        }
    }
    Err(format!(
        "Compatible OCCT SDK not found. Set OCCT_ROOT to a complete 7.9.x prefix.\n{}",
        failures.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "limo-cad-sdk-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn sdk(&self, name: &str, version: u32) -> PathBuf {
            let root = self.0.join(name);
            fs::create_dir_all(root.join("include/opencascade")).unwrap();
            fs::create_dir(root.join("lib")).unwrap();
            fs::write(
                root.join("include/opencascade/Standard_Version.hxx"),
                format!("#define OCC_VERSION_MAJOR {version}\n#define OCC_VERSION_MINOR 9\n"),
            )
            .unwrap();
            for lib in LIBRARIES {
                fs::write(root.join(format!("lib/{lib}.lib")), []).unwrap();
            }
            root
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn incomplete_default_root_does_not_hide_sdk() {
        let fixture = Fixture::new();
        let empty = fixture.0.join("empty");
        fs::create_dir(&empty).unwrap();
        let good = fixture.sdk("good", 7);
        assert_eq!(
            resolve(&[empty, good.clone()], "windows", "x86_64", None)
                .unwrap()
                .lib,
            good.join("lib")
        );
        fs::remove_file(good.join("lib/TKDESTEP.lib")).unwrap();
        assert!(resolve(&[good], "windows", "x86_64", None).is_err());
    }
    #[test]
    fn overrides_fail_closed_and_wrong_abi_is_rejected() {
        let fixture = Fixture::new();
        let good = fixture.sdk("good", 7);
        let bad = fixture.sdk("bad", 8);
        assert!(resolve(&[bad], "windows", "x86_64", None).is_err());
        let override_roots = roots(
            "windows",
            "x86_64",
            &fixture.0,
            Some(fixture.0.join("typo")),
            Some(good.clone()),
            None,
        )
        .unwrap();
        assert_eq!(override_roots.len(), 1);
        assert!(resolve(&override_roots, "windows", "x86_64", None).is_err());
        assert!(resolve(&[good], "windows", "x86_64", Some(&fixture.0.join("typo"))).is_err());
    }
    #[test]
    fn windows_triplet_matches_cargo_target() {
        assert_eq!(
            roots("windows", "aarch64", Path::new("repo"), None, None, None).unwrap(),
            [PathBuf::from("repo/vcpkg_installed/arm64-windows")]
        );
        assert!(roots(
            "windows",
            "aarch64",
            Path::new("repo"),
            None,
            None,
            Some("x64-windows".into())
        )
        .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn canonical_sdk_paths_are_usable_by_msvc() {
        let fixture = Fixture::new();
        let root = fixture.sdk("SDK with spaces", 7);
        let sdk = resolve(
            &[fs::canonicalize(&root).unwrap()],
            "windows",
            "x86_64",
            None,
        )
        .unwrap();
        for (actual, expected) in [
            (&sdk.include, root.join("include/opencascade")),
            (&sdk.lib, root.join("lib")),
        ] {
            assert_eq!(
                fs::canonicalize(actual).unwrap(),
                fs::canonicalize(expected).unwrap()
            );
            assert!(actual.is_absolute());
            assert!(matches!(
                actual.components().next(),
                Some(std::path::Component::Prefix(prefix))
                    if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::UNC(_, _))
            ));
        }
        assert!(sdk.include.join("Standard_Version.hxx").is_file());
    }

    #[cfg(windows)]
    #[test]
    fn compiler_paths_preserve_unc_and_unicode_locations() {
        for (input, expected) in [
            (
                r"\\?\C:\SDK with spaces\零件\include",
                r"C:\SDK with spaces\零件\include",
            ),
            (
                r"\\?\UNC\server\share\SDK\include",
                r"\\server\share\SDK\include",
            ),
            (r"C:\SDK\include", r"C:\SDK\include"),
            (r"relative\include", r"relative\include"),
        ] {
            assert_eq!(compiler_path(PathBuf::from(input)), PathBuf::from(expected));
        }
    }
}
