use super::*;
use limo_cad_cam::{CamResolvedStockDto, CamToolKind, Point3Dto};
use limo_cad_solid::SolidSceneDto;

fn scene() -> SolidSceneDto {
    let bodies: Vec<_> = [(11, "Left part", [0.,0.,-6.], [20.,10.,-1.]), (12, "Right part", [30.,0.,-6.], [40.,20.,-1.])].into_iter().map(|(id,name,min,max)| {
        let mut positions=vec![];
        for x in [min[0],max[0]] { for y in [min[1],max[1]] { for z in [min[2],max[2]] { positions.extend([x,y,z]); } } }
        json!({"id":id,"name":name,"feature_id":id,"mesh":{"positions":positions,"normals":[],"indices":[]},"faces":[],"edges":[]})
    }).collect();
    serde_json::from_value(json!({"bodies":bodies,"errors":[]})).unwrap()
}
fn setup_draft(cam: &CamDocumentDto, id: u64) -> Draft {
    let mut draft = Draft::new(cam, Selection::Setup(id)).unwrap();
    setup::extend(&mut draft, cam, &scene(), &[]).unwrap();
    draft
}
fn geometry_cam() -> CamDocumentDto {
    let mut cam = job();
    cam.setups[0].body_ids = vec![limo_cad_core::BodyId(11)];
    cam
}

#[test]
fn native_cam_setup_stock_and_multibody_edits_preserve_unexposed_intent() {
    let cam = geometry_cam();
    let mut draft = setup_draft(&cam, 3);
    set(&mut draft, "/name", "Renamed setup");
    let next = draft.edited(&cam).unwrap();
    let mut expected = cam.clone();
    expected.setups[0].name = "Renamed setup".into();
    assert_eq!(
        next, expected,
        "Renaming must not re-resolve or round geometry"
    );

    let mut draft = setup_draft(&cam, 3);
    for (path, value) in [
        ("/native/setup/body/12", "true"),
        ("/native/setup/mode", "from_model"),
        ("/native/setup/origin", "stock_box_point"),
    ] {
        set(&mut draft, path, value);
    }
    let next = draft.edited(&cam).unwrap();
    let setup = &next.setups[0];
    assert_eq!(
        setup.body_ids,
        vec![limo_cad_core::BodyId(11), limo_cad_core::BodyId(12)]
    );
    assert_eq!(setup.wcs.origin, Point3Dto::new(-2., -2., 0.));
    assert_eq!(setup.stock.min, Point3Dto::new(0., 0., -8.));
    assert_eq!(setup.stock.max, Point3Dto::new(44., 24., 0.));
    assert_eq!(setup.operations, cam.setups[0].operations);
    assert_eq!(setup.machine, cam.setups[0].machine);
    assert_eq!(next.tools, cam.tools);
    assert_eq!(next.height_expressions, cam.height_expressions);
    assert_eq!(next.linking, cam.linking);
    assert_eq!(next.post_defaults, cam.post_defaults);
    assert!(setup::preview(&draft, &cam)
        .unwrap()
        .contains("44.00 × 24.00 × 8.00"));
    set(&mut draft, "/native/setup/offset/z_max", "-1");
    assert!(draft.edited(&cam).is_err());
    set(&mut draft, "/native/setup/offset/z_max", "1");
    set(&mut draft, "/native/setup/body/11", "false");
    set(&mut draft, "/native/setup/body/12", "false");
    assert!(
        draft.edited(&cam).is_err(),
        "An empty body selection is not an inferred all-bodies selection"
    );
}

#[test]
fn native_cam_setup_round_stock_rotation_and_rest_inherit_existing_frame() {
    let mut cam = geometry_cam();
    cam.setups[0].operations.clear();
    cam.height_expressions.clear();
    let mut draft = setup_draft(&cam, 3);
    set(&mut draft, "/native/setup/mode", "from_model");
    set(&mut draft, "/native/setup/shape", "cylinder");
    let cylinder = draft.edited(&cam).unwrap();
    let CamResolvedStockDto::Cylinder { radius, .. } = cylinder.setups[0].resolved_stock else {
        panic!("Expected cylinder")
    };
    assert!((radius - (10_f64.hypot(5.) + 2.)).abs() < 1e-12);
    set(&mut draft, "/native/setup/shape", "hex");
    set(&mut draft, "/native/setup/orientation", "down90");
    let hex = draft.edited(&cam).unwrap();
    let setup = &hex.setups[0];
    assert_eq!(setup.wcs.x_axis, [0., 1., 0.]);
    assert_eq!(setup.wcs.y_axis, [1., 0., 0.]);
    assert_eq!(setup.wcs.z_axis, [0., 0., -1.]);
    let CamResolvedStockDto::Hex { across_flats, .. } = setup.resolved_stock else {
        panic!("Expected hex")
    };
    assert!((across_flats - (5_f64.max(2.5 + 10. * 3_f64.sqrt() * 0.5) * 2. + 4.)).abs() < 1e-12);

    let mut rest = hex.clone();
    let mut second = rest.setups[0].clone();
    second.id = 4;
    second.name = "Second setup".into();
    second.wcs = Default::default();
    second.stock_spec = limo_cad_cam::CamStockSpecDto::LegacyBox;
    second.resolved_stock = CamResolvedStockDto::Box;
    rest.setups.push(second);
    rest.next_setup_id = 5;
    let mut draft = setup_draft(&rest, 4);
    set(&mut draft, "/native/setup/mode", "rest_from_setup");
    set(&mut draft, "/native/setup/source_setup", "3");
    let next = draft.edited(&rest).unwrap();
    assert_eq!(next.setups[1].wcs, next.setups[0].wcs);
    assert_eq!(next.setups[1].stock, next.setups[0].stock);
    assert_eq!(next.setups[1].wcs_origin, next.setups[0].wcs_origin);
    assert_eq!(
        next.setups[1].resolved_stock,
        CamResolvedStockDto::Rest { source_setup_id: 3 }
    );
    set(&mut draft, "/native/setup/source_setup", "4");
    assert!(draft.edited(&rest).is_err());
}

