//! Height-qualified removal certificates: a cut proves empty space upward
//! through the cutting length, never below its floor. Each cut must prove
//! full-diameter clearance at its Ap ceiling (Ap <= flute length). Together
//! with that prior cleared column, its removal certificate extends upward
//! to incoming stock top even when the tool is shorter than the total depth.
use super::*;

pub(super) struct Removal {
    pub depth: f64,
    pub exterior: Option<ConvexStock>,
    pub centers: Vec<Point2Dto>,
}

pub(super) fn restore(
    history: &[Removal],
    depth: f64,
    cleared: &mut Cleared,
    work: &mut Work,
) -> Result<(), CamPlanError> {
    for cut in history.iter().filter(|cut| cut.depth <= depth + EPS) {
        work.spend(cut.centers.len() * 81 + 1, 0)?;
        for &c in &cut.centers {
            cleared.add(c);
        }
        if let Some(bound) = &cut.exterior {
            if cleared
                .exterior
                .as_ref()
                .is_none_or(|old| old.contains_bound(bound))
            {
                cleared.exterior = Some(bound.clone());
            }
        }
    }
    Ok(())
}

pub(super) fn depth_order(
    setup: &CamSetupDto,
    meshes: &[CamStockMeshDto],
    top: f64,
    bottom: f64,
    ceiling: f64,
    p: &CamAdaptiveParametersDto,
    corner_height: f64,
) -> Result<Vec<f64>, CamPlanError> {
    if corner_height + EPS >= p.maximum_stepdown && top - bottom > p.maximum_stepdown + EPS {
        return roughing_depth_levels(setup, meshes, top, bottom, ceiling, p);
    }
    let terraces = roughing_terraces(setup, meshes, top, bottom, ceiling, p);
    let mut ordered = Vec::new();
    let mut upper = top;
    loop {
        let step = if ordered.is_empty() {
            p.maximum_stepdown
        } else {
            p.maximum_stepdown - corner_height
        };

        let lower = (upper - step).max(bottom);
        ordered.push(lower);

        ordered.extend(
            terraces
                .iter()
                .rev()
                .copied()
                .filter(|z| *z > lower + EPS && *z < upper - EPS),
        );
        upper = lower;
        if ordered.len() > 512 {
            return Err(CamPlanError(
                "High Speed Roughing is limited to 512 depth levels per operation.".into(),
            ));
        }
        if lower <= bottom + EPS {
            break;
        }
    }
    Ok(ordered)
}
