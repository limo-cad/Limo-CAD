//! Bounded object identities for edits blocked by retained drafting content.
use super::*;

fn identities(label: &str, ids: impl Iterator<Item = u64>) -> Option<String> {
    let mut values = Vec::new();
    let mut count = 0;
    for id in ids {
        count += 1;
        if values.len() < 8 {
            values.push(id.to_string());
        }
    }
    if count == 0 {
        return None;
    }
    let remainder = if count > values.len() {
        format!(", plus {} more", count - values.len())
    } else {
        String::new()
    };
    Some(format!("{label} [{}]{remainder}", values.join(", ")))
}

impl Document {
    pub(crate) fn sheet_dependents(&self, sheet_id: u64) -> Option<String> {
        let entities = identities(
            "drafting entities",
            self.entities
                .iter()
                .filter(|entity| entity.space == Space::Paper(sheet_id))
                .map(|entity| entity.id.0),
        );
        let viewports = identities(
            "viewports",
            self.viewports
                .iter()
                .filter(|viewport| viewport.sheet_id == sheet_id)
                .map(|viewport| viewport.id.0),
        );
        match (entities, viewports) {
            (Some(entities), Some(viewports)) => Some(format!("{entities} and {viewports}")),
            (entities, viewports) => entities.or(viewports),
        }
    }

    pub(crate) fn layer_dependents(&self, layer_id: LayerId) -> Option<String> {
        let entities = identities(
            "drafting entities",
            self.entities
                .iter()
                .filter(|entity| entity.layer == layer_id)
                .map(|entity| entity.id.0),
        );
        let viewports = identities(
            "viewport hidden-layer lists",
            self.viewports
                .iter()
                .filter(|viewport| viewport.hidden_layers.contains(&layer_id))
                .map(|viewport| viewport.id.0),
        );
        match (entities, viewports) {
            (Some(entities), Some(viewports)) => Some(format!("{entities} and {viewports}")),
            (entities, viewports) => entities.or(viewports),
        }
    }
}
