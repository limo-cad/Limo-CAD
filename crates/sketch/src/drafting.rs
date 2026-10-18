//! Authored drafting graph, independent of generated ink and solver state.
//!
//! This contract is not yet part of project persistence or host commands. Its
//! adoption must bump the project schema and preserve existing sheet identities.
//! Validate against the owning drawing's sheet IDs before accepting a candidate.
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;
mod validate;

macro_rules! ids {
    ($($name:ident),+) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);
    )+};
}
ids!(
    EntityId,
    LayerId,
    LineTypeId,
    BlockId,
    AttributeId,
    ReferenceId,
    ViewportId
);

/// IDs share one document-local allocation space. Zero is never an object ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub version: u32,
    pub next_id: u64,
    pub layers: Vec<Layer>,
    pub line_types: Vec<LineType>,
    pub blocks: Vec<Block>,
    pub references: Vec<Reference>,
    pub entities: Vec<Entity>,
    pub viewports: Vec<Viewport>,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            next_id: 1,
            layers: vec![],
            line_types: vec![],
            blocks: vec![],
            references: vec![],
            entities: vec![],
            viewports: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub plottable: bool,
    pub style: Style,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Style {
    /// None is continuous. Entity None inherits the entire layer style.
    pub line_type: Option<LineTypeId>,
    pub color_rgb: [u8; 3],
    pub width_mm: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternSpace {
    Model,
    Paper,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineType {
    pub id: LineTypeId,
    pub name: String,
    /// Positive is ink, negative is a gap, zero is a dot; lengths in mm.
    pub pattern: Vec<f64>,
    pub space: PatternSpace,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Space {
    Model,
    Paper(u64),
    Block(BlockId),
}

/// x' = a*x + b*y + tx; y' = c*x + d*y + ty. Supports mirrors and
/// nonuniform scale without rewriting the definition's exact geometry.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Affine(pub [f64; 6]);
impl Default for Affine {
    fn default() -> Self {
        Self([1., 0., 0., 1., 0., 0.])
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub id: BlockId,
    pub name: String,
    pub base_point: [f64; 2],
    pub attributes: Vec<Attribute>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attribute {
    pub id: AttributeId,
    pub tag: String,
    pub default_value: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributeValue {
    pub attribute: AttributeId,
    pub value: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceMode {
    Attach,
    Overlay,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Resolution {
    Unresolved {
        diagnostic: String,
    },
    Resolved {
        block: BlockId,
        sha256: String,
    },
    Stale {
        block: BlockId,
        sha256: String,
        diagnostic: String,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub id: ReferenceId,
    /// Locator only; validation never performs filesystem or network access.
    pub locator: String,
    pub mode: ReferenceMode,
    /// Retained source unit conversion. Resolved block coordinates are in mm.
    pub source_mm_per_unit: f64,
    pub resolution: Resolution,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: EntityId,
    pub space: Space,
    pub layer: LayerId,
    pub style: Option<Style>,
    pub content: Content,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Content {
    Curve {
        curve: Curve,
    },
    Text {
        position: [f64; 2],
        height_mm: f64,
        rotation_rad: f64,
        text: String,
    },
    Block {
        block: BlockId,
        transform: Affine,
        attributes: Vec<AttributeValue>,
    },
    Reference {
        reference: ReferenceId,
        transform: Affine,
    },
}
/// Exact authored curves. Evaluation/rendering adapters remain separate work;
/// the NURBS variant must never be reinterpreted as a fit-point spline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Curve {
    Line {
        start: [f64; 2],
        end: [f64; 2],
    },
    /// Signed nonzero sweep, no larger than one revolution.
    Arc {
        center: [f64; 2],
        radius: f64,
        start_rad: f64,
        sweep_rad: f64,
    },
    Ellipse {
        center: [f64; 2],
        axis_u: [f64; 2],
        axis_v: [f64; 2],
        start_rad: f64,
        sweep_rad: f64,
    },
    Polyline {
        points: Vec<[f64; 2]>,
        closed: bool,
    },
    Nurbs {
        degree: u32,
        control_points: Vec<[f64; 2]>,
        weights: Vec<f64>,
        knots: Vec<f64>,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    pub id: ViewportId,
    pub sheet_id: u64,
    /// Model center mapped to paper_center_mm. Rotation is CCW in sheet axes.
    pub model_center_mm: [f64; 2],
    pub paper_center_mm: [f64; 2],
    pub rotation_rad: f64,
    pub paper_mm_per_model_mm: f64,
    /// Axis-aligned clipping rectangle: left, top, width, height in paper mm.
    pub clip_mm: [f64; 4],
    pub hidden_layers: Vec<LayerId>,
}
