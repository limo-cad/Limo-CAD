use super::*;
use limo_cad_core::{BodyId, OriginPlane, PlaneRef};
use limo_cad_native_engine::teaching::{self, assembly::body_poses, Frame};
use limo_cad_sketch::{
    CreateOccurrenceRequestDto, ProjectVisibilityDto, RectangleMode, RectangleRequest,
    SetOccurrencePoseRequestDto, SketchManager, UpdateOccurrenceRequestDto, Vec2,
};
use limo_cad_solid::{
    CommitKernelRequest, ExtrudeExtent, ExtrudeOperation, ExtrudeRequest, KernelBodyDto,
    KernelJobDto, KernelSceneDto,
};

// Commit a tiny synthetic tessellation through the real pure manager API. This
// fixture needs no kernel execution; native baseline replay below recomputes
// the real extrusions from the archive's retained sketches and feature history.
fn add_stock(manager: &mut SketchManager, x: f64) -> BodyId {
    let mut bodies = manager
        .solid_scene_ref()
        .bodies
        .iter()
        .map(|body| {
            serde_json::from_value::<KernelBodyDto>(json!({
                "body_id":body.id,"positions":body.mesh.positions,"normals":body.mesh.normals,
                "indices":body.mesh.indices,"faces":[],"edges":body.edges
            }))
            .unwrap()
        })
        .collect::<Vec<_>>();
    let sketch = manager
        .begin_sketch(PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        })
        .unwrap();
    manager
        .add_rectangle(RectangleRequest {
            mode: RectangleMode::TwoPoint,
            p1: Vec2::new(x, 0.),
            p2: Vec2::new(x + 10., 10.),
            ctrl_held: false,
        })
        .unwrap();
    manager.end_sketch().unwrap();
    let request: ExtrudeRequest = serde_json::from_value(json!({
        "sketch_name":sketch.name,"profile_indices":[0],"operation":ExtrudeOperation::NewBody,
        "extent":ExtrudeExtent::Distance { distance: 2. },"taper_angle_deg":0.,"flip":false,
        "target_body_ids":[]
    }))
    .unwrap();
    let plan = manager.prepare_extrude(request).unwrap();
    let KernelJobDto::Extrude(job) = plan.jobs.last().unwrap() else {
        panic!("Expected an extrusion");
    };
    let id = job.result_body_ids[0];
    let raw: KernelBodyDto = serde_json::from_value(json!({
        "body_id":id,"positions":[x,0,0,x+10.,0,0,x,10,0],
        "normals":[0,0,1,0,0,1,0,0,1],"indices":[0,1,2],"faces":[],
        "edges":[{"key":"edge:0","points":[
            {"x":x,"y":0,"z":0},{"x":x+10.,"y":0,"z":0}
        ]}]
    }))
    .unwrap();
    bodies.push(raw);
    manager
        .commit_solid(CommitKernelRequest {
            transaction_id: plan.transaction_id,
            scene: KernelSceneDto {
                bodies,
                errors: Vec::new(),
            },
        })
        .unwrap();
    id
}

fn fixture() -> (SketchManager, BodyId, BodyId) {
    let mut manager = SketchManager::new();
    let visible = add_stock(&mut manager, 0.);
    let hidden = add_stock(&mut manager, 4000.);
    let document = manager.assembly_document();
    let component = document
        .component_structure
        .definitions
        .iter()
        .find(|d| d.body_ids == vec![visible])
        .unwrap();
    let original = document
        .component_structure
        .occurrences
        .iter()
        .find(|o| o.component_id == component.id)
        .unwrap();
    manager
        .set_occurrence_pose(SetOccurrencePoseRequestDto {
            occurrence_id: original.id,
            local_pose: serde_json::from_value(json!({
                "translation":[100,0,0],"rotation":[0,0,1,1]
            }))
            .unwrap(),
        })
        .unwrap();
    for (x, visible) in [(300., true), (9000., false)] {
        let mut occurrence = manager
            .create_occurrence(CreateOccurrenceRequestDto {
                component_id: component.id,
                name: format!("Copy {x}"),
                parent_occurrence_id: None,
                local_pose: serde_json::from_value(json!({
                    "translation":[x,0,0],"rotation":[0,0,0,1]
                }))
                .unwrap(),
            })
            .unwrap();
        occurrence.visible = visible;
        manager
            .update_occurrence(UpdateOccurrenceRequestDto {
                occurrence: occurrence.into(),
            })
            .unwrap();
    }
    manager
        .set_project_visibility(ProjectVisibilityDto {
            hidden_body_ids: vec![hidden.0],
            ..Default::default()
        })
        .unwrap();
    (manager, visible, hidden)
}

