use super::*;
use std::path::Path;

fn metadata() -> SaveMetadata<'static> {
    SaveMetadata {
        application_version: "0.1.0",
        saved_at: "2026-09-13T00:00:00.000Z",
    }
}

fn model(schema: u64) -> String {
    json!({"format":PROJECT_FORMAT,"schema_version":schema,"document":{"name":"Editable part"}})
        .to_string()
}

fn fixture(manifest: &Value, model: &[u8], extras: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer.set_comment("project annotations").unwrap();
    writer.start_file("manifest.json", options).unwrap();
    writer.write_all(manifest.to_string().as_bytes()).unwrap();
    writer.start_file("model.json", options).unwrap();
    writer.write_all(model).unwrap();
    writer.add_directory("extensions/", options).unwrap();
    for (name, bytes) in extras {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn opened_project_preserves_recipes_thumbnails_and_future_manifest_data_on_repeated_saves() {
    let initial = ProjectArchive::new(model(7), metadata()).unwrap();
    let mut manifest = initial.manifest().clone();
    manifest["thumbnail"] = json!("preview.png");
    manifest["recipes"] = json!({"build":"extensions/bench.limo-script.toml","revision":3});
    manifest["future_vendor"] = json!({"nested":[1,true,"preserve me"]});
    let thumbnail: &[u8] = b"\x89PNG\r\n\x1a\nthumbnail payload";
    let recipe: &[u8] = b"# Editable manufacturing example\nversion = 1\n";
    let initial_bytes = fixture(
        &manifest,
        initial.model_json().as_bytes(),
        &[
            ("preview.png", thumbnail),
            ("extensions/bench.limo-script.toml", recipe),
        ],
    );
    let mut bytes = initial_bytes;
    let mut saved_size = None;
    for _ in 0..3 {
        let mut opened = ProjectArchive::decode(bytes).unwrap();
        opened.update_model(model(7), metadata()).unwrap();
        bytes = opened.encode().unwrap();
        if let Some(previous) = saved_size {
            assert_eq!(
                bytes.len(),
                previous,
                "re-save must not accumulate stale ZIP entries"
            );
        }
        saved_size = Some(bytes.len());
        let decoded = ProjectArchive::decode(bytes.clone()).unwrap();
        assert_eq!(decoded.manifest(), &manifest);
        assert_eq!(decoded.model_json(), initial.model_json());
        let mut zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        assert_eq!(zip.len(), 5);
        assert_eq!(zip.comment(), b"project annotations");
        assert!(zip.by_name("extensions/").unwrap().is_dir());
        for (name, expected) in [
            ("preview.png", thumbnail),
            ("extensions/bench.limo-script.toml", recipe),
        ] {
            let mut actual = Vec::new();
            zip.by_name(name).unwrap().read_to_end(&mut actual).unwrap();
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn repository_project_and_legacy_container_remain_readable() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/testPiece.nbcad");
    let archive = ProjectArchive::decode(crate::read_binary_file(&path).unwrap()).unwrap();
    assert_eq!(archive.manifest()["container_version"], 1);
    assert!(serde_json::from_str::<Value>(archive.model_json())
        .unwrap()
        .is_object());
    let current = ProjectArchive::new(model(7), metadata()).unwrap();
    let mut manifest = current.manifest().clone();
    manifest["format"] = json!(LEGACY_FORMAT);
    manifest
        .as_object_mut()
        .unwrap()
        .remove("model_schema_version");
    let legacy = json!({"format":LEGACY_FORMAT,"schema_version":1}).to_string();
    let mut decoded = ProjectArchive::decode(fixture(&manifest, legacy.as_bytes(), &[])).unwrap();
    assert_eq!(decoded.model_json(), legacy);
    decoded.update_model(model(7), metadata()).unwrap();
    let migrated = ProjectArchive::decode(decoded.encode().unwrap()).unwrap();
    assert_eq!(migrated.manifest()["format"], PROJECT_FORMAT);
    assert_eq!(migrated.manifest()["model_schema_version"], 7);
}

#[test]
fn previous_brand_archives_migrate_without_losing_extension_data() {
    let current = ProjectArchive::new(model(10), metadata()).unwrap();
    let mut manifest = current.manifest().clone();
    manifest["format"] = json!(PREVIOUS_FORMAT);
    let mut previous: Value = serde_json::from_str(&model(10)).unwrap();
    previous["format"] = json!(PREVIOUS_FORMAT);
    manifest["future_extension"] = json!({"preserve":true});
    let payload = b"private future payload";
    let mut opened = ProjectArchive::decode(fixture(
        &manifest,
        previous.to_string().as_bytes(),
        &[("extensions/private.bin", payload)],
    ))
    .unwrap();
    opened.update_model(model(10), metadata()).unwrap();
    let bytes = opened.encode().unwrap();
    let migrated = ProjectArchive::decode(bytes.clone()).unwrap();
    assert_eq!(migrated.manifest()["format"], PROJECT_FORMAT);
    assert_eq!(
        migrated.manifest()["future_extension"],
        manifest["future_extension"]
    );
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut actual = Vec::new();
    zip.by_name("extensions/private.bin")
        .unwrap()
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(actual, payload);
}

#[test]
fn invalid_envelopes_cannot_replace_a_good_model_or_sneak_past_container_validation() {
    let mut archive = ProjectArchive::new(model(7), metadata()).unwrap();
    let previous_manifest = archive.manifest().clone();
    let previous_model = archive.model_json().to_owned();
    for invalid in [
        "{}",
        "not JSON",
        r#"{"format":"limo-cad-project","schema_version":1.5}"#,
    ] {
        assert!(archive.update_model(invalid.into(), metadata()).is_err());
        assert_eq!(archive.manifest(), &previous_manifest);
        assert_eq!(archive.model_json(), previous_model);
    }
    let mismatched = fixture(&previous_manifest, model(6).as_bytes(), &[]);
    assert!(ProjectArchive::decode(mismatched)
        .err()
        .unwrap()
        .to_string()
        .contains("schema versions do not match"));
    let mut unsupported = previous_manifest.clone();
    unsupported["container_version"] = json!(2);
    assert!(ProjectArchive::decode(fixture(&unsupported, previous_model.as_bytes(), &[])).is_err());
    assert!(ProjectArchive::decode(fixture(&previous_manifest, b"\xff\xfe", &[])).is_err());
    assert!(ProjectArchive::decode(b"PK damaged archive".to_vec()).is_err());
    let mut missing = ZipWriter::new(Cursor::new(Vec::new()));
    missing
        .start_file("manifest.json", SimpleFileOptions::default())
        .unwrap();
    missing
        .write_all(previous_manifest.to_string().as_bytes())
        .unwrap();
    assert!(ProjectArchive::decode(missing.finish().unwrap().into_inner()).is_err());
}

#[test]
fn compressed_and_expanded_limits_include_future_entries_without_unbounded_allocation() {
    let archive = ProjectArchive::new(model(7), metadata()).unwrap();
    let extras = vec![b'x'; 4096];
    let bytes = fixture(
        archive.manifest(),
        archive.model_json().as_bytes(),
        &[("extensions/data.bin", &extras)],
    );
    assert!(ProjectArchive::decode_bounded(bytes.clone(), 16, 8192).is_err());
    assert!(ProjectArchive::decode_bounded(bytes.clone(), 8192, 2048).is_err());
    let loaded = ProjectArchive::decode_bounded(bytes, 8192, 8192).unwrap();
    assert!(loaded.encode_bounded(8192, 2048).is_err());
    assert!(loaded.encode_bounded(16, 8192).is_err());
    assert!(loaded.encode_bounded(8192, 8192).is_ok());
}

#[test]
fn unsupported_opaque_zip_metadata_is_rejected_instead_of_silently_discarded() {
    let archive = ProjectArchive::new(model(7), metadata()).unwrap();
    for extra_name in ["model.json", "extensions/future.bin"] {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, contents) in [
            ("manifest.json", archive.manifest().to_string()),
            ("model.json", archive.model_json().to_owned()),
            ("extensions/future.bin", "preserve this payload".to_owned()),
        ] {
            let mut options = zip::write::FullFileOptions::default();
            if name == extra_name {
                options
                    .add_extra_data(
                        0xcafe,
                        b"opaque extension".to_vec().into_boxed_slice(),
                        false,
                    )
                    .unwrap();
            }
            writer.start_file(name, options).unwrap();
            writer.write_all(contents.as_bytes()).unwrap();
        }
        let bytes = writer.finish().unwrap().into_inner();
        let decoded = ProjectArchive::decode(bytes.clone()).unwrap();
        assert!(decoded
            .encode()
            .err()
            .unwrap()
            .to_string()
            .contains("cannot preserve opaque ZIP metadata"));
        assert_eq!(decoded.original.as_ref().unwrap(), &bytes);
    }
}
