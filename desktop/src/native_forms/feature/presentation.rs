//! Display metadata comes from accepted references, never from placeholder text.
use super::*;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub(crate) struct ReferencePresentation {
    pub caption: String,
    pub has_selection: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SolidFormPresentation {
    pub references: HashMap<SolidField, ReferencePresentation>,
    pub operation_hint: Option<&'static str>,
    pub automatic_hint: Option<&'static str>,
    pub auto_focus: Option<(SolidField, String)>,
    pub units: String,
}

impl SolidForm {
    pub(crate) fn planar_source(&self) -> Option<PlanarFaceSourceDto> {
        match self.source {
            ProfileSource::Face(face) => Some(face),
            _ => None,
        }
    }
    /// Keep the projected editor alive while its number is being replaced.
    pub(crate) fn extrude_handle_request(&self, model: &FormModel<'_>) -> Option<ExtrudeRequest> {
        if self.kind() != SolidFormKind::Extrude
            || !matches!(self.extent, ExtrudeExtent::Distance { .. })
        {
            return None;
        }
        let (sketch_name, profile_indices, source_face) = match &self.source {
            ProfileSource::None => return None,
            ProfileSource::Profiles {
                sketch_name,
                indices,
            } => (sketch_name.clone(), indices.clone(), None),
            ProfileSource::Face(face) => (String::new(), vec![], Some(*face)),
        };
        Some(ExtrudeRequest {
            sketch_name,
            profile_indices,
            source_face,
            operation: self.operation,
            extent: ExtrudeExtent::Distance {
                distance: self
                    .distance
                    .evaluate(model.document.settings.units, model.parameters)
                    .unwrap_or(10.),
            },
            taper_angle_deg: 0.,
            flip: self.flip,
            target_body_ids: self.targets.clone(),
        })
    }

    pub(crate) fn drag_extrude_distance(
        &mut self,
        effective: f64,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        if self.extrude_handle_request(model).is_none() || !effective.is_finite() {
            return Err("The extrusion distance is not available".into());
        }
        let value = MeasurementInput::new(
            DimensionKind::Length,
            effective * if self.flip { -1. } else { 1. },
            model.document.settings.units,
        );
        self.set_value(SolidField::Distance, value.text(), model)
    }
    fn has_reference(&self, field: SolidField) -> bool {
        use SolidField::*;
        match field {
            Source => {
                !self.selected_profiles().is_empty()
                    || matches!(self.source, ProfileSource::Face(_))
            }
            Edges => self
                .selected_edges()
                .is_some_and(|(_, ids)| !ids.is_empty()),
            Faces => self
                .selected_faces()
                .is_some_and(|(_, ids)| !ids.is_empty()),
            Cylinder => self.thread_face().is_some(),
            HoleSupport => self.hole_support().is_some(),
            HolePositions => self.hole_position_count() > 0,
            Bodies => !self.selected_bodies().is_empty() || self.move_occurrence().is_some(),
            TargetBody | ToolBodies => !self.combine_bodies(field).is_empty(),
            Targets => !self.targets.is_empty(),
            StopFace => self.stop_face.is_some(),
            FirstPlane | SecondPlane => self.plane_reference(field).is_some(),
            AxisLine => self.has_axis_line(),
            Path | Guide => self
                .path(field)
                .is_some_and(|path| !path.entity_ids.is_empty()),
            field if field.is_move_point() => self.has_move_reference(field),
            field if field.is_straight_reference() => {
                if self.kind() == SolidFormKind::MoveCopy {
                    self.has_move_reference(field)
                } else if self.kind().is_pattern() {
                    self.has_pattern_reference(field)
                } else {
                    self.plane_axis().is_some()
                }
            }
            _ => false,
        }
    }

    pub(crate) fn presentation(&self, model: &FormModel<'_>) -> SolidFormPresentation {
        use SolidField as F;
        use SolidFormKind as K;
        let fields = self.fields(model);
        let mut result = SolidFormPresentation {
            units: json!(model.document.settings.units)
                .as_str()
                .unwrap()
                .into(),
            ..Default::default()
        };
        for row in fields.iter().filter(|row| {
            row.visible && matches!(row.value, Field::None) && !row.field.is_hole_position_action()
        }) {
            let selected = self.has_reference(row.field);
            let caption = if selected && row.field == F::Source {
                match &self.source {
                    ProfileSource::Face(face) => model
                        .scene
                        .bodies
                        .iter()
                        .find(|b| b.id == face.body_id)
                        .map(|b| format!("{} · planar face selected", b.name))
                        .unwrap_or_else(|| row.label.clone()),
                    _ => {
                        let profiles = self.selected_profiles();
                        let count = profiles.len();
                        let noun = if self.kind() == K::Loft {
                            "section"
                        } else {
                            "profile"
                        };
                        let sketch = profiles
                            .first()
                            .filter(|first| {
                                profiles.iter().all(|p| p.sketch_name == first.sketch_name)
                            })
                            .map(|p| format!(" · {}", p.sketch_name))
                            .unwrap_or_default();
                        format!(
                            "{count} {noun}{} selected{sketch}",
                            if count == 1 { "" } else { "s" }
                        )
                    }
                }
            } else if selected && row.field == F::HolePositions {
                let count = self.hole_position_count();
                format!(
                    "{count} position{} selected",
                    if count == 1 { "" } else { "s" }
                )
            } else if row.field == F::HolePositions {
                if self.hole_support().is_some() {
                    "No positions selected".into()
                } else {
                    "Select a support face first".into()
                }
            } else if selected && row.field == F::Targets {
                format!(
                    "{} {} selected",
                    self.targets.len(),
                    if self.targets.len() == 1 {
                        "body"
                    } else {
                        "bodies"
                    }
                )
            } else if selected && matches!(row.field, F::Edges | F::Faces) {
                let (body, count, noun) = if row.field == F::Edges {
                    let (body, ids) = self.selected_edges().unwrap();
                    (body, ids.len(), "edge")
                } else {
                    let (body, ids) = self.selected_faces().unwrap();
                    (body, ids.len(), "face")
                };
                let name = model
                    .scene
                    .bodies
                    .iter()
                    .find(|b| b.id == body)
                    .map(|b| format!(" · {}", b.name))
                    .unwrap_or_default();
                format!(
                    "{count} {noun}{} selected{name}",
                    if count == 1 { "" } else { "s" }
                )
            } else if selected
                && matches!(row.field, F::TargetBody | F::ToolBodies | F::Bodies)
                && !self.move_is_component()
            {
                let bodies = if row.field == F::Bodies {
                    self.selected_bodies().to_vec()
                } else {
                    self.combine_bodies(row.field)
                };
                if bodies.len() == 1 {
                    model
                        .scene
                        .bodies
                        .iter()
                        .find(|b| b.id == bodies[0])
                        .map(|b| format!("{} selected", b.name))
                        .unwrap_or_else(|| row.label.clone())
                } else {
                    format!("{} bodies selected", bodies.len())
                }
            } else if selected && matches!(row.field, F::Path | F::Guide) {
                let path = self.path(row.field).unwrap();
                format!(
                    "{} curve{} selected · {}",
                    path.entity_ids.len(),
                    if path.entity_ids.len() == 1 { "" } else { "s" },
                    path.sketch_name
                )
            } else if selected && row.field.is_straight_reference() {
                "Straight edge selected".into()
            } else {
                row.label.clone()
            };
            result.references.insert(
                row.field,
                ReferencePresentation {
                    caption,
                    has_selection: selected,
                },
            );
        }
        if self.kind() == K::Extrude {
            result.operation_hint = Some(match self.operation {
                ExtrudeOperation::NewBody if self.selected_profiles().len() > 1 => {
                    "extrude.newBodyMultipleHint"
                }
                ExtrudeOperation::NewBody => "extrude.newBodyHint",
                ExtrudeOperation::Join if self.targets.is_empty() => "extrude.joinProfilesHint",
                ExtrudeOperation::Join => "extrude.joinTargetHint",
                ExtrudeOperation::Cut => "extrude.cutHint",
                ExtrudeOperation::Intersect => "extrude.intersectHint",
            });
            // Do not describe a boolean inference the draft has not performed.
            if !self.operation_manual
                && !self.selected_profiles().is_empty()
                && model.scene.bodies.is_empty()
            {
                result.automatic_hint = Some("extrude.autoNewBodyHint");
            }
        }
        let (focus, sources): (F, &[F]) = match self.kind() {
            K::Extrude => (F::Distance, &[F::Source]),
            K::Revolve => (F::Angle, &[F::Source, F::AxisLine]),
            K::Fillet => (F::Radius, &[F::Edges]),
            K::Chamfer => (F::Distance, &[F::Edges]),
            K::Shell => (F::Thickness, &[F::Faces]),
            K::Hole => (F::HoleDiameter, &[F::HoleSupport]),
            K::Rib => (F::Thickness, &[F::Path]),
            K::OffsetPlane => (F::Distance, &[F::FirstPlane]),
            K::AnglePlane => (F::Angle, &[F::FirstPlane, F::AxisEdge]),
            _ => return result,
        };
        if fields
            .iter()
            .any(|r| r.field == focus && r.visible && r.enabled)
        {
            let signature = fields
                .iter()
                .filter(|r| sources.contains(&r.field) && self.has_reference(r.field))
                .map(|r| format!("{:?}:{}", r.field, r.label))
                .collect::<Vec<_>>()
                .join(";");
            if !signature.is_empty() {
                result.auto_focus = Some((focus, signature));
            }
        }
        result
    }

    pub(crate) fn step_value(
        &mut self,
        field: SolidField,
        delta: i32,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        let row = self
            .fields(model)
            .into_iter()
            .find(|r| r.field == field && r.visible && r.enabled)
            .ok_or("This measurement is not available")?;
        let Field::Text { value, .. } = row.value else {
            return Err("This is not a numeric field".into());
        };
        let kind = field.dimension_kind().ok_or("This field contains text")?;
        let mut input = MeasurementInput::new(kind, 0., model.document.settings.units);
        input.set_text(value);
        let value = input.evaluate(model.document.settings.units, model.parameters)?;
        let display = MeasurementInput::new(kind, value, model.document.settings.units)
            .text()
            .parse::<f64>()
            .unwrap();
        let next = display + f64::from(delta);
        if !next.is_finite() {
            return Err("The measurement exceeds its range".into());
        }
        self.set_value(field, &next.to_string(), model)
    }
}

impl SolidField {
    pub(crate) fn is_hole_position_action(self) -> bool {
        matches!(
            self,
            Self::HolePositionAdd | Self::HolePositionRemove(_) | Self::HolePositionIndependent(_)
        )
    }

    pub(crate) fn dimension_kind(self) -> Option<DimensionKind> {
        use SolidField::*;
        Some(match self {
            ThreadClass
            | Designation
            | HolePositionSelection
            | HolePositionAdd
            | HolePositionRemove(_)
            | HolePositionIndependent(_) => return None,
            Taper | Angle | RotationX | RotationY | RotationZ | CountersinkAngle
            | DrillPointAngle => DimensionKind::Angle,
            Count | SecondCount | DirectionX | DirectionY | DirectionZ | SecondDirectionX
            | SecondDirectionY | SecondDirectionZ => DimensionKind::Unitless,
            _ => DimensionKind::Length,
        })
    }
}
