//! Body booleans keep their target distinct from the independently picked tools.
use super::*;
use limo_cad_solid::{BodyFeatureDefinitionDto, CombineOperation, CombineRequest};

#[derive(Debug)]
pub(super) struct CombineFields {
    target: Option<BodyId>,
    tools: Vec<BodyId>,
    operation: CombineOperation,
    keep_tools: bool,
}
impl Default for CombineFields {
    fn default() -> Self {
        Self {
            target: None,
            tools: vec![],
            operation: CombineOperation::Join,
            keep_tools: false,
        }
    }
}
impl CombineFields {
    pub fn set(&mut self, field: SolidField, value: &str) -> Result<(), String> {
        match field {
            SolidField::Operation => {
                self.operation = match value {
                    "join" => CombineOperation::Join,
                    "cut" => CombineOperation::Cut,
                    "intersect" => CombineOperation::Intersect,
                    _ => return Err("Choose Add, Cut or Intersect".into()),
                }
            }
            SolidField::KeepTools => {
                self.keep_tools = match value {
                    "true" => true,
                    "false" => false,
                    _ => return Err("Keep tool bodies expects true or false".into()),
                }
            }
            _ => return Err("This Combine field is not editable".into()),
        }
        Ok(())
    }
}
impl SolidForm {
    pub(crate) fn edit_combine(definition: &Value, model: &FormModel<'_>) -> Result<Self, String> {
        let BodyFeatureDefinitionDto::Combine {
            feature_id,
            target_body_id,
            tool_body_ids,
            operation,
            keep_tools,
            ..
        } = serde_json::from_value(definition.clone()).map_err(|e| e.to_string())?
        else {
            return Err("The selected feature is not Combine".into());
        };
        let mut form = Self::new_kind(SolidFormKind::Combine, model);
        form.feature = Some(feature_id);
        form.set_combine_bodies(SolidField::TargetBody, vec![target_body_id], model)?;
        form.set_combine_bodies(SolidField::ToolBodies, tool_body_ids, model)?;
        let fields = form.combine.as_mut().unwrap();
        fields.operation = operation;
        fields.keep_tools = keep_tools;
        form.combine_request(model).map_err(first_error)?;
        Ok(form)
    }
    pub(crate) fn combine_bodies(&self, field: SolidField) -> Vec<BodyId> {
        self.combine
            .as_ref()
            .map(|fields| match field {
                SolidField::TargetBody => fields.target.into_iter().collect(),
                SolidField::ToolBodies => fields.tools.clone(),
                _ => vec![],
            })
            .unwrap_or_default()
    }
    pub(crate) fn set_combine_bodies(
        &mut self,
        field: SolidField,
        bodies: Vec<BodyId>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        validate_targets(&bodies, model)?;
        let fields = self
            .combine
            .as_mut()
            .ok_or("This feature has no Combine selector")?;
        match field {
            SolidField::TargetBody => {
                if bodies.len() > 1 {
                    return Err("Choose one target body".into());
                }
                fields.target = bodies.first().copied();
                fields.tools.retain(|b| Some(*b) != fields.target);
            }
            SolidField::ToolBodies => {
                if fields.target.is_none() && !bodies.is_empty() {
                    return Err("Select the target body first".into());
                }
                if bodies.iter().any(|b| Some(*b) == fields.target) {
                    return Err("The target cannot also be a tool body".into());
                }
                fields.tools = bodies;
            }
            _ => return Err("This field does not select Combine bodies".into()),
        }
        self.changed();
        Ok(())
    }
    fn combine_request(
        &self,
        model: &FormModel<'_>,
    ) -> Result<CombineRequest, Vec<(SolidField, String)>> {
        use SolidField as F;
        let fields = self.combine.as_ref().unwrap();
        let mut issues = Vec::new();
        if let Err(e) = self.check_model(model) {
            issues.push((F::TargetBody, e));
        }
        if let Some(id) = self.feature {
            if !model
                .document
                .features
                .iter()
                .any(|f| f.id == id && f.kind == FeatureKind::Combine)
            {
                issues.push((F::TargetBody, "The edited Combine no longer exists".into()));
            }
        }
        if !fields
            .target
            .is_some_and(|id| model.scene.bodies.iter().any(|b| b.id == id))
        {
            issues.push((F::TargetBody, "Choose the target body".into()));
        }
        if fields.tools.is_empty() {
            issues.push((F::ToolBodies, "Choose one or more tool bodies".into()));
        } else if let Err(e) = validate_targets(&fields.tools, model) {
            issues.push((F::ToolBodies, e));
        }
        if fields.tools.iter().any(|b| Some(*b) == fields.target) {
            issues.push((
                F::ToolBodies,
                "The target cannot also be a tool body".into(),
            ));
        }
        if !issues.is_empty() {
            return Err(issues);
        }
        Ok(CombineRequest {
            target_body_id: fields.target.unwrap(),
            tool_body_ids: fields.tools.clone(),
            operation: fields.operation,
            keep_tools: fields.keep_tools,
        })
    }
    pub(super) fn combine_payload(
        &self,
        model: &FormModel<'_>,
    ) -> Result<(&'static str, Value), Vec<(SolidField, String)>> {
        let request = self.combine_request(model)?;
        Ok(if let Some(feature_id) = self.feature {
            (
                "solid_edit_combine",
                json!({"feature_id":feature_id,"request":request}),
            )
        } else {
            ("solid_combine", json!(request))
        })
    }
    pub(super) fn combine_fields(&self, model: &FormModel<'_>) -> Vec<SolidFieldView> {
        use SolidField as F;
        let fields = self.combine.as_ref().unwrap();
        let issues = self.combine_request(model).err().unwrap_or_default();
        let enabled = self.phase == Phase::Editing && self.check_model(model).is_ok();
        let target = model
            .scene
            .bodies
            .iter()
            .find(|b| Some(b.id) == fields.target)
            .map(|b| format!("{} selected", b.name))
            .unwrap_or_else(|| "Click the target body".into());
        let tools = if fields.tools.is_empty() {
            "Click one or more tool bodies".into()
        } else if fields.tools.len() == 1 {
            "1 tool body selected".into()
        } else {
            format!("{} tool bodies selected", fields.tools.len())
        };
        let operation = match fields.operation {
            CombineOperation::Join => "join",
            CombineOperation::Cut => "cut",
            CombineOperation::Intersect => "intersect",
        };
        vec![
            (F::TargetBody, target, Field::None),
            (F::ToolBodies, tools, Field::None),
            (
                F::Operation,
                "Operation".into(),
                Field::Choice {
                    value: operation.into(),
                    options: [
                        ("join", "Add"),
                        ("cut", "Subtract"),
                        ("intersect", "Common"),
                    ]
                    .into_iter()
                    .map(|(value, label)| ChoiceOption {
                        value: value.into(),
                        label: label.into(),
                        disabled: false,
                    })
                    .collect(),
                },
            ),
            (
                F::KeepTools,
                "Keep tool bodies".into(),
                Field::Toggle(fields.keep_tools),
            ),
        ]
        .into_iter()
        .map(|(field, label, value)| SolidFieldView {
            field,
            label,
            value,
            visible: true,
            enabled: enabled && (field != F::ToolBodies || fields.target.is_some()),
            error: issues
                .iter()
                .find(|(f, _)| *f == field)
                .map(|(_, e)| e.clone()),
        })
        .collect()
    }
}
