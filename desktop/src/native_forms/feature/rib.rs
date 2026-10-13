//! Rib-specific dimensions and extents on the shared Build transaction.
use super::*;
use limo_cad_core::UnitSystem;
use limo_cad_solid::{EditRibRequest, PathRefDto, RibDefinitionDto, RibExtent, RibRequest};

#[derive(Debug)]
pub(super) struct RibFields {
    pub centerline: Option<PathRefDto>,
    thickness: MeasurementInput,
    depth: MeasurementInput,
    extent: RibExtent,
    symmetric: bool,
}
impl RibFields {
    pub fn new(units: UnitSystem) -> Self {
        Self {
            centerline: None,
            thickness: MeasurementInput::new(DimensionKind::Length, 2., units),
            depth: MeasurementInput::new(DimensionKind::Length, 10., units),
            extent: RibExtent::Distance { depth: 10. },
            symmetric: false,
        }
    }
    pub fn set(&mut self, field: SolidField, value: &str) -> Result<(), String> {
        match field {
            SolidField::Thickness => self.thickness.set_text(value.into()),
            SolidField::Distance => self.depth.set_text(value.into()),
            SolidField::Extent => {
                self.extent = serde_json::from_value(json!({"type":value,"depth":10.,"face_id":0}))
                    .map_err(|e| e.to_string())?
            }
            SolidField::Symmetric => {
                self.symmetric = match value {
                    "true" => true,
                    "false" => false,
                    _ => return Err("Symmetric expects true or false".into()),
                }
            }
            _ => return Err("This Rib field is not editable".into()),
        }
        Ok(())
    }
}
impl SolidForm {
    pub(crate) fn edit_rib(d: &RibDefinitionDto, model: &FormModel<'_>) -> Result<Self, String> {
        if !model
            .document
            .features
            .iter()
            .any(|f| f.id == d.feature_id && f.kind == FeatureKind::Rib)
        {
            return Err("The selected Rib no longer exists".into());
        }
        let mut form = Self::new_kind(SolidFormKind::Rib, model);
        form.feature = Some(d.feature_id);
        form.operation = d.operation;
        form.operation_manual = true;
        form.targets = d.target_body_ids.clone();
        form.flip = d.flip;
        let p = form.rib.as_mut().unwrap();
        p.centerline = Some(PathRefDto {
            sketch_name: d.sketch_name.clone(),
            entity_ids: d.line_entity_ids.clone(),
        });
        p.thickness = MeasurementInput::new(
            DimensionKind::Length,
            d.thickness,
            model.document.settings.units,
        );
        p.extent = d.extent.unwrap_or(RibExtent::Distance { depth: d.depth });
        let depth = if let RibExtent::Distance { depth } = p.extent {
            depth
        } else {
            d.depth
        };
        p.depth =
            MeasurementInput::new(DimensionKind::Length, depth, model.document.settings.units);
        p.symmetric = d.symmetric;
        if let RibExtent::ToFace { face_id } = p.extent {
            let mut refs = model.scene.bodies.iter().flat_map(|b| {
                b.faces
                    .iter()
                    .filter(move |f| f.id == face_id && f.plane.is_some())
                    .map(move |_| PlanarFaceSourceDto {
                        body_id: b.id,
                        face_id,
                    })
            });
            form.stop_face = refs.next();
            if refs.next().is_some() {
                form.stop_face = None;
            }
        }
        Ok(form)
    }
    pub(super) fn rib_payload(
        &self,
        model: &FormModel<'_>,
    ) -> Result<(&'static str, Value), Vec<(SolidField, String)>> {
        use SolidField as F;
        let p = self.rib.as_ref().unwrap();
        let mut errors = vec![];
        if let Err(e) = self.check_model(model) {
            return Err(vec![(F::Path, e)]);
        }
        if self.feature.is_some_and(|id| {
            !model
                .document
                .features
                .iter()
                .any(|f| f.id == id && f.kind == FeatureKind::Rib)
        }) {
            errors.push((F::Path, "The edited Rib no longer exists".into()));
        }
        match &p.centerline {
            Some(path) => {
                if let Err(e) = paths::validate_path(path, model) {
                    errors.push((F::Path, e));
                }
            }
            None => errors.push((F::Path, "Select centerline curves".into())),
        }
        let mut measure = |field, input: &MeasurementInput| match input
            .evaluate(model.document.settings.units, model.parameters)
        {
            Ok(value) if value > 1e-6 => value,
            Ok(_) => {
                errors.push((
                    field,
                    "Use a positive length greater than 0.000001 mm".into(),
                ));
                0.
            }
            Err(e) => {
                errors.push((field, e));
                0.
            }
        };
        let thickness = measure(F::Thickness, &p.thickness);
        let depth = if matches!(p.extent, RibExtent::Distance { .. }) {
            measure(F::Distance, &p.depth)
        } else {
            10.
        };
        let extent = match p.extent {
            RibExtent::Distance { .. } => RibExtent::Distance { depth },
            RibExtent::ToNext => {
                if self.operation == ExtrudeOperation::NewBody {
                    errors.push((
                        F::Extent,
                        "To Next requires a target body and Join, Cut or Intersect".into(),
                    ));
                }
                RibExtent::ToNext
            }
            RibExtent::ThroughAll => {
                if let Err(error) =
                    limo_cad_solid::validate_rib_extent(self.operation, Some(RibExtent::ThroughAll))
                {
                    errors.push((F::Extent, error.to_string()));
                }
                RibExtent::ThroughAll
            }
            RibExtent::ToFace { .. } => {
                if let Some(face) = self.stop_face {
                    if let Err(e) = validate_face(face, model) {
                        errors.push((F::StopFace, e));
                    }
                    RibExtent::ToFace {
                        face_id: face.face_id,
                    }
                } else {
                    errors.push((F::StopFace, "Select a planar stop face".into()));
                    p.extent
                }
            }
        };
        if self.operation != ExtrudeOperation::NewBody {
            if self.targets.is_empty() {
                errors.push((F::Targets, "Select target bodies".into()));
            }
            if let Err(e) = validate_targets(&self.targets, model) {
                errors.push((F::Targets, e));
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let path = p.centerline.as_ref().unwrap();
        let request = RibRequest {
            sketch_name: path.sketch_name.clone(),
            line_entity_ids: path.entity_ids.clone(),
            thickness,
            depth,
            extent: Some(extent),
            symmetric: p.symmetric,
            flip: self.flip,
            operation: self.operation,
            target_body_ids: if self.operation == ExtrudeOperation::NewBody {
                vec![]
            } else {
                self.targets.clone()
            },
        };
        let (op, value) = if let Some(feature_id) = self.feature {
            (
                "solid_edit_rib",
                serde_json::to_value(EditRibRequest {
                    feature_id,
                    rib: request,
                }),
            )
        } else {
            ("solid_rib", serde_json::to_value(request))
        };
        value
            .map(|v| (op, v))
            .map_err(|e| vec![(F::Path, e.to_string())])
    }
    pub(super) fn rib_fields(&self, model: &FormModel<'_>) -> Vec<SolidFieldView> {
        use SolidField as F;
        let p = self.rib.as_ref().unwrap();
        let errors = self.rib_payload(model).err().unwrap_or_default();
        let enabled = self.phase == Phase::Editing && self.check_model(model).is_ok();
        let text = |p: &MeasurementInput| Field::Text {
            value: p.text().into(),
            read_only: false,
            selection: None,
        };
        let choice = |value: Value, options: &[(&str, &str)]| Field::Choice {
            value: value.as_str().unwrap().into(),
            options: options
                .iter()
                .map(|(value, label)| ChoiceOption {
                    value: (*value).into(),
                    label: (*label).into(),
                    disabled: false,
                })
                .collect(),
        };
        let rows = vec![
            (
                F::Path,
                p.centerline
                    .as_ref()
                    .map(|p| format!("{} · {} curve(s)", p.sketch_name, p.entity_ids.len()))
                    .unwrap_or_else(|| "Select centerline curves".into()),
                Field::None,
                true,
            ),
            (
                F::Extent,
                "Extent".into(),
                choice(
                    json!(p.extent)["type"].clone(),
                    &[
                        ("distance", "Distance"),
                        ("to_next", "To Next"),
                        ("to_face", "To Face"),
                        ("through_all", "Through All"),
                    ],
                ),
                true,
            ),
            (
                F::Thickness,
                format!(
                    "Thickness ({})",
                    json!(model.document.settings.units).as_str().unwrap()
                ),
                text(&p.thickness),
                true,
            ),
            (
                F::Distance,
                format!(
                    "Depth ({})",
                    json!(model.document.settings.units).as_str().unwrap()
                ),
                text(&p.depth),
                matches!(p.extent, RibExtent::Distance { .. }),
            ),
            (
                F::StopFace,
                self.stop_face
                    .map(|f| format!("Stop: body {} · face {}", f.body_id.0, f.face_id.0))
                    .unwrap_or_else(|| "Select stop face".into()),
                Field::None,
                matches!(p.extent, RibExtent::ToFace { .. }),
            ),
            (
                F::Symmetric,
                "Symmetric depth".into(),
                Field::Toggle(p.symmetric),
                matches!(p.extent, RibExtent::Distance { .. }),
            ),
            (
                F::Flip,
                "Flip direction".into(),
                Field::Toggle(self.flip),
                !matches!(p.extent, RibExtent::ToFace { .. }),
            ),
            (
                F::Operation,
                "Operation".into(),
                choice(
                    json!(self.operation),
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
        ];
        rows.into_iter()
            .map(|(field, label, value, visible)| SolidFieldView {
                field,
                label,
                value,
                visible,
                enabled,
                error: errors
                    .iter()
                    .find(|(f, _)| *f == field)
                    .map(|(_, e)| e.clone()),
            })
            .collect()
    }
}
