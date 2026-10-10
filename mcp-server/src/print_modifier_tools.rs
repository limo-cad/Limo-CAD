use super::*;
use print_intent_tools::{guarded, settings_schema};

pub(super) fn modifier_schema() -> Value {
    let coordinate = json!({"type":"number","minimum":-1_000_000,"maximum":1_000_000});
    let size = json!({"type":"number","exclusiveMinimum":0,"maximum":1_000_000});
    object_schema(
        json!({
            "id":{"type":"string","pattern":"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"},
            "name":{"type":"string","minLength":1,"maxLength":256},
            "body_id":{"type":"integer","minimum":1,"maximum":9_007_199_254_740_990_u64},
            "enabled":{"type":"boolean"},
            "local_pose":object_schema(json!({
                "translation_mm":{"type":"array","minItems":3,"maxItems":3,"items":coordinate},
                "rotation":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"number","minimum":-1,"maximum":1},"description":"Unit quaternion x,y,z,w in the source definition frame."}
            }), &["translation_mm","rotation"]),
            "primitive":{"oneOf":[
                object_schema(json!({"kind":{"const":"box"},"size_mm":{"type":"array","minItems":3,"maxItems":3,"items":size}}), &["kind","size_mm"]),
                object_schema(json!({"kind":{"const":"cylinder"},"radius_mm":size,"height_mm":size}), &["kind","radius_mm","height_mm"])
            ]},
            "settings":settings_schema()
        }),
        &["id", "name", "body_id", "enabled", "primitive", "settings"],
    )
}

pub fn specs() -> Vec<ToolSpec> {
    let write = guarded(json!({"modifier":modifier_schema()}), &["modifier"]);
    let identity = guarded(
        json!({"id":{"type":"string","minLength":36,"maxLength":36}}),
        &["id"],
    );
    vec![
        ToolSpec::direct("print_modifier_create", "Create print-only modifier", "Create a bounded centered box/cylinder zone in a stable CAD body's definition coordinates; intentional repeats inherit it. Supply a unique UUID and exact completed model snapshot. This allocates no mechanical body and never changes CAD geometry/material. Conflicting overlapping requested zones are rejected conservatively.", "print_modifier_create", Payload::Object, write.clone()),
        ToolSpec::direct("print_modifier_update", "Edit print-only modifier", "Replace an existing zone while retaining its stable UUID and body attachment. Shape, rigid local pose, enabled state and the five qualified requested process settings may change; reattachment requires explicit copy. Current completed model snapshot required.", "print_modifier_update", Payload::Object, write),
        ToolSpec::direct("print_modifier_remove", "Delete print-only modifier", "Remove an explicit print zone, without changing any CAD feature or appearance. Current completed model snapshot required.", "print_modifier_remove", Payload::Object, identity.clone()),
        ToolSpec::direct("print_modifier_reset", "Reset modifier settings", "Clear only this zone's requested settings so it inherits defaults and is omitted as a no-effect native volume; preserve its UUID, shape, local pose, name and attachment. Current completed model snapshot required.", "print_modifier_reset", Payload::Object, identity),
        ToolSpec::direct("print_modifier_copy", "Copy print-only modifier", "Copy local shape/pose and explicit settings to a known stable body with a newly allocated modifier UUID. No physical body or feature is allocated. Deliberate recovery may copy a retained orphan to a known body; update cannot rebind it.", "print_modifier_copy", Payload::Object,
            guarded(json!({"source_id":{"type":"string","minLength":36,"maxLength":36},"target_body_id":{"type":"integer","minimum":1,"maximum":9_007_199_254_740_990_u64},"name":{"type":"string","minLength":1,"maxLength":256}}), &["source_id","target_body_id"])),
        ToolSpec::direct("print_modifier_effective", "Inspect effective print zones", "Read the unified requested print-intent report including stable source bindings, deliberate occurrence repeats, local bounds, inheritance sources and unsupported targets. Portable/STL omit print-only geometry. Native Bambu export additionally checks actual material intersection and prevents cross-sibling targeting. Requested values are not realized toolpath/strength evidence.", "print_modifier_effective", Payload::Object,
            object_schema(json!({"body_ids":{"type":"array","maxItems":4096,"uniqueItems":true,"items":{"type":"integer","minimum":1}},"target":{"type":"string","enum":["portable","bambu_studio","orca_slicer","prusa_slicer"]}}), &[])),
    ]
}
