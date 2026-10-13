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
        Command::Inspect(action),
        (x, y, w, 28.),
        None,
        (blocked, selected),
        field,
    )
}
fn input(
    p: &mut Paint<'_>,
    key: &str,
    label: &str,
    value: &str,
    field: Edit,
    (x, y, w): (f32, f32, f32),
    blocked: bool,
) -> Result<(), String> {
    p.input(
        key,
        label,
        value,
        Command::Inspect(Action::Field(field)),
        (x, y, w),
        blocked,
    )
}
fn choice(
    p: &mut Paint<'_>,
    (s, a, placed): (&State, &AssemblyDocumentDto, &[(u64, u64)]),
    kind: Choice,
    label: &str,
    y: &mut f32,
    width: f32,
    blocked: bool,
) -> Result<(), String> {
    let key = format!("inspect-{kind:?}");
    let options = choices(kind, a, placed);
    button(
        p,
        (&key, label, label),
        Action::Choice(kind),
        (10., *y, width - 20.),
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
    *y += 34.;
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
                (16., *y, width - 32.),
                blocked,
                Field::None,
                Some(selected(s, kind) == value),
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
    units: UnitSystem,
    width: f32,
    blocked: bool,
) -> Result<f32, String> {
    let placed = placed(p.world);
    s.update(a, units, &placed);
    let mut y = 10.;
    let half = (width - 26.) / 2.;
    p.heading(
        "inspect-title",
        "INTERFERENCE & CLEARANCE",
        Icon::ShieldAlert,
        y,
        width,
    );
    y += 28.;
    p.text("clearance-label", "CLEARANCE", (10., y, half, 18.), 9.);
    p.text(
        "rate-label",
        "SAMPLES / SECOND",
        (16. + half, y, half, 18.),
        9.,
    );
    y += 20.;
    input(
        p,
        "inspect-clearance",
        "Inspection clearance",
        s.clearance.text(),
        Edit::Clearance,
        (10., y, half),
        blocked,
    )?;
    input(
        p,
        "inspect-rate",
        "Inspection sample rate",
        &s.sample_rate,
        Edit::SampleRate,
        (16. + half, y, half),
        blocked,
    )?;
    y += 36.;
    if !a.motion_studies.is_empty() {
        choice(
            p,
            (s, a, &placed),
            Choice::Study,
            "Swept motion study",
            &mut y,
            width,
            blocked,
        )?;
        button(
            p,
            (
                "inspect-stop",
                "Stop at first collision",
                "Stop at first collision",
            ),
            Action::StopFirst,
            (10., y, width - 20.),
            blocked,
            Field::Toggle(s.stop_first),
            None,
        )?;
        y += 34.;
    }
    let clearance = s.clearance(units);
    button(
        p,
        (
            "inspect-check",
            "Check current interference",
            "Check current",
        ),
        Action::Check,
        (10., y, half),
        blocked || clearance.is_err(),
        Field::None,
        None,
    )?;
    button(
        p,
        ("inspect-swept", "Check swept collisions", "Swept study"),
        Action::Swept,
        (16. + half, y, half),
        blocked || clearance.is_err() || s.sample_rate().is_err() || s.study.is_none(),
        Field::None,
        None,
    )?;
    y += 38.;
    if let Some(error) = clearance
        .err()
        .or_else(|| s.sample_rate().err())
        .or_else(|| s.error.clone())
    {
        p.text("inspect-error", &error, (10., y, width - 20., 42.), 10.);
        y += 48.;
    }
    if let Some(report) = &s.report {
        let violations = report
            .pairs
            .iter()
            .filter(|p| p.interfering || p.below_clearance)
            .count()
            .min(20);
        p.card(
            "static-result-card",
            y - 4.,
            width,
            if violations == 0 {
                64.
            } else {
                28. + 42. * violations as f32
            },
        );
        p.text(
            "inspect-result-title",
            if report.exact {
                "STATIC RESULT · EXACT OCCT"
            } else {
                "STATIC RESULT · MESH FALLBACK"
            },
            (10., y, width - 20., 20.),
            10.,
        );
        y += 24.;
        let pairs = report
            .pairs
            .iter()
            .filter(|p| p.interfering || p.below_clearance)
            .collect::<Vec<_>>();
        if pairs.is_empty() {
            p.text(
                "inspect-clear",
                "No overlap or clearance violations.",
                (10., y, width - 20., 30.),
                10.,
            );
            y += 36.;
        }
        for (i, pair) in pairs.iter().take(20).enumerate() {
            let result = if pair.interfering {
                format!("Overlap {:.3} mm³", pair.overlap_volume_mm3)
            } else {
                format!("Clearance {:.3} mm", pair.minimum_clearance_mm)
            };
            p.text(
                &format!("inspect-pair-{i}"),
                &format!(
                    "O{}/B{} ↔ O{}/B{}\n{result}",
                    pair.occurrence_a.0, pair.body_a.0, pair.occurrence_b.0, pair.body_b.0
                ),
                (10., y, width - 20., 38.),
                10.,
            );
            p.warning(&format!("inspect-pair-{i}"));
            y += 42.;
        }
    }
    if let Some(report) = &s.swept {
        p.card(
            "swept-result-card",
            y - 4.,
            width,
            60. + 42. * report.events.len().min(8) as f32,
        );
        p.text(
            "swept-result-title",
            &format!(
                "SWEPT RESULT · {} {} SAMPLES",
                report.sample_count,
                if report.exact { "EXACT" } else { "MESH" }
            ),
            (10., y, width - 20., 20.),
            10.,
        );
        y += 24.;
        p.text(
            "swept-result-summary",
            &if report.events.is_empty() {
                "No swept collisions.".into()
            } else {
                format!("{} collision intervals", report.events.len())
            },
            (10., y, width - 20., 28.),
            10.,
        );
        y += 32.;
        for (i, event) in report.events.iter().take(8).enumerate() {
            p.text(
                &format!("swept-event-{i}"),
                &format!(
                    "O{}/B{} ↔ O{}/B{}\n{:.4}–{:.4} s",
                    event.occurrence_a.0,
                    event.body_a.0,
                    event.occurrence_b.0,
                    event.body_b.0,
                    event.first_time_seconds,
                    event.last_time_seconds
                ),
                (10., y, width - 20., 38.),
                10.,
            );
            y += 42.;
        }
    }
    y += 12.;
    p.heading(
        "contacts-title",
        "CONTACT STOPS",
        Icon::TimerReset,
        y,
        width,
    );
    y += 28.;
    choice(
        p,
        (s, a, &placed),
        Choice::First,
        "First contact body",
        &mut y,
        width,
        blocked,
    )?;
    choice(
        p,
        (s, a, &placed),
        Choice::Second,
        "Second contact body",
        &mut y,
        width,
        blocked,
    )?;
    button(
        p,
        (
            "contact-create",
            "Create physical stop",
            "Create physical stop",
        ),
        Action::CreateContact,
        (10., y, width - 20.),
        blocked
            || s.first.is_none()
            || s.second.is_none()
            || s.first == s.second
            || s.clearance(units).is_err(),
        Field::None,
        None,
    )?;
    y += 40.;
    for c in &a.contact_sets {
        let draft = &s.contacts[&c.id.0];
        p.card(&format!("contact-{}-card", c.id.0), y - 4., width, 150.);
        let key = format!("contact-{}", c.id.0);
        input(
            p,
            &format!("{key}-name"),
            &format!("Contact {} name", c.id.0),
            &draft.name,
            Edit::ContactName(c.id.0),
            (10., y, width - 54.),
            blocked,
        )?;
        button(
            p,
            (
                &format!("{key}-delete"),
                &format!("Delete contact {}", c.id.0),
                "×",
            ),
            Action::Delete(c.id.0),
            (width - 38., y, 28.),
            blocked,
            Field::None,
            None,
        )?;
        y += 34.;
        p.text(
            &format!("{key}-bodies"),
            &format!(
                "O{}/B{} ↔ O{}/B{}",
                c.occurrence_a.0, c.body_a.0, c.occurrence_b.0, c.body_b.0
            ),
            (10., y, width - 20., 18.),
            9.,
        );
        y += 22.;
        button(
            p,
            (
                &format!("{key}-enabled"),
                &format!("Contact {} enabled", c.id.0),
                "Enabled",
            ),
            Action::Enabled(c.id.0),
            (10., y, half),
            blocked,
            Field::Toggle(c.enabled),
            None,
        )?;
        button(
            p,
            (
                &format!("{key}-stop"),
                &format!("Contact {} stops motion", c.id.0),
                "Stop motion",
            ),
            Action::Stop(c.id.0),
            (16. + half, y, half),
            blocked,
            Field::Toggle(c.stop_motion),
            None,
        )?;
        y += 34.;
        p.text(
            &format!("{key}-clearance-label"),
            "CLEARANCE",
            (10., y, half, 18.),
            9.,
        );
        y += 20.;
        input(
            p,
            &format!("{key}-clearance"),
            &format!("Contact {} clearance", c.id.0),
            draft.clearance.text(),
            Edit::ContactClearance(c.id.0),
            (10., y, half),
            blocked,
        )?;
        let invalid = draft.name.trim().is_empty()
            || draft
                .clearance
                .evaluate(units, &[])
                .map_or(true, |v| v < 0.);
        button(
            p,
            (
                &format!("{key}-apply"),
                &format!("Apply contact {}", c.id.0),
                "Apply changes",
            ),
            Action::ApplyContact(c.id.0),
            (16. + half, y, half),
            blocked || invalid,
            Field::None,
            None,
        )?;
        y += 44.;
    }
    Ok(y)
}
