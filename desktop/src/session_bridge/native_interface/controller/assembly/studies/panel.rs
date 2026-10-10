use super::super::panel::Paint;
use super::*;
use crate::native_viewport::interface_shell::ribbon::Icon;
use limo_cad_interface::ChoiceOption;

fn button(
    p: &mut Paint<'_>,
    (key, label, caption): (&str, &str, &str),
    action: Action,
    (x, y, w): (f32, f32, f32),
    blocked: bool,
    field: Field,
    selected: Option<bool>,
) -> Result<(), String> {
    p.button(
        (key, label, Some(caption)),
        Command::Study(action),
        (x, y, w, 28.),
        None,
        (blocked, selected),
        field,
    )
}
fn text(
    p: &mut Paint<'_>,
    key: &str,
    label: &str,
    value: &str,
    edit: Edit,
    (x, y, w): (f32, f32, f32),
    blocked: bool,
) -> Result<(), String> {
    p.input(
        key,
        label,
        value,
        Command::Study(Action::Field(edit)),
        (x, y, w),
        blocked,
    )
}
fn choice(
    p: &mut Paint<'_>,
    (s, a): (&State, &AssemblyDocumentDto),
    kind: Choice,
    label: &str,
    y: &mut f32,
    width: f32,
    blocked: bool,
) -> Result<(), String> {
    let key = format!("study-choice-{kind:?}");
    let options = options(s, a, kind);
    button(
        p,
        (&key, label, label),
        Action::Choice(kind),
        (14., *y, width - 28.),
        blocked || options.is_empty(),
        Field::Choice {
            value: selected(s, kind),
            options: options
                .iter()
                .map(|(value, label)| ChoiceOption {
                    value: value.clone(),
                    label: label.clone(),
                    disabled: false,
                })
                .collect(),
        },
        Some(s.choice == Some(kind)),
    )?;
    *y += 32.;
    if s.choice == Some(kind) {
        for (value, caption) in options {
            button(
                p,
                (
                    &format!("{key}-{value}"),
                    &format!("{label}: {caption}"),
                    &caption,
                ),
                Action::Select(kind, value.clone()),
                (20., *y, width - 40.),
                blocked,
                Field::None,
                Some(value == selected(s, kind)),
            )?;
            *y += 30.;
        }
    }
    Ok(())
}
pub(in super::super) fn paint(
    p: &mut Paint<'_>,
    s: &mut State,
    a: &AssemblyDocumentDto,
    width: f32,
    blocked: bool,
) -> Result<f32, String> {
    let mut y = 10.;
    let half = (width - 26.) / 2.;
    p.heading(
        "positions-title",
        "NAMED POSITIONS",
        Icon::Save,
        y,
        width - 72.,
    );
    button(
        p,
        ("position-capture", "Capture assembly position", "Capture"),
        Action::Capture,
        (width - 74., y - 4., 64.),
        blocked,
        Field::None,
        None,
    )?;
    y += 30.;
    if a.positions.is_empty() {
        p.text(
            "positions-empty",
            "Capture joint coordinates to return to a named position.",
            (10., y, width - 20., 38.),
            10.,
        );
        y += 42.;
    }
    for position in &a.positions {
        let id = position.id.0;
        let key = format!("position-{id}");
        p.card(&format!("{key}-card"), y - 3., width, 34.);
        p.input(
            &format!("{key}-name"),
            &format!("Position {id} name"),
            s.names
                .get(&id)
                .map(String::as_str)
                .unwrap_or(&position.name),
            Command::Study(Action::PositionName(id)),
            (14., y, width - 96.),
            blocked,
        )?;
        button(
            p,
            (
                &format!("{key}-apply"),
                &format!("Apply position {id}"),
                "Apply",
            ),
            Action::ApplyPosition(id),
            (width - 78., y, 40.),
            blocked,
            Field::None,
            None,
        )?;
        button(
            p,
            (
                &format!("{key}-delete"),
                &format!("Delete position {id}"),
                "×",
            ),
            Action::DeletePosition(id),
            (width - 34., y, 20.),
            blocked,
            Field::None,
            None,
        )?;
        y += 40.;
    }
    p.heading(
        "study-title",
        "MOTION STUDY",
        Icon::Activity,
        y,
        width - 40.,
    );
    button(
        p,
        ("study-create", "Create motion study", "+"),
        Action::Create,
        (width - 38., y - 4., 28.),
        blocked,
        Field::None,
        None,
    )?;
    y += 30.;
    let Some(f) = s.form.as_ref() else {
        button(
            p,
            (
                "study-create-empty",
                "Create first motion study",
                "Create motion study",
            ),
            Action::Create,
            (10., y, width - 20.),
            blocked,
            Field::None,
            None,
        )?;
        return Ok(y + 42.);
    };
    choice(
        p,
        (s, a),
        Choice::Study,
        "Motion study",
        &mut y,
        width,
        blocked,
    )?;
    text(
        p,
        "study-name",
        "Motion study name",
        &f.name,
        Edit::Name,
        (10., y, width - 82.),
        blocked,
    )?;
    button(
        p,
        ("study-delete", "Delete motion study", "Delete"),
        Action::Delete,
        (width - 64., y, 54.),
        blocked,
        Field::None,
        None,
    )?;
    y += 34.;
    p.text(
        "duration-label",
        "DURATION (SECONDS)",
        (10., y, half, 18.),
        9.,
    );
    p.text(
        "speed-label",
        "PLAYBACK SPEED",
        (16. + half, y, half, 18.),
        9.,
    );
    y += 18.;
    text(
        p,
        "study-duration",
        "Study duration seconds",
        &f.duration,
        Edit::Duration,
        (10., y, half),
        blocked,
    )?;
    text(
        p,
        "study-speed",
        "Study playback speed",
        &f.speed,
        Edit::Speed,
        (16. + half, y, half),
        blocked,
    )?;
    y += 34.;
    button(
        p,
        ("study-loop", "Loop playback", "Loop playback"),
        Action::Loop,
        (10., y, width - 20.),
        blocked,
        Field::Toggle(f.looped),
        Some(f.looped),
    )?;
    y += 34.;
    let valid = s.validation.get_or_insert_with(|| f.value(a));
    let dirty = valid.as_ref().map_or(true, |v| *v != f.original);
    let invalid = valid.is_err();
    let validation_error = valid.as_ref().err().cloned();
    p.card("study-playback", y - 4., width, 40.);
    p.button(
        (
            "study-play",
            if s.started.is_some() {
                "Pause motion study"
            } else {
                "Play motion study"
            },
            Some(""),
        ),
        Command::Study(Action::Play),
        (14., y, 28., 28.),
        Some(if s.started.is_some() {
            Icon::Pause
        } else {
            Icon::Play
        }),
        (blocked || dirty, None),
        Field::None,
    )?;
    p.button(
        ("study-stop", "Stop motion study", Some("")),
        Command::Study(Action::Stop),
        (46., y, 28., 28.),
        Some(Icon::Square),
        (blocked, None),
        Field::None,
    )?;
    button(
        p,
        ("study-time", "Motion study time", ""),
        Action::Time,
        (80., y, width - 150.),
        blocked || dirty,
        Field::Range {
            value: s.time.clamp(0., f.original.duration_seconds),
            min: 0.,
            max: f.original.duration_seconds,
            step: 0.001,
        },
        None,
    )?;
    p.text(
        "study-time-caption",
        &format!("{:.2}s", s.time),
        (width - 65., y, 51., 28.),
        10.,
    );
    y += 44.;
    if let Some(e) = s
        .evaluation
        .as_ref()
        .filter(|e| e.stopped_by_contact.is_some())
    {
        p.text(
            "study-stopped",
            &format!(
                "Stopped by contact {} at {:.4}s",
                e.stopped_by_contact.unwrap().0,
                e.sample.time_seconds
            ),
            (10., y, width - 20., 30.),
            9.,
        );
        p.warning("study-stopped");
        y += 34.;
    }
    button(
        p,
        ("study-add-driver", "Add motion driver", "+ Driver"),
        Action::AddDriver,
        (10., y, half),
        blocked,
        Field::None,
        None,
    )?;
    button(
        p,
        ("study-export", "Export motion path CSV", "Path CSV"),
        Action::Export,
        (16. + half, y, half),
        blocked || dirty || s.picker.is_some(),
        Field::None,
        None,
    )?;
    y += 40.;
    button(
        p,
        ("study-apply", "Apply motion study", "Apply study"),
        Action::Apply,
        (10., y, half),
        blocked || invalid || !dirty,
        Field::None,
        None,
    )?;
    button(
        p,
        (
            "study-revert",
            "Revert motion study changes",
            "Revert changes",
        ),
        Action::Revert,
        (16. + half, y, half),
        blocked || !dirty,
        Field::None,
        None,
    )?;
    y += 40.;
    if let Some(error) = s.error.as_deref().or(validation_error.as_deref()) {
        p.text("study-error", error, (10., y, width - 20., 44.), 10.);
        p.warning("study-error");
        y += 50.;
    }
    for d in &f.drivers {
        let id = d.record.id.0;
        let key = format!("driver-{id}");
        let top = y;
        text(
            p,
            &format!("{key}-name"),
            &format!("Driver {id} name"),
            &d.name,
            Edit::DriverName(id),
            (14., y, width - 106.),
            blocked,
        )?;
        button(
            p,
            (&format!("{key}-on"), &format!("Enable driver {id}"), "on"),
            Action::Enabled(id),
            (width - 90., y, 48.),
            blocked,
            Field::Toggle(d.record.enabled),
            Some(d.record.enabled),
        )?;
        button(
            p,
            (
                &format!("{key}-delete"),
                &format!("Delete driver {id}"),
                "×",
            ),
            Action::DeleteDriver(id),
            (width - 38., y, 24.),
            blocked,
            Field::None,
            None,
        )?;
        y += 32.;
        choice_row(
            p,
            s,
            a,
            Choice::Joint(id),
            &format!("Driver {id} joint"),
            (14., y, half - 2.),
            blocked,
        )?;
        choice_row(
            p,
            s,
            a,
            Choice::Coordinate(id),
            &format!("Driver {id} coordinate"),
            (16. + half, y, half - 4.),
            blocked,
        )?;
        y += 32.;
        choice_options(
            p,
            (s, a),
            Choice::Joint(id),
            &format!("Driver {id} joint"),
            &mut y,
            width,
            blocked,
        )?;
        choice_options(
            p,
            (s, a),
            Choice::Coordinate(id),
            &format!("Driver {id} coordinate"),
            &mut y,
            width,
            blocked,
        )?;
        for (index, (caption, motor)) in [("Keyframes", false), ("Motor", true)]
            .into_iter()
            .enumerate()
        {
            button(
                p,
                (
                    &format!("{key}-law-{motor}"),
                    &format!("Driver {id} {caption}"),
                    caption,
                ),
                Action::Law(id, motor),
                (14. + index as f32 * half, y, half - 2.),
                blocked,
                Field::None,
                Some(d.is_motor == motor),
            )?;
        }
        y += 34.;
        if d.is_motor {
            let third = (width - 36.) / 3.;
            for (index, label) in ["Start", "Speed / s", "Accel / s²"].into_iter().enumerate() {
                let x = 14. + index as f32 * (third + 4.);
                p.text(
                    &format!("{key}-motor-label-{index}"),
                    label,
                    (x, y, third, 18.),
                    9.,
                );
                text(
                    p,
                    &format!("{key}-motor-{index}"),
                    &format!("Driver {id} motor {label}"),
                    &d.motor[index],
                    Edit::Motor(id, index),
                    (x, y + 18., third),
                    blocked,
                )?;
            }
            y += 52.;
        } else {
            let col = (width - 126.) / 2.;
            p.text(
                &format!("{key}-labels"),
                "TIME (s)       VALUE          INTERPOLATION",
                (14., y, width - 28., 18.),
                8.,
            );
            y += 18.;
            for (index, k) in d.keys.iter().enumerate() {
                let row = format!("{key}-key-{index}");
                let prefix = format!("Driver {id} keyframe {}", index + 1);
                text(
                    p,
                    &format!("{row}-time"),
                    &format!("{prefix} time"),
                    &k.time,
                    Edit::KeyTime(id, index),
                    (14., y, col),
                    blocked,
                )?;
                text(
                    p,
                    &format!("{row}-value"),
                    &format!("{prefix} value"),
                    &k.value,
                    Edit::KeyValue(id, index),
                    (18. + col, y, col),
                    blocked,
                )?;
                choice_row(
                    p,
                    s,
                    a,
                    Choice::Interpolation(id, index),
                    &format!("{prefix} interpolation"),
                    (22. + 2. * col, y, 64.),
                    blocked,
                )?;
                button(
                    p,
                    (
                        &format!("{row}-delete"),
                        &format!("Delete driver {id} keyframe {}", index + 1),
                        "×",
                    ),
                    Action::DeleteKey(id, index),
                    (width - 36., y, 22.),
                    blocked || d.keys.len() <= 1,
                    Field::None,
                    None,
                )?;
                y += 32.;
                choice_options(
                    p,
                    (s, a),
                    Choice::Interpolation(id, index),
                    &format!("{prefix} interpolation"),
                    &mut y,
                    width,
                    blocked,
                )?;
            }
            button(
                p,
                (
                    &format!("{key}-add-key"),
                    &format!("Add driver {id} keyframe"),
                    "+ Keyframe",
                ),
                Action::AddKey(id),
                (14., y, width - 28.),
                blocked,
                Field::None,
                None,
            )?;
            y += 34.;
        }
        p.card(&format!("{key}-card"), top - 4., width, y - top + 8.);
        y += 20.;
    }
    Ok(y)
}
fn choice_row(
    p: &mut Paint<'_>,
    s: &State,
    a: &AssemblyDocumentDto,
    kind: Choice,
    label: &str,
    (x, y, w): (f32, f32, f32),
    blocked: bool,
) -> Result<(), String> {
    let options = options(s, a, kind);
    button(
        p,
        (&format!("study-choice-{kind:?}"), label, label),
        Action::Choice(kind),
        (x, y, w),
        blocked || options.is_empty(),
        Field::Choice {
            value: selected(s, kind),
            options: options
                .into_iter()
                .map(|(value, label)| ChoiceOption {
                    value,
                    label,
                    disabled: false,
                })
                .collect(),
        },
        Some(s.choice == Some(kind)),
    )
}
fn choice_options(
    p: &mut Paint<'_>,
    (s, a): (&State, &AssemblyDocumentDto),
    kind: Choice,
    label: &str,
    y: &mut f32,
    width: f32,
    blocked: bool,
) -> Result<(), String> {
    if s.choice == Some(kind) {
        for (value, caption) in options(s, a, kind) {
            button(
                p,
                (
                    &format!("study-choice-{kind:?}-{value}"),
                    &format!("{label}: {caption}"),
                    &caption,
                ),
                Action::Select(kind, value.clone()),
                (20., *y, width - 40.),
                blocked,
                Field::None,
                Some(value == selected(s, kind)),
            )?;
            *y += 30.;
        }
    }
    Ok(())
}
