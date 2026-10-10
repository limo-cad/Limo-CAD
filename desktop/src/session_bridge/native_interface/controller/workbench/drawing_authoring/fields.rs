//! Native inspector strings are transient; values are applied to typed shared
//! annotations and validated by DrawingDocumentDto before a commit.
use super::{draft::Draft, *};
use limo_cad_interface::{ChoiceOption, ControlInput};
use limo_cad_sketch::*;
mod hole;
mod technical;
pub(super) use hole::hole_preview;

pub(super) fn bom_options(document: &DrawingDocumentDto, sheet_id: u64) -> Vec<ChoiceOption> {
    document
        .sheets
        .iter()
        .find(|s| s.id == sheet_id)
        .into_iter()
        .flat_map(|s| &s.bom)
        .map(|item| ChoiceOption {
            value: item.id.to_string(),
            label: format!(
                "{} · {}",
                item.item_number.chars().take(32).collect::<String>(),
                item.description.chars().take(80).collect::<String>()
            ),
            disabled: false,
        })
        .collect()
}

/// Identity is independent of pagination and conditional presentation fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Id {
    Technical(&'static str),
    Note,
    Quantity,
    Diameter,
    Depth,
    ThroughAll,
    HoleStyle,
    CounterboreDiameter,
    CounterboreDepth,
    CountersinkDiameter,
    CountersinkAngle,
    Thread,
    ThreadDepth,
    PatternNote,
    Revision,
    X,
    Y,
    Mode,
    Layout,
    Spacing,
    Axis,
    Offset,
    Extension,
    LeaderAngle,
    ArcRadius,
    ChamferSetback,
    ChamferAngle,
    Precision,
    Prefix,
    Suffix,
    Tolerance,
    Upper,
    Lower,
    Basic,
    Reference,
    Fit,
    Dual,
    DualUnit,
    DualPrecision,
    DualPlacement,
}
#[derive(Clone, Copy)]
pub(super) enum Choice {
    Options(&'static [(&'static str, &'static str)]),
    HoleStyle,
    Mode,
    Layout,
    Axis,
    Radial(bool),
    Tolerance,
    Unit,
    Placement,
}
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Text,
    Multiline,
    Number,
    Choice(Choice),
    Toggle,
}
pub(super) struct Field {
    pub id: Id,
    pub label: &'static str,
    pub kind: Kind,
    pub text: String,
    pub original: String,
}
impl Field {
    pub fn options(&self) -> Option<Vec<ChoiceOption>> {
        let pairs: &[(&str, &str)] = match self.kind {
            Kind::Choice(Choice::Options(options)) => options,
            Kind::Choice(Choice::HoleStyle) => &[
                ("simple", "Simple"),
                ("counterbore", "Counterbore"),
                ("countersink", "Countersink"),
            ],
            Kind::Choice(Choice::Mode) => &[
                ("aligned", "Aligned"),
                ("horizontal", "Horizontal"),
                ("vertical", "Vertical"),
            ],
            Kind::Choice(Choice::Layout) => &[
                ("chain", "Chain"),
                ("baseline", "Baseline"),
                ("continued", "Continued"),
            ],
            Kind::Choice(Choice::Axis) => &[("both", "X and Y"), ("x", "X"), ("y", "Y")],
            Kind::Choice(Choice::Radial(_)) => &[("diameter", "Diameter"), ("radius", "Radius")],
            Kind::Choice(Choice::Tolerance) => &[
                ("none", "None"),
                ("symmetric", "Plus/minus"),
                ("deviation", "Unequal deviation"),
                ("limits", "Limit dimensions"),
            ],
            Kind::Choice(Choice::Unit) => &[
                ("millimetre", "Millimetre"),
                ("centimetre", "Centimetre"),
                ("inch", "Inch"),
            ],
            Kind::Choice(Choice::Placement) => {
                &[("bracketed", "Bracketed"), ("stacked", "Stacked")]
            }
            _ => return None,
        };
        Some(
            pairs
                .iter()
                .map(|(value, label)| ChoiceOption {
                    value: (*value).into(),
                    label: (*label).into(),
                    disabled: matches!(self.kind, Kind::Choice(Choice::Radial(false)))
                        && *value == "diameter",
                })
                .collect(),
        )
    }
    pub fn caption(&self) -> String {
        if matches!(self.kind, Kind::Toggle) {
            return self.label.into();
        }
        self.options()
            .and_then(|options| {
                options
                    .into_iter()
                    .find(|o| o.value == self.text)
                    .map(|o| o.label)
            })
            .unwrap_or_else(|| self.text.clone())
    }
}
fn field(id: Id, label: &'static str, kind: Kind, value: impl ToString) -> Field {
    let text = value.to_string();
    Field {
        id,
        label,
        kind,
        original: text.clone(),
        text,
    }
}
pub(super) fn note_creation(position: [f64; 2]) -> Vec<Field> {
    vec![
        field(Id::Note, "Note text", Kind::Multiline, "NOTE"),
        field(Id::X, "Paper X (mm)", Kind::Number, position[0]),
        field(Id::Y, "Paper Y (mm)", Kind::Number, position[1]),
    ]
}
pub(super) fn from_annotation(annotation: &DrawingAnnotationDto) -> Vec<Field> {
    match annotation {
        DrawingAnnotationDto::HoleNote { .. } => hole::fields(annotation),
        DrawingAnnotationDto::CenterMark { extension, .. }
        | DrawingAnnotationDto::CenterLine { extension, .. } => vec![field(
            Id::Extension,
            "Extension (paper mm)",
            Kind::Number,
            extension,
        )],
        DrawingAnnotationDto::RevisionCloud { revision, .. } => {
            vec![field(Id::Revision, "Revision", Kind::Text, revision)]
        }
        DrawingAnnotationDto::ChamferNote {
            position,
            length,
            angle_deg,
            prefix,
            ..
        } => vec![
            field(
                Id::ChamferSetback,
                "Chamfer setback (mm)",
                Kind::Number,
                length,
            ),
            field(
                Id::ChamferAngle,
                "Chamfer angle (degrees)",
                Kind::Number,
                angle_deg,
            ),
            field(Id::Prefix, "Prefix", Kind::Text, prefix),
            field(Id::X, "Paper X (mm)", Kind::Number, position[0]),
            field(Id::Y, "Paper Y (mm)", Kind::Number, position[1]),
        ],
        DrawingAnnotationDto::Note { text, position, .. } => {
            let mut fields = note_creation(*position);
            fields[0].text = text.clone();
            fields[0].original = text.clone();
            fields
        }
        DrawingAnnotationDto::LinearDimension {
            mode,
            offset,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        } => {
            let mut fields = vec![
                field(
                    Id::Mode,
                    "Dimension mode",
                    Kind::Choice(Choice::Mode),
                    match mode {
                        DrawingLinearDimensionMode::Aligned => "aligned",
                        DrawingLinearDimensionMode::Horizontal => "horizontal",
                        DrawingLinearDimensionMode::Vertical => "vertical",
                    },
                ),
                field(Id::Offset, "Offset (paper mm)", Kind::Number, offset),
                field(Id::Precision, "Precision", Kind::Number, precision),
                field(Id::Prefix, "Prefix", Kind::Text, prefix),
                field(Id::Suffix, "Suffix", Kind::Text, suffix),
            ];
            dimension_fields(&mut fields, presentation);
            fields
        }
        DrawingAnnotationDto::LineDimension {
            position,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        }
        | DrawingAnnotationDto::PointLineDimension {
            position,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        } => {
            let mut fields = vec![
                field(Id::X, "Paper X (mm)", Kind::Number, position[0]),
                field(Id::Y, "Paper Y (mm)", Kind::Number, position[1]),
            ];
            text_fields(&mut fields, *precision, prefix, suffix, presentation);
            fields
        }
        DrawingAnnotationDto::ChainDimension {
            layout,
            mode,
            offset,
            spacing,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        } => {
            let mut fields = vec![
                field(
                    Id::Layout,
                    "Layout",
                    Kind::Choice(Choice::Layout),
                    match layout {
                        DrawingChainDimensionLayout::Chain => "chain",
                        DrawingChainDimensionLayout::Baseline => "baseline",
                        DrawingChainDimensionLayout::Continued => "continued",
                    },
                ),
                field(
                    Id::Mode,
                    "Dimension mode",
                    Kind::Choice(Choice::Mode),
                    match mode {
                        DrawingLinearDimensionMode::Aligned => "aligned",
                        DrawingLinearDimensionMode::Horizontal => "horizontal",
                        DrawingLinearDimensionMode::Vertical => "vertical",
                    },
                ),
                field(Id::Offset, "Offset (paper mm)", Kind::Number, offset),
                field(Id::Spacing, "Baseline spacing (mm)", Kind::Number, spacing),
            ];
            text_fields(&mut fields, *precision, prefix, suffix, presentation);
            fields
        }
        DrawingAnnotationDto::OrdinateDimension {
            axis,
            offset,
            precision,
            presentation,
            ..
        } => {
            let mut fields = vec![
                field(
                    Id::Axis,
                    "Axis",
                    Kind::Choice(Choice::Axis),
                    match axis {
                        DrawingOrdinateAxis::X => "x",
                        DrawingOrdinateAxis::Y => "y",
                        DrawingOrdinateAxis::Both => "both",
                    },
                ),
                field(Id::Offset, "Leader offset (paper mm)", Kind::Number, offset),
                field(Id::Precision, "Precision", Kind::Number, precision),
            ];
            dimension_fields(&mut fields, presentation);
            fields
        }
        DrawingAnnotationDto::RadialDimension {
            feature,
            mode,
            leader_angle_deg,
            offset,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        } => {
            let mut fields = vec![
                field(
                    Id::Mode,
                    "Dimension type",
                    Kind::Choice(Choice::Radial(feature.closed)),
                    match mode {
                        DrawingRadialDimensionMode::Diameter => "diameter",
                        DrawingRadialDimensionMode::Radius => "radius",
                    },
                ),
                field(
                    Id::LeaderAngle,
                    "Leader angle (degrees)",
                    Kind::Number,
                    leader_angle_deg,
                ),
                field(Id::Offset, "Leader offset (paper mm)", Kind::Number, offset),
            ];
            text_fields(&mut fields, *precision, prefix, suffix, presentation);
            fields
        }
        DrawingAnnotationDto::AngularDimension {
            radius,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        } => {
            let mut fields = vec![field(
                Id::ArcRadius,
                "Arc radius (paper mm)",
                Kind::Number,
                radius,
            )];
            text_fields(&mut fields, *precision, prefix, suffix, presentation);
            fields
        }
        _ => technical::fields(annotation),
    }
}
fn text_fields(
    fields: &mut Vec<Field>,
    precision: u8,
    prefix: &str,
    suffix: &str,
    presentation: &DrawingDimensionPresentationDto,
) {
    fields.extend([
        field(Id::Precision, "Precision", Kind::Number, precision),
        field(Id::Prefix, "Prefix", Kind::Text, prefix),
        field(Id::Suffix, "Suffix", Kind::Text, suffix),
    ]);
    dimension_fields(fields, presentation);
}
fn dimension_fields(fields: &mut Vec<Field>, p: &DrawingDimensionPresentationDto) {
    let dual = p.dual_units.clone().unwrap_or(DrawingDualUnitDto {
        unit: DrawingSecondaryUnit::Inch,
        precision: 3,
        placement: DrawingDualUnitPlacement::Bracketed,
    });
    fields.extend([
        field(
            Id::Tolerance,
            "Tolerance mode",
            Kind::Choice(Choice::Tolerance),
            match p.tolerance.mode {
                DrawingDimensionToleranceMode::None => "none",
                DrawingDimensionToleranceMode::Symmetric => "symmetric",
                DrawingDimensionToleranceMode::Deviation => "deviation",
                DrawingDimensionToleranceMode::Limits => "limits",
            },
        ),
        field(
            Id::Upper,
            "Upper tolerance",
            Kind::Number,
            p.tolerance.upper,
        ),
        field(
            Id::Lower,
            "Lower tolerance",
            Kind::Number,
            p.tolerance.lower,
        ),
        field(Id::Basic, "Basic dimension", Kind::Toggle, p.basic),
        field(
            Id::Reference,
            "Reference dimension",
            Kind::Toggle,
            p.reference,
        ),
        field(Id::Fit, "Fit class", Kind::Text, &p.fit_class),
        field(Id::Dual, "Dual units", Kind::Toggle, p.dual_units.is_some()),
        field(
            Id::DualUnit,
            "Secondary unit",
            Kind::Choice(Choice::Unit),
            match dual.unit {
                DrawingSecondaryUnit::Millimetre => "millimetre",
                DrawingSecondaryUnit::Centimetre => "centimetre",
                DrawingSecondaryUnit::Inch => "inch",
            },
        ),
        field(
            Id::DualPrecision,
            "Dual precision",
            Kind::Number,
            dual.precision,
        ),
        field(
            Id::DualPlacement,
            "Dual placement",
            Kind::Choice(Choice::Placement),
            match dual.placement {
                DrawingDualUnitPlacement::Bracketed => "bracketed",
                DrawingDualUnitPlacement::Stacked => "stacked",
            },
        ),
    ]);
}
pub(super) fn dirty(fields: &[Field]) -> bool {
    fields.iter().any(|f| f.text != f.original)
}
fn get(fields: &[Field], id: Id) -> Result<&Field, String> {
    fields
        .iter()
        .find(|f| f.id == id)
        .ok_or("Drawing field was removed".into())
}
fn text(fields: &[Field], id: Id) -> Result<&str, String> {
    Ok(&get(fields, id)?.text)
}
pub(super) fn visible(fields: &[Field]) -> Vec<usize> {
    let tolerance = text(fields, Id::Tolerance).is_ok_and(|v| v != "none");
    let dual = text(fields, Id::Dual) == Ok("true");
    let hole_style = text(fields, Id::HoleStyle).ok();
    fields
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            let edited = f.text != f.original;
            match f.id {
                Id::CounterboreDiameter | Id::CounterboreDepth => {
                    hole_style == Some("counterbore") || edited
                }
                Id::CountersinkDiameter | Id::CountersinkAngle => {
                    hole_style == Some("countersink") || edited
                }
                Id::Upper | Id::Lower => tolerance || edited,
                Id::DualUnit | Id::DualPrecision | Id::DualPlacement => dual || edited,
                _ => true,
            }
        })
        .map(|(i, _)| i)
        .collect()
}
pub(super) fn edit(fields: &mut [Field], id: Id, input: &ControlInput) -> Result<bool, String> {
    let index = fields
        .iter()
        .position(|f| f.id == id)
        .ok_or("Drawing field was removed")?;
    if !visible(fields).contains(&index) {
        return Err("Drawing field is hidden; use the refreshed controls".into());
    }
    let f = &fields[index];
    let activation = super::super::super::super::is_activation(input);
    let next = match f.kind {
        Kind::Choice(_) => {
            let navigation = matches!(input, ControlInput::Key(k) if !k.ctrl && !k.meta && !k.alt && !k.shift && matches!(k.key.as_str(), "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" | "Home" | "End"));
            if !activation && !navigation && !matches!(input, ControlInput::SetValue(_)) {
                return Ok(false);
            }
            super::super::cam::choose(&f.options().unwrap(), &f.text, input)
                .map_err(|_| format!("Choose an available value for {}", f.label))?
        }
        Kind::Toggle => match input {
            ControlInput::SetValue(value) => value
                .parse::<bool>()
                .map_err(|_| format!("Choose true or false for {}", f.label))?
                .to_string(),
            _ if activation => (!boolean(fields, id)?).to_string(),
            _ => return Ok(false),
        },
        _ => match input {
            ControlInput::SetValue(value) => value.clone(),
            _ => return Ok(false),
        },
    };
    let next = if id == Id::Revision {
        next.to_uppercase()
    } else {
        next
    };
    let mut changed = fields[index].text != next;
    fields[index].text = next;
    let hole_other = match id {
        Id::ThroughAll if fields[index].text == "true" => Some((Id::Depth, "")),
        Id::Depth if !fields[index].text.trim().is_empty() => Some((Id::ThroughAll, "false")),
        _ => None,
    };
    if let Some((other, value)) = hole_other {
        let field = fields
            .iter_mut()
            .find(|f| f.id == other)
            .ok_or("Hole extent field was removed")?;
        changed |= field.text != value;
        field.text = value.into();
    }
    if matches!(id, Id::Basic | Id::Reference) && fields[index].text == "true" {
        let other = if id == Id::Basic {
            Id::Reference
        } else {
            Id::Basic
        };
        let field = fields
            .iter_mut()
            .find(|f| f.id == other)
            .ok_or("Dimension field was removed")?;
        changed |= field.text != "false";
        field.text = "false".into();
    }
    Ok(changed)
}
fn number(fields: &[Field], id: Id) -> Result<f64, String> {
    let f = get(fields, id)?;
    let value = f
        .text
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("Enter a number for {}", f.label))?;
    if !value.is_finite() {
        return Err(format!("{} must be finite", f.label));
    }
    Ok(value)
}
fn precision(fields: &[Field], id: Id) -> Result<u8, String> {
    let f = get(fields, id)?;
    f.text
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|n| *n <= 6)
        .ok_or_else(|| format!("{} must be an integer from 0 to 6", f.label))
}
fn boolean(fields: &[Field], id: Id) -> Result<bool, String> {
    let f = get(fields, id)?;
    f.text
        .parse()
        .map_err(|_| format!("Choose true or false for {}", f.label))
}
fn presentation(fields: &[Field]) -> Result<DrawingDimensionPresentationDto, String> {
    let mode = match text(fields, Id::Tolerance)? {
        "none" => DrawingDimensionToleranceMode::None,
        "symmetric" => DrawingDimensionToleranceMode::Symmetric,
        "deviation" => DrawingDimensionToleranceMode::Deviation,
        "limits" => DrawingDimensionToleranceMode::Limits,
        _ => return Err("Choose a tolerance mode".into()),
    };
    let fit_class = text(fields, Id::Fit)?.to_owned();
    if fit_class.chars().count() > 64 {
        return Err("Fit class must contain at most 64 characters".into());
    }
    let dual_units = if boolean(fields, Id::Dual)? {
        Some(DrawingDualUnitDto {
            unit: match text(fields, Id::DualUnit)? {
                "millimetre" => DrawingSecondaryUnit::Millimetre,
                "centimetre" => DrawingSecondaryUnit::Centimetre,
                "inch" => DrawingSecondaryUnit::Inch,
                _ => return Err("Choose a secondary unit".into()),
            },
            precision: precision(fields, Id::DualPrecision)?,
            placement: match text(fields, Id::DualPlacement)? {
                "bracketed" => DrawingDualUnitPlacement::Bracketed,
                "stacked" => DrawingDualUnitPlacement::Stacked,
                _ => return Err("Choose dual-unit placement".into()),
            },
        })
    } else {
        None
    };
    Ok(DrawingDimensionPresentationDto {
        tolerance: DrawingDimensionToleranceDto {
            mode,
            upper: number(fields, Id::Upper)?,
            lower: number(fields, Id::Lower)?,
        },
        basic: boolean(fields, Id::Basic)?,
        reference: boolean(fields, Id::Reference)?,
        fit_class,
        dual_units,
    })
}
pub(super) fn note_request(sheet_id: u64, fields: &[Field]) -> Result<AddNote, String> {
    Ok(AddNote {
        sheet_id,
        text: text(fields, Id::Note)?.to_owned(),
        position: [number(fields, Id::X)?, number(fields, Id::Y)?],
    })
}
pub(super) fn apply(draft: &mut Draft, fields: &[Field]) -> Result<(), String> {
    match draft.annotation() {
        DrawingAnnotationDto::HoleNote { .. } => {
            draft.hole_note(hole::edited(draft.annotation(), fields)?)?
        }
        DrawingAnnotationDto::CenterMark { .. } | DrawingAnnotationDto::CenterLine { .. } => {
            draft.center_extension(number(fields, Id::Extension)?)?
        }
        DrawingAnnotationDto::RevisionCloud { .. } => {
            draft.revision_cloud(text(fields, Id::Revision)?.into())?
        }
        DrawingAnnotationDto::ChamferNote { .. } => draft.chamfer(
            [number(fields, Id::X)?, number(fields, Id::Y)?],
            number(fields, Id::ChamferSetback)?,
            number(fields, Id::ChamferAngle)?,
            text(fields, Id::Prefix)?.into(),
        )?,
        DrawingAnnotationDto::Note { .. } => {
            let note = note_request(0, fields)?;
            draft.note(note.text)?;
            draft.set_note_position(note.position)?;
        }
        DrawingAnnotationDto::LinearDimension { .. } => {
            let mode = match text(fields, Id::Mode)? {
                "aligned" => DrawingLinearDimensionMode::Aligned,
                "horizontal" => DrawingLinearDimensionMode::Horizontal,
                "vertical" => DrawingLinearDimensionMode::Vertical,
                _ => return Err("Choose a dimension mode".into()),
            };
            draft.linear(
                mode,
                number(fields, Id::Offset)?,
                precision(fields, Id::Precision)?,
                text(fields, Id::Prefix)?.into(),
                text(fields, Id::Suffix)?.into(),
                presentation(fields)?,
            )?;
        }
        DrawingAnnotationDto::LineDimension { .. }
        | DrawingAnnotationDto::PointLineDimension { .. } => {
            draft.straight(
                [number(fields, Id::X)?, number(fields, Id::Y)?],
                precision(fields, Id::Precision)?,
                text(fields, Id::Prefix)?.into(),
                text(fields, Id::Suffix)?.into(),
                presentation(fields)?,
            )?;
        }
        DrawingAnnotationDto::ChainDimension { .. } => {
            let layout = match text(fields, Id::Layout)? {
                "chain" => DrawingChainDimensionLayout::Chain,
                "baseline" => DrawingChainDimensionLayout::Baseline,
                "continued" => DrawingChainDimensionLayout::Continued,
                _ => return Err("Choose a dimension layout".into()),
            };
            let mode = match text(fields, Id::Mode)? {
                "aligned" => DrawingLinearDimensionMode::Aligned,
                "horizontal" => DrawingLinearDimensionMode::Horizontal,
                "vertical" => DrawingLinearDimensionMode::Vertical,
                _ => return Err("Choose a dimension mode".into()),
            };
            draft.series(
                layout,
                mode,
                number(fields, Id::Offset)?,
                number(fields, Id::Spacing)?,
                (
                    precision(fields, Id::Precision)?,
                    text(fields, Id::Prefix)?.into(),
                    text(fields, Id::Suffix)?.into(),
                    presentation(fields)?,
                ),
            )?;
        }
        DrawingAnnotationDto::OrdinateDimension { .. } => {
            let axis = match text(fields, Id::Axis)? {
                "both" => DrawingOrdinateAxis::Both,
                "x" => DrawingOrdinateAxis::X,
                "y" => DrawingOrdinateAxis::Y,
                _ => return Err("Choose an ordinate axis".into()),
            };
            draft.ordinate(
                axis,
                number(fields, Id::Offset)?,
                precision(fields, Id::Precision)?,
                presentation(fields)?,
            )?;
        }
        DrawingAnnotationDto::RadialDimension { .. } => {
            let mode = match text(fields, Id::Mode)? {
                "diameter" => DrawingRadialDimensionMode::Diameter,
                "radius" => DrawingRadialDimensionMode::Radius,
                _ => return Err("Choose a radial dimension type".into()),
            };
            draft.radial(
                mode,
                number(fields, Id::LeaderAngle)?,
                number(fields, Id::Offset)?,
                (
                    precision(fields, Id::Precision)?,
                    text(fields, Id::Prefix)?.into(),
                    text(fields, Id::Suffix)?.into(),
                    presentation(fields)?,
                ),
            )?;
        }
        DrawingAnnotationDto::AngularDimension { .. } => {
            draft.angular(
                number(fields, Id::ArcRadius)?,
                precision(fields, Id::Precision)?,
                text(fields, Id::Prefix)?.into(),
                text(fields, Id::Suffix)?.into(),
                presentation(fields)?,
            )?;
        }
        _ => technical::apply(draft, fields)?,
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests;
