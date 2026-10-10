//! Named cutting profiles are canonical tool DTO snapshots. Row navigation is
//! transient; selecting a profile for an operation copies only cutting data.
use super::*;
use limo_cad_cam::{CamCuttingPresetDto, CamUnits, CuttingParametersDto};
use std::collections::BTreeSet;

const PREFIX: &str = "/native/presets/";
const MARK: &str = "/native/presets/changed";
const SECTION: &str = "/native/ui/tool_section";
const CURRENT: &str = "/native/ui/cutting_profile";
const OP_CHOICE: &str = "/native/ui/apply_cutting_profile";
const OP_COPY: &str = "/native/cutting/copied_profile";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Add,
    Duplicate,
    Remove,
}

pub(super) struct Context {
    rows: Vec<CamCuttingPresetDto>,
    removed: BTreeSet<usize>,
    changes: u64,
}
fn prefix(index: usize) -> String {
    format!("{PREFIX}{index}/")
}
fn current(draft: &Draft) -> Option<usize> {
    let index = form::text(draft, CURRENT).ok()?.parse().ok()?;
    let context = draft.presets.as_ref()?;
    (index < context.rows.len() && !context.removed.contains(&index)).then_some(index)
}
fn name(draft: &Draft, index: usize, source: &CamCuttingPresetDto) -> String {
    form::text(draft, &format!("{}name", prefix(index)))
        .unwrap_or(&source.name)
        .to_owned()
}
fn extend_row(draft: &mut Draft, units: CamUnits) {
    let Some(index) = current(draft) else {
        return;
    };
    let base = prefix(index);
    if draft
        .fields
        .iter()
        .any(|field| field.path == format!("{base}name"))
    {
        return;
    }
    let source = serde_json::to_value(&draft.presets.as_ref().unwrap().rows[index]).unwrap();
    for (path, label, kind) in [
        ("name", "Preset name", InputKind::Name),
        (
            "cutting/spindle_rpm",
            "Preset spindle (rpm)",
            InputKind::Integer,
        ),
        ("cutting/feed_xy", "Preset cutting feed", InputKind::Feed),
        ("cutting/feed_z", "Preset plunge feed", InputKind::Feed),
        ("cutting/coolant", "Preset coolant", InputKind::Choice),
    ] {
        form::push(
            draft,
            &format!("{base}{path}"),
            label,
            kind,
            source.pointer(&format!("/{path}")).unwrap().clone(),
            units,
            (path == "cutting/coolant")
                .then(|| form::options(&[("off", "Off"), ("mist", "Mist"), ("flood", "Flood")])),
        );
    }
}
fn refresh(draft: &mut Draft, units: CamUnits) {
    let context = draft.presets.as_ref().unwrap();
    let options = context
        .rows
        .iter()
        .enumerate()
        .filter(|(index, _)| !context.removed.contains(index))
        .map(|(index, source)| ChoiceOption {
            value: index.to_string(),
            label: name(draft, index, source),
            disabled: false,
        })
        .collect::<Vec<_>>();
    let index = form::text(draft, CURRENT).unwrap_or("");
    if !options.iter().any(|option| option.value == index) {
        form::set(
            draft,
            CURRENT,
            options
                .first()
                .map(|option| option.value.as_str())
                .unwrap_or(""),
        );
    }
    draft
        .fields
        .iter_mut()
        .find(|field| field.path == CURRENT)
        .unwrap()
        .options = Some(options);
    extend_row(draft, units);
}
pub(super) fn extend_tool(draft: &mut Draft, units: CamUnits) -> Result<(), String> {
    let rows = serde_json::from_value(
        draft
            .record
            .get("cutting_presets")
            .cloned()
            .unwrap_or(json!([])),
    )
    .map_err(|error| error.to_string())?;
    draft.presets = Some(Context {
        rows,
        removed: BTreeSet::new(),
        changes: 0,
    });
    let at = draft.fields.len();
    form::push(
        draft,
        SECTION,
        "Tool section",
        InputKind::Choice,
        json!("tool"),
        units,
        Some(form::options(&[
            ("tool", "Cutter and defaults"),
            ("presets", "Cutting presets"),
        ])),
    );
    let section = draft.fields.remove(at);
    draft.fields.insert(0, section);
    form::push(
        draft,
        CURRENT,
        "Cutting preset",
        InputKind::Choice,
        json!(""),
        units,
        Some(vec![]),
    );
    form::push(draft, MARK, "", InputKind::Integer, json!(0), units, None);
    refresh(draft, units);
    Ok(())
}
fn read_row(draft: &Draft, index: usize, units: CamUnits) -> Result<CamCuttingPresetDto, String> {
    let original = &draft.presets.as_ref().ok_or("Reopen the tool editor")?.rows[index];
    let base = prefix(index);
    let mut record = serde_json::to_value(original).map_err(|error| error.to_string())?;
    for field in draft
        .fields
        .iter()
        .filter(|field| field.path.starts_with(&base) && field.text != field.original)
    {
        let path = format!("/{}", field.path.strip_prefix(&base).unwrap());
        let value = match field.kind {
            InputKind::Name | InputKind::Choice => json!(field.text.trim()),
            InputKind::Integer => json!(field
                .text
                .trim()
                .parse::<u32>()
                .map_err(|_| format!("Enter {}", field.label))?),
            _ => json!(form::number(draft, &field.path, units)?),
        };
        *record
            .pointer_mut(&path)
            .ok_or("Preset field is unavailable")? = value;
    }
    serde_json::from_value(record).map_err(|error| error.to_string())
}
fn default_cutting(draft: &Draft, units: CamUnits) -> Result<CuttingParametersDto, String> {
    let creating = draft.creation.is_some();
    let field = |name: &str| {
        if creating {
            format!("/{name}")
        } else {
            format!("/cutting/{name}")
        }
    };
    let rpm = form::text(draft, &field("spindle_rpm"))?
        .parse::<u32>()
        .map_err(|_| "Enter the default spindle speed before adding a preset")?;
    let feed = |name: &str| -> Result<f64, String> {
        let path = field(name);
        if !creating && !form::changed(draft, &path) {
            if let Some(value) = draft.record["cutting"][name].as_f64() {
                return Ok(value);
            }
        }
        form::number(draft, &path, units)
    };
    let xy = feed("feed_xy")?;
    let z = feed("feed_z")?;
    if rpm == 0 || xy <= 0. || z <= 0. {
        return Err("Enter positive default cutting data before adding a preset".into());
    }
    serde_json::from_value(json!({"spindle_rpm":rpm,"feed_xy":xy,"feed_z":z,"coolant":form::text(draft,"/cutting/coolant")?})).map_err(|error|error.to_string())
}
pub(super) fn changed_tool(draft: &mut Draft, units: CamUnits, path: &str) -> Result<(), String> {
    if draft.presets.is_none() {
        return Ok(());
    }
    if path == CURRENT || path.starts_with(PREFIX) {
        refresh(draft, units);
    }
    Ok(())
}
pub(super) fn edit_tool(draft: &mut Draft, units: CamUnits, action: Command) -> Result<(), String> {
    if draft.presets.is_none() {
        return Err("Open the cutting preset editor".into());
    }
    {
        let selected = current(draft);
        let candidate = match action {
            Command::Add => Some(CamCuttingPresetDto {
                name: "Preset".into(),
                cutting: default_cutting(draft, units)?,
            }),
            Command::Duplicate => Some(read_row(
                draft,
                selected.ok_or("Select a cutting preset")?,
                units,
            )?),
            Command::Remove => None,
        };
        let mut context = draft.presets.take().unwrap();
        if let Some(mut candidate) = candidate {
            if context.rows.len() >= 4096 {
                draft.presets = Some(context);
                return Err("Apply the current preset changes before adding more profiles".into());
            }
            let stem = if action == Command::Duplicate {
                format!("{} copy", candidate.name)
            } else {
                candidate.name.clone()
            };
            let taken = context
                .rows
                .iter()
                .enumerate()
                .filter(|(index, _)| !context.removed.contains(index))
                .map(|(index, preset)| name(draft, index, preset))
                .collect::<BTreeSet<_>>();
            let mut n = 1;
            candidate.name = stem.clone();
            while taken.contains(&candidate.name) {
                n += 1;
                candidate.name = format!("{stem} {n}");
            }
            form::set(draft, CURRENT, &context.rows.len().to_string());
            context.rows.push(candidate);
        } else {
            let Some(selected) = selected else {
                draft.presets = Some(context);
                return Err("Select a cutting preset".into());
            };
            context.removed.insert(selected);
        }
        context.changes += 1;
        form::set(draft, MARK, &context.changes.to_string());
        draft.presets = Some(context);
    }
    refresh(draft, units);
    Ok(())
}
pub(super) fn editing(draft: &Draft) -> bool {
    draft.presets.is_some() && form::text(draft, SECTION).unwrap_or("tool") == "presets"
}
pub(super) fn actions(
    widgets: &mut Widgets,
    world: &mut World,
    camera: Entity,
    draft: &Draft,
    width: f32,
    y: f32,
) -> Result<(), String> {
    let bw = (width - 28.) / 3.;
    let selected = current(draft).is_some();
    for (index, (label, caption, command)) in [
        ("Add preset", "Add preset", Command::Add),
        ("Copy preset", "Copy preset", Command::Duplicate),
        ("Remove preset", "Remove", Command::Remove),
    ]
    .into_iter()
    .enumerate()
    {
        let mut control = InterfaceControl::button("cam/document", label);
        control.disabled = command != Command::Add && !selected;
        widgets.button(
            world,
            camera,
            &format!("cam-preset-action-{index}"),
            control,
            Some(caption),
            NativeCommand::Cam(super::Command::Preset(command)),
            rect(10. + index as f32 * (bw + 4.), y, bw, 28.),
            None,
            46,
        )?;
    }
    Ok(())
}
pub(super) fn apply_tool(draft: &Draft, record: &mut Value, units: CamUnits) -> Result<(), String> {
    if !form::changed(draft, PREFIX) {
        return Ok(());
    }
    let context = draft.presets.as_ref().ok_or("Reopen the cutting presets")?;
    let rows = (0..context.rows.len())
        .filter(|index| !context.removed.contains(index))
        .map(|index| read_row(draft, index, units))
        .collect::<Result<Vec<_>, _>>()?;
    record["cutting_presets"] = serde_json::to_value(rows).map_err(|error| error.to_string())?;
    Ok(())
}
pub(super) fn visible_tool(draft: &Draft, path: &str) -> bool {
    if draft.presets.is_none() {
        return true;
    }
    if path == SECTION {
        return true;
    }
    if path == MARK {
        return false;
    }
    let editing = editing(draft);
    if path == CURRENT {
        return editing;
    }
    if path.starts_with(PREFIX) {
        return editing && current(draft).is_some_and(|index| path.starts_with(&prefix(index)));
    }
    !editing
}
pub(super) fn retain(previous: Option<&Draft>, next: &mut Draft, units: CamUnits) {
    if let Some(previous) = previous.filter(|previous| previous.selection == next.selection) {
        if let Ok(section) = form::text(previous, SECTION) {
            form::set(next, SECTION, section);
        }
        let prior = current(previous).and_then(|index| {
            previous
                .presets
                .as_ref()
                .map(|context| name(previous, index, &context.rows[index]))
        });
        if let Some(label) = prior {
            let value = next
                .fields
                .iter()
                .find(|field| field.path == CURRENT)
                .and_then(|field| field.options.as_ref())
                .and_then(|options| options.iter().find(|option| option.label == label))
                .map(|option| option.value.clone());
            if let Some(value) = value {
                form::set(next, CURRENT, &value);
                extend_row(next, units);
            }
        }
    }
}

