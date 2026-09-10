//! Persistent, host-neutral technical drawing document.
//!
//! Drawing sheets are part of the authoritative project model.  They store
//! view intent (camera, scale, placement and display options), not generated
//! line work: desktop OCCT HLR and the browser development fallback regenerate
//! projection geometry from the current solid bodies.

use std::collections::HashSet;

use nbcad_core::{BodyId, EdgeId};
use serde::{Deserialize, Serialize};

const MAX_SHEETS: usize = 64;
const MAX_VIEWS_PER_SHEET: usize = 256;
const MAX_ANNOTATIONS_PER_SHEET: usize = 2_048;
const MAX_NOTE_LENGTH: usize = 4_096;
const MAX_REVISIONS_PER_SHEET: usize = 512;
const MAX_BOM_ITEMS_PER_SHEET: usize = 4_096;
const MAX_DRAWING_TEMPLATES: usize = 128;

fn first_id() -> u64 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingDocumentDto {
    #[serde(default)]
    pub sheets: Vec<DrawingSheetDto>,
    #[serde(default)]
    pub active_sheet_id: Option<u64>,
    #[serde(default = "first_id")]
    pub next_sheet_id: u64,
    #[serde(default = "first_id")]
    pub next_view_id: u64,
    #[serde(default = "first_id")]
    pub next_annotation_id: u64,
    #[serde(default = "first_id")]
    pub next_revision_id: u64,
    #[serde(default = "first_id")]
    pub next_bom_item_id: u64,
    /// Project-local company templates. A sheet receives a full copy when a
    /// template is applied so an issued drawing never changes retroactively.
    #[serde(default)]
    pub templates: Vec<DrawingTemplateDto>,
    #[serde(default = "first_id")]
    pub next_template_id: u64,
}

