use crate::{bambu_project::BambuProjectRequest, MeshExportRequest};
use serde::{Deserialize, Serialize};

/// Explicit target-project mode. Portable mesh export never implicitly reads a profile/template.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BambuExportRequest {
    pub export: MeshExportRequest,
    pub project: BambuProjectRequest,
    /// A complete saved unsliced project, supplied by the caller without overwriting its source.
    pub template_base64: String,
}

impl BambuExportRequest {
    pub fn validate(&self, current_model_json: &str) -> Result<(), crate::ExportError> {
        if self.export.scope != crate::MeshExportScope::Assembly {
            return Err(crate::ExportError("Bambu project export requires resolved assembly/layout scope; choose portable export for source definitions.".into()));
        }
        if self
            .export
            .expected_model_json
            .as_deref()
            .is_none_or(str::is_empty)
        {
            return Err(crate::ExportError("Bambu project export requires expected_model_json from the reviewed completed document.".into()));
        }
        self.export.check_model_snapshot(current_model_json)?;
        let model: serde_json::Value = serde_json::from_str(current_model_json)
            .map_err(|error| crate::ExportError(error.to_string()))?;
        let intent: limo_cad_core::PrintIntentDocumentDto = serde_json::from_value(
            model
                .get("print_intent")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        )
        .map_err(|error| crate::ExportError(error.to_string()))?;
        for layout in intent
            .height_ranges
            .iter()
            .filter(|range| range.enabled)
            .map(|range| &range.binding.layout)
            .chain(
                intent
                    .layer_height_profiles
                    .iter()
                    .filter(|profile| profile.enabled)
                    .map(|profile| &profile.binding.layout),
            )
        {
            if let limo_cad_core::PrintHeightLayoutDto::NamedLayout { id } = layout {
                let view = model.get("views").and_then(serde_json::Value::as_array)
                    .and_then(|views| views.iter().find(|view| view.get("id").and_then(serde_json::Value::as_str) == Some(id.as_str())))
                    .ok_or_else(|| crate::ExportError("The persistent height layout identity is absent from the owning document; explicitly rebind before export".into()))?;
                if let Some(name) = self.export.named_view.as_deref() {
                    if view.get("name").and_then(serde_json::Value::as_str) != Some(name) {
                        return Err(crate::ExportError("Selected saved/assembled export differs from the persistent height layout binding".into()));
                    }
                }
            } else if self
                .export
                .named_view
                .as_deref()
                .is_some_and(|name| !name.is_empty())
            {
                return Err(crate::ExportError("Assembled height intent cannot be applied through another saved layout; explicitly rebind".into()));
            }
        }
        if self.template_base64.len() > 180 * 1024 * 1024 {
            return Err(crate::ExportError(
                "Saved Bambu template exceeds the 128 MiB input limit.".into(),
            ));
        }
        if !self.export.include_appearance {
            return Err(crate::ExportError("Bambu projects require an explicit material/color mapping; include_appearance cannot be disabled.".into()));
        }
        if self.project.placement == crate::bambu_project::BambuPlacementMode::Template
            && self
                .export
                .named_view
                .as_deref()
                .is_some_and(|name| !name.is_empty())
        {
            return Err(crate::ExportError("Template placement preserves its saved plates and orientation. Clear named_view to choose it explicitly, or use resolved_scene with a compatible template to export the selected CAD layout.".into()));
        }
        Ok(())
    }
}
