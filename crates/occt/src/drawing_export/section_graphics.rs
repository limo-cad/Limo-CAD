use super::graphics::{bytes, finite_points, Graphics};
use super::*;
use std::mem::size_of;

mod tiled;

/// Generate only section hatching. Contour selection, contour line style and
/// paint order remain each caller's existing policy. Coordinates are paper mm.
pub fn section_hatch(
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
    style: &DrawingLineStyleDto,
    pattern: HatchPattern,
    budget: &mut PaperGraphicsBudget,
) -> Result<Vec<PaperPrimitive>, String> {
    generate(view, projection, style, pattern, budget, false)
}

/// Live SVG-pattern policy: dash phase starts at each global along-line tile
/// origin, whose tile length is the hatch spacing. Clipping a boundary or void
/// never restarts that phase. Returned on-spans have solid line styles.
/// The caller still supplies the SVG vertical-line angle + 90 degrees.
pub fn section_hatch_tiled(
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
    style: &DrawingLineStyleDto,
    pattern: HatchPattern,
    budget: &mut PaperGraphicsBudget,
) -> Result<Vec<PaperPrimitive>, String> {
    generate(view, projection, style, pattern, budget, true)
}

fn generate(
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
    style: &DrawingLineStyleDto,
    pattern: HatchPattern,
    budget: &mut PaperGraphicsBudget,
    tiled: bool,
) -> Result<Vec<PaperPrimitive>, String> {
    finite_points(&[
        view.position,
        [projection.bounds[0], projection.bounds[1]],
        [projection.bounds[2], projection.bounds[3]],
    ])?;
    if !view.scale.is_finite() || view.scale <= 0. {
        return Err("Section hatch needs a finite positive view scale".into());
    }
    if !pattern.angle_deg.is_finite() || !pattern.spacing_mm.is_finite() || pattern.spacing_mm <= 0.
    {
        return Err("Section hatch needs a finite angle and positive spacing".into());
    }
    let tiles = if tiled && !style.dash_mm.is_empty() {
        Some(tiled::Pattern::new(style, pattern.spacing_mm, budget)?)
    } else {
        None
    };
    let angle = pattern.angle_deg.to_radians();
    if !angle.is_finite() {
        return Err("Section hatch angle is too large".into());
    }
    let u = [angle.cos(), angle.sin()];
    let n = [-angle.sin(), angle.cos()];
    let dot2 = |p: P, basis: P| p[0] * basis[0] + p[1] * basis[1];
    budget.work(projection.section.len() as u64)?;
    let count = projection.section.iter().try_fold(0usize, |sum, line| {
        sum.checked_add(line.points.len().saturating_sub(1))
            .ok_or("Section edge count overflow")
    })?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let edge_bytes = bytes(count, size_of::<[P; 2]>())?;
    let scratch_bytes = edge_bytes
        .checked_add(bytes(count, size_of::<f64>())?)
        .ok_or("Section scratch size overflow")?;
    budget.scratch(scratch_bytes)?;
    budget.work(count as u64)?;
    let mut edges = Vec::<[P; 2]>::new();
    edges
        .try_reserve_exact(count)
        .map_err(|_| "Cannot allocate section boundaries")?;
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for line in &projection.section {
        for pair in line.points.windows(2) {
            let pair = [
                paper_point(view, pair[0], projection),
                paper_point(view, pair[1], projection),
            ];
            finite_points(&pair)?;
            for point in pair {
                let y = dot2(point, n);
                if !y.is_finite() {
                    return Err("Section hatch bounds are non-finite".into());
                }
                min = min.min(y);
                max = max.max(y);
            }
            edges.push(pair);
        }
    }
    let first = (min / pattern.spacing_mm).floor();
    let last = (max / pattern.spacing_mm).ceil();
    if !first.is_finite() || !last.is_finite() || first < i64::MIN as f64 || last >= i64::MAX as f64
    {
        return Err("Section hatch coordinates exceed the supported range".into());
    }
    let start = first as i64;
    let end = last as i64;
    let span = end
        .checked_sub(start)
        .ok_or("Section hatch range overflow")?;
    if !(0..=20_000).contains(&span) {
        return Err("Section hatch is too dense for the sheet scale".into());
    }
    let scans = span as u64 + 1;
    budget.work(
        scans
            .checked_mul(count as u64)
            .ok_or("Section hatch work overflow")?,
    )?;
    let mut xs = Vec::<f64>::new();
    xs.try_reserve_exact(count)
        .map_err(|_| "Cannot allocate section intersections")?;
    let mut graphics = Graphics::new(budget);
    for i in start..=end {
        let y = i as f64 * pattern.spacing_mm;
        xs.clear();
        for [a, b] in &edges {
            let ay = dot2(*a, n);
            let by = dot2(*b, n);
            if (ay <= y && by > y) || (by <= y && ay > y) {
                let t = (y - ay) / (by - ay);
                let x = dot2([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])], u);
                if !x.is_finite() {
                    return Err("Section intersection is non-finite".into());
                }
                xs.push(x);
            }
        }
        let levels = if xs.len() < 2 {
            0
        } else {
            usize::BITS - (xs.len() - 1).leading_zeros()
        };
        graphics.budget.work(
            (xs.len() as u64)
                .checked_mul(levels as u64)
                .ok_or("Section sort work overflow")?,
        )?;
        xs.sort_unstable_by(f64::total_cmp);
        if !xs.len().is_multiple_of(2) {
            return Err("Section boundary is open; cannot hatch a manufacturing drawing".into());
        }
        for pair in xs.as_chunks::<2>().0 {
            if let Some(tiles) = &tiles {
                tiles.emit(&mut graphics, [pair[0], pair[1]], y, u, n)?;
                continue;
            }
            graphics.line(
                &[
                    [pair[0] * u[0] + y * n[0], pair[0] * u[1] + y * n[1]],
                    [pair[1] * u[0] + y * n[0], pair[1] * u[1] + y * n[1]],
                ],
                "HATCH",
                style,
            )?;
        }
    }
    Ok(graphics.finish())
}
