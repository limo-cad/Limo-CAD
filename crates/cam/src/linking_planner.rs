//! Geometry for explicitly requested leads and links. Proofs operate on the
//! physical cutter, before controller compensation is converted to NC words.
use super::*;
use crate::linking::{CamFaceTransition, CamLeadDto, CamRetractionPolicy};

#[derive(Clone)]
pub(super) struct PredrilledHole {
    pub center: Point2Dto,
    pub radius: f64,
    pub bottom: f64,
}
pub(super) fn predrilled_holes(
    document: &CamDocumentDto,
    setup: &crate::model::CamSetupDto,
    operation_id: u64,
) -> Vec<PredrilledHole> {
    let mut result = Vec::new();
    for operation in setup
        .operations
        .iter()
        .take_while(|o| o.id() != operation_id)
        .filter(|o| o.enabled())
    {
        let CamOperationDto::Drill {
            points,
            holes,
            top_z,
            bottom_z,
            cycle,
            drill_tip_through,
            breakthrough_depth,
            ..
        } = operation
        else {
            continue;
        };
        if !matches!(
            cycle,
            DrillCycle::Drill | DrillCycle::ChipBreaking | DrillCycle::DeepHole
        ) {
            continue;
        }
        let Some(tool) = document.tool(operation.tool_id()) else {
            continue;
        };
        let radius = tool.diameter / 2.0;
        let tip = radius / (tool.point_angle_degrees.unwrap_or(118.0).to_radians() / 2.0).tan();
        for (center, top, bottom) in points
            .iter()
            .map(|&p| (p, *top_z, *bottom_z))
            .chain(holes.iter().map(|h| (h.point, h.top_z, h.bottom_z)))
        {
            if top + 1e-6 < setup.stock.max.z {
                continue;
            }
            let bottom = bottom
                - if *drill_tip_through {
                    tip + breakthrough_depth
                } else {
                    0.0
                }
                + tip;
            result.push(PredrilledHole {
                center,
                radius,
                bottom,
            });
        }
    }
    result
}

fn shifted(p: Point2Dto, t: Point2Dto, amount: f64) -> Point2Dto {
    Point2Dto::new(p.x + t.x * amount, p.y + t.y * amount)
}
pub(super) fn outside_stock(
    builder: &ProgramBuilder,
    a: Point2Dto,
    b: Point2Dto,
    radius: f64,
) -> bool {
    let Some(stock) = &builder.incoming_bounds else {
        return false;
    };
    let polygon = [
        Point2Dto::new(stock.min.x, stock.min.y),
        Point2Dto::new(stock.max.x, stock.min.y),
        Point2Dto::new(stock.max.x, stock.max.y),
        Point2Dto::new(stock.min.x, stock.max.y),
    ];
    !point_in_polygon(a, &polygon)
        && !point_in_polygon(b, &polygon)
        && (0..4).all(|i| {
            segment_segment_distance(a, b, polygon[i], polygon[(i + 1) % 4]) >= radius - 1e-7
        })
}

/// Linearized quarter circle in an arbitrary vertical plane: <= 5 degrees
/// per chord and <= 0.005 mm chordal error, with a bounded command count.
pub(super) fn vertical_points(
    anchor: Point2Dto,
    tangent: Point2Dto,
    z: f64,
    radius: f64,
    entry: bool,
) -> Result<Vec<Point3Dto>, CamPlanError> {
    if radius <= EPSILON {
        return Ok(vec![Point3Dto::new(anchor.x, anchor.y, z)]);
    }
    let angle = (2.0 * (1.0 - (0.005 / radius).min(1.0)).acos()).min(5.0_f64.to_radians());
    let n = (std::f64::consts::FRAC_PI_2 / angle).ceil() as usize;
    if n > 4096 {
        return Err(CamPlanError(
            "Vertical lead exceeds its geometry budget; reduce the radius.".into(),
        ));
    }
    Ok((0..=n)
        .map(|i| {
            let a = std::f64::consts::FRAC_PI_2 * i as f64 / n as f64;
            let (travel, height) = if entry {
                (-radius * a.cos(), radius * (1.0 - a.sin()))
            } else {
                (radius * a.sin(), radius * (1.0 - a.cos()))
            };
            let p = shifted(anchor, tangent, travel);
            Point3Dto::new(p.x, p.y, z + height)
        })
        .collect())
}
pub(super) fn entry(
    builder: &mut ProgramBuilder,
    anchor: Point2Dto,
    tangent: Point2Dto,
    depth: f64,
    radius: f64,
    plunge: f64,
    feed: f64,
) -> Result<(), CamPlanError> {
    if depth + radius > builder.feed_height_z + EPSILON {
        return Err(CamPlanError("Vertical lead-in radius reaches above Feed Height. Reduce the radius or raise Feed Height.".into()));
    }
    let points = vertical_points(anchor, tangent, depth, radius, true)?;
    let first = points[0];
    if !stay_down(builder, first) {
        builder.approach(Point2Dto::new(first.x, first.y), first.z, plunge);
    }
    ensure_program_budget(builder.commands.len(), points.len(), "vertical entry")?;
    for p in points.into_iter().skip(1) {
        builder.linear(p, feed);
    }
    Ok(())
}

