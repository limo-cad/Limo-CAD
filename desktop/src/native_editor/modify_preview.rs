//! Modification previews use the engine's exact curve construction. They do
//! not mutate a sketch, add history entries, or invent solver results.
use super::*;
use crate::native_viewport::{ViewportColorRole, ViewportLineLayer};
use limo_cad_sketch::{FilletPreviewDto, OffsetPreviewDto, PreviewCurve, TrimPreviewDto};
use serde::de::DeserializeOwned;

fn query<T: DeserializeOwned>(
    engine: &AppState,
    method: &str,
    arguments: &Value,
) -> Result<T, String> {
    let result: Value = serde_json::from_str(&engine.engine_call(method, &arguments.to_string()))
        .map_err(|e| e.to_string())?;
    if result["ok"] != true {
        return Err(result["error"]
            .as_str()
            .unwrap_or("Cannot preview this operation")
            .into());
    }
    serde_json::from_value(result["value"].clone()).map_err(|e| e.to_string())
}

fn curve_points(curve: PreviewCurve) -> Vec<[SketchPoint; 2]> {
    let (center, radius, start, sweep) = match curve {
        PreviewCurve::Line { a, b } => return vec![[a, b]],
        PreviewCurve::Circle { center, radius } => (center, radius, 0., std::f64::consts::TAU),
        PreviewCurve::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let sweep = (end_angle - start_angle).rem_euclid(std::f64::consts::TAU);
            (
                center,
                radius,
                start_angle,
                if sweep < 1e-12 {
                    std::f64::consts::TAU
                } else {
                    sweep
                },
            )
        }
    };
    let count = (sweep / std::f64::consts::TAU * 192.)
        .ceil()
        .clamp(12., 192.) as usize;
    let at = |i: usize| {
        let a = start + sweep * i as f64 / count as f64;
        center + SketchPoint::new(a.cos(), a.sin()) * radius
    };
    (0..count).map(|i| [at(i), at(i + 1)]).collect()
}

fn layer(
    basis: PlaneBasis,
    curves: impl IntoIterator<Item = PreviewCurve>,
    color: [f32; 4],
    color_role: ViewportColorRole,
) -> Result<ViewportLineLayer, String> {
    let segments: Vec<_> = curves
        .into_iter()
        .flat_map(curve_points)
        .flatten()
        .flat_map(|p| basis.to_3d([p.x, p.y]).map(|v| v as f32))
        .collect();
    if segments.iter().any(|v| !v.is_finite()) {
        return Err("Preview is outside the renderer's coordinate range".into());
    }
    Ok(ViewportLineLayer {
        color,
        color_role,
        width: 2.,
        segments: segments.into(),
        ..default()
    })
}

pub(super) fn form(
    engine: &AppState,
    basis: PlaneBasis,
    command: &Prepared,
) -> Result<Option<ViewportPreview>, String> {
    let curve = match command.operation {
        "sketch_fillet" => {
            let p: FilletPreviewDto = query(engine, "fillet_preview", &command.arguments)?;
            let (start_angle, end_angle) = if p.ccw {
                (p.start_angle, p.end_angle)
            } else {
                (p.end_angle, p.start_angle)
            };
            PreviewCurve::Arc {
                center: p.center,
                radius: p.radius,
                start_angle,
                end_angle,
            }
        }
        "sketch_chamfer" => query(engine, "chamfer_preview", &command.arguments)?,
        "sketch_offset" => {
            query::<OffsetPreviewDto>(engine, "offset_preview", &command.arguments)?.curve
        }
        _ => return Ok(None),
    };
    Ok(Some(ViewportPreview {
        lines: vec![layer(
            basis,
            [curve],
            [1.; 4],
            ViewportColorRole::SketchPreview,
        )?],
        ..default()
    }))
}

