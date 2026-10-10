use super::*;
use operation_geometry::hole_picking::{self as picker, FaceKey};

const COUNT: &str = "/native/geometry/hole_count";
const CURRENT: &str = "/native/ui/geometry_hole";
const SECTION: &str = "/native/ui/operation_section";
const KEY: FaceKey = FaceKey {
    body_id: 11,
    face_id: 1,
};

fn hole(reference: Option<&str>) -> Value {
    json!({"point":{"x":5.,"y":5.},"top_z":-1.,"bottom_z":-6.,
        "axis":[0.,0.,1.],"face_key":reference})
}
fn with_holes(kind: &str, holes: Vec<Value>) -> CamDocumentDto {
    let mut cam = geometry::cam(kind);
    let mut record = serde_json::to_value(&cam.setups[0].operations[0]).unwrap();
    record["holes"] = json!(holes);
    cam.setups[0].operations[0] = serde_json::from_value(record).unwrap();
    cam
}
fn open(cam: &CamDocumentDto, scene: &limo_cad_solid::SolidSceneDto) -> Draft {
    let mut draft = Draft::new(cam, Selection::Operation(7)).unwrap();
    operation_editor::extend(&mut draft, cam, scene, &[]).unwrap();
    set(&mut draft, SECTION, "geometry");
    draft
}
fn fields(draft: &Draft) -> Vec<(String, String, String)> {
    draft
        .fields
        .iter()
        .map(|f| (f.path.clone(), f.original.clone(), f.text.clone()))
        .collect()
}
fn apply_record(draft: &Draft, cam: &CamDocumentDto) -> Value {
    serde_json::to_value(&draft.edited(cam).unwrap().setups[0].operations[0]).unwrap()
}
fn toggle(draft: &mut Draft, cam: &CamDocumentDto) {
    let before = picker::snapshot(draft).unwrap();
    picker::stage(draft, cam, &before, KEY).unwrap();
}

#[test]
fn native_cam_hole_picks_toggle_only_the_draft_and_preserve_shared_records() {
    for kind in ["drill", "thread"] {
        let cam = geometry::cam(kind);
        let saved = serde_json::to_value(&cam).unwrap();
        let mut draft = open(&cam, &geometry::scene());
        let original = draft.record.clone();
        let count = draft.fields.len();
        toggle(&mut draft, &cam);
        assert_eq!(picker::snapshot(&draft).unwrap().keys, vec![KEY]);
        assert_eq!(
            draft.record, original,
            "physical selection never replaces the saved DTO"
        );
        assert_eq!(serde_json::to_value(&cam).unwrap(), saved);
        assert!(draft.dirty());
        assert_eq!(
            apply_record(&draft, &cam)["holes"],
            json!([hole(Some("11:1"))])
        );
        let next = draft.edited(&cam).unwrap();
        assert_eq!(next.tools, cam.tools);
        assert_eq!(next.setups[0].wcs, cam.setups[0].wcs);
        assert_eq!(next.toolpath_generations, cam.toolpath_generations);
        assert_eq!(apply_record(&draft, &cam)["points"], original["points"]);
        assert!(draft.fields.len() < count + 20);
        toggle(&mut draft, &cam);
        assert!(picker::snapshot(&draft).unwrap().keys.is_empty());
        assert_eq!(apply_record(&draft, &cam), original);
        toggle(&mut draft, &cam);
        assert_eq!(
            apply_record(&draft, &cam)["holes"],
            json!([hole(Some("11:1"))])
        );
    }
}

