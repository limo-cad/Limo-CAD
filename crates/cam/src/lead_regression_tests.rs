//! Synthetic adverse inputs for lead geometry and stock-clearance regression tests.
use crate::post::post_setup_unchecked as post_setup;
use crate::*;

fn fixture(name: &str) -> CamDocumentDto {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/lead-clearance.json")).unwrap();
    serde_json::from_value(
        data["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap()["document"]
            .clone(),
    )
    .unwrap()
}

#[test]
fn unsafe_leads_and_hole_heights_fail_before_posting() {
    for name in [
        "picked-hole-high-top",
        "thin-wall-between-lead-samples",
        "in-control-six-mm-tool-four-mm-slot",
    ] {
        let document = fixture(name);
        let error = plan_setup(&document, 1).expect_err(name);
        assert!(
            error.0.contains("top") || error.0.contains("profile"),
            "{name}: {error}"
        );
        for dialect in [
            PostDialect::Siemens828d,
            PostDialect::Fanuc,
            PostDialect::LinuxCnc,
        ] {
            let post = CamPostConfigDto {
                dialect,
                siemens_828d: Some(Siemens828dPostConfigDto::default()),
                ..Default::default()
            };
            assert!(
                post_setup(
                    &document,
                    &CamPostRequestDto {
                        setup_id: 1,
                        post: Some(post),
                        program_name: None
                    }
                )
                .is_err(),
                "{name}: {dialect:?}"
            );
        }
    }
}

#[test]
fn custom_facing_respects_incoming_top_and_never_returns_low() {
    let document = fixture("face-inner-bounds-low-feed");
    let setup = &document.setups[0];
    let program = plan_setup(&document, 1).unwrap();
    let mut position = None;
    let mut first_cut = None;
    let mut traverses = 0;
    for command in &program.commands {
        if let Some(to) = command.endpoint() {
            if let Some(from) = position {
                let from: Point3Dto = from;
                if matches!(command, CamCommandDto::Rapid { .. })
                    && ((from.x - to.x).abs() > 1e-8 || (from.y - to.y).abs() > 1e-8)
                {
                    assert!(from.z > setup.stock.max.z && to.z > setup.stock.max.z);
                    traverses += 1;
                }
                if matches!(command, CamCommandDto::Linear { .. }) && to.z < setup.stock.max.z {
                    first_cut.get_or_insert(to.z);
                }
            }
            position = Some(to);
        }
    }
    let CamOperationDto::Face { step_down, .. } = setup.operations[0] else {
        unreachable!()
    };
    assert!(first_cut.unwrap() >= setup.stock.max.z - step_down - 1e-8);
    assert!(traverses > 2);
}

fn tangents(from: Point3Dto, command: &CamCommandDto) -> Option<([f64; 2], [f64; 2])> {
    let to = command.endpoint()?;
    if (to.z - from.z).abs() > 1e-8 {
        return None;
    }
    let unit = |x: f64, y: f64| {
        let length = x.hypot(y);
        [x / length, y / length]
    };
    match command {
        CamCommandDto::Linear { .. } if (to.x - from.x).hypot(to.y - from.y) > 1e-8 => {
            let t = unit(to.x - from.x, to.y - from.y);
            Some((t, t))
        }
        CamCommandDto::Circular {
            center, clockwise, ..
        } => {
            let sign = if *clockwise { -1.0 } else { 1.0 };
            Some((
                unit(-sign * (from.y - center.y), sign * (from.x - center.x)),
                unit(-sign * (to.y - center.y), sign * (to.x - center.x)),
            ))
        }
        _ => None,
    }
}

#[test]
fn inside_entry_and_exit_are_tangent_in_both_directions_and_compensation_modes() {
    for direction in [MillingDirection::Climb, MillingDirection::Conventional] {
        for mode in [CompensationMode::InSoftware, CompensationMode::InControl] {
            for arc in [None, Some(1.0)] {
                let mut document = fixture("inside-software-nontangent");
                if let CamOperationDto::Contour2d {
                    direction: d,
                    compensation_mode,
                    lead_arc_radius,
                    ..
                } = &mut document.setups[0].operations[0]
                {
                    *d = direction;
                    *compensation_mode = mode;
                    *lead_arc_radius = arc;
                }
                let program = plan_setup(&document, 1).unwrap();

                if mode == CompensationMode::InControl {
                    let request: CamSimulationRequestDto =
                        serde_json::from_value(serde_json::json!({"setup_id":1,"voxel_size":1.0}))
                            .unwrap();
                    simulate_setup(&document, &request).unwrap();
                    continue;
                }
                let mut position = None;
                let mut horizontal = Vec::new();
                for command in &program.commands {
                    if let Some(from) = position {
                        if let Some(t) = tangents(from, command) {
                            horizontal.push(t);
                        }
                    }
                    if let Some(to) = command.endpoint() {
                        position = Some(to);
                    }
                }
                for i in [0, horizontal.len() - 2] {
                    let (a, b) = (horizontal[i].1, horizontal[i + 1].0);
                    assert!(
                        a[0] * b[0] + a[1] * b[1] > 1.0 - 1e-8,
                        "{direction:?}, {arc:?}: {a:?} -> {b:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn high_speed_links_have_no_horizontal_tangent_breaks() {
    let program = plan_setup(&fixture("high-speed-join-angles"), 1).unwrap();
    let mut position = None;
    let mut previous = None;
    let mut junctions = 0;
    let mut mixed = 0;
    for command in &program.commands {
        let t = position.and_then(|from| tangents(from, command));
        let is_line = matches!(command, CamCommandDto::Linear { .. });
        if let (Some((before, before_line)), Some((after, _))) = (previous, t) {
            let before: [f64; 2] = before;
            assert!(
                before[0] * after[0] + before[1] * after[1] > 1.0 - 1e-8,
                "non-tangent link at {command:?}"
            );
            junctions += 1;
            mixed += usize::from(before_line != is_line);
        }
        previous = t.map(|(_, end)| (end, is_line));
        if let Some(to) = command.endpoint() {
            position = Some(to);
        }
    }
    assert!(
        junctions >= 64 && mixed >= 32 && program.commands.len() < 200,
        "must retain continuous line/arc joins with a compact exterior program: {junctions}, {mixed}"
    );
}

#[test]
fn chip_breaking_honors_small_and_large_explicit_lifts() {
    for lift in [0.1, 0.5, 1.2] {
        let mut document = fixture("chip-breaking-0.1-retract");
        if let CamOperationDto::Drill { peck_retract, .. } = &mut document.setups[0].operations[0] {
            *peck_retract = Some(lift);
        }
        let program = plan_setup(&document, 1).unwrap();
        let index = program
            .commands
            .iter()
            .position(|c| matches!(c, CamCommandDto::Linear { to, .. } if (to.z+2.0).abs()<1e-8))
            .unwrap();
        assert!(
            matches!(program.commands[index+1], CamCommandDto::Rapid { to } if (to.z-(-2.0+lift)).abs()<1e-8)
        );
        assert!(matches!(
            program.commands[index + 2],
            CamCommandDto::Linear { .. }
        ));
    }
}