fn retained(frame: &super::super::ProjectPreviewFrame) -> Frame {
    Frame {
        scene: frame.scene.clone(),
        assembly: Some(frame.assembly.clone()),
        ..Default::default()
    }
}

#[test]
fn capture_combines_live_body_and_occurrence_visibility_for_placement_and_fitting() {
    let (manager, visible, hidden) = fixture();
    let before = manager.export_project_model().unwrap();
    // The raw assembly really does expose the body that the live Browser hides.
    assert_eq!(
        body_poses(Some(&manager.assembly_solution()), hidden.0).len(),
        1
    );
    let mut server = crate::CadServer::new().unwrap();
    server.manager = manager;
    let mut check = || Ok(());
    let mut host = DetachedHost {
        server: &mut server,
        archive: None,
        baseline_attempted: false,
        baseline_loaded: true,
        frames: Frames::default(),
        unrendered_domain_intents: Default::default(),
        check: &mut check,
    };
    host.capture(ProjectPreviewStage::Baseline).unwrap();
    host.capture(ProjectPreviewStage::Final).unwrap();
    assert_eq!(
        host.frames.values.len(),
        1,
        "Baseline-only replay should deduplicate its final capture"
    );
    let frame = &host.frames.values[0];
    assert_eq!(frame.scene.bodies.len(), 2, "Retain both body definitions");
    assert_eq!(frame.stage, ProjectPreviewStage::Baseline);
    assert!(body_poses(Some(&frame.assembly), hidden.0).is_empty());
    let poses = body_poses(Some(&frame.assembly), visible.0);
    assert_eq!(
        poses.len(),
        2,
        "The separately hidden occurrence stays hidden"
    );
    let point = poses[0].transform_point([10., 0., 0.]);
    assert!((point[0] - 100.).abs() < 0.001 && (point[1] - 10.).abs() < 0.001);
    // Mesh and edge playback use the same pose list. Neither hidden geometry at
    // X=4000 nor the hidden copy at X=9000 contributes to shared Frame fitting.
    let bounds = teaching::validate_frames(&[retained(frame)]).unwrap();
    assert!((bounds.center[0] - 200.).abs() < 0.001);
    assert!((bounds.center[1] - 5.).abs() < 0.001);
    assert!(bounds.radius < 120.);
    assert_eq!(host.server.manager.export_project_model().unwrap(), before);
}

#[test]
fn visibility_only_final_capture_retains_hidden_rows_without_legacy_fallback() {
    let (manager, visible, hidden) = fixture();
    let mut server = crate::CadServer::new().unwrap();
    server.manager = manager;
    let mut check = || Ok(());
    let mut host = DetachedHost {
        server: &mut server,
        archive: None,
        baseline_attempted: false,
        baseline_loaded: true,
        frames: Frames::default(),
        unrendered_domain_intents: Default::default(),
        check: &mut check,
    };
    host.capture(ProjectPreviewStage::Baseline).unwrap();
    host.server
        .manager
        .set_project_visibility(ProjectVisibilityDto {
            hidden_body_ids: vec![visible.0, hidden.0],
            ..Default::default()
        })
        .unwrap();
    host.capture(ProjectPreviewStage::Final).unwrap();
    assert_eq!(host.frames.values.len(), 2);
    let baseline = &host.frames.values[0];
    let final_frame = &host.frames.values[1];
    assert_eq!(final_frame.stage, ProjectPreviewStage::Final);
    assert_eq!(baseline.scene, final_frame.scene);
    assert_eq!(body_poses(Some(&baseline.assembly), visible.0).len(), 2);
    assert!(!final_frame.assembly.body_poses.is_empty());
    assert!(!final_frame.assembly.instance_body_poses.is_empty());
    assert!(final_frame
        .assembly
        .instance_body_poses
        .iter()
        .all(|p| !p.visible));
    for body in &final_frame.scene.bodies {
        assert!(body_poses(Some(&final_frame.assembly), body.id.0).is_empty());
    }
    let final_bounds = teaching::validate_frames(&[retained(final_frame)]).unwrap();
    assert_eq!((final_bounds.center, final_bounds.radius), ([0.; 3], 1.));
    let retained_bounds =
        teaching::validate_frames(&[retained(baseline), retained(final_frame)]).unwrap();
    assert!((retained_bounds.center[0] - 200.).abs() < 0.001);
    assert!(retained_bounds.radius < 120.);
}

