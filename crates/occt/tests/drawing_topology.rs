#![cfg(feature = "native-occt")]

use nbcad_assembly::AssemblyDocumentDto;
use nbcad_core::{BodyId, EdgeId, FeatureId};
use nbcad_occt::{
    drawing_export::{export_sheet, DrawingExportFormat, DrawingExportRequest},
    project_drawing, OcctKernel,
};
use nbcad_sketch::{
    drawing_topology::{capture_drawing_topology, validate_drawing_topology},
    DrawingDocumentDto, SketchManager,
};
use nbcad_solid::{
    BodyDto, EdgeDto, ExtrudeOperation, KernelBodyDto, KernelCurveDto, KernelExtrudeJobDto,
    KernelJobDto, KernelProfileDto, MeshDto, Point3Dto, RecomputePlanDto, SolidSceneDto,
};
use serde_json::json;

fn scene(raw: &KernelBodyDto, owner: u64) -> SolidSceneDto {
    SolidSceneDto {
        bodies: vec![BodyDto {
            id: raw.body_id,
            topology_signature: raw.topology_signature.clone(),
            name: "Native box".into(),
            feature_id: FeatureId(owner),
            mesh: MeshDto {
                positions: raw.positions.clone(),
                normals: raw.normals.clone(),
                indices: raw.indices.clone(),
            },
            faces: vec![],
            edges: raw
                .edges
                .iter()
                .enumerate()
                .map(|(i, e)| EdgeDto {
                    id: EdgeId(i as u64 + 1),
                    key: e.key.clone(),
                    points: e.points.clone(),
                    circle: e.circle,
                    refinable: e.refinable,
                })
                .collect(),
        }],
        errors: vec![],
    }
}

fn svg(
    doc: &DrawingDocumentDto,
    scene: &SolidSceneDto,
    kernel: &OcctKernel,
) -> Result<String, String> {
    let assembly = AssemblyDocumentDto::default();
    export_sheet(
        doc,
        scene,
        &assembly,
        &DrawingExportRequest {
            sheet_id: 1,
            format: DrawingExportFormat::Svg,
        },
        |request| project_drawing(kernel, scene, &assembly, request).map_err(|e| e.to_string()),
    )
}

