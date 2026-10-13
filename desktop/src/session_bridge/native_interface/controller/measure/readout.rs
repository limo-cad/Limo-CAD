use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Guide {
    Distance,
    Centers,
    Maximum,
    Objects,
    Selection(usize),
    Delta(usize),
    None,
}
pub(super) struct Row {
    pub(super) label: String,
    pub(super) value: String,
    pub(super) guide: Guide,
}
fn scale(units: UnitSystem) -> (f64, &'static str) {
    match units {
        UnitSystem::Mm => (1., "mm"),
        UnitSystem::Cm => (10., "cm"),
        UnitSystem::In => (25.4, "in"),
    }
}
fn number(value: f64, precision: usize) -> String {
    let value = if value.abs() < 0.5 * 10f64.powi(-(precision as i32)) {
        0.
    } else {
        value
    };
    format!("{value:.precision$}")
}
pub(super) fn format_value(value: f64, power: i32, units: UnitSystem, state: &State) -> String {
    let precision = state.precision.parse().unwrap_or(3);
    if power == 0 {
        return format!("{}°", number(value, precision));
    }
    let suffix = match power {
        2 => "²",
        3 => "³",
        _ => "",
    };
    let (factor, unit) = scale(units);
    let mut text = format!(
        "{} {unit}{suffix}",
        number(value / factor.powi(power), precision)
    );
    let secondary = match state.secondary.as_str() {
        "mm" => Some(UnitSystem::Mm),
        "cm" => Some(UnitSystem::Cm),
        "in" => Some(UnitSystem::In),
        _ => None,
    };
    if let Some(secondary) = secondary.filter(|s| *s != units) {
        let (factor, unit) = scale(secondary);
        text.push_str(&format!(
            " ({} {unit}{suffix})",
            number(value / factor.powi(power), precision)
        ));
    }
    text
}
pub(super) fn rows(state: &State, units: UnitSystem) -> Vec<Row> {
    let Some(report) = &state.report else {
        return vec![];
    };
    let mut rows = vec![];
    let mut add = |label: &str, value: Option<f64>, power, guide| {
        if let Some(value) = value.filter(|v| v.is_finite()) {
            rows.push(Row {
                label: label.into(),
                value: format_value(value, power, units, state),
                guide,
            });
        }
    };
    add("Minimum distance", report.distance, 1, Guide::Distance);
    add("Angle", report.angle_degrees, 0, Guide::Objects);
    add("Center distance", report.center_distance, 1, Guide::Centers);
    add(
        "Maximum distance",
        report.maximum_distance,
        1,
        Guide::Maximum,
    );
    if state.delta != "none" {
        if let Some([a, b]) = report.closest_points {
            let delta =
                state.delta_rotation().inverse() * (DVec3::from_array(b) - DVec3::from_array(a));
            for (i, label) in ["Δ X", "Δ Y", "Δ Z"].iter().enumerate() {
                add(label, Some(delta[i]), 1, Guide::Delta(i));
            }
        }
    }
    for (index, p) in report.selections.iter().enumerate() {
        let kind = match state.selections[index].target {
            MeasurementTarget::Body => "Body",
            MeasurementTarget::Face { .. } => "Face",
            MeasurementTarget::Edge { .. } => "Edge",
            MeasurementTarget::Vertex { .. } => "Vertex",
            MeasurementTarget::Center { .. } => "Center",
        };
        rows.push(Row {
            label: format!("Selection {} · {kind}", index + 1),
            value: String::new(),
            guide: Guide::None,
        });
        let mut add = |label: &str, value: Option<f64>, power| {
            if let Some(value) = value.filter(|v| v.is_finite()) {
                rows.push(Row {
                    label: label.into(),
                    value: format_value(value, power, units, state),
                    guide: Guide::Selection(index),
                });
            }
        };
        add("Length", p.length, 1);
        add(
            if matches!(state.selections[index].target, MeasurementTarget::Body) {
                "Surface area"
            } else {
                "Area"
            },
            p.area,
            2,
        );
        add("Loop length", p.loop_length, 1);
        add("Volume", p.volume, 3);
        add("Radius", p.radius, 1);
        add("Diameter", p.radius.map(|r| r * 2.), 1);
        for (position, prefix) in [
            (p.position, "Position"),
            (p.center, "Center"),
            (p.size, "Size"),
        ] {
            if let Some(position) = position {
                for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
                    add(&format!("{prefix} {axis}"), Some(position[i]), 1);
                }
            }
        }
    }
    rows
}
