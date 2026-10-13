use serde::{Deserialize, Serialize};

use crate::browser::BrowserNode;
use crate::feature::Feature;
use crate::units::DocumentSettings;
use crate::Document;

/// Serializable snapshot of a [`Document`] shared by native UI and MCP hosts.
///
/// This is the shared `get_document` engine contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentDto {
    pub name: String,
    pub settings: DocumentSettings,
    pub browser: Vec<BrowserNode>,
    pub features: Vec<Feature>,
    pub rollback_index: usize,
}

impl From<&Document> for DocumentDto {
    fn from(doc: &Document) -> Self {
        Self {
            name: doc.name().to_string(),
            settings: doc.settings().clone(),
            browser: doc.browser().to_vec(),
            features: doc.features().features.clone(),
            rollback_index: doc.features().rollback_index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dto_mirrors_document() {
        let doc = Document::new("Untitled");
        let dto = DocumentDto::from(&doc);
        assert_eq!(dto.name, "Untitled");
        assert_eq!(dto.settings, *doc.settings());
        assert_eq!(dto.browser.len(), doc.browser().len());
        assert_eq!(dto.features, doc.features().features);
    }

    #[test]
    fn dto_serializes_to_expected_shape() {
        let doc = Document::new("Untitled");
        let dto = DocumentDto::from(&doc);
        let v = serde_json::to_value(&dto).unwrap();
        assert_eq!(v["name"], "Untitled");
        assert_eq!(v["settings"]["units"], "mm");
        assert_eq!(v["browser"][0]["kind"], "document_settings");
        assert_eq!(v["browser"][2]["kind"], "origin");
        assert_eq!(v["browser"][2]["children"][0]["kind"], "origin_plane_xy");
        assert_eq!(v["rollback_index"], 0);
    }

    #[test]
    fn dto_roundtrips_through_json() {
        let doc = Document::new("Untitled");
        let dto = DocumentDto::from(&doc);
        let json = serde_json::to_string(&dto).unwrap();
        let back: DocumentDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, dto);
    }
}
