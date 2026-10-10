//! Durable, owned OCCT source/build cache. Incomplete installs never become hits.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Write, path::Path, process::Command};

pub fn lock(directory: &Path) -> Result<fs::File> {
    fs::create_dir_all(directory)?;
    lock_file(&directory.join(".lock"))
}
fn lock_file(path: &Path) -> Result<fs::File> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "cache lock must be a regular file"
        );
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.try_lock()
        .context("OCCT cache is in use; retry or select another --cache-dir")?;
    Ok(file)
}

pub fn compiler_identity(cache: &Path) -> Result<String> {
    let probe = tempfile::tempdir_in(cache)?;
    fs::write(
        probe.path().join("CMakeLists.txt"),
        r#"
cmake_minimum_required(VERSION 3.16)
project(limo_cad_sdk_identity LANGUAGES C CXX)
find_package(Freetype REQUIRED)
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
  find_package(Fontconfig REQUIRED)
endif()
file(WRITE "${CMAKE_BINARY_DIR}/identity" "")
foreach(name CMAKE_VERSION CMAKE_GENERATOR CMAKE_SYSTEM_NAME CMAKE_SYSTEM_VERSION CMAKE_SYSTEM_PROCESSOR
    CMAKE_SIZEOF_VOID_P CMAKE_C_COMPILER_ID CMAKE_C_COMPILER_VERSION
    CMAKE_CXX_COMPILER_ID CMAKE_CXX_COMPILER_VERSION CMAKE_CXX_COMPILER_ARCHITECTURE_ID
    CMAKE_C_FLAGS CMAKE_C_FLAGS_RELEASE CMAKE_CXX_FLAGS CMAKE_CXX_FLAGS_RELEASE
    CMAKE_SHARED_LINKER_FLAGS CMAKE_SHARED_LINKER_FLAGS_RELEASE
    CMAKE_CXX_SIMULATE_ID CMAKE_CXX_SIMULATE_VERSION CMAKE_CXX_IMPLICIT_INCLUDE_DIRECTORIES
    CMAKE_CXX_IMPLICIT_LINK_DIRECTORIES CMAKE_CXX_IMPLICIT_LINK_LIBRARIES
    CMAKE_OSX_SYSROOT CMAKE_OSX_ARCHITECTURES CMAKE_OSX_DEPLOYMENT_TARGET
    FREETYPE_VERSION_STRING FREETYPE_INCLUDE_DIRS FREETYPE_LIBRARIES
    FREETYPE_INCLUDE_DIR_ft2build FREETYPE_INCLUDE_DIR_freetype2 FREETYPE_LIBRARY_RELEASE)
  file(APPEND "${CMAKE_BINARY_DIR}/identity" "${name}=${${name}}\n")
endforeach()
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
  foreach(name Fontconfig_VERSION Fontconfig_INCLUDE_DIRS Fontconfig_LIBRARIES Fontconfig_COMPILE_OPTIONS)
    file(APPEND "${CMAKE_BINARY_DIR}/identity" "${name}=${${name}}\n")
  endforeach()
endif()
foreach(path "${CMAKE_C_COMPILER}" "${CMAKE_CXX_COMPILER}" ${FREETYPE_LIBRARIES} ${Fontconfig_LIBRARIES})
  if(EXISTS "${path}" AND NOT IS_DIRECTORY "${path}")
    file(SHA256 "${path}" digest)
    file(APPEND "${CMAKE_BINARY_DIR}/identity" "input=${path}:${digest}\n")
  endif()
endforeach()
foreach(directory ${FREETYPE_INCLUDE_DIRS})
  foreach(name ft2build.h freetype/freetype.h freetype/config/ftconfig.h)
    if(EXISTS "${directory}/${name}")
      file(SHA256 "${directory}/${name}" digest)
      file(APPEND "${CMAKE_BINARY_DIR}/identity" "header=${directory}/${name}:${digest}\n")
    endif()
  endforeach()
endforeach()
foreach(directory ${Fontconfig_INCLUDE_DIRS})
  foreach(name fontconfig/fontconfig.h fontconfig/fcprivate.h)
    if(EXISTS "${directory}/${name}")
      file(SHA256 "${directory}/${name}" digest)
      file(APPEND "${CMAKE_BINARY_DIR}/identity" "header=${directory}/${name}:${digest}\n")
    endif()
  endforeach()
endforeach()
"#,
    )?;
    crate::build_tools::run(
        Command::new("cmake")
            .arg("-S")
            .arg(probe.path())
            .arg("-B")
            .arg(probe.path().join("build"))
            .args(["-G", "Ninja", "-DCMAKE_BUILD_TYPE=Release"]),
    )?;
    let mut identity = fs::read_to_string(probe.path().join("build/identity"))?;
    for name in [
        "CC",
        "CXX",
        "CFLAGS",
        "CXXFLAGS",
        "CPPFLAGS",
        "LDFLAGS",
        "CMAKE_PREFIX_PATH",
        "SDKROOT",
        "MACOSX_DEPLOYMENT_TARGET",
        "INCLUDE",
        "LIB",
        "LIBPATH",
        "VCToolsVersion",
        "WindowsSDKVersion",
        "WindowsSdkDir",
    ] {
        identity.push_str(&format!(
            "{name}={}\n",
            std::env::var_os(name).unwrap_or_default().to_string_lossy()
        ));
    }
    Ok(identity)
}