fn stay_down(builder: &mut ProgramBuilder, to: Point3Dto) -> bool {
    let Some(link) = builder.linking.clone().filter(|l| l.keep_tool_down) else {
        return false;
    };
    let Some(from) = builder
        .position
        .filter(|p| p.z < builder.retract_z - EPSILON)
    else {
        return false;
    };
    let a = Point2Dto::new(from.x, from.y);
    let b = Point2Dto::new(to.x, to.y);
    let lift = from.z.max(to.z) + link.lift_height;
    if lift > builder.feed_height_z
        || distance_2d(a, b) + (lift - from.z) + (lift - to.z) > link.maximum_stay_down
        || !outside_stock(
            builder,
            a,
            b,
            builder.tool_radius + link.minimum_clearance.max(link.safe_distance),
        )
    {
        return false;
    }
    builder.linear(Point3Dto::new(a.x, a.y, lift), link.no_engagement_feed);
    builder.linear(Point3Dto::new(b.x, b.y, lift), link.no_engagement_feed);
    builder.linear(to, link.no_engagement_feed);
    true
}

/// Non-cutting policy for roughing, after a target/stock travel envelope has
/// been captured. Straight diagonals are always fed; G0 dog-leg behavior is
/// never assumed. Minimum retraction uses a conservative obstacle-top bound.
pub(super) fn approach_policy(
    builder: &mut ProgramBuilder,
    p: Point2Dto,
    depth: f64,
    plunge: f64,
) -> bool {
    let Some(bounds) = builder.link_obstacles else {
        return false;
    };
    let Some(link) = builder
        .linking
        .clone()
        .filter(|l| l.retraction_policy != CamRetractionPolicy::Full)
    else {
        return false;
    };
    let Some(from) = builder.position else {
        return false;
    };
    let to = Point3Dto::new(p.x, p.y, builder.feed_plane(depth));
    let safe_z = (bounds.max.z + link.safe_distance).max(builder.feed_height_z);
    if safe_z > builder.clearance_z {
        return false;
    }
    let mut checker = ProgramBuilder::new();
    checker.incoming_bounds = Some(bounds);
    let straight_clear = from.z.min(to.z) > bounds.max.z + link.safe_distance
        || outside_stock(
            &checker,
            Point2Dto::new(from.x, from.y),
            p,
            builder.tool_radius + link.safe_distance,
        );
    if link.retraction_policy == CamRetractionPolicy::Shortest && straight_clear {
        builder.linear(to, link.high_feed);
    } else {
        let travel = safe_z.max(from.z);
        builder.rapid(Point3Dto::new(from.x, from.y, travel));
        builder.rapid(Point3Dto::new(p.x, p.y, travel));
        builder.rapid(to);
    }
    builder.linear(Point3Dto::new(p.x, p.y, depth), plunge);
    true
}
pub(super) fn exit(
    builder: &mut ProgramBuilder,
    anchor: Point2Dto,
    tangent: Point2Dto,
    depth: f64,
    radius: f64,
    feed: f64,
) -> Result<(), CamPlanError> {
    if depth + radius > builder.feed_height_z + EPSILON {
        return Err(CamPlanError("Vertical lead-out radius reaches above Feed Height. Reduce the radius or raise Feed Height.".into()));
    }
    let points = vertical_points(anchor, tangent, depth, radius, false)?;
    ensure_program_budget(builder.commands.len(), points.len(), "vertical exit")?;
    for p in points.into_iter().skip(1) {
        builder.linear(p, feed);
    }
    Ok(())
}

