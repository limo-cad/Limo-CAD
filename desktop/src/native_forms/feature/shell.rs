//! Shell uses the same typed measurements and revision-bound form transaction.
use super::*;
use limo_cad_core::{FaceId, UnitSystem};
use limo_cad_solid::{BodyFeatureDefinitionDto, ShellRequest};

#[derive(Debug)]
pub(super) struct ShellFields {
    body: Option<BodyId>,
    faces: Vec<FaceId>,
    thickness: MeasurementInput,
    inward: bool,
}
impl ShellFields {
    pub fn new(units: UnitSystem) -> Self {
        Self {
            body: None,
            faces: vec![],
            thickness: MeasurementInput::new(DimensionKind::Length, 2., units),
            inward: true,
        }
    }
    pub fn set(&mut self, field: SolidField, value: &str) -> Result<(), String> {
        match field {
            SolidField::Thickness => self.thickness.set_text(value.into()),
            SolidField::Inward => {
                self.inward = match value {
                    "true" => true,
                    "false" => false,
                    _ => return Err("Offset walls inward expects true or false".into()),
                }
            }
            _ => return Err("This Shell field is not editable".into()),
        }
        Ok(())
    }
}
impl SolidForm {
    pub(crate) fn edit_shell(definition: &Value, model: &FormModel<'_>) -> Result<Self, String> {
        let BodyFeatureDefinitionDto::Shell {
            feature_id,
            body_id,
            face_ids,
            thickness,
            inward,
            ..
        } = serde_json::from_value(definition.clone()).map_err(|e| e.to_string())?
        else {
            return Err("The selected feature is not a Shell".into());
        };
        let mut form = Self::new_kind(SolidFormKind::Shell, model);
        form.feature = Some(feature_id);
        form.set_faces(Some(body_id), face_ids, model)?;
        let fields = form.shell.as_mut().unwrap();
        fields.thickness = MeasurementInput::new(
            DimensionKind::Length,
            thickness,
            model.document.settings.units,
        );
        fields.inward = inward;
        form.shell_request(model).map_err(first_error)?;
        Ok(form)
    }
    pub(crate) fn selected_faces(&self) -> Option<(BodyId, &[FaceId])> {
        let fields = self.shell.as_ref()?;
        Some((fields.body?, &fields.faces))
    }
    pub(crate) fn set_faces(
        &mut self,
        body: Option<BodyId>,
        faces: Vec<FaceId>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        let fields = self
            .shell
            .as_mut()
            .ok_or("This feature has no face selector")?;
        if !faces.is_empty() {
            let source = model
                .scene
                .bodies
                .iter()
                .find(|b| Some(b.id) == body)
                .ok_or("Select faces on an existing body")?;
            if faces
                .iter()
                .any(|id| !source.faces.iter().any(|f| f.id == *id))
            {
                return Err("Select faces from the same body".into());
            }
            let unique: std::collections::HashSet<_> = faces.iter().collect();
            if unique.len() != faces.len() {
                return Err("A face can only be selected once".into());
            }
        }
        fields.body = if faces.is_empty() { None } else { body };
        fields.faces = faces;
        self.changed();
        Ok(())
    }
    fn shell_request(
        &self,
        model: &FormModel<'_>,
    ) -> Result<ShellRequest, Vec<(SolidField, String)>> {
        use SolidField as F;
        let fields = self.shell.as_ref().unwrap();
        let mut issues = Vec::new();
        if let Err(e) = self.check_model(model) {
            issues.push((F::Faces, e));
        }
        if let Some(id) = self.feature {
            if !model
                .document
                .features
                .iter()
                .any(|f| f.id == id && f.kind == FeatureKind::Shell)
            {
                issues.push((F::Faces, "The edited Shell no longer exists".into()));
            }
        }
        let body = model
            .scene
            .bodies
            .iter()
            .find(|b| Some(b.id) == fields.body);
        if body.is_none() || fields.faces.is_empty() {
            issues.push((F::Faces, "Select one or more faces to remove".into()));
        } else if fields
            .faces
            .iter()
            .any(|id| !body.unwrap().faces.iter().any(|f| f.id == *id))
        {
            issues.push((F::Faces, "A selected face no longer exists".into()));
        }
        let thickness = match fields
            .thickness
            .evaluate(model.document.settings.units, model.parameters)
        {
            Ok(v) if v > 0. => v,
            Ok(_) => {
                issues.push((F::Thickness, "Wall thickness must be positive".into()));
                0.
            }
            Err(e) => {
                issues.push((F::Thickness, e));
                0.
            }
        };
        if !issues.is_empty() {
            return Err(issues);
        }
        Ok(ShellRequest {
            body_id: body.unwrap().id,
            face_ids: fields.faces.clone(),
            thickness,
            inward: fields.inward,
        })
    }
    pub(super) fn shell_payload(
        &self,
        model: &FormModel<'_>,
    ) -> Result<(&'static str, Value), Vec<(SolidField, String)>> {
        let request = self.shell_request(model)?;
        Ok(if let Some(feature_id) = self.feature {
            (
                "solid_edit_shell",
                json!({"feature_id":feature_id,"request":request}),
            )
        } else {
            ("solid_shell", json!(request))
        })
    }
    pub(super) fn shell_fields(&self, model: &FormModel<'_>) -> Vec<SolidFieldView> {
        use SolidField as F;
        let fields = self.shell.as_ref().unwrap();
        let issues = self.shell_request(model).err().unwrap_or_default();
        let enabled = self.phase == Phase::Editing && self.check_model(model).is_ok();
        let reference = model
            .scene
            .bodies
            .iter()
            .find(|b| Some(b.id) == fields.body)
            .map(|b| format!("{} face(s) · {}", fields.faces.len(), b.name))
            .unwrap_or_else(|| "Click faces to remove".into());
        vec![
            (F::Faces, reference, Field::None),
            (
                F::Thickness,
                "Wall thickness".into(),
                Field::Text {
                    value: fields.thickness.text().into(),
                    read_only: false,
                    selection: None,
                },
            ),
            (
                F::Inward,
                "Offset walls inward".into(),
                Field::Toggle(fields.inward),
            ),
        ]
        .into_iter()
        .map(|(field, label, value)| SolidFieldView {
            field,
            label,
            value,
            enabled,
            visible: true,
            error: issues
                .iter()
                .find(|(f, _)| *f == field)
                .map(|(_, e)| e.clone()),
        })
        .collect()
    }
}
