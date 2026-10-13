use super::*;

pub(super) fn settings_schema() -> Value {
    let count = json!({"type":["integer","null"],"minimum":0,"maximum":1000});
    object_schema(
        json!({
            "wall_count":count,
            "infill_density_percent":{"type":["number","null"],"minimum":0,"maximum":100},
            "infill_pattern":{"type":["string","null"],"enum":[null,"grid","gyroid","rectilinear","concentric","cubic","honeycomb","lightning"]},
            "top_shell_layers":count,"bottom_shell_layers":count
        }),
        &[],
    )
}

pub(super) fn guarded(properties: Value, required: &[&str]) -> Value {
    let mut properties = properties.as_object().unwrap().clone();
    properties.insert("expected_model_json".into(), json!({
        "type":"string","minLength":1,
        "description":"Exact completed model snapshot from cad_project_model; compared atomically by the owning engine."
    }));
    let mut required = required.to_vec();
    required.push("expected_model_json");
    object_schema(Value::Object(properties), &required)
}

fn document_schema() -> Value {
    let settings = settings_schema();
    let pinned = object_schema(
        json!({
            "repository":{"type":"string","minLength":1,"maxLength":512},
            "revision":{"type":"string","pattern":"^[0-9a-fA-F]{40}$"},
            "profile":{"type":"string","minLength":1,"maxLength":4096},
            "files":{"type":"object","minProperties":1,"maxProperties":128,"additionalProperties":{"type":"string","pattern":"^[0-9a-fA-F]{64}$"}}
        }),
        &["repository", "revision", "profile", "files"],
    );
    let source = json!({"oneOf":[
        {"type":"null"},
        object_schema(json!({"kind":{"const":"pinned_repository"},"source":pinned}), &["kind","source"]),
        object_schema(json!({"kind":{"const":"saved_template"},"sha256":{"type":"string","pattern":"^[0-9a-fA-F]{64}$"},"source_label":{"type":"string","minLength":1,"maxLength":256}}), &["kind","sha256","source_label"])
    ]});
    let profile = object_schema(
        json!({
            "profile_id":{"type":"string","minLength":1,"maxLength":256},
            "name":{"type":"string","minLength":1,"maxLength":256},
            "source":source,"status":{"type":"string","enum":["resolved","unresolved"]},"defaults":settings
        }),
        &["profile_id", "name", "status"],
    );
    object_schema(
        json!({
            "version":{"const":4},
            "source_document_id":{"type":["string","null"],"description":"Immutable UUID assigned by the engine on the first successful print-intent write."},
            "selected_process":{"oneOf":[{"type":"null"},profile]},
            "defaults":settings,
            "target_handoffs":{"type":"array","maxItems":16,"items":manufacturing_tools::handoff_schema()},
            "modifiers":{"type":"array","maxItems":256,"items":print_modifier_tools::modifier_schema()},
            "height_ranges":{"type":"array","maxItems":256,"items":print_height_tools::stored_range_schema()},
            "layer_height_profiles":{"type":"array","maxItems":128,"items":print_height_tools::stored_profile_schema()},
            "parts":{"type":"array","maxItems":4096,"items":object_schema(json!({"body_id":{"type":"integer","minimum":1},"settings":settings}), &["body_id","settings"])},
            "presets":{"type":"array","maxItems":128,"items":object_schema(json!({"name":{"type":"string","minLength":1,"maxLength":256},"settings":settings}), &["name","settings"])}
        }),
        &["version"],
    )
}

