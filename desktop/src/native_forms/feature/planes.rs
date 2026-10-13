//! Reference planes use the engine's own geometry for validation and preview.
use super::*;
use limo_cad_core::{EdgeId, PlaneBasis, PlaneRef, UnitSystem};
use limo_cad_solid::{DatumPlaneDefinitionDto, DatumPlaneSourceDto};

impl SolidFormKind {
    pub(crate) fn is_plane(self) -> bool {
        matches!(self, Self::OffsetPlane | Self::Midplane | Self::AnglePlane)
    }
}

#[derive(Debug)]
pub(super) struct PlaneFields {
    pub kind: SolidFormKind,
    first: Option<PlaneRef>,
    second: Option<PlaneRef>,
    axis: Option<(BodyId, EdgeId)>,
    distance: MeasurementInput,
    angle: MeasurementInput,
}
impl PlaneFields {
    pub fn new(kind: SolidFormKind, units: UnitSystem) -> Self {
        Self {
            kind,
            first: None,
            second: None,
            axis: None,
            distance: MeasurementInput::new(DimensionKind::Length, 10., units),
            angle: MeasurementInput::new(DimensionKind::Angle, 45., units),
        }
    }
    pub fn set(&mut self, field: SolidField, value: &str) -> Result<(), String> {
        match field {
            SolidField::Distance => self.distance.set_text(value.into()),
            SolidField::Angle => self.angle.set_text(value.into()),
            _ => return Err("This construction-plane field is not editable".into()),
        }
        Ok(())
    }
}

