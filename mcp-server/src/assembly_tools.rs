//! Typed assembly operations shared by the native inspector and MCP.
use super::*;

pub fn specs() -> Vec<ToolSpec> {
    let id = json!({"type":"integer","minimum":1});
    let motion = object_schema(
        json!({"joint_id":id,"angle_offset_deg":{"type":"number"},"linear_offset_mm":{"type":"number"},"secondary_angle_offset_deg":{"type":"number"},"tertiary_angle_offset_deg":{"type":"number"},"secondary_linear_offset_mm":{"type":"number"}}),
        &["joint_id", "angle_offset_deg", "linear_offset_mm"],
    );
    let mut contact = json!({
        "name":{"type":"string","minLength":1},
        "occurrence_a":id,"body_a":id,"occurrence_b":id,"body_b":id,
        "clearance_mm":{"type":"number","minimum":0},
        "stop_motion":{"type":"boolean"}
    });
    let create = object_schema(
        contact.clone(),
        &["name", "occurrence_a", "body_a", "occurrence_b", "body_b"],
    );
    contact["id"] = id.clone();
    contact["enabled"] = json!({"type":"boolean"});
    let mut tools = vec![
        ToolSpec::direct("assembly_preview_mechanism_drag","Preview component drag",
            "Solve a grabbed component through its joint mechanism without changing saved intent. Supply the picked point in body-local coordinates and its target in document coordinates. Returns solved poses, coordinates and convergence diagnostics; it never moves source geometry.",
            "assembly_preview_mechanism_drag",Payload::Object,object_schema(json!({"body_id":id,"occurrence_id":id,
            "target_pose":object_schema(json!({"body_id":id,"translation":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"rotation":{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4}}),&["body_id","translation","rotation"]),
            "grab_point_local":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"target_point_world":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"initial_joint_motions":{"type":"array","items":motion},"solve_orientation":{"type":"boolean"},"maximum_iterations":{"type":"integer","minimum":1,"maximum":96}}),&["body_id","target_pose"])),
        ToolSpec::direct("assembly_apply_joint_motions","Save mechanism position",
            "Apply joint coordinates atomically through the assembly solver. This is the commit used when releasing a component drag.",
            "assembly_apply_joint_motions",Payload::Object,object_schema(json!({"motions":{"type":"array","items":motion}}),&["motions"])),
        ToolSpec::direct("assembly_preview_joint_coordinates","Preview joint motion",
            "Solve all five supported joint coordinates without changing saved joint intent or source geometry. Returns the solved component poses and diagnostics.",
            "assembly_preview_joint_coordinates",Payload::Object,object_schema(json!({"motion":motion}), &["motion"])),
        ToolSpec::direct("assembly_set_joint_coordinates","Save joint position",
            "Persist joint coordinates using the same solver and limits as the live motion controls. Rotations are degrees and translations are millimetres.",
            "assembly_set_joint_coordinates",Payload::Object,object_schema(json!({"motion":motion}), &["motion"])),
        ToolSpec::direct("assembly_swept_collision_check", "Check swept assembly collisions",
            "Check exact placed B-reps throughout a persisted motion study. Read-only; rates are 1–240 Hz with at most 100,001 samples. Results are sampled collision intervals, not a continuous collision proof.",
            "assembly_swept_collision_check", Payload::Object,
            object_schema(json!({"study_id":id,"sample_rate_hz":{"type":"number","minimum":1,"maximum":240},"clearance_threshold_mm":{"type":"number","minimum":0},"stop_at_first":{"type":"boolean"}}), &["study_id"])),
        ToolSpec::direct("assembly_create_contact_set", "Create assembly contact stop",
            "Create a persisted contact between two placed component bodies. Clearance is millimetres; stop_motion defaults to true.",
            "assembly_create_contact_set", Payload::Object, create),
        ToolSpec::direct("assembly_update_contact_set", "Update assembly contact stop",
            "Replace the full queried contact record, including its stable id, body instances, clearance and enabled/stop flags.",
            "assembly_update_contact_set", Payload::Object,
            object_schema(contact, &["id","name","occurrence_a","body_a","occurrence_b","body_b","clearance_mm","stop_motion","enabled"])),
        ToolSpec::direct("assembly_delete_contact_set", "Delete assembly contact stop",
            "Remove the contact by its stable id without modifying part geometry.",
            "assembly_delete_contact_set", Payload::Field("contact_id"),
            object_schema(json!({"contact_id":id}), &["contact_id"])),
    ];
    tools.extend(motion_specs(motion, id));
    tools
}

