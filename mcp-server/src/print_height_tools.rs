use super::*;

fn uuid_schema() -> Value {
    json!({"type":"string","pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"})
}
fn body_schema() -> Value {
    json!({"type":"integer","minimum":1,"maximum":9007199254740990u64})
}
fn name_schema() -> Value {
    json!({"type":"string","minLength":1,"maxLength":200})
}
pub(super) fn layout_schema() -> Value {
    json!({"oneOf":[
        object_schema(json!({"kind":{"const":"assembly"}}), &["kind"]),
        object_schema(json!({"kind":{"const":"named_layout"},"id":uuid_schema()}), &["kind","id"])
    ]})
}
fn speeds_schema() -> Value {
    let speed = json!({"type":["number","null"],"minimum":0,"maximum":1000000});
    object_schema(
        json!({"outer_wall_mm_s":speed,"inner_wall_mm_s":speed,"infill_mm_s":speed}),
        &[],
    )
}
fn points_schema() -> Value {
    json!({"type":"array","minItems":3,"maxItems":4096,"items":object_schema(json!({
        "z_mm":{"type":"number","minimum":0,"maximum":1000000},
        "height_mm":{"type":"number","exclusiveMinimum":0,"maximum":1000000}
    }), &["z_mm","height_mm"])})
}
pub(super) fn binding_schema() -> Value {
    let vector = json!({"type":"array","minItems":3,"maxItems":3,"items":{"type":"number","minimum":-1000000,"maximum":1000000}});
    let pose = object_schema(
        json!({"translation_mm":vector,
            "rotation":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"number","minimum":-1,"maximum":1}}
        }),
        &["translation_mm", "rotation"],
    );
    object_schema(
        json!({
            "layout":layout_schema(),
            "groups":{"type":"array","minItems":1,"maxItems":4096,"items":object_schema(json!({
                "root_occurrence_id":body_schema(),
                "members":{"type":"array","minItems":1,"maxItems":4096,"items":object_schema(json!({"body_id":body_schema(),"occurrence_id":body_schema()}), &["body_id","occurrence_id"])},
                "min_z_mm":{"type":"number","minimum":-1000000,"maximum":1000000},
                "max_z_mm":{"type":"number","minimum":-1000000,"maximum":1000000}
            }), &["root_occurrence_id","members","min_z_mm","max_z_mm"])},
            "occurrences":{"type":"array","minItems":1,"maxItems":4096,"items":object_schema(json!({
                "body_id":body_schema(),"occurrence_id":body_schema(),"root_occurrence_id":body_schema(),"pose":pose,
                "min_z_mm":{"type":"number","minimum":-1000000,"maximum":1000000},
                "max_z_mm":{"type":"number","minimum":-1000000,"maximum":1000000}
            }), &["body_id","occurrence_id","root_occurrence_id","pose","min_z_mm","max_z_mm"])}
        }),
        &["layout", "occurrences", "groups"],
    )
}
fn range_properties() -> Value {
    json!({"name":name_schema(),"body_id":body_schema(),"enabled":{"type":"boolean"},
        "coordinate":{"enum":["object_bottom","build_plate"]},
        "min_z_mm":{"type":"number","minimum":0,"maximum":1000000},
        "max_z_mm":{"type":"number","minimum":0,"maximum":1000000},
        "settings":print_intent_tools::settings_schema(),"speeds":speeds_schema()
    })
}
pub(super) fn stored_range_schema() -> Value {
    let mut props = range_properties();
    props["id"] = uuid_schema();
    props["binding"] = binding_schema();
    object_schema(
        props,
        &[
            "id",
            "name",
            "body_id",
            "enabled",
            "coordinate",
            "min_z_mm",
            "max_z_mm",
            "settings",
            "binding",
        ],
    )
}
pub(super) fn stored_profile_schema() -> Value {
    object_schema(
        json!({"id":uuid_schema(),"name":name_schema(),"body_id":body_schema(),
            "enabled":{"type":"boolean"},"binding":binding_schema(),"points":points_schema()
        }),
        &["id", "name", "body_id", "enabled", "binding", "points"],
    )
}
pub(super) fn refresh_object_schema() -> Value {
    let ranges = json!({"type":"array","maxItems":256,"items":object_schema(json!({
        "min_z_mm":{"type":"number","minimum":0,"maximum":1000000},
        "max_z_mm":{"type":"number","minimum":0,"maximum":1000000},
        "layer_height_mm":{"type":"number","exclusiveMinimum":0,"maximum":1000000},
        "settings":print_intent_tools::settings_schema(),"speeds":speeds_schema()
    }), &["min_z_mm","max_z_mm","layer_height_mm","settings"])});
    let profile = json!({"oneOf":[{"type":"null"},points_schema()]});
    object_schema(
        json!({
            "source_bindings":{"type":"array","minItems":1,"maxItems":4096,"items":object_schema(json!({"body_id":body_schema(),"occurrence_id":body_schema()}), &["body_id","occurrence_id"])},
            "baseline_ranges":ranges,"written_ranges":ranges,"baseline_profile":profile,"written_profile":profile
        }),
        &[
            "source_bindings",
            "baseline_ranges",
            "written_ranges",
            "baseline_profile",
            "written_profile",
        ],
    )
}
fn guarded(mut props: Value, required: &[&str]) -> Value {
    props["expected_model_json"] = json!({"type":"string","minLength":1,
        "description":"Exact completed cad_project_model snapshot, checked atomically by the owning engine."});
    let mut required = required.to_vec();
    required.push("expected_model_json");
    object_schema(props, &required)
}
pub(super) fn specs() -> Vec<ToolSpec> {
    let optional_id = json!({"oneOf":[{"type":"null"},uuid_schema()]});
    let mut range = range_properties();
    range["id"] = optional_id.clone();
    range["layout"] = layout_schema();
    let range = object_schema(
        range,
        &[
            "name",
            "body_id",
            "enabled",
            "coordinate",
            "min_z_mm",
            "max_z_mm",
            "layout",
            "settings",
        ],
    );
    let profile = object_schema(
        json!({"id":optional_id,"name":name_schema(),"body_id":body_schema(),
            "enabled":{"type":"boolean"},"layout":layout_schema(),"points":points_schema()
        }),
        &["name", "body_id", "enabled", "layout", "points"],
    );
    vec![
        ToolSpec::direct("print_intent_height_binding","Inspect resolved print Z binding",
            "Read every visible intentional source occurrence, its canonical resolved pose, multipart root and printable group Z bounds for assembled or a stable saved layout. Does not create a layout identity or metadata.",
            "print_intent_height_binding",Payload::Object,
            object_schema(json!({"body_id":body_schema(),"layout":layout_schema()}), &["body_id","layout"])),
        ToolSpec::direct("print_intent_upsert_height_range","Save requested print height interval",
            "Create a definition-level requested range by omitting id, or update its existing id. New bindings are captured by the engine; updates preserve placement evidence. Every intentional repeat inherits. Changed orientations require explicit rebind. Bambu's qualified adapter writes uniform extruder-variant speed vectors; other targets report those fields unsupported.",
            "print_intent_upsert_height_range",Payload::Object,guarded(json!({"range":range}), &["range"])),
        ToolSpec::direct("print_intent_upsert_layer_profile","Save requested variable layer profile",
            "Create by omitting id or update an existing id. Separate opt-in from settings-only ranges. Provide at least three increasing object-bottom Z samples, zero and exact final object height. Nozzle/first-layer/support/plate constraints come from the verified slicer template; saved metadata is not toolpath evidence.",
            "print_intent_upsert_layer_profile",Payload::Object,guarded(json!({"profile":profile}), &["profile"])),
        ToolSpec::direct("print_intent_remove_height","Remove print height intent",
            "Remove a range or variable profile by its stable id, including orphan intent. Geometry, layout poses and identity allocation remain unchanged.",
            "print_intent_remove_height",Payload::Object,guarded(json!({"id":uuid_schema()}), &["id"])),
        ToolSpec::direct("print_intent_rebind_height","Review and rebind print orientation",
            "Deliberately capture current canonical poses and group bounds for an existing range/profile. Absent interval/points keeps numeric intent. Explicit interval (range only) or points (variable profile only) permits atomic reviewed correction after geometry or placement changes; values are never clamped.",
            "print_intent_rebind_height",Payload::Object,guarded(json!({"id":uuid_schema(),"layout":layout_schema(),
                "interval":{"oneOf":[{"type":"null"},object_schema(json!({
                    "coordinate":{"enum":["object_bottom","build_plate"]},
                    "min_z_mm":{"type":"number","minimum":0,"maximum":1000000},
                    "max_z_mm":{"type":"number","minimum":0,"maximum":1000000}
                }), &["coordinate","min_z_mm","max_z_mm"])]},
                "points":{"oneOf":[{"type":"null"},points_schema()]}
            }), &["id","layout"])),
    ]
}
