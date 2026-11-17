//! One localized teaching catalog for native hints, demonstrations and Help.

pub mod coverage;
use crate::Page;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

pub const LOCALES: [&str; 4] = ["en", "zh-CN", "es", "de"];
pub const RECIPE: &str = "extrude-basics";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Id {
    Sketch,
    Extrude,
    Cut,
    Fillet,
    Revolve,
    Sweep,
    Loft,
    Rib,
    Hole,
    ExternalThread,
    Chamfer,
    Shell,
    Mirror,
    RectangularPattern,
    CircularPattern,
    MoveCopy,
    Combine,
    SplitBody,
    OffsetPlane,
    Midplane,
    PlaneAtAngle,
    SketchLine,
    SketchMidpointLine,
    SketchPoint,
    SketchRectangle,
    SketchCircle,
    SketchArc,
    SketchSpline,
    SketchPolygon,
    SketchSlot,
    SketchMoveCopy,
    SketchScale,
    SketchOffset,
    SketchTrim,
    SketchExtend,
    SketchBreak,
    SketchFillet,
    SketchChamfer,
    SketchDimension,
    SketchMirror,
    SketchRectangularPattern,
    SketchCircularPattern,
    SketchCoincident,
    SketchHorizontalVertical,
    SketchTangent,
    SketchParallel,
    SketchPerpendicular,
    SketchEqual,
    SketchFixUnfix,
    SketchMidpoint,
    SketchCollinear,
    SketchConcentric,
    SketchSymmetry,
    ConstructionVisibility,
    SectionAnalysis,
    SelectionSolid,
    SelectionSketch,
    Measure,
}
pub const ALL: [Id; 58] = [
    Id::Sketch,
    Id::Extrude,
    Id::Cut,
    Id::Fillet,
    Id::Revolve,
    Id::Sweep,
    Id::Loft,
    Id::Rib,
    Id::Hole,
    Id::ExternalThread,
    Id::Chamfer,
    Id::Shell,
    Id::Mirror,
    Id::RectangularPattern,
    Id::CircularPattern,
    Id::MoveCopy,
    Id::Combine,
    Id::SplitBody,
    Id::OffsetPlane,
    Id::Midplane,
    Id::PlaneAtAngle,
    Id::SketchLine,
    Id::SketchMidpointLine,
    Id::SketchPoint,
    Id::SketchRectangle,
    Id::SketchCircle,
    Id::SketchArc,
    Id::SketchSpline,
    Id::SketchPolygon,
    Id::SketchSlot,
    Id::SketchMoveCopy,
    Id::SketchScale,
    Id::SketchOffset,
    Id::SketchTrim,
    Id::SketchExtend,
    Id::SketchBreak,
    Id::SketchFillet,
    Id::SketchChamfer,
    Id::SketchDimension,
    Id::SketchMirror,
    Id::SketchRectangularPattern,
    Id::SketchCircularPattern,
    Id::SketchCoincident,
    Id::SketchHorizontalVertical,
    Id::SketchTangent,
    Id::SketchParallel,
    Id::SketchPerpendicular,
    Id::SketchEqual,
    Id::SketchFixUnfix,
    Id::SketchMidpoint,
    Id::SketchCollinear,
    Id::SketchConcentric,
    Id::SketchSymmetry,
    Id::ConstructionVisibility,
    Id::SectionAnalysis,
    Id::SelectionSolid,
    Id::SelectionSketch,
    Id::Measure,
];

