//! Formatting of shared drawing intent, matching drawing/annotations.ts.
use limo_cad_core::UnitSystem;
use limo_cad_sketch::*;

/// Existing native paper-label sizing. Variation selectors add no advance.
pub fn width(text: &str, size: f64) -> f64 {
    let advance: f64 = text
        .chars()
        .map(|c| {
            if c == '\u{fe0e}' {
                0.
            } else if c == ' ' {
                0.34
            } else if "1ilI.,:;'|".contains(c) {
                0.32
            } else if "MW@%".contains(c) {
                0.86
            } else {
                0.58
            }
        })
        .sum();
    (size * 1.8).max(advance * size + 2.2)
}

/// Unrotated native label rectangle as [left, top, right, bottom] in paper mm.
pub fn label_bounds(baseline: [f64; 2], text: &str, size: f64, align: f64) -> [f64; 4] {
    let width = width(text, size);
    let height = size * 1.18 + 1.5;
    let x = baseline[0] + align * width * 0.5;
    let y = baseline[1] - size * 0.4;
    [
        x - width * 0.5,
        y - height * 0.5,
        x + width * 0.5,
        y + height * 0.5,
    ]
}

fn converted(value: f64, units: UnitSystem) -> f64 {
    match units {
        UnitSystem::Mm => value,
        UnitSystem::Cm => value / 10.,
        UnitSystem::In => value / 25.4,
    }
}
pub fn unit_label(units: UnitSystem) -> &'static str {
    match units {
        UnitSystem::Mm => "mm",
        UnitSystem::Cm => "cm",
        UnitSystem::In => "in",
    }
}
pub fn trim(value: f64, precision: usize) -> String {
    let text = format!("{value:.precision$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}
fn finish(mut text: String, presentation: &DrawingDimensionPresentationDto) -> String {
    if presentation.basic {
        text = format!("[{text}]");
    }
    if presentation.reference {
        text = format!("({text})");
    }
    text
}
pub fn dimension(
    value: f64,
    precision: u8,
    prefix: &str,
    suffix: &str,
    units: UnitSystem,
    p: &DrawingDimensionPresentationDto,
) -> String {
    let precision = usize::from(precision);
    let value_in_units = converted(value, units);
    let rounded = if value_in_units.abs() < 0.5 * 10_f64.powi(-(precision as i32)) {
        0.
    } else {
        value_in_units
    };
    let unit = unit_label(units);
    let mut text = format!("{rounded:.precision$} {unit}");
    let upper = converted(p.tolerance.upper, units);
    let lower = converted(p.tolerance.lower, units);
    match p.tolerance.mode {
        DrawingDimensionToleranceMode::None => {}
        DrawingDimensionToleranceMode::Symmetric => {
            text.push_str(&format!(" ±{:.precision$}", upper.abs()))
        }
        DrawingDimensionToleranceMode::Deviation => text.push_str(&format!(
            " +{:.precision$}/-{:.precision$}",
            upper.abs(),
            lower.abs()
        )),
        DrawingDimensionToleranceMode::Limits => {
            text = format!(
                "{:.precision$} / {:.precision$} {unit}",
                value_in_units + upper,
                value_in_units + lower
            )
        }
    }
    if !p.fit_class.trim().is_empty() {
        text.push_str(&format!(" {}", p.fit_class.trim()));
    }
    if let Some(dual) = &p.dual_units {
        let units = match dual.unit {
            DrawingSecondaryUnit::Millimetre => UnitSystem::Mm,
            DrawingSecondaryUnit::Centimetre => UnitSystem::Cm,
            DrawingSecondaryUnit::Inch => UnitSystem::In,
        };
        let value = converted(value, units);
        let precision = usize::from(dual.precision);
        let value = format!("{value:.precision$} {}", unit_label(units));
        text.push_str(&match dual.placement {
            DrawingDualUnitPlacement::Bracketed => format!(" [{value}]"),
            DrawingDualUnitPlacement::Stacked => format!(" / {value}"),
        });
    }
    format!("{prefix}{}{suffix}", finish(text, p))
}
pub fn angular(
    value: f64,
    precision: u8,
    prefix: &str,
    suffix: &str,
    p: &DrawingDimensionPresentationDto,
) -> String {
    let precision = usize::from(precision);
    let mut text = format!("{value:.precision$}°");
    match p.tolerance.mode {
        DrawingDimensionToleranceMode::None => {}
        DrawingDimensionToleranceMode::Symmetric => {
            text.push_str(&format!(" ±{:.precision$}°", p.tolerance.upper.abs()))
        }
        DrawingDimensionToleranceMode::Deviation => text.push_str(&format!(
            " +{:.precision$}°/-{:.precision$}°",
            p.tolerance.upper.abs(),
            p.tolerance.lower.abs()
        )),
        DrawingDimensionToleranceMode::Limits => {
            text = format!(
                "{:.precision$}° / {:.precision$}°",
                value + p.tolerance.upper,
                value + p.tolerance.lower
            )
        }
    }
    if !p.fit_class.trim().is_empty() {
        text.push_str(&format!(" {}", p.fit_class.trim()));
    }
    format!("{prefix}{}{suffix}", finish(text, p))
}
fn length(value: f64, units: UnitSystem, standard: DrawingStandard) -> String {
    let text = trim(
        converted(value, units),
        if units == UnitSystem::In { 3 } else { 2 },
    );
    if units == UnitSystem::In && standard == DrawingStandard::Ansi {
        if let Some(t) = text.strip_prefix("0.") {
            return format!(".{t}");
        }
        if let Some(t) = text.strip_prefix("-0.") {
            return format!("-.{t}");
        }
    }
    text
}
pub fn chamfer(
    value: f64,
    angle: f64,
    prefix: &str,
    units: UnitSystem,
    standard: DrawingStandard,
) -> String {
    format!(
        "{prefix}{} {} {}°",
        length(value, units, standard),
        if standard == DrawingStandard::Iso {
            "×"
        } else {
            "X"
        },
        trim(angle, 2)
    )
}
pub fn hole(
    annotation: &DrawingAnnotationDto,
    units: UnitSystem,
    standard: DrawingStandard,
) -> String {
    let DrawingAnnotationDto::HoleNote {
        quantity,
        diameter,
        depth,
        through_all,
        thread,
        note,
        hole_style,
        counterbore_diameter,
        counterbore_depth,
        countersink_diameter,
        countersink_angle_deg,
        thread_depth,
        pattern_note,
        ..
    } = annotation
    else {
        unreachable!()
    };
    let length = |v| length(v, units, standard);
    let through =
        depth.is_none() && through_all.unwrap_or_else(|| note.trim().eq_ignore_ascii_case("THRU"));
    let multiplier = if standard == DrawingStandard::Iso {
        "×"
    } else {
        "X"
    };
    let quantity_text = if *quantity > 1 {
        format!("{quantity}{multiplier} ")
    } else {
        String::new()
    };
    let mut lines = vec![if thread.trim().is_empty() {
        format!(
            "{quantity_text}⌀{}{}",
            length(*diameter),
            depth.map_or_else(
                || if through {
                    " THRU".into()
                } else {
                    String::new()
                },
                |v| format!(" ↧{}", length(v))
            )
        )
    } else {
        format!(
            "{quantity_text}{}{}",
            thread.trim(),
            thread_depth.map_or_else(
                || if through {
                    " THRU".into()
                } else {
                    String::new()
                },
                |v| format!(" ↧{}", length(v))
            )
        )
    }];
    if *hole_style == DrawingHoleStyle::Counterbore {
        if let Some(v) = counterbore_diameter {
            lines.push(format!(
                "⌴ ⌀{}{}",
                length(*v),
                counterbore_depth.map_or(String::new(), |d| format!(" ↧{}", length(d)))
            ));
        }
    }
    if *hole_style == DrawingHoleStyle::Countersink {
        if let Some(v) = countersink_diameter {
            lines.push(format!(
                "⌵ ⌀{}{}",
                length(*v),
                countersink_angle_deg
                    .map_or(String::new(), |a| format!(" {multiplier} {}°", trim(a, 2)))
            ));
        }
    }
    if !pattern_note.trim().is_empty() && pattern_note.trim() != format!("{quantity} HOLES") {
        lines.push(pattern_note.trim().into());
    }
    if !note.trim().is_empty() && !note.trim().eq_ignore_ascii_case("THRU") {
        lines.push(note.trim().into());
    }
    lines.join("\n")
}
fn modifier(value: DrawingMaterialCondition) -> &'static str {
    match value {
        DrawingMaterialCondition::None => "",
        DrawingMaterialCondition::Maximum => "Ⓜ\u{fe0e}",
        DrawingMaterialCondition::Least => "Ⓛ",
        DrawingMaterialCondition::Regardless => "Ⓢ",
    }
}
pub fn gdt_cells(annotation: &DrawingAnnotationDto) -> Vec<String> {
    let DrawingAnnotationDto::GdtFrame {
        characteristic,
        tolerance,
        diameter_zone,
        material_condition,
        datums,
        projected_zone,
        free_state,
        ..
    } = annotation
    else {
        unreachable!()
    };
    let symbol = match characteristic {
        DrawingGdtCharacteristic::Straightness => "—",
        DrawingGdtCharacteristic::Flatness => "▱",
        DrawingGdtCharacteristic::Circularity => "○",
        DrawingGdtCharacteristic::Cylindricity => "⌭",
        DrawingGdtCharacteristic::ProfileLine => "⌒",
        DrawingGdtCharacteristic::ProfileSurface => "⌓",
        DrawingGdtCharacteristic::Angularity => "∠",
        DrawingGdtCharacteristic::Perpendicularity => "⊥",
        DrawingGdtCharacteristic::Parallelism => "∥",
        DrawingGdtCharacteristic::Position => "⌖",
        DrawingGdtCharacteristic::Concentricity => "◎",
        DrawingGdtCharacteristic::Symmetry => "⌯",
        DrawingGdtCharacteristic::CircularRunout => "↗",
        DrawingGdtCharacteristic::TotalRunout => "↗↗",
    };
    let mut tolerance = format!(
        "{}{}",
        if *diameter_zone { "⌀" } else { "" },
        trim(*tolerance, 3)
    );
    let condition = modifier(*material_condition);
    if !condition.is_empty() {
        tolerance.push_str(&format!(" {condition}"));
    }
    let mut result = vec![symbol.into(), tolerance];
    for datum in datums {
        result.push(format!(
            "{}{}",
            datum.label,
            modifier(datum.material_condition)
        ));
    }
    if let Some(value) = projected_zone {
        result.push(format!("P{}", trim(*value, 3)));
    }
    if *free_state {
        result.push("F".into());
    }
    result
}
