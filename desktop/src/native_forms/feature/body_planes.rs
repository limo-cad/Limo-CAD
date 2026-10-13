//! Mirror and Split Body share persistent plane references and source-body picking.
use super::*;
use limo_cad_core::PlaneRef;
use limo_cad_solid::{BodyFeatureDefinitionDto, SolidMirrorRequest, SplitBodyRequest};

#[derive(Debug)]
pub(super) struct BodyPlaneFields {
    pub kind: SolidFormKind,
    pub plane: Option<PlaneRef>,
    bodies: Vec<BodyId>,
}
impl BodyPlaneFields {
    pub fn new(kind: SolidFormKind) -> Self {
        Self {
            kind,
            plane: None,
            bodies: Vec::new(),
        }
    }
}
impl SolidFormKind {
    pub(crate) fn is_body_plane(self) -> bool {
        matches!(self, Self::Mirror | Self::SplitBody)
    }
    pub(crate) fn has_plane_references(self) -> bool {
        self.is_plane() || self.is_body_plane()
    }
}
impl SolidForm {
    pub(crate) fn selected_bodies(&self) -> &[BodyId] {
        if let Some(f) = &self.move_copy {
            return &f.bodies;
        }
        if let Some(fields) = &self.patterns {
            return &fields.bodies;
        }
        self.body_planes
            .as_ref()
            .map(|f| f.bodies.as_slice())
            .unwrap_or(&[])
    }
    pub(crate) fn set_bodies(
        &mut self,
        bodies: Vec<BodyId>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        validate_targets(&bodies, model)?;
        if let Some(f) = &mut self.move_copy {
            f.select(bodies, model);
            self.changed();
            return Ok(());
        }
        if let Some(fields) = &mut self.patterns {
            fields.bodies = bodies;
            self.changed();
            return Ok(());
        }
        let fields = self
            .body_planes
            .as_mut()
            .ok_or("This feature has no body selector")?;
        if fields.kind == SolidFormKind::SplitBody && bodies.len() > 1 {
            return Err("Choose one body to split".into());
        }
        fields.bodies = bodies;
        self.changed();
        Ok(())
    }
    pub(crate) fn edit_body_plane(
        definition: &Value,
        model: &FormModel<'_>,
    ) -> Result<Self, String> {
        let definition: BodyFeatureDefinitionDto =
            serde_json::from_value(definition.clone()).map_err(|e| e.to_string())?;
        let (kind, id, bodies, plane) = match definition {
            BodyFeatureDefinitionDto::Mirror {
                feature_id,
                body_ids,
                plane,
                ..
            } => (SolidFormKind::Mirror, feature_id, body_ids, plane),
            BodyFeatureDefinitionDto::SplitBody {
                feature_id,
                body_id,
                plane,
                ..
            } => (SolidFormKind::SplitBody, feature_id, vec![body_id], plane),
            _ => return Err("The selected feature is not Mirror or Split Body".into()),
        };
        let mut form = Self::new_kind(kind, model);
        form.feature = Some(id);
        form.set_bodies(bodies, model)?;
        form.set_plane_reference(SolidField::FirstPlane, Some(plane), model)?;
        form.body_plane_payload(model).map_err(first_error)?;
        Ok(form)
    }
    pub(super) fn body_plane_payload(
        &self,
        model: &FormModel<'_>,
    ) -> Result<(&'static str, Value), Vec<(SolidField, String)>> {
        use SolidField as F;
        let fields = self.body_planes.as_ref().unwrap();
        let mut issues = Vec::new();
        if let Err(e) = self.check_model(model) {
            issues.push((F::Bodies, e));
        }
        if self.feature.is_some_and(|id| {
            !model.document.features.iter().any(|f| {
                f.id == id && SolidFormKind::from_feature_kind(f.kind) == Some(fields.kind)
            })
        }) {
            issues.push((F::Bodies, "The edited feature no longer exists".into()));
        }
        if fields.bodies.is_empty() {
            issues.push((F::Bodies, "Select the source bodies".into()));
        }
        if let Err(e) = validate_targets(&fields.bodies, model) {
            issues.push((F::Bodies, e));
        }
        if fields.kind == SolidFormKind::SplitBody && fields.bodies.len() != 1 {
            issues.push((F::Bodies, "Choose one body to split".into()));
        }
        match fields.plane {
            Some(plane) => {
                if let Err(e) = planes::reference_basis(plane, model) {
                    issues.push((F::FirstPlane, e));
                }
            }
            None => issues.push((F::FirstPlane, "Choose a reference plane".into())),
        }
        if !issues.is_empty() {
            return Err(issues);
        }
        let request = if fields.kind == SolidFormKind::Mirror {
            json!(SolidMirrorRequest {
                body_ids: fields.bodies.clone(),
                plane: fields.plane.unwrap(),
                plane_basis: None
            })
        } else {
            json!(SplitBodyRequest {
                body_id: fields.bodies[0],
                plane: fields.plane.unwrap(),
                plane_basis: None
            })
        };
        Ok(if let Some(feature_id) = self.feature {
            (
                if fields.kind == SolidFormKind::Mirror {
                    "solid_edit_mirror"
                } else {
                    "solid_edit_split_body"
                },
                json!({"feature_id":feature_id,"request":request}),
            )
        } else {
            (fields.kind.operation(), request)
        })
    }
    pub(super) fn body_plane_fields(&self, model: &FormModel<'_>) -> Vec<SolidFieldView> {
        use SolidField as F;
        let fields = self.body_planes.as_ref().unwrap();
        let body = if fields.bodies.is_empty() {
            if fields.kind == SolidFormKind::Mirror {
                "Click one or more bodies"
            } else {
                "Click a body to split"
            }
            .into()
        } else if fields.bodies.len() == 1 {
            model
                .scene
                .bodies
                .iter()
                .find(|b| b.id == fields.bodies[0])
                .map(|b| format!("{} selected", b.name))
                .unwrap_or_else(|| "Missing body".into())
        } else {
            format!("{} bodies selected", fields.bodies.len())
        };
        let issues = self.body_plane_payload(model).err().unwrap_or_default();
        [
            (F::Bodies, body),
            (F::FirstPlane, planes::reference_label(fields.plane, model)),
        ]
        .into_iter()
        .map(|(field, label)| SolidFieldView {
            field,
            label,
            value: Field::None,
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
