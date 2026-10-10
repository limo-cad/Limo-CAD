//! Private post storage. Source scripts are preserved byte-for-byte but never
//! executed. Runnable profiles contain a validated built-in machine snapshot,
//! including narrowly typed private extensions in reader version 2.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_FILES: usize = 128;

#[derive(Serialize)]
pub struct Catalog {
    pub directory: String,
    pub entries: Vec<Entry>,
}

#[derive(Serialize)]
pub struct Entry {
    pub file_name: String,
    pub bytes: u64,
    pub kind: String,
    pub message: String,
    pub machine: Option<limo_cad_cam::CamMachineAssignmentDto>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    format: String,
    schema_version: u32,
    machine: limo_cad_cam::CamMachineAssignmentDto,
}

fn reject_unknown_fields(
    input: &serde_json::Value,
    known: &serde_json::Value,
    path: &str,
) -> Result<(), String> {
    match (input, known) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            for (key, value) in a {
                let at = format!("{path}.{key}");
                let expected = b
                    .get(key)
                    .ok_or_else(|| format!("Unsupported native profile setting {at}"))?;
                reject_unknown_fields(value, expected, &at)?;
            }
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            for (index, (value, expected)) in a.iter().zip(b).enumerate() {
                reject_unknown_fields(value, expected, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn directory(config: &Path) -> Result<PathBuf, String> {
    let path = config.join("cam-posts");
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("The private post folder must not be a symbolic link".into());
    }
    fs::create_dir_all(&path).map_err(|e| format!("Could not create private post folder: {e}"))?;
    path.canonicalize().map_err(|e| e.to_string())
}

fn valid_name(name: &str) -> Result<(), String> {
    let parts: Vec<_> = Path::new(name).components().collect();
    if !matches!(parts.as_slice(), [Component::Normal(_)])
        || name.starts_with('.')
        || name.contains(['/', '\\', ':'])
        || name.len() > 180
        || name.chars().any(char::is_control)
        || !matches!(
            Path::new(name)
                .extension()
                .and_then(|s| s.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("cps" | "nbpost")
        )
    {
        return Err("Choose a plain .nbpost or .cps filename, without directories".into());
    }
    Ok(())
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_BYTES || meta.len() == 0
    {
        return Err(
            "Post source must be a nonempty regular file of at most 2 MiB; links are not imported"
                .into(),
        );
    }
    let mut data = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() as u64 > MAX_BYTES {
        return Err("Post exceeds 2 MiB".into());
    }
    std::str::from_utf8(&data).map_err(|_| "Post must be UTF-8 text")?;
    Ok(data)
}

fn entry(path: &Path, bytes: &[u8]) -> Entry {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut result = Entry {
        file_name: name,
        bytes: bytes.len() as u64,
        kind: "reference_only".into(),
        message: "Source reference only — scripts are stored unchanged, not executed by Limo CAD."
            .into(),
        machine: None,
    };
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nbpost"))
        && bytes.iter().copied().find(|b| !b.is_ascii_whitespace()) == Some(b'{')
    {
        let parsed = (|| -> Result<_, String> {
            let mut p: Profile = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            let raw = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            let known = serde_json::to_value(&p).map_err(|e| e.to_string())?;
            reject_unknown_fields(&raw, &known, "profile")?;
            if p.format != "nbpost" || !matches!(p.schema_version, 1 | 2) {
                return Err("Unsupported native post profile format/version".into());
            }
            if p.machine.profile.schema_version > p.schema_version {
                return Err("Native profile version must cover its machine reader version".into());
            }
            p.machine.tool_calls.clear();
            p.machine.validate()?;
            p.machine.ensure_supported_motion()?;

            let mut check = p.machine.clone();
            if check.profile.post.dialect.requires_machine_retract()
                && check.profile.post.machine_retract_z.is_none()
            {
                check.profile.post.machine_retract_z = Some(0.0);
            }
            check.ensure_post_matches(&check.profile.post)?;
            Ok(p.machine)
        })();
        match parsed {
            Ok(machine) => {
                result.kind = "native_profile".into();
                result.message = if let Some(name) = machine
                    .profile
                    .post
                    .siemens_828d
                    .as_ref()
                    .and_then(|s| s.spindle_stop_subprogram.as_deref())
                {
                    format!("Native private profile: {name} before each spindle stop. Controller-side verification required; the subprogram is not simulated.")
                } else {
                    "Built-in renderer with a private machine snapshot. Review machine settings before output.".into()
                };
                result.machine = Some(machine);
            }
            Err(error) => {
                result.kind = "invalid".into();
                result.message = format!("Profile cannot be selected: {error}");
            }
        }
    }
    result
}

pub fn list(config: &Path) -> Result<Catalog, String> {
    let dir = directory(config)?;
    let mut entries = vec![];
    for item in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let path = item.map_err(|e| e.to_string())?.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if valid_name(&name).is_err() {
            continue;
        }
        if entries.len() >= MAX_FILES {
            return Err("Private post folder exceeds the 128-file limit; archive unused posts in another folder".into());
        }
        entries.push(match read(&path) {
            Ok(bytes) => entry(&path, &bytes),
            Err(message) => Entry {
                file_name: name.into_owned(),
                bytes: 0,
                kind: "invalid".into(),
                message,
                machine: None,
            },
        });
    }
    entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(Catalog {
        directory: dir.to_string_lossy().into_owned(),
        entries,
    })
}

fn publish(config: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    valid_name(name)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_BYTES {
        return Err("Post must contain at most 2 MiB of nonempty text".into());
    }
    std::str::from_utf8(bytes).map_err(|_| "Post must be UTF-8 text")?;
    let dir = directory(config)?;
    let target = dir.join(name);
    if let Ok(existing) = read(&target) {
        if existing == bytes {
            return Ok(());
        }
    }
    if fs::symlink_metadata(&target).is_ok() {
        return Err("A different post already has that filename. Rename the import; the existing file was kept.".into());
    }
    if list(config)?.entries.len() >= MAX_FILES {
        return Err("Private post folder is full (128 files)".into());
    }
    let temp = dir.join(format!(".import-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);

        fs::hard_link(&temp, &target)
            .map_err(|e| format!("Could not import without overwriting: {e}"))
    })();
    let _ = fs::remove_file(temp);
    result
}

pub fn import(config: &Path, source: &Path) -> Result<Catalog, String> {
    if !source.is_absolute() {
        return Err("Select an absolute source file path".into());
    }
    let name = source
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Invalid source filename")?;
    valid_name(name)?;
    let bytes = read(source)?;
    publish(config, name, &bytes)?;
    list(config)
}

pub fn save_profile(
    config: &Path,
    name: &str,
    mut machine: limo_cad_cam::CamMachineAssignmentDto,
) -> Result<Catalog, String> {
    if !name.ends_with(".nbpost") {
        return Err("Native profiles use .nbpost".into());
    }
    machine.tool_calls.clear();
    let profile = Profile {
        format: "nbpost".into(),
        schema_version: machine.profile.schema_version,
        machine,
    };
    let data = serde_json::to_vec_pretty(&profile).map_err(|e| e.to_string())?;
    let inspected = entry(Path::new(name), &data);
    if inspected.kind != "native_profile" {
        return Err(inspected.message);
    }
    publish(config, name, &data)?;
    list(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("limo-cad-post-test-{}", uuid::Uuid::new_v4())))
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn source_is_preserved_not_executed_and_never_overwritten() {
        let dir = TestDir::new();
        let bytes = b"// Copyright Example\r\nfunction onOpen() { throw 'do not run'; }\r\n";
        publish(&dir.0, "shop.cps", bytes).unwrap();
        publish(&dir.0, "shop.cps", bytes).unwrap();
        assert!(publish(&dir.0, "shop.cps", b"different").is_err());
        assert_eq!(read(&dir.0.join("cam-posts/shop.cps")).unwrap(), bytes);
        assert_eq!(list(&dir.0).unwrap().entries[0].kind, "reference_only");
        for name in [
            "../x.cps",
            "a/b.cps",
            "a\\b.cps",
            ".hidden.cps",
            "test.exe",
            "a\nb.cps",
        ] {
            assert!(publish(&dir.0, name, bytes).is_err());
        }
    }
    #[test]
    fn native_profiles_validate_and_snapshot_without_tool_mappings() {
        let dir = TestDir::new();
        let machine =
            limo_cad_cam::CamMachineAssignmentDto::three_axis(limo_cad_cam::CamPostConfigDto {
                dialect: limo_cad_cam::PostDialect::Haas,
                ..Default::default()
            });
        let result = save_profile(&dir.0, "mill.nbpost", machine).unwrap();
        assert_eq!(result.entries[0].kind, "native_profile");
        assert!(result.entries[0]
            .machine
            .as_ref()
            .unwrap()
            .profile
            .post
            .machine_retract_z
            .is_none());
        publish(
            &dir.0,
            "broken.nbpost",
            br#"{"format":"nbpost","schema_version":999}"#,
        )
        .unwrap();
        assert_eq!(list(&dir.0).unwrap().entries[0].kind, "invalid");
    }

    #[test]
    fn native_private_stop_profile_requires_reader_v2_and_survives_storage() {
        let dir = TestDir::new();
        let machine =
            limo_cad_cam::CamMachineAssignmentDto::three_axis(limo_cad_cam::CamPostConfigDto {
                dialect: limo_cad_cam::PostDialect::Siemens828d,
                siemens_828d: Some(limo_cad_cam::Siemens828dPostConfigDto {
                    spindle_stop_subprogram: Some("SHOP_STOP".into()),
                    ..Default::default()
                }),
                ..Default::default()
            });
        let result = save_profile(&dir.0, "private.nbpost", machine.clone()).unwrap();
        assert_eq!(result.entries[0].kind, "native_profile");
        assert_eq!(result.entries[0].machine.as_ref(), Some(&machine));
        assert!(result.entries[0].message.contains("SHOP_STOP"));
        let mut raw: serde_json::Value =
            serde_json::from_slice(&read(&dir.0.join("cam-posts/private.nbpost")).unwrap())
                .unwrap();
        assert_eq!(raw["schema_version"], 2);
        raw["schema_version"] = 1.into();
        assert_eq!(
            entry(Path::new("bad.nbpost"), &serde_json::to_vec(&raw).unwrap()).kind,
            "invalid"
        );
        raw["machine"]["profile"]["schema_version"] = 1.into();
        assert_eq!(
            entry(Path::new("bad.nbpost"), &serde_json::to_vec(&raw).unwrap()).kind,
            "invalid"
        );
    }

    #[test]
    #[ignore = "operator-supplied file; set LIMO_CAD_PRIVATE_POST_PATH"]
    fn inspect_operator_private_profile() {
        let path = PathBuf::from(
            std::env::var("LIMO_CAD_PRIVATE_POST_PATH").expect("private profile path"),
        );
        let inspected = entry(&path, &read(&path).unwrap());
        assert_eq!(inspected.kind, "native_profile", "{}", inspected.message);
        println!("{}: {}", inspected.file_name, inspected.message);
    }
    #[test]
    fn unknown_custom_settings_are_not_silently_ignored() {
        let dir = TestDir::new();
        let machine =
            limo_cad_cam::CamMachineAssignmentDto::three_axis(limo_cad_cam::CamPostConfigDto {
                dialect: limo_cad_cam::PostDialect::Haas,
                ..Default::default()
            });
        save_profile(&dir.0, "base.nbpost", machine).unwrap();
        let bytes = read(&dir.0.join("cam-posts/base.nbpost")).unwrap();
        let mut raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        raw["machine"]["profile"]["post"]["custom_start_code"] = "M123".into();
        let inspected = entry(
            Path::new("custom.nbpost"),
            &serde_json::to_vec(&raw).unwrap(),
        );
        assert_eq!(inspected.kind, "invalid");
        assert!(inspected.message.contains("custom_start_code"));
        assert!(inspected.machine.is_none());
        assert!(publish(&dir.0, "empty.cps", b"").is_err());
        assert!(publish(&dir.0, "large.cps", &vec![b'x'; MAX_BYTES as usize + 1]).is_err());
        assert!(publish(&dir.0, "binary.cps", &[0xff]).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn links_are_not_followed() {
        let dir = TestDir::new();
        let folder = directory(&dir.0).unwrap();
        std::os::unix::fs::symlink("/does/not/exist", folder.join("link.cps")).unwrap();
        assert!(publish(&dir.0, "link.cps", b"post").is_err());
        assert_eq!(list(&dir.0).unwrap().entries[0].kind, "invalid");
    }
}
