//! Reveal and drive the real gear relation without changing assembly placement.
use super::*;

/// Explicit stock framing prevents Finish Sketch from returning construction
/// to an old, distant assembly camera. IDs never consume the geometry serial.
pub(super) fn construction(a: &mut Author, name: &str, body: Value) {
    let caption = match name {
        "stage" => "The 198 mm rotor disc is extruded from its driving diameter. The shaft hub and bucket walls are added to this plate.",
        "stage_hub" => "The 24 mm hub stops at 18 mm height, leaving the bucket overlap open above it.",
        "bucket_half" => "A hollow cylinder becomes a 2 mm semicircular bucket wall. The second bucket is patterned about the shaft axis.",
        "stage_shaft_fit" => "Both bucket walls join the stage plate. An 8.3 mm bore provides nominal clearance for the 8 mm shaft.",
        "base" => "The base starts as printable stock. Bearing support, mounting holes and service access are added next.",
        "tower" => "The bearing carrier starts with its flange. The upper support and bearing seats share the same shaft axis.",
        "motor_mount_cavity" => "The generator cartridge has an open case cavity. Its provisional fit must be checked against the delivered motor.",
        "guard_wire_exit" => "The guard has a dedicated pointed wire exit, separate from the moving gears and clamp split.",
        "rotor_gear" => "The rotor gear starts with its root disc. A patterned involute will form the 72 teeth.",
        "pinion" => "The generator pinion starts with its smaller root disc. Its 18 teeth will share module 1 with the rotor gear.",
        _ => return,
    };
    show_body(a, name, body, caption);
}

pub(super) fn show_body(a: &mut Author, id: &str, body: Value, caption: &str) {
    let saved = format!("{id}_presentation_visibility");
    a.call(
        &saved,
        "document/appearance",
        "project_visibility",
        json!({}),
    );

    let mut hidden: Vec<_> = a.parts.iter().map(|part| part["body_id"].clone()).collect();
    if id == "bucket_half" {
        hidden.push(body_ref("stage"));
    }
    a.call(
        &format!("{id}_presentation_isolate"),
        "document/appearance",
        "project_set_visibility",
        json!({
            "hidden_body_ids":hidden,
            "hidden_sketch_names":at(&saved,"/hidden_sketch_names"),
            "hidden_datum_plane_ids":at(&saved,"/hidden_datum_plane_ids")
        }),
    );
    a.steps.push(
        json!({"id":format!("{id}_presentation_fit"),"view":"isometric","fit":true,
        "body_id":body,"duration_ms":600}),
    );
    a.steps
        .push(json!({"id":format!("{id}_presentation_read"),"note":caption,"duration_ms":1200}));
    a.call(
        &format!("{id}_presentation_restore"),
        "document/appearance",
        "project_set_visibility",
        r(&saved),
    );

    a.steps
        .push(json!({"id":format!("{id}_presentation_restore_fit"),
        "view":"isometric","fit":true,"duration_ms":600}));
}

pub(super) fn run(a: &mut Author) {
    a.call(
        "turbine_demo_home",
        "assembly/joints",
        "assembly_document",
        json!({}),
    );
    a.call(
        "turbine_demo_visibility",
        "document/appearance",
        "project_visibility",
        json!({}),
    );
    let drive = [
        "base",
        "tower",
        "motor_bracket",
        "motor_mount",
        "motor",
        "motor_shaft",
        "rotor_gear",
        "pinion",
    ];
    let hidden: Vec<_> = a
        .parts
        .iter()
        .filter(|part| !drive.contains(&part["id"].as_str().unwrap()))
        .map(|part| part["body_id"].clone())
        .collect();
    a.call(
        "turbine_demo_reveal_drive",
        "document/appearance",
        "project_set_visibility",
        json!({
            "hidden_body_ids":hidden,
            "hidden_sketch_names":at("turbine_demo_visibility","/hidden_sketch_names"),
            "hidden_datum_plane_ids":at("turbine_demo_visibility","/hidden_datum_plane_ids")
        }),
    );
    a.steps
        .push(json!({"view":"top","fit":true,"duration_ms":650}));
    a.note("See the 4:1 drive", "Hide the rotor and covers to inspect the gears. A 90 degree rotor turn drives the generator through one full reverse turn. Only the rotor joint is commanded.");
    for (direction, samples) in [
        ("drive", (1..=30).collect::<Vec<_>>()),
        ("return", (0..30).rev().collect::<Vec<_>>()),
    ] {
        if direction == "return" {
            a.note("Return through the same relation", "The 72:18 relation solves the generator position on every step, including the return to the original mesh phase.");
        }
        for sample in samples {
            let angle = sample as f64 * 3.;
            let id = format!("turbine_demo_{direction}_{sample:02}");
            a.call(&id, "assembly/joints", "assembly_set_joint_motion", json!({
                "joint_id":at("rotor_rotation","/id"),"angle_offset_deg":angle,"linear_offset_mm":0.
            }));

            a.steps
                .push(json!({"view":"current","fit":false,"duration_ms":160}));
        }
        let last = if direction == "drive" {
            "turbine_demo_drive_30"
        } else {
            "turbine_demo_return_00"
        };
        let generator_angle = if direction == "drive" { -350. } else { 10. };
        a.steps.push(json!({"assert":select(r(last),"/joints",json!({"/id":at("generator_rotation","/id")}),"one","/angle_offset_deg"),"equals":generator_angle}));
        let solution = format!("turbine_demo_{direction}_solution");
        a.call(&solution, "assembly/joints", "assembly_solution", json!({}));
        a.steps.last_mut().unwrap()["expect"] = json!({"/solved":true,"/diagnostics":[]});
    }
    a.steps.push(json!({"assert":at("turbine_demo_return_00","/joints"),"equals":at("turbine_demo_home","/joints")}));
    a.call(
        "turbine_demo_restore_visibility",
        "document/appearance",
        "project_set_visibility",
        r("turbine_demo_visibility"),
    );
    a.steps
        .push(json!({"view":"isometric","fit":true,"duration_ms":650}));
    a.note("Restore the complete assembly", "The rotor, guard and hardware are visible again, with the original joint positions restored. Fit, bearing drag and electrical output still need physical testing.");
}