pub fn freetype_arguments(identity: &str) -> Result<Vec<String>> {
    let value = |name: &str| -> Result<&str> {
        let prefix = format!("{name}=");
        identity
            .lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .filter(|value| !value.is_empty())
            .with_context(|| format!("FreeType probe missing {name}"))
    };
    let library = Path::new(value("FREETYPE_LIBRARY_RELEASE")?);
    ensure!(
        std::env::consts::OS != "linux"
            || library.extension().is_none_or(|extension| extension != "a"),
        "Native Linux OCCT requires shared FreeType; select its shared SDK via CMAKE_PREFIX_PATH"
    );
    Ok(vec![
        format!(
            "-D3RDPARTY_FREETYPE_INCLUDE_DIR_ft2build={}",
            value("FREETYPE_INCLUDE_DIR_ft2build")?
        ),
        format!(
            "-D3RDPARTY_FREETYPE_INCLUDE_DIR_freetype2={}",
            value("FREETYPE_INCLUDE_DIR_freetype2")?
        ),
        format!("-D3RDPARTY_FREETYPE_LIBRARY={}", library.display()),
        format!(
            "-D3RDPARTY_FREETYPE_LIBRARY_DIR={}",
            library
                .parent()
                .context("FreeType library parent")?
                .display()
        ),
    ])
}

pub fn key(source_hash: &str, compiler: &str, recipe: &str) -> Result<String> {
    crate::hash::reader(
        format!("limo-cad-occt-cache-v1\n{source_hash}\n{compiler}\n{recipe}").as_bytes(),
    )
}

const OWNER: &str = ".limo-sdk-owner";
const RECEIPT: &str = ".limo-sdk-complete.json";
const PREFIX_LOCK: &str = ".limo-sdk-build.lock";

#[derive(Serialize, Deserialize)]
struct Receipt {
    key: String,
    files: BTreeMap<String, String>,
}

fn inventory(root: &Path, directory: &Path, files: &mut BTreeMap<String, String>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)?
            .to_str()
            .context("SDK contains non-UTF8 path")?
            .replace('\\', "/");
        if relative == OWNER || relative == RECEIPT || relative == PREFIX_LOCK {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            let target =
                fs::canonicalize(&path).context("SDK contains a dangling or cyclic symlink")?;
            ensure!(
                target.starts_with(root),
                "SDK symlink escapes its prefix: {}",
                path.display()
            );
            ensure!(
                ![OWNER, RECEIPT, PREFIX_LOCK]
                    .iter()
                    .any(|name| target == root.join(name)),
                "SDK symlink targets untracked cache metadata"
            );
            files.insert(
                relative,
                format!("symlink:{}", fs::read_link(&path)?.to_string_lossy()),
            );
        } else if kind.is_dir() {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                ensure!(
                    entry.metadata()?.file_attributes() & 0x400 == 0,
                    "SDK junctions are not supported"
                );
            }
            inventory(root, &path, files)?;
        } else {
            ensure!(
                kind.is_file(),
                "SDK contains non-regular file {}",
                path.display()
            );
            files.insert(relative, crate::hash::file(&path)?);
        }
    }
    Ok(())
}

