use super::*;

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    editor: &mut Editor,
    width: f32,
    height: f32,
    busy: bool,
) -> Result<(), String> {
    let w = (width - 300.).clamp(280., 520.);
    let h = (height - 224.).clamp(280., 700.);
    let x = (width - w - 16.).max(4.);
    let y = 132.;
    let theme = crate::native_viewport::ui::theme(world);
    super::super::super::card(
        (&mut editor.widgets, world, camera),
        "nc-card",
        rect(x, y, w, h),
        theme.panel.with_alpha(1.),
        6.,
        80,
    );
    editor.widgets.text(
        world,
        camera,
        "nc-heading",
        rect(x + 12., y + 8., w - 24., 22.),
        "NC workpiece simulation",
        15.,
        82,
    );
    editor.widgets.text(
        world,
        camera,
        "nc-purpose",
        rect(x + 12., y + 34., w - 24., 32.),
        "Use this setup's tools and stock. Paste controller code or open an NC file.",
        11.,
        82,
    );
    let fields = [
        (
            "nc-name",
            "Program name",
            Command::FileName,
            Field::Text {
                value: editor.input.file_name.clone().unwrap_or_default(),
                read_only: false,
                selection: None,
            },
            rect(x + 12., y + 84., w - 110., 28.),
        ),
        (
            "nc-dialect",
            "Controller language",
            Command::Dialect,
            Field::Choice {
                value: dialect_key(editor.input.dialect)?,
                options: dialects(),
            },
            rect(x + 12., y + 135., w - 24., 28.),
        ),
        (
            "nc-source",
            "Controller code",
            Command::Source,
            Field::Text {
                value: editor.input.source.clone(),
                read_only: false,
                selection: None,
            },
            rect(x + 12., y + 186., w - 24., (h - 254.).max(36.)),
        ),
    ];
    for (key, label, command, field, mut bounds) in fields {
        let top = match bounds.top {
            Val::Px(top) => top,
            _ => unreachable!(),
        };
        editor.widgets.text(
            world,
            camera,
            &format!("{key}-label"),
            rect(x + 12., top - 18., w - 24., 16.),
            label,
            10.,
            82,
        );
        let mut control = InterfaceControl::button("cam-nc-source", label);
        control.modal_scope = Some("cam-nc-source".into());
        control.disabled = editor.picker.is_some();
        control.field = field;
        let caption = if command == Command::Dialect {
            control.role = "combobox".into();
            let value = dialect_key(editor.input.dialect)?;
            dialects()
                .into_iter()
                .find(|o| o.value == value)
                .map(|o| o.label)
        } else {
            None
        };
        bounds.border = UiRect::all(px(1.));
        let entity = editor.widgets.button(
            world,
            camera,
            key,
            control,
            caption.as_deref(),
            native(editor.serial, command),
            bounds,
            None,
            82,
        )?;
        if command == Command::Source {
            interface_shell::fields::multiline::enable(world, entity)?;
            interface_shell::fields::limits::enable(world, entity, limo_cad_cam::MAX_GCODE_BYTES);
            let font = theme.code_text(world.resource::<ViewportUiAssets>(), 12.);
            if world.get::<TextFont>(entity) != Some(&font) {
                world.entity_mut(entity).insert(font);
            }
        }
    }
    for (key, label, command, bounds, disabled) in [
        (
            "nc-open",
            "Open NC file",
            Command::Pick,
            rect(x + w - 91., y + 84., 79., 28.),
            busy,
        ),
        (
            "nc-close",
            "Cancel NC input",
            Command::Close,
            rect(x + 12., y + h - 38., 110., 26.),
            false,
        ),
        (
            "nc-run",
            "Build NC simulation",
            Command::Run,
            rect(x + w - 172., y + h - 38., 160., 26.),
            busy,
        ),
    ] {
        let mut control = InterfaceControl::button("cam-nc-source", label);
        control.modal_scope = Some("cam-nc-source".into());
        control.disabled = disabled;
        editor.widgets.button(
            world,
            camera,
            key,
            control,
            Some(match command {
                Command::Pick => "Open",
                Command::Close => "Cancel",
                _ => label,
            }),
            native(editor.serial, command),
            bounds,
            None,
            82,
        )?;
    }
    editor.widgets.text(
        world,
        camera,
        "nc-error",
        rect(x + 12., y + h - 66., w - 24., 26.),
        if editor.picker.is_some() {
            "Choosing an NC file..."
        } else {
            editor
                .limit_error
                .as_deref()
                .or(editor.source_error.as_deref())
                .unwrap_or(&editor.error)
        },
        11.,
        82,
    );
    if let Some(entity) = editor.widgets.entity("nc-error") {
        world
            .entity_mut(entity)
            .insert(TextColor(Color::srgb(0.95, 0.35, 0.3)));
    }
    Ok(())
}
