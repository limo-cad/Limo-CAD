//! Ordered sketch creation gestures. The engine owns geometry, constraints,
//! snapping and history; this state owns only the user's unfinished picks.

#[path = "sketch_preview.rs"]
mod preview;

use limo_cad_sketch::{
    Arc3PointRequest, ArcCenterRequest, CircleMode, CircleRequest, MidpointLineRequest,
    PointRequest, RectangleMode, RectangleRequest, SegmentRequest, SlotMode, SlotRequest,
    SplineRequest, Vec2,
};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CreateTool {
    Line,
    MidpointLine,
    Point,
    Rectangle(RectangleMode),
    Circle(CircleMode),
    Arc3Point,
    ArcCenter,
    Slot(SlotMode),
    Spline,
}

impl CreateTool {
    pub(crate) fn primary_variant(self, active: Option<Self>) -> Self {
        match (self, active) {
            (Self::Line | Self::MidpointLine, Some(tool @ (Self::Line | Self::MidpointLine)))
            | (
                Self::Arc3Point | Self::ArcCenter,
                Some(tool @ (Self::Arc3Point | Self::ArcCenter)),
            )
            | (Self::Rectangle(_), Some(tool @ Self::Rectangle(_)))
            | (Self::Circle(_), Some(tool @ Self::Circle(_)))
            | (Self::Slot(_), Some(tool @ Self::Slot(_))) => tool,
            _ => self,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Line => "Line",
            Self::MidpointLine => "Midpoint line",
            Self::Point => "Point",
            Self::Rectangle(RectangleMode::TwoPoint) => "Rectangle",
            Self::Rectangle(RectangleMode::Center) => "Center rectangle",
            Self::Circle(CircleMode::CenterDiameter) => "Circle",
            Self::Circle(CircleMode::TwoPoint) => "Two-point circle",
            Self::Arc3Point => "Three-point arc",
            Self::ArcCenter => "Center arc",
            Self::Slot(SlotMode::CenterToCenter) => "Center-to-center slot",
            Self::Slot(SlotMode::Overall) => "Overall slot",
            Self::Slot(SlotMode::CenterPoint) => "Center-point slot",
            Self::Spline => "Fit-point spline",
        }
    }

    fn required_picks(self) -> Option<usize> {
        match self {
            Self::Point => Some(1),
            Self::Arc3Point | Self::ArcCenter | Self::Slot(_) => Some(3),
            Self::Spline => None,
            _ => Some(2),
        }
    }
}

/// A prepared command is not an accepted point. The caller advances a draft
/// only after successful dispatch, so a rejected degenerate arc/line can be
/// corrected without losing the first picks or introducing another undo entry.
#[derive(Debug)]
pub(crate) struct Prepared {
    pub operation: &'static str,
    pub arguments: Value,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Draft {
    pub tool: Option<CreateTool>,
    pub points: Vec<Vec2>,
    /// Raw pointer hint for the engine's snapping and constrained construction.
    pub cursor: Option<Vec2>,
    /// Anchor and endpoint actually used by the visible construction preview.
    pub resolved_preview: Option<[Vec2; 2]>,
    chain_start: Option<Vec2>,
    pub generation: u64,
    pub sizes: super::dynamic::Sizes,
    pub snap_context: Option<limo_cad_sketch::ViewportSnapContext>,
}

fn encoded<T: Serialize>(operation: &'static str, value: T) -> Result<Prepared, String> {
    Ok(Prepared {
        operation,
        arguments: serde_json::to_value(value).map_err(|e| e.to_string())?,
    })
}

impl Draft {
    pub fn select(&mut self, tool: Option<CreateTool>) {
        self.reset_sizes();
        self.tool = tool;
        self.points.clear();
        self.cursor = None;
        self.chain_start = None;
    }

    /// Escape first cancels an unfinished primitive; a second Escape leaves
    /// the tool. Already committed chain segments remain ordinary history.
    pub fn escape(&mut self) {
        self.reset_sizes();
        if self.points.is_empty() {
            self.tool = None;
        }
        self.points.clear();
        self.cursor = None;
        self.chain_start = None;
    }