#[test]
fn native_dimension_edit_stays_associated_but_boolean_ordinal_alias_rejects() {
    let p = |x, y| Point3Dto { x, y, z: 0. };
    let normal = Point3Dto {
        x: 0.,
        y: 0.,
        z: 1.,
    };
    let stock = KernelExtrudeJobDto {
        feature_id: FeatureId(1),
        operation: ExtrudeOperation::NewBody,
        source_face: None,
        profiles: vec![KernelProfileDto {
            profile_index: 0,
            points: vec![p(0., 0.), p(10., 0.), p(10., 10.), p(0., 10.)],
            curves: vec![],
            holes: vec![],
        }],
        normal,
        start_offset: 0.,
        end_offset: 10.,
        taper_angle_deg: 0.,
        target_body_ids: vec![],
        result_body_ids: vec![BodyId(1)],
    };
    let mut plan = RecomputePlanDto {
        transaction_id: 1,
        jobs: vec![KernelJobDto::Extrude(stock.clone())],
        errors: vec![],
    };
    let mut kernel = OcctKernel::new().unwrap();
    let initial = kernel.recompute(&plan).unwrap();
    let initial_scene = scene(&initial.bodies[0], 1);
    let edge = &initial_scene.bodies[0].edges[4];
    assert_eq!(edge.points.first().unwrap().x, 10.);
    assert_eq!(edge.points.first().unwrap().z, 0.);
    assert_eq!(edge.points.last().unwrap().z, 10.);
    let reference = |endpoint| json!({"body_id":1,"edge_id":edge.id,"edge_key":edge.key,"endpoint":endpoint,"fallback_point":[999.,999.,999.]});
    let mut manager = SketchManager::new();
    let mut doc=manager.drawing_command(serde_json::from_value(json!({"type":"create_sheet","arguments":{"name":"Native dimensions","format":"a4","orientation":"landscape"}})).unwrap()).unwrap();
    doc.sheets[0].views.push(serde_json::from_value(json!({"id":1,"name":"Front","kind":"front","direction":[0.,-1.,0.],"up":[0.,0.,1.],"position":[90.,65.],"scale":2.,"body_ids":[1]})).unwrap());
    doc.next_view_id = 2;
    doc.sheets[0].annotations.push(serde_json::from_value(json!({"kind":"linear_dimension","id":1,"view_id":1,"first":reference("start"),"second":reference("end"),"mode":"vertical","offset":10.})).unwrap());
    doc.next_annotation_id = 2;
    let legacy = doc.clone();
    let still_legacy =
        capture_drawing_topology(doc.clone(), &initial_scene, Some(&legacy)).unwrap();
    assert!(
        validate_drawing_topology(&still_legacy, &initial_scene).is_err(),
        "unrelated drawing edits cannot bless legacy ordinal references"
    );
    let doc = capture_drawing_topology(doc, &initial_scene, None).unwrap();
    assert!(svg(&doc, &initial_scene, &kernel)
        .unwrap()
        .contains("10.00"));

    if let KernelJobDto::Extrude(job) = &mut plan.jobs[0] {
        job.end_offset = 20.;
    }
    let resized = kernel.recompute(&plan).unwrap();
    assert_eq!(
        initial.bodies[0].topology_signature,
        resized.bodies[0].topology_signature
    );
    let resized_scene = scene(&resized.bodies[0], 1);
    let resized_svg = svg(&doc, &resized_scene, &kernel).unwrap();
    assert!(
        resized_svg.contains("20.00"),
        "native height edit must update the measured dimension"
    );
    let saved: DrawingDocumentDto =
        serde_json::from_value(serde_json::to_value(&doc).unwrap()).unwrap();
    assert_eq!(svg(&saved, &resized_scene, &kernel).unwrap(), resized_svg);

    let mut cut = stock;
    cut.feature_id = FeatureId(2);
    cut.operation = ExtrudeOperation::Cut;
    cut.end_offset = 20.;
    cut.target_body_ids = vec![BodyId(1)];
    cut.result_body_ids = vec![];
    cut.profiles = vec![KernelProfileDto {
        profile_index: 0,
        points: (0..64)
            .map(|i| {
                let angle = std::f64::consts::TAU * i as f64 / 64.;
                p(5. + 2. * angle.cos(), 5. + 2. * angle.sin())
            })
            .collect(),
        curves: vec![KernelCurveDto::Circle {
            entity_id: 1,
            center: p(5., 5.),
            axis_point: p(7., 5.),
            normal,
        }],
        holes: vec![],
    }];
    plan.jobs.push(KernelJobDto::Extrude(cut));
    let bored = kernel.recompute(&plan).unwrap();
    assert!(bored.errors.is_empty());
    let bored_scene = scene(&bored.bodies[0], 2);
    let alias = &bored_scene.bodies[0].edges[4];
    assert_eq!((alias.id, alias.key.as_str()), (edge.id, edge.key.as_str()));
    assert_eq!(
        alias.points.first().unwrap().x,
        0.,
        "native Boolean reuses the ordinal for another edge"
    );
    assert_ne!(
        bored.bodies[0].topology_signature,
        resized.bodies[0].topology_signature
    );
    assert!(svg(&doc, &bored_scene, &kernel)
        .unwrap_err()
        .contains("topology changed"));
    let mut unaffected_sheet = doc.clone();
    let mut stale_sheet = unaffected_sheet.sheets[0].clone();
    stale_sheet.id = 2;
    stale_sheet.views[0].id = 2;
    let mut stale_json = serde_json::to_value(&stale_sheet.annotations[0]).unwrap();
    stale_json["view_id"] = json!(2);
    stale_sheet.annotations[0] = serde_json::from_value(stale_json).unwrap();
    unaffected_sheet.sheets[0].annotations.clear();
    unaffected_sheet.sheets.push(stale_sheet);
    unaffected_sheet.next_sheet_id = 3;
    unaffected_sheet.next_view_id = 3;
    let unrelated_export = svg(&unaffected_sheet, &bored_scene, &kernel);
    assert!(
        unrelated_export.is_ok(),
        "an unrelated sheet must remain exportable while another sheet needs reassociation: {unrelated_export:?}"
    );
    assert!(
        validate_drawing_topology(
            &capture_drawing_topology(doc.clone(), &bored_scene, Some(&doc)).unwrap(),
            &bored_scene
        )
        .is_err(),
        "editing drawing content cannot silently replace existing association guards"
    );
    let mut changed_owner = resized_scene.clone();
    changed_owner.bodies[0].feature_id = FeatureId(2);
    assert!(
        validate_drawing_topology(&doc, &changed_owner).is_err(),
        "new owning feature is also an intent change"
    );

    plan.jobs.pop();
    let restored = kernel.recompute(&plan).unwrap();
    assert_eq!(
        svg(&doc, &scene(&restored.bodies[0], 1), &kernel).unwrap(),
        resized_svg
    );
}