pub(super) fn plan_face(
    builder: &mut ProgramBuilder,
    setup: &crate::model::CamSetupDto,
    operation: &CamOperationDto,
    tool: &CamToolDto,
) -> Result<(), CamPlanError> {
    let CamOperationDto::Face {
        bounds,
        top_z,
        target_z,
        step_over,
        step_down,
        safe_distance,
        direction,
        cutting,
        name,
        ..
    } = operation
    else {
        unreachable!()
    };
    let link = builder.linking.clone().unwrap();
    let out = link.exit();
    let rin = if link.lead_in.enabled {
        link.lead_in.vertical_radius
    } else {
        0.0
    };
    let rout = if out.enabled {
        out.vertical_radius
    } else {
        0.0
    };
    let material_top = top_z.max(builder.incoming_top);
    require_flute_length(tool, material_top - target_z, name)?;
    let r = tool.diameter / 2.0;
    let n = (((bounds.max.y - bounds.min.y - tool.diameter).max(0.0) / step_over).ceil() as usize)
        .saturating_add(1);
    let depths = depth_levels(material_top, *target_z, *step_down)?;
    ensure_program_budget(
        builder.commands.len(),
        depths.len().saturating_mul(n.saturating_mul(60)),
        name,
    )?;
    let ys = (0..n)
        .map(|i| {
            (bounds.min.y + bounds.max.y) / 2.0 + (i as f64 - (n - 1) as f64 / 2.0) * step_over
        })
        .collect::<Vec<_>>();
    let pad = safe_distance.max(link.safe_distance).max(1e-5);
    let left = setup.stock.min.x - r - pad;
    let right = setup.stock.max.x + r + pad;
    for depth in depths {
        for (index, &y) in ys.iter().enumerate() {
            let backwards = match direction {
                FaceDirection::Climb => true,
                FaceDirection::Conventional => false,
                FaceDirection::BothWays => index % 2 == 1,
            };
            let t = Point2Dto::new(if backwards { -1.0 } else { 1.0 }, 0.0);
            let start = Point2Dto::new(if backwards { right } else { left }, y);
            let end_x = if link.extend_before_retract {
                if backwards {
                    left
                } else {
                    right
                }
            } else if backwards {
                bounds.min.x - r
            } else {
                bounds.max.x + r
            };
            let end = Point2Dto::new(end_x, y);

            if !outside_stock(builder, end, shifted(end, t, rout), r) {
                return Err(CamPlanError(format!("Face '{name}' exit is not clear of incoming stock. Enable Extend before retract or use a full-stock boundary.")));
            }
            let already_there = builder
                .position
                .is_some_and(|p| distance(p, Point3Dto::new(start.x, start.y, depth)) < 1e-7);
            if !already_there {
                entry(
                    builder,
                    start,
                    t,
                    depth,
                    rin,
                    cutting.feed_z,
                    link.lead_in_feed,
                )?;
            }
            builder.linear(Point3Dto::new(end.x, end.y, depth), cutting.feed_xy);
            let next_y = ys.get(index + 1).copied();
            let can_link = next_y.is_some()
                && matches!(direction, FaceDirection::BothWays)
                && link.keep_tool_down
                && link.transition != CamFaceTransition::NoContact
                && link.lift_height == 0.0;
            if let Some(ny) = next_y.filter(|_| can_link) {
                let next = Point2Dto::new(if backwards { left } else { right }, ny);
                let delta = ny - y;
                let distance = if link.transition == CamFaceTransition::Smooth {
                    delta.abs() * std::f64::consts::FRAC_PI_2 + (end.x - next.x).abs()
                } else {
                    distance_2d(end, next)
                };
                if distance <= link.maximum_stay_down
                    && outside_stock(builder, end, next, r + link.minimum_clearance)
                {
                    if link.transition == CamFaceTransition::Smooth {
                        let end = Point2Dto::new(next.x, y);
                        builder
                            .linear(Point3Dto::new(end.x, end.y, depth), link.no_engagement_feed);
                        builder.circular(
                            Point3Dto::new(next.x, next.y, depth),
                            Point2Dto::new(next.x, (y + ny) / 2.0),
                            backwards,
                            link.no_engagement_feed,
                        );
                    } else if link.transition == CamFaceTransition::Straight {
                        builder.linear(Point3Dto::new(next.x, y, depth), link.no_engagement_feed);
                        builder.linear(Point3Dto::new(next.x, ny, depth), link.no_engagement_feed);
                    } else {
                        builder.linear(Point3Dto::new(next.x, ny, depth), link.no_engagement_feed);
                    }
                    continue;
                }
            }
            exit(builder, end, t, depth, rout, link.lead_out_feed)?;
            builder.retract_to_clearance();
        }
    }
    let (cut_min_x, cut_max_x) = if link.extend_before_retract {
        (left, right)
    } else {
        ((bounds.min.x - r).max(left), (bounds.max.x + r).min(right))
    };
    if facing_clears_stock_floor(tool, &setup.stock, &ys, cut_min_x, cut_max_x) {
        builder.incoming_top = builder.incoming_top.min(*target_z);
    }
    Ok(())
}

