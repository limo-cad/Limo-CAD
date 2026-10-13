//! Typed viewport choices stage only the existing setup form fields.
use super::*;
use std::hash::{Hash, Hasher};

pub(in super::super) const BUTTON: &str = "/native/setup/pick_origin";
const MAX_POINTS: usize = 4096;
const MAX_BODIES: usize = 2048;
const MAX_TEXT: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in super::super) enum Anchor {
    Min,
    Center,
    Max,
}
impl Anchor {
    fn text(self) -> &'static str {
        match self {
            Self::Min => "min",
            Self::Center => "center",
            Self::Max => "max",
        }
    }
    fn coordinate(self, min: f64, max: f64) -> f64 {
        match self {
            Self::Min => min,
            Self::Center => min * 0.5 + max * 0.5,
            Self::Max => max,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in super::super) enum Mode {
    Stock,
    Model,
    Sketch,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super) enum Key {
    Box { mode: Mode, axes: [Anchor; 3] },
    Sketch { sketch: String, entity_id: u64 },
}
#[derive(Clone, Debug)]
pub(in super::super) struct Candidate {
    pub key: Key,
    pub point: [f64; 3],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super) struct SelectionState {
    pub selection: Selection,
    pub mode: Mode,
    epoch: u64,
}
pub(in super::super) enum Source {
    Box(Mode, StockBoxDto),
    Sketch(Vec<Candidate>),
}
fn mode(draft: &Draft) -> Result<Mode, String> {
    match text(draft, "origin")? {
        "stock_box_point" => Ok(Mode::Stock),
        "model_box_point" => Ok(Mode::Model),
        "sketch_point" => Ok(Mode::Sketch),
        _ => Err("Choose a stock box, model box, or sketch point origin".into()),
    }
}
pub(in super::super) fn snapshot(draft: &Draft) -> Result<SelectionState, String> {
    if !matches!(draft.selection, Selection::Setup(id) if id != 0)
        || !visible(draft, BUTTON)
        || !machine::visible(draft, BUTTON)
    {
        return Err("Open the setup WCS origin fields before picking".into());
    }
    let context = draft.setup.as_ref().ok_or("Reopen the setup editor")?;
    if !context.model_valid {
        return Err("Resolve model errors before picking the WCS origin".into());
    }
    if context.points.len() > MAX_POINTS
        || context.bodies.len() > MAX_BODIES
        || draft.fields.len() > 8192
    {
        return Err("WCS viewport picking supports at most 4096 sketch points and 2048 bodies; use the existing origin fields".into());
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (std::sync::Arc::as_ptr(&context.stamp) as usize).hash(&mut hash);
    let mut bytes = 0usize;
    for field in &draft.fields {
        if field.path.starts_with(PREFIX) || field.path == "/native/ui/setup_section" {
            bytes = bytes
                .saturating_add(field.path.len())
                .saturating_add(field.text.len());
            if bytes > MAX_TEXT {
                return Err("WCS form exceeds the viewport-picking text budget; use the existing origin fields".into());
            }
            field.path.hash(&mut hash);
            field.text.hash(&mut hash);
        }
    }
    context
        .points
        .iter()
        .try_fold(bytes, |sum, (key, sketch, _, _)| {
            let sum = sum.saturating_add(key.len()).saturating_add(sketch.len());
            (sum <= MAX_TEXT)
                .then_some(sum)
                .ok_or("WCS point names exceed the viewport-picking text budget")
        })?;
    Ok(SelectionState {
        selection: draft.selection,
        mode: mode(draft)?,
        epoch: hash.finish(),
    })
}
pub(in super::super) fn source(
    draft: &Draft,
    cam: &CamDocumentDto,
    expected: &SelectionState,
) -> Result<Source, String> {
    if snapshot(draft)? != *expected {
        return Err("WCS draft changed; start picking again".into());
    }
    if expected.mode == Mode::Sketch {
        let context = draft.setup.as_ref().ok_or("Reopen the setup editor")?;
        return Ok(Source::Sketch(
            context
                .points
                .iter()
                .map(|(_, sketch, entity_id, p)| Candidate {
                    key: Key::Sketch {
                        sketch: sketch.clone(),
                        entity_id: *entity_id,
                    },
                    point: [p.x, p.y, p.z],
                })
                .collect(),
        ));
    }
    let bounds = if expected.mode == Mode::Stock {
        resolved_stock(draft, cam)?.model_box
    } else {
        included_model(draft)?.1
    };
    Ok(Source::Box(expected.mode, bounds))
}
pub(in super::super) fn candidates(source: Source) -> Result<Vec<Candidate>, String> {
    let candidates = match source {
        Source::Sketch(points) => points,
        Source::Box(mode, b) => {
            b.validate()?;
            let mut points = Vec::with_capacity(27);
            for x in [Anchor::Min, Anchor::Center, Anchor::Max] {
                for y in [Anchor::Min, Anchor::Center, Anchor::Max] {
                    for z in [Anchor::Min, Anchor::Center, Anchor::Max] {
                        points.push(Candidate {
                            key: Key::Box {
                                mode,
                                axes: [x, y, z],
                            },
                            point: [
                                x.coordinate(b.min.x, b.max.x),
                                y.coordinate(b.min.y, b.max.y),
                                z.coordinate(b.min.z, b.max.z),
                            ],
                        });
                    }
                }
            }
            points
        }
    };
    if candidates.is_empty() {
        return Err("Add a standalone sketch point before picking the WCS origin".into());
    }
    if candidates.len() > MAX_POINTS
        || candidates
            .iter()
            .flat_map(|p| p.point)
            .any(|v| !v.is_finite() || !(v as f32).is_finite())
    {
        return Err("WCS point preview exceeds its finite coordinate or count budget".into());
    }
    Ok(candidates)
}
pub(in super::super) fn stage(
    draft: &mut Draft,
    expected: &SelectionState,
    key: &Key,
) -> Result<(), String> {
    if snapshot(draft)? != *expected {
        return Err("WCS draft changed; start picking again".into());
    }
    match key {
        Key::Box { mode, axes } if *mode == expected.mode && *mode != Mode::Sketch => {
            for (axis, anchor) in ["x", "y", "z"].into_iter().zip(axes) {
                form::set(draft, &format!("{PREFIX}anchor/{axis}"), anchor.text());
            }
        }
        Key::Sketch { sketch, entity_id } if expected.mode == Mode::Sketch => {
            let context = draft.setup.as_ref().ok_or("Reopen the setup editor")?;
            let (key, _, _, _) = context
                .points
                .iter()
                .find(|(_, name, id, p)| {
                    name == sketch
                        && id == entity_id
                        && [p.x, p.y, p.z].iter().all(|v| v.is_finite())
                })
                .ok_or("Picked sketch point is unavailable; start picking again")?;
            let key = key.clone();
            form::set(draft, &format!("{PREFIX}point"), &key);
        }
        _ => return Err("WCS origin mode changed; start picking again".into()),
    }
    Ok(())
}