    pub fn prepare(&mut self, point: Vec2, ctrl: bool) -> Result<Option<Prepared>, String> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err("Sketch coordinates must be finite".into());
        }
        let Some(tool) = self.tool else {
            return Ok(None);
        };
        let mut picks = self.points.clone();
        if tool == CreateTool::Spline && picks.last() == Some(&point) {
            return Ok(None);
        }
        picks.push(point);
        if tool
            .required_picks()
            .is_none_or(|count| picks.len() < count)
        {
            self.points = picks;
            self.cursor = Some(point);
            self.resolved_preview = None;
            return Ok(None);
        }
        let p1 = picks[0];
        let p2 = *picks.get(1).unwrap_or(&p1);
        if let Some(command) = self.sizes.prepare(tool, &picks, ctrl)? {
            return Ok(Some(command));
        }
        let value = match tool {
            CreateTool::Line => encoded(
                "sketch_add_line",
                SegmentRequest {
                    from: p1,
                    to_raw: p2,
                    ctrl_held: ctrl,
                },
            ),
            CreateTool::MidpointLine => encoded(
                "sketch_add_midpoint_line",
                MidpointLineRequest {
                    mid_raw: p1,
                    end_raw: p2,
                    ctrl_held: ctrl,
                },
            ),
            CreateTool::Point => encoded(
                "sketch_add_point",
                PointRequest {
                    position: p1,
                    coincident_with: None,
                    ctrl_held: ctrl,
                },
            ),
            CreateTool::Rectangle(mode) => encoded(
                "sketch_add_rectangle",
                RectangleRequest {
                    mode,
                    p1,
                    p2,
                    ctrl_held: ctrl,
                },
            ),
            CreateTool::Circle(mode) => encoded(
                "sketch_add_circle",
                CircleRequest {
                    mode,
                    p1,
                    p2,
                    ctrl_held: ctrl,
                },
            ),
            CreateTool::Arc3Point => encoded(
                "sketch_add_arc_3pt",
                Arc3PointRequest {
                    p1,
                    p2,
                    p3: point,
                    ctrl_held: ctrl,
                },
            ),
            CreateTool::ArcCenter => encoded(
                "sketch_add_arc_center",
                ArcCenterRequest {
                    center: p1,
                    start: p2,
                    sweep: point,
                    ctrl_held: ctrl,
                    radius_mm: None,
                    radius_text: None,
                    angle_text: None,
                    sweep_rad: None,
                },
            ),
            CreateTool::Slot(mode) => encoded(
                "sketch_add_slot",
                SlotRequest {
                    mode,
                    p1,
                    p2,
                    cursor: point,
                    width_mm: None,
                    width_text: None,
                    ctrl_held: ctrl,
                },
            ),
            CreateTool::Spline => unreachable!("splines finish explicitly"),
        }?;
        Ok(Some(value))
    }

    /// Prepare a completed shape without changing its anchor or typed sizes
    /// before the engine accepts the construction.
    pub fn complete(&self) -> Result<Option<Prepared>, String> {
        if self.tool != Some(CreateTool::Spline) {
            if !self.sizes.has_locks() {
                return Ok(None);
            }
            let Some(cursor) = self.cursor else {
                return Ok(None);
            };
            return self.clone().prepare(cursor, false);
        }
        if self.points.len() < 2 {
            return Err("Pick at least two fit points before finishing the spline".into());
        }
        encoded(
            "sketch_add_spline",
            SplineRequest {
                points: self.points.clone(),
            },
        )
        .map(Some)
    }

    pub fn accepted(&mut self, result: &Value) -> Result<(), String> {
        self.reset_sizes();
        self.points.clear();
        self.cursor = None;
        if self.tool == Some(CreateTool::Line) {
            let end_id = result["end_point_id"]
                .as_u64()
                .ok_or("Line result has no endpoint")?;
            let point = result["sketch"]["entities"]
                .as_array()
                .and_then(|entities| {
                    entities
                        .iter()
                        .find(|entity| entity["id"] == end_id && entity["kind"] == "point")
                })
                .ok_or("Committed line endpoint is unavailable")?;
            let end: Vec2 =
                serde_json::from_value(point["position"].clone()).map_err(|e| e.to_string())?;
            if self.chain_start.is_none() {
                let start_id = result["start_point_id"]
                    .as_u64()
                    .ok_or("Line result has no start point")?;
                self.chain_start = result["sketch"]["entities"]
                    .as_array()
                    .and_then(|entities| {
                        entities
                            .iter()
                            .find(|entity| entity["id"] == start_id && entity["kind"] == "point")
                    })
                    .and_then(|entity| serde_json::from_value(entity["position"].clone()).ok());
            }
            if self
                .chain_start
                .is_none_or(|start| start.distance(end) > 1e-6)
            {
                self.points.push(end);
            } else {
                self.chain_start = None;
            }
        }
        Ok(())
    }

    fn reset_sizes(&mut self) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("Sketch gesture identities exhausted");
        self.sizes = Default::default();
        self.snap_context = None;
        self.resolved_preview = None;
    }

    pub fn instruction(&self) -> &'static str {
        match (self.tool, self.points.len()) {
            (None, _) => "Select a sketch tool",
            (Some(CreateTool::Point), _) => "Pick a point",
            (Some(CreateTool::Spline), 0) => "Pick the first fit point",
            (Some(CreateTool::Spline), _) => {
                "Pick another fit point; Enter finishes, Escape cancels"
            }
            (Some(CreateTool::Arc3Point), 1) => "Pick a point on the arc",
            (Some(CreateTool::Arc3Point), 2) => "Pick the arc endpoint",
            (Some(CreateTool::ArcCenter), 1) => "Pick the arc start",
            (Some(CreateTool::ArcCenter), 2) => "Pick the arc end",
            (Some(CreateTool::Slot(_)), 1) => "Pick the other slot axis point",
            (Some(CreateTool::Slot(_)), 2) => "Pick the slot width",
            (_, 0) => "Pick the first point",
            _ => "Pick the second point; Escape cancels this shape",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use limo_cad_sketch::{host, SketchManager};

    fn manager() -> SketchManager {
        let mut manager = SketchManager::new();
        let result = host::handle(
            &mut manager,
            "begin_sketch",
            r#"{"type":"origin_plane","plane":"xy"}"#,
        );
        assert!(
            serde_json::from_str::<Value>(&result).unwrap()["ok"] == true,
            "{result}"
        );
        manager
    }

    fn apply(manager: &mut SketchManager, command: &Prepared) -> Value {
        let spec = limo_cad_mcp_mutate::lookup_mutate(command.operation).unwrap();
        let payload =
            limo_cad_mcp_mutate::encode_payload(spec.payload, &command.arguments).unwrap();
        let response: Value =
            serde_json::from_str(&host::handle(manager, spec.engine_method, &payload)).unwrap();
        assert_eq!(
            response["ok"], true,
            "{} {}: {response}",
            command.operation, command.arguments
        );
        response["value"].clone()
    }

    #[test]
    fn chain_uses_engine_snapped_endpoint_and_cancel_keeps_committed_history() {
        let mut manager = manager();
        let mut draft = Draft::default();
        draft.select(Some(CreateTool::Line));
        assert!(draft.prepare(Vec2::ZERO, false).unwrap().is_none());
        let command = draft.prepare(Vec2::new(30., 0.1), false).unwrap().unwrap();
        let result = apply(&mut manager, &command);
        draft.accepted(&result).unwrap();
        assert_eq!(draft.points[0].y, 0.);
        let before = host::handle(&mut manager, "active_sketch", "");
        draft.escape();
        assert_eq!(draft.tool, Some(CreateTool::Line));
        draft.escape();
        assert_eq!(draft.tool, None);
        assert_eq!(host::handle(&mut manager, "active_sketch", ""), before);
    }

    #[test]
    fn creation_gestures_use_real_engine_commands_and_one_undo_record() {
        let tools = [
            CreateTool::Point,
            CreateTool::MidpointLine,
            CreateTool::Rectangle(RectangleMode::TwoPoint),
            CreateTool::Rectangle(RectangleMode::Center),
            CreateTool::Circle(CircleMode::CenterDiameter),
            CreateTool::Circle(CircleMode::TwoPoint),
            CreateTool::Arc3Point,
            CreateTool::ArcCenter,
            CreateTool::Slot(SlotMode::CenterToCenter),
            CreateTool::Slot(SlotMode::Overall),
            CreateTool::Slot(SlotMode::CenterPoint),
        ];
        for tool in tools {
            let mut manager = manager();
            let before: Value =
                serde_json::from_str(&host::handle(&mut manager, "active_sketch", "")).unwrap();
            let mut draft = Draft::default();
            draft.select(Some(tool));
            let mut committed = false;
            for point in [Vec2::ZERO, Vec2::new(30., 10.), Vec2::new(16., 14.)] {
                if let Some(command) = draft.prepare(point, true).unwrap() {
                    let result = apply(&mut manager, &command);
                    draft.accepted(&result).unwrap();
                    committed = true;
                    break;
                }
            }
            assert!(committed, "{tool:?}");
            let undo: Value =
                serde_json::from_str(&host::handle(&mut manager, "undo", "")).unwrap();
            assert_eq!(undo["ok"], true, "{tool:?}: {undo}");
            assert_eq!(
                undo["value"]["sketch"]["entities"], before["value"]["entities"],
                "{tool:?}"
            );
        }
    }

    #[test]
    fn rejected_shape_keeps_original_picks_and_spline_finishes_explicitly() {
        let mut draft = Draft::default();
        draft.select(Some(CreateTool::Arc3Point));
        draft.prepare(Vec2::ZERO, true).unwrap();
        draft.prepare(Vec2::new(20., 0.), true).unwrap();
        let invalid = draft.prepare(Vec2::new(10., 0.), true).unwrap().unwrap();
        let mut unchanged = manager();
        let before = host::handle(&mut unchanged, "active_sketch", "");
        let spec = limo_cad_mcp_mutate::lookup_mutate(invalid.operation).unwrap();
        let payload =
            limo_cad_mcp_mutate::encode_payload(spec.payload, &invalid.arguments).unwrap();
        let response: Value =
            serde_json::from_str(&host::handle(&mut unchanged, spec.engine_method, &payload))
                .unwrap();
        assert_eq!(response["ok"], false);
        assert_eq!(host::handle(&mut unchanged, "active_sketch", ""), before);
        assert_eq!(draft.points, vec![Vec2::ZERO, Vec2::new(20., 0.)]);
        assert!(draft.prepare(Vec2::new(f64::NAN, 0.), true).is_err());
        draft.select(Some(CreateTool::Spline));
        draft.prepare(Vec2::ZERO, true).unwrap();
        assert!(draft.complete().is_err());
        draft.prepare(Vec2::new(20., 5.), true).unwrap();
        draft.prepare(Vec2::new(20., 5.), true).unwrap();
        assert_eq!(draft.points.len(), 2);
        let mut manager = manager();
        apply(&mut manager, &draft.complete().unwrap().unwrap());
    }

    #[test]
    fn starting_another_polyline_retires_the_previous_chain_origin() {
        let mut manager = manager();
        let mut draft = Draft::default();
        draft.select(Some(CreateTool::Line));
        draft.prepare(Vec2::ZERO, true).unwrap();
        let first = draft.prepare(Vec2::new(30., 0.), true).unwrap().unwrap();
        draft.accepted(&apply(&mut manager, &first)).unwrap();
        draft.select(draft.tool);
        draft.prepare(Vec2::new(0., 30.), true).unwrap();
        let second = draft.prepare(Vec2::ZERO, true).unwrap().unwrap();
        draft.accepted(&apply(&mut manager, &second)).unwrap();
        assert_eq!(draft.points, vec![Vec2::ZERO]);
        assert_eq!(draft.chain_start, Some(Vec2::new(0., 30.)));
    }
}