pub(crate) fn reference_basis(
    reference: PlaneRef,
    model: &FormModel<'_>,
) -> Result<PlaneBasis, String> {
    match reference {
        PlaneRef::OriginPlane { .. } => reference.origin_basis().map_err(|e| e.to_string()),
        PlaneRef::PlanarFace { face_id } => model
            .scene
            .bodies
            .iter()
            .flat_map(|body| &body.faces)
            .find(|face| face.id == face_id)
            .and_then(|face| face.plane)
            .ok_or_else(|| "The selected face is missing or no longer planar".into()),
        PlaneRef::DatumPlane { datum_id } => model
            .datum_planes
            .iter()
            .find(|plane| {
                plane.datum_id == datum_id
                    && model
                        .document
                        .features
                        .iter()
                        .take(model.document.rollback_index)
                        .any(|f| f.id == plane.feature_id)
            })
            .map(|plane| plane.basis)
            .ok_or_else(|| "The construction plane is missing or rolled back".into()),
    }
}
pub(super) fn reference_label(reference: Option<PlaneRef>, model: &FormModel<'_>) -> String {
    match reference {
        Some(PlaneRef::OriginPlane { plane }) => {
            format!("{} origin plane", format!("{plane:?}").to_uppercase())
        }
        Some(PlaneRef::DatumPlane { datum_id }) => model
            .datum_planes
            .iter()
            .find(|p| p.datum_id == datum_id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Missing construction plane".into()),
        Some(PlaneRef::PlanarFace { face_id }) => model
            .scene
            .bodies
            .iter()
            .find_map(|b| {
                b.faces
                    .iter()
                    .position(|f| f.id == face_id)
                    .map(|i| format!("{} · Planar face {}", b.name, i + 1))
            })
            .unwrap_or_else(|| "Missing planar face".into()),
        None => "Click a planar face or reference plane".into(),
    }
}

impl SolidForm {
    pub(crate) fn edit_plane(definition: &Value, model: &FormModel<'_>) -> Result<Self, String> {
        let definition: DatumPlaneDefinitionDto =
            serde_json::from_value(definition.clone()).map_err(|e| e.to_string())?;
        let units = model.document.settings.units;
        let (kind, first, second, axis, distance, angle) = match definition.source {
            DatumPlaneSourceDto::Offset {
                reference,
                distance,
            } => (
                SolidFormKind::OffsetPlane,
                reference,
                None,
                None,
                distance,
                45.,
            ),
            DatumPlaneSourceDto::Midplane { first, second } => {
                (SolidFormKind::Midplane, first, Some(second), None, 10., 45.)
            }
            DatumPlaneSourceDto::AtAngle {
                reference,
                body_id,
                edge_id,
                angle_deg,
                ..
            } => (
                SolidFormKind::AnglePlane,
                reference,
                None,
                Some((body_id, edge_id)),
                10.,
                angle_deg,
            ),
        };
        let mut form = Self::new_kind(kind, model);
        form.feature = Some(definition.feature_id);
        form.planes = Some(PlaneFields {
            kind,
            first: Some(first),
            second,
            axis,
            distance: MeasurementInput::new(DimensionKind::Length, distance, units),
            angle: MeasurementInput::new(DimensionKind::Angle, angle, units),
        });
        form.plane_source(model).map_err(first_error)?;
        Ok(form)
    }
    pub(crate) fn plane_reference(&self, field: SolidField) -> Option<PlaneRef> {
        if let Some(fields) = &self.body_planes {
            return if field == SolidField::FirstPlane {
                fields.plane
            } else {
                None
            };
        }
        self.planes.as_ref().and_then(|p| match field {
            SolidField::FirstPlane => p.first,
            SolidField::SecondPlane => p.second,
            _ => None,
        })
    }
    pub(crate) fn set_plane_reference(
        &mut self,
        field: SolidField,
        reference: Option<PlaneRef>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        if let Some(reference) = reference {
            reference_basis(reference, model)?;
        }
        if let Some(fields) = &mut self.body_planes {
            if field != SolidField::FirstPlane {
                return Err("This feature has only one reference plane".into());
            }
            fields.plane = reference;
            self.changed();
            return Ok(());
        }
        let planes = self
            .planes
            .as_mut()
            .ok_or("This feature has no plane selector")?;
        match field {
            SolidField::FirstPlane => planes.first = reference,
            SolidField::SecondPlane if planes.kind == SolidFormKind::Midplane => {
                planes.second = reference
            }
            _ => return Err("This field does not select a plane".into()),
        }
        self.changed();
        Ok(())
    }
    pub(crate) fn plane_axis(&self) -> Option<(BodyId, EdgeId)> {
        self.planes.as_ref().and_then(|p| p.axis)
    }
    pub(crate) fn set_plane_axis(
        &mut self,
        axis: Option<(BodyId, EdgeId)>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        if let Some((body, edge)) = axis {
            if !model
                .scene
                .bodies
                .iter()
                .any(|b| b.id == body && b.edges.iter().any(|e| e.id == edge))
            {
                return Err("The axis edge no longer exists".into());
            }
        }
        let planes = self
            .planes
            .as_mut()
            .ok_or("This feature has no plane axis")?;
        if planes.kind != SolidFormKind::AnglePlane {
            return Err("Only Plane at Angle uses an axis edge".into());
        }
        planes.axis = axis;
        self.changed();
        Ok(())
    }
    fn plane_source(
        &self,
        model: &FormModel<'_>,
    ) -> Result<DatumPlaneSourceDto, Vec<(SolidField, String)>> {
        use SolidField as F;
        let planes = self.planes.as_ref().unwrap();
        let issue = |field, e: String| vec![(field, e)];
        self.check_model(model)
            .map_err(|e| issue(F::FirstPlane, e))?;
        if self.feature.is_some_and(|id| {
            !model
                .document
                .features
                .iter()
                .any(|f| f.id == id && f.kind == FeatureKind::ConstructionPlane)
        }) {
            return Err(issue(
                F::FirstPlane,
                "The edited construction plane no longer exists".into(),
            ));
        }
        let reference = planes
            .first
            .ok_or_else(|| issue(F::FirstPlane, "Choose a reference plane".into()))?;
        let mut source = match planes.kind {
            SolidFormKind::OffsetPlane => DatumPlaneSourceDto::Offset {
                reference,
                distance: planes
                    .distance
                    .evaluate(model.document.settings.units, model.parameters)
                    .map_err(|e| issue(F::Distance, e))?,
            },
            SolidFormKind::Midplane => DatumPlaneSourceDto::Midplane {
                first: reference,
                second: planes.second.ok_or_else(|| {
                    issue(F::SecondPlane, "Choose the second parallel plane".into())
                })?,
            },
            SolidFormKind::AnglePlane => {
                let (body_id, edge_id) = planes.axis.ok_or_else(|| {
                    issue(
                        F::AxisEdge,
                        "Choose a straight edge on the reference plane".into(),
                    )
                })?;
                DatumPlaneSourceDto::AtAngle {
                    reference,
                    body_id,
                    edge_id,
                    angle_deg: planes
                        .angle
                        .evaluate(model.document.settings.units, model.parameters)
                        .map_err(|e| issue(F::Angle, e))?,
                    axis_points: None,
                }
            }
            _ => unreachable!(),
        };
        Self::resolve_plane_source(&mut source, model).map_err(|e| {
            issue(
                match planes.kind {
                    SolidFormKind::Midplane => F::SecondPlane,
                    SolidFormKind::AnglePlane => F::AxisEdge,
                    _ => F::Distance,
                },
                e,
            )
        })?;
        Ok(source)
    }
    fn resolve_plane_source(
        source: &mut DatumPlaneSourceDto,
        model: &FormModel<'_>,
    ) -> Result<PlaneBasis, String> {
        limo_cad_sketch::construction_plane_basis(
            source,
            |reference| {
                reference_basis(reference, model)
                    .map_err(limo_cad_sketch::SessionError::BrokenReference)
            },
            |body, edge| {
                model
                    .scene
                    .bodies
                    .iter()
                    .find(|b| b.id == body)?
                    .edges
                    .iter()
                    .find(|e| e.id == edge)
                    .map(|e| e.points.clone())
            },
        )
        .map_err(|e| e.to_string())
    }
    pub(crate) fn plane_preview(
        &self,
        model: &FormModel<'_>,
    ) -> Result<Option<PlaneBasis>, String> {
        if self.planes.is_none() {
            return Ok(None);
        }
        let Ok(mut source) = self.plane_source(model) else {
            return Ok(None);
        };
        Self::resolve_plane_source(&mut source, model).map(Some)
    }
    pub(crate) fn plane_guides(&self, model: &FormModel<'_>) -> Vec<PlaneBasis> {
        [SolidField::FirstPlane, SolidField::SecondPlane]
            .into_iter()
            .filter_map(|field| self.plane_reference(field))
            .filter_map(|reference| reference_basis(reference, model).ok())
            .collect()
    }
    pub(crate) fn plane_offset(&self, model: &FormModel<'_>) -> Option<(PlaneBasis, f64)> {
        let fields = self
            .planes
            .as_ref()
            .filter(|p| p.kind == SolidFormKind::OffsetPlane)?;
        Some((
            reference_basis(fields.first?, model).ok()?,
            fields
                .distance
                .evaluate(model.document.settings.units, model.parameters)
                .ok()?,
        ))
    }
    pub(crate) fn drag_plane_offset(
        &mut self,
        distance: f64,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        if !distance.is_finite() {
            return Err("Offset distance must be finite".into());
        }
        let fields = self
            .planes
            .as_mut()
            .filter(|p| p.kind == SolidFormKind::OffsetPlane)
            .ok_or("This feature has no offset handle")?;
        fields.distance = MeasurementInput::new(
            DimensionKind::Length,
            distance,
            model.document.settings.units,
        );
        self.changed();
        Ok(())
    }
    pub(super) fn plane_payload(
        &self,
        model: &FormModel<'_>,
    ) -> Result<(&'static str, Value), Vec<(SolidField, String)>> {
        let source = self.plane_source(model)?;
        let mut arguments = json!(source);
        arguments.as_object_mut().unwrap().remove("type");
        let operation = if let Some(id) = self.feature {
            arguments["feature_id"] = json!(id);
            match self.kind() {
                SolidFormKind::OffsetPlane => "construction_plane_edit_offset",
                SolidFormKind::Midplane => "construction_plane_edit_midplane",
                SolidFormKind::AnglePlane => "construction_plane_edit_at_angle",
                _ => unreachable!(),
            }
        } else {
            self.kind().operation()
        };
        Ok((operation, arguments))
    }
    pub(super) fn plane_fields(&self, model: &FormModel<'_>) -> Vec<SolidFieldView> {
        use SolidField as F;
        let planes = self.planes.as_ref().unwrap();
        let mut fields = vec![(
            F::FirstPlane,
            reference_label(planes.first, model),
            Field::None,
        )];
        let text = |input: &MeasurementInput| Field::Text {
            value: input.text().into(),
            read_only: false,
            selection: None,
        };
        match planes.kind {
            SolidFormKind::OffsetPlane => fields.push((
                F::Distance,
                "Offset distance".into(),
                text(&planes.distance),
            )),
            SolidFormKind::Midplane => fields.push((
                F::SecondPlane,
                reference_label(planes.second, model),
                Field::None,
            )),
            SolidFormKind::AnglePlane => {
                let axis = planes
                    .axis
                    .and_then(|(id, _)| model.scene.bodies.iter().find(|b| b.id == id))
                    .map(|b| format!("{} · Straight edge selected", b.name))
                    .unwrap_or_else(|| "Click a straight axis edge".into());
                fields.push((F::AxisEdge, axis, Field::None));
                fields.push((F::Angle, "Angle".into(), text(&planes.angle)));
            }
            _ => unreachable!(),
        }
        let issues = self.plane_source(model).err().unwrap_or_default();
        let enabled = self.phase == Phase::Editing && self.check_model(model).is_ok();
        fields
            .into_iter()
            .map(|(field, label, value)| SolidFieldView {
                field,
                label,
                value,
                visible: true,
                enabled,
                error: issues
                    .iter()
                    .find(|(f, _)| *f == field)
                    .map(|(_, e)| e.clone()),
            })
            .collect()
    }
}
