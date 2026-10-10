//! Remaining annotation tools use the shared drawing records and transaction.
//! Picks are transient and cannot survive a document, revision or view change.
use super::{anchors, radial, runtime::Target, straight, Stamp};
use limo_cad_sketch::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    Repair,
    CenterEdges,
    Symmetry,
    BoltCircle,
    ArcLength,
    JoggedRadius,
    Datum,
    Gdt,
    Surface,
    Edge,
    Weld,
    Balloon,
}
impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Repair => "Reassociate references",
            Self::CenterEdges => "Centerline between edges",
            Self::Symmetry => "Symmetry axis",
            Self::BoltCircle => "Bolt circle",
            Self::ArcLength => "Arc length dimension",
            Self::JoggedRadius => "Jogged radius",
            Self::Datum => "Datum feature",
            Self::Gdt => "GD&T frame",
            Self::Surface => "Surface texture",
            Self::Edge => "Edge requirement",
            Self::Weld => "Weld symbol",
            Self::Balloon => "Item balloon",
        }
    }
    pub fn anchors(self) -> bool {
        matches!(
            self,
            Self::Repair
                | Self::ArcLength
                | Self::Datum
                | Self::Gdt
                | Self::Surface
                | Self::Balloon
                | Self::Symmetry
        )
    }
    pub fn circles(self) -> bool {
        !matches!(self, Self::CenterEdges | Self::Edge | Self::Weld)
    }
    pub fn lines(self) -> bool {
        !matches!(
            self,
            Self::ArcLength | Self::JoggedRadius | Self::BoltCircle
        )
    }
    pub(super) fn instruction(self, p: &Placement) -> &'static str {
        match self {
            Self::Repair => "Choose a saved annotation or derived view and the reference to replace. Pick its replacement on the owning view, then Apply repair.",
            Self::CenterEdges if p.line.is_some() => {
                "Choose a second distinct parallel straight edge in the same view."
            }
            Self::CenterEdges => "Choose two parallel straight edges in one view.",
            Self::Symmetry => {
                "Choose projected geometry in the view that needs a symmetry axis. Edit X/Y axes and extension afterward."
            }
            Self::BoltCircle => "Choose three distinct complete circular edges in one view.",
            Self::ArcLength if p.circles.is_empty() => {
                "Choose the circular edge or arc to measure."
            }
            Self::ArcLength if p.anchor.is_none() => {
                "Choose the first arc endpoint in the same view."
            }
            Self::ArcLength => "Choose the second distinct arc endpoint in the same view.",
            Self::JoggedRadius => {
                "Choose a circular edge or arc. Drag the saved label or edit its jog and position."
            }
            Self::Edge | Self::Weld => {
                "Choose a straight edge, then edit the symbol and drag its saved label."
            }
            Self::Balloon => {
                "Choose an endpoint, straight edge or circular edge. Its body's BOM item is reused or added."
            }
            _ => {
                "Choose an endpoint, straight edge or circular edge, then edit the symbol and drag its saved label."
            }
        }
    }
}
#[derive(Default)]
pub(super) struct Placement {
    owner: Option<(Stamp, u64)>,
    pub line: Option<straight::LineTarget>,
    pub circles: Vec<radial::Target>,
    pub anchor: Option<Target>,
}
impl Placement {
    pub fn cancel(&mut self) {
        *self = Self::default();
    }
    fn observe(&mut self, stamp: &Stamp, view: u64) {
        if self
            .owner
            .as_ref()
            .is_none_or(|(s, v)| s != stamp || *v != view)
        {
            self.cancel();
            self.owner = Some((stamp.clone(), view));
        }
    }
    pub fn anchor(
        &mut self,
        tool: Tool,
        stamp: &Stamp,
        t: &Target,
        document: &DrawingDocumentDto,
        size: [f64; 2],
    ) -> Result<Option<DrawingDocumentDto>, String> {
        self.observe(stamp, t.view_id);
        let id = document.next_annotation_id;
        let a = if tool == Tool::ArcLength {
            let feature = self
                .circles
                .first()
                .ok_or("Choose the circular edge first")?
                .reference
                .clone();
            let Some(first) = &self.anchor else {
                self.anchor = Some(t.clone());
                return Ok(None);
            };
            if anchors::same_anchor(&first.reference, &t.reference) {
                return Ok(None);
            }
            DrawingAnnotationDto::ArcLengthDimension {
                id,
                view_id: t.view_id,
                feature,
                first: first.reference.clone(),
                second: t.reference.clone(),
                offset: 7.,
                precision: 2,
                presentation: Default::default(),
            }
        } else {
            return self.attachment(
                tool,
                stamp,
                (
                    t.view_id,
                    DrawingAttachmentRefDto::Anchor {
                        reference: t.reference.clone(),
                    },
                ),
                t.paper,
                document,
                size,
            );
        };
        self.finish(document, stamp, a, None)
    }
    pub fn circle(
        &mut self,
        tool: Tool,
        stamp: &Stamp,
        t: &radial::Target,
        document: &DrawingDocumentDto,
        size: [f64; 2],
    ) -> Result<Option<DrawingDocumentDto>, String> {
        self.observe(stamp, t.view_id);
        let id = document.next_annotation_id;
        let a = match tool {
            Tool::ArcLength => {
                self.circles = vec![t.clone()];
                self.anchor = None;
                return Ok(None);
            }
            Tool::BoltCircle => {
                if !t.reference.closed {
                    return Err("Choose complete circular edges".into());
                }
                if self
                    .circles
                    .iter()
                    .any(|c| same_circle(&c.reference, &t.reference))
                {
                    return Ok(None);
                }
                self.circles.push(t.clone());
                if self.circles.len() < 3 {
                    return Ok(None);
                }
                DrawingAnnotationDto::BoltCircleCenterLine {
                    id,
                    view_id: t.view_id,
                    features: self.circles.iter().map(|c| c.reference.clone()).collect(),
                    extension: 2.5,
                }
            }
            Tool::JoggedRadius => {
                let position = bounded([t.center[0] + 26., t.center[1] - 18.], size);
                DrawingAnnotationDto::JoggedRadiusDimension {
                    id,
                    view_id: t.view_id,
                    feature: t.reference.clone(),
                    jog: [position[0] - 10., position[1]],
                    position,
                    precision: 2,
                    presentation: Default::default(),
                }
            }
            _ => {
                return self.attachment(
                    tool,
                    stamp,
                    (
                        t.view_id,
                        DrawingAttachmentRefDto::Circle {
                            reference: t.reference.clone(),
                        },
                    ),
                    t.center,
                    document,
                    size,
                );
            }
        };
        self.finish(document, stamp, a, None)
    }
    pub fn line(
        &mut self,
        tool: Tool,
        stamp: &Stamp,
        t: &straight::LineTarget,
        document: &DrawingDocumentDto,
        size: [f64; 2],
    ) -> Result<Option<DrawingDocumentDto>, String> {
        self.observe(stamp, t.view_id);
        let id = document.next_annotation_id;
        let position = bounded([t.paper[1][0] + 18., t.paper[1][1] - 12.], size);
        let a = match tool {
            Tool::CenterEdges => {
                let Some(first) = &self.line else {
                    self.line = Some(t.clone());
                    return Ok(None);
                };
                if straight::same_line(&first.reference, &t.reference) {
                    return Ok(None);
                }
                if straight::mode(first, Some(t)) != DrawingLineDimensionMode::Distance {
                    return Err("Centerline requires parallel straight edges".into());
                }
                DrawingAnnotationDto::CenterLineBetweenEdges {
                    id,
                    view_id: t.view_id,
                    first: first.reference.clone(),
                    second: t.reference.clone(),
                    extension: 2.5,
                }
            }
            Tool::Edge => DrawingAnnotationDto::EdgeRequirement {
                id,
                view_id: t.view_id,
                attachment: t.reference.clone(),
                position,
                upper_deviation: 0.,
                lower_deviation: -0.2,
                note: String::new(),
            },
            Tool::Weld => DrawingAnnotationDto::WeldSymbol {
                id,
                view_id: t.view_id,
                attachment: t.reference.clone(),
                position,
                weld_type: DrawingWeldType::Fillet,
                side: DrawingWeldSide::Arrow,
                size: 3.,
                length: None,
                pitch: None,
                contour: DrawingWeldContour::None,
                finish: String::new(),
                all_around: false,
                field_weld: false,
                tail: String::new(),
            },
            _ => {
                return self.attachment(
                    tool,
                    stamp,
                    (
                        t.view_id,
                        DrawingAttachmentRefDto::Line {
                            reference: t.reference.clone(),
                        },
                    ),
                    [
                        (t.paper[0][0] + t.paper[1][0]) * 0.5,
                        (t.paper[0][1] + t.paper[1][1]) * 0.5,
                    ],
                    document,
                    size,
                );
            }
        };
        self.finish(document, stamp, a, None)
    }
    fn attachment(
        &mut self,
        tool: Tool,
        stamp: &Stamp,
        (view_id, attachment): (u64, DrawingAttachmentRefDto),
        paper: [f64; 2],
        document: &DrawingDocumentDto,
        size: [f64; 2],
    ) -> Result<Option<DrawingDocumentDto>, String> {
        let id = document.next_annotation_id;
        let position = bounded([paper[0] + 18., paper[1] - 12.], size);
        let mut bom = None;
        let a = match tool {
            Tool::Symmetry => DrawingAnnotationDto::AutomaticSymmetryAxis {
                id,
                view_id,
                axis: DrawingOrdinateAxis::Both,
                extension: 2.5,
            },
            Tool::Datum => DrawingAnnotationDto::DatumFeature {
                id,
                view_id,
                attachment,
                position,
                label: "A".into(),
                target_index: None,
            },
            Tool::Gdt => DrawingAnnotationDto::GdtFrame {
                id,
                view_id,
                attachment,
                position,
                characteristic: DrawingGdtCharacteristic::Position,
                tolerance: 0.1,
                diameter_zone: true,
                material_condition: DrawingMaterialCondition::None,
                datums: vec![],
                projected_zone: None,
                free_state: false,
            },
            Tool::Surface => DrawingAnnotationDto::SurfaceTexture {
                id,
                view_id,
                attachment,
                position,
                roughness_ra: 3.2,
                process: String::new(),
                lay: DrawingSurfaceLay::None,
                machining_allowance: None,
            },
            Tool::Balloon => {
                let body = match &attachment {
                    DrawingAttachmentRefDto::Anchor { reference } => reference.body_id,
                    DrawingAttachmentRefDto::Line { reference } => reference.body_id,
                    DrawingAttachmentRefDto::Circle { reference } => reference.body_id,
                };
                let sheet = document
                    .sheets
                    .iter()
                    .find(|s| s.id == stamp.sheet_id)
                    .ok_or("Sheet was removed")?;
                let bom_item_id =
                    if let Some(item) = sheet.bom.iter().find(|b| b.body_id == Some(body)) {
                        item.id
                    } else {
                        let item_id = document.next_bom_item_id;
                        bom = Some(DrawingBomItemDto {
                            id: item_id,
                            item_number: (sheet.bom.len() + 1).to_string(),
                            body_id: Some(body),
                            part_number: String::new(),
                            description: format!("Body {}", body.0),
                            quantity: 1.,
                            material: String::new(),
                            finish: String::new(),
                        });
                        item_id
                    };
                DrawingAnnotationDto::ItemBalloon {
                    id,
                    view_id,
                    attachment,
                    position,
                    bom_item_id,
                }
            }
            _ => return Err("Choose geometry for the active annotation tool".into()),
        };
        self.finish(document, stamp, a, bom)
    }
    fn finish(
        &mut self,
        document: &DrawingDocumentDto,
        stamp: &Stamp,
        annotation: DrawingAnnotationDto,
        bom: Option<DrawingBomItemDto>,
    ) -> Result<Option<DrawingDocumentDto>, String> {
        let mut next = document.clone();
        next.next_annotation_id = next
            .next_annotation_id
            .checked_add(1)
            .ok_or("Annotation IDs are exhausted")?;
        if bom.is_some() {
            next.next_bom_item_id = next
                .next_bom_item_id
                .checked_add(1)
                .ok_or("BOM IDs are exhausted")?;
        }
        let sheet = next
            .sheets
            .iter_mut()
            .find(|s| s.id == stamp.sheet_id)
            .ok_or("Sheet was removed")?;
        if let Some(item) = bom {
            sheet.bom.push(item);
        }
        sheet.annotations.push(annotation);
        if sheet.release.status == DrawingReleaseStatus::Released {
            sheet.release.status = DrawingReleaseStatus::Draft;
        }
        next.validate()?;
        self.cancel();
        Ok(Some(next))
    }
}
fn same_circle(a: &DrawingCircularRefDto, b: &DrawingCircularRefDto) -> bool {
    a.occurrence_id == b.occurrence_id
        && a.body_id == b.body_id
        && a.edge_id == b.edge_id
        && a.edge_key == b.edge_key
}
fn bounded(point: [f64; 2], size: [f64; 2]) -> [f64; 2] {
    std::array::from_fn(|i| point[i].clamp(5., size[i] - 5.))
}
