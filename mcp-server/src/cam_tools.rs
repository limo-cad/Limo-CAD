use super::{dto_schema, empty_schema, object_or_null, object_schema, Payload, ToolSpec};
use serde_json::{json, Value};

fn document_schema() -> Value {
    let records = |description: &str| {
        json!({
            "type": "array", "items": dto_schema(description), "description": description
        })
    };
    object_schema(
        json!({
            "setups": records("Manually configured setups: machine snapshot, WCS, stock definition/resolution, work offsets and operations with safe heights."),
            "active_setup_id": {"type": ["integer", "null"], "minimum": 1},
            "tools": records("Project tool snapshots, including internal id, machine number/name, cutter geometry, default cutting data and cutting presets."),
            "toolpath_generations": records("Engine-generated dependency stamps. Preserve on read/edit/write; never fabricate freshness evidence. Changed inputs are rechecked by the engine."),
            "height_expressions": records("Associative height intent per operation: clearance/retract/feed/top and optional bottom reference plus signed mm offset."),
            "linking": records("Lead-in/out, transitions, ramps and entry/exit position intent keyed by operation_id."),
            "load_warnings": records("Diagnostics returned by reads. Accepted on write but recomputed by the engine, not trusted as safety evidence."),
            "units": {"type": "string", "enum": ["millimeters", "inches"], "default": "millimeters",
                "description": "Display/output units. Stored geometry stays in canonical mm."},
            "post_defaults": dto_schema("Remembered CamPostConfigDto for unbound dialogs; a bound setup uses its machine profile snapshot."),
            "_disclosure": dto_schema("MCP read-result metadata. Accepted for read/edit/write round-tripping, ignored by the document serializer; never a machining setting or safety check."),
            "next_setup_id": {"type": "integer", "minimum": 1},
            "next_operation_id": {"type": "integer", "minimum": 1},
            "next_tool_id": {"type": "integer", "minimum": 1}
        }),
        &[],
    )
}

/// Simulation inputs shared by CAM and NC, matching the Rust DTOs. Geometry is
/// model-space mm; target meshes may be omitted after preparing a cache key.
fn simulation_properties() -> Value {
    let mesh = object_schema(
        json!({
            "positions": {"type": "array", "items": {"type": "number"}, "description": "Flat XYZ triplets in model coordinates, mm."},
            "indices": {"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": u32::MAX}, "description": "Flat triangle indices into positions."}
        }),
        &[],
    );
    let target = object_schema(
        json!({
            "cache_key": {"type": ["string", "null"]},
            "meshes": {"type": "array", "items": mesh},
            "tolerance_mm": {"type": "number", "minimum": 0}
        }),
        &[],
    );
    json!({
        "setup_id": {"type": "integer", "minimum": 1},
        "voxel_size": {"type": ["number", "null"], "exclusiveMinimum": 0},
        "max_voxels": {"type": ["integer", "null"], "minimum": 1},
        "stock_mesh": object_or_null(mesh),
        "target": object_or_null(target),
        "completed_steps": {"type": ["integer", "null"], "minimum": 0,
            "description": "Optional physical motion-step prefix. Zero shows uncut stock."}
    })
}

