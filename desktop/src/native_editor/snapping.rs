//! Shared creation acquisition. Preview and click query the same engine with
//! view-dependent distances; each queued command keeps its acquisition context.
use super::*;
use limo_cad_sketch::{CreationPointPreviewRequest, PreviewDto, ViewportSnapContext};

pub(super) fn context(
    world: &mut World,
    basis: PlaneBasis,
    raw: SketchPoint,
) -> ViewportSnapContext {
    let (_, camera, _, size) = native_viewport::interface_view(world);
    let position = Vec3::from_array(camera.position);
    let forward = (Vec3::from_array(camera.target) - position).normalize_or_zero();
    let point = Vec3::from_array(basis.to_3d([raw.x, raw.y]).map(|v| v as f32));
    let depth = (point - position).dot(forward).max(0.2);
    let pixel = f64::from(
        2. * depth * (camera.vertical_fov_degrees.to_radians() * 0.5).tan() / size[1].max(1.),
    );
    context_for_pixel(pixel)
}

fn context_for_pixel(pixel: f64) -> ViewportSnapContext {
    let pixel = if pixel.is_finite() && pixel > 0. {
        pixel
    } else {
        10. / 24.
    };
    let desired = (pixel * 24.).max(0.001);
    let decade = 10_f64.powi(desired.log10().floor() as i32);
    let normalized = desired / decade;
    let multiplier = if normalized < 2_f64.sqrt() {
        1.
    } else if normalized < 10_f64.sqrt() {
        2.
    } else if normalized < 50_f64.sqrt() {
        5.
    } else {
        10.
    };
    ViewportSnapContext {
        grid_step_mm: (multiplier * decade).clamp(0.001, 1_000_000.),
        point_tolerance_mm: pixel * 14.,
        grid_capture_mm: pixel * 7.,
    }
}