#[test]
fn native_cam_hole_removal_moves_raw_text_and_lazy_exact_inch_baselines_together() {
    let mut manual = hole(None);
    manual["point"]["x"] = json!(f64::from_bits(0x4014000000000001));
    manual["axis"] = json!([0.0001, 0., -0.9999999949999999]);
    let mut cam = with_holes(
        "drill",
        vec![hole(Some("11:1")), manual.clone(), manual.clone()],
    );
    cam.units = CamUnits::Inches;
    let mut draft = open(&cam, &geometry::scene());
    geometry::edit(&mut draft, &cam, CURRENT, "2");
    let original_x = draft
        .fields
        .iter()
        .find(|f| f.path == "/native/geometry/holes/1/x")
        .unwrap()
        .original
        .clone();
    set(&mut draft, "/native/geometry/holes/1/x", "  -  ");
    set(&mut draft, "/native/geometry/centers/0/x", "0.2");
    let original = draft.record.clone();
    toggle(&mut draft, &cam);
    assert_eq!(form::text(&draft, COUNT).unwrap(), "2");
    let shifted = draft
        .fields
        .iter()
        .find(|f| f.path == "/native/geometry/holes/0/x")
        .unwrap();
    assert_eq!(shifted.text, "  -  ");
    assert_eq!(shifted.original, original_x);
    assert!(
        !draft
            .fields
            .iter()
            .any(|f| f.path.starts_with("/native/geometry/holes/1/")),
        "unvisited row stays lazy"
    );
    assert!(
        draft.edited(&cam).is_err(),
        "unfinished manual text remains unfinished"
    );
    assert_eq!(draft.record, original);
    set(&mut draft, "/native/geometry/holes/0/x", &original_x);
    let edited = apply_record(&draft, &cam);
    assert_eq!(edited["holes"], json!([manual.clone(), manual]));
    assert_eq!(edited["points"][0]["x"], 5.08);
}

#[test]
fn native_cam_parameter_only_apply_revalidates_associated_hole_geometry() {
    for kind in ["drill", "thread"] {
        for failure in ["missing", "not-cylinder", "tilted", "degenerate", "scope"] {
            let mut cam = with_holes(kind, vec![hole(Some("11:1"))]);
            let mut scene = geometry::scene();
            match failure {
                "missing" => scene.bodies[0].faces.clear(),
                "not-cylinder" => scene.bodies[0].faces[0].cylinder = None,
                "tilted" => scene.bodies[0].faces[0].cylinder.as_mut().unwrap().axis.x = 1.,
                "degenerate" => scene.bodies[0].faces[0].index_count = 0,
                "scope" => cam.setups[0].body_ids.clear(),
                _ => unreachable!(),
            }
            let mut draft = open(&cam, &scene);
            set(&mut draft, "/cutting/feed_xy", "850");
            let before = fields(&draft);
            assert!(
                draft.edited(&cam).is_err(),
                "{kind}: {failure} must reject parameter-only Apply"
            );
            assert_eq!(fields(&draft), before);
            geometry::edit(&mut draft, &cam, COUNT, "0");
            let repaired = apply_record(&draft, &cam);
            let mut expected =
                serde_json::to_value(&geometry::cam(kind).setups[0].operations[0]).unwrap();
            expected["cutting"]["feed_xy"] = json!(850.);
            assert_eq!(repaired, expected);
        }
    }
}

#[test]
fn native_cam_hole_apply_refreshes_face_span_and_preserves_exact_manual_values() {
    let mut stale = hole(Some("11:1"));
    stale["point"]["x"] = json!(7.);
    stale["bottom_z"] = json!(-3.);
    let mut manual = hole(None);
    manual["axis"] = json!([0.0001, 0., -0.9999999949999999]);
    manual["bottom_z"] = json!(f64::from_bits((-5_f64).to_bits() + 1));
    let cam = with_holes("drill", vec![stale, manual.clone()]);
    let mut draft = open(&cam, &geometry::scene());
    set(&mut draft, "/cutting/feed_xy", "850");
    assert_eq!(
        apply_record(&draft, &cam)["holes"],
        json!([hole(Some("11:1")), manual])
    );
}

