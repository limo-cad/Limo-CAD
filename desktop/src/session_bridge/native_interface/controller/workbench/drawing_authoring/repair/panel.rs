use super::*;
use bevy::prelude::Entity;
use limo_cad_interface::{Field, KeyChord};
fn choice(
    (world, camera, e): (&mut World, Entity, &mut Editor),
    key: &str,
    label: &str,
    command: Command,
    value: String,
    options: Vec<ChoiceOption>,
    bounds: Node,
) -> Result<(), String> {
    let caption = options
        .iter()
        .find(|o| o.value == value)
        .map_or("Choose…", |o| o.label.as_str())
        .to_owned();
    let mut control = InterfaceControl::button("drawing/annotation", label);
    control.role = "combobox".into();
    control.disabled = options.is_empty() || e.repair.pending.is_some();
    control.owned_keys = [
        "ArrowUp",
        "ArrowDown",
        "ArrowLeft",
        "ArrowRight",
        "Home",
        "End",
    ]
    .map(KeyChord::plain)
    .into();
    control.field = Field::Choice { value, options };
    e.widgets.button(
        world,
        camera,
        key,
        control,
        Some(&caption),
        super::super::runtime::native(e.serial, command),
        bounds,
        None,
        46,
    )?;
    Ok(())
}
pub(in super::super) fn paint(
    world: &mut World,
    camera: Entity,
    e: &mut Editor,
    width: f32,
    mut y: f32,
) -> Result<f32, String> {
    let sheet = e.stamp.as_ref().ok_or("Create a sheet first")?.sheet_id;
    let options = options(&e.document, sheet);
    {
        let record = e.repair.record.clone();
        choice(
            (world, camera, e),
            "annotation-repair-record",
            "Annotation or derived view",
            Command::RepairRecord,
            record,
            options,
            rect(10., y, width - 20., 28.),
        )
    }?;
    y += 34.;
    let options = reference_options(&e.repair);
    {
        let reference = e.repair.reference.to_string();
        choice(
            (world, camera, e),
            "annotation-repair-reference",
            "Reference to replace",
            Command::RepairReference,
            reference,
            options,
            rect(10., y, width - 20., 28.),
        )
    }?;
    y += 34.;
    let label = if e.repair.pending.is_some() {
        "Replacement selected. Apply repair to commit it, or Reset to keep the saved reference."
            .into()
    } else if let Some(reference) = e.repair.references.get(e.repair.reference) {
        format!(
            "View {}: choose {} for {}.",
            e.repair.view_id,
            match reference.kind {
                Kind::Anchor => "a projected endpoint",
                Kind::Circle => "a circular edge",
                Kind::Line => "a straight edge",
            },
            reference.label
        )
    } else {
        "No associative annotations or derived views on this sheet.".into()
    };
    e.widgets.text(
        world,
        camera,
        "annotation-repair-instruction",
        rect(12., y, width - 24., 58.),
        &label,
        11.,
        45,
    );
    y += 64.;
    {
        let apply_disabled = e.repair.pending.is_none();
        super::super::panel::button(
            (world, camera, e),
            "annotation-repair-apply",
            "Apply repair",
            Command::Apply,
            rect(10., y, width - 20., 28.),
            apply_disabled,
        )
    }?;
    y += 34.;
    if let Some(id) = e
        .repair
        .record
        .strip_prefix("annotation:")
        .and_then(|id| id.parse::<u64>().ok())
    {
        {
            let edit_disabled = e.repair.pending.is_some();
            super::super::panel::button(
                (world, camera, e),
                "annotation-repair-edit",
                "Edit selected annotation",
                Command::Select(id),
                rect(10., y, width - 20., 28.),
                edit_disabled,
            )
        }?;
        y += 34.;
    }
    Ok(y)
}
