//! One collection of authored construction sources, not another interpreter.
//! The app, MCP and xtask all discover these recipes through this catalog.
use serde_json::{json, Value};

pub mod authoring;

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
        source: include_str!("../../../examples/scripts/d-screw-vise.limo.jsonc"),
        summary: "Build a captured-slide vise with 100 mm jaws, 90 mm travel and a custom rounded 24 x 4 screw with a shallow print flat. Six printed parts and M5/M6 hardware envelopes make 30 bodies including optional mounts, with seven drawing sheets and per-part print layouts. Physical qualification remains required.",
        kind: "flagship-candidate",
        focus_operations: &["solid_external_thread", "assembly_create_joint"],
        preview: false,
    },
    Recipe {
        id: "d-screw-vise-fit",
        source: include_str!("../../../examples/scripts/d-screw-vise-fit.limo.jsonc"),
        summary: "Build four fit specimens: a shallow-flat custom rounded 24 x 4 screw, matching relieved female thread, and male/female captured guides. Qualify the actual print process before making the full vise.",
        kind: "manufacturing-coupon",
        focus_operations: &["solid_external_thread", "solid_hole"],
        preview: false,
    },
    Recipe {
        id: "fillet-basics",
        source: include_str!("../../../examples/scripts/fillet-basics.limo.jsonc"),
        summary: "Locate a dimensioned sketch, extrude stock, then round only its top rim.",
        kind: "lesson",
        focus_operations: &["solid_extrude", "solid_fillet"],
        preview: true,
    },
    Recipe {
        id: "mounting-plate",
        source: include_str!("../../../examples/scripts/mounting-plate.limo.jsonc"),
        summary: "Drill four through holes from the current top-face basis of a fully located plate.",
        kind: "lesson",
        focus_operations: &["solid_hole"],
        preview: false,
    },
    Recipe {
        id: "component-edit-recovery",
        source: include_str!("../../../examples/scripts/component-edit-recovery.limo.jsonc"),
        summary: "Edit a rotated shared occurrence, undo a wrong driving dimension and recompute both placements from the local definition.",
        kind: "lesson",
        focus_operations: &["sketch_edit", "sketch_edit_dimension", "sketch_undo"],
        preview: true,
    },
    Recipe {
        id: "revolved-spacer",
        source: include_str!("../../../examples/scripts/revolved-spacer.limo.jsonc"),
        summary: "Revolve a located radial section into an annular spacer with an editable bore.",
        kind: "lesson",
        focus_operations: &["solid_revolve"],
        preview: false,
    },
    Recipe {
        id: "angle-bracket",
        source: include_str!("../../../examples/scripts/angle-bracket.limo.jsonc"),
        summary: "Constrain a six-edge L section and extrude a dimensioned angle bracket.",
        kind: "lesson",
        focus_operations: &["sketch_add_dimension", "solid_extrude"],
        preview: false,
    },
    Recipe {
        id: "repeated-bracket-assembly",
        source: include_str!("../../../examples/scripts/repeated-bracket-assembly.limo.jsonc"),
        summary: "Assemble three native parts as four occurrences, then edit the shared bracket definition.",
        kind: "assembly",
        focus_operations: &["assembly_create_occurrence", "assembly_create_joint", "solid_edit_extrude"],
        preview: false,
    },
    Recipe {
        id: "garden-bench",
        source: include_str!("../../../examples/scripts/garden-bench.limo.jsonc"),
        summary: "Build the timber bench, connected assembly, geometric manufacturing checks and 22 editable review sheets with SVG/DXF exports. Manufacturing and physical qualification remain required.",
        kind: "flagship-candidate",
        focus_operations: &["assembly_create_joint", "construction_plane_midplane"],
        preview: false,
    },
    Recipe {
        id: "turbine-fit-coupons",
        source: include_str!("../../../examples/scripts/turbine-fit-coupons.limo.jsonc"),
        summary: "Print dimensioned shaft, bearing, motor-case and motor-shaft fit specimens with the actual turbine clamp geometry before committing the full rotor.",
        kind: "calibration",
        focus_operations: &["sketch_add_circle_locked", "solid_extrude", "drawing_add_radial_dimension"],
        preview: false,
    },
    Recipe {
        id: "vertical-axis-turbine",
        source: include_str!("../../../examples/scripts/vertical-axis-turbine.limo.jsonc"),
        summary: "Build a two-stage printable Savonius turbine, constrained 4:1 spur drive and associative manufacturing drawings. Physical print and generator fit qualification pending.",
        kind: "flagship-candidate",
        focus_operations: &["solid_circular_pattern", "assembly_create_gear_relation", "drawing_add_radial_dimension"],
        preview: false,
    },
];

