//! Preflight for isolated teaching previews. The general product catalog also
//! contains session, process and file operations; catalog validity alone does
//! not establish preview isolation. New operations are unavailable here until
//! their detached execution and resource costs have been reviewed.
use limo_cad_script::Script;

pub(super) fn validate(script: &Script) -> Result<(), String> {
    let metadata = script.metadata();
    let steps = metadata["step_count"].as_u64().unwrap_or(0)
        + metadata["check_count"].as_u64().unwrap_or(0);
    if steps > 80 {
        return Err(
            "Preview supports at most 80 steps and checks; run this script in a new design instead"
                .into(),
        );
    }
    // Walk every call, including late checks, before any engine construction or
    // command execution. Do not infer permission from group or name prefixes.
    script.validate_calls(|group, operation| {
        crate::interface::validate_script_call(group, operation)?;
        if !supported(operation) {
            return Err(format!("Operation {operation} is not available in isolated preview; run the script explicitly in a new design"));
        }
        Ok(())
    })
}

fn supported(operation: &str) -> bool {
    // These commands operate on the detached manager/kernel used by the two
    // existing preview lessons. None can attach a session, route a request,
    // start a process or select a file destination.
    matches!(
        operation,
        "assembly_create_component"
            | "assembly_create_occurrence"
            | "assembly_set_occurrence_grounded"
            | "assembly_solution"
            | "cad_project_model"
            | "sketch_add_constraint"
            | "sketch_add_rectangle_locked"
            | "sketch_begin"
            | "sketch_edit"
            | "sketch_edit_dimension"
            | "sketch_finish"
            | "sketch_finished"
            | "sketch_undo"
            | "solid_edit_extrude"
            | "solid_extrude"
            | "solid_fillet"
            | "solid_recompute"
            | "solid_scene"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn existing_preview_recipes_pass_the_explicit_operation_policy() {
        let mut count = 0;
        for recipe in limo_cad_recipes::RECIPES
            .iter()
            .filter(|recipe| recipe.preview)
        {
            validate(&Script::parse(recipe.source).unwrap())
                .unwrap_or_else(|error| panic!("{}: {error}", recipe.id));
            count += 1;
        }
        assert!(count > 0);
    }

    #[test]
    fn external_effect_operations_fail_before_preview_engine_construction() {
        for operation in [
            "cad_attach",
            "cad_route",
            "bambu_local_verification_start",
            "print_calibrate",
            "solid_export_step",
        ] {
            let group = crate::interface::group_for(operation).unwrap();
            for late in [false, true] {
                let call = json!({"id":"forbidden","call":{"group":group,"operation":operation,"arguments":{}}});
                let mut document = json!({"version":1,"name":"Isolated preview policy","steps":[{"note":"No commands should execute"}]});
                if late {
                    document["checks"] = json!([call]);
                } else {
                    document["steps"].as_array_mut().unwrap().push(call);
                }
                let source = document.to_string();
                // These are actual product catalog operations, not unknown names
                // accidentally rejected by the ordinary script validator.
                crate::interface::validate_script(&Script::parse(&source).unwrap()).unwrap();
                let error = crate::preview_script(&source).unwrap_err();
                assert!(
                    error.contains(operation) && error.contains("isolated preview"),
                    "{error}"
                );
            }
        }
    }

    #[test]
    fn preview_limits_and_catalog_identity_remain_required() {
        let source =
            json!({"version":1,"name":"Too many steps","steps":vec![json!({"note":"Step"});81]})
                .to_string();
        assert!(crate::preview_script(&source)
            .unwrap_err()
            .contains("80 steps"));
        assert!(crate::preview_script(&" ".repeat(2 * 1024 * 1024 + 1))
            .unwrap_err()
            .contains("2 MiB"));
        let source = json!({"version":1,"name":"Wrong group","steps":[{"call":{"group":"wrong/group","operation":"solid_scene","arguments":{}}}]}).to_string();
        assert!(crate::preview_script(&source)
            .unwrap_err()
            .contains("belongs to"));
        assert!(!supported("future_external_operation"));
    }
}