fn files(prefix: &Path) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    let root = fs::canonicalize(prefix)?;
    inventory(&root, &root, &mut files)?;
    Ok(files)
}

/// Reserve an empty prefix, or resume this exact recipe's interrupted install.
pub fn prepare(prefix: &Path, key: &str) -> Result<()> {
    if prefix.exists() {
        ensure!(
            fs::symlink_metadata(prefix)?.is_dir(),
            "SDK prefix must be a directory"
        );
        if prefix.join(OWNER).is_file() {
            ensure!(
                fs::symlink_metadata(prefix.join(OWNER))?.is_file(),
                "SDK owner must be a regular file"
            );
            ensure!(
                fs::read_to_string(prefix.join(OWNER))? == key,
                "SDK prefix belongs to another build; choose a fresh --prefix"
            );
            return Ok(());
        }
        ensure!(
            fs::read_dir(prefix)?.next().is_none(),
            "refusing to overwrite an unmanaged SDK prefix; choose a fresh --prefix"
        );
    } else {
        fs::create_dir_all(prefix)?;
    }
    let mut owner = tempfile::NamedTempFile::new_in(prefix)?;
    owner.write_all(key.as_bytes())?;
    owner.persist_noclobber(prefix.join(OWNER))?;
    Ok(())
}

/// Initialize ownership atomically, then exclude every installer using this prefix,
/// independently of its build-cache directory. Recheck ownership under the lock.
pub fn prepare_locked(prefix: &Path, key: &str) -> Result<fs::File> {
    prepare(prefix, key)?;
    let lock = lock_file(&prefix.join(PREFIX_LOCK))?;
    prepare(prefix, key)?;
    Ok(lock)
}

pub fn complete(prefix: &Path, key: &str) -> Result<bool> {
    if !prefix.join(RECEIPT).is_file() {
        return Ok(false);
    }
    ensure!(
        fs::symlink_metadata(prefix.join(RECEIPT))?.is_file(),
        "SDK receipt must be a regular file"
    );
    let receipt: Receipt = serde_json::from_slice(&fs::read(prefix.join(RECEIPT))?)?;
    Ok(receipt.key == key && !receipt.files.is_empty() && receipt.files == files(prefix)?)
}

