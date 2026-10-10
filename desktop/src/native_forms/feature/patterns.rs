//! Body patterns use the existing solid requests; edge picks populate editable vectors.
use super::*;
use limo_cad_core::{EdgeId, UnitSystem};
use limo_cad_solid::{
    BodyFeatureDefinitionDto, CircularPatternRequest, Point3Dto, RectangularPatternRequest,
};

impl SolidFormKind {
    pub(crate) fn is_pattern(self) -> bool {
        matches!(self, Self::RectangularPattern | Self::CircularPattern)
    }
    pub(crate) fn selects_bodies(self) -> bool {
        self.is_pattern() || self.is_body_plane() || self == Self::MoveCopy
    }
}
impl SolidField {
    pub(crate) fn is_straight_reference(self) -> bool {
        matches!(
            self,
            Self::AxisEdge | Self::DirectionEdge | Self::SecondDirectionEdge
        )
    }
    /// Native layout only: requests and grouping remain the shared product contract.
    pub(crate) fn compact_row(
        self,
        kind: SolidFormKind,
    ) -> Option<(&'static str, usize, &'static [Self])> {
        use SolidField::*;
        let groups: &[(&str, &[Self])] =
            if matches!(kind, SolidFormKind::ExternalThread | SolidFormKind::Hole) {
                &[
                    ("", &[OriginX, OriginY]),
                    ("", &[CounterboreDiameter, CounterboreDepth]),
                    ("", &[CountersinkDiameter, CountersinkAngle]),
                    ("", &[ThreadStandard, ThreadSeries]),
                    ("", &[Diameter, Pitch]),
                    ("", &[ThreadClass, Designation]),
                    ("", &[ThreadHand, Representation]),
                    ("", &[RadialDepth, CornerRadius]),
                    ("", &[RadialClearance, AxialClearance]),
                ]
            } else if kind == SolidFormKind::Extrude {
                &[("", &[Distance, SecondDistance])]
            } else if kind == SolidFormKind::Sweep {
                &[("", &[Orientation, Transition])]
            } else if kind == SolidFormKind::Revolve {
                &[("", &[OriginX, OriginY]), ("", &[DirectionX, DirectionY])]
            } else if kind == SolidFormKind::Rib {
                &[("", &[Thickness, Distance])]
            } else if kind == SolidFormKind::MoveCopy {
                &[
                    ("Translation", &[TranslationX, TranslationY, TranslationZ]),
                    ("Rotation", &[RotationX, RotationY, RotationZ]),
                    ("Rotation pivot", &[PivotX, PivotY, PivotZ]),
                    ("Direction", &[DirectionX, DirectionY, DirectionZ]),
                    (
                        "Axis",
                        &[SecondDirectionX, SecondDirectionY, SecondDirectionZ],
                    ),
                    ("From point", &[FromX, FromY, FromZ]),
                    ("To point", &[ToX, ToY, ToZ]),
                ]
            } else if kind.is_pattern() {
                &[
                    ("Axis origin", &[OriginX, OriginY, OriginZ]),
                    (
                        if kind == SolidFormKind::CircularPattern {
                            "Axis direction"
                        } else {
                            "First direction"
                        },
                        &[DirectionX, DirectionY, DirectionZ],
                    ),
                    (
                        "Second direction",
                        &[SecondDirectionX, SecondDirectionY, SecondDirectionZ],
                    ),
                    (
                        "",
                        if kind == SolidFormKind::CircularPattern {
                            &[Count, Angle]
                        } else {
                            &[Distance, Count]
                        },
                    ),
                    ("", &[SecondDistance, SecondCount]),
                ]
            } else {
                return None;
            };
        groups.iter().find_map(|(title, fields)| {
            fields
                .iter()
                .position(|f| *f == self)
                .map(|i| (*title, i, *fields))
        })
    }
}