#[test]
fn native_cam_setup_geometry_changes_fail_closed_when_existing_cut_no_longer_fits() {
    let cam = geometry_cam();
    let mut draft = setup_draft(&cam, 3);
    set(&mut draft, "/native/setup/box/max/x", "10");
    assert!(
        draft.edited(&cam).is_err(),
        "Changing stock must not silently rewrite an existing face path"
    );
    set(&mut draft, "/native/setup/box/max/x", "-1");
    assert!(
        draft.edited(&cam).is_err(),
        "An inverted box must not be silently reordered"
    );
    set(&mut draft, "/native/setup/box/max/x", "1e309");
    assert!(draft.edited(&cam).is_err());
}

#[test]
fn native_cam_cutter_types_corners_and_defaults_use_shared_validation() {
    let mut cam = job();
    cam.tools[0].cutting_presets =
        serde_json::from_value(json!([{"name":"Aluminium","cutting":cam.tools[0].cutting}]))
            .unwrap();
    cam.tools[0].overall_length = f64::from_bits(0x4049800000000001);
    let original_operation = cam.setups[0].operations.clone();
    let mut draft = Draft::new(&cam, Selection::Tool(5)).unwrap();
    set(&mut draft, "/kind", "bull_nose_end_mill");
    tool::changed(&mut draft, "/kind");
    assert!(
        draft.edited(&cam).is_err(),
        "A bull-nose cutter requires its radius"
    );
    set(&mut draft, "/native/corner/radius", "0.5");
    set(&mut draft, "/default_step_down", "1.5");
    set(&mut draft, "/cutting/feed_xy", "1200");
    let next = draft.edited(&cam).unwrap();
    assert_eq!(next.tools[0].kind, CamToolKind::BullNoseEndMill);
    assert_eq!(next.tools[0].corner_radius, Some(0.5));
    assert_eq!(next.tools[0].default_step_down, Some(1.5));
    assert_eq!(
        next.tools[0].overall_length.to_bits(),
        cam.tools[0].overall_length.to_bits()
    );
    assert_eq!(next.tools[0].cutting_presets, cam.tools[0].cutting_presets);
    assert_eq!(
        next.setups[0].operations, original_operation,
        "Tool defaults never rewrite operation cutting data"
    );
    set(&mut draft, "/native/corner/radius", "4");
    assert!(draft.edited(&cam).is_err());
    set(&mut draft, "/kind", "flat_end_mill");
    tool::changed(&mut draft, "/kind");
    set(&mut draft, "/native/corner/shape", "chamfer");
    set(&mut draft, "/native/corner/width", "0.5");
    let next = draft.edited(&cam).unwrap();
    assert_eq!(next.tools[0].corner_radius, None);
    assert_eq!(next.tools[0].corner_chamfer.as_ref().unwrap().width, 0.5);
    set(&mut draft, "/kind", "drill");
    tool::changed(&mut draft, "/kind");
    let next = draft.edited(&cam).unwrap();
    assert_eq!(next.tools[0].point_angle_degrees, Some(118.));
    assert!(!next.tools[0].center_cutting);
    assert!(next.tools[0].corner_chamfer.is_none());
    assert_eq!(next.setups[0].operations, original_operation);
    set(&mut draft, "/point_angle_degrees", "180");
    assert!(draft.edited(&cam).is_err());
    let options = draft
        .fields
        .iter()
        .find(|f| f.path == "/kind")
        .unwrap()
        .options
        .as_ref()
        .unwrap();
    assert!(
        choose(
            options,
            "drill",
            &ControlInput::SetValue("turning_general".into())
        )
        .is_err(),
        "Reserved turning stays unavailable"
    );
}
