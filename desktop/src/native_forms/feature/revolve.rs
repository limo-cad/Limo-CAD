//! Revolve-specific fields use the shared profile editor's ownership,
//! transactions and widgets, and the existing solid engine request DTOs.
use super::*;
use limo_cad_solid::{EditRevolveRequest, Point2Dto, RevolveDefinitionDto, RevolveRequest};

#[derive(Debug)]
pub(super) struct RevolveFields {
    axis: &'static str,
    line: Option<(String, u64)>,
    origin: [MeasurementInput; 2],
    direction: [MeasurementInput; 2],
    angle: MeasurementInput,
}
impl RevolveFields {
    pub(super) fn new(units: limo_cad_core::UnitSystem) -> Self {
        Self {
            axis: "line",
            line: None,
            origin: [0., 0.].map(|v| MeasurementInput::new(DimensionKind::Length, v, units)),
            direction: [0., 1.].map(|v| MeasurementInput::new(DimensionKind::Unitless, v, units)),
            angle: MeasurementInput::new(DimensionKind::Angle, 360., units),
        }
    }
    pub(super) fn set(&mut self, field: SolidField, value: &str) -> Result<(), String> {
        match field {
            SolidField::Axis => {
                self.axis = match value {
                    "line" => "line",
                    "x" => "x",
                    "y" => "y",
                    "custom" => "custom",
                    _ => return Err("Choose a sketch line, X axis, Y axis or custom axis".into()),
                }
            }
            SolidField::OriginX => self.origin[0].set_text(value.into()),
            SolidField::OriginY => self.origin[1].set_text(value.into()),
            SolidField::DirectionX => self.direction[0].set_text(value.into()),
            SolidField::DirectionY => self.direction[1].set_text(value.into()),
            SolidField::Angle => self.angle.set_text(value.into()),
            _ => return Err("This is not a Revolve field".into()),
        }
        Ok(())
    }
}