pub(super) fn contour_leads(
    path: &[Point2Dto],
    options: ContourLeadOptions,
    link: &CamLinkingDto,
) -> Result<ContourLeads, CamPlanError> {
    let first = path[0];
    let last = path.len() - 1;
    let (end, t1) = if options.closed {
        (first, unit_direction(path[last], first)?)
    } else {
        (path[last], unit_direction(path[last - 1], path[last])?)
    };
    let t0 = unit_direction(path[0], path[1])?;
    let sign = if options.bend_left { 1.0 } else { -1.0 };
    let rotate = |v: Point2Dto, angle: f64| {
        Point2Dto::new(
            v.x * angle.cos() - v.y * angle.sin(),
            v.x * angle.sin() + v.y * angle.cos(),
        )
    };
    let normal = |t: Point2Dto| Point2Dto::new(-t.y * sign, t.x * sign);
    let compensate = |p: Point2Dto, t: Point2Dto| {
        if let Some((left, r)) = options.control_compensation {
            let s = if left { 1.0 } else { -1.0 };
            Point2Dto::new(p.x - t.y * s * r, p.y + t.x * s * r)
        } else {
            p
        }
    };
    let make = |anchor: Point2Dto,
                t: Point2Dto,
                lead: &CamLeadDto,
                entry: bool|
     -> (Point2Dto, Point2Dto, Option<LeadArc>) {
        let r = if lead.enabled && lead.sweep_degrees > 0.0 {
            lead.horizontal_radius
        } else {
            0.0
        };
        let length = if lead.enabled {
            lead.linear_distance
        } else {
            0.0
        };
        if r <= EPSILON {
            let travel = if lead.perpendicular {
                if entry {
                    shifted(Point2Dto::new(0.0, 0.0), normal(t), -1.0)
                } else {
                    normal(t)
                }
            } else {
                t
            };
            return (
                shifted(
                    compensate(anchor, t),
                    travel,
                    if entry { -length } else { length },
                ),
                anchor,
                None,
            );
        }
        let nominal = r + options.control_compensation.map_or(0.0, |(_, r)| r);
        let center = shifted(anchor, normal(t), nominal);
        let angle = sign * lead.sweep_degrees.to_radians() * if entry { -1.0 } else { 1.0 };
        let radial = rotate(
            Point2Dto::new(anchor.x - center.x, anchor.y - center.y),
            angle,
        );
        let arc_point = Point2Dto::new(center.x + radial.x, center.y + radial.y);
        let tangent = rotate(t, angle);
        let travel = if lead.perpendicular {
            Point2Dto::new(
                radial.x / nominal * if entry { 1.0 } else { -1.0 },
                radial.y / nominal * if entry { 1.0 } else { -1.0 },
            )
        } else {
            tangent
        };
        (
            shifted(
                compensate(arc_point, tangent),
                travel,
                if entry { -length } else { length },
            ),
            arc_point,
            Some(LeadArc {
                center,
                clockwise: !options.bend_left,
                arc_end: if entry { anchor } else { arc_point },
            }),
        )
    };
    let (start, line_end, start_arc) = make(first, t0, &link.lead_in, true);
    let (end, _, end_arc) = make(end, t1, &link.exit(), false);
    Ok(ContourLeads {
        start,
        line_end,
        start_arc,
        end_arc,
        end,
    })
}

