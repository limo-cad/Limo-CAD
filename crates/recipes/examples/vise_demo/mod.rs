//! Demonstrate the authored mechanism through its actual screw joint.
use super::*;

/// Sketch Finish restores the previous solid camera. Reframe important stock
/// explicitly, and isolate internal parts that an assembled jaw would obscure.
pub(super) fn construction(a: &mut Author, id: &str, part: &str) {
    let caption = match id {
        "Frame deck / 230 by 160 by 14" => "The 230 by 160 mm deck is 14 mm thick. This is the base for the fixed jaw and two captured guides.",
        "Fixed jaw / 100 mm face and 25 mm stock" => "The fixed jaw grows from the deck. Its 100 mm face and 25 mm stock carry the closing load.",
        "Captured rail right / 45 degree flanks" => "Both captured rails are now joined to the deck. Their 45 degree flanks retain the moving carriage.",
        "Carriage / 72 mm captured bearing length" => "The moving carriage begins as separate stock with 72 mm of bearing length.",
        "Moving jaw / 100 mm gripping face" => "The 100 mm gripping face joins the carriage. The next features add support and running clearance.",
        "Jaw dovetail right / profile clearance" => "The paired channels retain the carriage on its guides, with 0.4 mm nominal profile clearance to qualify in print.",
        "Threaded bridge / central housing" => "This separate bridge will carry the wear thread. Its keyed feet and retaining bolts come next.",
        "Screw / 24 mm rounded-thread blank" => "The 24 mm screw blank establishes the drive axis. The printed thread and shallow flat are added later.",
        "Screw grip / thick comfortable T profile" => "The compact T-grip joins the shaft as one printed part. Its upper transitions and touched rims are rounded next.",
        "Thrust fitting / 32 mm load head" => "The removable thrust fitting has a full round load head. Its keyed socket and captive-nut support are still to be cut.",
        "Keeper / 11.2 mm cross plate" => "The keeper starts as a separate cross plate. It will drop over the thrust sleeve and bear on the jaw shoulders.",
        "Keeper / downward installation throat" => "The open throat lets the keeper lower over the sleeve. The cross-pin retains it after the bearing faces seat.",
        _ => return,
    };
    show_part(a, id, part, caption);
}

pub(super) fn show_part(a: &mut Author, id: &str, part: &str, caption: &str) {
    let saved = format!("{id}_presentation_visibility");
    a.call(
        &saved,
        "document/appearance",
        "project_visibility",
        json!({}),
    );
    let hidden: Vec<_> = a
        .bodies
        .keys()
        .filter(|name| name.as_str() != part)
        .map(|name| a.body_id(name))
        .collect();
    a.call(
        &format!("{id}_presentation_isolate"),
        "document/appearance",
        "project_set_visibility",
        json!({
            "hidden_body_ids":hidden,
            "hidden_sketch_names":reference(&saved,"/hidden_sketch_names"),
            "hidden_datum_plane_ids":reference(&saved,"/hidden_datum_plane_ids")
        }),
    );
    a.steps.push(
        json!({"id":format!("{id}_presentation_fit"),"view":"isometric","fit":true,
        "body_id":a.body_id(part),"duration_ms":600}),
    );

    a.steps
        .push(json!({"id":format!("{id}_presentation_read"),"note":caption,"duration_ms":1200}));
    a.call(
        &format!("{id}_presentation_restore"),
        "document/appearance",
        "project_set_visibility",
        reference(&saved, ""),
    );

    a.steps
        .push(json!({"id":format!("{id}_presentation_restore_fit"),
        "view":"isometric","fit":true,"duration_ms":600}));
}

pub(super) fn run(a: &mut Author) {
    a.call(
        "vise_demo_home",
        "assembly/joints",
        "assembly_document",
        json!({}),
    );
    a.steps
        .push(json!({"view":"isometric","fit":true,"duration_ms":650}));
    a.note("Turn the screw; the jaw follows", "The 4 mm lead converts twelve turns into 48 mm of jaw travel. The captured guides prevent jaw rotation. These are solved joint positions.");
    for (direction, samples) in [
        ("close", (1..=48).collect::<Vec<_>>()),
        ("return", (0..48).rev().collect::<Vec<_>>()),
    ] {
        if direction == "return" {
            a.note("Reverse to the original opening", "Reverse the same screw joint. The thrust fitting pulls the guided jaw back to its original 90 mm opening.");
        }
        for sample in samples {
            let id = format!("vise_demo_{direction}_{sample:02}");
            a.call(
                &id,
                "assembly/joints",
                "assembly_set_joint_motion",
                json!({
                    "joint_id":reference("screw_drive","/id"),
                    "angle_offset_deg":sample as f64 * 90., "linear_offset_mm":0.
                }),
            );

            a.steps
                .push(json!({"view":"current","fit":false,"duration_ms":120}));
        }
        let last = if direction == "close" {
            "vise_demo_close_48"
        } else {
            "vise_demo_return_00"
        };
        let travel = if direction == "close" { 48. } else { 0. };
        a.steps.push(json!({"assert":select(reference(last,""),"/joints",json!({"/id":reference("jaw_guide","/id")}),"/linear_offset_mm"),"equals":travel}));
        let solution = format!("vise_demo_{direction}_solution");
        a.call(&solution, "assembly/joints", "assembly_solution", json!({}));
        a.steps.last_mut().unwrap()["expect"] = json!({"/solved":true,"/diagnostics":[]});
    }
    a.steps.push(json!({"assert":reference("vise_demo_return_00","/joints"),"equals":reference("vise_demo_home","/joints")}));

    a.call(
        "vise_demo_visibility",
        "document/appearance",
        "project_visibility",
        json!({}),
    );
    let hidden: Vec<_> = a
        .bodies
        .keys()
        .filter(|part| part.as_str() != "frame")
        .map(|part| a.body_id(part))
        .collect();
    a.call(
        "vise_demo_reveal_guides",
        "document/appearance",
        "project_set_visibility",
        json!({
            "hidden_body_ids":hidden,
            "hidden_sketch_names":reference("vise_demo_visibility","/hidden_sketch_names"),
            "hidden_datum_plane_ids":reference("vise_demo_visibility","/hidden_datum_plane_ids")
        }),
    );
    a.steps.push(
        json!({"view":"isometric","fit":true,"body_id":a.body_id("frame"),"duration_ms":600}),
    );
    a.note("Inspect the captured guides", "Hide the moving parts to see the two dovetail rails and open rear entry. The keyed bridge is installed after the carriage slides onto these rails.");
    a.steps
        .push(json!({"view":"current","fit":false,"duration_ms":1700}));
    a.call(
        "vise_demo_restore_visibility",
        "document/appearance",
        "project_set_visibility",
        reference("vise_demo_visibility", ""),
    );
    a.steps
        .push(json!({"view":"isometric","fit":true,"duration_ms":600}));
}
