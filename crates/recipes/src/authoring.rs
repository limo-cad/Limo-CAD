//! Naming for the feature-creating calls used by the flagship Rust authors.
use serde_json::{json, Value};

/// Label the newly returned history feature without changing the source call,
/// its result binding, geometry arguments or feature identity.
pub fn feature_name_step(id: &str, operation: &str, arguments: &Value) -> Option<Value> {
    let details = match operation {
        "solid_extrude" => {
            let action = match arguments["operation"].as_str().unwrap_or("new_body") {
                "new_body" => "Stock",
                "join" => "Add",
                "cut" => "Cut",
                value => value,
            };
            let extent = &arguments["extent"];
            if extent["type"] == "distance" {
                format!("{action} {} mm", extent["distance"])
            } else {
                format!("{action} {}", extent["type"].as_str()?.replace('_', " "))
            }
        }
        "solid_fillet" => format!("Fillet R{} mm", arguments["radius"]),
        "solid_chamfer" => format!("Chamfer {} mm", arguments["distance"]),
        "solid_hole" => format!("Hole Ø{} mm", arguments["diameter"]),
        "solid_external_thread" => "External thread".into(),
        "solid_circular_pattern" => format!("Circular pattern / {} instances", arguments["count"]),
        "solid_combine" => format!("Combine / {}", arguments["operation"].as_str()?),
        _ => return None,
    };
    let stem = arguments["sketch_name"]
        .as_str()
        .unwrap_or(id)
        .trim_end_matches("_build")
        .replace('_', " ");
    let name = format!("{stem} / {details}");
    assert!(
        (1..=256).contains(&name.chars().count()) && !name.chars().any(char::is_control),
        "Invalid generated feature name: expected 1..=256 characters and no controls"
    );
    Some(json!({
        "id":format!("{id}_feature_name"),
        "call":{
            "group":"document/history",
            "operation":"solid_rename_feature",
            "arguments":{
                "feature_id":{"$select":{"from":{"$ref":id},"path":"/document/features","take":"last","pointer":"/id"}},
                "name":name
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_history_names_preserve_feature_identity_and_requested_label() {
        let step = feature_name_step(
            "stock_creation",
            "solid_extrude",
            &json!({
                "sketch_name": "customer_design_build",
                "operation": "new_body",
                "extent": {"type": "distance", "distance": 28}
            }),
        )
        .expect("supported feature operation must produce a naming step");
        assert_eq!(step["id"], "stock_creation_feature_name");
        assert_eq!(step["call"]["operation"], "solid_rename_feature");
        assert_eq!(
            step["call"]["arguments"]["feature_id"]["$select"]["from"]["$ref"],
            "stock_creation"
        );
        assert_eq!(
            step["call"]["arguments"]["name"],
            "customer design / Stock 28 mm"
        );
    }

    #[test]
    fn rejected_history_name_diagnostics_do_not_disclose_feature_identity_or_label() {
        for label in [
            format!("private_design_{}", "x".repeat(256)),
            "private_design_\ncontrol".to_owned(),
        ] {
            // These identifiers and labels belong in the generated request, never
            // in failure logs. Exercise both independent validation failures.
            let panic = std::panic::catch_unwind(|| {
                feature_name_step(
                    "private_feature_identity",
                    "solid_fillet",
                    &json!({"sketch_name": label, "radius": 3}),
                )
            })
            .expect_err("invalid authored history name must still be rejected");
            let message = panic
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
                .expect("validation failure must retain a readable diagnostic");
            assert!(message.contains("feature name"));
            assert!(message.contains("1..=256"));
            assert!(message.contains("no controls"));
            assert!(!message.contains("private_feature_identity"));
            assert!(!message.contains("private_design"));
        }
    }
}