pub(super) fn leads_clear(
    leads: &ContourLeads,
    end_anchor: Point2Dto,
    profile: &[Point2Dto],
    material_inside: bool,
    r: f64,
) -> bool {
    let point = |p| {
        point_in_polygon(p, profile) != material_inside
            && (0..profile.len()).all(|i| {
                segment_distance(p, profile[i], profile[(i + 1) % profile.len()]) >= r - 1e-6
            })
    };
    let line = |a, b| {
        point(a)
            && point(b)
            && (0..profile.len()).all(|i| {
                segment_segment_distance(a, b, profile[i], profile[(i + 1) % profile.len()])
                    >= r - 1e-6
            })
    };
    let arc = |a, arc: &LeadArc| {
        point(a)
            && point(arc.arc_end)
            && (0..profile.len()).all(|i| {
                arc_segment_distance(a, arc, profile[i], profile[(i + 1) % profile.len()])
                    >= r - 1e-6
            })
    };
    line(leads.start, leads.line_end)
        && leads
            .start_arc
            .as_ref()
            .is_none_or(|a| arc(leads.line_end, a))
        && if let Some(a) = &leads.end_arc {
            arc(end_anchor, a) && line(a.arc_end, leads.end)
        } else {
            line(end_anchor, leads.end)
        }
}

/// Explicit chamfer leads share contour geometry and vertical blending.
/// The absent-linking path retains legacy automatic fitting. Here requested
/// dimensions are obligations: no auto-shrink, tool substitution or depth edit.
pub(super) fn plan_chamfer(
    builder: &mut ProgramBuilder,
    operation: &CamOperationDto,
    tool: &CamToolDto,
    path: &[Point2Dto],
    boundary: &[Point2Dto],
    profile_offset: f64,
    bend_left: bool,
) -> Result<(), CamPlanError> {
    let CamOperationDto::Chamfer2d {
        closed,
        top_z,
        chamfer_width,
        tip_offset,
        wall_side,
        cutting,
        name,
        ..
    } = operation
    else {
        unreachable!()
    };
    let link = builder.linking.clone().expect("manual chamfer linking");
    let out = link.exit();
    let depth = top_z - (chamfer_width + tip_offset);
    let rin = if link.lead_in.enabled {
        link.lead_in.vertical_radius
    } else {
        0.0
    };
    let rout = if out.enabled {
        out.vertical_radius
    } else {
        0.0
    };
    if depth + rin.max(rout) > builder.feed_height_z + EPSILON {
        return Err(CamPlanError(format!("Chamfer '{name}': a manual vertical lead reaches above Feed Height. Reduce its radius or raise Feed Height; requested radii were not changed.")));
    }
    let leads = contour_leads(
        path,
        ContourLeadOptions {
            closed: *closed,
            inside_closed: false,
            lead_in: 0.0,
            lead_out: 0.0,
            arc_radius: None,
            bend_left,
            control_compensation: None,
        },
        &link,
    )?;
    let profile_end = if *closed {
        path[0]
    } else {
        *path.last().unwrap()
    };
    let tin = if distance_2d(leads.start, leads.line_end) > EPSILON {
        unit_direction(leads.start, leads.line_end)?
    } else if let Some(arc) = &leads.start_arc {
        arc_tangent(leads.line_end, arc)
    } else {
        unit_direction(path[0], path[1])?
    };
    let exit_anchor = leads.end_arc.as_ref().map_or(profile_end, |a| a.arc_end);
    let tout = if distance_2d(exit_anchor, leads.end) > EPSILON {
        unit_direction(exit_anchor, leads.end)?
    } else if let Some(arc) = &leads.end_arc {
        arc_tangent(arc.arc_end, arc)
    } else {
        unit_direction(
            if *closed {
                *path.last().unwrap()
            } else {
                path[path.len() - 2]
            },
            profile_end,
        )?
    };

    let mut checked = leads.clone();
    checked.start = shifted(leads.start, tin, -rin);
    checked.end = shifted(leads.end, tout, rout);
    let clear = if *closed {
        leads_clear(
            &checked,
            profile_end,
            boundary,
            matches!(wall_side, ContourCompensation::Inside),
            profile_offset,
        )
    } else {
        open_leads_clear_profile(&checked, path, boundary, profile_offset, None)?
    };
    if !clear {
        return Err(CamPlanError(format!("Chamfer '{name}': manual lead-in/out crosses the selected protected profile. Reduce the horizontal/vertical radii, sweep or straight distance, or choose Automatic fitting. Requested dimensions were not changed.")));
    }
    builder.require_clear_approach(checked.start, tool.diameter * 0.5, name)?;
    entry(
        builder,
        leads.start,
        tin,
        depth,
        rin,
        cutting.feed_z,
        link.lead_in_feed,
    )?;
    builder.linear(
        Point3Dto::new(leads.line_end.x, leads.line_end.y, depth),
        link.lead_in_feed,
    );
    if let Some(arc) = &leads.start_arc {
        builder.circular(
            Point3Dto::new(path[0].x, path[0].y, depth),
            arc.center,
            arc.clockwise,
            link.lead_in_feed,
        );
    }
    emit_profile_lap(builder, path, *closed, depth, cutting.feed_xy);
    if let Some(arc) = &leads.end_arc {
        builder.circular(
            Point3Dto::new(arc.arc_end.x, arc.arc_end.y, depth),
            arc.center,
            arc.clockwise,
            link.lead_out_feed,
        );
    }
    builder.linear(
        Point3Dto::new(leads.end.x, leads.end.y, depth),
        link.lead_out_feed,
    );
    exit(builder, leads.end, tout, depth, rout, link.lead_out_feed)?;

    builder.retract_to_clearance();
    Ok(())
}

