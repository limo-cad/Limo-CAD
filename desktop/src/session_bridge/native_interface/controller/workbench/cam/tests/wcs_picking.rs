use super::*;
use limo_cad_cam::{Point3Dto, WcsOriginSpecDto};
use limo_cad_sketch::SketchDto;
use setup::picking::{self, Anchor, Key, Mode};

fn scene() -> limo_cad_solid::SolidSceneDto {
    serde_json::from_value(json!({"bodies":[
        {"id":11,"name":"Part","feature_id":1,"mesh":{"positions":[0.,0.,-6.,20.,10.,-1.],"indices":[],"normals":[]},"faces":[],"edges":[]},
        {"id":12,"name":"Other part","feature_id":2,"mesh":{"positions":[30.,-4.,-8.,40.,20.,2.],"indices":[],"normals":[]},"faces":[],"edges":[]}
    ],"errors":[]})).unwrap()
}
fn sketch() -> SketchDto {
    serde_json::from_value(
        json!({"name":"Datum:point","plane":{"type":"origin_plane","plane":"yz"},
        "basis":{"origin":[12.,3.,-7.],"u":[0.,1.,0.],"v":[0.,0.,1.],"normal":[1.,0.,0.]},
        "entities":[{"kind":"point","id":7,"position":{"x":4.,"y":5.},"fully_defined":true}],
        "constraints":[],"dimensions":[],"dimension_style":limo_cad_core::DimensionStyle::default(),
        "dof":{"value":0,"fully_defined":true},"can_undo":false,"can_redo":false}),
    )
    .unwrap()
}
fn cam() -> CamDocumentDto {
    let mut cam = job();
    cam.setups[0].operations.clear();
    cam.height_expressions.clear();
    cam.setups[0].body_ids = vec![limo_cad_core::BodyId(11)];
    cam
}
fn setup_draft(cam: &CamDocumentDto, sketches: &[SketchDto]) -> Draft {
    let mut draft = Draft::new(cam, Selection::Setup(3)).unwrap();
    setup::extend(&mut draft, cam, &scene(), sketches).unwrap();
    draft
}
fn values(draft: &Draft) -> Vec<(String, String, String)> {
    draft
        .fields
        .iter()
        .map(|field| {
            (
                field.path.clone(),
                field.text.clone(),
                field.original.clone(),
            )
        })
        .collect()
}
fn candidates(draft: &Draft, cam: &CamDocumentDto) -> Vec<picking::Candidate> {
    picking::candidates(picking::source(draft, cam, &picking::snapshot(draft).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn wcs_lattice_uses_unsaved_stock_and_model_bounds_and_stages_only_typed_anchors() {
    let cam = cam();
    let original = cam.clone();
    let mut draft = setup_draft(&cam, &[]);
    for (path, value) in [
        ("mode", "from_model"),
        ("body/12", "true"),
        ("offset/x_max", "3.5"),
        ("origin", "stock_box_point"),
        ("orientation", "down90"),
    ] {
        set(&mut draft, &format!("/native/setup/{path}"), value);
    }
    set(&mut draft, "/name", "unfinished name");
    set(&mut draft, "/native/setup/origin/x", "-");
    let state = picking::snapshot(&draft).unwrap();
    let picks = candidates(&draft, &cam);
    assert_eq!(picks.len(), 27);
    assert_eq!(picks[0].point, [-2., -6., -10.]);
    assert_eq!(picks[26].point, [43.5, 22., 3.]);
    let key = Key::Box {
        mode: Mode::Stock,
        axes: [Anchor::Center, Anchor::Max, Anchor::Min],
    };
    let position = picks.iter().find(|pick| pick.key == key).unwrap().point;
    let before = values(&draft);
    let record = draft.record.clone();
    picking::stage(&mut draft, &state, &key).unwrap();
    for ((path, text, baseline), (_, next, next_baseline)) in before.iter().zip(values(&draft)) {
        assert_eq!(baseline, &next_baseline);
        if !path.starts_with("/native/setup/anchor/") {
            assert_eq!(text, &next);
        }
    }
    assert_eq!(draft.record, record);
    assert_eq!(cam, original);
    let next = draft.edited(&cam).unwrap();
    assert_eq!(
        next.setups[0].wcs.origin,
        Point3Dto::new(position[0], position[1], position[2])
    );
    assert_eq!(next.setups[0].wcs.z_axis, [0., 0., -1.]);
    assert_eq!(next.tools, cam.tools);
    assert_eq!(next.linking, cam.linking);
    set(&mut draft, "/native/setup/origin", "model_box_point");
    set(&mut draft, "/native/setup/mode", "fixed");
    set(&mut draft, "/native/setup/size/x", "-");
    let picks = candidates(&draft, &cam);
    assert_eq!(picks[0].point, [0., -4., -8.]);
    assert_eq!(picks[26].point, [40., 20., 2.]);
}

#[test]
fn wcs_stock_handles_share_fixed_modeled_and_round_envelopes_with_apply() {
    let cam = cam();
    for mode in ["fixed", "model_body", "from_model"] {
        let mut draft = setup_draft(&cam, &[]);
        set(&mut draft, "/native/setup/origin", "stock_box_point");
        set(&mut draft, "/native/setup/mode", mode);
        match mode {
            "fixed" => {
                for (axis, size) in [("x", "40"), ("y", "30"), ("z", "12")] {
                    set(&mut draft, &format!("/native/setup/size/{axis}"), size);
                }
            }
            "model_body" => set(&mut draft, "/native/setup/stock_body", "12"),
            _ => set(&mut draft, "/native/setup/shape", "cylinder"),
        }
        let picks = candidates(&draft, &cam);
        let radius = 10_f64.hypot(5.) + 2.;
        let (min, max) = match mode {
            "fixed" => ([-10., -10., -6.], [30., 20., 6.]),
            "model_body" => ([30., -4., -8.], [40., 20., 2.]),
            _ => (
                [10. - radius, 5. - radius, -8.],
                [10. + radius, 5. + radius, 0.],
            ),
        };
        assert_eq!(picks[0].point, min, "{mode}");
        assert_eq!(picks[26].point, max, "{mode}");
        let expected = picking::snapshot(&draft).unwrap();
        picking::stage(&mut draft, &expected, &picks[26].key).unwrap();
        let next = draft.edited(&cam).unwrap();
        assert_eq!(
            next.setups[0].wcs.origin,
            Point3Dto::new(max[0], max[1], max[2])
        );
        let envelope = next.setups[0].stock_model_box.unwrap();
        assert_eq!(envelope.min, Point3Dto::new(min[0], min[1], min[2]));
        assert_eq!(envelope.max, Point3Dto::new(max[0], max[1], max[2]));
    }
}

#[test]
fn wcs_sketch_point_uses_its_real_basis_and_preserves_unfinished_stock_text() {
    let cam = cam();
    let mut draft = setup_draft(&cam, &[sketch()]);
    set(&mut draft, "/native/setup/origin", "sketch_point");
    set(&mut draft, "/native/setup/size/x", "-");
    let points = candidates(&draft, &cam);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].point, [12., 7., -2.]);
    let before = values(&draft);
    let state = picking::snapshot(&draft).unwrap();
    picking::stage(&mut draft, &state, &points[0].key).unwrap();
    for ((path, text, baseline), (_, next, next_baseline)) in before.iter().zip(values(&draft)) {
        assert_eq!(baseline, &next_baseline);
        if path != "/native/setup/point" {
            assert_eq!(text, &next);
        }
    }
    let next = draft.edited(&cam).unwrap();
    assert_eq!(next.setups[0].wcs.origin, Point3Dto::new(12., 7., -2.));
    assert_eq!(
        next.setups[0].wcs_origin,
        WcsOriginSpecDto::SketchPoint {
            sketch: "Datum:point".into(),
            entity_id: 7
        }
    );
}

#[test]
fn wcs_saved_missing_sketch_point_rejects_parameter_only_apply_until_explicit_repair() {
    let mut cam = cam();
    cam.setups[0].wcs_origin = WcsOriginSpecDto::SketchPoint {
        sketch: "Removed datum".into(),
        entity_id: 123,
    };
    let mut draft = setup_draft(&cam, &[sketch()]);
    set(&mut draft, "/name", "Name only");
    let field = draft
        .fields
        .iter()
        .find(|field| field.path == "/native/setup/point")
        .unwrap();
    assert!(field
        .options
        .as_ref()
        .unwrap()
        .iter()
        .any(|option| option.disabled && option.value == "Removed datum:123"));
    let before = values(&draft);
    assert!(draft.edited(&cam).unwrap_err().contains("unavailable"));
    assert_eq!(values(&draft), before);
    let state = picking::snapshot(&draft).unwrap();
    let point = candidates(&draft, &cam).remove(0);
    picking::stage(&mut draft, &state, &point.key).unwrap();
    assert!(draft.edited(&cam).is_ok());
    set(&mut draft, "/native/setup/origin", "explicit");
    assert!(draft.edited(&cam).is_ok());
    let mut draft = setup_draft(&cam, &[]);
    set(&mut draft, "/native/setup/mode", "rest_from_setup");
    assert!(!setup::visible(&draft, picking::BUTTON));
    assert!(picking::snapshot(&draft).is_err());
}

#[test]
fn wcs_saved_current_point_preserves_unchanged_metadata_and_refreshes_a_moved_basis() {
    let mut cam = cam();
    cam.setups[0].wcs_origin = WcsOriginSpecDto::SketchPoint {
        sketch: "Datum:point".into(),
        entity_id: 7,
    };
    cam.setups[0].wcs.origin = Point3Dto::new(12., 7., -2.);
    let mut draft = setup_draft(&cam, &[sketch()]);
    set(&mut draft, "/name", "Only the name");
    let mut expected = cam.clone();
    expected.setups[0].name = "Only the name".into();
    assert_eq!(draft.edited(&cam).unwrap(), expected);

    cam.setups[0].wcs.origin = Point3Dto::new(9., 7., -2.);
    let draft = setup_draft(&cam, &[sketch()]);
    let refreshed = draft.edited(&cam).unwrap();
    assert_eq!(refreshed.setups[0].wcs.origin, Point3Dto::new(12., 7., -2.));
    assert_eq!(refreshed.setups[0].wcs_origin, cam.setups[0].wcs_origin);
    assert_eq!(refreshed.tools, cam.tools);
}

#[test]
fn wcs_picker_preserves_exact_inch_baselines_and_rejects_stale_mode_budget_and_nonfinite_points() {
    let mut cam = cam();
    cam.units = CamUnits::Inches;
    cam.setups[0].stock.max.x = f64::from_bits(0x403e000000000001);
    cam.setups[0].stock_model_box = Some(cam.setups[0].stock);
    let mut draft = setup_draft(&cam, &[sketch()]);
    set(&mut draft, "/native/setup/origin", "stock_box_point");
    let before = values(&draft);
    let state = picking::snapshot(&draft).unwrap();
    let picks = candidates(&draft, &cam);
    assert_eq!(
        picks[26].point[0].to_bits(),
        cam.setups[0].stock.max.x.to_bits()
    );
    picking::stage(&mut draft, &state, &picks[26].key).unwrap();
    assert_eq!(
        draft.edited(&cam).unwrap().setups[0].stock_model_box,
        cam.setups[0].stock_model_box
    );
    assert!(values(&draft)
        .iter()
        .filter(|(path, _, _)| !path.starts_with("/native/setup/anchor/"))
        .eq(before
            .iter()
            .filter(|(path, _, _)| !path.starts_with("/native/setup/anchor/"))));
    let state = picking::snapshot(&draft).unwrap();
    set(&mut draft, "/native/setup/origin", "sketch_point");
    let before = values(&draft);
    assert!(picking::stage(&mut draft, &state, &picks[0].key).is_err());
    assert_eq!(values(&draft), before);
    set(&mut draft, "/native/setup/size/x", &"x".repeat(1024 * 1024));
    assert!(picking::snapshot(&draft).is_err());
    let mut many = sketch();
    many.entities = vec![many.entities[0].clone(); 4097];
    let mut oversized = setup_draft(&cam, &[many]);
    set(&mut oversized, "/native/setup/origin", "sketch_point");
    assert!(picking::snapshot(&oversized).is_err());
    assert!(
        picking::candidates(picking::Source::Sketch(vec![picking::Candidate {
            key: Key::Sketch {
                sketch: "bad".into(),
                entity_id: 1
            },
            point: [f64::NAN, 0., 0.]
        }]))
        .is_err()
    );
}
