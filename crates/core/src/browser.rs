use serde::{Deserialize, Serialize};

/// Stable identifier of a browser tree node within a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub u64);

/// Kind of a browser tree node.
///
/// The UI maps kinds to localized labels via i18n keys, so this enum
/// is part of the IPC contract. Unit variants serialize as plain snake_case
/// strings (e.g. `"origin_plane_xy"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserNodeKind {
    DocumentSettings,
    NamedViews,
    NamedView,
    Origin,
    OriginPlaneXy,
    OriginPlaneXz,
    OriginPlaneYz,
    OriginCenterPoint,
    BodiesFolder,
    Body,
    SketchesFolder,
    Sketch,
    ConstructionFolder,
    ConstructionPlane,
}

/// One node of the browser tree shown in the left panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserNode {
    pub id: NodeId,
    pub kind: BrowserNodeKind,
    /// Display name for user-named nodes (e.g. "Sketch1"). `None` means the
    /// UI resolves a localized label from `kind`.
    pub name: Option<String>,
    /// Stable model object id for object rows (Body today, extensible later).
    #[serde(default)]
    pub reference_id: Option<u64>,
    /// Visibility toggle state (eye icon in the UI).
    pub visible: bool,
    pub children: Vec<BrowserNode>,
}

impl BrowserNode {
    pub fn new(id: NodeId, kind: BrowserNodeKind) -> Self {
        Self {
            id,
            kind,
            name: None,
            reference_id: None,
            visible: true,
            children: Vec::new(),
        }
    }

    pub fn with_children(mut self, children: Vec<BrowserNode>) -> Self {
        self.children = children;
        self
    }

    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn referencing(mut self, id: u64) -> Self {
        self.reference_id = Some(id);
        self
    }
}
