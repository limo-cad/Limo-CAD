//! Edge refinements share the same form lifetime, dimensions and Apply ticket.
use super::*;
use limo_cad_core::{EdgeId, UnitSystem};
use limo_cad_solid::{
    EditSolidChamferRequest, EditSolidFilletRequest, SolidChamferRequest, SolidFilletRequest,
};

type EdgeValues = Result<(BodyId, Vec<EdgeId>, f64, bool), Vec<(SolidField, String)>>;

#[derive(Debug)]
pub(super) struct EdgeFields {
    pub kind: SolidFormKind,
    body: Option<BodyId>,
    edges: Vec<EdgeId>,
    size: MeasurementInput,
    tangent_chain: bool,
}
impl EdgeFields {
    pub fn new(kind: SolidFormKind, units: UnitSystem) -> Self {
        Self {
            kind,
            body: None,
            edges: vec![],
            size: MeasurementInput::new(DimensionKind::Length, 2., units),
            tangent_chain: false,
        }
    }
    pub fn set(&mut self, field: SolidField, value: &str) -> Result<(), String> {
        match field {
            SolidField::Radius if self.kind == SolidFormKind::Fillet => {
                self.size.set_text(value.into())
            }
            SolidField::Distance if self.kind == SolidFormKind::Chamfer => {
                self.size.set_text(value.into())
            }
            SolidField::TangentChain => {
                self.tangent_chain = match value {
                    "true" => true,
                    "false" => false,
                    _ => return Err("Tangent chain expects true or false".into()),
                }
            }
            _ => return Err("This edge field is not editable".into()),
        }
        Ok(())
    }
}
impl SolidForm {
    pub(crate) fn edit_edges(
        kind: SolidFormKind,
        definition: &Value,
        model: &FormModel<'_>,
    ) -> Result<Self, String> {
        let (id, body, edges, size, tangent) = if kind == SolidFormKind::Fillet {
            let d: limo_cad_solid::SolidFilletDefinitionDto =
                serde_json::from_value(definition.clone()).map_err(|e| e.to_string())?;
            (
                d.feature_id,
                d.body_id,
                d.edge_ids,
                d.radius,
                d.tangent_chain,
            )
        } else if kind == SolidFormKind::Chamfer {
            let d: limo_cad_solid::SolidChamferDefinitionDto =
                serde_json::from_value(definition.clone()).map_err(|e| e.to_string())?;
            (
                d.feature_id,
                d.body_id,
                d.edge_ids,
                d.distance,
                d.tangent_chain,
            )
        } else {
            return Err("This feature is not an edge refinement".into());
        };
        let mut form = Self::new_kind(kind, model);
        form.feature = Some(id);
        form.set_edges(Some(body), edges, model)?;
        let fields = form.edges.as_mut().unwrap();
        fields.size =
            MeasurementInput::new(DimensionKind::Length, size, model.document.settings.units);
        fields.tangent_chain = tangent;
        form.edge_values(model).map_err(first_error)?;
        Ok(form)
    }
    pub(crate) fn selected_edges(&self) -> Option<(BodyId, &[EdgeId])> {
        let fields = self.edges.as_ref()?;
        Some((fields.body?, &fields.edges))
    }
    pub(crate) fn set_edges(
        &mut self,
        body: Option<BodyId>,
        edges: Vec<EdgeId>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        let fields = self
            .edges
            .as_mut()
            .ok_or("This feature has no edge selector")?;
        if !edges.is_empty() {
            let body = model
                .scene
                .bodies
                .iter()
                .find(|b| Some(b.id) == body)
                .ok_or("Select edges on an existing body")?;
            if edges
                .iter()
                .any(|id| !body.edges.iter().any(|e| e.id == *id && e.refinable))
            {
                return Err("Select refinable edges from the same body".into());
            }
            if edges
                .iter()
                .enumerate()
                .any(|(i, id)| edges[..i].contains(id))
            {
                return Err("An edge can only be selected once".into());
            }
        }
        fields.body = if edges.is_empty() { None } else { body };
        fields.edges = edges;
        self.changed();
        Ok(())
    }
    fn edge_values(&self, model: &FormModel<'_>) -> EdgeValues {
        use SolidField as F;
        let fields = self.edges.as_ref().unwrap();
        let mut errors = Vec::new();
        if let Err(e) = self.check_model(model) {
            errors.push((F::Edges, e));
        }
        if let Some(id) = self.feature {
            let kind = if fields.kind == SolidFormKind::Fillet {
                FeatureKind::Fillet
            } else {
                FeatureKind::Chamfer
            };
            if !model
                .document
                .features
                .iter()
                .any(|f| f.id == id && f.kind == kind)
            {
                errors.push((F::Edges, "The edited feature no longer exists".into()));
            }
        }
        let body = model
            .scene
            .bodies
            .iter()
            .find(|b| Some(b.id) == fields.body);
        if body.is_none() || fields.edges.is_empty() {
            errors.push((F::Edges, "Select one or more edges on a body".into()));
        } else if fields.edges.iter().any(|id| {
            !body
                .unwrap()
                .edges
                .iter()
                .any(|e| e.id == *id && e.refinable)
        }) {
            errors.push((F::Edges, "A selected edge is no longer refinable".into()));
        }
        let size_field = if fields.kind == SolidFormKind::Fillet {
            F::Radius
        } else {
            F::Distance
        };
        let size = match fields
            .size
            .evaluate(model.document.settings.units, model.parameters)
        {
            Ok(v) if v > 0. => v,
            Ok(_) => {
                errors.push((size_field, "Size must be positive".into()));
                0.
            }
            Err(e) => {
                errors.push((size_field, e));
                0.
            }
        };
        if !errors.is_empty() {
            return Err(errors);
        }
        let body = body.unwrap();
        Ok((body.id, fields.edges.clone(), size, fields.tangent_chain))
    }
    pub(super) fn edge_payload(
        &self,
        model: &FormModel<'_>,
    ) -> Result<(&'static str, Value), Vec<(SolidField, String)>> {
        let (body_id, edge_ids, size, tangent_chain) = self.edge_values(model)?;
        Ok(if self.kind() == SolidFormKind::Fillet {
            let fillet = SolidFilletRequest {
                body_id,
                edge_ids,
                radius: size,
                tangent_chain,
            };
            if let Some(feature_id) = self.feature {
                (
                    "solid_edit_fillet",
                    json!(EditSolidFilletRequest { feature_id, fillet }),
                )
            } else {
                ("solid_fillet", json!(fillet))
            }
        } else {
            let chamfer = SolidChamferRequest {
                body_id,
                edge_ids,
                distance: size,
                tangent_chain,
            };
            if let Some(feature_id) = self.feature {
                (
                    "solid_edit_chamfer",
                    json!(EditSolidChamferRequest {
                        feature_id,
                        chamfer
                    }),
                )
            } else {
                ("solid_chamfer", json!(chamfer))
            }
        })
    }
    pub(super) fn edge_fields(&self, model: &FormModel<'_>) -> Vec<SolidFieldView> {
        use SolidField as F;
        let fields = self.edges.as_ref().unwrap();
        let issues = self.edge_values(model).err().unwrap_or_default();
        let enabled = self.phase == Phase::Editing && self.check_model(model).is_ok();
        let selected = fields
            .body
            .and_then(|id| model.scene.bodies.iter().find(|b| b.id == id));
        let reference = selected
            .map(|b| format!("{} edge(s) · {}", fields.edges.len(), b.name))
            .unwrap_or_else(|| "Select edges".into());
        let (field, label) = if fields.kind == SolidFormKind::Fillet {
            (F::Radius, "Radius")
        } else {
            (F::Distance, "Distance")
        };
        vec![
            (F::Edges, reference, Field::None),
            (
                field,
                label.into(),
                Field::Text {
                    value: fields.size.text().into(),
                    read_only: false,
                    selection: None,
                },
            ),
            (
                F::TangentChain,
                "Tangent chain".into(),
                Field::Toggle(fields.tangent_chain),
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