/// A small choice of outcomes within Cut, independent of lesson navigation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CutScenario {
    #[default]
    ThroughAll,
    MeasuredDepth,
}
impl CutScenario {
    pub const ALL: [Self; 2] = [Self::ThroughAll, Self::MeasuredDepth];
    pub const fn key(self) -> &'static str {
        match self {
            Self::ThroughAll => "through_all",
            Self::MeasuredDepth => "measured_depth",
        }
    }
    pub fn label(self, locale: &str) -> &'static str {
        text(locale, &format!("cut.scenario.{}", self.key()))
    }
    pub const fn frame(self, index: usize) -> usize {
        match (self, index) {
            (Self::MeasuredDepth, 2..=3) => index + 2,
            _ => index,
        }
    }
}
impl Id {
    /// The editable build that authored this guide's immutable geometry.
    pub const fn recipe(self) -> &'static str {
        match self {
            Self::ConstructionVisibility => "show-me-construction-visibility",
            Self::SectionAnalysis => "show-me-section-analysis",
            Self::SelectionSolid => "show-me-selection-solid",
            Self::SelectionSketch => "show-me-selection-sketch",
            Self::Measure => "show-me-measure",

            Self::Sketch => "extrude-basics",
            Self::Extrude => "extrude-basics",
            Self::Cut => "extrude-basics",
            Self::Fillet => "extrude-basics",
            Self::Revolve => "show-me-revolve",
            Self::Sweep => "show-me-sweep",
            Self::Loft => "show-me-loft",
            Self::Rib => "show-me-rib",
            Self::Hole => "show-me-hole",
            Self::ExternalThread => "show-me-external-thread",
            Self::Chamfer => "show-me-chamfer",
            Self::Shell => "show-me-shell",
            Self::Mirror => "show-me-mirror",
            Self::RectangularPattern => "show-me-rectangular-pattern",
            Self::CircularPattern => "show-me-circular-pattern",
            Self::MoveCopy => "show-me-move-copy",
            Self::Combine => "show-me-combine",
            Self::SplitBody => "show-me-split-body",
            Self::OffsetPlane => "show-me-offset-plane",
            Self::Midplane => "show-me-midplane",
            Self::PlaneAtAngle => "show-me-plane-at-angle",
            Self::SketchLine => "show-me-sketch-draw",
            Self::SketchMidpointLine => "show-me-sketch-draw",
            Self::SketchPoint => "show-me-sketch-draw",
            Self::SketchRectangle => "show-me-sketch-draw",
            Self::SketchCircle => "show-me-sketch-draw",
            Self::SketchArc => "show-me-sketch-draw",
            Self::SketchSpline => "show-me-sketch-draw",
            Self::SketchPolygon => "show-me-sketch-draw",
            Self::SketchSlot => "show-me-sketch-draw",
            Self::SketchMoveCopy => "show-me-sketch-edit",
            Self::SketchScale => "show-me-sketch-edit",
            Self::SketchOffset => "show-me-sketch-edit",
            Self::SketchTrim => "show-me-sketch-edit",
            Self::SketchExtend => "show-me-sketch-edit",
            Self::SketchBreak => "show-me-sketch-edit",
            Self::SketchFillet => "show-me-sketch-edit",
            Self::SketchChamfer => "show-me-sketch-edit",
            Self::SketchDimension => "show-me-sketch-repeat",
            Self::SketchMirror => "show-me-sketch-repeat",
            Self::SketchRectangularPattern => "show-me-sketch-repeat",
            Self::SketchCircularPattern => "show-me-sketch-repeat",
            Self::SketchCoincident => "show-me-sketch-constraints",
            Self::SketchHorizontalVertical => "show-me-sketch-constraints",
            Self::SketchTangent => "show-me-sketch-constraints",
            Self::SketchParallel => "show-me-sketch-constraints",
            Self::SketchPerpendicular => "show-me-sketch-constraints",
            Self::SketchEqual => "show-me-sketch-constraints",
            Self::SketchFixUnfix => "show-me-sketch-constraints",
            Self::SketchMidpoint => "show-me-sketch-constraints",
            Self::SketchCollinear => "show-me-sketch-constraints",
            Self::SketchConcentric => "show-me-sketch-constraints",
            Self::SketchSymmetry => "show-me-sketch-constraints",
        }
    }
    pub const fn frame_count(self) -> usize {
        match self {
            Self::ConstructionVisibility => 4,
            Self::SectionAnalysis => 4,
            Self::SelectionSolid => 4,
            Self::SelectionSketch => 4,
            Self::Measure => 4,

            Self::Sketch => 5,
            Self::Extrude => 5,
            Self::Cut => 6,
            Self::Fillet => 4,
            Self::Revolve => 4,
            Self::Sweep => 4,
            Self::Loft => 4,
            Self::Rib => 4,
            Self::Hole => 4,
            Self::ExternalThread => 4,
            Self::Chamfer => 4,
            Self::Shell => 4,
            Self::Mirror => 4,
            Self::RectangularPattern => 4,
            Self::CircularPattern => 4,
            Self::MoveCopy => 4,
            Self::Combine => 4,
            Self::SplitBody => 4,
            Self::OffsetPlane => 4,
            Self::Midplane => 4,
            Self::PlaneAtAngle => 4,
            Self::SketchLine => 4,
            Self::SketchMidpointLine => 4,
            Self::SketchPoint => 4,
            Self::SketchRectangle => 4,
            Self::SketchCircle => 4,
            Self::SketchArc => 4,
            Self::SketchSpline => 4,
            Self::SketchPolygon => 4,
            Self::SketchSlot => 4,
            Self::SketchMoveCopy => 4,
            Self::SketchScale => 4,
            Self::SketchOffset => 4,
            Self::SketchTrim => 4,
            Self::SketchExtend => 4,
            Self::SketchBreak => 4,
            Self::SketchFillet => 4,
            Self::SketchChamfer => 4,
            Self::SketchDimension => 4,
            Self::SketchMirror => 4,
            Self::SketchRectangularPattern => 4,
            Self::SketchCircularPattern => 4,
            Self::SketchCoincident => 4,
            Self::SketchHorizontalVertical => 4,
            Self::SketchTangent => 4,
            Self::SketchParallel => 4,
            Self::SketchPerpendicular => 4,
            Self::SketchEqual => 4,
            Self::SketchFixUnfix => 4,
            Self::SketchMidpoint => 4,
            Self::SketchCollinear => 4,
            Self::SketchConcentric => 4,
            Self::SketchSymmetry => 4,
        }
    }
    pub const fn key(self) -> &'static str {
        match self {
            Self::ConstructionVisibility => "construction_visibility",
            Self::SectionAnalysis => "section_analysis",
            Self::SelectionSolid => "selection_solid",
            Self::SelectionSketch => "selection_sketch",
            Self::Measure => "measure",

            Self::Sketch => "sketch",
            Self::Extrude => "extrude",
            Self::Cut => "cut",
            Self::Fillet => "fillet",
            Self::Revolve => "revolve",
            Self::Sweep => "sweep",
            Self::Loft => "loft",
            Self::Rib => "rib",
            Self::Hole => "hole",
            Self::ExternalThread => "external_thread",
            Self::Chamfer => "chamfer",
            Self::Shell => "shell",
            Self::Mirror => "mirror",
            Self::RectangularPattern => "rectangular_pattern",
            Self::CircularPattern => "circular_pattern",
            Self::MoveCopy => "move_copy",
            Self::Combine => "combine",
            Self::SplitBody => "split_body",
            Self::OffsetPlane => "offset_plane",
            Self::Midplane => "midplane",
            Self::PlaneAtAngle => "plane_at_angle",
            Self::SketchLine => "sketch_line",
            Self::SketchMidpointLine => "sketch_midpoint_line",
            Self::SketchPoint => "sketch_point",
            Self::SketchRectangle => "sketch_rectangle",
            Self::SketchCircle => "sketch_circle",
            Self::SketchArc => "sketch_arc",
            Self::SketchSpline => "sketch_spline",
            Self::SketchPolygon => "sketch_polygon",
            Self::SketchSlot => "sketch_slot",
            Self::SketchMoveCopy => "sketch_move_copy",
            Self::SketchScale => "sketch_scale",
            Self::SketchOffset => "sketch_offset",
            Self::SketchTrim => "sketch_trim",
            Self::SketchExtend => "sketch_extend",
            Self::SketchBreak => "sketch_break",
            Self::SketchFillet => "sketch_fillet",
            Self::SketchChamfer => "sketch_chamfer",
            Self::SketchDimension => "sketch_dimension",
            Self::SketchMirror => "sketch_mirror",
            Self::SketchRectangularPattern => "sketch_rectangular_pattern",
            Self::SketchCircularPattern => "sketch_circular_pattern",
            Self::SketchCoincident => "sketch_coincident",
            Self::SketchHorizontalVertical => "sketch_horizontal_vertical",
            Self::SketchTangent => "sketch_tangent",
            Self::SketchParallel => "sketch_parallel",
            Self::SketchPerpendicular => "sketch_perpendicular",
            Self::SketchEqual => "sketch_equal",
            Self::SketchFixUnfix => "sketch_fix_unfix",
            Self::SketchMidpoint => "sketch_midpoint",
            Self::SketchCollinear => "sketch_collinear",
            Self::SketchConcentric => "sketch_concentric",
            Self::SketchSymmetry => "sketch_symmetry",
        }
    }
    pub const fn help(self) -> &'static str {
        match self {
            Self::ConstructionVisibility => "concepts.show-me-construction-visibility",
            Self::SectionAnalysis => "concepts.show-me-section-analysis",
            Self::SelectionSolid => "concepts.show-me-selection-solid",
            Self::SelectionSketch => "concepts.show-me-selection-sketch",
            Self::Measure => "concepts.show-me-measure",

            Self::Sketch => "concepts.bracket-sketch",
            Self::Extrude => "concepts.extrude-basics",
            Self::Cut => "concepts.bracket-cut",
            Self::Fillet => "concepts.bracket-fillet",
            Self::Revolve => "concepts.show-me-revolve",
            Self::Sweep => "concepts.show-me-sweep",
            Self::Loft => "concepts.show-me-loft",
            Self::Rib => "concepts.show-me-rib",
            Self::Hole => "concepts.show-me-hole",
            Self::ExternalThread => "concepts.show-me-external-thread",
            Self::Chamfer => "concepts.show-me-chamfer",
            Self::Shell => "concepts.show-me-shell",
            Self::Mirror => "concepts.show-me-mirror",
            Self::RectangularPattern => "concepts.show-me-rectangular-pattern",
            Self::CircularPattern => "concepts.show-me-circular-pattern",
            Self::MoveCopy => "concepts.show-me-move-copy",
            Self::Combine => "concepts.show-me-combine",
            Self::SplitBody => "concepts.show-me-split-body",
            Self::OffsetPlane => "concepts.show-me-offset-plane",
            Self::Midplane => "concepts.show-me-midplane",
            Self::PlaneAtAngle => "concepts.show-me-plane-at-angle",
            Self::SketchLine => "concepts.show-me-sketch-line",
            Self::SketchMidpointLine => "concepts.show-me-sketch-midpoint-line",
            Self::SketchPoint => "concepts.show-me-sketch-point",
            Self::SketchRectangle => "concepts.show-me-sketch-rectangle",
            Self::SketchCircle => "concepts.show-me-sketch-circle",
            Self::SketchArc => "concepts.show-me-sketch-arc",
            Self::SketchSpline => "concepts.show-me-sketch-spline",
            Self::SketchPolygon => "concepts.show-me-sketch-polygon",
            Self::SketchSlot => "concepts.show-me-sketch-slot",
            Self::SketchMoveCopy => "concepts.show-me-sketch-move-copy",
            Self::SketchScale => "concepts.show-me-sketch-scale",
            Self::SketchOffset => "concepts.show-me-sketch-offset",
            Self::SketchTrim => "concepts.show-me-sketch-trim",
            Self::SketchExtend => "concepts.show-me-sketch-extend",
            Self::SketchBreak => "concepts.show-me-sketch-break",
            Self::SketchFillet => "concepts.show-me-sketch-fillet",
            Self::SketchChamfer => "concepts.show-me-sketch-chamfer",
            Self::SketchDimension => "concepts.show-me-sketch-dimension",
            Self::SketchMirror => "concepts.show-me-sketch-mirror",
            Self::SketchRectangularPattern => "concepts.show-me-sketch-rectangular-pattern",
            Self::SketchCircularPattern => "concepts.show-me-sketch-circular-pattern",
            Self::SketchCoincident => "concepts.show-me-sketch-coincident",
            Self::SketchHorizontalVertical => "concepts.show-me-sketch-horizontal-vertical",
            Self::SketchTangent => "concepts.show-me-sketch-tangent",
            Self::SketchParallel => "concepts.show-me-sketch-parallel",
            Self::SketchPerpendicular => "concepts.show-me-sketch-perpendicular",
            Self::SketchEqual => "concepts.show-me-sketch-equal",
            Self::SketchFixUnfix => "concepts.show-me-sketch-fix-unfix",
            Self::SketchMidpoint => "concepts.show-me-sketch-midpoint",
            Self::SketchCollinear => "concepts.show-me-sketch-collinear",
            Self::SketchConcentric => "concepts.show-me-sketch-concentric",
            Self::SketchSymmetry => "concepts.show-me-sketch-symmetry",
        }
    }
    pub const fn prerequisite(self) -> Option<Id> {
        match self {
            Self::ConstructionVisibility => Some(Self::Extrude),
            Self::SectionAnalysis => Some(Self::Extrude),
            Self::SelectionSolid => Some(Self::Extrude),
            Self::SelectionSketch => Some(Self::Sketch),
            Self::Measure => Some(Self::Extrude),

            Self::Sketch => None,
            Self::Extrude => Some(Self::Sketch),
            Self::Cut => Some(Self::Extrude),
            Self::Fillet => Some(Self::Extrude),
            Self::Revolve => Some(Self::Sketch),
            Self::Sweep => Some(Self::Sketch),
            Self::Loft => Some(Self::Sketch),
            Self::Rib => Some(Self::Extrude),
            Self::Hole => Some(Self::Extrude),
            Self::ExternalThread => Some(Self::Extrude),
            Self::Chamfer => Some(Self::Extrude),
            Self::Shell => Some(Self::Extrude),
            Self::Mirror => Some(Self::Extrude),
            Self::RectangularPattern => Some(Self::Extrude),
            Self::CircularPattern => Some(Self::Extrude),
            Self::MoveCopy => Some(Self::Extrude),
            Self::Combine => Some(Self::Extrude),
            Self::SplitBody => Some(Self::Extrude),
            Self::OffsetPlane => Some(Self::Extrude),
            Self::Midplane => Some(Self::Extrude),
            Self::PlaneAtAngle => Some(Self::Extrude),
            Self::SketchLine => Some(Self::Sketch),
            Self::SketchMidpointLine => Some(Self::Sketch),
            Self::SketchPoint => Some(Self::Sketch),
            Self::SketchRectangle => Some(Self::Sketch),
            Self::SketchCircle => Some(Self::Sketch),
            Self::SketchArc => Some(Self::Sketch),
            Self::SketchSpline => Some(Self::Sketch),
            Self::SketchPolygon => Some(Self::Sketch),
            Self::SketchSlot => Some(Self::Sketch),
            Self::SketchMoveCopy => Some(Self::Sketch),
            Self::SketchScale => Some(Self::Sketch),
            Self::SketchOffset => Some(Self::Sketch),
            Self::SketchTrim => Some(Self::Sketch),
            Self::SketchExtend => Some(Self::Sketch),
            Self::SketchBreak => Some(Self::Sketch),
            Self::SketchFillet => Some(Self::Sketch),
            Self::SketchChamfer => Some(Self::Sketch),
            Self::SketchDimension => Some(Self::Sketch),
            Self::SketchMirror => Some(Self::Sketch),
            Self::SketchRectangularPattern => Some(Self::Sketch),
            Self::SketchCircularPattern => Some(Self::Sketch),
            Self::SketchCoincident => Some(Self::Sketch),
            Self::SketchHorizontalVertical => Some(Self::Sketch),
            Self::SketchTangent => Some(Self::Sketch),
            Self::SketchParallel => Some(Self::Sketch),
            Self::SketchPerpendicular => Some(Self::Sketch),
            Self::SketchEqual => Some(Self::Sketch),
            Self::SketchFixUnfix => Some(Self::Sketch),
            Self::SketchMidpoint => Some(Self::Sketch),
            Self::SketchCollinear => Some(Self::Sketch),
            Self::SketchConcentric => Some(Self::Sketch),
            Self::SketchSymmetry => Some(Self::Sketch),
        }
    }
    pub const fn cues(self) -> [&'static str; 4] {
        match self {
            Self::ConstructionVisibility => ["body", "body", "body", "body"],
            Self::SectionAnalysis => ["body", "dimension", "dimension", "body"],
            Self::SelectionSolid => ["body", "body", "edges", "body"],
            Self::SelectionSketch => ["body", "body", "body", "body"],
            Self::Measure => ["body", "body", "dimension", "body"],

            Self::Sketch => ["plane", "profile", "dimension", "apply"],
            Self::Extrude => ["profile", "extrude", "depth", "apply"],
            Self::Cut => ["profile", "cut", "depth", "apply"],
            Self::Fillet => ["body", "edges", "radius", "apply"],
            Self::Revolve => ["profile", "dimension", "dimension", "apply"],
            Self::Sweep => ["profile", "profile", "dimension", "apply"],
            Self::Loft => ["profile", "profile", "dimension", "apply"],
            Self::Rib => ["profile", "dimension", "dimension", "apply"],
            Self::Hole => ["body", "depth", "dimension", "apply"],
            Self::ExternalThread => ["body", "depth", "depth", "apply"],
            Self::Chamfer => ["body", "edges", "dimension", "apply"],
            Self::Shell => ["body", "depth", "dimension", "apply"],
            Self::Mirror => ["body", "plane", "dimension", "apply"],
            Self::RectangularPattern => ["body", "dimension", "dimension", "apply"],
            Self::CircularPattern => ["body", "dimension", "dimension", "apply"],
            Self::MoveCopy => ["body", "dimension", "dimension", "apply"],
            Self::Combine => ["body", "profile", "dimension", "apply"],
            Self::SplitBody => ["body", "plane", "dimension", "apply"],
            Self::OffsetPlane => ["body", "plane", "depth", "apply"],
            Self::Midplane => ["body", "plane", "dimension", "apply"],
            Self::PlaneAtAngle => ["edges", "plane", "dimension", "apply"],
            Self::SketchLine => ["profile", "profile", "profile", "apply"],
            Self::SketchMidpointLine => ["profile", "profile", "dimension", "apply"],
            Self::SketchPoint => ["profile", "profile", "profile", "apply"],
            Self::SketchRectangle => ["profile", "profile", "dimension", "apply"],
            Self::SketchCircle => ["profile", "profile", "dimension", "apply"],
            Self::SketchArc => ["profile", "profile", "radius", "apply"],
            Self::SketchSpline => ["profile", "profile", "profile", "apply"],
            Self::SketchPolygon => ["profile", "profile", "dimension", "apply"],
            Self::SketchSlot => ["profile", "profile", "dimension", "apply"],
            Self::SketchMoveCopy => ["profile", "profile", "dimension", "apply"],
            Self::SketchScale => ["profile", "profile", "dimension", "apply"],
            Self::SketchOffset => ["profile", "profile", "dimension", "apply"],
            Self::SketchTrim => ["profile", "profile", "profile", "apply"],
            Self::SketchExtend => ["profile", "profile", "profile", "apply"],
            Self::SketchBreak => ["profile", "profile", "profile", "apply"],
            Self::SketchFillet => ["profile", "profile", "radius", "apply"],
            Self::SketchChamfer => ["profile", "profile", "dimension", "apply"],
            Self::SketchDimension => ["profile", "profile", "dimension", "apply"],
            Self::SketchMirror => ["profile", "profile", "profile", "apply"],
            Self::SketchRectangularPattern => ["profile", "profile", "dimension", "apply"],
            Self::SketchCircularPattern => ["profile", "profile", "dimension", "apply"],
            Self::SketchCoincident => ["profile", "profile", "profile", "apply"],
            Self::SketchHorizontalVertical => ["profile", "profile", "profile", "apply"],
            Self::SketchTangent => ["profile", "profile", "profile", "apply"],
            Self::SketchParallel => ["profile", "profile", "profile", "apply"],
            Self::SketchPerpendicular => ["profile", "profile", "profile", "apply"],
            Self::SketchEqual => ["profile", "profile", "profile", "apply"],
            Self::SketchFixUnfix => ["profile", "profile", "profile", "apply"],
            Self::SketchMidpoint => ["profile", "profile", "profile", "apply"],
            Self::SketchCollinear => ["profile", "profile", "profile", "apply"],
            Self::SketchConcentric => ["profile", "profile", "profile", "apply"],
            Self::SketchSymmetry => ["profile", "profile", "profile", "apply"],
        }
    }
    /// Exact authored captions, including both retained Cut scenarios.
    pub const fn captions(self) -> &'static [&'static str] {
        match self {
            Self::ConstructionVisibility => &[
                "construction_visibility.inputs",
                "construction_visibility.hide",
                "construction_visibility.isolate",
                "construction_visibility.restore",
            ],
            Self::SectionAnalysis => &[
                "section_analysis.inputs",
                "section_analysis.plane",
                "section_analysis.reverse",
                "section_analysis.close",
            ],
            Self::SelectionSolid => &[
                "selection_solid.inputs",
                "selection_solid.face",
                "selection_solid.edge",
                "selection_solid.clear",
            ],
            Self::SelectionSketch => &[
                "selection_sketch.inputs",
                "selection_sketch.first",
                "selection_sketch.add",
                "selection_sketch.clear",
            ],
            Self::Measure => &[
                "measure.inputs",
                "measure.first",
                "measure.distance",
                "measure.close",
            ],

            Self::Sketch => &[
                "sketch.plane",
                "sketch.outline",
                "sketch.close",
                "sketch.dimension",
                "sketch.finish",
            ],
            Self::Extrude => &[
                "extrude.profile",
                "extrude.start",
                "extrude.distance",
                "extrude.width",
                "extrude.apply",
            ],
            Self::Cut => &[
                "cut.profile",
                "cut.operation",
                "cut.through",
                "cut.apply",
                "cut.measured.depth",
                "cut.measured.apply",
            ],
            Self::Fillet => &[
                "fillet.body",
                "fillet.edges",
                "fillet.radius",
                "fillet.apply",
            ],
            Self::Revolve => &[
                "revolve.inputs",
                "revolve.preview",
                "revolve.parameter",
                "revolve.apply",
            ],
            Self::Sweep => &[
                "sweep.inputs",
                "sweep.preview",
                "sweep.parameter",
                "sweep.apply",
            ],
            Self::Loft => &[
                "loft.inputs",
                "loft.preview",
                "loft.parameter",
                "loft.apply",
            ],
            Self::Rib => &["rib.inputs", "rib.preview", "rib.parameter", "rib.apply"],
            Self::Hole => &[
                "hole.inputs",
                "hole.preview",
                "hole.parameter",
                "hole.apply",
            ],
            Self::ExternalThread => &[
                "external_thread.inputs",
                "external_thread.preview",
                "external_thread.parameter",
                "external_thread.apply",
            ],
            Self::Chamfer => &[
                "chamfer.inputs",
                "chamfer.preview",
                "chamfer.parameter",
                "chamfer.apply",
            ],
            Self::Shell => &[
                "shell.inputs",
                "shell.preview",
                "shell.parameter",
                "shell.apply",
            ],
            Self::Mirror => &[
                "mirror.inputs",
                "mirror.preview",
                "mirror.parameter",
                "mirror.apply",
            ],
            Self::RectangularPattern => &[
                "rectangular_pattern.inputs",
                "rectangular_pattern.preview",
                "rectangular_pattern.parameter",
                "rectangular_pattern.apply",
            ],
            Self::CircularPattern => &[
                "circular_pattern.inputs",
                "circular_pattern.preview",
                "circular_pattern.parameter",
                "circular_pattern.apply",
            ],
            Self::MoveCopy => &[
                "move_copy.inputs",
                "move_copy.preview",
                "move_copy.parameter",
                "move_copy.apply",
            ],
            Self::Combine => &[
                "combine.inputs",
                "combine.preview",
                "combine.parameter",
                "combine.apply",
            ],
            Self::SplitBody => &[
                "split_body.inputs",
                "split_body.preview",
                "split_body.parameter",
                "split_body.apply",
            ],
            Self::OffsetPlane => &[
                "offset_plane.inputs",
                "offset_plane.preview",
                "offset_plane.parameter",
                "offset_plane.apply",
            ],
            Self::Midplane => &[
                "midplane.inputs",
                "midplane.preview",
                "midplane.parameter",
                "midplane.apply",
            ],
            Self::PlaneAtAngle => &[
                "plane_at_angle.inputs",
                "plane_at_angle.preview",
                "plane_at_angle.parameter",
                "plane_at_angle.apply",
            ],
            Self::SketchLine => &[
                "sketch_line.selection",
                "sketch_line.prepare",
                "sketch_line.parameters",
                "sketch_line.result",
            ],
            Self::SketchMidpointLine => &[
                "sketch_midpoint_line.selection",
                "sketch_midpoint_line.prepare",
                "sketch_midpoint_line.parameters",
                "sketch_midpoint_line.result",
            ],
            Self::SketchPoint => &[
                "sketch_point.selection",
                "sketch_point.prepare",
                "sketch_point.parameters",
                "sketch_point.result",
            ],
            Self::SketchRectangle => &[
                "sketch_rectangle.selection",
                "sketch_rectangle.prepare",
                "sketch_rectangle.parameters",
                "sketch_rectangle.result",
            ],
            Self::SketchCircle => &[
                "sketch_circle.selection",
                "sketch_circle.prepare",
                "sketch_circle.parameters",
                "sketch_circle.result",
            ],
            Self::SketchArc => &[
                "sketch_arc.selection",
                "sketch_arc.prepare",
                "sketch_arc.parameters",
                "sketch_arc.result",
            ],
            Self::SketchSpline => &[
                "sketch_spline.selection",
                "sketch_spline.prepare",
                "sketch_spline.parameters",
                "sketch_spline.result",
            ],
            Self::SketchPolygon => &[
                "sketch_polygon.selection",
                "sketch_polygon.prepare",
                "sketch_polygon.parameters",
                "sketch_polygon.result",
            ],
            Self::SketchSlot => &[
                "sketch_slot.selection",
                "sketch_slot.prepare",
                "sketch_slot.parameters",
                "sketch_slot.result",
            ],
            Self::SketchMoveCopy => &[
                "sketch_move_copy.selection",
                "sketch_move_copy.prepare",
                "sketch_move_copy.parameters",
                "sketch_move_copy.result",
            ],
            Self::SketchScale => &[
                "sketch_scale.selection",
                "sketch_scale.prepare",
                "sketch_scale.parameters",
                "sketch_scale.result",
            ],
            Self::SketchOffset => &[
                "sketch_offset.selection",
                "sketch_offset.prepare",
                "sketch_offset.parameters",
                "sketch_offset.result",
            ],
            Self::SketchTrim => &[
                "sketch_trim.selection",
                "sketch_trim.prepare",
                "sketch_trim.parameters",
                "sketch_trim.result",
            ],
            Self::SketchExtend => &[
                "sketch_extend.selection",
                "sketch_extend.prepare",
                "sketch_extend.parameters",
                "sketch_extend.result",
            ],
            Self::SketchBreak => &[
                "sketch_break.selection",
                "sketch_break.prepare",
                "sketch_break.parameters",
                "sketch_break.result",
            ],
            Self::SketchFillet => &[
                "sketch_fillet.selection",
                "sketch_fillet.prepare",
                "sketch_fillet.parameters",
                "sketch_fillet.result",
            ],
            Self::SketchChamfer => &[
                "sketch_chamfer.selection",
                "sketch_chamfer.prepare",
                "sketch_chamfer.parameters",
                "sketch_chamfer.result",
            ],
            Self::SketchDimension => &[
                "sketch_dimension.selection",
                "sketch_dimension.prepare",
                "sketch_dimension.parameters",
                "sketch_dimension.result",
            ],
            Self::SketchMirror => &[
                "sketch_mirror.selection",
                "sketch_mirror.prepare",
                "sketch_mirror.parameters",
                "sketch_mirror.result",
            ],
            Self::SketchRectangularPattern => &[
                "sketch_rectangular_pattern.selection",
                "sketch_rectangular_pattern.prepare",
                "sketch_rectangular_pattern.parameters",
                "sketch_rectangular_pattern.result",
            ],
            Self::SketchCircularPattern => &[
                "sketch_circular_pattern.selection",
                "sketch_circular_pattern.prepare",
                "sketch_circular_pattern.parameters",
                "sketch_circular_pattern.result",
            ],
            Self::SketchCoincident => &[
                "sketch_coincident.selection",
                "sketch_coincident.prepare",
                "sketch_coincident.parameters",
                "sketch_coincident.result",
            ],
            Self::SketchHorizontalVertical => &[
                "sketch_horizontal_vertical.selection",
                "sketch_horizontal_vertical.prepare",
                "sketch_horizontal_vertical.parameters",
                "sketch_horizontal_vertical.result",
            ],
            Self::SketchTangent => &[
                "sketch_tangent.selection",
                "sketch_tangent.prepare",
                "sketch_tangent.parameters",
                "sketch_tangent.result",
            ],
            Self::SketchParallel => &[
                "sketch_parallel.selection",
                "sketch_parallel.prepare",
                "sketch_parallel.parameters",
                "sketch_parallel.result",
            ],
            Self::SketchPerpendicular => &[
                "sketch_perpendicular.selection",
                "sketch_perpendicular.prepare",
                "sketch_perpendicular.parameters",
                "sketch_perpendicular.result",
            ],
            Self::SketchEqual => &[
                "sketch_equal.selection",
                "sketch_equal.prepare",
                "sketch_equal.parameters",
                "sketch_equal.result",
            ],
            Self::SketchFixUnfix => &[
                "sketch_fix_unfix.selection",
                "sketch_fix_unfix.prepare",
                "sketch_fix_unfix.parameters",
                "sketch_fix_unfix.result",
            ],
            Self::SketchMidpoint => &[
                "sketch_midpoint.selection",
                "sketch_midpoint.prepare",
                "sketch_midpoint.parameters",
                "sketch_midpoint.result",
            ],
            Self::SketchCollinear => &[
                "sketch_collinear.selection",
                "sketch_collinear.prepare",
                "sketch_collinear.parameters",
                "sketch_collinear.result",
            ],
            Self::SketchConcentric => &[
                "sketch_concentric.selection",
                "sketch_concentric.prepare",
                "sketch_concentric.parameters",
                "sketch_concentric.result",
            ],
            Self::SketchSymmetry => &[
                "sketch_symmetry.selection",
                "sketch_symmetry.prepare",
                "sketch_symmetry.parameters",
                "sketch_symmetry.result",
            ],
        }
    }
    /// Stable extension point for additional operation and Sketch guides.
    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "construction_visibility" => Self::ConstructionVisibility,
            "section_analysis" => Self::SectionAnalysis,
            "selection_solid" => Self::SelectionSolid,
            "selection_sketch" => Self::SelectionSketch,
            "measure" => Self::Measure,

            "sketch" => Self::Sketch,
            "extrude" => Self::Extrude,
            "cut" => Self::Cut,
            "fillet" => Self::Fillet,
            "revolve" => Self::Revolve,
            "sweep" => Self::Sweep,
            "loft" => Self::Loft,
            "rib" => Self::Rib,
            "hole" => Self::Hole,
            "external_thread" => Self::ExternalThread,
            "chamfer" => Self::Chamfer,
            "shell" => Self::Shell,
            "mirror" => Self::Mirror,
            "rectangular_pattern" => Self::RectangularPattern,
            "circular_pattern" => Self::CircularPattern,
            "move_copy" => Self::MoveCopy,
            "combine" => Self::Combine,
            "split_body" => Self::SplitBody,
            "offset_plane" => Self::OffsetPlane,
            "midplane" => Self::Midplane,
            "plane_at_angle" => Self::PlaneAtAngle,
            "sketch_line" => Self::SketchLine,
            "sketch_midpoint_line" => Self::SketchMidpointLine,
            "sketch_point" => Self::SketchPoint,
            "sketch_rectangle" => Self::SketchRectangle,
            "sketch_circle" => Self::SketchCircle,
            "sketch_arc" => Self::SketchArc,
            "sketch_spline" => Self::SketchSpline,
            "sketch_polygon" => Self::SketchPolygon,
            "sketch_slot" => Self::SketchSlot,
            "sketch_move_copy" => Self::SketchMoveCopy,
            "sketch_scale" => Self::SketchScale,
            "sketch_offset" => Self::SketchOffset,
            "sketch_trim" => Self::SketchTrim,
            "sketch_extend" => Self::SketchExtend,
            "sketch_break" => Self::SketchBreak,
            "sketch_fillet" => Self::SketchFillet,
            "sketch_chamfer" => Self::SketchChamfer,
            "sketch_dimension" => Self::SketchDimension,
            "sketch_mirror" => Self::SketchMirror,
            "sketch_rectangular_pattern" => Self::SketchRectangularPattern,
            "sketch_circular_pattern" => Self::SketchCircularPattern,
            "sketch_coincident" => Self::SketchCoincident,
            "sketch_horizontal_vertical" => Self::SketchHorizontalVertical,
            "sketch_tangent" => Self::SketchTangent,
            "sketch_parallel" => Self::SketchParallel,
            "sketch_perpendicular" => Self::SketchPerpendicular,
            "sketch_equal" => Self::SketchEqual,
            "sketch_fix_unfix" => Self::SketchFixUnfix,
            "sketch_midpoint" => Self::SketchMidpoint,
            "sketch_collinear" => Self::SketchCollinear,
            "sketch_concentric" => Self::SketchConcentric,
            "sketch_symmetry" => Self::SketchSymmetry,
            _ => return None,
        })
    }
    pub fn title(self, locale: &str) -> &'static str {
        text(locale, &format!("{}.title", self.key()))
    }
    pub fn summary(self, locale: &str) -> &'static str {
        text(locale, &format!("{}.summary", self.key()))
    }
}
fn catalog() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("lessons.json")).expect("embedded teaching catalog")
    })
}
pub fn text(locale: &str, key: &str) -> &'static str {
    catalog()[locale][key]
        .as_str()
        .or_else(|| catalog()["en"][key].as_str())
        .unwrap_or("")
}
pub fn caption(locale: &str, key: &str) -> String {
    let value = text(locale, key);
    if value.is_empty() {
        key.to_owned()
    } else {
        value.to_owned()
    }
}
pub fn page(locale: &str, id: Id) -> Page {
    let key = format!("{}.instructions", id.key());
    let values = catalog()[locale][&key]
        .as_array()
        .unwrap_or_else(|| catalog()["en"][&key].as_array().unwrap());
    let body = values
        .iter()
        .enumerate()
        .map(|(index, value)| format!("{}. {}", index + 1, value.as_str().unwrap()))
        .collect::<Vec<_>>()
        .join("\n\n");
    Page {
        id: id.help().into(),
        title: id.title(locale).into(),
        topics: if matches!(id, Id::Sketch | Id::Extrude | Id::Cut | Id::Fillet) {
            vec![
                "sketch".into(),
                "extrude".into(),
                "cut".into(),
                "fillet".into(),
            ]
        } else {
            vec!["show-me".into(), id.key().into()]
        },
        keywords: if matches!(id, Id::Sketch | Id::Extrude | Id::Cut | Id::Fillet) {
            vec!["bracket".into(), id.key().into()]
        } else {
            vec![id.key().replace('_', " "), id.recipe().into()]
        },
        description: id.summary(locale).into(),
        body,
        related_recipes: vec![id.recipe().into()],
        status: "stable".into(),
    }
}
pub fn from_help(id: &str) -> Option<Id> {
    ALL.into_iter().find(|lesson| lesson.help() == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_lesson_has_a_unique_round_trip_identity_and_source_contract() {
        let mut keys = std::collections::HashSet::new();
        let mut pages = std::collections::HashSet::new();
        for id in ALL {
            assert!(keys.insert(id.key()));
            assert!(pages.insert(id.help()));
            assert_eq!(Id::from_key(id.key()), Some(id));
            assert_eq!(from_help(id.help()), Some(id));
            assert_eq!(
                serde_json::from_value::<Id>(serde_json::json!(id.key())).unwrap(),
                id
            );
            assert_eq!(id.captions().len(), id.frame_count());
            assert!(id.captions().iter().all(|caption| caption
                .split_once('.')
                .is_some_and(|(key, _)| key == id.key())));
            if let Some(prerequisite) = id.prerequisite() {
                assert_ne!(prerequisite, id);
            }
        }
        assert!(Id::from_key("unknown-operation").is_none());
    }

    #[test]
    fn all_guides_share_localized_help_and_search_identity() {
        let store = crate::HelpStore::bundled();
        for id in ALL {
            let hits = store.search(id.title("en"), Some(10));
            assert!(
                hits.iter().any(|hit| hit.id == id.help()),
                "{} must be searchable",
                id.key()
            );
            for locale in LOCALES {
                let result = store.get_localized(id.help(), locale).unwrap();
                let authored = page(locale, id);
                assert_eq!(result.body, authored.body);
                assert_eq!(result.title, authored.title);
                assert_eq!(result.description, authored.description);
                assert_eq!(result.related_recipes, [id.recipe()]);
                assert_eq!(result.demonstrations, [id]);
                assert!(!result.truncated);
            }
            assert_eq!(page("unknown-locale", id), page("en", id));
        }
    }

    #[test]
    fn step_chooser_labels_preserve_translated_unicode() {
        for locale in LOCALES {
            let dictionary = catalog()[locale].as_object().unwrap();
            for (key, value) in dictionary {
                if key.starts_with("action.") || key == "ui.chooseStep" {
                    let value = value.as_str().unwrap();
                    assert!(
                        !value.is_empty() && !value.contains(['?', '\u{fffd}']),
                        "Corrupted chooser label {locale}/{key}: {value}"
                    );
                }
            }
        }
        assert_eq!(text("zh-CN", "ui.chooseStep"), "选择步骤");
        assert_eq!(text("de", "ui.chooseStep"), "Schritt wählen");
        assert_eq!(text("es", "action.extrude.apply"), "Aplicar la extrusión");
    }

    #[test]
    fn guided_examples_have_explicit_localized_identity_and_playback_labels() {
        for locale in LOCALES {
            for key in [
                "ui.guidedExamples",
                "ui.instructions",
                "ui.hideInstructions",
                "ui.resume",
                "ui.back",
                "ui.backToGuides",
                "sketch.example",
                "extrude.example",
                "cut.example",
                "fillet.example",
            ] {
                let translated = catalog()[locale][key]
                    .as_str()
                    .expect("Explicit locale required");
                assert!(
                    !translated.is_empty()
                        && !translated.contains('?')
                        && !translated.contains('\u{fffd}'),
                    "{locale}: {key}"
                );
            }
            for lesson in ALL {
                for suffix in ["example", "outcome"] {
                    let key = format!("{}.{suffix}", lesson.key());
                    assert!(
                        catalog()[locale][&key]
                            .as_str()
                            .is_some_and(|text| !text.is_empty() && !text.contains('\u{fffd}')),
                        "{locale}/{key}"
                    );
                }
            }
            assert_ne!(text(locale, "extrude.example"), text(locale, "cut.example"));
            assert_ne!(text(locale, "extrude.example"), Id::Extrude.title(locale));
            assert_ne!(text(locale, "cut.example"), Id::Cut.title(locale));
        }
    }

    #[test]
    fn lessons_have_complete_locales_and_matching_authored_builds() {
        let english = catalog()["en"].as_object().unwrap();
        for locale in LOCALES {
            let dictionary = catalog()[locale].as_object().unwrap();
            assert_eq!(
                dictionary.keys().collect::<Vec<_>>(),
                english.keys().collect::<Vec<_>>(),
                "{locale}"
            );
            for (key, value) in dictionary {
                assert!(
                    value.as_str().is_some_and(|s| !s.is_empty())
                        || value.as_array().is_some_and(|a| !a.is_empty()
                            && a.iter().all(|s| s.as_str().is_some_and(|s| !s.is_empty()))),
                    "{locale}/{key}"
                );
            }
            for lesson in ALL {
                let page = page(locale, lesson);
                assert!(!page.body.contains("limo-cad-help"));
                assert_eq!(page.related_recipes, [lesson.recipe()]);
                for caption in lesson.captions() {
                    for key in [
                        caption.to_string(),
                        format!("step.{caption}"),
                        format!("action.{caption}"),
                    ] {
                        assert!(
                            dictionary[&key]
                                .as_str()
                                .is_some_and(|text| !text.is_empty()),
                            "{locale}/{key}"
                        );
                    }
                }
                for cue in lesson.cues() {
                    assert!(!text(locale, &format!("step.{cue}")).is_empty());
                }
            }
        }
        assert_eq!(Id::Cut.prerequisite(), Some(Id::Extrude));
        assert_eq!(Id::Extrude.prerequisite(), Some(Id::Sketch));
        for id in ALL {
            let file = crate::knowledge_files()
                .iter()
                .find(|file| {
                    crate::parse_markdown(file.path, file.text)
                        .is_some_and(|page| page.id == id.help())
                })
                .unwrap();
            let embedded = crate::parse_markdown(file.path, file.text).unwrap();
            let authored = page("en", id);
            assert_eq!(
                embedded.body, authored.body,
                "English knowledge must be generated from the lesson catalog"
            );
            assert_eq!(embedded.title, authored.title);
            assert_eq!(embedded.description, authored.description);
        }
    }
    #[test]
    fn sketch_titles_match_native_operation_labels_in_every_locale() {
        let labels = [
            (
                "en",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../assets/i18n/en.json"
                )),
            ),
            (
                "zh-CN",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../assets/i18n/zh-CN.json"
                )),
            ),
            (
                "es",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../assets/i18n/es.json"
                )),
            ),
            (
                "de",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../assets/i18n/de.json"
                )),
            ),
        ];
        for (locale, source) in labels {
            let operations: Value = serde_json::from_str(source).unwrap();
            for (id, label_key) in [
                (Id::SketchLine, "ribbon.sketch.line"),
                (Id::SketchMidpointLine, "ribbon.sketch.midpointLine"),
                (Id::SketchPoint, "ribbon.sketch.point"),
                (Id::SketchRectangle, "ribbon.sketch.rectangle"),
                (Id::SketchCircle, "ribbon.sketch.circle"),
                (Id::SketchArc, "ribbon.sketch.arc"),
                (Id::SketchSpline, "ribbon.sketch.spline"),
                (Id::SketchPolygon, "ribbon.sketch.polygon"),
                (Id::SketchSlot, "ribbon.sketch.slot"),
                (Id::SketchMoveCopy, "ribbon.sketch.moveCopy"),
                (Id::SketchScale, "ribbon.sketch.sketchScale"),
                (Id::SketchOffset, "ribbon.sketch.offset"),
                (Id::SketchTrim, "ribbon.sketch.trim"),
                (Id::SketchExtend, "ribbon.sketch.extend"),
                (Id::SketchBreak, "ribbon.sketch.break"),
                (Id::SketchFillet, "ribbon.sketch.fillet"),
                (Id::SketchChamfer, "ribbon.sketch.chamfer"),
                (Id::SketchDimension, "ribbon.sketch.sketchDimension"),
                (Id::SketchMirror, "ribbon.sketch.mirror"),
                (
                    Id::SketchRectangularPattern,
                    "ribbon.sketch.patternRectangular",
                ),
                (Id::SketchCircularPattern, "ribbon.sketch.patternCircular"),
                (Id::SketchCoincident, "ribbon.sketch.coincident"),
                (
                    Id::SketchHorizontalVertical,
                    "ribbon.sketch.horizontalVertical",
                ),
                (Id::SketchTangent, "ribbon.sketch.tangent"),
                (Id::SketchParallel, "ribbon.sketch.parallel"),
                (Id::SketchPerpendicular, "ribbon.sketch.perpendicular"),
                (Id::SketchEqual, "ribbon.sketch.equal"),
                (Id::SketchFixUnfix, "ribbon.sketch.fixUnfix"),
                (Id::SketchMidpoint, "ribbon.sketch.midpoint"),
                (Id::SketchCollinear, "ribbon.sketch.collinear"),
                (Id::SketchConcentric, "ribbon.sketch.concentric"),
                (Id::SketchSymmetry, "ribbon.sketch.symmetry"),
            ] {
                let label = label_key
                    .split('.')
                    .fold(&operations, |value, key| &value[key]);
                assert_eq!(
                    id.title(locale),
                    label.as_str().unwrap(),
                    "{locale}/{label_key}"
                );
                assert_eq!(id.prerequisite(), Some(Id::Sketch));
                assert_eq!(id.frame_count(), 4);
            }
        }
    }
}
