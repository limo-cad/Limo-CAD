//! Overview and exact authored-source navigation over shared inspection data.
use super::super::super::chrome::{self, rect};
use super::*;
use crate::native_viewport::interface_shell::fields;
use limo_cad_interface::{ControlKey, Field};
use std::ops::Range;

const PAGE: usize = 6;
#[derive(Default)]
pub(crate) struct State {
    pub open: bool,
    selected: usize,
    page: usize,
    pending: Option<(u64, Range<usize>)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Open,
    Close,
    Select(u64, usize),
    Page(u64, usize),
    Source(u64, usize),
}
fn items(loaded: &Loaded) -> &[Value] {
    loaded.inspection["chapters"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
fn source_range(loaded: &Loaded, index: usize) -> Option<Range<usize>> {
    let step = items(loaded).get(index)?.get("step_index")?.as_u64()?;
    loaded.inspection["authored_chapters"]
        .as_array()?
        .iter()
        .find_map(|entry| {
            (entry["step_index"].as_u64() == Some(step))
                .then(|| {
                    Some(
                        usize::try_from(entry["start"].as_u64()?).ok()?
                            ..usize::try_from(entry["end"].as_u64()?).ok()?,
                    )
                })
                .flatten()
        })
}
fn heading(note: &Value) -> String {
    let title = note["chapter"]
        .as_str()
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| note["text"].as_str().unwrap_or("Note"));
    format!(
        "Step {}: {}",
        note["step_index"],
        title.lines().next().unwrap_or("Note")
    )
}
pub(crate) fn command(world: &mut World, action: Action) -> Result<Value, String> {
    available(world)?;
    editor::source_ready(world)?;
    let state = &mut world.resource_mut::<Files>().script;
    if action == Action::Close {
        state.chapters.open = false;
        return Ok(json!({"chapters_open":false}));
    }
    let generation = match action {
        Action::Select(g, _) | Action::Page(g, _) | Action::Source(g, _) => g,
        _ => state.generation,
    };
    let loaded = state.selected(generation)?;
    let notes = items(&loaded);
    if notes.is_empty() {
        return Err("This script has no chapter notes".into());
    }
    match action {
        Action::Open => {
            state.chapters.open = true;
            state.library.open = false;
            state.chapters.selected = state.chapters.selected.min(notes.len() - 1);
            state.chapters.page = state.chapters.selected / PAGE;
        }
        Action::Select(_, index) => {
            if index >= notes.len() {
                return Err("Script chapter changed".into());
            }
            state.chapters.selected = index;
        }
        Action::Page(_, page) => {
            if page * PAGE >= notes.len() {
                return Err("Script chapter page changed".into());
            }
            state.chapters.page = page;
            state.chapters.selected = page * PAGE;
        }
        Action::Source(_, index) => {
            let range = source_range(&loaded, index).ok_or(
                "This chapter is in an included file; open that file separately to edit its source",
            )?;
            state.chapters.open = false;
            state.editor_open = true;
            state.chapters.pending = Some((generation, range));
        }
        Action::Close => unreachable!(),
    }
    Ok(json!({"chapters_open":state.chapters.open}))
}

pub(super) fn poll(world: &mut World) {
    let files = world.resource::<Files>();
    let state = &files.script;
    let Some((generation, range)) = state.chapters.pending.clone() else {
        return;
    };
    if !files.scripts || !state.editor_open || state.chapters.open {
        return;
    }
    if state.generation != generation {
        world.resource_mut::<Files>().script.chapters.pending = None;
        return;
    }
    let Some(entity) = state.source_entity else {
        return;
    };
    let source = state.source.clone();
    let Some(handle) = world.get_resource::<NativeInterfaceHandle>().cloned() else {
        return;
    };
    if world
        .get::<InterfaceControl>(entity)
        .is_none_or(|c| !c.visible || c.disabled)
        || handle
            .resolve_retained(ControlKey(entity.to_bits()))
            .is_err()
        || world
            .get::<bevy::text::EditableText>(entity)
            .is_none_or(|text| text.viewport.size.y <= 0.)
    {
        return;
    }
    let result = fields::select_reveal(world, &handle, entity, &source, range);
    let state = &mut world.resource_mut::<Files>().script;
    state.chapters.pending = None;
    if let Err(error) = result {
        state.status = Some(error);
    }
}

pub(crate) fn paint(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    width: f32,
    height: f32,
    theme: ViewportUiTheme,
    busy: bool,
) -> Result<(), String> {
    let state = &world.resource::<Files>().script;
    let loaded = state.loaded.clone().ok_or("Script source was removed")?;
    let generation = state.generation;
    let index = state.chapters.selected;
    let page = state.chapters.page;
    let notes = items(&loaded);
    let note = notes.get(index).ok_or("Script chapter was removed")?;
    let can_reveal = state.selected(generation).is_ok() && source_range(&loaded, index).is_some();
    let w = (width - 40.).clamp(280., 680.);
    let h = (height - 80.).clamp(420., 780.);
    let x = (width - w - 20.).max(4.);
    let y = 32.;
    widgets.panel(
        world,
        camera,
        "scripts-card",
        rect(x, y, w, h),
        theme.panel.with_alpha(1.),
        60,
    );
    widgets.text(
        world,
        camera,
        "scripts-chapters-heading",
        rect(x + 12., y + 8., w - 24., 24.),
        &format!("Chapter overview — {} notes", notes.len()),
        15.,
        61,
    );
    for (row, item) in notes.iter().enumerate().skip(page * PAGE).take(PAGE) {
        let label = heading(item);
        let mut control = InterfaceControl::button("document/scripts", &label);
        control.disabled = busy;
        control.selected = Some(row == index);
        widgets.button(
            world,
            camera,
            &format!("scripts-chapter-{}", row % PAGE),
            control,
            Some(&label),
            NativeCommand::File(FileCommand::ScriptChapter(Action::Select(generation, row))),
            rect(x + 12., y + 42. + (row % PAGE) as f32 * 28., w - 24., 24.),
            None,
            61,
        )?;
    }
    let note_text = format!(
        "{}\n\n{}\n\n{}",
        heading(note),
        note["text"].as_str().unwrap_or(""),
        if can_reveal {
            "Show in source selects this note in the authored file."
        } else {
            "Included chapter. Its text is retained from validation; open the included file separately to edit it."
        }
    );
    let mut control = InterfaceControl::button("document/scripts", "Selected chapter note");
    control.field = Field::Text {
        value: note_text,
        selection: None,
        read_only: true,
    };
    let entity = widgets.button(
        world,
        camera,
        "scripts-chapter-text",
        control,
        None,
        NativeCommand::File(FileCommand::ScriptChapter(Action::Select(
            generation, index,
        ))),
        rect(x + 12., y + 250., w - 24., h - 300.),
        None,
        61,
    )?;
    fields::multiline::enable(world, entity)?;
    let half = (w - 30.) / 2.;
    for (key, label, action, bounds, disabled) in [
        (
            "scripts-chapter-prev",
            "Previous chapters",
            Action::Page(generation, page.saturating_sub(1)),
            rect(x + 12., y + 214., half, 26.),
            busy || page == 0,
        ),
        (
            "scripts-chapter-next",
            "Next chapters",
            Action::Page(generation, page + 1),
            rect(x + 18. + half, y + 214., half, 26.),
            busy || (page + 1) * PAGE >= notes.len(),
        ),
        (
            "scripts-chapter-back",
            "Back to source",
            Action::Close,
            rect(x + 12., y + h - 38., half, 26.),
            busy,
        ),
        (
            "scripts-chapter-source",
            "Show in source",
            Action::Source(generation, index),
            rect(x + 18. + half, y + h - 38., half, 26.),
            busy || !can_reveal,
        ),
    ] {
        let mut control = InterfaceControl::button("document/scripts", label);
        control.disabled = disabled;
        widgets.button(
            world,
            camera,
            key,
            control,
            Some(label),
            NativeCommand::File(FileCommand::ScriptChapter(action)),
            bounds,
            None,
            61,
        )?;
    }
    Ok(())
}
