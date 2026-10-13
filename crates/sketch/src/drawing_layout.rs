//! View placement rules shared by drawing drafts and committed commands.
use crate::drawing::*;

fn root(sheet: &DrawingSheetDto, mut id: u64) -> Option<u64> {
    let mut visited = std::collections::HashSet::new();
    while visited.insert(id) {
        let view = sheet.views.iter().find(|v| v.id == id)?;
        if let Some(parent) = view.parent_view_id {
            if sheet.views.iter().any(|v| v.id == parent) {
                id = parent;
                continue;
            }
        }
        return Some(id);
    }
    None
}

/// Replace one view, preserving aligned children and optionally rescaling its group.
pub fn update_drawing_view(
    sheet: &mut DrawingSheetDto,
    mut edited: DrawingViewDto,
    rescale_group: bool,
) -> Result<(), String> {
    let index = sheet
        .views
        .iter()
        .position(|v| v.id == edited.id)
        .ok_or("View was removed")?;
    let before = sheet.views[index].position;
    if edited.position != before {
        if let Some(parent) = edited
            .parent_view_id
            .and_then(|id| sheet.views.iter().find(|v| v.id == id))
        {
            match edited.alignment {
                DrawingViewAlignment::Horizontal => edited.position[1] = parent.position[1],
                DrawingViewAlignment::Vertical => edited.position[0] = parent.position[0],
                DrawingViewAlignment::Free => (),
            }
        }
        let delta = [
            edited.position[0] - before[0],
            edited.position[1] - before[1],
        ];
        for child in sheet
            .views
            .iter_mut()
            .filter(|v| v.parent_view_id == Some(edited.id))
        {
            match child.alignment {
                DrawingViewAlignment::Horizontal => child.position[1] += delta[1],
                DrawingViewAlignment::Vertical => child.position[0] += delta[0],
                DrawingViewAlignment::Free => (),
            }
        }
    }
    let id = edited.id;
    let scale = edited.scale;
    sheet.views[index] = edited;
    if rescale_group {
        let root_id = root(sheet, id).ok_or("Drawing view group has a cycle")?;
        let members: std::collections::HashSet<_> = sheet
            .views
            .iter()
            .filter(|v| root(sheet, v.id) == Some(root_id))
            .map(|v| v.id)
            .collect();
        for member in &mut sheet.views {
            if members.contains(&member.id) {
                member.scale = scale;
            }
        }
    }
    Ok(())
}
