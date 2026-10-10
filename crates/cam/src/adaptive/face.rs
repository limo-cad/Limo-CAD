//! Face-mill roughing enters from air and clears nested convex exterior
//! sections. Each complete layer bounds remaining stock for the following
//! layer; no helix, plunge into stock, or unproved cavity fallback is used.
//! Insert programming geometry does not declare physical body/holder relief.
use super::*;

pub(super) fn plan(
    builder: &mut ProgramBuilder,
    setup: &CamSetupDto,
    operation: &CamOperationDto,
    tool: &CamToolDto,
    geometry: &crate::CamAdaptiveGeometryDto,
    ceiling: f64,
) -> Result<(), CamPlanError> {
    let CamOperationDto::Adaptive3d {
        name,
        top_z,
        bottom_z,
        parameters: p,
        cutting,
        ..
    } = operation
    else {
        unreachable!()
    };
    let profile = crate::CutterProfile::new(tool.into()).map_err(CamPlanError)?;
    let ap = tool.maximum_axial_depth.unwrap_or(tool.flute_length);
    if p.maximum_stepdown > ap + EPS {
        return Err(CamPlanError(format!(
            "Face-mill roughing stepdown exceeds tool {} maximum axial depth {ap:.3} mm",
            tool.label()
        )));
    }

    if builder.incoming_top - bottom_z > tool.overall_length + EPS {
        return Err(CamPlanError(
            "Face-mill roughing depth exceeds the declared tool length.".into(),
        ));
    }
    let r = tool.diameter * 0.5;
    let floor_r = profile.radius_at_height(0.0).unwrap();
    let mut work = Work::default();
    let envelope = capture_envelope(
        builder,
        setup,
        geometry,
        p.tolerance,
        2.0 * (r + p.radial_stock_to_leave + p.tolerance),
        &mut work,
    )?;
    let depths = roughing_depth_levels(setup, &geometry.targets, *top_z, *bottom_z, ceiling, p)?;
    let mut previous: Option<ConvexStock> = None;
    let mut previous_depth = builder.incoming_top;
    let mut passes = 0;
    let mut layers = 0;
    for depth in depths {
        if depth >= builder.incoming_top - EPS {
            continue;
        }
        if previous_depth - depth > ap + EPS || previous_depth - depth > p.maximum_stepdown + EPS {
            return Err(CamPlanError(
                "Face-mill layer exceeds the permitted axial engagement from remaining stock."
                    .into(),
            ));
        }
        let front = ConvexStock::from_envelope(&envelope, depth, p.axial_stock_to_leave,
            p.radial_stock_to_leave, &mut work)?.ok_or_else(|| CamPlanError(
                "Face-mill roughing cannot certify this target section within its convex-envelope budget. Split the operation or use an end mill.".into()))?;
        let mut front = front.refine_circular(
            setup,
            &geometry.targets,
            depth,
            p.axial_stock_to_leave,
            p.tolerance,
            &mut work,
        )?;
        if previous
            .as_ref()
            .is_some_and(|prior| !front.contains_bound(prior))
        {
            return Err(CamPlanError("Face-mill roughing cannot prove clearance above this layer; the preceding stock bound is not contained by the new section.".into()));
        }
        passes += front.clear_exterior(
            builder,
            setup,
            (r, floor_r, depth),
            p,
            (cutting.feed_xy, cutting.feed_z),
            (&mut work, &envelope, &[]),
        )?;
        builder.retract_to_clearance();
        front.mark_completed_cap(floor_r, p);
        previous = Some(front);
        previous_depth = depth;
        layers += 1;
    }
    if passes == 0 {
        if builder.rest_stock.is_some() {
            builder.warnings.push(format!("Face-mill roughing '{name}' found no remaining stock to cut; the operation is empty."));
            return Ok(());
        }
        return Err(CamPlanError(format!(
            "Face-mill roughing '{name}' found no accessible exterior stock."
        )));
    }
    builder.warnings.push(format!("Face-mill roughing '{name}': {layers} shallow layers, {passes} continuous exterior passes, 0 helical entries. Maximum Ap {:.3} mm; requested stepdown {:.3} mm and radial engagement {:.3} mm. Each layer proves clearance from the preceding remaining-stock bound.", ap, p.maximum_stepdown, p.optimal_load));
    builder.warnings.push("Face-mill roughing clears the convex exterior with outside-stock entry. Enclosed cavities, concave bays and allowance bands remain stock even when Machine cavities is enabled. A top cap can also retain a central core when the minimum cutting radius prevents center clearing. Inspect remaining stock; no complete-clearing claim is made.".into());
    builder.warnings.push("Face-mill simulation removes material with the programming-radius envelope up to the full diameter; a vendor programming radius encloses the real insert edge, so real stock can exceed the simulated stock by the vendor's uncut allowance in floor-to-wall blends. Maximum Ap is enforced independently. The non-cutting body, insert pockets, holder, fixtures and machine envelopes are not modeled; this is not a body-clearance verification.".into());
    Ok(())
}
