use super::super::presets as profiles;
use super::*;
use limo_cad_cam::{CamCuttingPresetDto, CuttingParametersDto};
const CURRENT: &str = "/native/ui/cutting_profile";
const SECTION: &str = "/native/ui/tool_section";
const OP: &str = "/native/ui/apply_cutting_profile";
fn cam() -> CamDocumentDto {
    let mut cam = job();
    cam.units = CamUnits::Inches;
    cam.tools[0].cutting.feed_xy = f64::from_bits(0x408d480000000001);
    cam.tools[0].cutting_presets = vec![
        CamCuttingPresetDto {
            name: "Aluminium".into(),
            cutting: CuttingParametersDto {
                spindle_rpm: 17000,
                feed_xy: f64::from_bits(0x40990b3333333334),
                feed_z: 273.1234567890123,
                coolant: limo_cad_cam::CoolantMode::Mist,
            },
        },
        CamCuttingPresetDto {
            name: "Steel".into(),
            cutting: cam.tools[0].cutting,
        },
    ];
    cam
}
fn change(draft: &mut Draft, cam: &CamDocumentDto, path: &str, value: &str) {
    set(draft, path, value);
    profiles::changed_tool(draft, cam.units, path).unwrap();
}
#[test]
fn native_cam_preset_navigation_and_scalar_edits_preserve_unedited_exact_data() {
    let cam = cam();
    let mut draft = Draft::new(&cam, Selection::Tool(5)).unwrap();
    change(&mut draft, &cam, SECTION, "presets");
    change(&mut draft, &cam, CURRENT, "1");
    change(&mut draft, &cam, CURRENT, "0");
    assert!(!draft.dirty(), "Browsing cutting profiles is not an edit");
    assert_eq!(draft.edited(&cam).unwrap(), cam);
    change(
        &mut draft,
        &cam,
        "/native/presets/0/name",
        "Aluminium finish",
    );
    let mut expected = cam.clone();
    expected.tools[0].cutting_presets[0].name = "Aluminium finish".into();
    assert_eq!(draft.edited(&cam).unwrap(), expected);
    change(&mut draft, &cam, "/native/presets/0/cutting/feed_z", "8");
    expected.tools[0].cutting_presets[0].cutting.feed_z = 203.2;
    let next = draft.edited(&cam).unwrap();
    assert_eq!(next, expected);
    assert_eq!(
        next.tools[0].cutting_presets[0].cutting.feed_xy.to_bits(),
        cam.tools[0].cutting_presets[0].cutting.feed_xy.to_bits()
    );
    assert_eq!(
        next.setups, cam.setups,
        "Tool profiles never rewrite operation cutting data"
    );
}
#[test]
fn native_cam_presets_add_duplicate_remove_and_retain_named_selection() {
    let cam = cam();
    let mut draft = Draft::new(&cam, Selection::Tool(5)).unwrap();
    change(&mut draft, &cam, SECTION, "presets");
    profiles::edit_tool(&mut draft, cam.units, profiles::Command::Add).unwrap();
    let next = draft.edited(&cam).unwrap();
    assert_eq!(next.tools[0].cutting_presets.len(), 3);
    assert_eq!(
        next.tools[0].cutting_presets[2].cutting, cam.tools[0].cutting,
        "Adding from untouched defaults preserves their canonical precision"
    );
    change(&mut draft, &cam, CURRENT, "0");
    profiles::edit_tool(&mut draft, cam.units, profiles::Command::Duplicate).unwrap();
    let next = draft.edited(&cam).unwrap();
    assert_eq!(next.tools[0].cutting_presets[3].name, "Aluminium copy");
    assert_eq!(
        next.tools[0].cutting_presets[3].cutting,
        cam.tools[0].cutting_presets[0].cutting
    );
    change(&mut draft, &cam, CURRENT, "0");
    profiles::edit_tool(&mut draft, cam.units, profiles::Command::Remove).unwrap();
    let next = draft.edited(&cam).unwrap();
    assert_eq!(
        next.tools[0]
            .cutting_presets
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Steel", "Preset", "Aluminium copy"]
    );
    assert_eq!(next.setups, cam.setups);
    let mut refreshed = Draft::new(&next, Selection::Tool(5)).unwrap();
    profiles::retain(Some(&draft), &mut refreshed, next.units);
    assert_eq!(form::text(&refreshed, SECTION).unwrap(), "presets");
    assert_eq!(
        form::text(&refreshed, "/native/presets/0/name").unwrap(),
        "Steel"
    );
    assert!(!refreshed.dirty());
}
#[test]
fn native_cam_invalid_profile_drafts_never_mutate_the_shared_document() {
    let cam = cam();
    for (path, value) in [
        ("/native/presets/0/name", "Steel"),
        ("/native/presets/0/name", ""),
        ("/native/presets/0/cutting/spindle_rpm", "0"),
        ("/native/presets/0/cutting/feed_xy", "NaN"),
        ("/native/presets/0/cutting/feed_z", "-1"),
    ] {
        let mut draft = Draft::new(&cam, Selection::Tool(5)).unwrap();
        change(&mut draft, &cam, path, value);
        assert!(draft.edited(&cam).is_err(), "{path} {value}");
        assert_eq!(cam, self::cam());
    }
}
#[test]
fn native_cam_operation_profile_copy_is_explicit_exact_and_keeps_programmed_steps() {
    let cam = cam();
    let mut draft = Draft::new(&cam, Selection::Operation(7)).unwrap();
    operation_editor::extend(&mut draft, &cam, &Default::default(), &[]).unwrap();
    assert!(!draft.dirty());
    assert_eq!(draft.edited(&cam).unwrap(), cam);
    set(&mut draft, OP, "0");
    profiles::changed_operation(&mut draft, &cam, OP).unwrap();
    let mut expected = cam.clone();
    let mut record = serde_json::to_value(&expected.setups[0].operations[0]).unwrap();
    record["cutting"] = serde_json::to_value(cam.tools[0].cutting_presets[0].cutting).unwrap();
    expected.setups[0].operations[0] = serde_json::from_value(record).unwrap();
    let next = draft.edited(&cam).unwrap();
    assert_eq!(next, expected);
    assert_eq!(next.tools, cam.tools);
    assert_eq!(next.height_expressions, cam.height_expressions);
    assert_eq!(next.linking, cam.linking);
    set(&mut draft, "/cutting/feed_z", "9");
    let edited = draft.edited(&cam).unwrap();
    let record = serde_json::to_value(&edited.setups[0].operations[0]).unwrap();
    assert_eq!(record["cutting"]["feed_z"], 228.6);
    assert_eq!(
        record["cutting"]["feed_xy"],
        cam.tools[0].cutting_presets[0].cutting.feed_xy
    );
}