#[test]
fn showing_a_body_in_final_capture_keeps_its_occurrence_visibility() {
    let (manager, visible, hidden) = fixture();
    let mut server = crate::CadServer::new().unwrap();
    server.manager = manager;
    let mut check = || Ok(());
    let mut host = DetachedHost {
        server: &mut server,
        archive: None,
        baseline_attempted: false,
        baseline_loaded: true,
        frames: Frames::default(),
        unrendered_domain_intents: Default::default(),
        check: &mut check,
    };
    host.capture(ProjectPreviewStage::Baseline).unwrap();
    host.server
        .manager
        .set_project_visibility(ProjectVisibilityDto::default())
        .unwrap();
    host.capture(ProjectPreviewStage::Final).unwrap();
    assert_eq!(host.frames.values.len(), 2);
    let final_frame = &host.frames.values[1];
    assert_eq!(body_poses(Some(&final_frame.assembly), visible.0).len(), 2);
    assert_eq!(body_poses(Some(&final_frame.assembly), hidden.0).len(), 1);
    assert!(body_poses(Some(&host.frames.values[0].assembly), hidden.0).is_empty());
    let bounds = teaching::validate_frames(&[retained(final_frame)]).unwrap();
    assert!(bounds.center[0] > 2000. && bounds.radius < 2100.);
}

#[test]
fn empty_scene_capture_keeps_default_frame_bounds() {
    let mut server = crate::CadServer::new().unwrap();
    let mut check = || Ok(());
    let mut host = DetachedHost {
        server: &mut server,
        archive: None,
        baseline_attempted: false,
        baseline_loaded: true,
        frames: Frames::default(),
        unrendered_domain_intents: Default::default(),
        check: &mut check,
    };
    host.capture(ProjectPreviewStage::Baseline).unwrap();
    let bounds = teaching::validate_frames(&[retained(&host.frames.values[0])]).unwrap();
    assert_eq!((bounds.center, bounds.radius), ([0.; 3], 1.));
}

#[test]
fn native_baseline_only_archive_preserves_saved_body_and_occurrence_visibility() {
    use limo_cad_project_file::{ReplayPackage, SaveMetadata};
    let (manager, visible, hidden) = fixture();
    let archive = ProjectArchive::new(
        manager.export_project_model().unwrap(),
        SaveMetadata {
            application_version: "test",
            saved_at: "2026-10-23T00:00:00Z",
        },
    )
    .unwrap();
    let source = json!({"version":2,"name":"Hidden stock","starting_state":"project","steps":[]})
        .to_string();
    let package = ReplayPackage::new(source.clone(), archive.encode().unwrap()).unwrap();
    let result = run_prepared_project_preview(
        super::super::prepare_project_preview_source(&package, &source).unwrap(),
        || Ok(()),
    )
    .unwrap();
    assert_eq!(result.frames.len(), 1);
    let frame = &result.frames[0];
    assert_eq!(frame.stage, ProjectPreviewStage::Baseline);
    assert_eq!(frame.scene.bodies.len(), 2);
    assert!(body_poses(Some(&frame.assembly), hidden.0).is_empty());
    assert_eq!(body_poses(Some(&frame.assembly), visible.0).len(), 2);
    let bounds = teaching::validate_frames(&[retained(frame)]).unwrap();
    assert!(bounds.center[0] > 190. && bounds.center[0] < 210. && bounds.radius < 120.);
}