pub fn specs() -> Vec<ToolSpec> {
    let id = json!({"type":"integer","minimum":1});
    let settings = settings_schema();
    let preset = object_schema(
        json!({"name":{"type":"string","minLength":1,"maxLength":256},"settings":settings}),
        &["name", "settings"],
    );
    vec![
        ToolSpec::direct(
            "print_intent_get",
            "Read persistent print intent",
            "Read versioned requested process settings, stable source namespace, part overrides and presets. Appearance, geometry and joints are independent. Missing settings inherit; zero is an explicit override.",
            "print_intent_get",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "print_intent_effective",
            "Inspect effective print settings",
            "Resolve profile defaults, project defaults and stable body-definition overrides, including value sources, orphan bindings and unsupported target capabilities. Every repeated occurrence inherits its definition. This reports requested settings, not applied toolpaths or physical qualification.",
            "print_intent_effective",
            Payload::Object,
            object_schema(
                json!({"body_ids":{"type":"array","maxItems":4096,"uniqueItems":true,"items":id},"target":{"type":"string","enum":["portable","bambu_studio","orca_slicer","prusa_slicer"]}}),
                &[],
            ),
        ),
        ToolSpec::direct(
            "print_intent_set_part",
            "Set part print overrides",
            "Replace a stable body definition's typed overrides for every intentional occurrence. Null or absent fields inherit. Requires a current model precondition; never changes geometry, filament chemistry or color.",
            "print_intent_set_part",
            Payload::Object,
            guarded(
                json!({"body_id":id,"settings":settings}),
                &["body_id", "settings"],
            ),
        ),
        ToolSpec::direct(
            "print_intent_reset_part",
            "Reset part print overrides",
            "Remove definition-level print overrides so the part inherits project/profile defaults. Geometry and appearance remain unchanged.",
            "print_intent_reset_part",
            Payload::Object,
            guarded(json!({"body_id":id}), &["body_id"]),
        ),
        ToolSpec::direct(
            "print_intent_copy_part",
            "Copy part print overrides",
            "Copy only the source definition's explicit process overrides to selected stable body definitions. This does not copy material, color, geometry or assembly placement.",
            "print_intent_copy_part",
            Payload::Object,
            guarded(
                json!({"source_body_id":id,"target_body_ids":{"type":"array","minItems":1,"maxItems":4096,"uniqueItems":true,"items":id}}),
                &["source_body_id", "target_body_ids"],
            ),
        ),
        ToolSpec::direct(
            "print_intent_set_document",
            "Replace document print intent",
            "Atomically replace validated versioned print intent, including project defaults and profile provenance. Read print_intent_get first and retain its stable source namespace. Definition-only inheritance is supported; occurrence/layout overrides are staged separately. Unknown slicer keys are rejected.",
            "print_intent_set_document",
            Payload::Object,
            guarded(json!({"document":document_schema()}), &["document"]),
        ),
        ToolSpec::direct(
            "print_intent_upsert_preset",
            "Save named print preset",
            "Create or replace a named preset of typed requested process settings. Applying a preset uses print_intent_set_part and never changes filament chemistry or color.",
            "print_intent_upsert_preset",
            Payload::Object,
            guarded(json!({"preset":preset}), &["preset"]),
        ),
        ToolSpec::direct(
            "print_intent_remove_preset",
            "Delete named print preset",
            "Delete a saved process preset without changing settings already copied onto parts.",
            "print_intent_remove_preset",
            Payload::Object,
            guarded(
                json!({"name":{"type":"string","minLength":1,"maxLength":256}}),
                &["name"],
            ),
        ),
        ToolSpec::direct(
            "print_intent_upsert_handoff",
            "Save reviewed target handoff",
            "Persist a named reviewed Bambu refresh reference with exact source namespace, UUID/instance identities and bounded five-setting baselines. Requires a current model snapshot; no geometry or appearance changes. Orphan source identities remain reserved and never bind newly created parts.",
            "print_intent_upsert_handoff",
            Payload::Object,
            guarded(
                json!({"handoff":manufacturing_tools::handoff_schema()}),
                &["handoff"],
            ),
        ),
        ToolSpec::direct(
            "print_intent_remove_handoff",
            "Remove a saved target handoff",
            "Remove one named target reference without changing geometry, settings or allocator reservations. Exact current model snapshot required.",
            "print_intent_remove_handoff",
            Payload::Object,
            guarded(
                json!({"name":{"type":"string","minLength":1,"maxLength":256}}),
                &["name"],
            ),
        ),
    ]
}