pub(super) fn extend_operation(draft: &mut Draft, cam: &CamDocumentDto) {
    if !matches!(draft.selection, Selection::Operation(_)) || draft.record.is_null() {
        return;
    }
    form::push(
        draft,
        OP_CHOICE,
        "Copy tool cutting preset",
        InputKind::Choice,
        json!("keep"),
        cam.units,
        Some(vec![]),
    );
    form::push(
        draft,
        OP_COPY,
        "",
        InputKind::Name,
        json!(""),
        cam.units,
        None,
    );
    refresh_operation(draft, cam);
}
fn selected_tool<'a>(
    draft: &Draft,
    cam: &'a CamDocumentDto,
) -> Option<&'a limo_cad_cam::CamToolDto> {
    cam.tool(form::text(draft, "/tool_id").ok()?.parse().ok()?)
}
fn refresh_operation(draft: &mut Draft, cam: &CamDocumentDto) {
    let mut options = form::options(&[
        ("keep", "Keep programmed cutting data"),
        ("default", "Copy tool defaults"),
    ]);
    if let Some(tool) = selected_tool(draft, cam) {
        options.extend(
            tool.cutting_presets
                .iter()
                .enumerate()
                .map(|(index, preset)| ChoiceOption {
                    value: index.to_string(),
                    label: preset.name.clone(),
                    disabled: false,
                }),
        );
    } else {
        options[1].disabled = true;
    }
    if let Some(field) = draft
        .fields
        .iter_mut()
        .find(|field| field.path == OP_CHOICE)
    {
        field.options = Some(options);
    }
}
fn copied_cutting<'a>(
    draft: &Draft,
    cam: &'a CamDocumentDto,
) -> Result<&'a CuttingParametersDto, String> {
    let tool = selected_tool(draft, cam).ok_or("Choose a project tool")?;
    let selection = form::text(draft, OP_CHOICE)?;
    if selection == "default" {
        Ok(&tool.cutting)
    } else {
        tool.cutting_presets
            .get(
                selection
                    .parse::<usize>()
                    .map_err(|_| "Choose a cutting preset")?,
            )
            .map(|preset| &preset.cutting)
            .ok_or("The selected cutting preset is unavailable".into())
    }
}
pub(super) fn changed_operation(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    path: &str,
) -> Result<(), String> {
    if !draft.fields.iter().any(|field| field.path == OP_CHOICE) {
        return Ok(());
    }
    if path == "/tool_id" {
        form::set(draft, OP_CHOICE, "keep");
        refresh_operation(draft, cam);
    } else if path == OP_CHOICE && form::text(draft, OP_CHOICE)? != "keep" {
        let cutting = *copied_cutting(draft, cam)?;
        for (path, text) in cutting_fields(&cutting, cam.units) {
            form::set(draft, path, &text);
        }
        let source = serde_json::to_string(&cutting).map_err(|error| error.to_string())?;
        form::set(draft, OP_COPY, &source);
    }
    Ok(())
}
fn cutting_fields(cutting: &CuttingParametersDto, units: CamUnits) -> Vec<(&'static str, String)> {
    vec![
        ("/cutting/spindle_rpm", cutting.spindle_rpm.to_string()),
        (
            "/cutting/feed_xy",
            units.from_mm(cutting.feed_xy).to_string(),
        ),
        ("/cutting/feed_z", units.from_mm(cutting.feed_z).to_string()),
        (
            "/cutting/coolant",
            serde_json::to_value(cutting.coolant)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned(),
        ),
    ]
}
pub(super) fn visible_operation(path: &str) -> bool {
    path != OP_COPY
}
pub(super) fn apply_operation(
    draft: &Draft,
    record: &mut Value,
    cam: &CamDocumentDto,
) -> Result<(), String> {
    let Ok(copy) = form::text(draft, OP_COPY) else {
        return Ok(());
    };
    if copy.is_empty() {
        return Ok(());
    }
    let cutting: CuttingParametersDto =
        serde_json::from_str(copy).map_err(|error| error.to_string())?;
    let canonical = serde_json::to_value(cutting).map_err(|error| error.to_string())?;
    for (path, text) in cutting_fields(&cutting, cam.units) {
        if form::text(draft, path)? == text {
            *record
                .pointer_mut(path)
                .ok_or("Cutting field is unavailable")? =
                canonical[path.strip_prefix("/cutting/").unwrap()].clone();
        }
    }
    Ok(())
}
