//! Retained linking controls over the isolated contour used by the parent
//! fixture. The original complete project is restored before returning.
use super::*;
#[path = "linking_pick.rs"]
mod picking;

pub(super) fn check(c: &mut Client, out: &Path, created: &Value, server: &str) -> Result<Value> {
    let original = document(c)?;
    ensure!(
        operation(&original)?["kind"] == "contour2d"
            && operation(&original)?["id"] == operation(created)?["id"],
        "Linking fixture needs its isolated contour"
    );
    let before = model(c)?;
    section(c, "linking")?;
    field(c, "Link programming", "custom")?;
    field(c, "Predrill position count", "2")?;
    field(c, "Predrill position 2 X (mm)", "10")?;
    field(c, "Predrill position 2 Y (mm)", "11")?;
    field(c, "Predrill position number", "1")?;
    field(c, "Predrill position 1 X (mm)", "0.1")?;
    field(c, "Predrill position 1 Y (mm)", "0.3")?;
    field(c, "Predrill position number", "2")?;
    field(c, "Predrill position number", "1")?;
    field(c, "Preferred entry position count", "1")?;
    field(c, "Preferred entry position 1 source", "manual")?;
    let state = ui(c, json!({"action":"inspect"}))?;
    let source = controls(&state)
        .find(|control| control["label"] == "Preferred entry position 1 source")
        .context("Actual preferred-entry candidate control missing")?;
    let candidate = source["options"]
        .as_array()
        .context("Position choices missing")?
        .iter()
        .find(|option| {
            option["disabled"] == false
                && option["value"]
                    .as_str()
                    .is_some_and(|value| value.starts_with("vertex:"))
        })
        .context("No explicit setup-body vertex candidate was offered")?["value"]
        .as_str()
        .unwrap()
        .to_owned();
    let parts = candidate.split(':').collect::<Vec<_>>();
    ensure!(parts.len() == 3, "Unexpected explicit vertex key");
    let body_id = parts[1].parse::<u64>()?;
    let index = parts[2].parse::<usize>()?;
    let solid = scene(c)?;
    let body = solid["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|body| body["id"] == body_id)
        .context("Selected vertex body is unavailable")?;
    let positions = body["mesh"]["positions"]
        .as_array()
        .context("Body mesh missing")?;
    let wcs = &original["setups"][0]["wcs"];
    let relative = ["x", "y", "z"].map(|axis| wcs["origin"][axis].as_f64().unwrap());
    let relative =
        [0, 1, 2].map(|axis| positions[index * 3 + axis].as_f64().unwrap() - relative[axis]);
    let project = |axis: &str| {
        (0..3)
            .map(|i| relative[i] * wcs[axis][i].as_f64().unwrap())
            .sum::<f64>()
    };
    let entry = json!({"x":project("x_axis"),"y":project("y_axis")});
    field(c, "Preferred entry position 1 source", &candidate)?;
    field(c, "Preferred exit position count", "1")?;
    field(c, "Preferred exit position 1 X (mm)", "NaN")?;
    field(c, "Preferred exit position 1 Y (mm)", "13")?;
    let error = control(c, "Apply", None)
        .err()
        .context("Nonfinite linking point unexpectedly applied")?
        .to_string();
    ensure!(
        error.contains("finite"),
        "Expected finite-coordinate validation, got {error}"
    );
    ensure!(
        model(c)? == before,
        "Invalid linking input changed the complete project"
    );
    field(c, "Preferred exit position 1 X (mm)", "12")?;
    capture(c, out, "geometry-linking-points")?;
    control(c, "Apply", None)?;
    let linked = document(c)?;
    let unchanged = restore_linking(&linked, &original)?;
    if unchanged != original {
        std::fs::write(
            out.join("geometry-linking-unrelated-difference.json"),
            serde_json::to_string_pretty(&json!({
                "entry_cam":original,"edited_cam":linked,
                "edited_cam_with_original_linking":unchanged,
                "different_top_level_keys":different_keys(&original, &unchanged)
            }))?,
        )?;
    }
    ensure!(
        unchanged == original,
        "Linking controls changed unrelated CAM DTOs or generation stamps; see geometry-linking-unrelated-difference.json"
    );
    let linking = linked["linking"]
        .as_array()
        .context("Shared linking records missing")?
        .iter()
        .find(|link| link["operation_id"] == operation(&original).unwrap()["id"])
        .context("Edited contour has no keyed linking record")?;
    ensure!(
        linking["predrill_positions"] == json!([{"x":0.1,"y":0.3},{"x":10.,"y":11.}])
            && linking["entry_positions"] == json!([entry])
            && linking["exit_positions"] == json!([{"x":12.,"y":13.}]),
        "Native linking row edits or explicit vertex selection changed coordinates: {linking}"
    );
    let after = model(c)?;
    history(c, &before, &after)?;
    let saved = save(c, &out.join("geometry-linking-points.limo"))?;
    let physical = if std::env::var("LIMO_CAD_NATIVE_CAM_PICK_INPUT").as_deref() == Ok("1") {
        Some(picking::exercise(c, out, server)?)
    } else {
        None
    };
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == before,
        "Linking fixture did not restore the original complete project"
    );
    section(c, "parameters")?;
    Ok(
        json!({"native_fields":true,"invalid_input_preserved_project":true,"explicit_vertex":candidate,
        "linked":linked,"saved_model":saved,"physical_input":physical,"restored_complete_project":true,
        "scope":"linking DTO editing and persistence; generation remains covered by the parent fixture"}),
    )
}

fn restore_linking(edited: &Value, original: &Value) -> Result<Value> {
    let mut unchanged = edited.clone();
    let object = unchanged
        .as_object_mut()
        .context("CAM document is not an object")?;
    match original.get("linking") {
        Some(linking) => {
            object.insert("linking".into(), linking.clone());
        }
        None => {
            object.remove("linking");
        }
    }
    Ok(unchanged)
}

fn different_keys<'a>(before: &'a Value, after: &'a Value) -> Vec<&'a str> {
    let mut keys = before
        .as_object()
        .into_iter()
        .flat_map(|value| value.keys())
        .chain(after.as_object().into_iter().flat_map(|value| value.keys()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .filter(|key| before.get(*key) != after.get(*key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linking_comparison_preserves_absent_and_present_keys_without_hiding_other_edits() {
        for original in [
            json!({"tools":[],"toolpath_generations":[{"id":7}]}),
            json!({"tools":[],"toolpath_generations":[{"id":7}],"linking":[]}),
            json!({"tools":[],"toolpath_generations":[{"id":7}],"linking":[{"operation_id":3}]}),
        ] {
            let mut edited = original.clone();
            edited["linking"] = json!([{"operation_id":7}]);
            assert_eq!(restore_linking(&edited, &original).unwrap(), original);
            edited["toolpath_generations"][0]["id"] = json!(8);
            let restored = restore_linking(&edited, &original).unwrap();
            assert_ne!(restored, original);
            assert_eq!(
                different_keys(&original, &restored),
                vec!["toolpath_generations"]
            );
        }
    }
}