pub fn publish(prefix: &Path, key: &str) -> Result<()> {
    let files = files(prefix)?;
    ensure!(!files.is_empty(), "cannot publish an empty SDK");
    let mut receipt = tempfile::NamedTempFile::new_in(prefix)?;
    serde_json::to_writer_pretty(
        receipt.as_file_mut(),
        &Receipt {
            key: key.into(),
            files,
        },
    )?;
    receipt.as_file_mut().sync_all()?;
    receipt.persist(prefix.join(RECEIPT))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builders_with_different_caches_cannot_install_to_the_same_prefix() {
        let temporary = tempfile::tempdir().unwrap();
        let prefix = temporary.path().join("sdk");
        let _cache_a = lock(&temporary.path().join("cache-a")).unwrap();
        let _cache_b = lock(&temporary.path().join("cache-b")).unwrap();
        let first = prepare_locked(&prefix, "key").unwrap();
        assert!(prepare_locked(&prefix, "key").is_err());
        fs::write(prefix.join("library"), "installed").unwrap();
        publish(&prefix, "key").unwrap();
        assert!(complete(&prefix, "key").unwrap());
        // An inherited descriptor can outlive this handle during parallel subprocess tests.
        // Explicit release does not depend on those inherited handles closing.
        first.unlock().unwrap();
        drop(first);
        let _next = prepare_locked(&prefix, "key").unwrap();
        assert!(complete(&prefix, "key").unwrap());
    }
    #[cfg(unix)]
    #[test]
    fn sdk_links_are_confined_and_internal_targets_are_fingerprinted() {
        use std::os::unix::fs::symlink;
        let temporary = tempfile::tempdir().unwrap();
        let prefix = temporary.path().join("sdk");
        prepare(&prefix, "key").unwrap();
        fs::write(prefix.join("TKernel.so.7"), "native").unwrap();
        symlink("TKernel.so.7", prefix.join("TKernel.so")).unwrap();
        publish(&prefix, "key").unwrap();
        assert!(complete(&prefix, "key").unwrap());
        fs::write(prefix.join("TKernel.so.7"), "modified").unwrap();
        assert!(!complete(&prefix, "key").unwrap());
        fs::remove_file(prefix.join("TKernel.so.7")).unwrap();
        assert!(complete(&prefix, "key").is_err());
        fs::remove_file(prefix.join("TKernel.so")).unwrap();
        fs::write(temporary.path().join("outside.so"), "external").unwrap();
        symlink(
            temporary.path().join("outside.so"),
            prefix.join("TKernel.so"),
        )
        .unwrap();
        assert!(publish(&prefix, "key").is_err());
    }
    #[test]
    fn abi_recipe_and_compiler_changes_invalidate_cache() {
        let baseline = key("source", "clang1/freetype1/target1", "recipe1").unwrap();
        for (source, compiler, recipe) in [
            ("new-source", "clang1/freetype1/target1", "recipe1"),
            ("source", "clang2/freetype1/target1", "recipe1"),
            ("source", "clang1/freetype2/target1", "recipe1"),
            ("source", "clang1/freetype1/target2", "recipe1"),
            ("source", "clang1/freetype1/target1", "recipe2"),
        ] {
            assert_ne!(baseline, key(source, compiler, recipe).unwrap());
        }
    }
    #[test]
    fn occt_receives_the_exact_probed_freetype_paths() {
        let arguments = freetype_arguments(
            "FREETYPE_INCLUDE_DIR_ft2build=SDK path/include\nFREETYPE_INCLUDE_DIR_freetype2=SDK path/include/freetype2\nFREETYPE_LIBRARY_RELEASE=SDK path/lib/freetype.lib\n"
        ).unwrap();
        assert!(arguments.contains(&"-D3RDPARTY_FREETYPE_LIBRARY=SDK path/lib/freetype.lib".into()));
        assert!(arguments.contains(&"-D3RDPARTY_FREETYPE_LIBRARY_DIR=SDK path/lib".into()));
        assert!(freetype_arguments("FREETYPE_LIBRARY_RELEASE=").is_err());
    }
    #[test]
    fn interrupted_or_modified_sdk_is_never_a_cache_hit() {
        let temporary = tempfile::tempdir().unwrap();
        let prefix = temporary.path().join("sdk");
        prepare(&prefix, "key").unwrap();
        fs::write(prefix.join("library"), b"first").unwrap();
        assert!(!complete(&prefix, "key").unwrap());
        publish(&prefix, "key").unwrap();
        assert!(complete(&prefix, "key").unwrap());
        fs::write(prefix.join("library"), b"changed").unwrap();
        assert!(!complete(&prefix, "key").unwrap());
        assert!(prepare(&prefix, "other-key").is_err());
    }
    #[test]
    fn unmanaged_prefix_is_preserved_and_competing_builder_fails_fast() {
        let temporary = tempfile::tempdir().unwrap();
        fs::write(temporary.path().join("keep"), b"user SDK").unwrap();
        assert!(prepare(temporary.path(), "key").is_err());
        assert_eq!(
            fs::read(temporary.path().join("keep")).unwrap(),
            b"user SDK"
        );
        let _lock = lock(temporary.path()).unwrap();
        assert!(lock(temporary.path()).is_err());
    }
}
