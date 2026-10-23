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
    manifest["recipes"] = json!({"build":"extensions/Café 零件.limo-script.toml","revision":3});
    manifest["future_vendor"] = json!({"nested":[1,true,"preserve me"]});
    let thumbnail: &[u8] = b"\x89PNG\r\n\x1a\nthumbnail payload";
    let recipe: &[u8] = b"# Editable manufacturing example\nversion = 1\n";
    let initial_bytes = fixture(
        &manifest,
        initial.model_json().as_bytes(),
        &[
            ("preview.png", thumbnail),
            ("extensions/Café 零件.limo-script.toml", recipe),
        ],
    );
    let mut original_raw_recipe = Vec::new();
    ZipArchive::new(Cursor::new(&initial_bytes))
        .unwrap()
        .by_index_raw(4)
        .unwrap()
        .read_to_end(&mut original_raw_recipe)
        .unwrap();
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
            ("extensions/Café 零件.limo-script.toml", recipe),
        ] {
            let mut actual = Vec::new();
            zip.by_name(name).unwrap().read_to_end(&mut actual).unwrap();
            assert_eq!(actual, expected);
        }
        let mut raw_recipe = Vec::new();
        zip.by_index_raw(4)
            .unwrap()
            .read_to_end(&mut raw_recipe)
            .unwrap();
        assert_eq!(
            raw_recipe, original_raw_recipe,
            "Ancillary entries must not be recompressed"
        );
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
fn legacy_archive_snapshots_resave_all_intents_and_ancillary_entries() {
    let mut thermal = Vec::new();
    ciborium::into_writer(
        &json!({"format":"limo-cad-thermal-studies","version":1,"studies":[]}),
        &mut thermal,
    )
    .unwrap();
    let vendor = [0, 255, 7, 42];
    for format in [LEGACY_FORMAT, PREVIOUS_FORMAT] {
        let current = ProjectArchive::new(model(2), metadata()).unwrap();
        let mut manifest = current.manifest().clone();
        manifest["format"] = json!(format);
        manifest["future_vendor"] = json!({"keep":[1,true,"opaque"]});
        let mut legacy: Value = serde_json::from_str(&model(2)).unwrap();
        legacy["format"] = json!(format);
        let text = serde_json::to_string_pretty(&legacy).unwrap() + "\n";
        let bytes = fixture(
            &manifest,
            text.as_bytes(),
            &[
                ("intent/thermal-studies.cbor", &thermal),
                ("intent/vendor.cbor", &vendor),
                ("preview.png", b"synthetic preview"),
            ],
        );
        let opened = ProjectArchive::decode(bytes.clone()).unwrap();
        let snapshot = opened.snapshot().unwrap();
        assert_eq!(opened.model_json(), text);
        assert_eq!(opened.original.as_deref(), Some(bytes.as_slice()));
        legacy["format"] = json!(PROJECT_FORMAT);
        assert_eq!(
            serde_json::from_str::<Value>(snapshot.model_json()).unwrap(),
            legacy
        );
        assert_eq!(
            snapshot.intents(),
            &[
                crate::SnapshotIntent {
                    name: "intent/thermal-studies.cbor".into(),
                    payload: thermal.clone(),
                },
                crate::SnapshotIntent {
                    name: "intent/vendor.cbor".into(),
                    payload: vendor.to_vec(),
                },
            ]
        );
        let migrated = crate::ProjectSnapshot::new(model(15), snapshot.intents().to_vec()).unwrap();
        let candidate = opened.with_snapshot(&migrated, metadata()).unwrap();
        assert_eq!(opened.model_json(), text);
        let saved = candidate.encode().unwrap();
        let reopened = ProjectArchive::decode(saved.clone()).unwrap();
        assert_eq!(reopened.snapshot().unwrap(), migrated);
        assert_eq!(reopened.manifest()["format"], PROJECT_FORMAT);
        assert_eq!(
            reopened.manifest()["future_vendor"],
            manifest["future_vendor"]
        );
        let mut zip = ZipArchive::new(Cursor::new(saved)).unwrap();
        let mut preview = Vec::new();
        zip.by_name("preview.png")
            .unwrap()
            .read_to_end(&mut preview)
            .unwrap();
        assert_eq!(preview, b"synthetic preview");
        assert_eq!(zip.comment(), b"project annotations");
    }
}

