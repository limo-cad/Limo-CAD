use crate::browser::{BrowserNode, BrowserNodeKind, NodeId};
use crate::feature::{Feature, FeatureId, FeatureKind, FeatureStatus, FeatureTree};
use crate::units::DocumentSettings;

/// In-memory CAD document: settings, browser tree, and feature history.
#[derive(Debug, Clone)]
pub struct Document {
    name: String,
    settings: DocumentSettings,
    /// Top-level nodes of the browser tree. The tree root itself is the
    /// document (rendered from `name` by the UI) and is not a node.
    browser: Vec<BrowserNode>,
    features: FeatureTree,
    /// Next browser node id. `Document::new` assigns ids 1..=10; nodes added
    /// later (sketches, bodies) continue from here, keeping ids stable and
    /// unique for the life of the document.
    next_node_id: u64,
    next_feature_id: u64,
}

impl Document {
    /// Create a document with the standard browser tree:
    /// Document Settings, Named Views, Origin (XY/XZ/YZ plane + center
    /// point), Bodies, Sketches, Construction. Node ids are deterministic
    /// (1..=10 in creation order), shared by UI and MCP snapshots.
    pub fn new(name: impl Into<String>) -> Self {
        let mut next = 1u64;
        let mut id = || {
            let id = NodeId(next);
            next += 1;
            id
        };

        let browser = vec![
            BrowserNode::new(id(), BrowserNodeKind::DocumentSettings),
            BrowserNode::new(id(), BrowserNodeKind::NamedViews),
            BrowserNode::new(id(), BrowserNodeKind::Origin).with_children(vec![
                BrowserNode::new(id(), BrowserNodeKind::OriginPlaneXy),
                BrowserNode::new(id(), BrowserNodeKind::OriginPlaneXz),
                BrowserNode::new(id(), BrowserNodeKind::OriginPlaneYz),
                BrowserNode::new(id(), BrowserNodeKind::OriginCenterPoint),
            ]),
            BrowserNode::new(id(), BrowserNodeKind::BodiesFolder),
            BrowserNode::new(id(), BrowserNodeKind::SketchesFolder),
            BrowserNode::new(id(), BrowserNodeKind::ConstructionFolder),
        ];

        Self {
            name: name.into(),
            settings: DocumentSettings::default(),
            browser,
            features: FeatureTree::default(),
            next_node_id: next,
            next_feature_id: 1,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub fn settings(&self) -> &DocumentSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut DocumentSettings {
        &mut self.settings
    }

    pub fn browser(&self) -> &[BrowserNode] {
        &self.browser
    }

    pub fn features(&self) -> &FeatureTree {
        &self.features
    }

    pub fn features_mut(&mut self) -> &mut FeatureTree {
        &mut self.features
    }

    pub fn add_feature(&mut self, name: impl Into<String>, kind: FeatureKind) -> FeatureId {
        let id = self.alloc_feature_id();
        self.push_feature(Feature::new(id, name, kind));
        id
    }

    pub fn alloc_feature_id(&mut self) -> FeatureId {
        let id = FeatureId(self.next_feature_id);
        self.next_feature_id += 1;
        id
    }

    pub fn push_feature(&mut self, feature: Feature) {
        self.features.push(feature);
    }

    /// Restore the persistent, browser-independent portion of a document.
    ///
    /// Browser rows are deliberately rebuilt by the owning manager from the
    /// restored sketches/bodies. This keeps `.limo` files independent from
    /// transient UI node ids while preserving stable feature ids.
    pub fn restore_history(&mut self, settings: DocumentSettings, features: FeatureTree) {
        self.settings = settings;
        self.next_feature_id = features
            .features
            .iter()
            .map(|feature| feature.id.0)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        self.features = features;
    }

    pub fn set_feature_status(&mut self, id: FeatureId, status: FeatureStatus) -> bool {
        let Some(feature) = self.features.feature_mut(id) else {
            return false;
        };
        feature.status = status;
        true
    }

    /// Allocate a fresh browser node id (monotonic for the document's life).
    pub fn alloc_node_id(&mut self) -> NodeId {
        let id = NodeId(self.next_node_id);
        self.next_node_id += 1;
        id
    }

    /// Append a child node to the first node of `parent_kind` (depth-first).
    /// Returns the new node's id, or `None` if no such parent exists.
    pub fn add_browser_child(
        &mut self,
        parent_kind: BrowserNodeKind,
        kind: BrowserNodeKind,
        name: impl Into<String>,
    ) -> Option<NodeId> {
        fn find_mut(nodes: &mut [BrowserNode], kind: BrowserNodeKind) -> Option<&mut BrowserNode> {
            for node in nodes.iter_mut() {
                if node.kind == kind {
                    return Some(node);
                }
                if let Some(found) = find_mut(&mut node.children, kind) {
                    return Some(found);
                }
            }
            None
        }

        let id = self.alloc_node_id();
        let parent = find_mut(&mut self.browser, parent_kind)?;
        parent
            .children
            .push(BrowserNode::new(id, kind).named(name.into()));
        Some(id)
    }

    /// Add a body row linked to its stable model id.
    pub fn add_body_node(&mut self, body_id: u64, name: impl Into<String>) -> Option<NodeId> {
        fn find_mut(nodes: &mut [BrowserNode]) -> Option<&mut BrowserNode> {
            for node in nodes {
                if node.kind == BrowserNodeKind::BodiesFolder {
                    return Some(node);
                }
                if let Some(found) = find_mut(&mut node.children) {
                    return Some(found);
                }
            }
            None
        }

        let id = self.alloc_node_id();
        find_mut(&mut self.browser)?.children.push(
            BrowserNode::new(id, BrowserNodeKind::Body)
                .named(name)
                .referencing(body_id),
        );
        Some(id)
    }

    /// Synchronize saved named views while retaining unchanged browser identities.
    pub fn set_named_view_children(&mut self, names: &[String]) {
        fn find_folder(nodes: &mut [BrowserNode]) -> Option<&mut BrowserNode> {
            for node in nodes {
                if node.kind == BrowserNodeKind::NamedViews {
                    return Some(node);
                }
                if let Some(found) = find_folder(&mut node.children) {
                    return Some(found);
                }
            }
            None
        }

        let mut existing = {
            let Some(folder) = find_folder(&mut self.browser) else {
                return;
            };
            std::mem::take(&mut folder.children)
        };
        let mut next = Vec::with_capacity(names.len());
        for name in names {
            if let Some(index) = existing.iter().position(|child| {
                child.kind == BrowserNodeKind::NamedView
                    && child.name.as_deref() == Some(name.as_str())
            }) {
                next.push(existing.remove(index));
            } else {
                let id = self.alloc_node_id();
                next.push(BrowserNode::new(id, BrowserNodeKind::NamedView).named(name.clone()));
            }
        }
        if let Some(folder) = find_folder(&mut self.browser) {
            folder.children = next;
        }
    }

    /// Add a construction-plane row linked to its stable datum id.
    pub fn add_construction_plane_node(
        &mut self,
        datum_id: u64,
        name: impl Into<String>,
    ) -> Option<NodeId> {
        self.add_browser_child(
            BrowserNodeKind::ConstructionFolder,
            BrowserNodeKind::ConstructionPlane,
            name,
        )
        .inspect(|id| {
            fn set_reference(nodes: &mut [BrowserNode], id: NodeId, datum_id: u64) -> bool {
                for node in nodes {
                    if node.id == id {
                        node.reference_id = Some(datum_id);
                        return true;
                    }
                    if set_reference(&mut node.children, id, datum_id) {
                        return true;
                    }
                }
                false
            }
            set_reference(&mut self.browser, *id, datum_id);
        })
    }

    pub fn body_node_id(&self, body_id: u64) -> Option<NodeId> {
        fn find(nodes: &[BrowserNode], body_id: u64) -> Option<NodeId> {
            for node in nodes {
                if node.kind == BrowserNodeKind::Body && node.reference_id == Some(body_id) {
                    return Some(node.id);
                }
                if let Some(found) = find(&node.children, body_id) {
                    return Some(found);
                }
            }
            None
        }
        find(&self.browser, body_id)
    }

    /// Keep browser body rows synchronized with the current recomputed scene.
    pub fn retain_body_nodes(&mut self, keep: impl Fn(u64) -> bool) {
        fn retain(nodes: &mut Vec<BrowserNode>, keep: &impl Fn(u64) -> bool) {
            for node in nodes.iter_mut() {
                retain(&mut node.children, keep);
            }
            nodes.retain(|node| {
                node.kind != BrowserNodeKind::Body || node.reference_id.is_some_and(keep)
            });
        }
        retain(&mut self.browser, &keep);
    }

    /// Remove the browser row owned by a deleted sketch feature.
    pub fn remove_sketch_node(&mut self, name: &str) -> bool {
        remove_browser_nodes(&mut self.browser, BrowserNodeKind::Sketch, Some(name), None) > 0
    }

    /// Rename the existing sketch row without replacing its browser identity.
    pub fn rename_sketch_node(&mut self, old: &str, new: &str) {
        fn rename(nodes: &mut [BrowserNode], old: &str, new: &str) {
            for node in nodes {
                if node.kind == BrowserNodeKind::Sketch && node.name.as_deref() == Some(old) {
                    node.name = Some(new.to_owned());
                }
                rename(&mut node.children, old, new);
            }
        }
        rename(&mut self.browser, old, new);
    }

    /// Remove the browser row owned by a deleted construction plane.
    pub fn remove_construction_plane_node(&mut self, datum_id: u64) -> bool {
        remove_browser_nodes(
            &mut self.browser,
            BrowserNodeKind::ConstructionPlane,
            None,
            Some(datum_id),
        ) > 0
    }
}

fn remove_browser_nodes(
    nodes: &mut Vec<BrowserNode>,
    kind: BrowserNodeKind,
    name: Option<&str>,
    reference_id: Option<u64>,
) -> usize {
    let mut removed = 0;
    for node in nodes.iter_mut() {
        removed += remove_browser_nodes(&mut node.children, kind, name, reference_id);
    }
    nodes.retain(|node| {
        let name_matches = match name {
            Some(expected) => node.name.as_deref() == Some(expected),
            None => true,
        };
        let reference_matches = match reference_id {
            Some(expected) => node.reference_id == Some(expected),
            None => true,
        };
        let matches = node.kind == kind && name_matches && reference_matches;
        if matches {
            removed += 1;
        }
        !matches
    });
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserNodeKind as K;

    #[test]
    fn default_document_has_mm_units() {
        let doc = Document::new("Untitled");
        assert_eq!(doc.settings().units, crate::UnitSystem::Mm);
    }

    #[test]
    fn new_document_has_standard_browser_tree() {
        let doc = Document::new("Untitled");
        let kinds: Vec<K> = doc.browser().iter().map(|n| n.kind).collect();
        assert_eq!(
            kinds,
            vec![
                K::DocumentSettings,
                K::NamedViews,
                K::Origin,
                K::BodiesFolder,
                K::SketchesFolder,
                K::ConstructionFolder,
            ]
        );
    }

    #[test]
    fn origin_node_has_planes_and_center_point() {
        let doc = Document::new("Untitled");
        let origin = doc
            .browser()
            .iter()
            .find(|n| n.kind == K::Origin)
            .expect("origin node");
        let kinds: Vec<K> = origin.children.iter().map(|n| n.kind).collect();
        assert_eq!(
            kinds,
            vec![
                K::OriginPlaneXy,
                K::OriginPlaneXz,
                K::OriginPlaneYz,
                K::OriginCenterPoint,
            ]
        );
    }

    #[test]
    fn node_ids_are_unique_and_deterministic() {
        let doc = Document::new("Untitled");
        fn collect(nodes: &[BrowserNode], out: &mut Vec<u64>) {
            for n in nodes {
                out.push(n.id.0);
                collect(&n.children, out);
            }
        }
        let mut ids = Vec::new();
        collect(doc.browser(), &mut ids);
        assert_eq!(ids, (1u64..=10).collect::<Vec<_>>());

        let doc2 = Document::new("Other");
        let mut ids2 = Vec::new();
        collect(doc2.browser(), &mut ids2);
        assert_eq!(ids, ids2);
    }

    #[test]
    fn new_document_has_empty_feature_tree() {
        let doc = Document::new("Untitled");
        assert!(doc.features().is_empty());
    }

    #[test]
    fn sketch_nodes_register_under_the_sketches_folder() {
        let mut doc = Document::new("Untitled");
        let id = doc
            .add_browser_child(K::SketchesFolder, K::Sketch, "Sketch1")
            .expect("sketches folder exists");
        assert_eq!(id, NodeId(11));

        let sketches = doc
            .browser()
            .iter()
            .find(|n| n.kind == K::SketchesFolder)
            .expect("sketches folder");
        assert_eq!(sketches.children.len(), 1);
        assert_eq!(sketches.children[0].kind, K::Sketch);
        assert_eq!(sketches.children[0].name.as_deref(), Some("Sketch1"));

        let id2 = doc
            .add_browser_child(K::SketchesFolder, K::Sketch, "Sketch2")
            .unwrap();
        assert_eq!(id2, NodeId(12));
    }

    #[test]
    fn features_and_body_rows_get_stable_monotonic_ids() {
        let mut doc = Document::new("Untitled");
        let feature = doc.add_feature("Sketch1", FeatureKind::Sketch);
        assert_eq!(feature, FeatureId(1));
        let body_node = doc.add_body_node(42, "Body1").unwrap();
        assert_eq!(body_node, NodeId(11));
        assert_eq!(doc.body_node_id(42), Some(body_node));
        doc.retain_body_nodes(|id| id != 42);
        assert_eq!(doc.body_node_id(42), None);
    }

    #[test]
    fn feature_owned_browser_rows_can_be_removed() {
        let mut doc = Document::new("Untitled");
        doc.add_browser_child(K::SketchesFolder, K::Sketch, "Sketch1");
        doc.add_browser_child(K::SketchesFolder, K::Sketch, "Sketch2");
        doc.add_construction_plane_node(17, "Plane1");

        assert!(doc.remove_sketch_node("Sketch1"));
        assert!(!doc.remove_sketch_node("Missing"));
        let sketches = doc
            .browser()
            .iter()
            .find(|node| node.kind == K::SketchesFolder)
            .expect("sketches folder");
        assert_eq!(
            sketches
                .children
                .iter()
                .filter_map(|node| node.name.as_deref())
                .collect::<Vec<_>>(),
            vec!["Sketch2"]
        );

        assert!(doc.remove_construction_plane_node(17));
        assert!(!doc.remove_construction_plane_node(17));
    }
}
