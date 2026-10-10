//! A duplicate stays an unsaved tool form until Create. Its immutable source
//! record preserves precision and the project's original cutter is untouched.
use super::*;

pub(in super::super) fn copy_tool(cam: &CamDocumentDto, id: u64) -> Result<Draft, String> {
    let source = cam.tool(id).ok_or("The project tool was removed")?;
    let mut draft = Draft::new(cam, Selection::Tool(id))?;
    draft.copied_tool = true;
    form::set(&mut draft, "/name", &format!("{} copy", source.name));
    let number = cam
        .tools
        .iter()
        .filter_map(|tool| tool.number)
        .max()
        .unwrap_or(0)
        .checked_add(1);
    form::set(
        &mut draft,
        "/number",
        &number.map_or(String::new(), |value| value.to_string()),
    );
    Ok(draft)
}

pub(in super::super) fn create_copy(
    draft: &Draft,
    cam: &CamDocumentDto,
) -> Result<(CamDocumentDto, Selection), String> {
    if !draft.copied_tool {
        return Err("Open a copied tool form".into());
    }
    let Selection::Tool(source) = draft.selection else {
        return Err("Copy a project tool".into());
    };
    let edited = draft.edited_without_validation(cam)?;
    let mut tool = edited
        .tool(source)
        .ok_or("The source project tool was removed")?
        .clone();
    tool.id = next_id(cam.next_tool_id, cam.tools.iter().map(|tool| tool.id))?;
    let selected = Selection::Tool(tool.id);
    let mut next = cam.clone();
    next.next_tool_id = tool.id + 1;
    next.tools.push(tool);
    next.validate_for_editing()?;
    Ok((next, selected))
}