#[test]
fn repository_legacy_archive_supports_snapshot_open_and_resave() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/testPiece.nbcad");
    let bytes = crate::read_binary_file(&path).unwrap();
    let opened = ProjectArchive::decode(bytes.clone()).unwrap();
    let original: Value = serde_json::from_str(opened.model_json()).unwrap();
    assert_eq!(original["format"], PREVIOUS_FORMAT);
    let snapshot = opened.snapshot().unwrap();
    let mut expected = original;
    expected["format"] = json!(PROJECT_FORMAT);
    assert_eq!(
        serde_json::from_str::<Value>(snapshot.model_json()).unwrap(),
        expected
    );
    let candidate = opened.with_snapshot(&snapshot, metadata()).unwrap();
    let reopened = ProjectArchive::decode(candidate.encode().unwrap()).unwrap();
    assert_eq!(reopened.snapshot().unwrap(), snapshot);
    assert_eq!(opened.original.as_deref(), Some(bytes.as_slice()));
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
    for (extra_name, central_only) in [
        ("model.json", false),
        ("model.json", true),
        ("extensions/future.bin", false),
        ("extensions/future.bin", true),
    ] {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, contents) in [
            ("manifest.json", archive.manifest().to_string()),
            ("model.json", archive.model_json().to_owned()),
            ("extensions/future.bin", "preserve this payload".to_owned()),
        ] {
            let mut options = zip::write::FullFileOptions::default();
            if name == extra_name {
                options
                    .add_extra_field(
                        0xcafe,
                        b"opaque extension".to_vec().into_boxed_slice(),
                        central_only,
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
        assert_eq!(
            decoded.original.as_ref().unwrap().as_ref(),
            bytes.as_slice()
        );
    }
}

#[test]
fn zip64_extensible_data_and_raw_archive_comment_survive_repeated_saves() {
    let current = ProjectArchive::new(model(7), metadata()).unwrap();
    let data = b"opaque ZIP64 extension\0\xff";
    let comment = b"original archive annotations\0\xff";
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.set_raw_zip64_extensible_data_sector(data.to_vec().into_boxed_slice());
    writer
        .set_raw_comment(comment.to_vec().into_boxed_slice())
        .unwrap();
    for (name, payload) in [
        ("manifest.json", current.manifest().to_string()),
        ("model.json", current.model_json().to_owned()),
    ] {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(payload.as_bytes()).unwrap();
    }
    let mut bytes = writer.finish().unwrap().into_inner();
    for _ in 0..3 {
        let mut archive = ProjectArchive::decode(bytes).unwrap();
        archive.update_model(model(7), metadata()).unwrap();
        bytes = archive.encode().unwrap();
        let zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        assert_eq!(
            zip.raw_zip64_extensible_data_sector(),
            Some(data.as_slice())
        );
        assert_eq!(zip.comment(), comment);
    }
}

#[test]
fn local_only_extra_fields_cannot_disappear_during_a_resave() {
    let current = ProjectArchive::new(model(7), metadata()).unwrap();
    let name = "extensions/future.bin";
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (entry_name, payload) in [
        ("manifest.json", current.manifest().to_string()),
        ("model.json", current.model_json().to_owned()),
        (name, "preserve this payload".to_owned()),
    ] {
        let mut options = zip::write::FullFileOptions::default();
        if entry_name == name {
            options
                .add_extra_field(0xcafe, b"local opaque data", false)
                .unwrap();
        }
        writer.start_file(entry_name, options).unwrap();
        writer.write_all(payload.as_bytes()).unwrap();
    }
    let mut bytes = writer.finish().unwrap().into_inner();
    let mut zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let central = zip.by_name(name).unwrap().central_header_start() as usize;
    drop(zip);
    let extra_size =
        u16::from_le_bytes(bytes[central + 30..central + 32].try_into().unwrap()) as usize;
    assert!(extra_size > 0);
    // Retain the local extension but remove its central-directory copy.
    bytes[central + 30..central + 32].copy_from_slice(&0_u16.to_le_bytes());
    let extra = central + 46 + name.len();
    bytes.drain(extra..extra + extra_size);
    let end = bytes.len() - 22;
    assert_eq!(&bytes[end..end + 4], b"PK\x05\x06");
    let size = u32::from_le_bytes(bytes[end + 12..end + 16].try_into().unwrap());
    bytes[end + 12..end + 16].copy_from_slice(&(size - extra_size as u32).to_le_bytes());
    let archive = ProjectArchive::decode(bytes.clone()).unwrap();
    assert!(archive
        .encode()
        .unwrap_err()
        .to_string()
        .contains("cannot preserve opaque ZIP metadata"));
    assert_eq!(archive.original.as_deref(), Some(bytes.as_slice()));
}

#[test]
fn non_utf8_entry_names_leave_the_original_archive_intact() {
    let current = ProjectArchive::new(model(7), metadata()).unwrap();
    let name = b"extensions/bad.bin";
    let mut bytes = fixture(
        current.manifest(),
        current.model_json().as_bytes(),
        &[(std::str::from_utf8(name).unwrap(), b"preserved extension")],
    );
    let names: Vec<_> = bytes
        .windows(name.len())
        .enumerate()
        .filter_map(|(index, found)| (found == name).then_some(index))
        .collect();
    assert_eq!(
        names.len(),
        2,
        "Local and central names must both be altered"
    );
    for index in names {
        bytes[index + name.len() - 1] = 0xff;
    }
    let archive = ProjectArchive::decode(bytes.clone()).unwrap();
    let failure = archive.encode().unwrap_err().to_string();
    assert!(failure.contains("project entry name"), "{failure}");
    assert!(
        failure.contains("keep the original project archive"),
        "{failure}"
    );
    assert_eq!(archive.original.as_deref(), Some(bytes.as_slice()));
}

#[test]
fn intent_overlay_preserves_unrelated_entries_and_replaces_without_duplicates() {
    let base = ProjectArchive::new(model(14), metadata()).unwrap();
    let bytes = fixture(
        base.manifest(),
        base.model_json().as_bytes(),
        &[
            ("preview.png", b"thumbnail"),
            ("intent/vendor.cbor", b"opaque"),
        ],
    );
    let mut archive = ProjectArchive::decode(bytes).unwrap();
    assert_eq!(
        archive.read_intent("intent/thermal.cbor", 100).unwrap(),
        None
    );
    for payload in [b"first".as_slice(), b"second".as_slice()] {
        archive
            .set_intent("intent/thermal.cbor", Some(payload.to_vec()))
            .unwrap();
        assert_eq!(
            archive
                .read_intent("intent/thermal.cbor", 100)
                .unwrap()
                .unwrap(),
            payload
        );
        let encoded = archive.encode().unwrap();
        let mut zip = ZipArchive::new(Cursor::new(&encoded)).unwrap();
        assert_eq!(zip.comment(), b"project annotations");
        let mut preview = Vec::new();
        zip.by_name("preview.png")
            .unwrap()
            .read_to_end(&mut preview)
            .unwrap();
        assert_eq!(preview, b"thumbnail");
        assert_eq!(
            zip.file_names()
                .filter(|name| name
                    .as_ref()
                    .is_ok_and(|name| name.as_ref() == "intent/thermal.cbor"))
                .count(),
            1
        );
        archive = ProjectArchive::decode(encoded).unwrap();
        assert_eq!(
            archive
                .read_intent("intent/vendor.cbor", 100)
                .unwrap()
                .unwrap(),
            b"opaque"
        );
        assert_eq!(
            archive
                .read_intent("intent/thermal.cbor", 100)
                .unwrap()
                .unwrap(),
            payload
        );
    }
    archive.set_intent("intent/thermal.cbor", None).unwrap();
    assert_eq!(
        archive.read_intent("intent/thermal.cbor", 100).unwrap(),
        None
    );
    let reopened = ProjectArchive::decode(archive.encode().unwrap()).unwrap();
    assert_eq!(
        reopened.read_intent("intent/thermal.cbor", 100).unwrap(),
        None
    );
    assert_eq!(
        reopened
            .read_intent("intent/vendor.cbor", 100)
            .unwrap()
            .unwrap(),
        b"opaque"
    );
}
#[test]
fn intent_names_and_caller_limits_reject_without_modifying_candidate() {
    let mut archive = ProjectArchive::new(model(14), metadata()).unwrap();
    archive
        .set_intent("intent/thermal.cbor", Some(b"keep".to_vec()))
        .unwrap();
    for name in [
        "model.json",
        "manifest.json",
        "/intent/x",
        "intent/../x",
        "intent/./x",
        "intent//x",
        "intent/x/",
        "intent\\x",
        "intent/C:x",
    ] {
        assert!(archive.set_intent(name, Some(vec![1])).is_err(), "{name}");
        assert!(archive.read_intent(name, 100).is_err());
    }
    assert!(archive.read_intent("intent/thermal.cbor", 3).is_err());
    assert_eq!(
        archive
            .read_intent("intent/thermal.cbor", 4)
            .unwrap()
            .unwrap(),
        b"keep"
    );
    let reopened = ProjectArchive::decode(archive.encode().unwrap()).unwrap();
    assert!(reopened.read_intent("intent/thermal.cbor", 3).is_err());
    assert_eq!(
        reopened
            .read_intent("intent/thermal.cbor", 4)
            .unwrap()
            .unwrap(),
        b"keep"
    );
}
#[test]
fn staged_intents_obey_count_and_expanded_archive_limits() {
    let mut archive = ProjectArchive::new(model(14), metadata()).unwrap();
    for n in 0..64 {
        archive
            .set_intent(&format!("intent/{n}"), Some(vec![0; 16]))
            .unwrap();
    }
    assert!(archive.set_intent("intent/extra", None).is_err());
    archive.set_intent("intent/0", Some(vec![1; 17])).unwrap();
    assert!(archive.encode_bounded(MAX_FILE_BYTES, 1000).is_err());
    assert!(archive.encode().is_ok());
}