impl SolidForm {
    pub(super) fn has_axis_line(&self) -> bool {
        self.revolve
            .as_ref()
            .is_some_and(|fields| fields.line.is_some())
    }
    pub(crate) fn revolution_axis(
        &self,
        model: &FormModel<'_>,
    ) -> Result<Option<[[f64; 3]; 2]>, String> {
        let Some(fields) = &self.revolve else {
            return Ok(None);
        };
        if fields.axis == "line" {
            let Some((name, id)) = &fields.line else {
                return Ok(None);
            };
            let line = self.axis_line(name, *id, model)?;
            let sketch = model
                .profiles
                .iter()
                .find(|s| s.sketch_name == *name)
                .unwrap();
            return Ok(Some([
                sketch.basis.to_3d([line.start.x, line.start.y]),
                sketch.basis.to_3d([line.end.x, line.end.y]),
            ]));
        }
        let ProfileSource::Profiles { sketch_name, .. } = &self.source else {
            return Ok(None);
        };
        let sketch = model
            .profiles
            .iter()
            .find(|s| s.sketch_name == *sketch_name)
            .ok_or("The source sketch changed")?;
        let measure =
            |v: &MeasurementInput| v.evaluate(model.document.settings.units, model.parameters);
        let (origin, direction) = match fields.axis {
            "x" => ([0., 0.], [1., 0.]),
            "y" => ([0., 0.], [0., 1.]),
            _ => (
                [measure(&fields.origin[0])?, measure(&fields.origin[1])?],
                [
                    measure(&fields.direction[0])?,
                    measure(&fields.direction[1])?,
                ],
            ),
        };
        let length = direction[0].hypot(direction[1]);
        if length <= 1e-9 {
            return Err("Axis direction must be nonzero".into());
        }
        let extent = sketch
            .profiles
            .iter()
            .flat_map(|p| &p.points)
            .map(|p| (p.x - origin[0]).hypot(p.y - origin[1]))
            .fold(10., f64::max)
            * 1.2;
        Ok(Some([-1., 1.].map(|sign| {
            sketch.basis.to_3d([
                origin[0] + sign * extent * direction[0] / length,
                origin[1] + sign * extent * direction[1] / length,
            ])
        })))
    }
    pub(crate) fn edit_revolve(
        definition: &RevolveDefinitionDto,
        model: &FormModel<'_>,
    ) -> Result<Self, String> {
        if !model
            .document
            .features
            .iter()
            .any(|f| f.id == definition.feature_id && f.kind == FeatureKind::Revolve)
        {
            return Err("The Revolve feature no longer exists".into());
        }
        let mut form = Self::new_kind(SolidFormKind::Revolve, model);
        form.feature = Some(definition.feature_id);
        form.source = ProfileSource::Profiles {
            sketch_name: definition.sketch_name.clone(),
            indices: definition.profile_indices.clone(),
        };
        form.operation = definition.operation;
        form.operation_manual = true;
        form.targets = definition.target_body_ids.clone();
        form.flip = definition.flip;
        let fields = form.revolve.as_mut().unwrap();
        let units = model.document.settings.units;
        fields.angle = MeasurementInput::new(DimensionKind::Angle, definition.angle_deg, units);
        fields.origin = [definition.axis_origin.x, definition.axis_origin.y]
            .map(|v| MeasurementInput::new(DimensionKind::Length, v, units));
        fields.direction = [definition.axis_direction.x, definition.axis_direction.y]
            .map(|v| MeasurementInput::new(DimensionKind::Unitless, v, units));
        fields.line = definition.axis_line_entity_id.map(|id| {
            (
                definition
                    .axis_line_sketch_name
                    .clone()
                    .unwrap_or_else(|| definition.sketch_name.clone()),
                id,
            )
        });
        fields.axis = if fields.line.is_some() {
            "line"
        } else if definition.axis_origin == Point2Dto::new(0., 0.)
            && definition.axis_direction == Point2Dto::new(1., 0.)
        {
            "x"
        } else if definition.axis_origin == Point2Dto::new(0., 0.)
            && definition.axis_direction == Point2Dto::new(0., 1.)
        {
            "y"
        } else {
            "custom"
        };
        Ok(form)
    }
    pub(crate) fn set_axis(
        &mut self,
        line: Option<(String, u64)>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        if self.revolve.is_none() {
            return Err("This feature has no revolution axis".into());
        }
        if let Some((sketch, id)) = &line {
            self.axis_line(sketch, *id, model)?;
        }
        let fields = self.revolve.as_mut().unwrap();
        fields.line = line;
        fields.axis = "line";
        self.changed();
        Ok(())
    }
    /// Stable line references may come from an earlier coplanar sketch,
    /// including construction lines in a sketch with no closed profile.
    fn axis_line<'a>(
        &self,
        name: &str,
        id: u64,
        model: &FormModel<'a>,
    ) -> Result<&'a limo_cad_solid::SketchLineDto, String> {
        let sketch = model
            .profiles
            .iter()
            .find(|s| s.sketch_name == name)
            .ok_or("The axis sketch no longer exists")?;
        if let ProfileSource::Profiles { sketch_name, .. } = &self.source {
            let source = model
                .profiles
                .iter()
                .find(|s| s.sketch_name == *sketch_name)
                .ok_or("The profile sketch no longer exists")?;
            if !limo_cad_solid::plane_bases_coplanar(source.basis, sketch.basis) {
                return Err("Choose a straight line on the profile's plane".into());
            }
        }
        sketch
            .lines
            .iter()
            .find(|l| l.entity_id == id && (l.end.x - l.start.x).hypot(l.end.y - l.start.y) > 1e-9)
            .ok_or_else(|| "The axis line is missing or has zero length".into())
    }
    pub(crate) fn accepts_axis(&self, name: &str, id: u64, model: &FormModel<'_>) -> bool {
        self.revolve.is_some() && self.axis_line(name, id, model).is_ok()
    }
    fn revolve_request(
        &self,
        model: &FormModel<'_>,
    ) -> Result<RevolveRequest, Vec<(SolidField, String)>> {
        use SolidField as F;
        let mut errors = vec![];
        if let Err(e) = self.check_model(model) {
            return Err(vec![(F::Source, e)]);
        }
        if self.feature.is_some_and(|id| {
            !model
                .document
                .features
                .iter()
                .any(|f| f.id == id && f.kind == FeatureKind::Revolve)
        }) {
            errors.push((
                F::Source,
                "The edited Revolve feature no longer exists".into(),
            ));
        }
        if let Err(e) = validate_source(&self.source, model) {
            errors.push((F::Source, e));
        }
        let (sketch_name, profile_indices) = match &self.source {
            ProfileSource::Profiles {
                sketch_name,
                indices,
            } => (sketch_name.clone(), indices.clone()),
            _ => {
                errors.push((F::Source, "Select closed sketch profiles".into()));
                (String::new(), vec![])
            }
        };
        let fields = self.revolve.as_ref().unwrap();
        let mut measure = |field, input: &MeasurementInput| match input
            .evaluate(model.document.settings.units, model.parameters)
        {
            Ok(v) => v,
            Err(e) => {
                errors.push((field, e));
                0.
            }
        };
        let angle = measure(F::Angle, &fields.angle);
        let (origin, direction) = match fields.axis {
            "x" => ([0., 0.], [1., 0.]),
            "y" => ([0., 0.], [0., 1.]),
            "custom" => (
                [
                    measure(F::OriginX, &fields.origin[0]),
                    measure(F::OriginY, &fields.origin[1]),
                ],
                [
                    measure(F::DirectionX, &fields.direction[0]),
                    measure(F::DirectionY, &fields.direction[1]),
                ],
            ),
            _ => ([0., 0.], [0., 1.]),
        };
        if angle.abs() <= 1e-6 || angle.abs() > 360. {
            errors.push((
                F::Angle,
                "Angle must be nonzero and no larger than 360 degrees".into(),
            ));
        }
        if direction[0].hypot(direction[1]) <= 1e-9 {
            errors.push((F::DirectionX, "Axis direction must be nonzero".into()));
        }
        let line = if fields.axis == "line" {
            match fields.line.as_ref() {
                Some((name, id)) => {
                    if let Err(e) = self.axis_line(name, *id, model) {
                        errors.push((F::AxisLine, e));
                    }
                    Some((name.clone(), *id))
                }
                None => {
                    errors.push((F::AxisLine, "Select a straight sketch line".into()));
                    None
                }
            }
        } else {
            None
        };
        if self.operation != ExtrudeOperation::NewBody {
            if let Err(e) = validate_targets(&self.targets, model) {
                errors.push((F::Targets, e));
            }
            if self.targets.is_empty() {
                errors.push((F::Targets, "Select a target body for this operation".into()));
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(RevolveRequest {
            sketch_name,
            profile_indices,
            axis_origin: Point2Dto::new(origin[0], origin[1]),
            axis_direction: Point2Dto::new(direction[0], direction[1]),
            axis_line_sketch_name: line.as_ref().map(|l| l.0.clone()),
            axis_line_entity_id: line.map(|l| l.1),
            angle_deg: angle,
            flip: self.flip,
            operation: self.operation,
            target_body_ids: if self.operation == ExtrudeOperation::NewBody {
                vec![]
            } else {
                self.targets.clone()
            },
        })
    }
    pub(super) fn revolve_payload(
        &self,
        model: &FormModel<'_>,
    ) -> Result<(&'static str, Value), Vec<(SolidField, String)>> {
        let request = self.revolve_request(model)?;
        let (op, value) = if let Some(feature_id) = self.feature {
            (
                "solid_edit_revolve",
                serde_json::to_value(EditRevolveRequest {
                    feature_id,
                    revolve: request,
                }),
            )
        } else {
            ("solid_revolve", serde_json::to_value(request))
        };
        value
            .map(|v| (op, v))
            .map_err(|e| vec![(SolidField::Source, e.to_string())])
    }
    pub(super) fn revolve_fields(&self, model: &FormModel<'_>) -> Vec<SolidFieldView> {
        use SolidField as F;
        let fields = self.revolve.as_ref().unwrap();
        let issues = self.revolve_request(model).err().unwrap_or_default();
        let enabled = self.phase == Phase::Editing && self.check_model(model).is_ok();
        let text = |input: &MeasurementInput| Field::Text {
            value: input.text().into(),
            read_only: false,
            selection: None,
        };
        let choice = |value: &str, options: &[(&str, &str)]| Field::Choice {
            value: value.into(),
            options: options
                .iter()
                .map(|(value, label)| ChoiceOption {
                    value: (*value).into(),
                    label: (*label).into(),
                    disabled: false,
                })
                .collect(),
        };
        let source = match &self.source {
            ProfileSource::Profiles {
                sketch_name,
                indices,
            } => format!("{sketch_name} · {} profile(s)", indices.len()),
            _ => "Select profiles".into(),
        };
        let axis = fields
            .line
            .as_ref()
            .map(|(name, id)| format!("Axis: {name} · line {id}"))
            .unwrap_or_else(|| "Select axis line".into());
        let rows = vec![
            (F::Source, source, Field::None, true),
            (
                F::Axis,
                "Axis".into(),
                choice(
                    fields.axis,
                    &[
                        ("line", "Sketch line"),
                        ("x", "Sketch X axis"),
                        ("y", "Sketch Y axis"),
                        ("custom", "Custom sketch axis"),
                    ],
                ),
                true,
            ),
            (F::AxisLine, axis, Field::None, fields.axis == "line"),
            (
                F::OriginX,
                "Origin X".into(),
                text(&fields.origin[0]),
                fields.axis == "custom",
            ),
            (
                F::OriginY,
                "Origin Y".into(),
                text(&fields.origin[1]),
                fields.axis == "custom",
            ),
            (
                F::DirectionX,
                "Direction X".into(),
                text(&fields.direction[0]),
                fields.axis == "custom",
            ),
            (
                F::DirectionY,
                "Direction Y".into(),
                text(&fields.direction[1]),
                fields.axis == "custom",
            ),
            (
                F::Angle,
                "Angle (degrees)".into(),
                text(&fields.angle),
                true,
            ),
            (
                F::Operation,
                "Operation".into(),
                choice(
                    json!(self.operation).as_str().unwrap(),
                    &[
                        ("new_body", "Create Body"),
                        ("join", "Add"),
                        ("cut", "Subtract"),
                        ("intersect", "Common"),
                    ],
                ),
                true,
            ),
            (
                F::Targets,
                format!("Target bodies ({})", self.targets.len()),
                Field::None,
                self.operation != ExtrudeOperation::NewBody,
            ),
            (
                F::Flip,
                "Flip direction".into(),
                Field::Toggle(self.flip),
                true,
            ),
        ];
        rows.into_iter()
            .map(|(field, label, value, visible)| SolidFieldView {
                field,
                label,
                value,
                visible,
                enabled,
                error: issues
                    .iter()
                    .find(|(key, _)| *key == field)
                    .map(|(_, e)| e.clone()),
            })
            .collect()
    }
}