#[test]
fn native_cam_copied_profile_precision_survives_keep_and_tool_changes() {
    let mut cam = cam();
    cam.tools[0].cutting_presets[0].cutting.feed_xy = 0.1;
    let copied = cam.tools[0].cutting_presets[0].cutting;
    let mut another = cam.tools[0].clone();
    another.id = 6;
    another.name = "Another cutter".into();
    another.number = Some(6);
    another.cutting_presets.clear();
    cam.tools.push(another);
    cam.next_tool_id = 7;
    for (path, value) in [(OP, "keep"), ("/tool_id", "6")] {
        let mut draft = Draft::new(&cam, Selection::Operation(7)).unwrap();
        operation_editor::extend(&mut draft, &cam, &Default::default(), &[]).unwrap();
        set(&mut draft, OP, "0");
        profiles::changed_operation(&mut draft, &cam, OP).unwrap();
        set(&mut draft, path, value);
        profiles::changed_operation(&mut draft, &cam, path).unwrap();
        let next = draft.edited(&cam).unwrap();
        let record = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
        let actual: CuttingParametersDto =
            serde_json::from_value(record["cutting"].clone()).unwrap();
        assert_eq!(
            actual, copied,
            "Changing {path} preserves the copied snapshot"
        );
        assert_eq!(actual.feed_xy.to_bits(), 0.1_f64.to_bits());
        set(&mut draft, "/cutting/feed_z", "9");
        let next = draft.edited(&cam).unwrap();
        let record = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
        assert_eq!(record["cutting"]["feed_z"], 228.6);
        assert_eq!(
            record["cutting"]["feed_xy"].as_f64().unwrap().to_bits(),
            0.1_f64.to_bits()
        );
    }
}

#[test]
fn native_cam_new_tool_can_define_presets_from_explicit_valid_cutting_defaults() {
    let cam = CamDocumentDto::default();
    let context = creation::Context::new(&Default::default(), &cam).unwrap();
    let mut draft = creation::draft(Tab::Tools, &cam, context);
    assert!(profiles::edit_tool(&mut draft, cam.units, profiles::Command::Add).is_err());
    for (path, value) in [
        ("/name", "Finishing cutter"),
        ("/diameter", "6"),
        ("/flute_length", "20"),
        ("/overall_length", "50"),
        ("/spindle_rpm", "14000"),
        ("/feed_xy", "700"),
        ("/feed_z", "100"),
    ] {
        set(&mut draft, path, value);
    }
    profiles::edit_tool(&mut draft, cam.units, profiles::Command::Add).unwrap();
    change(&mut draft, &cam, "/native/presets/0/name", "Finish");
    change(&mut draft, &cam, "/native/presets/0/cutting/feed_xy", "500");
    let (next, _) = creation::create(&draft, &cam).unwrap();
    let tool = &next.tools[0];
    assert_eq!(tool.cutting_presets.len(), 1);
    assert_eq!(tool.cutting_presets[0].name, "Finish");
    assert_eq!(tool.cutting_presets[0].cutting.feed_xy, 500.);
    assert_eq!(tool.cutting.feed_xy, 700.);
    assert_eq!(tool.cutting_presets[0].cutting.spindle_rpm, 14000);
    assert!(cam.tools.is_empty());
}