#[derive(Debug)]
pub(super) struct PatternFields {
    pub kind: SolidFormKind,
    pub bodies: Vec<BodyId>,
    origin: [MeasurementInput; 3],
    direction: [MeasurementInput; 3],
    second_direction: [MeasurementInput; 3],
    spacing: MeasurementInput,
    second_spacing: MeasurementInput,
    count: MeasurementInput,
    second_count: MeasurementInput,
    angle: MeasurementInput,
    second_enabled: bool,
    edges: [Option<(BodyId, EdgeId)>; 2],
}
fn point(x: f64, y: f64, z: f64) -> Point3Dto {
    Point3Dto { x, y, z }
}
fn vector(values: Point3Dto, kind: DimensionKind, units: UnitSystem) -> [MeasurementInput; 3] {
    [values.x, values.y, values.z].map(|v| MeasurementInput::new(kind, v, units))
}
impl PatternFields {
    pub fn new(kind: SolidFormKind, units: UnitSystem) -> Self {
        Self {
            kind,
            bodies: Vec::new(),
            origin: vector(point(0., 0., 0.), DimensionKind::Length, units),
            direction: vector(
                if kind == SolidFormKind::CircularPattern {
                    point(0., 0., 1.)
                } else {
                    point(1., 0., 0.)
                },
                DimensionKind::Unitless,
                units,
            ),
            second_direction: vector(point(0., 1., 0.), DimensionKind::Unitless, units),
            spacing: MeasurementInput::new(DimensionKind::Length, 10., units),
            second_spacing: MeasurementInput::new(DimensionKind::Length, 10., units),
            count: MeasurementInput::new(DimensionKind::Unitless, 3., units),
            second_count: MeasurementInput::new(DimensionKind::Unitless, 2., units),
            angle: MeasurementInput::new(DimensionKind::Angle, 360., units),
            second_enabled: false,
            edges: [None; 2],
        }
    }
    pub fn set(&mut self, field: SolidField, value: &str) -> Result<(), String> {
        use SolidField::*;
        if field == SecondEnabled {
            self.second_enabled = match value {
                "true" => true,
                "false" => false,
                _ => return Err("Second direction expects true or false".into()),
            };
            return Ok(());
        }
        let input = match field {
            OriginX => &mut self.origin[0],
            OriginY => &mut self.origin[1],
            OriginZ => &mut self.origin[2],
            DirectionX => &mut self.direction[0],
            DirectionY => &mut self.direction[1],
            DirectionZ => &mut self.direction[2],
            SecondDirectionX => &mut self.second_direction[0],
            SecondDirectionY => &mut self.second_direction[1],
            SecondDirectionZ => &mut self.second_direction[2],
            Distance => &mut self.spacing,
            SecondDistance => &mut self.second_spacing,
            Count => &mut self.count,
            SecondCount => &mut self.second_count,
            Angle => &mut self.angle,
            _ => return Err("This pattern field is not editable".into()),
        };
        input.set_text(value.into());
        if matches!(
            field,
            OriginX | OriginY | OriginZ | DirectionX | DirectionY | DirectionZ
        ) {
            self.edges[0] = None;
        }
        if matches!(
            field,
            SecondDirectionX | SecondDirectionY | SecondDirectionZ
        ) {
            self.edges[1] = None;
        }
        Ok(())
    }
}
impl SolidForm {
    pub(super) fn has_pattern_reference(&self, field: SolidField) -> bool {
        self.patterns.as_ref().is_some_and(|fields| {
            fields.edges[usize::from(field == SolidField::SecondDirectionEdge)].is_some()
        })
    }
    pub(crate) fn set_pattern_edge(
        &mut self,
        field: SolidField,
        edge: Option<(BodyId, EdgeId)>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        let fields = self
            .patterns
            .as_mut()
            .ok_or("This feature has no pattern axis")?;
        let index = match field {
            SolidField::SecondDirectionEdge
                if fields.kind == SolidFormKind::RectangularPattern && fields.second_enabled =>
            {
                1
            }
            SolidField::DirectionEdge if fields.kind == SolidFormKind::RectangularPattern => 0,
            SolidField::AxisEdge if fields.kind == SolidFormKind::CircularPattern => 0,
            _ => return Err("This pattern reference is unavailable".into()),
        };
        let units = model.document.settings.units;
        let (origin, direction) = if let Some((body, edge)) = edge {
            let edge = model
                .scene
                .bodies
                .iter()
                .find(|b| b.id == body)
                .and_then(|b| b.edges.iter().find(|e| e.id == edge))
                .ok_or("The selected edge no longer exists")?;
            if !limo_cad_solid::edge_is_straight(edge) {
                return Err("Choose a straight edge".into());
            }
            let a = edge.points.first().ok_or("The edge has no endpoints")?;
            let b = edge.points.last().unwrap();
            (*a, point(b.x - a.x, b.y - a.y, b.z - a.z))
        } else {
            (point(0., 0., 0.), point(0., 0., 0.))
        };
        if index == 1 {
            fields.second_direction = vector(direction, DimensionKind::Unitless, units);
        } else {
            fields.direction = vector(direction, DimensionKind::Unitless, units);
            if fields.kind == SolidFormKind::CircularPattern {
                fields.origin = vector(origin, DimensionKind::Length, units);
            }
        }
        fields.edges[index] = edge;
        self.changed();
        Ok(())
    }
    pub(crate) fn pattern_edges(&self) -> impl Iterator<Item = (BodyId, EdgeId)> + '_ {
        self.patterns.iter().flat_map(|f| {
            f.edges
                .iter()
                .enumerate()
                .filter_map(|(i, e)| (i == 0 || f.second_enabled).then_some(*e).flatten())
        })
    }
    pub(crate) fn edit_pattern(definition: &Value, model: &FormModel<'_>) -> Result<Self, String> {
        let definition: BodyFeatureDefinitionDto =
            serde_json::from_value(definition.clone()).map_err(|e| e.to_string())?;
        let units = model.document.settings.units;
        let (id, mut fields) = match definition {
            BodyFeatureDefinitionDto::RectangularPattern {
                feature_id,
                body_ids,
                direction,
                spacing,
                count,
                second_direction,
                second_spacing,
                second_count,
                ..
            } => {
                let mut f = PatternFields::new(SolidFormKind::RectangularPattern, units);
                f.bodies = body_ids;
                f.direction = vector(direction, DimensionKind::Unitless, units);
                f.spacing = MeasurementInput::new(DimensionKind::Length, spacing, units);
                f.count = MeasurementInput::new(DimensionKind::Unitless, count as f64, units);
                f.second_enabled = second_direction.is_some();
                if let Some(direction) = second_direction {
                    f.second_direction = vector(direction, DimensionKind::Unitless, units);
                    f.second_spacing =
                        MeasurementInput::new(DimensionKind::Length, second_spacing, units);
                    f.second_count =
                        MeasurementInput::new(DimensionKind::Unitless, second_count as f64, units);
                }
                (feature_id, f)
            }
            BodyFeatureDefinitionDto::CircularPattern {
                feature_id,
                body_ids,
                axis_origin,
                axis_direction,
                count,
                total_angle_deg,
                ..
            } => {
                let mut f = PatternFields::new(SolidFormKind::CircularPattern, units);
                f.bodies = body_ids;
                f.origin = vector(axis_origin, DimensionKind::Length, units);
                f.direction = vector(axis_direction, DimensionKind::Unitless, units);
                f.count = MeasurementInput::new(DimensionKind::Unitless, count as f64, units);
                f.angle = MeasurementInput::new(DimensionKind::Angle, total_angle_deg, units);
                (feature_id, f)
            }
            _ => return Err("The selected feature is not a body pattern".into()),
        };
        let mut form = Self::new_kind(fields.kind, model);
        form.feature = Some(id);
        let bodies = std::mem::take(&mut fields.bodies);
        form.patterns = Some(fields);
        form.set_bodies(bodies, model)?;
        form.pattern_payload(model).map_err(first_error)?;
        Ok(form)
    }
    pub(super) fn pattern_payload(
        &self,
        model: &FormModel<'_>,
    ) -> Result<(&'static str, Value), Vec<(SolidField, String)>> {
        use SolidField as F;
        let fields = self.patterns.as_ref().unwrap();
        let issue = |field, error: String| vec![(field, error)];
        self.check_model(model).map_err(|e| issue(F::Bodies, e))?;
        if self.feature.is_some_and(|id| {
            !model.document.features.iter().any(|f| {
                f.id == id && SolidFormKind::from_feature_kind(f.kind) == Some(fields.kind)
            })
        }) {
            return Err(issue(
                F::Bodies,
                "The edited feature no longer exists".into(),
            ));
        }
        if fields.bodies.is_empty() {
            return Err(issue(F::Bodies, "Select the source bodies".into()));
        }
        validate_targets(&fields.bodies, model).map_err(|e| issue(F::Bodies, e))?;
        let number = |field, input: &MeasurementInput| {
            input
                .evaluate(model.document.settings.units, model.parameters)
                .map_err(|e| issue(field, e))
        };
        let count = |field, input: &MeasurementInput| {
            let value = number(field, input)?;
            if value.fract() != 0. || !(2. ..=10_000.).contains(&value) {
                return Err(issue(
                    field,
                    "Count must be a whole number from 2 to 10000".into(),
                ));
            }
            Ok(value as u32)
        };
        let vector = |keys: [F; 3], values: &[MeasurementInput; 3], direction: bool| {
            let v = point(
                number(keys[0], &values[0])?,
                number(keys[1], &values[1])?,
                number(keys[2], &values[2])?,
            );
            let length = v.x.hypot(v.y).hypot(v.z);
            if direction && (!length.is_finite() || length <= 1e-9) {
                return Err(issue(
                    keys[0],
                    "Direction must be a finite, non-zero vector".into(),
                ));
            }
            Ok(v)
        };
        let direction = vector(
            [F::DirectionX, F::DirectionY, F::DirectionZ],
            &fields.direction,
            true,
        )?;
        let count = count(F::Count, &fields.count)?;
        limo_cad_solid::pattern_copy_count(count, 1, fields.bodies.len())
            .map_err(|e| issue(F::Count, e.to_string()))?;
        let request = if fields.kind == SolidFormKind::CircularPattern {
            let origin = vector([F::OriginX, F::OriginY, F::OriginZ], &fields.origin, false)?;
            let angle = number(F::Angle, &fields.angle)?;
            if angle.abs() <= 1e-9 || angle.abs() > 360. {
                return Err(issue(
                    F::Angle,
                    "Angle must be non-zero and within -360° to 360°".into(),
                ));
            }
            json!(CircularPatternRequest {
                body_ids: fields.bodies.clone(),
                axis_origin: origin,
                axis_direction: direction,
                count,
                total_angle_deg: angle
            })
        } else {
            let spacing = number(F::Distance, &fields.spacing)?;
            if spacing.abs() <= 1e-9 {
                return Err(issue(F::Distance, "Spacing must be non-zero".into()));
            }
            let (second_direction, second_spacing, second_count) = if fields.second_enabled {
                let direction = vector(
                    [
                        F::SecondDirectionX,
                        F::SecondDirectionY,
                        F::SecondDirectionZ,
                    ],
                    &fields.second_direction,
                    true,
                )?;
                let spacing = number(F::SecondDistance, &fields.second_spacing)?;
                if spacing.abs() <= 1e-9 {
                    return Err(issue(F::SecondDistance, "Spacing must be non-zero".into()));
                }
                let value = number(F::SecondCount, &fields.second_count)?;
                if value.fract() != 0. || !(2. ..=10_000.).contains(&value) {
                    return Err(issue(
                        F::SecondCount,
                        "Count must be a whole number from 2 to 10000".into(),
                    ));
                }
                (Some(direction), spacing, value as u32)
            } else {
                (None, 0., 1)
            };
            limo_cad_solid::pattern_copy_count(count, second_count, fields.bodies.len())
                .map_err(|e| issue(F::SecondCount, e.to_string()))?;
            json!(RectangularPatternRequest {
                body_ids: fields.bodies.clone(),
                direction,
                spacing,
                count,
                second_direction,
                second_spacing,
                second_count
            })
        };
        Ok(if let Some(feature_id) = self.feature {
            (
                if fields.kind == SolidFormKind::CircularPattern {
                    "solid_edit_circular_pattern"
                } else {
                    "solid_edit_rectangular_pattern"
                },
                json!({"feature_id":feature_id,"request":request}),
            )
        } else {
            (fields.kind.operation(), request)
        })
    }
    pub(super) fn pattern_fields(&self, model: &FormModel<'_>) -> Vec<SolidFieldView> {
        use SolidField as F;
        let fields = self.patterns.as_ref().unwrap();
        let circular = fields.kind == SolidFormKind::CircularPattern;
        let text = |input: &MeasurementInput| Field::Text {
            value: input.text().into(),
            read_only: false,
            selection: None,
        };
        let mut rows = vec![
            (
                F::Bodies,
                if fields.bodies.is_empty() {
                    "Click one or more bodies".into()
                } else {
                    if fields.bodies.len() == 1 {
                        "1 body selected".into()
                    } else {
                        format!("{} bodies selected", fields.bodies.len())
                    }
                },
                Field::None,
            ),
            (
                if circular {
                    F::AxisEdge
                } else {
                    F::DirectionEdge
                },
                if fields.edges[0].is_some() {
                    "Straight edge selected"
                } else if circular {
                    "Pick an edge or enter an axis"
                } else {
                    "Pick an edge or enter XYZ"
                }
                .into(),
                Field::None,
            ),
        ];
        if circular {
            for (i, f) in [F::OriginX, F::OriginY, F::OriginZ].into_iter().enumerate() {
                rows.push((
                    f,
                    format!("Axis origin {}", ["X", "Y", "Z"][i]),
                    text(&fields.origin[i]),
                ));
            }
        }
        for (i, f) in [F::DirectionX, F::DirectionY, F::DirectionZ]
            .into_iter()
            .enumerate()
        {
            rows.push((
                f,
                format!(
                    "{} {}",
                    if circular {
                        "Axis direction"
                    } else {
                        "First direction"
                    },
                    ["X", "Y", "Z"][i]
                ),
                text(&fields.direction[i]),
            ));
        }
        if !circular {
            rows.push((F::Distance, "Spacing".into(), text(&fields.spacing)));
        }
        rows.push((F::Count, "Count".into(), text(&fields.count)));
        if circular {
            rows.push((F::Angle, "Total angle".into(), text(&fields.angle)));
        } else {
            rows.push((
                F::SecondEnabled,
                "Add a second direction".into(),
                Field::Toggle(fields.second_enabled),
            ));
            if fields.second_enabled {
                rows.push((
                    F::SecondDirectionEdge,
                    if fields.edges[1].is_some() {
                        "Second straight edge selected"
                    } else {
                        "Pick a second edge or enter XYZ"
                    }
                    .into(),
                    Field::None,
                ));
                for (i, f) in [
                    F::SecondDirectionX,
                    F::SecondDirectionY,
                    F::SecondDirectionZ,
                ]
                .into_iter()
                .enumerate()
                {
                    rows.push((
                        f,
                        format!("Second direction {}", ["X", "Y", "Z"][i]),
                        text(&fields.second_direction[i]),
                    ));
                }
                rows.push((
                    F::SecondDistance,
                    "Second spacing".into(),
                    text(&fields.second_spacing),
                ));
                rows.push((
                    F::SecondCount,
                    "Second count".into(),
                    text(&fields.second_count),
                ));
            }
        }
        let issues = self.pattern_payload(model).err().unwrap_or_default();
        rows.into_iter()
            .map(|(field, label, value)| SolidFieldView {
                field,
                label,
                value,
                visible: true,
                enabled: self.phase == Phase::Editing && self.check_model(model).is_ok(),
                error: issues
                    .iter()
                    .find(|(f, _)| *f == field)
                    .map(|(_, e)| e.clone()),
            })
            .collect()
    }
}
