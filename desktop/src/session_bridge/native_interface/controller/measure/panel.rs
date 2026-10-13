use super::*;

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    state: &mut State,
    units: UnitSystem,
    width: f32,
    height: f32,
) -> Result<(), String> {
    state.units = units;
    let w = 324f32.min((width - 24.).max(220.));
    let x = (width - w - 12.).max(0.);
    let y = 128.;
    let h = (height - y - 72.).max(240.);
    let theme = native_viewport::ui::theme(world);
    state.widgets.panel(
        world,
        camera,
        "measure-panel",
        chrome::rect(x, y, w, h),
        theme.panel.with_alpha(1.),
        70,
    );
    state.widgets.text(
        world,
        camera,
        "measure-title",
        chrome::rect(x + 12., y + 8., w - 160., 24.),
        "Measure",
        15.,
        72,
    );
    for (key, label, caption, command, bx, bw) in [
        (
            "clear",
            "Clear measurement selections",
            "Clear",
            Command::Clear,
            x + w - 136.,
            64.,
        ),
        (
            "close",
            "Close Measure",
            "Close",
            Command::Close,
            x + w - 68.,
            56.,
        ),
    ] {
        state.widgets.button(
            world,
            camera,
            &format!("measure-{key}"),
            InterfaceControl::button("solid/measure", label),
            Some(caption),
            NativeCommand::Measure(state.generation, command),
            chrome::rect(bx, y + 6., bw, 28.),
            None,
            72,
        )?;
    }
    for (i, field) in [
        Field::Filter,
        Field::Precision,
        Field::Secondary,
        Field::Delta,
    ]
    .iter()
    .enumerate()
    {
        let label = match field {
            Field::Filter => "Selection filter",
            Field::Precision => "Precision",
            Field::Secondary => "Secondary units",
            Field::Delta => "XYZ delta",
        };
        let options = State::options(*field);
        let caption = options
            .iter()
            .find(|o| o.value == state.value(*field))
            .map(|o| o.label.clone())
            .unwrap_or_default();
        state.widgets.text(
            world,
            camera,
            &format!("measure-field-label-{i}"),
            chrome::rect(x + 12., y + 42. + i as f32 * 32., 106., 28.),
            label,
            10.,
            72,
        );
        let mut control = InterfaceControl::button("solid/measure", label);
        control.role = "combobox".into();
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
        control.field = ControlField::Choice {
            value: state.value(*field).into(),
            options,
        };
        state.widgets.button(
            world,
            camera,
            &format!("measure-field-{i}"),
            control,
            Some(&caption),
            NativeCommand::Measure(state.generation, Command::Field(*field)),
            chrome::rect(x + 120., y + 42. + i as f32 * 32., w - 132., 28.),
            None,
            72,
        )?;
    }
    let mut snap = InterfaceControl::button("solid/measure", "Show snap points");
    snap.field = ControlField::Toggle(state.snaps);
    snap.disabled = state.filter == "body";
    state.widgets.button(
        world,
        camera,
        "measure-snaps",
        snap,
        Some(if state.snaps {
            "☑ Show snap points"
        } else {
            "☐ Show snap points"
        }),
        NativeCommand::Measure(state.generation, Command::SnapPoints),
        chrome::rect(x + 12., y + 176., w - 24., 26.),
        None,
        72,
    )?;
    let row_list = rows(state, units);
    let capacity = ((h - 272.) / 24.).floor().max(1.) as usize;
    state.scroll = state.scroll.min(row_list.len().saturating_sub(capacity));
    if row_list.is_empty() {
        let prompt = if state.message.is_empty() {
            "Select one object for properties, then a second for distance and angle."
        } else {
            &state.message
        };
        state.widgets.text(
            world,
            camera,
            "measure-prompt",
            chrome::rect(x + 12., y + 212., w - 24., 72.),
            prompt,
            11.,
            72,
        );
    }
    for (visible, (index, row)) in row_list
        .iter()
        .enumerate()
        .skip(state.scroll)
        .take(capacity)
        .enumerate()
    {
        let top = y + 210. + visible as f32 * 24.;
        if row.value.is_empty() {
            state.widgets.text(
                world,
                camera,
                &format!("measure-section-{index}"),
                chrome::rect(x + 12., top, w - 24., 24.),
                &row.label,
                12.,
                72,
            );
        } else {
            state.widgets.text(
                world,
                camera,
                &format!("measure-label-{index}"),
                chrome::rect(x + 12., top, 112., 24.),
                &row.label,
                10.,
                72,
            );
            let control = InterfaceControl::button(
                "solid/measure",
                format!("Copy {}: {}", row.label, row.value),
            );
            let e = state.widgets.button(
                world,
                camera,
                &format!("measure-row-{index}"),
                control,
                Some(&row.value),
                NativeCommand::Measure(state.generation, Command::Copy(index)),
                chrome::rect(x + 124., top, w - 136., 24.),
                None,
                72,
            )?;
            interface_shell::caption_size(world, e, 10.);
        }
    }
    if row_list.len() > capacity {
        for (key, caption, amount, bx) in
            [("up", "↑", -3, x + w - 70.), ("down", "↓", 3, x + w - 40.)]
        {
            state.widgets.button(
                world,
                camera,
                &format!("measure-{key}"),
                InterfaceControl::button("solid/measure", format!("Scroll {key}")),
                Some(caption),
                NativeCommand::Measure(state.generation, Command::Scroll(amount)),
                chrome::rect(bx, y + h - 56., 28., 24.),
                None,
                72,
            )?;
        }
    }
    state.widgets.text(
        world,
        camera,
        "measure-help",
        chrome::rect(x + 12., y + h - 48., w - 90., 42.),
        if state.message.is_empty() {
            "Click a value to copy. Ctrl / ⌘ locks to snap points; Shift hides snaps."
        } else {
            &state.message
        },
        9.,
        72,
    );
    Ok(())
}
