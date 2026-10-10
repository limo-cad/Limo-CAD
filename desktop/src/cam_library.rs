//! Per-user library routing, with explicit copy/use-existing behavior.
//! Project tool snapshots are never migrated or merged by a location change.
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    fs::{self, OpenOptions},
    hash::{Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

static STORAGE: Mutex<()> = Mutex::new(());
const LIBRARY: &str = "cam-tool-library.json";
const PREFERENCE: &str = "cam-library-storage.json";
const MAX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize, Default)]
struct Preference {
    directory: Option<PathBuf>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Location {
    pub directory: String,
    pub path: String,
    pub is_default: bool,
    pub exists: bool,
    pub tool_count: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct Snapshot {
    pub json: Option<String>,
    pub path: String,
    pub revision: String,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum LocationAction {
    UseExisting,
    CopyCurrent,
}

fn read_bounded(path: &Path, max: u64) -> Result<Option<String>, String> {
    let metadata = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not inspect {}: {error}", path.display())),
    };
    if !metadata.is_file() || metadata.len() > max {
        return Err(format!(
            "{} is not a regular file within the storage size limit",
            path.display()
        ));
    }

    use std::io::Read;
    let file =
        fs::File::open(path).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
    let mut text = String::new();
    file.take(max + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    if text.len() as u64 > max {
        return Err("Tool library exceeds the 16 MB safety limit".into());
    }
    Ok(Some(text))
}

pub(crate) fn validate_library(json: &str) -> Result<usize, String> {
    if json.len() as u64 > MAX_BYTES {
        return Err("Tool library exceeds the 16 MB safety limit".into());
    }
    #[derive(Deserialize)]
    struct LibraryData {
        next_tool_id: u64,
        tools: Vec<limo_cad_cam::CamToolDto>,
    }
    let parsed: LibraryData = serde_json::from_str(json)
        .map_err(|e| format!("Tool library is not valid library JSON: {e}"))?;
    if parsed.next_tool_id == 0
        || parsed.next_tool_id > 9_007_199_254_740_991
        || parsed.tools.iter().any(|t| t.id >= parsed.next_tool_id)
    {
        return Err("Tool library has an invalid next-tool counter".into());
    }
    let count = parsed.tools.len();
    let mut ids = std::collections::HashSet::new();

    for tool in parsed.tools {
        if !ids.insert(tool.id) {
            return Err("Tool library has duplicate internal tool ids".into());
        }
        let doc = limo_cad_cam::CamDocumentDto {
            next_tool_id: parsed.next_tool_id,
            tools: vec![tool],
            ..Default::default()
        };
        doc.validate_for_editing()
            .map_err(|e| format!("Invalid central tool: {e}"))?;
    }
    Ok(count)
}

/// Older files may omit fields that Rust supplies through serde defaults.
/// Give the UI complete tool records without rewriting the file on read or
/// dropping additional collection/tool metadata. Revisions still hash disk bytes.
fn normalized_library(json: &str) -> Result<String, String> {
    validate_library(json)?;
    let mut value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let tools = value
        .get_mut("tools")
        .and_then(|v| v.as_array_mut())
        .ok_or("Library tools must be an array")?;
    for tool in tools {
        let typed: limo_cad_cam::CamToolDto =
            serde_json::from_value(tool.clone()).map_err(|e| e.to_string())?;
        let fields = serde_json::to_value(typed).map_err(|e| e.to_string())?;
        tool.as_object_mut()
            .ok_or("Library tool must be an object")?
            .extend(
                fields
                    .as_object()
                    .ok_or("Could not serialize library tool")?
                    .clone(),
            );
    }
    let result = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    if result.len() as u64 > MAX_BYTES {
        return Err(
            "Tool library exceeds the 16 MB limit after expanding legacy tool fields".into(),
        );
    }
    Ok(result)
}

fn configured_directory(config: &Path) -> Result<(PathBuf, bool), String> {
    let preference = match read_bounded(&config.join(PREFERENCE), 64 * 1024)? {
        Some(json) => serde_json::from_str::<Preference>(&json)
            .map_err(|e| format!("Library location preference is invalid: {e}"))?,
        None => Preference::default(),
    };
    directory_choice(config, preference.directory.as_deref())
}

fn directory_choice(config: &Path, chosen: Option<&Path>) -> Result<(PathBuf, bool), String> {
    match chosen {
        None => Ok((config.to_path_buf(), true)),
        Some(directory) => {
            if !directory.is_absolute() || !directory.is_dir() {
                return Err("The selected tool-library folder is unavailable. Reconnect it or choose another folder in Settings; no local replacement is created.".into());
            }
            let path = directory
                .canonicalize()
                .map_err(|e| format!("Could not resolve library folder: {e}"))?;
            let default = config
                .canonicalize()
                .unwrap_or_else(|_| config.to_path_buf());
            let is_default = path == default;
            Ok((path, is_default))
        }
    }
}

fn snapshot(directory: &Path) -> Result<Snapshot, String> {
    let path = directory.join(LIBRARY);
    let json = read_bounded(&path, MAX_BYTES)?;
    snapshot_from_raw(directory, json.as_deref())
}

fn snapshot_from_raw(directory: &Path, json: Option<&str>) -> Result<Snapshot, String> {
    let mut hash = DefaultHasher::new();
    json.hash(&mut hash);
    Ok(Snapshot {
        json: json.map(normalized_library).transpose()?,
        path: directory.join(LIBRARY).to_string_lossy().into_owned(),
        revision: format!("{:016x}", hash.finish()),
    })
}

fn location(directory: &Path, is_default: bool) -> Result<Location, String> {
    let data = snapshot(directory)?;
    Ok(Location {
        directory: directory.to_string_lossy().into_owned(),
        path: data.path,
        is_default,
        exists: data.json.is_some(),
        tool_count: data
            .json
            .as_deref()
            .map(validate_library)
            .transpose()?
            .unwrap_or(0),
    })
}

struct FileLock(PathBuf);
impl FileLock {
    fn acquire(directory: &Path) -> Result<Self, String> {
        let path = directory.join(".cam-tool-library.lock");
        OpenOptions::new().write(true).create_new(true).open(&path)
            .map_err(|e|format!("Tool library is busy or not writable ({e}). Retry after the other writer finishes; if a crashed app left {}, remove that lock only after closing all writers.",path.display()))?;
        Ok(Self(path))
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn atomic_write(target: &Path, text: &str, replace: bool) -> Result<(), String> {
    let parent = target.parent().ok_or("Storage path has no parent")?;
    let temp = parent.join(format!(".limo-library-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| format!("Could not create library file: {e}"))?;
        file.write_all(text.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("Could not write library file: {e}"))?;
        drop(file);
        if replace {fs::rename(&temp,target)} else {fs::hard_link(&temp,target)}
            .map_err(|e|format!("Could not publish {}: {e}. Existing files are not merged or overwritten when copying.",target.display()))
    })();
    let _ = fs::remove_file(&temp);
    result
}

pub fn load(config: &Path) -> Result<Snapshot, String> {
    let _guard = STORAGE.lock().map_err(|_| "Library storage lock failed")?;
    let (directory, _) = configured_directory(config)?;
    snapshot(&directory)
}

pub fn save(
    config: &Path,
    json: &str,
    expected_path: &str,
    expected_revision: &str,
) -> Result<Snapshot, String> {
    let _guard = STORAGE.lock().map_err(|_| "Library storage lock failed")?;
    validate_library(json)?;
    let (directory, is_default) = configured_directory(config)?;
    if is_default {
        fs::create_dir_all(&directory)
            .map_err(|e| format!("Could not create default library folder: {e}"))?;
    }
    let _file_lock = FileLock::acquire(&directory)?;
    let current = snapshot(&directory)?;
    if current.path != expected_path || current.revision != expected_revision {
        return Err("The central library or its location changed after loading. Reload the library and retry; nothing was overwritten.".into());
    }
    atomic_write(&directory.join(LIBRARY), json, true)?;
    snapshot(&directory)
}

pub fn get_location(config: &Path) -> Result<Location, String> {
    let _guard = STORAGE.lock().map_err(|_| "Library storage lock failed")?;
    let (directory, is_default) = configured_directory(config)?;
    location(&directory, is_default)
}
pub fn inspect_location(config: &Path, directory: Option<&Path>) -> Result<Location, String> {
    let _guard = STORAGE.lock().map_err(|_| "Library storage lock failed")?;
    let (directory, is_default) = directory_choice(config, directory)?;
    location(&directory, is_default)
}
#[cfg(test)]
pub fn set_location(
    config: &Path,
    directory: Option<&Path>,
    action: LocationAction,
) -> Result<Location, String> {
    set_location_at(config, directory, action, None)
}

/// Copy the collection opened by the caller, retaining its exact source bytes.
/// UseExisting intentionally needs no readable old source, so an offline custom
/// location can always be replaced by an explicitly selected valid folder.
pub fn set_location_at(
    config: &Path,
    directory: Option<&Path>,
    action: LocationAction,
    expected: Option<(&str, &str)>,
) -> Result<Location, String> {
    let _guard = STORAGE.lock().map_err(|_| "Library storage lock failed")?;
    let (target, is_default) = directory_choice(config, directory)?;
    if is_default {
        fs::create_dir_all(&target)
            .map_err(|e| format!("Could not create default library folder: {e}"))?;
    }
    let _file_lock = FileLock::acquire(&target)?;
    let destination = location(&target, is_default)?;
    if matches!(action, LocationAction::CopyCurrent) {
        if destination.exists {
            return Err("That folder already contains a library. Use it explicitly, or choose an empty folder for a copy. Nothing was overwritten.".into());
        }
        let (source, source_is_default) = configured_directory(config)?;
        if source_is_default {
            fs::create_dir_all(&source)
                .map_err(|e| format!("Could not create default library folder: {e}"))?;
        }
        let same_directory = source.canonicalize().map_err(|e| e.to_string())?
            == target.canonicalize().map_err(|e| e.to_string())?;
        let _source_lock = if same_directory {
            None
        } else {
            Some(FileLock::acquire(&source)?)
        };
        let raw = read_bounded(&source.join(LIBRARY), MAX_BYTES)?;
        let opened = snapshot_from_raw(&source, raw.as_deref())?;
        if expected
            .is_some_and(|(path, revision)| opened.path != path || opened.revision != revision)
        {
            return Err("The current library or its location changed after loading. Refresh before copying; no destination or preference was changed.".into());
        }
        let json = raw.unwrap_or_else(|| "{\"next_tool_id\":1,\"tools\":[]}".into());
        atomic_write(&target.join(LIBRARY), &json, false)?;
    }
    fs::create_dir_all(config).map_err(|e| format!("Could not save library location: {e}"))?;
    let preference = Preference {
        directory: if is_default {
            None
        } else {
            Some(target.clone())
        },
    };
    atomic_write(&config.join(PREFERENCE),&serde_json::to_string(&preference).map_err(|e|e.to_string())?,true)
        .map_err(|e|format!("{e} Location was not changed. Any completed library copy remains in the selected folder."))?;
    location(&target, is_default)
}

#[cfg(test)]
mod native_copy_tests;

#[cfg(test)]
mod tests {
    use super::*;
    const EMPTY: &str = "{\"next_tool_id\":1,\"tools\":[]}";
    const ONE: &str = r#"{"next_tool_id":2,"tools":[{"id":1,"number":1,"name":"Shop mill","kind":"flat_end_mill","diameter":6,"flute_length":15,"overall_length":50}]}"#;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir()
                .join(format!("limo-cad-library-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
        fn dir(&self, name: &str) -> PathBuf {
            let p = self.0.join(name);
            fs::create_dir(&p).unwrap();
            p
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn put(config: &Path, json: &str) -> Snapshot {
        let s = load(config).unwrap();
        save(config, json, &s.path, &s.revision).unwrap()
    }

    #[test]
    fn default_path_reads_legacy_file_and_persists_custom_preference() {
        let t = Temp::new();
        let config = t.dir("config");
        let dest = t.dir("shared");
        fs::write(config.join(LIBRARY), ONE).unwrap();
        assert_eq!(get_location(&config).unwrap().tool_count, 1);
        assert!(get_location(&config).unwrap().is_default);
        let changed = set_location(&config, Some(&dest), LocationAction::UseExisting).unwrap();
        assert!(!changed.exists);
        assert!(!changed.is_default);
        assert_eq!(
            load(&config).unwrap().path,
            dest.canonicalize().unwrap().join(LIBRARY).to_string_lossy()
        );
        assert_eq!(fs::read_to_string(config.join(LIBRARY)).unwrap(), ONE);
        put(&config, EMPTY);
        set_location(&config, None, LocationAction::UseExisting).unwrap();
        assert_eq!(
            load(&config).unwrap().json,
            Some(normalized_library(ONE).unwrap())
        );
    }
    #[test]
    fn copy_is_explicit_keeps_source_and_never_overwrites_destination() {
        let t = Temp::new();
        let config = t.dir("config");
        let dest = t.dir("copy");
        put(&config, ONE);
        let changed = set_location(&config, Some(&dest), LocationAction::CopyCurrent).unwrap();
        assert_eq!(changed.tool_count, 1);
        assert_eq!(fs::read_to_string(config.join(LIBRARY)).unwrap(), ONE);
        assert_eq!(fs::read_to_string(dest.join(LIBRARY)).unwrap(), ONE);
        set_location(&config, None, LocationAction::UseExisting).unwrap();
        put(&config, EMPTY);
        assert!(
            set_location(&config, Some(&dest), LocationAction::CopyCurrent)
                .unwrap_err()
                .contains("already contains")
        );
        assert!(get_location(&config).unwrap().is_default);
        assert_eq!(fs::read_to_string(dest.join(LIBRARY)).unwrap(), ONE);
        set_location(&config, Some(&dest), LocationAction::UseExisting).unwrap();
        assert_eq!(get_location(&config).unwrap().tool_count, 1);
    }
    #[test]
    fn stale_content_and_location_never_overwrite_newer_library() {
        let t = Temp::new();
        let config = t.dir("config");
        let dest = t.dir("other");
        let old = put(&config, EMPTY);
        put(&config, ONE);
        assert!(save(&config, EMPTY, &old.path, &old.revision)
            .unwrap_err()
            .contains("changed"));
        let old = load(&config).unwrap();
        set_location(&config, Some(&dest), LocationAction::UseExisting).unwrap();
        assert!(save(&config, ONE, &old.path, &old.revision)
            .unwrap_err()
            .contains("changed"));
        assert!(!dest.join(LIBRARY).exists());
        assert_eq!(fs::read_to_string(config.join(LIBRARY)).unwrap(), ONE);
    }
    #[test]
    fn corrupt_missing_relative_and_busy_locations_fail_without_changing_preference() {
        let t = Temp::new();
        let config = t.dir("config");
        let bad = t.dir("bad");
        put(&config, ONE);
        fs::write(bad.join(LIBRARY), "not json").unwrap();
        assert!(set_location(&config, Some(&bad), LocationAction::UseExisting).is_err());
        assert!(set_location(
            &config,
            Some(Path::new("relative")),
            LocationAction::UseExisting
        )
        .is_err());
        assert!(set_location(
            &config,
            Some(&t.0.join("missing")),
            LocationAction::UseExisting
        )
        .is_err());
        let lock = FileLock::acquire(&config).unwrap();
        let current = load(&config).unwrap();
        assert!(save(&config, EMPTY, &current.path, &current.revision)
            .unwrap_err()
            .contains("busy"));
        drop(lock);
        assert!(get_location(&config).unwrap().is_default);
        assert_eq!(
            load(&config).unwrap().json,
            Some(normalized_library(ONE).unwrap())
        );
    }
    #[test]
    fn offline_custom_folder_is_not_recreated_and_can_be_reset() {
        let t = Temp::new();
        let config = t.dir("config");
        let external = t.dir("external");
        set_location(&config, Some(&external), LocationAction::UseExisting).unwrap();
        let previous = load(&config).unwrap();
        fs::rename(&external, t.0.join("unmounted")).unwrap();
        assert!(save(&config, EMPTY, &previous.path, &previous.revision)
            .unwrap_err()
            .contains("unavailable"));
        assert!(!external.exists());
        set_location(&config, None, LocationAction::UseExisting).unwrap();
        assert!(get_location(&config).unwrap().is_default);
    }
    #[test]
    fn malformed_library_is_rejected_before_any_replace() {
        let t = Temp::new();
        let config = t.dir("config");
        let current = put(&config, ONE);
        for json in [
            "{}",
            r#"{"next_tool_id":0,"tools":[]}"#,
            r#"{"next_tool_id":1,"tools":[{"id":1}]}"#,
        ] {
            assert!(save(&config, json, &current.path, &current.revision).is_err());
        }
        assert_eq!(
            load(&config).unwrap().json,
            Some(normalized_library(ONE).unwrap())
        );
    }

    #[test]
    fn legacy_tools_are_completed_in_memory_without_rewriting_or_losing_extra_metadata() {
        let t = Temp::new();
        let config = t.dir("config");
        let mut legacy: serde_json::Value = serde_json::from_str(ONE).unwrap();
        legacy["collection_label"] = "Shop A".into();
        legacy["tools"][0]["vendor_note"] = "Keep this".into();
        let original = serde_json::to_string(&legacy).unwrap();
        fs::write(config.join(LIBRARY), &original).unwrap();
        let loaded = load(&config).unwrap();
        let normalized: serde_json::Value =
            serde_json::from_str(loaded.json.as_ref().unwrap()).unwrap();
        assert_eq!(
            normalized["tools"][0]["cutting_presets"],
            serde_json::json!([])
        );
        assert!(normalized["tools"][0]["cutting"]["feed_xy"].is_number());
        assert_eq!(normalized["tools"][0]["vendor_note"], "Keep this");
        assert_eq!(normalized["collection_label"], "Shop A");
        assert_eq!(fs::read_to_string(config.join(LIBRARY)).unwrap(), original);

        save(
            &config,
            loaded.json.as_ref().unwrap(),
            &loaded.path,
            &loaded.revision,
        )
        .unwrap();
        assert_eq!(load(&config).unwrap().json, loaded.json);
    }
}
