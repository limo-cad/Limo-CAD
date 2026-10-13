//! Shared paper-space clearance. The 1 mm gap is the same offset
//! `centers::caption_baseline` already adds past neighboring ink.
use super::text;

pub const DIMENSION_OFFSET_MM: f64 = 6.;
pub const EXTENSION_PAST_MM: f64 = 1.2;
pub const GAP_MM: f64 = 1.;
const DEFAULT_DIMENSION_STROKE_MM: f64 = 0.25;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Motion {
    #[default]
    Fixed,
    Weld,
    Cloud,
    Table,
}

#[derive(Clone, Copy, Debug)]
pub struct Obstacle {
    pub bounds: [f64; 4],
    pub motion: Motion,
    pub group: u32,
}

/// Baseline whose label box clears a dimension placed 6 mm from the view.
/// `further_ink` is additional dimension ink already measured in paper mm.
pub fn view_caption_baseline(
    center_y: f64,
    paper_height: f64,
    text_height: f64,
    further_ink: Option<f64>,
) -> f64 {
    let height = paper_height.abs();
    let existing = center_y - height * 0.5 + height.max(1.) + 5.;
    let view_bottom = center_y + height * 0.5;
    let reserved = view_bottom + DIMENSION_OFFSET_MM;
    let mut obstacle = dimension_ink_y(reserved, reserved, DEFAULT_DIMENSION_STROKE_MM, true);
    if let Some(ink) = further_ink.filter(|value| value.is_finite()) {
        obstacle = obstacle.max(ink);
    }
    let top = text::label_bounds([0., existing], "", text_height, 0.)[1];
    let needed = obstacle + GAP_MM;
    if top.is_finite() && top < needed {
        existing + (needed - top)
    } else {
        existing
    }
}

/// Lowest paper Y of a dimension line, including the stroke and the 1.2 mm
/// extension the linear layout already draws past the line.
pub fn dimension_ink_y(start_y: f64, end_y: f64, stroke: f64, extend_down: bool) -> f64 {
    let mut ink = start_y.max(end_y) + stroke.abs() * 0.5;
    if extend_down {
        ink += EXTENSION_PAST_MM;
    }
    ink
}

/// One paper shift per obstacle. Movable groups yield to fixed text and to
/// lower group ids by the smallest move that restores `GAP_MM`.
pub fn clearance_deltas(items: &[Obstacle]) -> Vec<[f64; 2]> {
    let mut deltas = vec![[0.; 2]; items.len()];
    for _ in 0..12 {
        let mut moved = false;
        let mut groups = Vec::new();
        for item in items {
            if item.motion != Motion::Fixed && !groups.contains(&item.group) {
                groups.push(item.group);
            }
        }
        groups.sort_unstable();
        for group in groups {
            let Some(mut bounds) = union(items, &deltas, group) else {
                continue;
            };
            let mut shift = [0., 0.];
            for (index, other) in items.iter().enumerate() {
                if other.group == group || (other.motion != Motion::Fixed && other.group > group) {
                    continue;
                }
                let obstacle = translate(other.bounds, deltas[index]);
                if let Some(step) = separate(bounds, obstacle) {
                    bounds = translate(bounds, step);
                    shift = [shift[0] + step[0], shift[1] + step[1]];
                }
            }
            if !shift[0].is_finite()
                || !shift[1].is_finite()
                || shift[0].abs() > 400.
                || shift[1].abs() > 400.
                || (shift[0].abs() < 1e-6 && shift[1].abs() < 1e-6)
            {
                continue;
            }
            for (index, item) in items.iter().enumerate() {
                if item.group == group {
                    deltas[index][0] += shift[0];
                    deltas[index][1] += shift[1];
                }
            }
            moved = true;
        }
        if !moved {
            break;
        }
    }
    deltas
}

fn union(items: &[Obstacle], deltas: &[[f64; 2]], group: u32) -> Option<[f64; 4]> {
    let mut acc: Option<[f64; 4]> = None;
    for (index, item) in items.iter().enumerate() {
        if item.group != group || item.motion == Motion::Fixed {
            continue;
        }
        let bounds = translate(item.bounds, deltas[index]);
        if bounds.into_iter().any(|value| !value.is_finite()) {
            return None;
        }
        acc = Some(match acc {
            None => bounds,
            Some(prior) => [
                prior[0].min(bounds[0]),
                prior[1].min(bounds[1]),
                prior[2].max(bounds[2]),
                prior[3].max(bounds[3]),
            ],
        });
    }
    acc
}

fn translate(bounds: [f64; 4], delta: [f64; 2]) -> [f64; 4] {
    [
        bounds[0] + delta[0],
        bounds[1] + delta[1],
        bounds[2] + delta[0],
        bounds[3] + delta[1],
    ]
}

fn separate(mover: [f64; 4], obstacle: [f64; 4]) -> Option<[f64; 2]> {
    let left = obstacle[0] - GAP_MM;
    let top = obstacle[1] - GAP_MM;
    let right = obstacle[2] + GAP_MM;
    let bottom = obstacle[3] + GAP_MM;
    if mover[2] <= left || mover[0] >= right || mover[3] <= top || mover[1] >= bottom {
        return None;
    }
    let candidates = [
        [left - mover[2], 0.],
        [right - mover[0], 0.],
        [0., top - mover[3]],
        [0., bottom - mover[1]],
    ];
    candidates.into_iter().min_by(|a, b| {
        (a[0].abs() + a[1].abs())
            .partial_cmp(&(b[0].abs() + b[1].abs()))
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}
