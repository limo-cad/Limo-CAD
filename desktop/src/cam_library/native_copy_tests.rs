//! Native library-location receipts must guard the bytes copied, not a
//! separate preflight read. All storage in these tests is isolated on disk.
use super::*;

const LEGACY_TOOL: &str = "{\r\n  \"next_tool_id\": 2,\r\n  \"shop_extension\": {\"keep\": true},\r\n  \"tools\": [{\"id\":1,\"number\":1,\"name\":\"Shop mill\",\"kind\":\"flat_end_mill\",\"diameter\":6,\"flute_length\":15,\"overall_length\":50,\"vendor_note\":\"Keep this\"}]\r\n}\r\n";

struct Fixture {
    root: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "limo-cad-native-library-copy-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        let config = root.join("config");
        fs::create_dir(&config).unwrap();
        Self { root, config }
    }

    fn directory(&self, name: &str) -> PathBuf {
        let directory = self.root.join(name);
        fs::create_dir(&directory).unwrap();
        directory
    }

    fn seed_default(&self) -> Snapshot {
        fs::write(self.config.join(LIBRARY), LEGACY_TOOL).unwrap();
        set_location(&self.config, None, LocationAction::UseExisting).unwrap();
        load(&self.config).unwrap()
    }

    fn preference(&self) -> Vec<u8> {
        fs::read(self.config.join(PREFERENCE)).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_empty_destination(directory: &Path) {
    assert!(
        fs::read_dir(directory).unwrap().next().is_none(),
        "A rejected copy must leave no library, temporary file, or lock behind"
    );
}

#[test]
fn native_copy_of_an_empty_default_via_its_canonical_path_does_not_lock_it_twice() {
    let fixture = Fixture::new();
    let opened = load(&fixture.config).unwrap();
    assert!(opened.json.is_none());
    let canonical = fixture.config.canonicalize().unwrap();
    let copied = set_location_at(
        &fixture.config,
        Some(&canonical),
        LocationAction::CopyCurrent,
        Some((&opened.path, &opened.revision)),
    )
    .unwrap();
    assert!(copied.is_default);
    assert_eq!(copied.tool_count, 0);
    assert_eq!(
        fs::read_to_string(fixture.config.join(LIBRARY)).unwrap(),
        "{\"next_tool_id\":1,\"tools\":[]}"
    );
    assert!(!fixture.config.join(".cam-tool-library.lock").exists());
}

#[test]
fn native_copy_rejects_stale_source_content_before_writing_destination_or_preference() {
    let fixture = Fixture::new();
    let opened = fixture.seed_default();
    let destination = fixture.directory("copy");
    let newer_text = LEGACY_TOOL.replace("Shop mill", "Saved by another window");
    let newer = save(&fixture.config, &newer_text, &opened.path, &opened.revision).unwrap();
    let before_preference = fixture.preference();
    let before_source = fs::read(&newer.path).unwrap();

    let rejected = set_location_at(
        &fixture.config,
        Some(&destination),
        LocationAction::CopyCurrent,
        Some((&opened.path, &opened.revision)),
    );

    assert!(rejected.is_err(), "A stale source receipt must reject Copy");
    assert_empty_destination(&destination);
    assert_eq!(fixture.preference(), before_preference);
    assert_eq!(fs::read(&newer.path).unwrap(), before_source);
    let current = load(&fixture.config).unwrap();
    assert_eq!(current.path, newer.path);
    assert_eq!(current.revision, newer.revision);
}

#[test]
fn native_copy_rejects_a_changed_source_location_even_when_library_bytes_match() {
    let fixture = Fixture::new();
    let opened = fixture.seed_default();
    let original_bytes = fs::read(&opened.path).unwrap();
    let other = fixture.directory("other-collection");
    fs::write(other.join(LIBRARY), &original_bytes).unwrap();
    set_location(&fixture.config, Some(&other), LocationAction::UseExisting).unwrap();
    let current = load(&fixture.config).unwrap();
    assert_eq!(current.revision, opened.revision);
    assert_ne!(current.path, opened.path);
    let before_preference = fixture.preference();
    let destination = fixture.directory("copy");

    let rejected = set_location_at(
        &fixture.config,
        Some(&destination),
        LocationAction::CopyCurrent,
        Some((&opened.path, &opened.revision)),
    );

    assert!(
        rejected.is_err(),
        "Equal content cannot authorize a different source collection"
    );
    assert_empty_destination(&destination);
    assert_eq!(fixture.preference(), before_preference);
    assert_eq!(fs::read(&opened.path).unwrap(), original_bytes);
    assert_eq!(fs::read(&current.path).unwrap(), original_bytes);
    assert_eq!(load(&fixture.config).unwrap().path, current.path);
}

#[test]
fn native_copy_respects_the_source_writer_lock_and_retry_preserves_exact_raw_bytes() {
    let fixture = Fixture::new();
    let opened = fixture.seed_default();
    let destination = fixture.directory("copy");
    let before_preference = fixture.preference();
    let original_bytes = fs::read(&opened.path).unwrap();
    assert_ne!(
        opened.json.as_deref().unwrap().as_bytes(),
        original_bytes,
        "The fixture distinguishes normalized UI JSON from original file bytes"
    );
    let source_lock = FileLock::acquire(&fixture.config).unwrap();

    let rejected = set_location_at(
        &fixture.config,
        Some(&destination),
        LocationAction::CopyCurrent,
        Some((&opened.path, &opened.revision)),
    );

    assert!(
        rejected.is_err(),
        "Copy cannot read past a cooperating source writer's lock"
    );
    assert_empty_destination(&destination);
    assert_eq!(fixture.preference(), before_preference);
    assert_eq!(fs::read(&opened.path).unwrap(), original_bytes);
    drop(source_lock);

    let copied = set_location_at(
        &fixture.config,
        Some(&destination),
        LocationAction::CopyCurrent,
        Some((&opened.path, &opened.revision)),
    )
    .unwrap();

    assert_eq!(copied.tool_count, 1);
    assert_eq!(fs::read(destination.join(LIBRARY)).unwrap(), original_bytes);
    assert_eq!(fs::read(&opened.path).unwrap(), original_bytes);
    let current = load(&fixture.config).unwrap();
    assert_eq!(current.path, copied.path);
    assert_eq!(current.revision, opened.revision);
    assert!(!destination.join(".cam-tool-library.lock").exists());
    assert!(!fixture.config.join(".cam-tool-library.lock").exists());
}

#[test]
fn native_use_existing_recovers_from_an_offline_source_without_requiring_its_receipt() {
    let fixture = Fixture::new();
    let default = fixture.seed_default();
    let default_bytes = fs::read(&default.path).unwrap();
    let external = fixture.directory("external");
    let external_text = LEGACY_TOOL.replace("Shop mill", "External mill");
    fs::write(external.join(LIBRARY), &external_text).unwrap();
    set_location(
        &fixture.config,
        Some(&external),
        LocationAction::UseExisting,
    )
    .unwrap();
    let opened = load(&fixture.config).unwrap();
    let offline = fixture.root.join("offline");
    fs::rename(&external, &offline).unwrap();
    assert!(load(&fixture.config).is_err());

    let recovered = set_location_at(
        &fixture.config,
        None,
        LocationAction::UseExisting,
        Some((&opened.path, &opened.revision)),
    )
    .unwrap();

    assert!(recovered.is_default);
    assert_eq!(recovered.tool_count, 1);
    assert!(
        !external.exists(),
        "Recovery must not recreate the old folder"
    );
    assert_eq!(
        fs::read(offline.join(LIBRARY)).unwrap(),
        external_text.as_bytes()
    );
    assert_eq!(fs::read(&default.path).unwrap(), default_bytes);
    let current = load(&fixture.config).unwrap();
    assert_eq!(current.path, default.path);
    assert_eq!(current.revision, default.revision);
}
