use super::super::panel::Paint;
use super::*;
use crate::native_viewport::interface_shell::ribbon::Icon;
pub(in super::super) fn paint(
    p: &mut Paint<'_>,
    s: &State,
    y: &mut f32,
    width: f32,
    blocked: bool,
) -> Result<(), String> {
    let Some(form) = s.form.as_ref() else {
        return Ok(());
    };
    let Some(joint) = form.original.as_ref() else {
        return Ok(());
    };
    *y += 12.;
    p.heading("joint-motion-title", "MOTION", Icon::Joint, *y, width);
    *y += 24.;
    p.text(
        "joint-motion-name",
        &joint.name,
        (10., *y, width - 52., 24.),
        11.,
    );
    p.button(
        ("joint-motion-edit", "Edit joint definition", Some("")),
        Command::Joint(joint::Command::Open(Some(joint.id.0))),
        (width - 36., *y, 26., 24.),
        Some(Icon::Pencil),
        (blocked, None),
        Field::None,
    )?;
    *y += 30.;
    if !joint.enabled || form.axes().is_empty() {
        p.text(
            "joint-motion-none",
            if joint.enabled {
                "A rigid joint has no motion coordinates."
            } else {
                "This joint is suppressed. Enable it to preview motion."
            },
            (10., *y, width - 20., 38.),
            10.,
        );
        *y += 44.;
        return Ok(());
    }
    p.button(
        (
            "joint-motion-demo",
            if s.started.is_some() {
                "Stop motion demo"
            } else {
                "Demo motion"
            },
            None,
        ),
        Command::Motion(if s.started.is_some() {
            Action::Revert
        } else {
            Action::Demo
        }),
        (10., *y, 128., 28.),
        None,
        (blocked, None),
        Field::None,
    )?;
    p.text(
        "joint-motion-preview",
        "Preview only",
        (152., *y, width - 162., 28.),
        9.,
    );
    *y += 36.;
    for (index, label) in form.axes() {
        let key = format!("joint-motion-{index}");
        let c = &form.coordinates[index];
        let linear = matches!(index, 1 | 4);
        let value = c.values[0].evaluate(UnitSystem::Mm, &[]);
        let unit = if linear { "mm" } else { "deg" };
        p.text(
            &format!("{key}-label"),
            &format!("{label} ({unit})"),
            (10., *y, width - 110., 28.),
            10.,
        );
        p.input(
            &format!("{key}-value"),
            &format!("{label} position"),
            c.values[0].text(),
            Command::Motion(Action::Field(index)),
            (width - 94., *y, 84.),
            blocked,
        )?;
        *y += 32.;
        let fallback = if linear { 100. } else { 180. };
        let min = if c.limited {
            c.values[1]
                .evaluate(UnitSystem::Mm, &[])
                .unwrap_or(-fallback)
        } else {
            (-fallback).min(value.as_ref().copied().unwrap_or(0.))
        };
        let max = if c.limited {
            c.values[2]
                .evaluate(UnitSystem::Mm, &[])
                .unwrap_or(fallback)
        } else {
            fallback.max(value.as_ref().copied().unwrap_or(0.))
        };
        let current = value.as_ref().copied().unwrap_or(0.).clamp(min, max);
        if min < max {
            p.button(
                (&format!("{key}-slider"), &format!("{label} slider"), None),
                Command::Motion(Action::Field(index)),
                (10., *y, width - 20., 24.),
                None,
                (blocked || value.is_err(), None),
                Field::Range {
                    value: current,
                    min,
                    max,
                    step: if linear { 0.5 } else { 1. },
                },
            )?;
            *y += 26.;
        }
    }
    if let Some(error) = &s.error {
        p.text(
            "joint-motion-error",
            error,
            (10., *y, width - 20., 42.),
            10.,
        );
        p.warning("joint-motion-error");
        *y += 48.;
    }
    if s.preview || s.started.is_some() {
        let half = (width - 26.) / 2.;
        p.button(
            (
                "joint-motion-revert",
                "Revert joint position",
                Some("Revert"),
            ),
            Command::Motion(Action::Revert),
            (10., *y, half, 28.),
            None,
            (blocked, None),
            Field::None,
        )?;
        p.button(
            (
                "joint-motion-save",
                "Save joint position",
                Some("Save position"),
            ),
            Command::Motion(Action::Save),
            (16. + half, *y, half, 28.),
            None,
            (blocked || s.error.is_some() || s.started.is_some(), None),
            Field::None,
        )?;
        *y += 36.;
    }
    Ok(())
}