pub fn find(id: &str) -> Result<&'static Recipe, String> {
    RECIPES
        .iter()
        .find(|recipe| recipe.id == id)
        .ok_or_else(|| format!("Unknown recipe '{id}'; list the recipe catalog first"))
}

/// Links select installed, authored source only. No URL decoding, paths,
/// parameters, remote fetches or implicit execution cross this boundary.
pub fn from_open_uri(uri: &str) -> Result<&'static Recipe, String> {
    let id = uri
        .strip_prefix("limo-cad://recipe/")
        .or_else(|| uri.strip_prefix("nbcad://recipe/"))
        .ok_or("Expected limo-cad://recipe/<built-in recipe ID>")?;
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(
            "Recipe links accept a built-in ID only, without parameters or extra paths".into(),
        );
    }
    find(id)
}

/// Titles, chapters, actual calls and counts come from the source itself.
/// A recipe has no pretend `cad_script` operation: that operation records traces.
pub fn catalog(include_source: bool) -> Value {
    Value::Array(
        RECIPES
            .iter()
            .map(|recipe| {
                let mut entry = limo_cad_script::Script::parse(recipe.source)
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
    fn flagship_creations_have_explicit_history_names() {
        for id in ["garden-bench", "d-screw-vise", "vertical-axis-turbine"] {
            let source = find(id).unwrap().source;
            limo_cad_script::Script::parse(source).unwrap();
            let source = source
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            let document: Value = serde_json::from_str(&source).unwrap();
            let steps = document["steps"].as_array().unwrap();
            let mut names = 0;
            for (index, step) in steps.iter().enumerate() {
                let Some(operation) = step["call"]["operation"].as_str() else {
                    continue;
                };
                let Some(mut naming) = authoring::feature_name_step(
                    step["id"].as_str().unwrap(),
                    operation,
                    &step["call"]["arguments"],
                ) else {
                    continue;
                };
                // Authors may replace the generated name with a more descriptive one;
                // the rename step itself must still follow and target this feature.
                let mut actual = steps.get(index + 1).cloned().unwrap_or(Value::Null);
                let name = actual["call"]["arguments"]
                    .as_object_mut()
                    .and_then(|arguments| arguments.remove("name"));
                naming["call"]["arguments"]
                    .as_object_mut()
                    .unwrap()
                    .remove("name");
                assert_eq!(actual, naming, "{id}: unnamed {operation} at step {index}");
                assert!(
                    name.as_ref()
                        .and_then(Value::as_str)
                        .is_some_and(|name| !name.trim().is_empty()),
                    "{id}: empty history name for {operation} at step {index}"
                );
                names += 1;
            }
            assert!(
                names > 50,
                "The regression must cover the full flagship source"
            );
        }
    }

    #[test]
    fn recipe_links_select_only_installed_source() {
        for recipe in RECIPES {
            assert_eq!(
                from_open_uri(&format!("nbcad://recipe/{}", recipe.id))
                    .unwrap()
                    .id,
                recipe.id
            );
            assert_eq!(
                from_open_uri(&format!("limo-cad://recipe/{}", recipe.id))
                    .unwrap()
                    .source,
                recipe.source
            );
        }
        for uri in [
            "https://example.org/model.jsonc",
            "limo-cad://recipe/",
            "limo-cad://recipe/unknown",
            "limo-cad://recipe/garden-bench?run=true",
            "limo-cad://recipe/garden-bench#run",
            "limo-cad://recipe/garden-bench/",
            "limo-cad://recipe/../garden-bench",
            "limo-cad://recipe/%67arden-bench",
            "limo-cad://user@recipe/garden-bench",
            "limo-cad://recipe:80/garden-bench",
            "limo-cad://recipe/garden-bench\n",
            "limo-cad://recipe/C:\\model.jsonc",
        ] {
            assert!(from_open_uri(uri).is_err(), "accepted {uri:?}");
        }
    }

    #[test]
    fn showcase_landing_links_select_real_bundled_recipes() {
        let page = include_str!("../../../knowledge/open.html");
        let links = page
            .split("href=\"limo-cad:")
            .skip(1)
            .map(|tail| format!("limo-cad:{}", tail.split('"').next().unwrap()))
            .collect::<Vec<_>>();
        assert!(!links.is_empty());
        for uri in links {
            from_open_uri(&uri).unwrap();
        }
    }

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