pub(super) fn acquire(
    engine: &AppState,
    draft: &Draft,
    raw: SketchPoint,
    ctrl: bool,
    context: ViewportSnapContext,
) -> Result<PreviewDto, String> {
    let request = CreationPointPreviewRequest {
        raw,
        ctrl_held: ctrl,
        allow_midpoint: draft.tool != Some(CreateTool::Point),
        exclude_position: (draft.tool == Some(CreateTool::Line))
            .then(|| draft.points.last().copied())
            .flatten(),
    };
    let mut arguments = serde_json::to_value(request).map_err(|e| e.to_string())?;
    attach(&mut arguments, Some(context));
    let value = crate::session_bridge::parse_engine_envelope(
        engine.engine_call("preview_creation_point", &arguments.to_string()),
    )?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

pub(super) fn attach(arguments: &mut Value, context: Option<ViewportSnapContext>) {
    if let Some(context) = context {
        arguments["viewport_snap"] = serde_json::to_value(context).expect("finite snap context");
    }
}

/// Locked geometry and line inference must receive the same raw direction
/// hint used by their engine preview. Their resolver acquires the final point.
pub(super) fn pick(draft: &Draft, raw: SketchPoint, acquired: &PreviewDto) -> SketchPoint {
    let engine_hint = !draft.points.is_empty()
        && (draft.tool == Some(CreateTool::Line)
            || (draft.sizes.has_locks()
                && matches!(
                    draft.tool,
                    Some(CreateTool::Rectangle(_) | CreateTool::Circle(_))
                )));
    if engine_hint {
        raw
    } else {
        acquired.snapped_to
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_distances_remain_in_pixels_while_the_grid_tracks_zoom() {
        for (pixel, step) in [
            (0.0001, 0.002),
            (0.01, 0.2),
            (0.1, 2.),
            (0.3, 10.),
            (2., 50.),
        ] {
            let context = context_for_pixel(pixel);
            assert!((context.grid_step_mm - step).abs() < 1e-9);
            assert!((context.point_tolerance_mm / pixel - 14.).abs() < 1e-9);
            assert!((context.grid_capture_mm / pixel - 7.).abs() < 1e-9);
        }
    }
    fn call(engine: &AppState, method: &str, arguments: Value) -> Value {
        crate::session_bridge::parse_engine_envelope(
            engine.engine_call(method, &arguments.to_string()),
        )
        .unwrap()
    }

    #[test]
    fn every_creation_tool_acquires_origin_grid_and_points_with_identical_preview_and_click() {
        for tool in [
            CreateTool::Line,
            CreateTool::MidpointLine,
            CreateTool::Point,
            CreateTool::Rectangle(RectangleMode::TwoPoint),
            CreateTool::Rectangle(RectangleMode::Center),
            CreateTool::Circle(CircleMode::CenterDiameter),
            CreateTool::Circle(CircleMode::TwoPoint),
            CreateTool::Arc3Point,
            CreateTool::ArcCenter,
            CreateTool::Slot(SlotMode::CenterToCenter),
            CreateTool::Slot(SlotMode::Overall),
            CreateTool::Slot(SlotMode::CenterPoint),
            CreateTool::Spline,
        ] {
            let engine = AppState::new();
            call(
                &engine,
                "begin_sketch",
                json!({"type":"origin_plane","plane":"xy"}),
            );
            let mut draft = Draft::default();
            draft.select(Some(tool));
            let context = context_for_pixel(0.1);
            let before = engine.engine_call("project_export_model", "");
            let revision = engine.geometry_revision();
            for (raw, expected, target) in [
                (SketchPoint::new(0.6, -0.4), SketchPoint::ZERO, "origin"),
                (
                    SketchPoint::new(30.4, 10.3),
                    SketchPoint::new(30., 10.),
                    "grid",
                ),
            ] {
                let preview = acquire(&engine, &draft, raw, false, context).unwrap();
                let click = acquire(&engine, &draft, raw, false, context).unwrap();
                assert_eq!(preview, click, "{tool:?}");
                assert_eq!(click.snapped_to, expected, "{tool:?}");
                assert_eq!(
                    serde_json::to_value(click.snap).unwrap()["kind"],
                    target,
                    "{tool:?}"
                );
            }
            assert_eq!(engine.engine_call("project_export_model", ""), before);
            assert_eq!(engine.geometry_revision(), revision, "{tool:?}");
            // Real queued payloads carry a scope; exercising all tool requests
            // verifies their committed geometry and one-step undo as well.
            let mut command = None;
            for raw in [
                SketchPoint::new(0.6, -0.4),
                SketchPoint::new(30.4, 10.3),
                SketchPoint::new(16.2, 14.3),
            ] {
                let point = acquire(&engine, &draft, raw, false, context)
                    .unwrap()
                    .snapped_to;
                draft.snap_context = Some(context);
                command = draft.prepare(point, false).unwrap();
                if command.is_some() {
                    break;
                }
            }
            let mut command = command
                .or_else(|| draft.complete().unwrap())
                .expect("complete primitive");
            attach(&mut command.arguments, draft.snap_context);
            let spec = limo_cad_mcp_mutate::lookup_mutate(command.operation).unwrap();
            let result = call(&engine, spec.engine_method, command.arguments);
            assert!(
                !result["sketch"]["entities"].as_array().unwrap().is_empty(),
                "{tool:?}"
            );
            call(&engine, "undo", json!(null));
            let restored = call(&engine, "active_sketch", json!(null));
            assert!(
                restored["entities"].as_array().unwrap().is_empty(),
                "{tool:?}"
            );
            // Relational and grid switches have the reference app's semantics.
            call(
                &engine,
                "add_point",
                json!({"position":{"x":5.,"y":7.},"ctrl_held":true}),
            );
            let near_point =
                acquire(&engine, &draft, SketchPoint::new(5.6, 7.4), false, context).unwrap();
            assert!(
                matches!(near_point.snap, limo_cad_sketch::SnapTarget::Point { .. }),
                "{tool:?}"
            );
            let suppressed =
                acquire(&engine, &draft, SketchPoint::new(0.6, 0.4), true, context).unwrap();
            assert!(
                !matches!(suppressed.snap, limo_cad_sketch::SnapTarget::Origin),
                "{tool:?}"
            );
            call(&engine, "set_grid_snap", json!({"enabled":false}));
            let raw = SketchPoint::new(0.6, 0.4);
            assert_eq!(
                acquire(&engine, &draft, raw, false, context)
                    .unwrap()
                    .snapped_to,
                raw,
                "{tool:?}"
            );
        }
    }

    #[test]
    fn midpoint_commit_keeps_the_viewport_grid_instead_of_the_script_default() {
        let engine = AppState::new();
        call(
            &engine,
            "begin_sketch",
            json!({"type":"origin_plane","plane":"xy"}),
        );
        let mut args = json!({"mid_raw":{"x":0.6,"y":-0.4},"end_raw":{"x":31.8,"y":20.3}});
        attach(&mut args, Some(context_for_pixel(0.1)));
        let result = call(&engine, "add_line_midpoint", args);
        assert!(result["sketch"]["entities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entity| entity["position"] == json!({"x":32.,"y":20.})));
    }

    #[test]
    fn scoped_grid_and_capture_are_restored_after_failed_and_successful_calls() {
        let engine = AppState::new();
        call(
            &engine,
            "begin_sketch",
            json!({"type":"origin_plane","plane":"xy"}),
        );
        let request = json!({"raw":{"x":3.,"y":0.}});
        let original = call(&engine, "preview_creation_point", request.clone());
        let mut scoped = request.clone();
        attach(&mut scoped, Some(context_for_pixel(0.5)));
        assert_eq!(
            call(&engine, "preview_creation_point", scoped)["snap"]["kind"],
            "origin"
        );
        assert_eq!(
            call(&engine, "preview_creation_point", request.clone()),
            original
        );
        let mut invalid = json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":0.,"y":0.}});
        attach(&mut invalid, Some(context_for_pixel(0.01)));
        let failure: Value =
            serde_json::from_str(&engine.engine_call("add_rectangle", &invalid.to_string()))
                .unwrap();
        assert_eq!(failure["ok"], false);
        assert_eq!(call(&engine, "preview_creation_point", request), original);
    }
}
