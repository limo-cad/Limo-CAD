//! Guide coverage is independent of native UI qualification.
//! Pending guides never become a working entry merely because they are listed.
use super::{Id, ALL};
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Authored,
    Pending,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryPoint {
    pub workspace: String,
    pub control: String,
    pub label_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guide {
    pub id: String,
    pub state: State,
    pub entry_points: Vec<EntryPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowArea {
    pub id: String,
    pub scope: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub schema_version: u32,
    pub inventory_complete: bool,
    pub inventory_note: String,
    pub guides: Vec<Guide>,
    pub unavailable_controls: Vec<EntryPoint>,
    pub workflow_areas: Vec<WorkflowArea>,
}

/// Loaded only when coverage is requested; this does not initialize previews,
/// renderers, assets, timers or the localized instruction catalog.
pub fn catalog() -> &'static Coverage {
    static DATA: OnceLock<Coverage> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../interface/demonstrations.json"
        )))
        .expect("embedded demonstration coverage")
    })
}

/// Only authored lesson IDs can be offered by an operation's guidance entry.
/// This iterator allocates no list and never prepares geometry.
pub fn for_control<'a>(workspace: &'a str, control: &'a str) -> impl Iterator<Item = Id> + 'a {
    catalog()
        .guides
        .iter()
        .filter(move |guide| {
            guide.state == State::Authored
                && guide
                    .entry_points
                    .iter()
                    .any(|entry| entry.workspace == workspace && entry.control == control)
        })
        .filter_map(|guide| Id::from_key(&guide.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::{BTreeMap, BTreeSet};

    fn controls(
        nodes: &[Value],
        workspace: &str,
        result: &mut BTreeMap<(String, String), (bool, String)>,
    ) {
        for node in nodes {
            if let Some(children) = node["children"].as_array() {
                controls(children, workspace, result);
                continue;
            }
            let (Some(id), Some(label)) = (node["id"].as_str(), node["labelKey"].as_str()) else {
                continue;
            };
            let enabled = node["enabled"].as_bool().unwrap_or(true);
            result
                .entry((workspace.into(), id.into()))
                .and_modify(|value| value.0 |= enabled)
                .or_insert((enabled, label.into()));
        }
    }

    #[test]
    fn guide_inventory_covers_every_catalog_control_without_claiming_pending_work() {
        let product: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../interface/catalog.json"
        )))
        .unwrap();
        let mut expected = BTreeMap::new();
        for workspace in product["workspaces"].as_array().unwrap() {
            let name = workspace["id"].as_str().unwrap();
            for panel in workspace["panels"].as_array().unwrap() {
                for key in ["buttons", "menu"] {
                    if let Some(nodes) = panel[key].as_array() {
                        controls(nodes, name, &mut expected);
                    }
                }
            }
        }
        let coverage = catalog();
        assert_eq!(coverage.schema_version, 1);
        assert!(!coverage.inventory_note.trim().is_empty());
        let mut ids = BTreeSet::new();
        let mut authored = BTreeSet::new();
        let mut covered = BTreeSet::new();
        for guide in &coverage.guides {
            assert!(ids.insert(&guide.id), "Duplicate guide {}", guide.id);
            assert!(!guide.entry_points.is_empty());
            assert!(guide
                .id
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'));
            if guide.state == State::Authored {
                authored.insert(guide.id.as_str());
            }
            let mut local = BTreeSet::new();
            for entry in &guide.entry_points {
                let key = (entry.workspace.clone(), entry.control.clone());
                assert!(local.insert(key.clone()), "Duplicate entry on {}", guide.id);
                let (enabled, label) = expected
                    .get(&key)
                    .expect("Guide entry must be a real catalog control");
                assert!(*enabled, "Disabled operation cannot offer a guide: {key:?}");
                assert_eq!(label, &entry.label_key);
                covered.insert(key);
            }
        }
        let lesson_ids: BTreeSet<_> = ALL.into_iter().map(Id::key).collect();
        assert_eq!(
            authored, lesson_ids,
            "Authored coverage must agree with actual bundled lessons"
        );
        let enabled: BTreeSet<_> = expected
            .iter()
            .filter(|(_, v)| v.0)
            .map(|(k, _)| k.clone())
            .collect();
        assert_eq!(
            covered, enabled,
            "A new enabled control needs an explicit guide coverage entry"
        );
        let unavailable: BTreeSet<_> = coverage
            .unavailable_controls
            .iter()
            .map(|e| {
                let key = (e.workspace.clone(), e.control.clone());
                let (enabled, label) = expected.get(&key).expect("Unavailable entry must exist");
                assert!(!enabled);
                assert_eq!(label, &e.label_key);
                key
            })
            .collect();
        let disabled: BTreeSet<_> = expected
            .iter()
            .filter(|(_, v)| !v.0)
            .map(|(k, _)| k.clone())
            .collect();
        assert_eq!(unavailable, disabled);
        if coverage
            .workflow_areas
            .iter()
            .any(|area| area.state == "pending_entry_audit")
        {
            assert!(
                !coverage.inventory_complete,
                "Unreconciled workflow areas keep the inventory incomplete"
            );
        }
    }

    #[test]
    fn pending_guides_are_not_returned_as_available_demonstrations() {
        assert_eq!(
            for_control("solid", "extrude").collect::<Vec<_>>(),
            [Id::Extrude, Id::Cut]
        );
        assert_eq!(
            for_control("solid", "createSketch").collect::<Vec<_>>(),
            [Id::Sketch]
        );
        for guide in &catalog().guides {
            if guide.state == State::Pending {
                assert!(!ALL.into_iter().any(|id| id.key() == guide.id));
            }
        }
        assert_eq!(for_control("missing", "extrude").count(), 0);
    }
}
