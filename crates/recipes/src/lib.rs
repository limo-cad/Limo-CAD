//! One collection of authored construction sources, not another interpreter.
//! The app, MCP and xtask all discover these recipes through this catalog.
use serde_json::{json, Value};

pub struct Recipe {
    pub id: &'static str,
    pub source: &'static str,
    pub summary: &'static str,
    pub kind: &'static str,
    pub focus_operations: &'static [&'static str],
    pub preview: bool,
}

pub const RECIPES: &[Recipe] = &[
    Recipe {
        id: "d-screw-vise",
        source: include_str!("../../../examples/scripts/d-screw-vise.nbcad.jsonc"),
        summary: "Build a five-part vise with a real interrupted helical screw, circular wear nut and guided retained jaw. Physical print-fit qualification remains required.",
        kind: "flagship-candidate",
        focus_operations: &["solid_external_thread", "assembly_create_joint"],
        preview: false,
    },
    Recipe {
        id: "d-screw-vise-fit",
        source: include_str!("../../../examples/scripts/d-screw-vise-fit.nbcad.jsonc"),
        summary: "Print a true half-section M20 screw and explicitly relieved custom female thread in their intended FDM orientations.",
        kind: "manufacturing-coupon",
        focus_operations: &["solid_external_thread", "solid_hole"],
        preview: false,
    },
    Recipe {
        id: "fillet-basics",
        source: include_str!("../../../examples/scripts/fillet-basics.nbcad.jsonc"),
        summary: "Locate a dimensioned sketch, extrude stock, then round only its top rim.",
        kind: "lesson",
        focus_operations: &["solid_extrude", "solid_fillet"],
        preview: true,
    },
    Recipe {
        id: "mounting-plate",
        source: include_str!("../../../examples/scripts/mounting-plate.nbcad.jsonc"),
        summary: "Drill four through holes from the current top-face basis of a fully located plate.",
        kind: "lesson",
        focus_operations: &["solid_hole"],
        preview: false,
    },
    Recipe {
        id: "revolved-spacer",
        source: include_str!("../../../examples/scripts/revolved-spacer.nbcad.jsonc"),
        summary: "Revolve a located radial section into an annular spacer with an editable bore.",
        kind: "lesson",
        focus_operations: &["solid_revolve"],
        preview: false,
    },
    Recipe {
        id: "angle-bracket",
        source: include_str!("../../../examples/scripts/angle-bracket.nbcad.jsonc"),
        summary: "Constrain a six-edge L section and extrude a dimensioned angle bracket.",
        kind: "lesson",
        focus_operations: &["sketch_add_dimension", "solid_extrude"],
        preview: false,
    },
    Recipe {
        id: "repeated-bracket-assembly",
        source: include_str!("../../../examples/scripts/repeated-bracket-assembly.nbcad.jsonc"),
        summary: "Assemble three native parts as four occurrences, then edit the shared bracket definition.",
        kind: "assembly",
        focus_operations: &["assembly_create_occurrence", "assembly_create_joint", "solid_edit_extrude"],
        preview: false,
    },
    Recipe {
        id: "garden-bench",
        source: include_str!("../../../examples/scripts/garden-bench.nbcad.jsonc"),
        summary: "Build the timber bench, connected assembly and geometric manufacturing checks. Design candidate; full drafting remains open.",
        kind: "flagship-candidate",
        focus_operations: &["assembly_create_joint", "construction_plane_midplane"],
        preview: false,
    },
];

pub fn find(id: &str) -> Result<&'static Recipe, String> {
    RECIPES
        .iter()
        .find(|recipe| recipe.id == id)
        .ok_or_else(|| format!("Unknown recipe '{id}'; list the recipe catalog first"))
}

/// Titles, chapters, actual calls and counts come from the source itself.
/// A recipe has no pretend `cad_script` operation: that operation records traces.
pub fn catalog(include_source: bool) -> Value {
    Value::Array(
        RECIPES
            .iter()
            .map(|recipe| {
                let mut entry = nbcad_script::Script::parse(recipe.source)
                    .expect("bundled recipe must pass preflight")
                    .metadata();
                entry["id"] = json!(recipe.id);
                entry["summary"] = json!(recipe.summary);
                entry["kind"] = json!(recipe.kind);
                entry["focus_operations"] = json!(recipe.focus_operations);
                entry["preview"] = json!(recipe.preview);
                entry["run"] = json!({"action":"script", "recipe":recipe.id});
                if include_source {
                    entry["source"] = json!(recipe.source);
                }
                entry
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn recipes_have_distinct_ids_and_teach_operations_they_execute() {
        let entries = catalog(false);
        let mut ids = BTreeSet::new();
        for entry in entries.as_array().unwrap() {
            assert!(ids.insert(entry["id"].as_str().unwrap()));
            assert!(!entry["chapters"].as_array().unwrap().is_empty());
            for operation in entry["focus_operations"].as_array().unwrap() {
                assert!(
                    entry["operations"].as_array().unwrap().contains(operation),
                    "{} claims an operation it does not execute: {operation}",
                    entry["id"]
                );
            }
            if entry["preview"] == true {
                assert!(
                    entry["step_count"].as_u64().unwrap() + entry["check_count"].as_u64().unwrap()
                        <= 80
                );
            }
        }
    }
}