impl Default for DrawingDocumentDto {
    fn default() -> Self {
        Self {
            sheets: Vec::new(),
            active_sheet_id: None,
            next_sheet_id: 1,
            next_view_id: 1,
            next_annotation_id: 1,
            next_revision_id: 1,
            next_bom_item_id: 1,
            templates: Vec::new(),
            next_template_id: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingSheetDto {
    pub id: u64,
    pub name: String,
    pub format: DrawingSheetFormat,
    pub orientation: DrawingSheetOrientation,
    /// Drafting convention applied to projection layout, sheet labels, and
    /// default tolerance notes. Older drawing files default to ISO.
    #[serde(default)]
    pub standard: DrawingStandard,
    /// First-angle is the ISO default; third-angle is the ANSI default. It is
    /// persisted separately so a company template can intentionally override
    /// the standards-family default.
    #[serde(default)]
    pub projection_method: DrawingProjectionMethod,
    #[serde(default)]
    pub tolerance_note: DrawingToleranceNoteDto,
    #[serde(default)]
    pub title_block: DrawingTitleBlockDto,
    #[serde(default)]
    pub views: Vec<DrawingViewDto>,
    #[serde(default)]
    pub annotations: Vec<DrawingAnnotationDto>,
    /// Standards-aware screen/print/DXF presentation. Company templates copy
    /// their values here, keeping every issued sheet self-contained.
    #[serde(default)]
    pub style: DrawingSheetStyleDto,
    #[serde(default)]
    pub template_name: String,
    #[serde(default)]
    pub revisions: Vec<DrawingRevisionDto>,
    #[serde(default)]
    pub bom: Vec<DrawingBomItemDto>,
    #[serde(default)]
    pub release: DrawingReleaseDto,
    #[serde(default)]
    pub revision_table_position: Option<[f64; 2]>,
    #[serde(default)]
    pub bom_table_position: Option<[f64; 2]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingSheetFormat {
    A0,
    A1,
    A2,
    A4,
    A3,
    /// ANSI A / US Letter. Kept as `letter` for project-file compatibility.
    Letter,
    AnsiB,
    AnsiC,
    AnsiD,
    AnsiE,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingSheetOrientation {
    Landscape,
    Portrait,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingStandard {
    #[default]
    Iso,
    Ansi,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingProjectionMethod {
    #[default]
    FirstAngle,
    ThirdAngle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingTolerancePreset {
    #[default]
    None,
    Iso2768Fine,
    Iso2768Medium,
    Iso2768Coarse,
    Iso2768VeryCoarse,
    AnsiDecimal,
    Custom,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawingToleranceNoteDto {
    #[serde(default)]
    pub preset: DrawingTolerancePreset,
    #[serde(default)]
    pub custom: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawingTitleBlockDto {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub drawing_number: String,
    #[serde(default)]
    pub revision: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub checked_by: String,
    #[serde(default)]
    pub approved_by: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub finish: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingLineStyleDto {
    pub width_mm: f64,
    #[serde(default)]
    pub dash_mm: Vec<f64>,
}

impl DrawingLineStyleDto {
    fn continuous(width_mm: f64) -> Self {
        Self {
            width_mm,
            dash_mm: Vec::new(),
        }
    }

    fn dashed(width_mm: f64, dash_mm: &[f64]) -> Self {
        Self {
            width_mm,
            dash_mm: dash_mm.to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingSheetStyleDto {
    pub name: String,
    pub font_family: String,
    pub text_height_mm: f64,
    pub small_text_height_mm: f64,
    pub arrow_size_mm: f64,
    pub visible: DrawingLineStyleDto,
    pub hidden: DrawingLineStyleDto,
    pub center: DrawingLineStyleDto,
    pub cutting_plane: DrawingLineStyleDto,
    pub phantom: DrawingLineStyleDto,
    pub break_line: DrawingLineStyleDto,
    pub dimension: DrawingLineStyleDto,
    pub extension: DrawingLineStyleDto,
    pub leader: DrawingLineStyleDto,
    pub hatch: DrawingLineStyleDto,
    pub hatch_angle_deg: f64,
    pub hatch_spacing_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingTemplateDto {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub standard: DrawingStandard,
    #[serde(default)]
    pub projection_method: DrawingProjectionMethod,
    #[serde(default)]
    pub tolerance_note: DrawingToleranceNoteDto,
    #[serde(default)]
    pub title_defaults: DrawingTitleBlockDto,
    #[serde(default)]
    pub style: DrawingSheetStyleDto,
}

impl Default for DrawingSheetStyleDto {
    fn default() -> Self {
        Self {
            name: "noBS CAD Default".to_string(),
            font_family: "Arial, Helvetica, sans-serif".to_string(),
            text_height_mm: 3.5,
            small_text_height_mm: 2.5,
            arrow_size_mm: 2.5,
            visible: DrawingLineStyleDto::continuous(0.5),
            hidden: DrawingLineStyleDto::dashed(0.25, &[4.0, 2.0]),
            center: DrawingLineStyleDto::dashed(0.25, &[8.0, 1.5, 1.2, 1.5]),
            cutting_plane: DrawingLineStyleDto::dashed(0.7, &[10.0, 2.0, 2.0, 2.0]),
            phantom: DrawingLineStyleDto::dashed(0.25, &[10.0, 1.5, 1.2, 1.5, 1.2, 1.5]),
            break_line: DrawingLineStyleDto::continuous(0.35),
            dimension: DrawingLineStyleDto::continuous(0.25),
            extension: DrawingLineStyleDto::continuous(0.25),
            leader: DrawingLineStyleDto::continuous(0.25),
            hatch: DrawingLineStyleDto::continuous(0.18),
            hatch_angle_deg: 45.0,
            hatch_spacing_mm: 2.5,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingReleaseStatus {
    #[default]
    Draft,
    InReview,
    Released,
    Superseded,
    Obsolete,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawingReleaseDto {
    #[serde(default)]
    pub status: DrawingReleaseStatus,
    #[serde(default)]
    pub released_revision: String,
    #[serde(default)]
    pub released_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawingRevisionDto {
    pub id: u64,
    pub revision: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub checked_by: String,
    #[serde(default)]
    pub approved_by: String,
    #[serde(default)]
    pub change_order: String,
    #[serde(default)]
    pub status: DrawingReleaseStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingBomItemDto {
    pub id: u64,
    pub item_number: String,
    #[serde(default)]
    pub body_id: Option<BodyId>,
    #[serde(default)]
    pub part_number: String,
    #[serde(default)]
    pub description: String,
    pub quantity: f64,
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub finish: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingViewScope {
    #[default]
    Definition,
    Assembly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingViewDto {
    /// Definition coordinates or authoritative solved assembly occurrences.
    #[serde(default)]
    pub scope: DrawingViewScope,
    /// Empty selects all visible occurrences; selected containers include descendants.
    #[serde(default)]
    pub occurrence_ids: Vec<nbcad_assembly::OccurrenceId>,
    #[serde(default)]
    pub id: u64,
    pub name: String,
    pub kind: DrawingViewKind,
    /// Direction from the model toward the viewer, in model coordinates.
    pub direction: [f64; 3],
    /// Desired page-up direction before orthogonalization against `direction`.
    pub up: [f64; 3],
    /// View origin on the sheet, in millimetres from the upper-left paper edge.
    pub position: [f64; 2],
    /// Paper millimetres per model millimetre (1.0 is a 1:1 view).
    pub scale: f64,
    #[serde(default)]
    pub body_ids: Vec<BodyId>,
    #[serde(default)]
    pub show_hidden_lines: bool,
    #[serde(default)]
    pub show_tangent_edges: bool,
    /// Related orthographic views retain their base relationship so manual
    /// dragging can preserve drafting alignment.
    #[serde(default)]
    pub parent_view_id: Option<u64>,
    #[serde(default)]
    pub alignment: DrawingViewAlignment,
    /// Optional associative parent relationship for sections, details,
    /// auxiliary, broken, and removed-section views.
    #[serde(default)]
    pub derivation: Option<DrawingViewDerivationDto>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingViewAlignment {
    #[default]
    Free,
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingViewKind {
    Front,
    Rear,
    Left,
    Right,
    Top,
    Bottom,
    Isometric,
    Custom,
    Section,
    Detail,
    Auxiliary,
    Broken,
    RemovedSection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingBreakAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DrawingViewDerivationDto {
    Section {
        parent_view_id: u64,
        first: DrawingTopologyAnchorRefDto,
        second: DrawingTopologyAnchorRefDto,
        label: String,
        #[serde(default)]
        depth: Option<f64>,
        hatch_angle_deg: f64,
        hatch_spacing_mm: f64,
    },
    Detail {
        parent_view_id: u64,
        center: DrawingTopologyAnchorRefDto,
        radius: f64,
        label: String,
    },
    Auxiliary {
        parent_view_id: u64,
        reference: DrawingLineRefDto,
        label: String,
        #[serde(default)]
        flipped: bool,
    },
    Broken {
        parent_view_id: u64,
        axis: DrawingBreakAxis,
        first: f64,
        second: f64,
        gap_mm: f64,
    },
    RemovedSection {
        parent_view_id: u64,
        first: DrawingTopologyAnchorRefDto,
        second: DrawingTopologyAnchorRefDto,
        label: String,
        hatch_angle_deg: f64,
        hatch_spacing_mm: f64,
    },
}

/// Stable model reference used by associative drawing annotations. The edge
/// id/key pair is authoritative when topology survives a recompute; the
/// fallback point is retained for diagnostics and the explicit reassociation
/// workflow. It is never accepted silently as replacement manufacturing
/// topology.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingTopologyAnchorRefDto {
    #[serde(default)]
    pub occurrence_id: Option<nbcad_assembly::OccurrenceId>,
    pub body_id: BodyId,
    pub edge_id: EdgeId,
    pub edge_key: String,
    pub endpoint: DrawingEdgeEndpoint,
    pub fallback_point: [f64; 3],
    /// Resolve this point from the analytic center of the referenced circle.
    /// Missing in older project files, where it defaults to an edge endpoint.
    #[serde(default)]
    pub circle_center: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingEdgeEndpoint {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingLinearDimensionMode {
    Aligned,
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingLineDimensionMode {
    Length,
    Distance,
    Angle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingRadialDimensionMode {
    Diameter,
    Radius,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingDimensionToleranceMode {
    #[default]
    None,
    Symmetric,
    Deviation,
    Limits,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingDimensionToleranceDto {
    #[serde(default)]
    pub mode: DrawingDimensionToleranceMode,
    #[serde(default)]
    pub upper: f64,
    #[serde(default)]
    pub lower: f64,
}

impl Default for DrawingDimensionToleranceDto {
    fn default() -> Self {
        Self {
            mode: DrawingDimensionToleranceMode::None,
            upper: 0.0,
            lower: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingSecondaryUnit {
    Millimetre,
    Centimetre,
    #[default]
    Inch,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingDualUnitPlacement {
    #[default]
    Bracketed,
    Stacked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawingDualUnitDto {
    pub unit: DrawingSecondaryUnit,
    #[serde(default = "default_dimension_precision")]
    pub precision: u8,
    #[serde(default)]
    pub placement: DrawingDualUnitPlacement,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DrawingDimensionPresentationDto {
    #[serde(default)]
    pub tolerance: DrawingDimensionToleranceDto,
    #[serde(default)]
    pub basic: bool,
    #[serde(default)]
    pub reference: bool,
    #[serde(default)]
    pub dual_units: Option<DrawingDualUnitDto>,
    #[serde(default)]
    pub fit_class: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingChainDimensionLayout {
    Chain,
    Baseline,
    Continued,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingOrdinateAxis {
    X,
    Y,
    Both,
}

/// Stable circular-edge reference used by radial dimensions and hole notes.
/// Exact topology is preferred; the fitted model-space circle is retained for
/// diagnostics and explicit user-confirmed reassociation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingCircularRefDto {
    #[serde(default)]
    pub occurrence_id: Option<nbcad_assembly::OccurrenceId>,
    pub body_id: BodyId,
    pub edge_id: EdgeId,
    pub edge_key: String,
    pub fallback_center: [f64; 3],
    pub fallback_normal: [f64; 3],
    pub fallback_radius: f64,
    pub closed: bool,
}

/// Stable exact-topology reference to a straight model edge. The OCCT edge
/// identity is authoritative; endpoints are retained for diagnostics and
/// explicit user-confirmed reassociation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingLineRefDto {
    #[serde(default)]
    pub occurrence_id: Option<nbcad_assembly::OccurrenceId>,
    pub body_id: BodyId,
    pub edge_id: EdgeId,
    pub edge_key: String,
    pub fallback_start: [f64; 3],
    pub fallback_end: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DrawingAttachmentRefDto {
    Anchor {
        reference: DrawingTopologyAnchorRefDto,
    },
    Line {
        reference: DrawingLineRefDto,
    },
    Circle {
        reference: DrawingCircularRefDto,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingGdtCharacteristic {
    Straightness,
    Flatness,
    Circularity,
    Cylindricity,
    ProfileLine,
    ProfileSurface,
    Angularity,
    Perpendicularity,
    Parallelism,
    Position,
    Concentricity,
    Symmetry,
    CircularRunout,
    TotalRunout,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingMaterialCondition {
    #[default]
    None,
    Maximum,
    Least,
    Regardless,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawingDatumReferenceDto {
    pub label: String,
    #[serde(default)]
    pub material_condition: DrawingMaterialCondition,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingSurfaceLay {
    #[default]
    None,
    Parallel,
    Perpendicular,
    Crossed,
    Multidirectional,
    Circular,
    Radial,
    Particulate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingWeldType {
    Fillet,
    SquareGroove,
    VGroove,
    BevelGroove,
    UGroove,
    JGroove,
    PlugSlot,
    Spot,
    Seam,
    Surfacing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingWeldSide {
    Arrow,
    Other,
    Both,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingWeldContour {
    #[default]
    None,
    Flush,
    Convex,
    Concave,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingHoleStyle {
    #[default]
    Simple,
    Counterbore,
    Countersink,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DrawingAnnotationDto {
    LinearDimension {
        id: u64,
        view_id: u64,
        first: DrawingTopologyAnchorRefDto,
        second: DrawingTopologyAnchorRefDto,
        mode: DrawingLinearDimensionMode,
        /// Signed paper-space offset in millimetres from the measured span.
        offset: f64,
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        suffix: String,
        #[serde(default = "default_dimension_precision")]
        precision: u8,
        #[serde(default)]
        presentation: DrawingDimensionPresentationDto,
    },
    LineDimension {
        id: u64,
        view_id: u64,
        first: DrawingLineRefDto,
        #[serde(default)]
        second: Option<DrawingLineRefDto>,
        mode: DrawingLineDimensionMode,
        /// Paper-space cursor/drag position. Rendering constrains it according
        /// to the semantic relationship between the selected exact edges.
        position: [f64; 2],
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        suffix: String,
        #[serde(default = "default_dimension_precision")]
        precision: u8,
        #[serde(default)]
        presentation: DrawingDimensionPresentationDto,
    },
    PointLineDimension {
        id: u64,
        view_id: u64,
        point: DrawingTopologyAnchorRefDto,
        line: DrawingLineRefDto,
        /// Paper-space cursor/drag position. Rendering constrains this point
        /// parallel to the referenced edge so the measured span stays normal.
        position: [f64; 2],
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        suffix: String,
        #[serde(default = "default_dimension_precision")]
        precision: u8,
        #[serde(default)]
        presentation: DrawingDimensionPresentationDto,
    },
    Note {
        id: u64,
        text: String,
        /// Paper-space millimetres from the upper-left sheet corner.
        position: [f64; 2],
    },
    RadialDimension {
        id: u64,
        view_id: u64,
        feature: DrawingCircularRefDto,
        mode: DrawingRadialDimensionMode,
        leader_angle_deg: f64,
        offset: f64,
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        suffix: String,
        #[serde(default = "default_dimension_precision")]
        precision: u8,
        #[serde(default)]
        presentation: DrawingDimensionPresentationDto,
    },
    AngularDimension {
        id: u64,
        view_id: u64,
        vertex: DrawingTopologyAnchorRefDto,
        first: DrawingTopologyAnchorRefDto,
        second: DrawingTopologyAnchorRefDto,
        radius: f64,
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        suffix: String,
        #[serde(default = "default_dimension_precision")]
        precision: u8,
        #[serde(default)]
        presentation: DrawingDimensionPresentationDto,
    },
    HoleNote {
        id: u64,
        view_id: u64,
        feature: DrawingCircularRefDto,
        position: [f64; 2],
        quantity: u32,
        diameter: f64,
        #[serde(default)]
        depth: Option<f64>,
        #[serde(default)]
        thread: String,
        #[serde(default)]
        note: String,
        #[serde(default)]
        source_feature_id: Option<u64>,
        #[serde(default)]
        feature_name: String,
        #[serde(default)]
        hole_style: DrawingHoleStyle,
        #[serde(default)]
        counterbore_diameter: Option<f64>,
        #[serde(default)]
        counterbore_depth: Option<f64>,
        #[serde(default)]
        countersink_diameter: Option<f64>,
        #[serde(default)]
        countersink_angle_deg: Option<f64>,
        #[serde(default)]
        thread_depth: Option<f64>,
        #[serde(default)]
        pattern_note: String,
    },
    ChamferNote {
        id: u64,
        view_id: u64,
        first: DrawingTopologyAnchorRefDto,
        second: DrawingTopologyAnchorRefDto,
        position: [f64; 2],
        length: f64,
        angle_deg: f64,
        #[serde(default)]
        prefix: String,
    },
    CenterMark {
        id: u64,
        view_id: u64,
        feature: DrawingCircularRefDto,
        /// Paper-space extension beyond the circular edge.
        extension: f64,
    },
    CenterLine {
        id: u64,
        view_id: u64,
        first: DrawingCircularRefDto,
        second: DrawingCircularRefDto,
        /// Paper-space extension beyond the two circular edges.
        extension: f64,
    },
    CenterLineBetweenEdges {
        id: u64,
        view_id: u64,
        first: DrawingLineRefDto,
        second: DrawingLineRefDto,
        /// Paper-space extension beyond the selected parallel edges.
        extension: f64,
    },
    AutomaticSymmetryAxis {
        id: u64,
        view_id: u64,
        axis: DrawingOrdinateAxis,
        extension: f64,
    },
    BoltCircleCenterLine {
        id: u64,
        view_id: u64,
        features: Vec<DrawingCircularRefDto>,
        extension: f64,
    },
    ChainDimension {
        id: u64,
        view_id: u64,
        anchors: Vec<DrawingTopologyAnchorRefDto>,
        mode: DrawingLinearDimensionMode,
        layout: DrawingChainDimensionLayout,
        offset: f64,
        spacing: f64,
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        suffix: String,
        #[serde(default = "default_dimension_precision")]
        precision: u8,
        #[serde(default)]
        presentation: DrawingDimensionPresentationDto,
    },
    OrdinateDimension {
        id: u64,
        view_id: u64,
        origin: DrawingTopologyAnchorRefDto,
        target: DrawingTopologyAnchorRefDto,
        axis: DrawingOrdinateAxis,
        offset: f64,
        #[serde(default = "default_dimension_precision")]
        precision: u8,
        #[serde(default)]
        presentation: DrawingDimensionPresentationDto,
    },
    ArcLengthDimension {
        id: u64,
        view_id: u64,
        feature: DrawingCircularRefDto,
        first: DrawingTopologyAnchorRefDto,
        second: DrawingTopologyAnchorRefDto,
        offset: f64,
        #[serde(default = "default_dimension_precision")]
        precision: u8,
        #[serde(default)]
        presentation: DrawingDimensionPresentationDto,
    },
    JoggedRadiusDimension {
        id: u64,
        view_id: u64,
        feature: DrawingCircularRefDto,
        jog: [f64; 2],
        position: [f64; 2],
        #[serde(default = "default_dimension_precision")]
        precision: u8,
        #[serde(default)]
        presentation: DrawingDimensionPresentationDto,
    },
    DatumFeature {
        id: u64,
        view_id: u64,
        attachment: DrawingAttachmentRefDto,
        label: String,
        position: [f64; 2],
        #[serde(default)]
        target_index: Option<u32>,
    },
    GdtFrame {
        id: u64,
        view_id: u64,
        attachment: DrawingAttachmentRefDto,
        position: [f64; 2],
        characteristic: DrawingGdtCharacteristic,
        tolerance: f64,
        #[serde(default)]
        diameter_zone: bool,
        #[serde(default)]
        material_condition: DrawingMaterialCondition,
        #[serde(default)]
        datums: Vec<DrawingDatumReferenceDto>,
        #[serde(default)]
        projected_zone: Option<f64>,
        #[serde(default)]
        free_state: bool,
    },
    SurfaceTexture {
        id: u64,
        view_id: u64,
        attachment: DrawingAttachmentRefDto,
        position: [f64; 2],
        roughness_ra: f64,
        #[serde(default)]
        process: String,
        #[serde(default)]
        lay: DrawingSurfaceLay,
        #[serde(default)]
        machining_allowance: Option<f64>,
    },
    EdgeRequirement {
        id: u64,
        view_id: u64,
        attachment: DrawingLineRefDto,
        position: [f64; 2],
        upper_deviation: f64,
        lower_deviation: f64,
        #[serde(default)]
        note: String,
    },
    WeldSymbol {
        id: u64,
        view_id: u64,
        attachment: DrawingLineRefDto,
        position: [f64; 2],
        weld_type: DrawingWeldType,
        side: DrawingWeldSide,
        size: f64,
        #[serde(default)]
        length: Option<f64>,
        #[serde(default)]
        pitch: Option<f64>,
        #[serde(default)]
        contour: DrawingWeldContour,
        #[serde(default)]
        finish: String,
        #[serde(default)]
        all_around: bool,
        #[serde(default)]
        field_weld: bool,
        #[serde(default)]
        tail: String,
    },
    ItemBalloon {
        id: u64,
        view_id: u64,
        attachment: DrawingAttachmentRefDto,
        position: [f64; 2],
        bom_item_id: u64,
    },
    RevisionCloud {
        id: u64,
        revision: String,
        points: Vec<[f64; 2]>,
    },
}

impl DrawingAnnotationDto {
    pub fn id(&self) -> u64 {
        match self {
            Self::LinearDimension { id, .. }
            | Self::LineDimension { id, .. }
            | Self::PointLineDimension { id, .. }
            | Self::Note { id, .. }
            | Self::RadialDimension { id, .. }
            | Self::AngularDimension { id, .. }
            | Self::HoleNote { id, .. }
            | Self::ChamferNote { id, .. }
            | Self::CenterMark { id, .. }
            | Self::CenterLine { id, .. }
            | Self::CenterLineBetweenEdges { id, .. }
            | Self::AutomaticSymmetryAxis { id, .. }
            | Self::BoltCircleCenterLine { id, .. }
            | Self::ChainDimension { id, .. }
            | Self::OrdinateDimension { id, .. }
            | Self::ArcLengthDimension { id, .. }
            | Self::JoggedRadiusDimension { id, .. }
            | Self::DatumFeature { id, .. }
            | Self::GdtFrame { id, .. }
            | Self::SurfaceTexture { id, .. }
            | Self::EdgeRequirement { id, .. }
            | Self::WeldSymbol { id, .. }
            | Self::ItemBalloon { id, .. }
            | Self::RevisionCloud { id, .. } => *id,
        }
    }
}

fn default_dimension_precision() -> u8 {
    2
}

impl DrawingDocumentDto {
    pub fn validate(&self) -> Result<(), String> {
        if self.sheets.len() > MAX_SHEETS {
            return Err(format!(
                "a project can contain at most {MAX_SHEETS} drawing sheets"
            ));
        }
        if self.templates.len() > MAX_DRAWING_TEMPLATES {
            return Err(format!(
                "a project can contain at most {MAX_DRAWING_TEMPLATES} drawing templates"
            ));
        }
        if self.next_sheet_id == 0
            || self.next_view_id == 0
            || self.next_annotation_id == 0
            || self.next_revision_id == 0
            || self.next_bom_item_id == 0
            || self.next_template_id == 0
        {
            return Err("drawing id counters must be non-zero".to_string());
        }

        let mut sheet_ids = HashSet::new();
        let mut view_ids = HashSet::new();
        let mut annotation_ids = HashSet::new();
        let mut revision_ids = HashSet::new();
        let mut bom_item_ids = HashSet::new();
        let mut template_ids = HashSet::new();
        let mut max_sheet_id = 0;
        let mut max_view_id = 0;
        let mut max_annotation_id = 0;
        let mut max_revision_id = 0;
        let mut max_bom_item_id = 0;
        let mut max_template_id = 0;
        for template in &self.templates {
            if template.id == 0 || !template_ids.insert(template.id) {
                return Err(format!(
                    "duplicate or zero drawing template id {}",
                    template.id
                ));
            }
            max_template_id = max_template_id.max(template.id);
            if template.name.trim().is_empty() || template.name.chars().count() > 256 {
                return Err(format!(
                    "drawing template {} has an invalid name",
                    template.id
                ));
            }
            if template.tolerance_note.custom.chars().count() > MAX_NOTE_LENGTH {
                return Err(format!(
                    "drawing template '{}' tolerance note is too long",
                    template.name
                ));
            }
            validate_sheet_style(&template.style, &template.name)?;
            validate_title_block(&template.title_defaults, &template.name)?;
        }
        for sheet in &self.sheets {
            if sheet.id == 0 || !sheet_ids.insert(sheet.id) {
                return Err(format!("duplicate or zero drawing sheet id {}", sheet.id));
            }
            max_sheet_id = max_sheet_id.max(sheet.id);
            if sheet.name.trim().is_empty() {
                return Err(format!("drawing sheet {} has an empty name", sheet.id));
            }
            if sheet.views.len() > MAX_VIEWS_PER_SHEET {
                return Err(format!(
                    "drawing sheet '{}' can contain at most {MAX_VIEWS_PER_SHEET} views",
                    sheet.name
                ));
            }
            if sheet.annotations.len() > MAX_ANNOTATIONS_PER_SHEET {
                return Err(format!(
                    "drawing sheet '{}' can contain at most {MAX_ANNOTATIONS_PER_SHEET} annotations",
                    sheet.name
                ));
            }
            if sheet.revisions.len() > MAX_REVISIONS_PER_SHEET {
                return Err(format!(
                    "drawing sheet '{}' can contain at most {MAX_REVISIONS_PER_SHEET} revisions",
                    sheet.name
                ));
            }
            if sheet.bom.len() > MAX_BOM_ITEMS_PER_SHEET {
                return Err(format!(
                    "drawing sheet '{}' can contain at most {MAX_BOM_ITEMS_PER_SHEET} BOM items",
                    sheet.name
                ));
            }

            if sheet.tolerance_note.custom.chars().count() > MAX_NOTE_LENGTH {
                return Err(format!(
                    "drawing sheet '{}' tolerance note exceeds {MAX_NOTE_LENGTH} characters",
                    sheet.name
                ));
            }
            validate_sheet_style(&sheet.style, &sheet.name)?;
            validate_title_block(&sheet.title_block, &sheet.name)?;
            for revision in &sheet.revisions {
                if revision.id == 0 || !revision_ids.insert(revision.id) {
                    return Err(format!(
                        "duplicate or zero drawing revision id {}",
                        revision.id
                    ));
                }
                max_revision_id = max_revision_id.max(revision.id);
                if revision.revision.trim().is_empty()
                    || revision.revision.chars().count() > 32
                    || revision.description.chars().count() > MAX_NOTE_LENGTH
                {
                    return Err(format!(
                        "drawing revision {} contains invalid values",
                        revision.id
                    ));
                }
            }
            let sheet_bom_ids = sheet.bom.iter().map(|item| item.id).collect::<HashSet<_>>();
            for item in &sheet.bom {
                if item.id == 0 || !bom_item_ids.insert(item.id) {
                    return Err(format!("duplicate or zero drawing BOM item id {}", item.id));
                }
                max_bom_item_id = max_bom_item_id.max(item.id);
                if item.item_number.trim().is_empty()
                    || !item.quantity.is_finite()
                    || item.quantity <= 0.0
                    || item.body_id.is_some_and(|id| id.0 == 0)
                    || item.description.chars().count() > MAX_NOTE_LENGTH
                {
                    return Err(format!(
                        "drawing BOM item {} contains invalid values",
                        item.id
                    ));
                }
            }
            for position in [sheet.revision_table_position, sheet.bom_table_position]
                .into_iter()
                .flatten()
            {
                if position.iter().any(|value| !value.is_finite()) {
                    return Err(format!(
                        "drawing sheet '{}' contains an invalid table position",
                        sheet.name
                    ));
                }
            }

            let sheet_view_ids = sheet
                .views
                .iter()
                .map(|view| view.id)
                .collect::<HashSet<_>>();

            for view in &sheet.views {
                if view.id == 0 || !view_ids.insert(view.id) {
                    return Err(format!("duplicate or zero drawing view id {}", view.id));
                }
                max_view_id = max_view_id.max(view.id);
                if view.name.trim().is_empty() {
                    return Err(format!("drawing view {} has an empty name", view.id));
                }
                if !view.scale.is_finite() || view.scale <= 0.0 || view.scale > 10_000.0 {
                    return Err(format!("drawing view '{}' has an invalid scale", view.name));
                }
                if view.position.iter().any(|value| !value.is_finite())
                    || view.direction.iter().any(|value| !value.is_finite())
                    || view.up.iter().any(|value| !value.is_finite())
                {
                    return Err(format!(
                        "drawing view '{}' contains non-finite coordinates",
                        view.name
                    ));
                }
                let direction_length_sq = squared_length(view.direction);
                let up_length_sq = squared_length(view.up);
                if direction_length_sq < 1.0e-12 || up_length_sq < 1.0e-12 {
                    return Err(format!(
                        "drawing view '{}' needs non-zero direction and up vectors",
                        view.name
                    ));
                }
                let cross = cross(view.direction, view.up);
                if squared_length(cross) < direction_length_sq * up_length_sq * 1.0e-12 {
                    return Err(format!(
                        "drawing view '{}' direction and up vectors are parallel",
                        view.name
                    ));
                }
                let mut occurrence_ids = HashSet::new();
                if view.scope == DrawingViewScope::Definition && !view.occurrence_ids.is_empty() {
                    return Err(
                        "Definition drawing views cannot select assembly occurrences".into(),
                    );
                }
                for occurrence in &view.occurrence_ids {
                    if occurrence.0 == 0 || !occurrence_ids.insert(occurrence.0) {
                        return Err("Drawing occurrence IDs must be unique and nonzero".into());
                    }
                }
                let mut body_ids = HashSet::new();
                for body_id in &view.body_ids {
                    if body_id.0 == 0 || !body_ids.insert(body_id.0) {
                        return Err(format!(
                            "drawing view '{}' has a duplicate or zero body id",
                            view.name
                        ));
                    }
                }
                match view.parent_view_id {
                    Some(parent_id) if parent_id == view.id => {
                        return Err(format!(
                            "drawing view '{}' cannot align to itself",
                            view.name
                        ));
                    }
                    Some(parent_id) if !sheet_view_ids.contains(&parent_id) => {
                        return Err(format!(
                            "drawing view '{}' references missing parent view {parent_id}",
                            view.name
                        ));
                    }
                    None if view.alignment != DrawingViewAlignment::Free => {
                        return Err(format!(
                            "drawing view '{}' needs a parent for constrained alignment",
                            view.name
                        ));
                    }
                    _ => {}
                }
                validate_view_derivation(view, &sheet_view_ids)?;
            }

            for annotation in &sheet.annotations {
                let annotation_id = annotation.id();
                if annotation_id == 0 || !annotation_ids.insert(annotation_id) {
                    return Err(format!(
                        "duplicate or zero drawing annotation id {annotation_id}"
                    ));
                }
                max_annotation_id = max_annotation_id.max(annotation_id);
                match annotation {
                    DrawingAnnotationDto::LinearDimension {
                        view_id,
                        first,
                        second,
                        offset,
                        precision,
                        presentation,
                        ..
                    } => {
                        if !sheet_view_ids.contains(view_id) {
                            return Err(format!(
                                "drawing dimension {annotation_id} references missing view {view_id}"
                            ));
                        }
                        validate_anchor(first, annotation_id)?;
                        validate_anchor(second, annotation_id)?;
                        if first == second {
                            return Err(format!(
                                "drawing dimension {annotation_id} needs two distinct anchors"
                            ));
                        }
                        if !offset.is_finite() || offset.abs() > 1.0e6 {
                            return Err(format!(
                                "drawing dimension {annotation_id} has an invalid offset"
                            ));
                        }
                        if *precision > 6 {
                            return Err(format!(
                                "drawing dimension {annotation_id} precision exceeds 6 decimals"
                            ));
                        }
                        validate_dimension_presentation(presentation, annotation_id)?;
                    }
                    DrawingAnnotationDto::LineDimension {
                        view_id,
                        first,
                        second,
                        mode,
                        position,
                        precision,
                        presentation,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_line_ref(first, annotation_id)?;
                        match mode {
                            DrawingLineDimensionMode::Length if second.is_some() => {
                                return Err(format!(
                                    "drawing line dimension {annotation_id} length mode accepts one edge"
                                ));
                            }
                            DrawingLineDimensionMode::Distance
                            | DrawingLineDimensionMode::Angle => {
                                let second = second.as_ref().ok_or_else(|| {
                                    format!(
                                    "drawing line dimension {annotation_id} needs a second edge"
                                )
                                })?;
                                validate_line_ref(second, annotation_id)?;
                                if first == second {
                                    return Err(format!(
                                        "drawing line dimension {annotation_id} needs two distinct edges"
                                    ));
                                }
                            }
                            DrawingLineDimensionMode::Length => {}
                        }
                        if position
                            .iter()
                            .any(|value| !value.is_finite() || value.abs() > 1.0e6)
                        {
                            return Err(format!(
                                "drawing line dimension {annotation_id} has an invalid position"
                            ));
                        }
                        validate_precision(*precision, annotation_id)?;
                        validate_dimension_presentation(presentation, annotation_id)?;
                    }
                    DrawingAnnotationDto::PointLineDimension {
                        view_id,
                        point,
                        line,
                        position,
                        precision,
                        presentation,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_anchor(point, annotation_id)?;
                        validate_line_ref(line, annotation_id)?;
                        if position
                            .iter()
                            .any(|value| !value.is_finite() || value.abs() > 1.0e6)
                        {
                            return Err(format!(
                                "drawing point-to-line dimension {annotation_id} has an invalid position"
                            ));
                        }
                        validate_precision(*precision, annotation_id)?;
                        validate_dimension_presentation(presentation, annotation_id)?;
                    }
                    DrawingAnnotationDto::Note { text, position, .. } => {
                        if text.trim().is_empty() || text.chars().count() > MAX_NOTE_LENGTH {
                            return Err(format!(
                                "drawing note {annotation_id} must contain 1 to {MAX_NOTE_LENGTH} characters"
                            ));
                        }
                        if position.iter().any(|value| !value.is_finite()) {
                            return Err(format!(
                                "drawing note {annotation_id} has a non-finite position"
                            ));
                        }
                    }
                    DrawingAnnotationDto::RadialDimension {
                        view_id,
                        feature,
                        leader_angle_deg,
                        offset,
                        precision,
                        presentation,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_circular_ref(feature, annotation_id)?;
                        if !leader_angle_deg.is_finite() || !offset.is_finite() || *offset <= 0.0 {
                            return Err(format!(
                                "drawing radial dimension {annotation_id} has invalid leader geometry"
                            ));
                        }
                        validate_precision(*precision, annotation_id)?;
                        validate_dimension_presentation(presentation, annotation_id)?;
                    }
                    DrawingAnnotationDto::AngularDimension {
                        view_id,
                        vertex,
                        first,
                        second,
                        radius,
                        precision,
                        presentation,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_anchor(vertex, annotation_id)?;
                        validate_anchor(first, annotation_id)?;
                        validate_anchor(second, annotation_id)?;
                        if vertex == first || vertex == second || first == second {
                            return Err(format!(
                                "drawing angular dimension {annotation_id} needs three distinct anchors"
                            ));
                        }
                        if !radius.is_finite() || *radius <= 0.0 || *radius > 1.0e6 {
                            return Err(format!(
                                "drawing angular dimension {annotation_id} has an invalid radius"
                            ));
                        }
                        validate_precision(*precision, annotation_id)?;
                        validate_dimension_presentation(presentation, annotation_id)?;
                    }
                    DrawingAnnotationDto::HoleNote {
                        view_id,
                        feature,
                        position,
                        quantity,
                        diameter,
                        depth,
                        thread,
                        note,
                        counterbore_diameter,
                        counterbore_depth,
                        countersink_diameter,
                        countersink_angle_deg,
                        thread_depth,
                        feature_name,
                        pattern_note,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_circular_ref(feature, annotation_id)?;
                        if position.iter().any(|value| !value.is_finite())
                            || *quantity == 0
                            || *quantity > 10_000
                            || !diameter.is_finite()
                            || *diameter <= 0.0
                            || depth.is_some_and(|value| !value.is_finite() || value <= 0.0)
                            || counterbore_diameter
                                .is_some_and(|value| !value.is_finite() || value <= *diameter)
                            || counterbore_depth
                                .is_some_and(|value| !value.is_finite() || value <= 0.0)
                            || countersink_diameter
                                .is_some_and(|value| !value.is_finite() || value <= *diameter)
                            || countersink_angle_deg.is_some_and(|value| {
                                !value.is_finite() || value <= 0.0 || value >= 180.0
                            })
                            || thread_depth.is_some_and(|value| !value.is_finite() || value <= 0.0)
                            || thread.chars().count() > 256
                            || note.chars().count() > MAX_NOTE_LENGTH
                            || feature_name.chars().count() > 256
                            || pattern_note.chars().count() > 512
                        {
                            return Err(format!(
                                "drawing hole note {annotation_id} contains invalid values"
                            ));
                        }
                    }
                    DrawingAnnotationDto::ChamferNote {
                        view_id,
                        first,
                        second,
                        position,
                        length,
                        angle_deg,
                        prefix,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_anchor(first, annotation_id)?;
                        validate_anchor(second, annotation_id)?;
                        if first.body_id != second.body_id
                            || first.edge_id != second.edge_id
                            || first.edge_key != second.edge_key
                            || first.endpoint == second.endpoint
                            || position.iter().any(|value| !value.is_finite())
                            || !length.is_finite()
                            || *length <= 0.0
                            || !angle_deg.is_finite()
                            || *angle_deg <= 0.0
                            || *angle_deg >= 180.0
                            || prefix.chars().count() > 256
                        {
                            return Err(format!(
                                "drawing chamfer note {annotation_id} contains invalid values"
                            ));
                        }
                    }
                    DrawingAnnotationDto::CenterMark {
                        view_id,
                        feature,
                        extension,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_circular_ref(feature, annotation_id)?;
                        if !feature.closed
                            || !extension.is_finite()
                            || *extension < 0.0
                            || *extension > 1.0e6
                        {
                            return Err(format!(
                                "drawing center mark {annotation_id} contains invalid values"
                            ));
                        }
                    }
                    DrawingAnnotationDto::CenterLine {
                        view_id,
                        first,
                        second,
                        extension,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_circular_ref(first, annotation_id)?;
                        validate_circular_ref(second, annotation_id)?;
                        if !first.closed
                            || !second.closed
                            || first == second
                            || !extension.is_finite()
                            || *extension < 0.0
                            || *extension > 1.0e6
                        {
                            return Err(format!(
                                "drawing center line {annotation_id} contains invalid values"
                            ));
                        }
                    }
                    DrawingAnnotationDto::CenterLineBetweenEdges {
                        view_id,
                        first,
                        second,
                        extension,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_line_ref(first, annotation_id)?;
                        validate_line_ref(second, annotation_id)?;
                        if first == second
                            || !extension.is_finite()
                            || *extension < 0.0
                            || *extension > 1.0e6
                        {
                            return Err(format!(
                                "drawing edge center line {annotation_id} contains invalid values"
                            ));
                        }
                    }
                    DrawingAnnotationDto::AutomaticSymmetryAxis {
                        view_id, extension, ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_nonnegative_finite(*extension, annotation_id, "axis extension")?;
                    }
                    DrawingAnnotationDto::BoltCircleCenterLine {
                        view_id,
                        features,
                        extension,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        if features.len() < 3 || features.len() > 512 {
                            return Err(format!(
                                "drawing bolt circle {annotation_id} needs 3 to 512 centers"
                            ));
                        }
                        for feature in features {
                            validate_circular_ref(feature, annotation_id)?;
                        }
                        validate_nonnegative_finite(
                            *extension,
                            annotation_id,
                            "bolt-circle extension",
                        )?;
                    }
                    DrawingAnnotationDto::ChainDimension {
                        view_id,
                        anchors,
                        offset,
                        spacing,
                        precision,
                        presentation,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        if anchors.len() < 2 || anchors.len() > 256 {
                            return Err(format!(
                                "drawing chain dimension {annotation_id} needs 2 to 256 anchors"
                            ));
                        }
                        for anchor in anchors {
                            validate_anchor(anchor, annotation_id)?;
                        }
                        if !offset.is_finite() || !spacing.is_finite() || *spacing < 0.0 {
                            return Err(format!(
                                "drawing chain dimension {annotation_id} has invalid layout"
                            ));
                        }
                        validate_precision(*precision, annotation_id)?;
                        validate_dimension_presentation(presentation, annotation_id)?;
                    }
                    DrawingAnnotationDto::OrdinateDimension {
                        view_id,
                        origin,
                        target,
                        offset,
                        precision,
                        presentation,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_anchor(origin, annotation_id)?;
                        validate_anchor(target, annotation_id)?;
                        if origin == target || !offset.is_finite() {
                            return Err(format!(
                                "drawing ordinate dimension {annotation_id} has invalid geometry"
                            ));
                        }
                        validate_precision(*precision, annotation_id)?;
                        validate_dimension_presentation(presentation, annotation_id)?;
                    }
                    DrawingAnnotationDto::ArcLengthDimension {
                        view_id,
                        feature,
                        first,
                        second,
                        offset,
                        precision,
                        presentation,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_circular_ref(feature, annotation_id)?;
                        validate_anchor(first, annotation_id)?;
                        validate_anchor(second, annotation_id)?;
                        if first == second || !offset.is_finite() || *offset <= 0.0 {
                            return Err(format!(
                                "drawing arc-length dimension {annotation_id} has invalid geometry"
                            ));
                        }
                        validate_precision(*precision, annotation_id)?;
                        validate_dimension_presentation(presentation, annotation_id)?;
                    }
                    DrawingAnnotationDto::JoggedRadiusDimension {
                        view_id,
                        feature,
                        jog,
                        position,
                        precision,
                        presentation,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_circular_ref(feature, annotation_id)?;
                        validate_position(*jog, annotation_id)?;
                        validate_position(*position, annotation_id)?;
                        validate_precision(*precision, annotation_id)?;
                        validate_dimension_presentation(presentation, annotation_id)?;
                    }
                    DrawingAnnotationDto::DatumFeature {
                        view_id,
                        attachment,
                        label,
                        position,
                        target_index,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_attachment(attachment, annotation_id)?;
                        if !valid_datum_label(label)
                            || target_index.is_some_and(|index| index == 0 || index > 999)
                        {
                            return Err(format!(
                                "drawing datum {annotation_id} contains invalid values"
                            ));
                        }
                        validate_position(*position, annotation_id)?;
                    }
                    DrawingAnnotationDto::GdtFrame {
                        view_id,
                        attachment,
                        position,
                        tolerance,
                        datums,
                        projected_zone,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_attachment(attachment, annotation_id)?;
                        validate_position(*position, annotation_id)?;
                        if !tolerance.is_finite()
                            || *tolerance <= 0.0
                            || projected_zone
                                .is_some_and(|value| !value.is_finite() || value <= 0.0)
                            || datums.len() > 3
                            || datums.iter().any(|datum| !valid_datum_label(&datum.label))
                        {
                            return Err(format!(
                                "drawing GD&T frame {annotation_id} contains invalid values"
                            ));
                        }
                    }
                    DrawingAnnotationDto::SurfaceTexture {
                        view_id,
                        attachment,
                        position,
                        roughness_ra,
                        process,
                        machining_allowance,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_attachment(attachment, annotation_id)?;
                        validate_position(*position, annotation_id)?;
                        if !roughness_ra.is_finite()
                            || *roughness_ra <= 0.0
                            || process.chars().count() > 256
                            || machining_allowance
                                .is_some_and(|value| !value.is_finite() || value < 0.0)
                        {
                            return Err(format!(
                                "drawing surface texture {annotation_id} contains invalid values"
                            ));
                        }
                    }
                    DrawingAnnotationDto::EdgeRequirement {
                        view_id,
                        attachment,
                        position,
                        upper_deviation,
                        lower_deviation,
                        note,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_line_ref(attachment, annotation_id)?;
                        validate_position(*position, annotation_id)?;
                        if !upper_deviation.is_finite()
                            || !lower_deviation.is_finite()
                            || note.chars().count() > MAX_NOTE_LENGTH
                        {
                            return Err(format!(
                                "drawing edge requirement {annotation_id} contains invalid values"
                            ));
                        }
                    }
                    DrawingAnnotationDto::WeldSymbol {
                        view_id,
                        attachment,
                        position,
                        size,
                        length,
                        pitch,
                        finish,
                        tail,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_line_ref(attachment, annotation_id)?;
                        validate_position(*position, annotation_id)?;
                        if !size.is_finite()
                            || *size <= 0.0
                            || length.is_some_and(|value| !value.is_finite() || value <= 0.0)
                            || pitch.is_some_and(|value| !value.is_finite() || value <= 0.0)
                            || finish.chars().count() > 64
                            || tail.chars().count() > MAX_NOTE_LENGTH
                        {
                            return Err(format!(
                                "drawing weld symbol {annotation_id} contains invalid values"
                            ));
                        }
                    }
                    DrawingAnnotationDto::ItemBalloon {
                        view_id,
                        attachment,
                        position,
                        bom_item_id,
                        ..
                    } => {
                        validate_annotation_view(&sheet_view_ids, *view_id, annotation_id)?;
                        validate_attachment(attachment, annotation_id)?;
                        validate_position(*position, annotation_id)?;
                        if !sheet_bom_ids.contains(bom_item_id) {
                            return Err(format!("drawing balloon {annotation_id} references missing BOM item {bom_item_id}"));
                        }
                    }
                    DrawingAnnotationDto::RevisionCloud {
                        revision, points, ..
                    } => {
                        if revision.trim().is_empty()
                            || points.len() < 3
                            || points.len() > 4096
                            || points.iter().flatten().any(|value| !value.is_finite())
                        {
                            return Err(format!(
                                "drawing revision cloud {annotation_id} contains invalid values"
                            ));
                        }
                    }
                }
            }
        }

        match (self.sheets.is_empty(), self.active_sheet_id) {
            (true, Some(_)) => {
                return Err("an empty drawing cannot have an active sheet".to_string())
            }
            (false, None) => return Err("a drawing with sheets needs an active sheet".to_string()),
            (_, Some(id)) if !sheet_ids.contains(&id) => {
                return Err(format!("active drawing sheet {id} does not exist"));
            }
            _ => {}
        }
        if self.next_sheet_id <= max_sheet_id
            || self.next_view_id <= max_view_id
            || self.next_annotation_id <= max_annotation_id
            || self.next_revision_id <= max_revision_id
            || self.next_bom_item_id <= max_bom_item_id
            || self.next_template_id <= max_template_id
        {
            return Err("drawing id counters must be greater than existing ids".to_string());
        }
        Ok(())
    }
}

fn validate_anchor(anchor: &DrawingTopologyAnchorRefDto, annotation_id: u64) -> Result<(), String> {
    if anchor.occurrence_id.is_some_and(|id| id.0 == 0)
        || anchor.body_id.0 == 0
        || anchor.edge_id.0 == 0
        || anchor.edge_key.trim().is_empty()
    {
        return Err(format!(
            "drawing dimension {annotation_id} contains an invalid topology anchor"
        ));
    }
    if anchor.fallback_point.iter().any(|value| !value.is_finite()) {
        return Err(format!(
            "drawing dimension {annotation_id} contains a non-finite fallback point"
        ));
    }
    Ok(())
}

fn validate_annotation_view(
    sheet_view_ids: &HashSet<u64>,
    view_id: u64,
    annotation_id: u64,
) -> Result<(), String> {
    if !sheet_view_ids.contains(&view_id) {
        return Err(format!(
            "drawing annotation {annotation_id} references missing view {view_id}"
        ));
    }
    Ok(())
}

fn validate_circular_ref(
    feature: &DrawingCircularRefDto,
    annotation_id: u64,
) -> Result<(), String> {
    if feature.occurrence_id.is_some_and(|id| id.0 == 0)
        || feature.body_id.0 == 0
        || feature.edge_id.0 == 0
        || feature.edge_key.trim().is_empty()
        || feature
            .fallback_center
            .iter()
            .any(|value| !value.is_finite())
        || feature
            .fallback_normal
            .iter()
            .any(|value| !value.is_finite())
        || !feature.fallback_radius.is_finite()
        || feature.fallback_radius <= 0.0
    {
        return Err(format!(
            "drawing annotation {annotation_id} contains an invalid circular reference"
        ));
    }
    Ok(())
}

fn validate_line_ref(feature: &DrawingLineRefDto, annotation_id: u64) -> Result<(), String> {
    let delta = [
        feature.fallback_end[0] - feature.fallback_start[0],
        feature.fallback_end[1] - feature.fallback_start[1],
        feature.fallback_end[2] - feature.fallback_start[2],
    ];
    if feature.occurrence_id.is_some_and(|id| id.0 == 0)
        || feature.body_id.0 == 0
        || feature.edge_id.0 == 0
        || feature.edge_key.trim().is_empty()
        || feature
            .fallback_start
            .iter()
            .chain(feature.fallback_end.iter())
            .any(|value| !value.is_finite())
        || squared_length(delta) <= 1.0e-14
    {
        return Err(format!(
            "drawing annotation {annotation_id} contains an invalid straight-edge reference"
        ));
    }
    Ok(())
}

fn validate_precision(precision: u8, annotation_id: u64) -> Result<(), String> {
    if precision > 6 {
        return Err(format!(
            "drawing annotation {annotation_id} precision exceeds 6 decimals"
        ));
    }
    Ok(())
}

fn validate_dimension_presentation(
    presentation: &DrawingDimensionPresentationDto,
    annotation_id: u64,
) -> Result<(), String> {
    if !presentation.tolerance.upper.is_finite()
        || !presentation.tolerance.lower.is_finite()
        || presentation.fit_class.chars().count() > 64
        || presentation
            .dual_units
            .as_ref()
            .is_some_and(|dual| dual.precision > 6)
    {
        return Err(format!(
            "drawing dimension {annotation_id} contains invalid tolerance or dual-unit metadata"
        ));
    }
    Ok(())
}

fn validate_attachment(
    attachment: &DrawingAttachmentRefDto,
    annotation_id: u64,
) -> Result<(), String> {
    match attachment {
        DrawingAttachmentRefDto::Anchor { reference } => validate_anchor(reference, annotation_id),
        DrawingAttachmentRefDto::Line { reference } => validate_line_ref(reference, annotation_id),
        DrawingAttachmentRefDto::Circle { reference } => {
            validate_circular_ref(reference, annotation_id)
        }
    }
}

fn validate_position(position: [f64; 2], annotation_id: u64) -> Result<(), String> {
    if position.iter().any(|value| !value.is_finite()) {
        return Err(format!(
            "drawing annotation {annotation_id} contains a non-finite position"
        ));
    }
    Ok(())
}

fn validate_nonnegative_finite(value: f64, annotation_id: u64, label: &str) -> Result<(), String> {
    if !value.is_finite() || value < 0.0 || value > 1.0e6 {
        return Err(format!(
            "drawing annotation {annotation_id} has an invalid {label}"
        ));
    }
    Ok(())
}

fn valid_datum_label(label: &str) -> bool {
    !label.trim().is_empty()
        && label.chars().count() <= 8
        && label
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

fn validate_title_block(title: &DrawingTitleBlockDto, owner_name: &str) -> Result<(), String> {
    let values = [
        &title.title,
        &title.drawing_number,
        &title.revision,
        &title.author,
        &title.checked_by,
        &title.approved_by,
        &title.company,
        &title.material,
        &title.finish,
    ];
    if values
        .iter()
        .any(|value| value.chars().count() > MAX_NOTE_LENGTH)
    {
        return Err(format!(
            "drawing '{owner_name}' contains oversized title-block text"
        ));
    }
    Ok(())
}

fn validate_sheet_style(style: &DrawingSheetStyleDto, sheet_name: &str) -> Result<(), String> {
    let line_styles = [
        &style.visible,
        &style.hidden,
        &style.center,
        &style.cutting_plane,
        &style.phantom,
        &style.break_line,
        &style.dimension,
        &style.extension,
        &style.leader,
        &style.hatch,
    ];
    if style.name.trim().is_empty()
        || style.font_family.trim().is_empty()
        || !style.text_height_mm.is_finite()
        || style.text_height_mm <= 0.0
        || !style.small_text_height_mm.is_finite()
        || style.small_text_height_mm <= 0.0
        || !style.arrow_size_mm.is_finite()
        || style.arrow_size_mm <= 0.0
        || !style.hatch_angle_deg.is_finite()
        || !style.hatch_spacing_mm.is_finite()
        || style.hatch_spacing_mm <= 0.0
        || line_styles.iter().any(|line| {
            !line.width_mm.is_finite()
                || line.width_mm <= 0.0
                || line.width_mm > 5.0
                || line.dash_mm.len() > 16
                || line
                    .dash_mm
                    .iter()
                    .any(|value| !value.is_finite() || *value <= 0.0)
        })
    {
        return Err(format!(
            "drawing sheet '{sheet_name}' has an invalid style template"
        ));
    }
    Ok(())
}

fn validate_view_derivation(
    view: &DrawingViewDto,
    sheet_view_ids: &HashSet<u64>,
) -> Result<(), String> {
    let Some(derivation) = &view.derivation else {
        if matches!(
            view.kind,
            DrawingViewKind::Section
                | DrawingViewKind::Detail
                | DrawingViewKind::Auxiliary
                | DrawingViewKind::Broken
                | DrawingViewKind::RemovedSection
        ) {
            return Err(format!(
                "derived drawing view '{}' is missing its derivation",
                view.name
            ));
        }
        return Ok(());
    };
    let parent_id = match derivation {
        DrawingViewDerivationDto::Section {
            parent_view_id,
            first,
            second,
            label,
            depth,
            hatch_angle_deg,
            hatch_spacing_mm,
        } => {
            validate_anchor(first, view.id)?;
            validate_anchor(second, view.id)?;
            if first == second
                || label.trim().is_empty()
                || depth.is_some_and(|value| !value.is_finite() || value <= 0.0)
                || !hatch_angle_deg.is_finite()
                || !hatch_spacing_mm.is_finite()
                || *hatch_spacing_mm <= 0.0
            {
                return Err(format!(
                    "section view '{}' has invalid derivation data",
                    view.name
                ));
            }
            *parent_view_id
        }
        DrawingViewDerivationDto::Detail {
            parent_view_id,
            center,
            radius,
            label,
        } => {
            validate_anchor(center, view.id)?;
            if !radius.is_finite() || *radius <= 0.0 || label.trim().is_empty() {
                return Err(format!(
                    "detail view '{}' has invalid derivation data",
                    view.name
                ));
            }
            *parent_view_id
        }
        DrawingViewDerivationDto::Auxiliary {
            parent_view_id,
            reference,
            label,
            ..
        } => {
            validate_line_ref(reference, view.id)?;
            if label.trim().is_empty() {
                return Err(format!("auxiliary view '{}' has an empty label", view.name));
            }
            *parent_view_id
        }
        DrawingViewDerivationDto::Broken {
            parent_view_id,
            first,
            second,
            gap_mm,
            ..
        } => {
            if !first.is_finite()
                || !second.is_finite()
                || first >= second
                || !gap_mm.is_finite()
                || *gap_mm <= 0.0
            {
                return Err(format!(
                    "broken view '{}' has invalid break positions",
                    view.name
                ));
            }
            *parent_view_id
        }
        DrawingViewDerivationDto::RemovedSection {
            parent_view_id,
            first,
            second,
            label,
            hatch_angle_deg,
            hatch_spacing_mm,
        } => {
            validate_anchor(first, view.id)?;
            validate_anchor(second, view.id)?;
            if first == second
                || label.trim().is_empty()
                || !hatch_angle_deg.is_finite()
                || !hatch_spacing_mm.is_finite()
                || *hatch_spacing_mm <= 0.0
            {
                return Err(format!(
                    "removed section view '{}' has invalid derivation data",
                    view.name
                ));
            }
            *parent_view_id
        }
    };
    if parent_id == view.id || !sheet_view_ids.contains(&parent_id) {
        return Err(format!(
            "derived drawing view '{}' references missing parent {parent_id}",
            view.name
        ));
    }
    Ok(())
}

fn squared_length(vector: [f64; 3]) -> f64 {
    vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_drawing_is_valid() {
        assert!(DrawingDocumentDto::default().validate().is_ok());
    }

    #[test]
    fn rejects_parallel_view_basis() {
        let drawing = DrawingDocumentDto {
            sheets: vec![DrawingSheetDto {
                id: 1,
                name: "Sheet 1".to_string(),
                format: DrawingSheetFormat::A4,
                orientation: DrawingSheetOrientation::Landscape,
                standard: DrawingStandard::Iso,
                projection_method: DrawingProjectionMethod::FirstAngle,
                tolerance_note: DrawingToleranceNoteDto::default(),
                title_block: DrawingTitleBlockDto::default(),
                views: vec![DrawingViewDto {
                    scope: Default::default(),
                    occurrence_ids: vec![],
                    id: 1,
                    name: "Front".to_string(),
                    kind: DrawingViewKind::Front,
                    direction: [0.0, 1.0, 0.0],
                    up: [0.0, 2.0, 0.0],
                    position: [100.0, 80.0],
                    scale: 1.0,
                    body_ids: vec![],
                    show_hidden_lines: false,
                    show_tangent_edges: false,
                    parent_view_id: None,
                    alignment: DrawingViewAlignment::Free,
                    derivation: None,
                }],
                annotations: vec![],
                style: DrawingSheetStyleDto::default(),
                template_name: String::new(),
                revisions: vec![],
                bom: vec![],
                release: DrawingReleaseDto::default(),
                revision_table_position: None,
                bom_table_position: None,
            }],
            active_sheet_id: Some(1),
            next_sheet_id: 2,
            next_view_id: 2,
            next_annotation_id: 1,
            next_revision_id: 1,
            next_bom_item_id: 1,
            templates: vec![],
            next_template_id: 1,
        };
        assert!(drawing.validate().is_err());
    }

    #[test]
    fn older_drawing_files_receive_standards_and_alignment_defaults() {
        let json = r#"{
          "sheets": [{
            "id": 1,
            "name": "Legacy sheet",
            "format": "a4",
            "orientation": "landscape",
            "title_block": {},
            "views": [{
              "id": 1,
              "name": "Front",
              "kind": "front",
              "direction": [0.0, -1.0, 0.0],
              "up": [0.0, 0.0, 1.0],
              "position": [100.0, 80.0],
              "scale": 1.0
            }],
            "annotations": []
          }],
          "active_sheet_id": 1,
          "next_sheet_id": 2,
          "next_view_id": 2,
          "next_annotation_id": 1
        }"#;
        let drawing: DrawingDocumentDto = serde_json::from_str(json).unwrap();
        let sheet = &drawing.sheets[0];
        assert_eq!(sheet.standard, DrawingStandard::Iso);
        assert_eq!(sheet.projection_method, DrawingProjectionMethod::FirstAngle);
        assert_eq!(sheet.tolerance_note, DrawingToleranceNoteDto::default());
        assert_eq!(sheet.views[0].parent_view_id, None);
        assert_eq!(sheet.views[0].alignment, DrawingViewAlignment::Free);
        assert!(drawing.templates.is_empty());
        assert_eq!(drawing.next_revision_id, 1);
        assert_eq!(drawing.next_bom_item_id, 1);
        assert_eq!(drawing.next_template_id, 1);
        assert!(drawing.validate().is_ok());
    }

    #[test]
    fn company_template_roundtrips_as_a_self_contained_style_snapshot() {
        let mut style = DrawingSheetStyleDto::default();
        style.name = "Acme ISO".to_string();
        style.font_family = "Arial".to_string();
        style.visible.width_mm = 0.7;
        style.hidden.dash_mm = vec![6.0, 2.0];
        style.hatch_angle_deg = 30.0;
        let template = DrawingTemplateDto {
            id: 7,
            name: "Acme A-series".to_string(),
            standard: DrawingStandard::Iso,
            projection_method: DrawingProjectionMethod::FirstAngle,
            tolerance_note: DrawingToleranceNoteDto {
                preset: DrawingTolerancePreset::Iso2768Fine,
                custom: "GENERAL TOLERANCES ISO 2768-f".to_string(),
            },
            title_defaults: DrawingTitleBlockDto {
                company: "Acme Manufacturing".to_string(),
                checked_by: "Quality".to_string(),
                ..DrawingTitleBlockDto::default()
            },
            style,
        };
        let drawing = DrawingDocumentDto {
            templates: vec![template],
            next_template_id: 8,
            ..DrawingDocumentDto::default()
        };
        assert!(drawing.validate().is_ok());

        let serialized = serde_json::to_string(&drawing).unwrap();
        let restored: DrawingDocumentDto = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored, drawing);
        assert_eq!(restored.templates[0].style.visible.width_mm, 0.7);
        assert_eq!(
            restored.templates[0].title_defaults.company,
            "Acme Manufacturing"
        );
    }

    #[test]
    fn duplicate_company_template_ids_are_rejected() {
        let template = DrawingTemplateDto {
            id: 2,
            name: "Shop default".to_string(),
            standard: DrawingStandard::Ansi,
            projection_method: DrawingProjectionMethod::ThirdAngle,
            tolerance_note: DrawingToleranceNoteDto::default(),
            title_defaults: DrawingTitleBlockDto::default(),
            style: DrawingSheetStyleDto::default(),
        };
        let drawing = DrawingDocumentDto {
            templates: vec![template.clone(), template],
            next_template_id: 3,
            ..DrawingDocumentDto::default()
        };
        let error = drawing.validate().unwrap_err();
        assert!(error.contains("duplicate or zero drawing template id"));
    }
}
