use super::*;

pub(super) fn present(world: &mut World, owner: &DocumentContext) -> Result<(), String> {
    let state = world.resource::<State>();
    let row_list = rows(state, state.units);
    let hovered = row_list
        .iter()
        .enumerate()
        .find(|(i, _)| {
            state
                .widgets
                .entity(&format!("measure-row-{i}"))
                .is_some_and(|e| {
                    world
                        .get::<bevy::picking::hover::Hovered>(e)
                        .is_some_and(|h| h.0)
                })
        })
        .map(|(_, r)| r.guide);
    let key = OverlayKey {
        generation: state.generation,
        units: state.units,
        hovered,
        hover_points: state.hover_points.clone(),
    };
    if state.overlay_key.as_ref() == Some(&key) {
        return apply_overlay(
            world,
            owner,
            state.overlay_lines.clone(),
            state.overlay_points.clone(),
        );
    }
    let mut lines = vec![];
    let mut points = vec![];
    let mut line = |a: Vec3, b: Vec3, color, label| {
        lines.push(ViewportMeasurementLine {
            points: [a.x, a.y, a.z, b.x, b.y, b.z],
            color,
            label,
        });
    };
    let accent = [1., 0.78, 0.12, 1.];
    if state.visible {
        points.clone_from(&state.hover_points);
        for (index, s) in state.selections.iter().enumerate() {
            let transform =
                native_viewport::interface_body_transform(world, s.body_id.0, s.occurrence_id);
            let Some(body) = native_viewport::interface_geometry(world)
                .scene
                .bodies
                .iter()
                .find(|b| b.id == s.body_id)
            else {
                continue;
            };
            let color = if index == 0 {
                accent
            } else {
                [0.35, 0.8, 1., 1.]
            };
            let highlight = hovered.is_none_or(|g| !matches!(g,Guide::Selection(i) if i != index));
            if !highlight {
                continue;
            }
            let at = |p: &limo_cad_solid::Point3Dto| {
                transform.transform_point(Vec3::new(p.x as f32, p.y as f32, p.z as f32))
            };
            for edge in &body.edges {
                let include = match s.target {
                    MeasurementTarget::Body => true,
                    MeasurementTarget::Face { face_id } => body
                        .faces
                        .iter()
                        .any(|f| f.id == face_id && f.edge_keys.contains(&edge.key)),
                    MeasurementTarget::Edge { edge_id } => edge.id == edge_id,
                    MeasurementTarget::Vertex { edge_id, end } if edge.id == edge_id => {
                        if let Some(p) = if end {
                            edge.points.last()
                        } else {
                            edge.points.first()
                        } {
                            points.push(at(p).to_array());
                        }
                        false
                    }
                    MeasurementTarget::Center { edge_id } if edge.id == edge_id => {
                        if let Some(c) = edge.circle {
                            points.push(at(&c.center).to_array());
                        }
                        false
                    }
                    _ => false,
                };
                if include {
                    for pair in edge.points.windows(2) {
                        line(at(&pair[0]), at(&pair[1]), color, None);
                    }
                }
            }
        }
        if let Some(report) = &state.report {
            let (connection, value, prefix) = match hovered {
                Some(Guide::Centers) => (report.center_points, report.center_distance, "Center: "),
                Some(Guide::Maximum) => (report.maximum_points, report.maximum_distance, "Max: "),
                Some(Guide::Objects | Guide::Selection(_)) => (None, None, ""),
                _ => (report.closest_points, report.distance, ""),
            };
            if let Some([a, b]) = connection {
                let a = DVec3::from_array(a);
                let b = DVec3::from_array(b);
                let value = value
                    .filter(|v| v.is_finite())
                    .unwrap_or_else(|| a.distance(b));
                line(
                    a.as_vec3(),
                    b.as_vec3(),
                    accent,
                    Some(format!(
                        "{prefix}{}",
                        readout::format_value(value, 1, state.units, state)
                    )),
                );
                points.push(a.as_vec3().to_array());
                points.push(b.as_vec3().to_array());
                if state.delta != "none"
                    && !matches!(hovered, Some(Guide::Centers | Guide::Maximum))
                {
                    let rotation = state.delta_rotation();
                    let delta = rotation.inverse() * (b - a);
                    let mut start = a;
                    for axis in 0..3 {
                        let mut step = DVec3::ZERO;
                        step[axis] = delta[axis];
                        let end = start + rotation * step;
                        if !matches!(hovered,Some(Guide::Delta(i)) if i != axis) {
                            line(
                                start.as_vec3(),
                                end.as_vec3(),
                                [[1., 0.3, 0.3, 1.], [0.3, 1., 0.4, 1.], [0.35, 0.6, 1., 1.]][axis],
                                Some(format!(
                                    "Δ {}: {}",
                                    ["X", "Y", "Z"][axis],
                                    readout::format_value(delta[axis], 1, state.units, state),
                                )),
                            );
                        }
                        start = end;
                    }
                }
            }
        }
    }
    let lines = Arc::new(lines);
    let points = Arc::new(points);
    let mut state = world.resource_mut::<State>();
    state.overlay_key = Some(key);
    state.overlay_lines = lines.clone();
    state.overlay_points = points.clone();
    apply_overlay(world, owner, lines, points)
}
fn apply_overlay(
    world: &mut World,
    owner: &DocumentContext,
    lines: Arc<Vec<ViewportMeasurementLine>>,
    points: Arc<Vec<[f32; 3]>>,
) -> Result<(), String> {
    let (_, _, mut view, _) = native_viewport::interface_view_snapshot(world);
    if view.measurement_lines != lines || view.measurement_points != points {
        view.measurement_lines = lines;
        view.measurement_points = points;
        native_viewport::apply_interface_view(world, &owner.document_id, None, Some(view))?;
    }
    Ok(())
}
