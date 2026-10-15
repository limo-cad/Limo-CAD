//! Read-only selection previews. Enter or a placement click commits one mutation.
use super::*;
use limo_cad_core::EdgeId;
use limo_cad_sketch::{DimensionDto, DimensionPreviewRequest, ProjectedEdgeDto};

/// Keep generated values readable in the selected inline field. Preserve the
/// exact value separately so accepting a rounded suggestion never resizes it.
pub(super) fn value_label(value: f64) -> String {
    if value == 0. {
        return "0".into();
    }
    let scientific = format!("{value:.8e}");
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return value.to_string();
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return value.to_string();
    };
    if !(-3..=8).contains(&exponent) {
        format!(
            "{}e{exponent}",
            mantissa.trim_end_matches('0').trim_end_matches('.')
        )
    } else {
        let fixed = format!("{value:.precision$}", precision = (8 - exponent) as usize);
        if fixed.contains('.') {
            fixed.trim_end_matches('0').trim_end_matches('.').into()
        } else {
            fixed
        }
    }
}

pub(super) fn begin_value(state: &mut interaction::Interaction, dimension: &DimensionDto) {
    state.dimension = Some(
        dimension
            .param_expression
            .clone()
            .unwrap_or_else(|| value_label(dimension.value)),
    );
    state.dimension_initial_value = Some(
        dimension
            .param_expression
            .clone()
            .unwrap_or_else(|| dimension.value.to_string()),
    );
    state.dimension_edited = false;
}

pub(super) fn committed_value(state: &interaction::Interaction) -> Option<&str> {
    if !state.dimension_edited {
        if let Some(exact) = state.dimension_initial_value.as_deref() {
            return Some(exact);
        }
    }
    state.dimension.as_deref()
}

pub(super) fn refresh(engine: &AppState, editor: &mut Editor) -> Result<(), String> {
    let selection = &editor.interaction.selection;
    let sketch = active(engine)?.ok_or("No active sketch")?;
    let waiting = selection.is_empty()
        || (selection.len() == 1
            && editor.interaction.dimension_edge.is_none()
            && sketch.entities.iter().any(|e| {
                e.id() == selection[0] && matches!(e, limo_cad_sketch::EntityDto::Point { .. })
            }));
    let preview: Option<DimensionDto> = if waiting {
        None
    } else {
        let request = DimensionPreviewRequest {
            entities: selection.clone(),
            reference_edge: editor.interaction.dimension_edge,
        };
        let value = crate::session_bridge::parse_engine_envelope(engine.engine_call(
            "preview_dimension",
            &serde_json::to_string(&request).map_err(|e| e.to_string())?,
        ))?;
        Some(serde_json::from_value(value).map_err(|e| e.to_string())?)
    };
    editor.form_serial = editor
        .form_serial
        .checked_add(1)
        .ok_or("Editor generation exhausted")?;
    if let Some(dimension) = &preview {
        begin_value(&mut editor.interaction, dimension);
    } else {
        editor.interaction.dimension = Some(String::new());
        editor.interaction.dimension_initial_value = None;
        editor.interaction.dimension_edited = false;
    }
    editor.interaction.dimension_position = preview.as_ref().map(|d| d.text_pos);
    editor.interaction.dimension_id = preview
        .as_ref()
        .and_then(|d| (d.constraint_id.0 != 0).then_some(d.constraint_id));
    editor.interaction.dimension_reference = preview
        .as_ref()
        .is_some_and(|d| d.mode == limo_cad_sketch::DimensionMode::Reference);
    editor.interaction.dimension_preview = preview;
    Ok(())
}

pub(super) fn follow_cursor(
    state: &mut interaction::Interaction,
    sketch: &SketchDto,
    cursor: SketchPoint,
) -> bool {
    if !state.dimension_tool || state.dimension.is_none() {
        return false;
    }
    let Some(preview) = &mut state.dimension_preview else {
        return false;
    };
    let Some(position) = annotations::preview_position(preview, sketch, cursor) else {
        return false;
    };
    if state
        .dimension_position
        .is_some_and(|old| old.distance(position) < 1e-9)
    {
        return false;
    }
    state.dimension_position = Some(position);
    preview.text_pos = position;
    true
}

pub(super) fn boundary_hit(
    edges: &[ProjectedEdgeDto],
    cursor: [f32; 2],
    mut project: impl FnMut(SketchPoint) -> Option<[f32; 2]>,
) -> Option<EdgeId> {
    edges
        .iter()
        .filter_map(|edge| {
            let (a, b) = edge.straight_line()?;
            let a = project(a)?;
            let b = project(b)?;
            let d = [b[0] - a[0], b[1] - a[1]];
            let n = d[0] * d[0] + d[1] * d[1];
            let t = if n > f32::EPSILON {
                (((cursor[0] - a[0]) * d[0] + (cursor[1] - a[1]) * d[1]) / n).clamp(0., 1.)
            } else {
                0.
            };
            let distance = (cursor[0] - a[0] - t * d[0]).hypot(cursor[1] - a[1] - t * d[1]);
            (distance <= 7.).then_some((edge.edge_id, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(edge, _)| edge)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_suggestions_are_readable_without_losing_the_accepted_value_or_expression() {
        for (value, expected) in [
            (6.53272650918237, "6.53272651"),
            (20., "20"),
            (-12.3456789012, "-12.3456789"),
            (0., "0"),
            (-0., "0"),
            (0.000123456789123, "1.23456789e-4"),
            (1.23456789123e100, "1.23456789e100"),
        ] {
            let mut dimension: DimensionDto = serde_json::from_value(json!({
                "constraint_id":0,"mode":"driving","kind":"distance",
                "entities":[3],"value":value,"text":"","text_pos":{"x":0.,"y":0.}
            }))
            .unwrap();
            let mut state = interaction::Interaction::default();
            begin_value(&mut state, &dimension);
            assert_eq!(state.dimension.as_deref(), Some(expected));
            assert_eq!(
                committed_value(&state).unwrap().parse::<f64>().unwrap(),
                value
            );
            dimension.param_expression = Some("width / 3".into());
            begin_value(&mut state, &dimension);
            assert_eq!(state.dimension.as_deref(), Some("width / 3"));
            assert_eq!(committed_value(&state), Some("width / 3"));
        }
        // Even the longest finite f64 suggestions fit the numeric editor;
        // small nonzero values never turn into a displayed zero.
        for value in [f64::MAX, -f64::MAX, f64::MIN_POSITIVE, f64::from_bits(1)] {
            let label = value_label(value);
            assert!(label.len() <= 16, "{label}");
            assert_ne!(label.parse::<f64>().unwrap(), 0.);
        }
    }

    #[test]
    fn support_edge_picking_is_finite_zoom_independent_and_returns_stable_topology() {
        let edges = [ProjectedEdgeDto {
            id: 1 << 40,
            edge_id: EdgeId(8),
            points: vec![SketchPoint::new(0., 10.), SketchPoint::new(100., 10.)],
            circle: None,
        }];
        for zoom in [0.1_f32, 1., 10.] {
            let project = |p: SketchPoint| Some([p.x as f32 * zoom, p.y as f32 * zoom]);
            assert_eq!(
                boundary_hit(&edges, [50. * zoom, 10. * zoom + 6.], project),
                Some(EdgeId(8))
            );
            assert_eq!(
                boundary_hit(&edges, [50. * zoom, 10. * zoom + 8.], project),
                None
            );
            assert_eq!(
                boundary_hit(&edges, [100. * zoom + 8., 10. * zoom], project),
                None
            );
        }
    }
}