pub(super) fn refresh_form(
    world: &mut World,
    engine: &AppState,
    owner: &DocumentContext,
    editor: &mut Editor,
    sketch: &SketchDto,
) -> Result<(), String> {
    let Some(draft) = &editor.interaction.form else {
        return Ok(());
    };
    if !matches!(
        draft.kind,
        FormKind::Fillet | FormKind::Chamfer | FormKind::Offset
    ) {
        return Ok(());
    }
    let preview = match draft.request(
        sketch,
        &editor.interaction.selection,
        engine.document_units(),
    ) {
        Ok(command) => match form(engine, sketch.basis, &command) {
            Ok(preview) => preview.unwrap_or_default(),
            Err(error) => {
                editor.error = error;
                ViewportPreview::default()
            }
        },
        Err(_) => ViewportPreview::default(),
    };
    native_viewport::apply_interface_preview(world, &owner.document_id, preview)
}

pub(super) fn trim(
    engine: &AppState,
    sketch: &SketchDto,
    id: limo_cad_sketch::EntityId,
    point: SketchPoint,
) -> Result<ViewportPreview, String> {
    let p: TrimPreviewDto = query(engine, "trim_preview", &json!({"entity":id,"click":point}))?;
    Ok(ViewportPreview {
        lines: vec![
            layer(
                sketch.basis,
                p.kept,
                [1.; 4],
                ViewportColorRole::SketchPreview,
            )?,
            layer(
                sketch.basis,
                [p.removed],
                [0.88, 0.33, 0.33, 1.],
                ViewportColorRole::Explicit,
            )?,
        ],
        ..default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_trim_preview_themes_kept_geometry_but_preserves_removed_warning_and_model() {
        use crate::app_preferences::{palette::viewport_palette, ResolvedTheme};
        let engine = AppState::new();
        query::<Value>(
            &engine,
            "begin_sketch",
            &json!({"type":"origin_plane","plane":"xy"}),
        )
        .unwrap();
        for (a, b) in [([-20., 0.], [20., 0.]), ([0., -10.], [0., 10.])] {
            query::<Value>(
                &engine,
                "add_line",
                &json!({
                    "from":{"x":a[0],"y":a[1]}, "to_raw":{"x":b[0],"y":b[1]}, "ctrl_held":true
                }),
            )
            .unwrap();
        }
        let sketch = active(&engine).unwrap().unwrap();
        let line = sketch
            .entities
            .iter()
            .find(|entity| {
                matches!(entity,
                    limo_cad_sketch::EntityDto::Line { start, end, .. } if start.y == 0. && end.y == 0.
                )
            })
            .unwrap()
            .id();
        let model = engine.engine_call("project_export_model", "");
        let revision = engine.geometry_revision();
        let preview = trim(&engine, &sketch, line, SketchPoint::new(-10., 0.)).unwrap();
        assert_eq!(preview.lines.len(), 2);
        let kept = &preview.lines[0];
        let removed = &preview.lines[1];
        assert!(!kept.segments.is_empty());
        assert!(!removed.segments.is_empty());
        assert_eq!(kept.color_role, ViewportColorRole::SketchPreview);
        assert_eq!(removed.color_role, ViewportColorRole::Explicit);
        for theme in [ResolvedTheme::Dark, ResolvedTheme::Light] {
            let palette = viewport_palette(theme);
            assert_eq!(
                kept.color_role.resolve(kept.color, palette),
                [
                    palette.preview[0],
                    palette.preview[1],
                    palette.preview[2],
                    1.
                ]
            );
            assert_eq!(
                removed.color_role.resolve(removed.color, palette),
                [0.88, 0.33, 0.33, 1.]
            );
        }
        assert_eq!(active(&engine).unwrap().unwrap(), sketch);
        assert_eq!(engine.geometry_revision(), revision);
        assert_eq!(engine.engine_call("project_export_model", ""), model);
    }

    #[test]
    fn wrapped_arc_uses_its_short_sweep_and_closes_circles() {
        let lines = curve_points(PreviewCurve::Arc {
            center: SketchPoint::ZERO,
            radius: 10.,
            start_angle: 350_f64.to_radians(),
            end_angle: 10_f64.to_radians(),
        });
        assert!(lines.iter().flatten().all(|p| p.x > 9.8));
        let lines = curve_points(PreviewCurve::Circle {
            center: SketchPoint::ZERO,
            radius: 10.,
        });
        assert!(lines[0][0].distance(lines.last().unwrap()[1]) < 1e-10);
    }
}
