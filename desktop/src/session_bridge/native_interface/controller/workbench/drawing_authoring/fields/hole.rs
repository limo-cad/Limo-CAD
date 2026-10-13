use super::*;
pub(super) fn fields(annotation: &DrawingAnnotationDto) -> Vec<Field> {
    let DrawingAnnotationDto::HoleNote {
        position,
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
        return vec![];
    };
    let optional = |id, label, value: &Option<f64>| {
        field(
            id,
            label,
            Kind::Number,
            value.map(|n| n.to_string()).unwrap_or_default(),
        )
    };
    vec![
        field(Id::Quantity, "Quantity", Kind::Number, quantity),
        field(Id::Diameter, "Diameter (mm)", Kind::Number, diameter),
        optional(Id::Depth, "Depth (mm, optional)", depth),
        field(
            Id::ThroughAll,
            "Through hole",
            Kind::Toggle,
            depth.is_none()
                && through_all.unwrap_or_else(|| note.trim().eq_ignore_ascii_case("THRU")),
        ),
        field(
            Id::HoleStyle,
            "Hole style",
            Kind::Choice(Choice::HoleStyle),
            match hole_style {
                DrawingHoleStyle::Simple => "simple",
                DrawingHoleStyle::Counterbore => "counterbore",
                DrawingHoleStyle::Countersink => "countersink",
            },
        ),
        optional(
            Id::CounterboreDiameter,
            "Counterbore diameter (mm, optional)",
            counterbore_diameter,
        ),
        optional(
            Id::CounterboreDepth,
            "Counterbore depth (mm, optional)",
            counterbore_depth,
        ),
        optional(
            Id::CountersinkDiameter,
            "Countersink diameter (mm, optional)",
            countersink_diameter,
        ),
        optional(
            Id::CountersinkAngle,
            "Countersink angle (degrees, optional)",
            countersink_angle_deg,
        ),
        field(Id::Thread, "Thread designation", Kind::Text, thread),
        optional(Id::ThreadDepth, "Thread depth (mm, optional)", thread_depth),
        field(Id::PatternNote, "Pattern note", Kind::Text, pattern_note),
        field(Id::Note, "Additional note", Kind::Multiline, note),
        field(Id::X, "Paper X (mm)", Kind::Number, position[0]),
        field(Id::Y, "Paper Y (mm)", Kind::Number, position[1]),
    ]
}
fn optional(fields: &[Field], id: Id) -> Result<Option<f64>, String> {
    if text(fields, id)?.trim().is_empty() {
        Ok(None)
    } else {
        number(fields, id).map(Some)
    }
}
fn bounded_text(fields: &[Field], id: Id, limit: usize) -> Result<String, String> {
    let field = get(fields, id)?;
    if field.text.chars().take(limit + 1).count() > limit {
        return Err(format!(
            "{} must contain at most {limit} characters",
            field.label
        ));
    }
    Ok(field.text.clone())
}
pub(super) fn edited(
    annotation: &DrawingAnnotationDto,
    fields: &[Field],
) -> Result<DrawingAnnotationDto, String> {
    let mut next = annotation.clone();
    let DrawingAnnotationDto::HoleNote {
        position,
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
    } = &mut next
    else {
        return Err("Select a hole note".into());
    };
    *quantity = text(fields, Id::Quantity)?
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|n| (1..=10_000).contains(n))
        .ok_or("Quantity must be an integer from 1 to 10000")?;
    *diameter = number(fields, Id::Diameter)?;
    *depth = optional(fields, Id::Depth)?;
    if [Id::Depth, Id::ThroughAll]
        .iter()
        .any(|id| fields.iter().any(|f| f.id == *id && f.text != f.original))
    {
        *through_all = Some(boolean(fields, Id::ThroughAll)?);
    }
    *hole_style = match text(fields, Id::HoleStyle)? {
        "simple" => DrawingHoleStyle::Simple,
        "counterbore" => DrawingHoleStyle::Counterbore,
        "countersink" => DrawingHoleStyle::Countersink,
        _ => return Err("Choose a hole style".into()),
    };
    *counterbore_diameter = optional(fields, Id::CounterboreDiameter)?;
    *counterbore_depth = optional(fields, Id::CounterboreDepth)?;
    *countersink_diameter = optional(fields, Id::CountersinkDiameter)?;
    *countersink_angle_deg = optional(fields, Id::CountersinkAngle)?;
    *thread_depth = optional(fields, Id::ThreadDepth)?;
    *thread = bounded_text(fields, Id::Thread, 256)?;
    *pattern_note = bounded_text(fields, Id::PatternNote, 512)?;
    *note = bounded_text(fields, Id::Note, 4096)?;
    *position = [number(fields, Id::X)?, number(fields, Id::Y)?];
    Ok(next)
}
pub(in super::super) fn hole_preview(
    annotation: &DrawingAnnotationDto,
    fields: &[Field],
    units: limo_cad_core::UnitSystem,
    standard: DrawingStandard,
) -> Option<String> {
    if !matches!(annotation, DrawingAnnotationDto::HoleNote { .. }) {
        return None;
    }
    let next = edited(annotation, fields).ok()?;
    Some(limo_cad_occt::drawing_presentation::text::hole(
        &next, units, standard,
    ))
}
