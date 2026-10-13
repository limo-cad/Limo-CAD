use super::*;
use std::fs;

struct Folder(PathBuf);
impl Folder {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("native-private-posts-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, contents).unwrap();
        path
    }
    fn config(&self) -> PathBuf {
        self.0.join("isolated-config")
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn profile() -> Vec<u8> {
    let machine = CamMachineAssignmentDto::three_axis(CamPostConfigDto {
        dialect: PostDialect::Grbl,
        ..Default::default()
    });
    serde_json::to_vec_pretty(&json!({"format":"nbpost","schema_version":machine.profile.schema_version,"machine":machine})).unwrap()
}

#[test]
fn native_private_post_catalog_surfaces_profiles_invalid_and_reference_only_entries() {
    let folder = Folder::new();
    let config = folder.config();
    for (name, bytes) in [
        ("valid.nbpost", profile()),
        ("invalid.nbpost", b"{invalid profile".to_vec()),
        (
            "reference.cps",
            b"// Original source\r\nthrow 'never execute';\r\n".to_vec(),
        ),
    ] {
        let source = folder.file(name, &bytes);
        io::import(&config, &source).unwrap();
        assert_eq!(
            fs::read(config.join("cam-posts").join(name)).unwrap(),
            bytes
        );
    }
    let catalog = io::load(&config).unwrap();
    assert_eq!(catalog.entries.len(), 3);
    assert_eq!(
        catalog
            .entries
            .iter()
            .map(|entry| entry.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["invalid", "reference_only", "native_profile"]
    );
    assert!(catalog.entries[0].machine.is_none());
    assert!(catalog.entries[1].machine.is_none());
    assert!(catalog.entries[2].machine.is_some());
    let expected_messages = catalog
        .entries
        .iter()
        .map(|entry| entry.message.clone())
        .collect::<Vec<_>>();
    let mut state = State::default();
    assert!(state
        .accept(Ok(Completion::Catalog {
            catalog,
            notice: "Loaded".into()
        }))
        .is_none());
    let text = state.text();
    for required in [
        "invalid.nbpost",
        "reference.cps",
        "valid.nbpost",
        "Needs attention",
        "Reference only",
        "Machine profile",
    ] {
        assert!(
            text.contains(required),
            "Missing readable catalog detail: {required}"
        );
    }
    for message in expected_messages {
        assert!(text.contains(&message));
    }
    assert!(state.refresh_machine);
}

#[test]
fn native_private_post_import_keeps_shared_collision_rules_and_retains_catalog_on_failure() {
    let folder = Folder::new();
    let config = folder.config();
    let source = folder.file("shop.cps", b"// Exact original\r\n");
    let catalog = io::import(&config, &source).unwrap();
    assert_eq!(
        io::import(&config, &source).unwrap().entries.len(),
        1,
        "Identical import is idempotent"
    );
    let mut state = State::default();
    let _ = state.accept(Ok(Completion::Catalog {
        catalog,
        notice: "Imported".into(),
    }));
    let prior = state.catalog.clone().unwrap();
    fs::write(&source, "// Different contents").unwrap();
    let error = io::import(&config, &source).err().unwrap();
    assert!(error.contains("different post already has that filename"));
    let _ = state.accept(Err(error));
    assert!(Arc::ptr_eq(state.catalog.as_ref().unwrap(), &prior));
    assert!(state.text().contains("shop.cps"));
    assert!(state.text().contains("existing file was kept"));
    assert_eq!(
        fs::read(config.join("cam-posts/shop.cps")).unwrap(),
        b"// Exact original\r\n"
    );
}

#[test]
fn native_private_post_picker_cancel_never_imports_and_pins_the_selected_config() {
    let folder = Folder::new();
    let config = folder.config();
    let source = folder.file("chosen.cps", b"// Source");
    let mut state = State {
        picking: true,
        ..Default::default()
    };
    assert!(state
        .accept(Ok(Completion::Picked {
            config: config.clone(),
            source: None
        }))
        .is_none());
    assert!(!state.picking);
    assert_eq!(state.notice, "Import cancelled");
    assert!(!config.exists());
    let action = state
        .accept(Ok(Completion::Picked {
            config: config.clone(),
            source: Some(source.clone()),
        }))
        .unwrap();
    assert_eq!(action, (config.clone(), source));
    assert!(
        !config.exists(),
        "Picker completion stages a storage task; it does not write directly"
    );
}

#[test]
fn native_private_post_import_preserves_the_complete_active_project() {
    use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let before =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let folder = Folder::new();
    let source = folder.file("machine.nbpost", &profile());
    let catalog = io::import(&folder.config(), &source).unwrap();
    let mut state = State::default();
    let _ = state.accept(Ok(Completion::Catalog {
        catalog,
        notice: "Imported".into(),
    }));
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
    assert!(state.catalog.as_ref().unwrap().entries[0].machine.is_some());
}
