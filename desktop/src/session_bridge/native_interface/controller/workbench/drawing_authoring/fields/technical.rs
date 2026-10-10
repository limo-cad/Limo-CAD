//! Inspector descriptors for the existing shared annotation DTO. Geometry and
//! identity are deliberately absent: editing text cannot rewrite associations.
use super::*;
use serde_json::{json, Value};

const MATERIAL: &[(&str, &str)] = &[
    ("none", "None"),
    ("maximum", "Maximum"),
    ("least", "Least"),
    ("regardless", "Regardless"),
];
const CHARACTERISTIC: &[(&str, &str)] = &[
    ("straightness", "Straightness"),
    ("flatness", "Flatness"),
    ("circularity", "Circularity"),
    ("cylindricity", "Cylindricity"),
    ("profile_line", "Profile of line"),
    ("profile_surface", "Profile of surface"),
    ("angularity", "Angularity"),
    ("perpendicularity", "Perpendicularity"),
    ("parallelism", "Parallelism"),
    ("position", "Position"),
    ("concentricity", "Concentricity"),
    ("symmetry", "Symmetry"),
    ("circular_runout", "Circular runout"),
    ("total_runout", "Total runout"),
];
const LAY: &[(&str, &str)] = &[
    ("none", "None"),
    ("parallel", "Parallel"),
    ("perpendicular", "Perpendicular"),
    ("crossed", "Crossed"),
    ("multidirectional", "Multidirectional"),
    ("circular", "Circular"),
    ("radial", "Radial"),
    ("particulate", "Particulate"),
];
const WELD: &[(&str, &str)] = &[
    ("fillet", "Fillet"),
    ("square_groove", "Square groove"),
    ("v_groove", "V groove"),
    ("bevel_groove", "Bevel groove"),
    ("u_groove", "U groove"),
    ("j_groove", "J groove"),
    ("plug_slot", "Plug / slot"),
    ("spot", "Spot"),
    ("seam", "Seam"),
    ("surfacing", "Surfacing"),
];
type Descriptor = (&'static str, &'static str, Kind);
fn descriptors(a: &DrawingAnnotationDto) -> Vec<Descriptor> {
    use DrawingAnnotationDto::*;
    use Kind::*;
    let mut fields = match a {
        CenterLineBetweenEdges { .. } | BoltCircleCenterLine { .. } => {
            vec![("/extension", "Extension (paper mm)", Number)]
        }
        AutomaticSymmetryAxis { .. } => vec![
            ("/axis", "Axes", Choice(super::Choice::Axis)),
            ("/extension", "Extension (paper mm)", Number),
        ],
        ArcLengthDimension { .. } => vec![("/offset", "Offset (paper mm)", Number)],
        JoggedRadiusDimension { .. } => vec![
            ("/jog/0", "Jog X (paper mm)", Number),
            ("/jog/1", "Jog Y (paper mm)", Number),
        ],
        DatumFeature { .. } => vec![
            ("/label", "Datum label", Text),
            ("/target_index", "Target index (blank for none)", Number),
        ],
        GdtFrame { .. } => vec![
            (
                "/characteristic",
                "Characteristic",
                Choice(super::Choice::Options(CHARACTERISTIC)),
            ),
            ("/tolerance", "Tolerance", Number),
            ("/diameter_zone", "Diameter zone", Toggle),
            (
                "/material_condition",
                "Material condition",
                Choice(super::Choice::Options(MATERIAL)),
            ),
            ("/projected_zone", "Projected zone (blank for none)", Number),
            ("/free_state", "Free state", Toggle),
        ],
        SurfaceTexture { .. } => vec![
            ("/roughness_ra", "Roughness Ra", Number),
            ("/process", "Process", Text),
            ("/lay", "Lay", Choice(super::Choice::Options(LAY))),
            (
                "/machining_allowance",
                "Machining allowance (blank for none)",
                Number,
            ),
        ],
        EdgeRequirement { .. } => vec![
            ("/upper_deviation", "Upper deviation", Number),
            ("/lower_deviation", "Lower deviation", Number),
            ("/note", "Note", Multiline),
        ],
        WeldSymbol { .. } => vec![
            (
                "/weld_type",
                "Weld type",
                Choice(super::Choice::Options(WELD)),
            ),
            (
                "/side",
                "Side",
                Choice(super::Choice::Options(&[
                    ("arrow", "Arrow"),
                    ("other", "Other"),
                    ("both", "Both"),
                ])),
            ),
            ("/size", "Size", Number),
            ("/length", "Length (blank for none)", Number),
            ("/pitch", "Pitch (blank for none)", Number),
            (
                "/contour",
                "Contour",
                Choice(super::Choice::Options(&[
                    ("none", "None"),
                    ("flush", "Flush"),
                    ("convex", "Convex"),
                    ("concave", "Concave"),
                ])),
            ),
            ("/finish", "Finish", Text),
            ("/all_around", "All around", Toggle),
            ("/field_weld", "Field weld", Toggle),
            ("/tail", "Tail", Multiline),
        ],
        ItemBalloon { .. } => vec![("/bom_item_id", "BOM item", Number)],
        _ => vec![],
    };
    if matches!(
        a,
        JoggedRadiusDimension { .. }
            | DatumFeature { .. }
            | GdtFrame { .. }
            | SurfaceTexture { .. }
            | EdgeRequirement { .. }
            | WeldSymbol { .. }
            | ItemBalloon { .. }
    ) {
        fields.extend([
            ("/position/0", "Paper X (mm)", Number),
            ("/position/1", "Paper Y (mm)", Number),
        ]);
    }
    fields
}
const DATUM_LABELS: [&str; 3] = ["/datums/0/label", "/datums/1/label", "/datums/2/label"];
const DATUM_CONDITIONS: [&str; 3] = [
    "/datums/0/material_condition",
    "/datums/1/material_condition",
    "/datums/2/material_condition",
];
pub(super) fn fields(a: &DrawingAnnotationDto) -> Vec<Field> {
    let record = serde_json::to_value(a).unwrap_or(Value::Null);
    let mut fields: Vec<_> = descriptors(a)
        .into_iter()
        .map(|(path, label, kind)| {
            let value = record.pointer(path).unwrap_or(&Value::Null);
            let value = match value {
                Value::Null => String::new(),
                Value::String(s) => s.clone(),
                _ => value.to_string(),
            };
            field(Id::Technical(path), label, kind, value)
        })
        .collect();
    match a {
        DrawingAnnotationDto::ArcLengthDimension {
            precision,
            presentation,
            ..
        }
        | DrawingAnnotationDto::JoggedRadiusDimension {
            precision,
            presentation,
            ..
        } => {
            fields.push(field(Id::Precision, "Precision", Kind::Number, precision));
            dimension_fields(&mut fields, presentation);
        }
        DrawingAnnotationDto::GdtFrame { datums, .. } => {
            for i in 0..3 {
                fields.push(field(
                    Id::Technical(DATUM_LABELS[i]),
                    [
                        "Primary datum (blank for none)",
                        "Secondary datum (blank for none)",
                        "Tertiary datum (blank for none)",
                    ][i],
                    Kind::Text,
                    datums.get(i).map_or("", |d| d.label.as_str()),
                ));
                let condition = datums
                    .get(i)
                    .map_or(DrawingMaterialCondition::None, |d| d.material_condition);
                let value = serde_json::to_value(condition).unwrap_or(json!("none"));
                fields.push(field(
                    Id::Technical(DATUM_CONDITIONS[i]),
                    [
                        "Primary datum condition",
                        "Secondary datum condition",
                        "Tertiary datum condition",
                    ][i],
                    Kind::Choice(Choice::Options(MATERIAL)),
                    value.as_str().unwrap_or("none"),
                ));
            }
        }
        _ => {}
    }
    fields
}
pub(super) fn apply(draft: &mut Draft, fields: &[Field]) -> Result<(), String> {
    let a = draft.annotation();
    let mut record = serde_json::to_value(a).map_err(|e| e.to_string())?;
    for (path, label, kind) in descriptors(a) {
        let id = Id::Technical(path);
        let source = text(fields, id)?;
        let optional = matches!(
            path,
            "/target_index" | "/projected_zone" | "/machining_allowance" | "/length" | "/pitch"
        );
        let value = match kind {
            Kind::Number if optional && source.trim().is_empty() => Value::Null,
            Kind::Number if matches!(path, "/target_index" | "/bom_item_id") => json!(source
                .trim()
                .parse::<u64>()
                .map_err(|_| format!("{label} must be a whole positive number"))?),
            Kind::Number => json!(number(fields, id)?),
            Kind::Toggle => json!(boolean(fields, id)?),
            Kind::Choice(_) => {
                let f = fields
                    .iter()
                    .find(|f| f.id == id)
                    .ok_or("Annotation field was removed")?;
                if !f
                    .options()
                    .is_some_and(|opts| opts.iter().any(|o| o.value == source && !o.disabled))
                {
                    return Err(format!("Choose {label}"));
                }
                json!(source)
            }
            _ => json!(source),
        };
        *record
            .pointer_mut(path)
            .ok_or("Annotation field was removed")? = value;
    }
    match a {
        DrawingAnnotationDto::ArcLengthDimension { .. }
        | DrawingAnnotationDto::JoggedRadiusDimension { .. } => {
            record["precision"] = json!(precision(fields, Id::Precision)?);
            record["presentation"] =
                serde_json::to_value(presentation(fields)?).map_err(|e| e.to_string())?;
        }
        DrawingAnnotationDto::GdtFrame { .. } => {
            let mut datums = Vec::new();
            let mut gap = false;
            for i in 0..3 {
                let label = text(fields, Id::Technical(DATUM_LABELS[i]))?.trim();
                if label.is_empty() {
                    gap = true;
                    continue;
                }
                if gap {
                    return Err(
                        "Enter datum references in primary, secondary, tertiary order".into(),
                    );
                }
                let material_condition: DrawingMaterialCondition = serde_json::from_value(json!(
                    text(fields, Id::Technical(DATUM_CONDITIONS[i]))?
                ))
                .map_err(|e| e.to_string())?;
                datums.push(DrawingDatumReferenceDto {
                    label: label.into(),
                    material_condition,
                });
            }
            record["datums"] = serde_json::to_value(datums).map_err(|e| e.to_string())?;
        }
        _ => {}
    }
    draft.technical(serde_json::from_value(record).map_err(|e| e.to_string())?)
}
