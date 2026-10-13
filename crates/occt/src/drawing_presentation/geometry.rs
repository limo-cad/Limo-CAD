//! Pure resolved paper-space dimension geometry, shared by native paint and export.
use limo_cad_sketch::{DrawingLineDimensionMode, DrawingLinearDimensionMode};

pub type P = [f64; 2];
pub fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}
pub fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}
pub fn scale(a: P, v: f64) -> P {
    [a[0] * v, a[1] * v]
}
pub fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
pub fn cross(a: P, b: P) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
pub fn length(a: P) -> f64 {
    a[0].hypot(a[1])
}
pub fn unit(a: P) -> Option<P> {
    let l = length(a);
    (l >= 1e-7 && l.is_finite()).then(|| scale(a, 1. / l))
}
pub fn midpoint(a: P, b: P) -> P {
    scale(add(a, b), 0.5)
}
pub fn normal(a: P) -> P {
    [-a[1], a[0]]
}
pub fn arc(center: P, radius: f64, start: f64, sweep: f64) -> Vec<P> {
    let count = ((sweep.abs() * radius.max(1.) / 0.4).ceil() as usize).clamp(8, 512);
    (0..=count)
        .map(|i| {
            let a = start + sweep * i as f64 / count as f64;
            add(center, [a.cos() * radius, a.sin() * radius])
        })
        .collect()
}

pub struct Linear {
    pub first: P,
    pub second: P,
    pub start: P,
    pub end: P,
    pub value: f64,
}

/// Resolve the existing linear/series intent in paper millimetres, reporting
/// the measured value in model millimetres. View scale never scales the offset.
pub fn dimension_span(
    mode: DrawingLinearDimensionMode,
    first: P,
    second: P,
    offset: f64,
    view_scale: f64,
) -> Option<Linear> {
    if !view_scale.is_finite()
        || view_scale <= 0.
        || !offset.is_finite()
        || first.into_iter().chain(second).any(|n| !n.is_finite())
    {
        return None;
    }
    let delta = sub(second, first);
    let value = match mode {
        DrawingLinearDimensionMode::Horizontal => delta[0].abs(),
        DrawingLinearDimensionMode::Vertical => delta[1].abs(),
        DrawingLinearDimensionMode::Aligned => length(delta),
    };
    if value < 1e-9 || !value.is_finite() {
        return None;
    }
    let (start, end) = match mode {
        DrawingLinearDimensionMode::Horizontal => (
            [first[0], first[1] + offset],
            [second[0], first[1] + offset],
        ),
        DrawingLinearDimensionMode::Vertical => (
            [first[0] + offset, first[1]],
            [first[0] + offset, second[1]],
        ),
        DrawingLinearDimensionMode::Aligned => {
            let normal = scale(normal(delta), offset / value);
            (add(first, normal), add(second, normal))
        }
    };
    let value = value / view_scale;
    (value.is_finite() && start.into_iter().chain(end).all(f64::is_finite)).then_some(Linear {
        first,
        second,
        start,
        end,
        value,
    })
}

pub struct Ordinate {
    pub origin: P,
    pub target: P,
    pub elbow: P,
    pub position: P,
    pub x_value: f64,
    pub y_value: f64,
}