pub(super) fn air_leads_against_stock(
    builder: &ProgramBuilder,
    start: Point2Dto,
    end: Point2Dto,
    r: f64,
    polygon: &[Point2Dto],
) -> Result<(ContourLeads, Point2Dto, Point2Dto), CamPlanError> {
    let link = builder.linking.as_ref().unwrap();
    let path = [start, end];
    let t = unit_direction(start, end)?;
    let leads = contour_leads(
        &path,
        ContourLeadOptions {
            closed: false,
            inside_closed: false,
            lead_in: 0.0,
            lead_out: 0.0,
            arc_radius: None,
            bend_left: true,
            control_compensation: None,
        },
        link,
    )?;
    let tangent = |a, b| {
        if distance_2d(a, b) > EPSILON {
            unit_direction(a, b).unwrap()
        } else {
            t
        }
    };
    let tin = leads
        .start_arc
        .as_ref()
        .filter(|_| distance_2d(leads.start, leads.line_end) < EPSILON)
        .map_or_else(
            || tangent(leads.start, leads.line_end),
            |a| arc_tangent(leads.line_end, a),
        );
    let tout = leads
        .end_arc
        .as_ref()
        .filter(|a| distance_2d(a.arc_end, leads.end) < EPSILON)
        .map_or_else(
            || tangent(leads.end_arc.as_ref().map_or(end, |a| a.arc_end), leads.end),
            |a| arc_tangent(a.arc_end, a),
        );
    let mut checked = leads.clone();
    checked.start = shifted(
        checked.start,
        tin,
        -if link.lead_in.enabled {
            link.lead_in.vertical_radius
        } else {
            0.0
        },
    );
    checked.end = shifted(
        checked.end,
        tout,
        if link.exit().enabled {
            link.exit().vertical_radius
        } else {
            0.0
        },
    );
    if !leads_clear(&checked, end, polygon, true, r) {
        return Err(CamPlanError("Requested roughing leads cannot fit outside the incoming billet; reduce the lead radii/lengths or choose a clearer entry side.".into()));
    }
    Ok((leads, tin, tout))
}

