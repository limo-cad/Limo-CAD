//! Rigid body placement: typed controls resolve to the same transform as MCP.
use super::*;
use bevy::math::{DQuat, DVec3, EulerRot};
use limo_cad_core::{EdgeId, UnitSystem};
use limo_cad_solid::{BodyFeatureDefinitionDto, MoveCopyBodyRequest, Point3Dto};
mod occurrence;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MoveMode {
    Free,
    Translate,
    Rotate,
    PointToPoint,
}
impl MoveMode {
    fn key(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Translate => "translate",
            Self::Rotate => "rotate",
            Self::PointToPoint => "point_to_point",
        }
    }
}

#[derive(Debug)]
pub(super) struct MoveFields {
    pub bodies: Vec<BodyId>,
    component: bool,
    occurrence: Option<u64>,
    mode: MoveMode,
    translation: [MeasurementInput; 3],
    rotation: [MeasurementInput; 3],
    pivot: [MeasurementInput; 3],
    direction: [MeasurementInput; 3],
    axis: [MeasurementInput; 3],
    from: [MeasurementInput; 3],
    to: [MeasurementInput; 3],
    distance: MeasurementInput,
    angle: MeasurementInput,
    copy: bool,
    manual_pivot: bool,
    manual_destination: bool,
    edges: [Option<(BodyId, EdgeId)>; 2],
    edge_occurrences: [Option<u64>; 2],
    exact_rotation: Option<[f64; 4]>,
}
fn vector(v: [f64; 3], kind: DimensionKind, units: UnitSystem) -> [MeasurementInput; 3] {
    v.map(|n| MeasurementInput::new(kind, n, units))
}
fn point(v: DVec3) -> Point3Dto {
    Point3Dto {
        x: v.x,
        y: v.y,
        z: v.z,
    }
}
fn xyz(p: Point3Dto) -> DVec3 {
    DVec3::new(p.x, p.y, p.z)
}
impl MoveFields {
    pub fn new(units: UnitSystem) -> Self {
        Self {
            bodies: vec![],
            component: false,
            occurrence: None,
            mode: MoveMode::Free,
            translation: vector([0.; 3], DimensionKind::Length, units),
            rotation: vector([0.; 3], DimensionKind::Angle, units),
            pivot: vector([0.; 3], DimensionKind::Length, units),
            direction: vector([1., 0., 0.], DimensionKind::Unitless, units),
            axis: vector([0., 0., 1.], DimensionKind::Unitless, units),
            from: vector([0.; 3], DimensionKind::Length, units),
            to: vector([0.; 3], DimensionKind::Length, units),
            distance: MeasurementInput::new(DimensionKind::Length, 0., units),
            angle: MeasurementInput::new(DimensionKind::Angle, 0., units),
            copy: false,
            manual_pivot: false,
            manual_destination: false,
            edges: [None; 2],
            edge_occurrences: [None; 2],
            exact_rotation: None,
        }
    }
    pub fn select(&mut self, bodies: Vec<BodyId>, model: &FormModel<'_>) {
        if !self.manual_pivot {
            let mut min = DVec3::splat(f64::INFINITY);
            let mut max = DVec3::splat(f64::NEG_INFINITY);
            for b in model.scene.bodies.iter().filter(|b| bodies.contains(&b.id)) {
                for p in b.mesh.positions.as_chunks::<3>().0 {
                    let v = DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
                    min = min.min(v);
                    max = max.max(v);
                }
            }
            if min.is_finite() && max.is_finite() {
                self.pivot = vector(
                    ((min + max) * 0.5).to_array(),
                    DimensionKind::Length,
                    model.document.settings.units,
                );
            }
        }
        self.bodies = bodies;
    }
    pub fn set(&mut self, field: SolidField, value: &str, editing: bool) -> Result<(), String> {
        use SolidField::*;
        if field == MoveObjectType {
            if editing {
                return Err("A body history edit cannot change object type".into());
            }
            self.component = match value {
                "bodies" => false,
                "component" => true,
                _ => return Err("Choose Bodies or Component".into()),
            };
            return Ok(());
        }
        if field == MoveMode {
            self.mode = match value {
                "free" => self::MoveMode::Free,
                "translate" => self::MoveMode::Translate,
                "rotate" => self::MoveMode::Rotate,
                "point_to_point" => self::MoveMode::PointToPoint,
                _ => return Err("Choose an available move type".into()),
            };
            return Ok(());
        }
        if field == Copy {
            if editing {
                return Err("A history edit cannot change Move into Copy".into());
            }
            self.copy = value
                .parse()
                .map_err(|_| "Create copy expects true or false")?;
            return Ok(());
        }
        let input = match field {
            TranslationX => &mut self.translation[0],
            TranslationY => &mut self.translation[1],
            TranslationZ => &mut self.translation[2],
            RotationX => &mut self.rotation[0],
            RotationY => &mut self.rotation[1],
            RotationZ => &mut self.rotation[2],
            PivotX => &mut self.pivot[0],
            PivotY => &mut self.pivot[1],
            PivotZ => &mut self.pivot[2],
            DirectionX => &mut self.direction[0],
            DirectionY => &mut self.direction[1],
            DirectionZ => &mut self.direction[2],
            SecondDirectionX => &mut self.axis[0],
            SecondDirectionY => &mut self.axis[1],
            SecondDirectionZ => &mut self.axis[2],
            FromX => &mut self.from[0],
            FromY => &mut self.from[1],
            FromZ => &mut self.from[2],
            ToX => &mut self.to[0],
            ToY => &mut self.to[1],
            ToZ => &mut self.to[2],
            Distance => &mut self.distance,
            Angle => &mut self.angle,
            _ => return Err("This move field is not editable".into()),
        };
        input.set_text(value.into());
        if matches!(field, RotationX | RotationY | RotationZ) {
            self.exact_rotation = None;
        }
        if matches!(field, ToX | ToY | ToZ) {
            self.manual_destination = true;
        }
        if matches!(field, PivotX | PivotY | PivotZ) {
            self.manual_pivot = true;
        }
        if matches!(field, DirectionX | DirectionY | DirectionZ) {
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

impl SolidField {
    pub(crate) fn is_move_point(self) -> bool {
        matches!(self, Self::FromPoint | Self::ToPoint | Self::PivotPoint)
    }
}
impl SolidForm {
    pub(super) fn has_move_reference(&self, field: SolidField) -> bool {
        self.move_copy.as_ref().is_some_and(|fields| {
            if field.is_move_point() {
                true // Coordinates define a point even without a viewport pick.
            } else {
                fields.edges[usize::from(field == SolidField::AxisEdge)].is_some()
            }
        })
    }
    pub(crate) fn move_mode(&self) -> Option<MoveMode> {
        self.move_copy.as_ref().map(|f| f.mode)
    }
    pub(crate) fn move_pick_target(&self) -> Option<SolidField> {
        Some(match self.move_mode()? {
            MoveMode::Free => SolidField::Bodies,
            MoveMode::Translate => SolidField::DirectionEdge,
            MoveMode::Rotate => SolidField::AxisEdge,
            MoveMode::PointToPoint => SolidField::FromPoint,
        })
    }
    pub(crate) fn set_move_point(
        &mut self,
        field: SolidField,
        p: [f64; 3],
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        if !p.iter().all(|v| v.is_finite()) {
            return Err("Choose a finite point".into());
        }
        let f = self.move_copy.as_mut().ok_or("Open Move/Copy first")?;
        let values = vector(p, DimensionKind::Length, model.document.settings.units);
        match field {
            SolidField::FromPoint => {
                if !f.manual_destination {
                    f.to = values.clone();
                }
                f.from = values;
            }
            SolidField::ToPoint => {
                f.to = values;
                f.manual_destination = true;
            }
            SolidField::PivotPoint => {
                f.pivot = values;
                f.manual_pivot = true;
            }
            _ => return Err("This field is not a move point".into()),
        }
        self.changed();
        Ok(())
    }
    pub(crate) fn set_move_edge(
        &mut self,
        field: SolidField,
        edge: Option<(BodyId, EdgeId)>,
        occurrence: Option<u64>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        let f = self.move_copy.as_mut().ok_or("Open Move/Copy first")?;
        let index = match (field, f.mode) {
            (SolidField::DirectionEdge, MoveMode::Translate) => 0,
            (SolidField::AxisEdge, MoveMode::Rotate) => 1,
            _ => return Err("This move reference is unavailable".into()),
        };
        let d = if let Some((body, id)) = edge {
            let e = model
                .scene
                .bodies
                .iter()
                .find(|b| b.id == body)
                .and_then(|b| b.edges.iter().find(|e| e.id == id))
                .ok_or("The selected edge no longer exists")?;
            if !limo_cad_solid::edge_is_straight(e) {
                return Err("Choose a straight edge".into());
            }
            let mut direction = xyz(*e.points.last().unwrap()) - xyz(e.points[0]);
            if let Some(id) = occurrence {
                let pose = model
                    .assembly_solution
                    .and_then(|s| {
                        s.instance_body_poses
                            .iter()
                            .find(|p| p.occurrence_id.0 == id && p.body_id == body)
                    })
                    .ok_or("The selected edge has no component placement")?;
                direction = DQuat::from_array(pose.rotation) * direction;
            }
            direction.to_array()
        } else {
            [0.; 3]
        };
        let d = vector(d, DimensionKind::Unitless, model.document.settings.units);
        if index == 0 {
            f.direction = d;
        } else {
            f.axis = d;
        }
        f.edges[index] = edge;
        f.edge_occurrences[index] = occurrence;
        self.changed();
        Ok(())
    }
    pub(crate) fn move_edges(&self) -> impl Iterator<Item = (BodyId, EdgeId, Option<u64>)> + '_ {
        self.move_copy.iter().filter_map(|f| match f.mode {
            MoveMode::Translate => f.edges[0].map(|(b, e)| (b, e, f.edge_occurrences[0])),
            MoveMode::Rotate => f.edges[1].map(|(b, e)| (b, e, f.edge_occurrences[1])),
            _ => None,
        })
    }
    pub(crate) fn move_request(
        &self,
        model: &FormModel<'_>,
    ) -> Result<MoveCopyBodyRequest, Vec<(SolidField, String)>> {
        use SolidField as F;
        let issue = |field, message| vec![(field, message)];
        self.check_model(model).map_err(|e| issue(F::Bodies, e))?;
        let f = self
            .move_copy
            .as_ref()
            .ok_or_else(|| issue(F::Bodies, "Open Move/Copy first".into()))?;
        if f.component {
            self.move_occurrence_targets(model)
                .map_err(|e| issue(F::Bodies, e))?;
        } else if f.bodies.is_empty() {
            return Err(issue(F::Bodies, "Select the bodies to move or copy".into()));
        }
        if !f.component {
            validate_targets(&f.bodies, model).map_err(|e| issue(F::Bodies, e))?;
        }
        let number = |key, v: &MeasurementInput| {
            v.evaluate(model.document.settings.units, model.parameters)
                .map_err(|e| issue(key, e))
        };
        let vector =
            |keys: [F; 3], values: &[MeasurementInput; 3]| -> Result<DVec3, Vec<(F, String)>> {
                Ok(DVec3::new(
                    number(keys[0], &values[0])?,
                    number(keys[1], &values[1])?,
                    number(keys[2], &values[2])?,
                ))
            };
        let pivot = vector([F::PivotX, F::PivotY, F::PivotZ], &f.pivot)?;
        let (translation, rotation) = match f.mode {
            MoveMode::Free => {
                let t = vector(
                    [F::TranslationX, F::TranslationY, F::TranslationZ],
                    &f.translation,
                )?;
                let angles = vector([F::RotationX, F::RotationY, F::RotationZ], &f.rotation)?
                    * std::f64::consts::PI
                    / 180.;
                let q = f.exact_rotation.map(DQuat::from_array).unwrap_or_else(|| {
                    DQuat::from_euler(EulerRot::ZYX, angles.z, angles.y, angles.x)
                });
                (t, q)
            }
            MoveMode::Translate => {
                let direction =
                    vector([F::DirectionX, F::DirectionY, F::DirectionZ], &f.direction)?
                        .try_normalize()
                        .ok_or_else(|| {
                            issue(F::DirectionX, "Direction must be a non-zero vector".into())
                        })?;
                (
                    direction * number(F::Distance, &f.distance)?,
                    DQuat::IDENTITY,
                )
            }
            MoveMode::Rotate => {
                let axis = vector(
                    [
                        F::SecondDirectionX,
                        F::SecondDirectionY,
                        F::SecondDirectionZ,
                    ],
                    &f.axis,
                )?
                .try_normalize()
                .ok_or_else(|| {
                    issue(
                        F::SecondDirectionX,
                        "Rotation axis must be a non-zero vector".into(),
                    )
                })?;
                (
                    DVec3::ZERO,
                    DQuat::from_axis_angle(axis, number(F::Angle, &f.angle)?.to_radians()),
                )
            }
            MoveMode::PointToPoint => (
                vector([F::ToX, F::ToY, F::ToZ], &f.to)?
                    - vector([F::FromX, F::FromY, F::FromZ], &f.from)?,
                DQuat::IDENTITY,
            ),
        };
        if !translation.is_finite()
            || translation.abs().max_element() > f32::MAX as f64 / 16.
            || pivot.abs().max_element() > f32::MAX as f64 / 16.
            || !pivot.is_finite()
            || !rotation.is_finite()
            || rotation.length_squared() < 1e-18
        {
            return Err(issue(
                F::MoveMode,
                "The move must have a finite rigid transform".into(),
            ));
        }
        Ok(MoveCopyBodyRequest {
            body_ids: f.bodies.clone(),
            translation: point(translation),
            rotation: rotation.normalize().to_array(),
            pivot: point(pivot),
            copy: f.copy,
        })
    }
    pub(super) fn move_payload(
        &self,
        model: &FormModel<'_>,
    ) -> Result<(&'static str, Value), Vec<(SolidField, String)>> {
        let request = self.move_request(model)?;
        if self.move_is_component() {
            return self
                .move_occurrence_payload(model, &request)
                .map_err(|e| vec![(SolidField::Bodies, e)]);
        }
        Ok(if let Some(id) = self.feature {
            (
                "solid_edit_move_copy",
                json!({"feature_id":id,"request":request}),
            )
        } else {
            ("solid_move_copy", json!(request))
        })
    }
    pub(crate) fn edit_move(definition: &Value, model: &FormModel<'_>) -> Result<Self, String> {
        let BodyFeatureDefinitionDto::MoveCopy {
            feature_id,
            body_ids,
            translation,
            rotation,
            pivot,
            copy,
            ..
        } = serde_json::from_value(definition.clone()).map_err(|e| e.to_string())?
        else {
            return Err("The selected feature is not Move/Copy".into());
        };
        let mut form = Self::new_kind(SolidFormKind::MoveCopy, model);
        form.feature = Some(feature_id);
        let f = form.move_copy.as_mut().unwrap();
        let units = model.document.settings.units;
        f.bodies = body_ids;
        f.copy = copy;
        f.manual_pivot = true;
        f.translation = vector(xyz(translation).to_array(), DimensionKind::Length, units);
        f.pivot = vector(xyz(pivot).to_array(), DimensionKind::Length, units);
        let q = DQuat::from_array(rotation);
        if !q.is_finite() || q.length_squared() < 1e-18 {
            return Err("The saved rotation is invalid".into());
        }
        let (z, y, x) = q.normalize().to_euler(EulerRot::ZYX);
        f.rotation = vector(
            [x.to_degrees(), y.to_degrees(), z.to_degrees()],
            DimensionKind::Angle,
            units,
        );
        f.exact_rotation = Some(rotation);
        form.move_payload(model).map_err(first_error)?;
        Ok(form)
    }
    pub(crate) fn drag_move(
        &mut self,
        translation: [f64; 3],
        rotation: [f64; 4],
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        let f = self.move_copy.as_mut().ok_or("Open Move/Copy first")?;
        let q = DQuat::from_array(rotation);
        if f.mode != MoveMode::Free
            || !q.is_finite()
            || q.length_squared() < 1e-18
            || !translation.iter().all(|v| v.is_finite())
        {
            return Err("The move handle is unavailable".into());
        }
        let units = model.document.settings.units;
        f.translation = vector(translation, DimensionKind::Length, units);
        let (z, y, x) = q.normalize().to_euler(EulerRot::ZYX);
        f.rotation = vector(
            [x.to_degrees(), y.to_degrees(), z.to_degrees()],
            DimensionKind::Angle,
            units,
        );
        f.exact_rotation = Some(q.normalize().to_array());
        self.changed();
        Ok(())
    }
    pub(super) fn move_fields(&self, model: &FormModel<'_>) -> Vec<SolidFieldView> {
        use SolidField as F;
        let f = self.move_copy.as_ref().unwrap();
        let issues = self.move_request(model).err().unwrap_or_default();
        let text = |v: &MeasurementInput| Field::Text {
            value: v.text().into(),
            read_only: false,
            selection: None,
        };
        let mut rows = vec![
            (
                F::MoveObjectType,
                "Object type".into(),
                Field::Choice {
                    value: if f.component { "component" } else { "bodies" }.into(),
                    options: vec![
                        ChoiceOption {
                            value: "bodies".into(),
                            label: "Bodies".into(),
                            disabled: false,
                        },
                        ChoiceOption {
                            value: "component".into(),
                            label: "Component".into(),
                            disabled: model
                                .assembly
                                .is_none_or(|a| a.component_structure.occurrences.is_empty()),
                        },
                    ],
                },
            ),
            (
                F::Bodies,
                if f.component {
                    model
                        .assembly
                        .and_then(|a| {
                            a.component_structure
                                .occurrences
                                .iter()
                                .find(|o| Some(o.id.0) == f.occurrence)
                        })
                        .map(|o| format!("Component selected · {}", o.name))
                        .unwrap_or_else(|| "Click a component".into())
                } else if f.bodies.is_empty() {
                    "Click one or more bodies".into()
                } else {
                    format!(
                        "{} selected · {}",
                        if f.bodies.len() == 1 {
                            "1 body".into()
                        } else {
                            format!("{} bodies", f.bodies.len())
                        },
                        model
                            .scene
                            .bodies
                            .iter()
                            .filter(|b| f.bodies.contains(&b.id))
                            .map(|b| b.name.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                },
                Field::None,
            ),
            (
                F::MoveMode,
                "Move type".into(),
                Field::Choice {
                    value: f.mode.key().into(),
                    options: [
                        ("free", "Free move — XYZ + XYZ rotation"),
                        ("translate", "Translate — direction and distance"),
                        ("rotate", "Rotate — axis and angle"),
                        ("point_to_point", "Point to point"),
                    ]
                    .map(|(value, label)| ChoiceOption {
                        value: value.into(),
                        label: label.into(),
                        disabled: false,
                    })
                    .into(),
                },
            ),
        ];
        let mut add_vector = |keys: [F; 3], caption: &str, values: &[MeasurementInput; 3]| {
            for (i, key) in keys.into_iter().enumerate() {
                rows.push((
                    key,
                    format!("{caption} {}", ["X", "Y", "Z"][i]),
                    text(&values[i]),
                ));
            }
        };
        if f.mode == MoveMode::Free {
            add_vector(
                [F::TranslationX, F::TranslationY, F::TranslationZ],
                "Translation",
                &f.translation,
            );
            add_vector(
                [F::RotationX, F::RotationY, F::RotationZ],
                "Rotation",
                &f.rotation,
            );
        }
        if matches!(f.mode, MoveMode::Translate | MoveMode::Rotate) {
            let rotate = f.mode == MoveMode::Rotate;
            rows.push((
                if rotate {
                    F::AxisEdge
                } else {
                    F::DirectionEdge
                },
                if f.edges[usize::from(rotate)].is_some() {
                    "Straight edge selected"
                } else {
                    "Pick a straight edge or enter XYZ"
                }
                .into(),
                Field::None,
            ));
            let (keys, values, label) = if rotate {
                (
                    [
                        F::SecondDirectionX,
                        F::SecondDirectionY,
                        F::SecondDirectionZ,
                    ],
                    &f.axis,
                    "Axis",
                )
            } else {
                (
                    [F::DirectionX, F::DirectionY, F::DirectionZ],
                    &f.direction,
                    "Direction",
                )
            };
            for (i, key) in keys.into_iter().enumerate() {
                rows.push((
                    key,
                    format!("{label} {}", ["X", "Y", "Z"][i]),
                    text(&values[i]),
                ));
            }
            rows.push(if rotate {
                (F::Angle, "Angle (deg)".into(), text(&f.angle))
            } else {
                (F::Distance, "Distance".into(), text(&f.distance))
            });
        }
        if f.mode == MoveMode::PointToPoint {
            for (key, keys, label, values) in [
                (
                    F::FromPoint,
                    [F::FromX, F::FromY, F::FromZ],
                    "From point",
                    &f.from,
                ),
                (F::ToPoint, [F::ToX, F::ToY, F::ToZ], "To point", &f.to),
            ] {
                rows.push((
                    key,
                    "Click a point or enter coordinates".into(),
                    Field::None,
                ));
                for (i, key) in keys.into_iter().enumerate() {
                    rows.push((
                        key,
                        format!("{label} {}", ["X", "Y", "Z"][i]),
                        text(&values[i]),
                    ));
                }
            }
        }
        if matches!(f.mode, MoveMode::Free | MoveMode::Rotate) {
            rows.push((
                F::PivotPoint,
                "Click a point or enter coordinates".into(),
                Field::None,
            ));
            for (i, key) in [F::PivotX, F::PivotY, F::PivotZ].into_iter().enumerate() {
                rows.push((
                    key,
                    format!("Rotation pivot {}", ["X", "Y", "Z"][i]),
                    text(&f.pivot[i]),
                ));
            }
        }
        rows.push((F::Copy, "Create copy".into(), Field::Toggle(f.copy)));
        let editable = self.phase == Phase::Editing && self.check_model(model).is_ok();
        rows.into_iter()
            .map(|(field, label, value)| SolidFieldView {
                field,
                label,
                value,
                visible: true,
                enabled: editable
                    && !(matches!(field, F::Copy | F::MoveObjectType) && self.feature.is_some()),
                error: issues
                    .iter()
                    .find(|(k, _)| *k == field)
                    .map(|(_, e)| e.clone()),
            })
            .collect()
    }
}
