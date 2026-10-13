//! Browser ordering is a permutation of existing canonical records. It never
//! moves an operation across setups, modifies a generation stamp or regenerates.
use super::*;
use std::collections::HashSet;

pub(super) fn position(
    cam: &CamDocumentDto,
    selection: Selection,
) -> Result<(Option<u64>, usize, usize), String> {
    match selection {
        Selection::Setup(id) => cam
            .setups
            .iter()
            .position(|setup| setup.id == id)
            .map(|index| (None, index, cam.setups.len()))
            .ok_or("Setup was removed".into()),
        Selection::Operation(id) => cam
            .setups
            .iter()
            .find_map(|setup| {
                setup
                    .operations
                    .iter()
                    .position(|operation| operation.id() == id)
                    .map(|index| (Some(setup.id), index, setup.operations.len()))
            })
            .ok_or("Toolpath was removed".into()),
        Selection::Tool(_) => Err("Select a setup or toolpath to reorder".into()),
    }
}

fn destination(index: usize, len: usize, delta: i32) -> Option<usize> {
    if !matches!(delta, -1 | 1) {
        return None;
    }
    index
        .checked_add_signed(delta as isize)
        .filter(|index| *index < len)
}

/// Presentation-only boundary check: never clone geometry while painting buttons.
pub(super) fn can_step(cam: &CamDocumentDto, selection: Selection, delta: i32) -> bool {
    position(cam, selection).is_ok_and(|(_, index, len)| destination(index, len, delta).is_some())
}

pub(super) fn step(
    cam: &CamDocumentDto,
    selection: Selection,
    delta: i32,
) -> Result<CamDocumentDto, String> {
    let (scope, index, len) = position(cam, selection)?;
    let target =
        destination(index, len, delta).ok_or("The item cannot move farther in this list")?;
    let mut ids = match scope {
        None => cam.setups.iter().map(|setup| setup.id).collect::<Vec<_>>(),
        Some(id) => cam
            .setup(id)
            .unwrap()
            .operations
            .iter()
            .map(CamOperationDto::id)
            .collect(),
    };
    ids.swap(index, target);
    apply_order(cam, scope, &ids)
}

/// Reordering requires a complete, unique
/// permutation in one scope. Shared DTO validation owns rest-stock dependencies.
pub(super) fn apply_order(
    cam: &CamDocumentDto,
    setup_id: Option<u64>,
    ids: &[u64],
) -> Result<CamDocumentDto, String> {
    let current = match setup_id {
        None => cam.setups.iter().map(|setup| setup.id).collect::<Vec<_>>(),
        Some(id) => cam
            .setup(id)
            .ok_or("Setup was removed")?
            .operations
            .iter()
            .map(CamOperationDto::id)
            .collect(),
    };
    let unique: HashSet<_> = ids.iter().copied().collect();
    if ids.len() != current.len()
        || unique.len() != current.len()
        || current.iter().any(|id| !unique.contains(id))
    {
        return Err("CAM list changed; choose the order from refreshed controls".into());
    }
    let slots: HashMap<_, _> = ids
        .iter()
        .copied()
        .enumerate()
        .map(|(index, id)| (id, index))
        .collect();
    let mut next = cam.clone();
    match setup_id {
        None => next.setups.sort_by_key(|setup| slots[&setup.id]),
        Some(id) => next
            .setups
            .iter_mut()
            .find(|setup| setup.id == id)
            .unwrap()
            .operations
            .sort_by_key(|operation| slots[&operation.id()]),
    }
    next.validate_for_editing()?;
    Ok(next)
}
