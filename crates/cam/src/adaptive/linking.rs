//! Optional rolls for conservative fallback laps. These leads may occupy
//! only certified empty stock: entry preferences never bypass the load gate.
use super::super::linking_planner;
use super::*;

pub(super) fn roll(
    builder: &mut ProgramBuilder,
    setup: &CamSetupDto,
    cleared: &Cleared,
    (anchor, tangent): (Point2Dto, Point2Dto),
    (depth, r): (f64, f64),
    entry: bool,
    plunge: f64,
) -> Result<bool, CamPlanError> {
    let Some(link) = builder.linking.clone() else {
        return Ok(false);
    };
    let spec = if entry {
        link.lead_in.clone()
    } else {
        link.exit()
    };
    if !spec.enabled {
        return Ok(false);
    }
    let path = if entry {
        [
            anchor,
            Point2Dto::new(anchor.x + tangent.x, anchor.y + tangent.y),
        ]
    } else {
        [
            Point2Dto::new(anchor.x - tangent.x, anchor.y - tangent.y),
            anchor,
        ]
    };
    let leads = linking_planner::contour_leads(
        &path,
        super::super::ContourLeadOptions {
            closed: false,
            inside_closed: false,
            lead_in: 0.0,
            lead_out: 0.0,
            arc_radius: None,
            bend_left: true,
            control_compensation: None,
        },
        &link,
    )?;
    let arc = if entry {
        leads.start_arc.as_ref()
    } else {
        leads.end_arc.as_ref()
    };
    let (a, b) = if entry {
        (leads.start, leads.line_end)
    } else {
        (arc.map_or(anchor, |a| a.arc_end), leads.end)
    };
    let t = if dist(a, b) > EPS {
        Point2Dto::new((b.x - a.x) / dist(a, b), (b.y - a.y) / dist(a, b))
    } else {
        arc.map_or(tangent, |a| {
            linking_planner::arc_tangent(if entry { leads.line_end } else { a.arc_end }, a)
        })
    };
    let vertical = if entry {
        Point2Dto::new(
            a.x - t.x * spec.vertical_radius,
            a.y - t.y * spec.vertical_radius,
        )
    } else {
        Point2Dto::new(
            b.x + t.x * spec.vertical_radius,
            b.y + t.y * spec.vertical_radius,
        )
    };
    let mut clear = link_clear(setup, cleared, a, b, r)
        && link_clear(
            setup,
            cleared,
            if entry { vertical } else { b },
            if entry { a } else { vertical },
            r,
        );
    if let Some(arc) = arc {
        let start = if entry { leads.line_end } else { anchor };
        let radius = dist(start, arc.center);
        let angle = (start.y - arc.center.y).atan2(start.x - arc.center.x);
        let end = (arc.arc_end.y - arc.center.y).atan2(arc.arc_end.x - arc.center.x);
        let sign = if arc.clockwise { -1.0 } else { 1.0 };
        let sweep = ((end - angle) * sign).rem_euclid(TAU);
        let n = (sweep / 5.0_f64.to_radians()).ceil().max(1.0) as usize;
        let padding = radius * (1.0 - (sweep / (2 * n) as f64).cos());
        clear &= (0..n).all(|i| {
            link_clear(
                setup,
                cleared,
                polar(
                    arc.center,
                    radius,
                    angle + sign * sweep * i as f64 / n as f64,
                ),
                polar(
                    arc.center,
                    radius,
                    angle + sign * sweep * (i + 1) as f64 / n as f64,
                ),
                r + padding,
            )
        });
    }
    if !clear {
        let warning="Some fallback roughing rolls did not fit certified empty stock. Those links use a checked axial entry/retract instead; requested radii are not silently shrunk.";
        if !builder.warnings.iter().any(|w| w == warning) {
            builder.warnings.push(warning.into());
        }
        return Ok(false);
    }
    let feed = if entry {
        link.lead_in_feed
    } else {
        link.lead_out_feed
    };
    if entry {
        linking_planner::entry(
            builder,
            leads.start,
            t,
            depth,
            spec.vertical_radius,
            plunge,
            feed,
        )?;
        builder.linear(
            Point3Dto::new(leads.line_end.x, leads.line_end.y, depth),
            feed,
        );
        if let Some(arc) = arc {
            builder.circular(
                Point3Dto::new(anchor.x, anchor.y, depth),
                arc.center,
                arc.clockwise,
                feed,
            );
        }
    } else {
        if let Some(arc) = arc {
            builder.circular(
                Point3Dto::new(arc.arc_end.x, arc.arc_end.y, depth),
                arc.center,
                arc.clockwise,
                feed,
            );
        }
        builder.linear(Point3Dto::new(leads.end.x, leads.end.y, depth), feed);
        linking_planner::exit(builder, leads.end, t, depth, spec.vertical_radius, feed)?;
    }
    Ok(true)
}

pub(super) fn exit_lap(
    builder: &mut ProgramBuilder,
    setup: &CamSetupDto,
    cleared: &Cleared,
    previous: Option<Point2Dto>,
    q: f64,
    r: f64,
    depth: f64,
) -> Result<(), CamPlanError> {
    if let (Some(c), Some(from)) = (previous, builder.position) {
        let a = xy(from);
        if (from.z - depth).abs() < EPS && (dist(a, c) - q).abs() < 1e-6 {
            let t = Point2Dto::new(-(a.y - c.y) / q, (a.x - c.x) / q);
            roll(builder, setup, cleared, (a, t), (depth, r), false, 1.0)?;
        }
    }
    Ok(())
}
