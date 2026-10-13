//! Viewport results use the existing exact-coordinate candidate chooser.
use super::*;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in super::super::super) enum Key {
    Drill { operation: u64, index: usize },
    Vertex { body: u64, index: usize },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in super::super::super) enum Target {
    Predrill,
    Entry,
    Exit,
}
impl Target {
    pub fn key(self) -> &'static str {
        match self {
            Self::Predrill => "predrill_positions",
            Self::Entry => "entry_positions",
            Self::Exit => "exit_positions",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super::super) struct SelectionState {
    pub selection: Selection,
    pub target: Target,
    epoch: u64,
}
#[derive(Clone, Debug)]
pub(in super::super::super) struct Candidate {
    pub key: Key,
    pub point: [f64; 3],
}
pub(in super::super::super) fn button(key: &str) -> String {
    format!("{}/pick", prefix(key))
}
fn target(path: &str) -> Option<Target> {
    [Target::Predrill, Target::Entry, Target::Exit]
        .into_iter()
        .find(|target| path == button(target.key()))
}
pub(in super::super::super) fn is_button(path: &str) -> bool {
    target(path).is_some()
}
pub(in super::super::super) fn snapshot(
    draft: &Draft,
    path: &str,
) -> Result<SelectionState, String> {
    let target = target(path).ok_or("Choose a linking point picker")?;
    if !matches!(draft.selection, Selection::Operation(_))
        || form::text(draft, "/native/ui/operation_section")? != "linking"
        || !visible(draft, path)
    {
        return Err("Open the operation's custom linking points before picking".into());
    }
    let context = &draft
        .operation_edit
        .as_ref()
        .ok_or("Reopen the operation editor")?
        .linking_points;
    if !context.model_valid {
        return Err("Resolve model errors before picking linking points".into());
    }
    if context.source_visits > 131_072
        || context.candidates(target.key()).len() > 4096
        || draft.fields.len() > 32_768
    {
        return Err("Linking viewport picking exceeds its source or point budget; use the existing coordinate fields".into());
    }
    let count = count(draft, target.key())?;
    if target == Target::Predrill && count == 32 {
        return Err("Use at most 32 predrill positions; remove a row before appending".into());
    }
    if count > 0 && points::selected(draft, &prefix(target.key())).is_none() {
        return Err("Choose a current linking position row".into());
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (std::sync::Arc::as_ptr(&context.stamp) as usize).hash(&mut hash);
    let mut bytes = 0usize;
    for field in &draft.fields {
        if field.path.starts_with("/native/linking/")
            || handles(&field.path)
            || field.path == "/native/ui/operation_section"
        {
            bytes = bytes
                .saturating_add(field.path.len())
                .saturating_add(field.text.len());
            if bytes > 1024 * 1024 {
                return Err("Linking form exceeds the viewport-picking text budget; use the existing coordinate fields".into());
            }
            field.path.hash(&mut hash);
            field.text.hash(&mut hash);
        }
    }
    for candidate in context.candidates(target.key()) {
        bytes = bytes
            .saturating_add(candidate.key.len())
            .saturating_add(candidate.label.len());
        if bytes > 1024 * 1024 {
            return Err("Linking candidate labels exceed the viewport-picking text budget".into());
        }
    }
    Ok(SelectionState {
        selection: draft.selection,
        target,
        epoch: hash.finish(),
    })
}
pub(in super::super::super) fn candidates(
    draft: &Draft,
    expected: &SelectionState,
) -> Result<Vec<Candidate>, String> {
    if snapshot(draft, &button(expected.target.key()))? != *expected {
        return Err("Linking draft changed; start picking again".into());
    }
    let context = &draft
        .operation_edit
        .as_ref()
        .ok_or("Reopen the operation editor")?
        .linking_points;
    let values = context.candidates(expected.target.key());
    if values.is_empty() {
        return Err(if expected.target == Target::Predrill {
            "There are no earlier enabled drill centers in this setup"
        } else {
            "There are no included model vertices to pick"
        }
        .into());
    }
    if values
        .iter()
        .flat_map(|candidate| candidate.world)
        .any(|value| !value.is_finite() || !(value as f32).is_finite())
    {
        return Err("Linking preview exceeds the renderer's finite coordinate range".into());
    }
    Ok(values
        .iter()
        .map(|candidate| Candidate {
            key: candidate.identity,
            point: candidate.world,
        })
        .collect())
}
pub(in super::super::super) fn stage(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    expected: &SelectionState,
    key: Key,
) -> Result<(), String> {
    if snapshot(draft, &button(expected.target.key()))? != *expected {
        return Err("Linking draft changed; start picking again".into());
    }
    let collection = expected.target.key();
    let context = &draft
        .operation_edit
        .as_ref()
        .ok_or("Reopen the operation editor")?
        .linking_points;
    let reference = context
        .candidates(collection)
        .iter()
        .find(|candidate| candidate.identity == key)
        .ok_or("Picked linking candidate is unavailable; start picking again")?
        .key
        .clone();
    let index = if expected.target == Target::Predrill {
        count(draft, collection)?
    } else {
        0
    };
    let prefix = prefix(collection);
    let count_path = format!("{prefix}/count");
    form::set(draft, &count_path, &(index + 1).to_string());
    form::set(draft, &points::cursor(&prefix), &(index + 1).to_string());
    operation_editor::changed(draft, cam, &count_path)?;
    let path = format!("{prefix}/{index}/candidate");
    form::set(draft, &path, &reference);
    operation_editor::changed(draft, cam, &path)
}