/// The same dominant-axis leader and signed model values used on native paper.
pub fn ordinate(origin: P, target: P, offset: f64, view_scale: f64) -> Option<Ordinate> {
    if !view_scale.is_finite()
        || view_scale <= 0.
        || !offset.is_finite()
        || origin.into_iter().chain(target).any(|n| !n.is_finite())
    {
        return None;
    }
    let delta = sub(target, origin);
    if length(delta) < 1e-7 || !length(delta).is_finite() {
        return None;
    }
    let horizontal = delta[0].abs() >= delta[1].abs();
    let elbow = add(
        target,
        if horizontal {
            [0., offset]
        } else {
            [offset, 0.]
        },
    );
    let sign = if offset < 0. { -1. } else { 1. };
    let position = add(
        elbow,
        if horizontal {
            [0., sign * 2.]
        } else {
            [sign * 2., 0.]
        },
    );
    let x_value = delta[0] / view_scale;
    let y_value = -delta[1] / view_scale;
    (elbow
        .into_iter()
        .chain(position)
        .chain([x_value, y_value])
        .all(f64::is_finite))
    .then_some(Ordinate {
        origin,
        target,
        elbow,
        position,
        x_value,
        y_value,
    })
}
pub struct Angular {
    pub vertex: P,
    pub first: P,
    pub second: P,
    pub points: Vec<P>,
    pub text: P,
    pub value: f64,
}
pub enum LineDimension {
    Linear(Linear),
    Angular(Angular),
}
pub fn angular(vertex: P, first: P, second: P, radius: f64) -> Option<Angular> {
    let a = unit(sub(first, vertex))?;
    let b = unit(sub(second, vertex))?;
    let angle = dot(a, b).clamp(-1., 1.).acos();
    if angle < 1e-7 {
        return None;
    }
    let bisector = unit(add(a, b)).unwrap_or(normal(a));
    let sweep = angle * if cross(a, b) >= 0. { 1. } else { -1. };
    Some(Angular {
        vertex,
        first: add(vertex, scale(a, radius + 3.)),
        second: add(vertex, scale(b, radius + 3.)),
        points: arc(vertex, radius, a[1].atan2(a[0]), sweep),
        text: add(vertex, scale(bisector, radius + 4.)),
        value: angle.to_degrees(),
    })
}
fn closest(point: P, line: [P; 2]) -> P {
    let v = sub(line[1], line[0]);
    let denominator = dot(v, v);
    if denominator < 1e-14 {
        line[0]
    } else {
        add(
            line[0],
            scale(v, (dot(sub(point, line[0]), v) / denominator).clamp(0., 1.)),
        )
    }
}
pub fn point_line(point: P, line: [P; 2], position: P, view_scale: f64) -> Option<Linear> {
    let direction = unit(sub(line[1], line[0]))?;
    let foot = add(
        line[0],
        scale(direction, dot(sub(point, line[0]), direction)),
    );
    let value = length(sub(foot, point)) / view_scale;
    if value < 1e-7 {
        return None;
    }
    let offset = scale(direction, dot(sub(position, point), direction));
    Some(Linear {
        first: point,
        second: foot,
        start: add(point, offset),
        end: add(foot, offset),
        value,
    })
}
pub fn line_dimension(
    first: [P; 2],
    second: Option<[P; 2]>,
    mode: DrawingLineDimensionMode,
    position: P,
    view_scale: f64,
) -> Option<LineDimension> {
    let direction = unit(sub(first[1], first[0]))?;
    let mid = midpoint(first[0], first[1]);
    if mode == DrawingLineDimensionMode::Length {
        let normal = normal(direction);
        let offset = scale(normal, dot(sub(position, mid), normal));
        return Some(LineDimension::Linear(Linear {
            first: first[0],
            second: first[1],
            start: add(first[0], offset),
            end: add(first[1], offset),
            value: length(sub(first[1], first[0])) / view_scale,
        }));
    }
    let second = second?;
    let other = unit(sub(second[1], second[0]))?;
    let parallel_tolerance = 1_f64.to_radians().sin();
    if mode == DrawingLineDimensionMode::Distance {
        if cross(direction, other).abs() > parallel_tolerance {
            return None;
        }
        let normal = normal(direction);
        let separation = dot(sub(midpoint(second[0], second[1]), mid), normal);
        if separation.abs() < 1e-7 {
            return None;
        }
        let start = add(mid, scale(direction, dot(sub(position, mid), direction)));
        let end = add(start, scale(normal, separation));
        return Some(LineDimension::Linear(Linear {
            first: closest(start, first),
            second: closest(end, second),
            start,
            end,
            value: separation.abs() / view_scale,
        }));
    }
    let denominator = cross(direction, other);
    if denominator.abs() <= parallel_tolerance {
        return None;
    }
    let vertex = add(
        first[0],
        scale(
            direction,
            cross(sub(second[0], first[0]), other) / denominator,
        ),
    );
    let toward = sub(position, vertex);
    let a = scale(
        direction,
        if dot(toward, direction) < 0. { -1. } else { 1. },
    );
    let b = scale(other, if dot(toward, other) < 0. { -1. } else { 1. });
    angular(
        vertex,
        add(vertex, a),
        add(vertex, b),
        length(toward).max(4.),
    )
    .map(LineDimension::Angular)
}

/// Projected bounds are centered on the saved paper-space view position.
pub fn paper_point(
    view: &limo_cad_sketch::DrawingViewDto,
    point: P,
    projection: &crate::DrawingProjectionDto,
) -> P {
    let b = projection.bounds;
    [
        view.position[0] + (point[0] - (b[0] + b[2]) * 0.5) * view.scale,
        view.position[1] - (point[1] - (b[1] + b[3]) * 0.5) * view.scale,
    ]
}
