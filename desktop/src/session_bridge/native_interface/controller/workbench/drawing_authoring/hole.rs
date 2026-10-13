//! Hole callouts use the saved circular association and existing modeled-hole
//! definitions. Selection never introduces another hole or annotation model.
use super::{radial, Stamp};
use limo_cad_sketch::*;
use limo_cad_solid::{HoleDefinitionDto, HoleExtent, HoleStyle, HoleThreadHand};

mod submit;
pub(super) use submit::submit;

fn best_definition<'a>(
    definitions: &'a [HoleDefinitionDto],
    feature: &DrawingCircularRefDto,
) -> Result<Option<&'a HoleDefinitionDto>, String> {
    if definitions.len() > 16_384
        || definitions
            .iter()
            .map(|d| d.positions.len().max(1))
            .sum::<usize>()
            > 65_536
    {
        return Err("Too many modeled hole positions to create a callout".into());
    }
    if feature.occurrence_id.is_some() {
        return Ok(None);
    }
    let normal_length = feature
        .fallback_normal
        .iter()
        .map(|v| v * v)
        .sum::<f64>()
        .sqrt();
    if !normal_length.is_finite() || normal_length < 1e-9 {
        return Ok(None);
    }
    let tolerance = (feature.fallback_radius * 1e-6).max(1e-5);
    let mut matched = None;
    for d in definitions {
        if d.body_id != feature.body_id {
            continue;
        }
        let radius_error = (d.diameter * 0.5 - feature.fallback_radius).abs();
        if !radius_error.is_finite() || radius_error > tolerance {
            continue;
        }
        let associative = if d.positions.is_empty() {
            d.position_reference.is_some()
        } else {
            d.positions.iter().any(|p| p.position_reference.is_some())
        };
        if associative {
            return Ok(None);
        }
        let Some(basis) = &d.face_basis else {
            return Ok(None);
        };
        let normal_sq = basis.normal.iter().map(|v| v * v).sum::<f64>();
        if !normal_sq.is_finite() || (normal_sq - 1.).abs() > 1e-6 {
            return Ok(None);
        }
        let alignment = feature
            .fallback_normal
            .iter()
            .zip(basis.normal)
            .map(|(a, b)| a / normal_length * b)
            .sum::<f64>()
            .abs();
        if !alignment.is_finite() || alignment < 1. - 1e-6 {
            continue;
        }
        let distance = |p: &limo_cad_solid::Point2Dto| {
            let center = basis.to_3d([p.x, p.y]);
            center
                .iter()
                .zip(feature.fallback_center)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt()
        };
        let center_error = if d.positions.is_empty() {
            distance(&d.position)
        } else {
            d.positions
                .iter()
                .map(|p| distance(&p.position))
                .fold(f64::INFINITY, f64::min)
        };
        if !center_error.is_finite() || center_error > tolerance {
            continue;
        }
        if matched.is_some() {
            return Ok(None);
        }
        matched = Some(d);
    }
    Ok(matched)
}

pub(super) fn create(
    document: &DrawingDocumentDto,
    stamp: &Stamp,
    target: &radial::Target,
    definitions: &[HoleDefinitionDto],
) -> Result<DrawingDocumentDto, String> {
    let feature = &target.reference;
    if !feature.closed
        || !feature.fallback_radius.is_finite()
        || feature.fallback_radius <= 0.
        || target.center.iter().any(|v| !v.is_finite())
    {
        return Err("Hole notes require a complete circular edge".into());
    }
    let definition = best_definition(definitions, feature)?;
    let quantity = definition.map_or(1, |d| d.positions.len().max(1)) as u32;
    let style = definition.map_or(HoleStyle::Simple, |d| d.style);
    let thread = definition.and_then(|d| d.thread.as_ref());
    let annotation = DrawingAnnotationDto::HoleNote {
        id: document.next_annotation_id,
        view_id: target.view_id,
        feature: feature.clone(),
        position: [target.center[0] + 20., target.center[1] + 15.],
        quantity,
        diameter: definition.map_or(feature.fallback_radius * 2., |d| d.diameter),
        depth: definition.and_then(|d| match d.extent {
            HoleExtent::Distance { depth } => Some(depth),
            HoleExtent::ThroughAll => None,
        }),
        through_all: Some(definition.is_some_and(|d| matches!(d.extent, HoleExtent::ThroughAll))),
        thread: thread.map_or_else(String::new, |t| {
            format!(
                "{}{}{}",
                t.designation,
                if t.class.is_empty() {
                    String::new()
                } else {
                    format!(" - {}", t.class)
                },
                if t.hand == HoleThreadHand::Left {
                    " LH"
                } else {
                    ""
                }
            )
        }),
        note: if definition.is_some_and(|d| matches!(d.extent, HoleExtent::ThroughAll)) {
            "THRU".into()
        } else {
            String::new()
        },
        source_feature_id: definition.map(|d| d.feature_id.0),
        feature_name: definition.map_or_else(String::new, |d| d.name.clone()),
        hole_style: match style {
            HoleStyle::Simple => DrawingHoleStyle::Simple,
            HoleStyle::Counterbore => DrawingHoleStyle::Counterbore,
            HoleStyle::Countersink => DrawingHoleStyle::Countersink,
        },
        counterbore_diameter: definition
            .filter(|_| style == HoleStyle::Counterbore)
            .map(|d| d.counterbore_diameter),
        counterbore_depth: definition
            .filter(|_| style == HoleStyle::Counterbore)
            .map(|d| d.counterbore_depth),
        countersink_diameter: definition
            .filter(|_| style == HoleStyle::Countersink)
            .map(|d| d.countersink_diameter),
        countersink_angle_deg: definition
            .filter(|_| style == HoleStyle::Countersink)
            .map(|d| d.countersink_angle_deg),
        thread_depth: thread.and_then(|t| t.depth),
        pattern_note: if quantity > 1 {
            format!("{quantity} HOLES")
        } else {
            String::new()
        },
    };
    let mut next = document.clone();
    let sheet = next
        .sheets
        .iter_mut()
        .find(|s| s.id == stamp.sheet_id)
        .ok_or("Drawing sheet changed")?;
    if !sheet.views.iter().any(|v| v.id == target.view_id) {
        return Err("Drawing view changed".into());
    }
    sheet.annotations.push(annotation);
    if sheet.release.status == DrawingReleaseStatus::Released {
        sheet.release.status = DrawingReleaseStatus::Draft;
    }
    next.next_annotation_id = next
        .next_annotation_id
        .checked_add(1)
        .ok_or("Annotation IDs are exhausted")?;
    next.validate()?;
    Ok(next)
}

#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod tests;
