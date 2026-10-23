//! Offline adapter. Called while the prerequisite's isolated host is still alive.
//! No MCP operation is added and no result is invented when native inspection is unavailable.
use limo_cad_native_engine::NativeEngineHost;
use limo_cad_occt::measurement::MeasurementRequest;
use serde_json::{json, Value};

pub fn complete_measure_frames(host: &NativeEngineHost, exports: &mut Value) -> Result<(), String> {
    let mut request: MeasurementRequest =
        serde_json::from_value(exports["native_measurement_request"].clone())
            .map_err(|e| e.to_string())?;
    let expected: limo_cad_solid::SolidSceneDto =
        serde_json::from_value(exports["source_scene"].clone()).map_err(|e| e.to_string())?;
    if host.viewport_frame().document.scene.as_ref() != &expected {
        return Err("Measure authoring host does not match the recipe's exact source scene".into());
    }
    if exports["expected_distance_mm"].as_f64() != Some(8.) {
        return Err("This bounded bracket lesson requires an 8 mm wall".into());
    }
    let before = host.viewport_frame();
    resolve_occurrences(&mut request, &before.instance_body_poses)?;
    let frames = exports["lessons"]["measure"]
        .as_array()
        .ok_or("Missing measure templates")?;
    if frames.len() != 4 || frames[2]["inspection"]["native_measurement_pending"] != true {
        return Err("Expected exactly four incomplete measure frames".into());
    }
    for frame in frames {
        let scene: limo_cad_solid::SolidSceneDto =
            serde_json::from_value(frame["scene"].clone()).map_err(|e| e.to_string())?;
        if scene != expected {
            return Err("Every Measure frame must retain the exact prerequisite scene".into());
        }
    }
    let finished = host.prepare_measurement_review(&request)?.inspect()?;
    host.validate_measurement_review(&finished)?;
    let report = finished.into_report();
    if report
        .distance
        .is_none_or(|d| !d.is_finite() || (d - 8.).abs() > 1e-6)
        || report.closest_points.is_none_or(|points| {
            points.iter().flatten().any(|v| !v.is_finite())
                || ((0..3)
                    .map(|i| (points[0][i] - points[1][i]).powi(2))
                    .sum::<f64>()
                    .sqrt()
                    - 8.)
                    .abs()
                    > 1e-6
        })
    {
        return Err("Native wall distance did not verify as 8 mm".into());
    }
    let after = host.viewport_frame();
    if after.session_id != before.session_id
        || after.geometry_revision != before.geometry_revision
        || after.document.metadata_revision != before.document.metadata_revision
        || !std::sync::Arc::ptr_eq(&after.document.scene, &before.document.scene)
        || after.document.scene.as_ref() != &expected
        || after.body_poses != before.body_poses
        || after.instance_body_poses != before.instance_body_poses
    {
        return Err("Native measurement changed its source identity or geometry".into());
    }
    let frames = exports["lessons"]["measure"]
        .as_array_mut()
        .ok_or("Missing measure templates")?;
    if frames.len() != 4 || frames[2]["inspection"]["native_measurement_pending"] != true {
        return Err("Expected exactly four incomplete measure frames".into());
    }
    let data = frames[2]["inspection"]
        .as_object_mut()
        .ok_or("Missing inspection frame")?;
    data.remove("native_measurement_pending");
    data.insert(
        "measurement".into(),
        json!({"request":request,"report":report,
            "body_poses":before.body_poses.as_ref(),
            "instance_body_poses":before.instance_body_poses.as_ref()}),
    );
    Ok(())
}

/// Body selections in the authored source are resolved against the prerequisite's
/// actual presentation. Never guess between multiple visible occurrences.
fn resolve_occurrences(
    request: &mut MeasurementRequest,
    instances: &[limo_cad_sketch::InstanceBodyPoseDto],
) -> Result<(), String> {
    for selection in &mut request.selections {
        if selection.occurrence_id.is_some() {
            return Err("Bundled Measure must resolve its occurrence on the isolated host".into());
        }
        if !instances.is_empty() {
            let mut matching = instances
                .iter()
                .filter(|p| p.body_id == selection.body_id && p.visible);
            let pose = matching
                .next()
                .ok_or("Measure body has no visible native occurrence")?;
            if matching.next().is_some() {
                return Err("Measure body has ambiguous native occurrences".into());
            }
            if pose.translation != [0.; 3] || pose.rotation != [0., 0., 0., 1.] {
                return Err("This bracket guide requires its original native placement".into());
            }
            selection.occurrence_id = Some(pose.occurrence_id.0);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn occurrence_resolution_uses_actual_unique_visible_identity_and_refuses_ambiguity() {
        let request: MeasurementRequest = serde_json::from_value(json!({"selections":[
            {"body_id":1,"occurrence_id":null,"target":{"kind":"face","face_id":2}}
        ]}))
        .unwrap();
        let pose: limo_cad_sketch::InstanceBodyPoseDto = serde_json::from_value(json!({
            "occurrence_id":7,"component_id":3,"body_id":1,
            "translation":[0,0,0],"rotation":[0,0,0,1],"visible":true
        }))
        .unwrap();
        let mut actual = request.clone();
        resolve_occurrences(&mut actual, &[pose]).unwrap();
        assert_eq!(actual.selections[0].occurrence_id, Some(7));
        assert!(resolve_occurrences(&mut request.clone(), &[pose, pose]).is_err());
        assert!(resolve_occurrences(
            &mut request.clone(),
            &[limo_cad_sketch::InstanceBodyPoseDto {
                visible: false,
                ..pose
            }]
        )
        .is_err());
        assert!(resolve_occurrences(
            &mut request.clone(),
            &[limo_cad_sketch::InstanceBodyPoseDto {
                translation: [1., 0., 0.],
                ..pose
            }]
        )
        .is_err());
        resolve_occurrences(&mut request.clone(), &[]).unwrap();
    }
}
