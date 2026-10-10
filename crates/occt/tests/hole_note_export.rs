//! Headless real-drilled-solid proof. Live native controls are covered by the
//! separate native-drawing-hole fixture; this test never opens a window.
#![cfg(feature = "native-occt")]
use limo_cad_core::{OriginPlane, PlaneRef, UnitSystem};
use limo_cad_occt::{
    drawing_export::{export_sheet_with_units, DrawingExportFormat, DrawingExportRequest},
    project_drawing, OcctKernel,
};
use limo_cad_sketch::{DrawingAnnotationDto, DrawingDocumentDto, SketchManager};
use limo_cad_solid::{CommitKernelRequest, RecomputePlanDto};
use serde_json::json;

fn apply(manager: &mut SketchManager, kernel: &mut OcctKernel, plan: RecomputePlanDto) {
    assert!(plan.errors.is_empty(), "{:?}", plan.errors);
    let scene = kernel.recompute(&plan).unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    manager
        .commit_solid(CommitKernelRequest {
            transaction_id: plan.transaction_id,
            scene,
        })
        .unwrap();
}

#[test]
fn real_drilled_hole_metadata_labels_exports_and_archive_remain_exact() {
    for through in [false, true] {
        let mut manager = SketchManager::new();
        let mut kernel = OcctKernel::new().unwrap();
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();
        manager.add_rectangle(serde_json::from_value(json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":60.,"y":40.},"ctrl_held":true})).unwrap()).unwrap();
        manager.end_sketch().unwrap();
        let plan=manager.prepare_extrude(serde_json::from_value(json!({"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body",
            "extent":{"type":"distance","distance":10.},"taper_angle_deg":0.,"flip":false,"target_body_ids":[]})).unwrap()).unwrap();
        apply(&mut manager, &mut kernel, plan);
        let scene = manager.solid_scene();
        let body = &scene.bodies[0];
        let face = body
            .faces
            .iter()
            .find(|f| f.plane.is_some_and(|p| p.normal[2] > 0.99))
            .unwrap();
        let basis = face.plane.unwrap();
        let positions: Vec<_> = [20., 40.]
            .into_iter()
            .map(|x| {
                let delta = [
                    x - basis.origin[0],
                    20. - basis.origin[1],
                    10. - basis.origin[2],
                ];
                let dot = |axis: [f64; 3]| axis.iter().zip(delta).map(|(a, b)| a * b).sum::<f64>();
                json!({"position":{"x":dot(basis.u),"y":dot(basis.v)}})
            })
            .collect();
        let plan=manager.prepare_hole(serde_json::from_value(json!({"body_id":body.id,"face_id":face.id,
            "position":positions[0]["position"],"positions":positions,"diameter":6.,
            "extent":if through {json!({"type":"through_all"})} else {json!({"type":"distance","depth":8.})},
            "bottom_style":"flat","flip":false})).unwrap()).unwrap();
        apply(&mut manager, &mut kernel, plan);
        let scene = manager.solid_scene();
        assert!(scene.errors.is_empty());
        assert_eq!(scene.bodies.len(), 1);
        let posed = || limo_cad_occt::PlacedBodyQueryDto {
            body_id: scene.bodies[0].id,
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
        };
        let actual = kernel
            .exact_interference(posed(), posed())
            .unwrap()
            .overlap_volume_mm3;
        let depth = if through { 10. } else { 8. };
        assert!(
            (actual - (24_000. - 2. * std::f64::consts::PI * 9. * depth)).abs() < 1e-6,
            "The fixture must contain two physical drilled cavities: actual={actual}, depth={depth}, definitions={:?}",
            manager.hole_definitions()
        );
        let defs = manager.hole_definitions();
        assert_eq!(defs.len(), 1);
        let def = &defs[0];
        assert_eq!(def.positions.len(), 2);
        assert_eq!(def.diameter, 6.);
        assert!(def.face_basis.is_some());
        assert!(def.positions.iter().all(|p| p.position_reference.is_none()));
        let assembly = Default::default();
        let request = serde_json::from_value(
            json!({"direction":[0.,0.,1.],"up":[0.,1.,0.],"include_hidden":true}),
        )
        .unwrap();
        let projection = project_drawing(&kernel, &scene, &assembly, &request).unwrap();
        let circle = projection
            .circles
            .iter()
            .find(|c| {
                c.closed
                    && !c.hidden
                    && (c.center_model[0] - 20.).abs() < 1e-6
                    && (c.center_model[1] - 20.).abs() < 1e-6
                    && (c.center_model[2] - 10.).abs() < 1e-6
                    && (c.radius - 3.).abs() < 1e-6
            })
            .unwrap();
        let mut drawing=manager.drawing_command(serde_json::from_value(json!({"type":"create_sheet","arguments":{"name":"Real drilled holes","format":"a4","orientation":"landscape"}})).unwrap()).unwrap();
        drawing.sheets[0].views.push(serde_json::from_value(json!({"id":1,"name":"Top","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[130.,90.],"scale":2.})).unwrap());
        drawing.sheets[0].annotations.push(serde_json::from_value(json!({"kind":"hole_note","id":1,"view_id":1,
            "feature":{"body_id":circle.body_id,"edge_id":circle.edge_id,"edge_key":circle.edge_key,"occurrence_id":circle.occurrence_id,
                "topology_signature":projection.topology_signatures[&circle.body_id.0.to_string()],
                "fallback_center":[999.,999.,999.],"fallback_normal":[0.,0.,1.],"fallback_radius":999.,"closed":true},
            "position":[180.,125.],"quantity":2,"diameter":def.diameter,
            "depth":if through {None} else {Some(8.)},"through_all":through,
            "source_feature_id":def.feature_id,"feature_name":def.name,"note":"Deburr","pattern_note":"2 HOLES"})).unwrap());
        drawing.next_view_id = 2;
        drawing.next_annotation_id = 2;
        manager.set_drawing_document(drawing.clone()).unwrap();
        let before = manager.export_project_model().unwrap();
        let label = limo_cad_occt::drawing_presentation::text::hole(
            &drawing.sheets[0].annotations[0],
            UnitSystem::Mm,
            drawing.sheets[0].standard,
        );
        assert_eq!(
            label,
            if through {
                "2× ⌀6 THRU\nDeburr"
            } else {
                "2× ⌀6 ↧8\nDeburr"
            }
        );
        let export = |document: &DrawingDocumentDto, format| {
            export_sheet_with_units(
                document,
                &scene,
                &assembly,
                &DrawingExportRequest {
                    sheet_id: 1,
                    format,
                },
                UnitSystem::Mm,
                |r| project_drawing(&kernel, &scene, &assembly, r).map_err(|e| e.to_string()),
            )
        };
        for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
            let content = export(&drawing, format).unwrap();
            for line in label.lines() {
                assert!(content.contains(line), "{line}");
            }
            assert!(content.contains("LEADER"));
            assert!(!content.contains("999.00000"));
            let loaded: DrawingDocumentDto =
                serde_json::from_str(&serde_json::to_string(&drawing).unwrap()).unwrap();
            assert_eq!(export(&loaded, format).unwrap(), content);
            let mut stale = loaded;
            if let DrawingAnnotationDto::HoleNote { feature, .. } =
                &mut stale.sheets[0].annotations[0]
            {
                feature.topology_signature = Some("reused ordinal".into());
            }
            assert!(export(&stale, format).is_err());
        }
        assert_eq!(manager.export_project_model().unwrap(), before);
    }
}