fn motion_specs(motion: Value, id: Value) -> Vec<ToolSpec> {
    let motors = object_schema(
        json!({"kind":{"const":"motor"},"initial_value":{"type":"number"},"velocity_per_second":{"type":"number"},"acceleration_per_second2":{"type":"number"}}),
        &[
            "kind",
            "initial_value",
            "velocity_per_second",
            "acceleration_per_second2",
        ],
    );
    let keys = object_schema(
        json!({"kind":{"const":"keyframes"},"keyframes":{"type":"array","minItems":1,"items":object_schema(json!({"time_seconds":{"type":"number","minimum":0},"value":{"type":"number"},"interpolation":{"enum":["step","linear","smooth"]}}),&["time_seconds","value"])}}),
        &["kind", "keyframes"],
    );
    let driver = object_schema(
        json!({"id":id,"name":{"type":"string","minLength":1},"joint_id":id,"coordinate":{"enum":["primary_angle","secondary_angle","tertiary_angle","primary_linear","secondary_linear"]},"enabled":{"type":"boolean"},"law":{"oneOf":[motors,keys]}}),
        &["id", "name", "joint_id", "coordinate", "law"],
    );
    let position = json!({"id":id,"name":{"type":"string","minLength":1},"motions":{"type":"array","items":motion}});
    vec![
        ToolSpec::direct("assembly_create_position","Capture assembly position","Save named joint coordinates, without baking source geometry.","assembly_create_position",Payload::Object,object_schema(json!({"name":{"type":"string","minLength":1},"motions":{"type":"array","items":motion}}),&["name","motions"])),
        ToolSpec::direct("assembly_update_position","Edit assembly position","Replace the queried named position including all joint coordinates.","assembly_update_position",Payload::Object,object_schema(position,&["id","name","motions"])),
        ToolSpec::direct("assembly_delete_position","Delete assembly position","Remove a saved position by id.","assembly_delete_position",Payload::Field("position_id"),object_schema(json!({"position_id":id}),&["position_id"])),
        ToolSpec::direct("assembly_apply_position","Apply assembly position","Solve and persist the saved joint coordinates.","assembly_apply_position",Payload::Field("position_id"),object_schema(json!({"position_id":id}),&["position_id"])),
        ToolSpec::direct("assembly_create_motion_study","Create motion study","Create a study with a duration in seconds.","assembly_create_motion_study",Payload::Object,object_schema(json!({"name":{"type":"string","minLength":1},"duration_seconds":{"type":"number","exclusiveMinimum":0}}),&["name","duration_seconds"])),
        ToolSpec::direct("assembly_update_motion_study","Edit motion study","Replace the full queried study with typed motor or keyframe drivers. Angles use degrees and translation uses millimetres.","assembly_update_motion_study",Payload::Object,object_schema(json!({"id":id,"name":{"type":"string","minLength":1},"duration_seconds":{"type":"number","exclusiveMinimum":0},"playback_speed":{"type":"number","exclusiveMinimum":0},"looped":{"type":"boolean"},"drivers":{"type":"array","items":driver},"next_driver_id":id}),&["id","name","duration_seconds","playback_speed","looped","drivers","next_driver_id"])),
        ToolSpec::direct("assembly_delete_motion_study","Delete motion study","Remove a study by id.","assembly_delete_motion_study",Payload::Field("study_id"),object_schema(json!({"study_id":id}),&["study_id"])),
        ToolSpec::direct("assembly_evaluate_motion_study","Evaluate motion study","Read-only contact-aware sampling over exact B-reps where bounding checks permit contact. Contacts are probed across the requested interval; this is not a continuous collision guarantee.","assembly_evaluate_motion_study",Payload::Object,object_schema(json!({"study_id":id,"time_seconds":{"type":"number","minimum":0},"previous_time_seconds":{"type":["number","null"],"minimum":0},"enforce_contacts":{"type":"boolean"}}),&["study_id","time_seconds"])),
        ToolSpec::direct("assembly_sample_motion_study","Sample motion study","Read-only joint and component poses at the requested time without contact enforcement.","assembly_sample_motion_study",Payload::Object,object_schema(json!({"study_id":id,"time_seconds":{"type":"number","minimum":0}}),&["study_id","time_seconds"])),
        ToolSpec::direct("assembly_export_motion_path_csv","Export motion paths","Return sampled component paths as CSV text; does not change the document.","assembly_export_motion_path_csv",Payload::Object,object_schema(json!({"study_id":id,"sample_rate_hz":{"type":"number","minimum":1,"maximum":240},"occurrence_ids":{"type":"array","items":id}}),&["study_id"])),
    ]
}
