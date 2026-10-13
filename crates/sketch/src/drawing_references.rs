//! Borrowed topology references for drawing command validation.
use crate::drawing::*;
use limo_cad_assembly::OccurrenceId;
use limo_cad_core::{BodyId, EdgeId};

pub(crate) enum DrawingReference<'a> {
    Anchor(&'a DrawingTopologyAnchorRefDto),
    Line(&'a DrawingLineRefDto),
    Circle(&'a DrawingCircularRefDto),
}
impl DrawingReference<'_> {
    pub(crate) fn identity(
        &self,
    ) -> (
        BodyId,
        EdgeId,
        &str,
        Option<OccurrenceId>,
        Option<&str>,
        bool,
    ) {
        match self {
            Self::Anchor(r) => (
                r.body_id,
                r.edge_id,
                &r.edge_key,
                r.occurrence_id,
                r.topology_signature.as_deref(),
                r.circle_center,
            ),
            Self::Line(r) => (
                r.body_id,
                r.edge_id,
                &r.edge_key,
                r.occurrence_id,
                r.topology_signature.as_deref(),
                false,
            ),
            Self::Circle(r) => (
                r.body_id,
                r.edge_id,
                &r.edge_key,
                r.occurrence_id,
                r.topology_signature.as_deref(),
                true,
            ),
        }
    }
}
impl DrawingAnnotationDto {
    pub(crate) fn visit_references<E>(
        &self,
        visit: &mut impl FnMut(DrawingReference<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        use DrawingReference::{Anchor, Circle, Line};
        match self {
            Self::Note { .. } | Self::RevisionCloud { .. } | Self::AutomaticSymmetryAxis { .. } => {
            }
            Self::LinearDimension { first, second, .. }
            | Self::ChamferNote { first, second, .. } => {
                visit(Anchor(first))?;
                visit(Anchor(second))?;
            }
            Self::AngularDimension {
                vertex,
                first,
                second,
                ..
            } => {
                visit(Anchor(vertex))?;
                visit(Anchor(first))?;
                visit(Anchor(second))?;
            }
            Self::LineDimension { first, second, .. } => {
                visit(Line(first))?;
                if let Some(second) = second {
                    visit(Line(second))?;
                }
            }
            Self::PointLineDimension { point, line, .. } => {
                visit(Anchor(point))?;
                visit(Line(line))?;
            }
            Self::RadialDimension { feature, .. }
            | Self::HoleNote { feature, .. }
            | Self::CenterMark { feature, .. }
            | Self::JoggedRadiusDimension { feature, .. } => visit(Circle(feature))?,
            Self::CenterLine { first, second, .. } => {
                visit(Circle(first))?;
                visit(Circle(second))?;
            }
            Self::CenterLineBetweenEdges { first, second, .. } => {
                visit(Line(first))?;
                visit(Line(second))?;
            }
            Self::BoltCircleCenterLine { features, .. } => {
                for feature in features {
                    visit(Circle(feature))?;
                }
            }
            Self::ChainDimension { anchors, .. } => {
                for anchor in anchors {
                    visit(Anchor(anchor))?;
                }
            }
            Self::OrdinateDimension { origin, target, .. } => {
                visit(Anchor(origin))?;
                visit(Anchor(target))?;
            }
            Self::ArcLengthDimension {
                feature,
                first,
                second,
                ..
            } => {
                visit(Circle(feature))?;
                visit(Anchor(first))?;
                visit(Anchor(second))?;
            }
            Self::DatumFeature { attachment, .. }
            | Self::GdtFrame { attachment, .. }
            | Self::SurfaceTexture { attachment, .. }
            | Self::ItemBalloon { attachment, .. } => match attachment {
                DrawingAttachmentRefDto::Anchor { reference } => visit(Anchor(reference))?,
                DrawingAttachmentRefDto::Line { reference } => visit(Line(reference))?,
                DrawingAttachmentRefDto::Circle { reference } => visit(Circle(reference))?,
            },
            Self::EdgeRequirement { attachment, .. } | Self::WeldSymbol { attachment, .. } => {
                visit(Line(attachment))?
            }
        }
        Ok(())
    }
}

impl DrawingViewDto {
    pub(crate) fn visit_references<E>(
        &self,
        visit: &mut impl FnMut(DrawingReference<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        use DrawingReference::{Anchor, Line};
        match &self.derivation {
            Some(
                DrawingViewDerivationDto::Section { first, second, .. }
                | DrawingViewDerivationDto::RemovedSection { first, second, .. },
            ) => {
                visit(Anchor(first))?;
                visit(Anchor(second))?;
            }
            Some(DrawingViewDerivationDto::Detail { center, .. }) => visit(Anchor(center))?,
            Some(DrawingViewDerivationDto::Auxiliary { reference, .. }) => visit(Line(reference))?,
            Some(DrawingViewDerivationDto::Broken { .. }) | None => {}
        }
        Ok(())
    }
}
