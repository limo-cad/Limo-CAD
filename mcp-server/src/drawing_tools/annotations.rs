use super::*;

pub(super) fn schema(
    anchor: &Value,
    circle: &Value,
    line: &Value,
    presentation: &Value,
    updating: bool,
) -> Value {
    let number = json!({"type":"number"});
    let text = json!({"type":"string"});
    let positive = json!({"type":"number","exclusiveMinimum":0});
    let position = vector(2);
    let attachment = json!({"oneOf":[
        object_schema(json!({"type":choice(&["anchor"]),"reference":anchor}), &["type","reference"]),
        object_schema(json!({"type":choice(&["line"]),"reference":line}), &["type","reference"]),
        object_schema(json!({"type":choice(&["circle"]),"reference":circle}), &["type","reference"])
    ]});
    let mut variants = Vec::new();
    let mut add =
        |kind: &str, mut properties: Value, fields: &[&str], dimension: bool, attached: bool| {
            let map = properties.as_object_mut().unwrap();
            map.insert("kind".into(), choice(&[kind]));
            map.insert("id".into(), json!({"type":"integer","minimum":1}));
            let mut required = vec!["kind"];
            if updating {
                required.push("id");
            }
            if attached {
                map.insert("view_id".into(), json!({"type":"integer","minimum":1}));
                required.push("view_id");
            }
            if dimension {
                map.insert("prefix".into(), text.clone());
                map.insert("suffix".into(), text.clone());
                map.insert(
                    "precision".into(),
                    json!({"type":"integer","minimum":0,"maximum":6}),
                );
                map.insert("presentation".into(), presentation.clone());
            }
            required.extend_from_slice(fields);
            variants.push(object_schema(properties, &required));
        };
    add(
        "linear_dimension",
        json!({"first":anchor,"second":anchor,"mode":choice(&["aligned","horizontal","vertical"]),"offset":number}),
        &["first", "second", "mode", "offset"],
        true,
        true,
    );
    add(
        "line_dimension",
        json!({"first":line,"second":{"oneOf":[line,{"type":"null"}]},"mode":choice(&["length","distance","angle"]),"position":position}),
        &["first", "mode", "position"],
        true,
        true,
    );
    add(
        "point_line_dimension",
        json!({"point":anchor,"line":line,"position":position}),
        &["point", "line", "position"],
        true,
        true,
    );
    add(
        "note",
        json!({"text":{"type":"string","maxLength":4096},"position":position}),
        &["text", "position"],
        false,
        false,
    );
    add(
        "radial_dimension",
        json!({"feature":circle,"mode":choice(&["radius","diameter"]),"leader_angle_deg":number,"offset":number}),
        &["feature", "mode", "leader_angle_deg", "offset"],
        true,
        true,
    );
    add(
        "angular_dimension",
        json!({"vertex":anchor,"first":anchor,"second":anchor,"radius":positive}),
        &["vertex", "first", "second", "radius"],
        true,
        true,
    );
    add(
        "hole_note",
        json!({"feature":circle,"position":position,"quantity":{"type":"integer","minimum":1},"diameter":positive,
        "depth":positive,"through_all":{"type":"boolean"},"thread":text,"note":text,"source_feature_id":{"type":"integer","minimum":1},"feature_name":text,
        "hole_style":choice(&["simple","counterbore","countersink"]),"counterbore_diameter":positive,"counterbore_depth":positive,
        "countersink_diameter":positive,"countersink_angle_deg":positive,"thread_depth":positive,"pattern_note":text}),
        &["feature", "position", "quantity", "diameter"],
        false,
        true,
    );
    add(
        "chamfer_note",
        json!({"first":anchor,"second":anchor,"position":position,"length":positive,"angle_deg":positive,"prefix":text}),
        &["first", "second", "position", "length", "angle_deg"],
        false,
        true,
    );
    add(
        "center_mark",
        json!({"feature":circle,"extension":number}),
        &["feature", "extension"],
        false,
        true,
    );
    add(
        "center_line",
        json!({"first":circle,"second":circle,"extension":number}),
        &["first", "second", "extension"],
        false,
        true,
    );
    add(
        "center_line_between_edges",
        json!({"first":line,"second":line,"extension":number}),
        &["first", "second", "extension"],
        false,
        true,
    );
    add(
        "automatic_symmetry_axis",
        json!({"axis":choice(&["x","y","both"]),"extension":number}),
        &["axis", "extension"],
        false,
        true,
    );
    add(
        "bolt_circle_center_line",
        json!({"features":{"type":"array","items":circle,"minItems":3,"maxItems":2048},"extension":number}),
        &["features", "extension"],
        false,
        true,
    );
    add(
        "chain_dimension",
        json!({"anchors":{"type":"array","items":anchor,"minItems":2,"maxItems":2048},"mode":choice(&["aligned","horizontal","vertical"]),"layout":choice(&["chain","baseline","continued"]),"offset":number,"spacing":positive}),
        &["anchors", "mode", "layout", "offset", "spacing"],
        true,
        true,
    );
    add(
        "ordinate_dimension",
        json!({"origin":anchor,"target":anchor,"axis":choice(&["x","y","both"]),"offset":number}),
        &["origin", "target", "axis", "offset"],
        true,
        true,
    );
    add(
        "arc_length_dimension",
        json!({"feature":circle,"first":anchor,"second":anchor,"offset":number}),
        &["feature", "first", "second", "offset"],
        true,
        true,
    );
    add(
        "jogged_radius_dimension",
        json!({"feature":circle,"jog":position,"position":position}),
        &["feature", "jog", "position"],
        true,
        true,
    );
    add(
        "datum_feature",
        json!({"attachment":attachment,"label":text,"position":position,"target_index":{"type":"integer","minimum":1}}),
        &["attachment", "label", "position"],
        false,
        true,
    );
    add(
        "gdt_frame",
        json!({"attachment":attachment,"position":position,
        "characteristic":choice(&["straightness","flatness","circularity","cylindricity","profile_line","profile_surface","angularity","perpendicularity","parallelism","position","concentricity","symmetry","circular_runout","total_runout"]),
        "tolerance":positive,"diameter_zone":{"type":"boolean"},"material_condition":choice(&["none","maximum","least","regardless"]),
        "datums":{"type":"array","maxItems":8,"items":object_schema(json!({"label":text,"material_condition":choice(&["none","maximum","least","regardless"])}), &["label"])},"projected_zone":positive,"free_state":{"type":"boolean"}}),
        &["attachment", "position", "characteristic", "tolerance"],
        false,
        true,
    );
    add(
        "surface_texture",
        json!({"attachment":attachment,"position":position,"roughness_ra":positive,"process":text,"lay":choice(&["none","parallel","perpendicular","crossed","multidirectional","circular","radial","particulate"]),"machining_allowance":number}),
        &["attachment", "position", "roughness_ra"],
        false,
        true,
    );
    add(
        "edge_requirement",
        json!({"attachment":line,"position":position,"upper_deviation":number,"lower_deviation":number,"note":text}),
        &[
            "attachment",
            "position",
            "upper_deviation",
            "lower_deviation",
        ],
        false,
        true,
    );
    add(
        "weld_symbol",
        json!({"attachment":line,"position":position,"weld_type":choice(&["fillet","square_groove","v_groove","bevel_groove","u_groove","j_groove","plug_slot","spot","seam","surfacing"]),"side":choice(&["arrow","other","both"]),
        "size":positive,"length":positive,"pitch":positive,"contour":choice(&["none","flush","convex","concave"]),"finish":text,"all_around":{"type":"boolean"},"field_weld":{"type":"boolean"},"tail":text}),
        &["attachment", "position", "weld_type", "side", "size"],
        false,
        true,
    );
    add(
        "item_balloon",
        json!({"attachment":attachment,"position":position,"bom_item_id":{"type":"integer","minimum":1}}),
        &["attachment", "position", "bom_item_id"],
        false,
        true,
    );
    add(
        "revision_cloud",
        json!({"revision":text,"points":{"type":"array","items":position,"minItems":3,"maxItems":2048}}),
        &["revision", "points"],
        false,
        false,
    );
    for variant in &mut variants {
        let kind = variant["properties"]["kind"]["enum"][0].as_str().unwrap();
        if [
            "ordinate_dimension",
            "arc_length_dimension",
            "jogged_radius_dimension",
        ]
        .contains(&kind)
        {
            variant["properties"]
                .as_object_mut()
                .unwrap()
                .remove("prefix");
            variant["properties"]
                .as_object_mut()
                .unwrap()
                .remove("suffix");
        }
        for field in [
            "depth",
            "through_all",
            "source_feature_id",
            "counterbore_diameter",
            "counterbore_depth",
            "countersink_diameter",
            "countersink_angle_deg",
            "thread_depth",
            "target_index",
            "projected_zone",
            "machining_allowance",
            "length",
            "pitch",
        ] {
            if let Some(schema) = variant["properties"].get_mut(field) {
                *schema = json!({"oneOf":[schema.clone(),{"type":"null"}]});
            }
        }
    }
    json!({"oneOf":variants})
}