pub fn specs() -> Vec<ToolSpec> {
    let setup_id = json!({"type": "integer", "minimum": 1});
    let setup = object_schema(json!({"setup_id": setup_id}), &["setup_id"]);
    let mut cam = simulation_properties();
    cam["through_operation_id"] = json!({"type": ["integer", "null"], "minimum": 1,
        "description": "Stop after this operation, inclusive; earlier operations in the setup still contribute stock removal."});
    cam["playback_time_seconds"] = json!({"type": ["number", "null"], "minimum": 0,
        "description": "Playback prefix in seconds, including partial motion. Partial-time frames omit comparison verdicts; request the complete scope for verification."});
    let mut nc = simulation_properties();
    nc["source"] = json!({"type": "string", "description": "NC program text, not a file path. Unsupported motion/macros fail closed."});
    nc["file_name"] = json!({"type": ["string", "null"]});
    nc["dialect"] = json!({"type": "string", "enum": ["auto", "iso", "fanuc", "haas", "siemens828d"], "default": "auto"});
    vec![
        ToolSpec::direct("cam_get_document", "Inspect CAM document",
            "Return the complete machining document, including tools, setups, machine/post settings, generation stamps and associative height/linking intent. Preserve these fields when editing with cam_set_document.",
            "cam_document", Payload::Empty, empty_schema()),
        ToolSpec::direct("cam_set_document", "Write CAM document",
            "Replace the whole machining document after validation, not a partial patch. Start from cam_get_document and preserve unedited fields. Supports face, adaptive3d (High Speed Roughing), flat3d (Flat finishing), contour2d, pocket2d, chamfer2d, drill and thread operations; tools and setups are never auto-created. Geometry/tool compatibility and generation freshness remain engine-owned.",
            "cam_set_document", Payload::Object, document_schema()),
        ToolSpec::direct("cam_toolpath_statuses", "Inspect CAM toolpath safety",
            "Compare saved generation signatures against current CAD, setup, tool and operation dependencies. Returns current, never_generated, stale or invalid with actionable reasons.",
            "cam_toolpath_statuses", Payload::Empty, empty_schema()),
        ToolSpec::direct("cam_regenerate_operation", "Regenerate CAM toolpath",
            "Plan one enabled operation against current inputs and record its generation signature only after planning succeeds.",
            "cam_regenerate_operation", Payload::Field("operation_id"),
            object_schema(json!({"operation_id": setup_id}), &["operation_id"])),
        ToolSpec::direct("cam_regenerate_setup", "Regenerate CAM setup",
            "Plan every enabled operation in a setup and refresh generation signatures only after the complete plan succeeds.",
            "cam_regenerate_setup", Payload::Field("setup_id"), setup.clone()),
        ToolSpec::direct("cam_plan_setup", "Plan CAM setup",
            "Return the validated toolpath program and warnings. Optionally stop after through_operation_id, inclusive.",
            "cam_plan", Payload::Object,
            object_schema(json!({"setup_id": setup_id, "through_operation_id": {"type": ["integer", "null"], "minimum": 1}}), &["setup_id"])),
        ToolSpec::direct("cam_post_setup", "Post CAM setup",
            "Render NC text using the setup's machine/post snapshot. Posting rejects stale, ungenerated or invalid paths. Optional post overrides must match the bound controller and machine-specific settings; they do not select a different machine. Output follows the document's units. Output requires machine-specific review, not a safety certification.",
            "cam_post", Payload::Object,
            object_schema(json!({"setup_id": setup_id,
                "post": object_or_null(dto_schema("CamPostConfigDto: program settings consistent with the setup's bound machine; omission uses its snapshot.")),
                "program_name": {"type": ["string", "null"]}
            }), &["setup_id"])),
        ToolSpec::direct("cam_simulate_setup", "Simulate CAM setup",
            "Run bounded stock simulation and return remaining stock plus collision/comparison reports. Supply stock_mesh for modeled stock and target for finished-model comparison. Supports operation and playback prefixes, including zero-time uncut stock. This is not whole-machine collision certification.",
            "cam_simulate", Payload::Object, object_schema(cam, &["setup_id"])),
        ToolSpec::direct("cam_simulate_gcode", "Simulate NC program",
            "Interpret supported NC text against the setup and project tools, then run the shared stock simulation. Accepts modeled stock, target comparison and a motion-step prefix. Unsupported codes fail closed; no macros or files are executed.",
            "cam_simulate_gcode", Payload::Object, object_schema(nc, &["setup_id", "source"])),
        ToolSpec::direct("cam_post_events", "Export neutral post events",
            "Return the neutral post-event stream for a setup. Requires current toolpaths and the same pre-post verification as NC export. Returns data only; does not write or execute a private post file.",
            "cam_post_events", Payload::Field("setup_id"), setup),
    ]
}

#[cfg(test)]
mod tests;