/// Move a lead anchor toward the cutting endpoint without changing its
/// requested radius, sweep, straight extension or vertical blend. Every
/// accepted position passes the same complete lead-envelope clearance test.
/// The caller supplies an already conservative outside-stock distance.
pub(super) fn fit_air_lead_distance(
    builder: &ProgramBuilder,
    cut: Point2Dto,
    tangent: Point2Dto,
    r: f64,
    polygon: &[Point2Dto],
    entry: bool,
    maximum: f64,
) -> Result<f64, CamPlanError> {
    let fits = |d: f64| {
        let anchor = shifted(cut, tangent, if entry { -d } else { d });
        let (a, b) = if entry {
            (anchor, shifted(anchor, tangent, 1.0))
        } else {
            (shifted(anchor, tangent, -1.0), anchor)
        };
        air_leads_against_stock(builder, a, b, r, polygon)
    };
    fits(maximum)?;
    if fits(0.0).is_ok() {
        return Ok(0.0);
    }
    let (mut lo, mut hi) = (0.0, maximum);
    for _ in 0..24 {
        let mid = (lo + hi) * 0.5;
        if fits(mid).is_ok() {
            hi = mid;
        } else {
            lo = mid;
        }
    }

    fits(hi)?;
    Ok(hi)
}
pub(super) fn arc_tangent(p: Point2Dto, arc: &LeadArc) -> Point2Dto {
    let v = unit_direction(arc.center, p).expect("validated nonzero lead radius");
    if arc.clockwise {
        Point2Dto::new(v.y, -v.x)
    } else {
        Point2Dto::new(-v.y, v.x)
    }
}

/// Prefer the nearest straight station, not a corner; the downstream complete
/// lead-envelope proof still decides whether the preference is machinable.
pub(super) fn split_at_hint(
    path: &[Point2Dto],
    hint: Point2Dto,
) -> Result<Vec<Point2Dto>, CamPlanError> {
    let (index, p) = (0..path.len())
        .filter_map(|i| {
            let a = path[i];
            let b = path[(i + 1) % path.len()];
            let length = distance_2d(a, b);
            if length <= 1e-7 {
                return None;
            }
            let t = (((hint.x - a.x) * (b.x - a.x) + (hint.y - a.y) * (b.y - a.y))
                / (length * length))
                .clamp(0.01, 0.99);
            let p = Point2Dto::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y));
            Some((i, p, distance_2d(p, hint)))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(i, p, _)| (i, p))
        .ok_or_else(|| CamPlanError("No usable edge near the preferred entry position.".into()))?;
    Ok(std::iter::once(p)
        .chain((1..=path.len()).map(|j| path[(index + j) % path.len()]))
        .collect())
}

pub(super) fn extend_to_exit(path: &[Point2Dto], hint: Point2Dto) -> Vec<Point2Dto> {
    let mut result = path.to_vec();
    result.push(path[0]);
    let (edge, p) = (0..path.len())
        .map(|i| {
            let a = path[i];
            let b = path[(i + 1) % path.len()];
            let d = distance_2d(a, b).max(1e-9);
            let t = (((hint.x - a.x) * (b.x - a.x) + (hint.y - a.y) * (b.y - a.y)) / (d * d))
                .clamp(0.01, 0.99);
            let p = Point2Dto::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y));
            (i, p, distance_2d(p, hint))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(i, p, _)| (i, p))
        .unwrap();
    result.extend(path.iter().copied().take(edge + 1).skip(1));
    result.push(p);
    result
}

pub(super) fn ramp_profile(
    builder: &mut ProgramBuilder,
    path: &[Point2Dto],
    closed: bool,
    top: f64,
    bottom: f64,
    link: &CamLinkingDto,
) -> Result<(), CamPlanError> {
    if !closed {
        return Err(CamPlanError(
            "Profile ramping requires a closed contour and no separate exit station.".into(),
        ));
    }
    let perimeter: f64 = (0..path.len())
        .map(|i| distance_2d(path[i], path[(i + 1) % path.len()]))
        .sum();
    let pitch = link
        .ramp_stepdown
        .min(perimeter * link.ramp_angle.to_radians().tan());
    let laps = ((top - bottom) / pitch).ceil().max(1.0) as usize;
    ensure_program_budget(
        builder.commands.len(),
        laps.saturating_mul(path.len()),
        "contour ramp",
    )?;
    let mut traveled = 0.0;
    for _ in 0..laps {
        for i in 0..path.len() {
            let p = path[(i + 1) % path.len()];
            traveled += distance_2d(path[i], p);
            let z = (top + (bottom - top) * traveled / (laps as f64 * perimeter)).max(bottom);
            builder.linear(Point3Dto::new(p.x, p.y, z), link.ramp_feed);
        }
    }
    Ok(())
}