#[test]
fn native_cam_hole_picker_rejects_stale_form_selection_and_scene_without_mutation() {
    let cam = geometry::cam("drill");
    for change in ["section", "count", "selection", "scene"] {
        let mut draft = open(&cam, &geometry::scene());
        let expected = picker::snapshot(&draft).unwrap();
        match change {
            "section" => set(&mut draft, SECTION, "parameters"),
            "count" => geometry::edit(&mut draft, &cam, COUNT, "1"),
            "selection" => draft.selection = Selection::Operation(99),
            "scene" => {
                let replacement = open(&cam, &geometry::scene());
                draft = replacement;
            }
            _ => unreachable!(),
        }
        let before = fields(&draft);
        let record = draft.record.clone();
        assert!(!picker::unchanged(&draft, &expected).unwrap_or(false));
        assert!(
            picker::stage(&mut draft, &cam, &expected, KEY).is_err(),
            "{change}"
        );
        assert_eq!(fields(&draft), before);
        assert_eq!(draft.record, record);
    }
}

#[test]
fn native_cam_hole_picker_budgets_reject_atomically_without_truncating_large_forms() {
    let cam = with_holes("drill", vec![hole(None); 4096]);
    let mut draft = open(&cam, &geometry::scene());
    let expected = picker::snapshot(&draft).unwrap();
    let before = fields(&draft);
    assert!(picker::stage(&mut draft, &cam, &expected, KEY)
        .unwrap_err()
        .contains("4096"));
    assert_eq!(fields(&draft), before);
    assert_eq!(draft.record["holes"].as_array().unwrap().len(), 4096);
    geometry::edit(&mut draft, &cam, COUNT, "4097");
    assert!(picker::snapshot(&draft).unwrap_err().contains("4096"));

    let cam = with_holes("drill", vec![hole(None)]);
    let mut draft = open(&cam, &geometry::scene());
    let bytes: usize = draft
        .fields
        .iter()
        .filter(|f| f.path.starts_with("/native/geometry/"))
        .map(|f| f.path.len() + f.text.len())
        .sum();
    let old = form::text(&draft, "/native/geometry/holes/0/x")
        .unwrap()
        .len();
    set(
        &mut draft,
        "/native/geometry/holes/0/x",
        &" ".repeat(1024 * 1024 - bytes + old - 1500),
    );
    let expected = picker::snapshot(&draft).unwrap();
    let before = fields(&draft);
    assert!(picker::stage(&mut draft, &cam, &expected, KEY)
        .unwrap_err()
        .contains("budget"));
    assert_eq!(fields(&draft), before);
}

#[test]
fn native_cam_hole_rows_retain_only_the_active_face_catalog_and_exact_row_text() {
    let mut scene = geometry::scene();
    let face = scene.bodies[0].faces[0].clone();
    for id in 2..=64 {
        let mut next = face.clone();
        next.id.0 = id;
        next.key = format!("face-{id}");
        scene.bodies[0].faces.push(next);
    }
    let cam = with_holes("drill", vec![hole(None); 32]);
    let mut draft = open(&cam, &scene);
    for index in 1..=32 {
        geometry::edit(&mut draft, &cam, CURRENT, &index.to_string());
        let choices: usize = draft
            .fields
            .iter()
            .filter(|f| f.path.starts_with("/native/geometry/holes/") && f.path.ends_with("/face"))
            .map(|f| f.options.as_ref().map_or(0, Vec::len))
            .sum();
        assert_eq!(choices, 64, "row {index} duplicated the face catalog");
    }
    set(&mut draft, "/native/geometry/holes/31/x", "  incomplete  ");
    toggle(&mut draft, &cam);
    geometry::edit(&mut draft, &cam, CURRENT, "32");
    let field = draft
        .fields
        .iter()
        .find(|f| f.path == "/native/geometry/holes/31/x")
        .unwrap();
    assert_eq!(field.text, "  incomplete  ");
    geometry::edit(&mut draft, &cam, CURRENT, "33");
    assert_eq!(
        form::text(&draft, "/native/geometry/holes/32/face").unwrap(),
        "11:1"
    );
    let choices: usize = draft
        .fields
        .iter()
        .filter(|f| f.path.starts_with("/native/geometry/holes/") && f.path.ends_with("/face"))
        .map(|f| f.options.as_ref().map_or(0, Vec::len))
        .sum();
    assert_eq!(choices, 64);
}
