//! Real adjacent reorder controls. The same selected identity survives each
//! move and its single Undo/Redo step; only its canonical list position changes.
use super::*;
use crate::native_fixture::control_in;
use std::path::Path;

#[derive(Clone)]
struct Row {
    setup: usize,
    operation: Option<usize>,
    label: String,
}

fn selected(c: &mut Client, document: &Value, operation: bool) -> Result<Row> {
    let view = ui(c, json!({"action":"inspect"}))?;
    let mut found = Vec::new();
    for (setup_index, setup) in document["setups"]
        .as_array()
        .context("CAM setups missing")?
        .iter()
        .enumerate()
    {
        let name = setup["name"].as_str().context("Setup name missing")?;
        let rows = if operation {
            setup["operations"]
                .as_array()
                .context("CAM operations missing")?
                .iter()
                .enumerate()
                .map(|(index, operation)| {
                    Ok(Row {
                        setup: setup_index,
                        operation: Some(index),
                        label: format!(
                            "{name} / {}",
                            operation["name"]
                                .as_str()
                                .context("Operation name missing")?
                        ),
                    })
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            vec![Row {
                setup: setup_index,
                operation: None,
                label: name.into(),
            }]
        };
        for row in rows {
            if controls(&view).any(|control| {
                control["surface"] == "cam/document"
                    && control["label"] == row.label
                    && control["selected"] == true
                    && control["disabled"] == false
            }) {
                found.push(row);
            }
        }
    }
    ensure!(
        found.len() == 1,
        "Choose one visible CAM {} before checking reorder",
        if operation { "toolpath" } else { "setup" }
    );
    Ok(found.remove(0))
}

fn swap_up(document: &mut Value, row: &Row) -> Result<()> {
    let (rows, index) = if let Some(index) = row.operation {
        (
            document["setups"][row.setup]["operations"]
                .as_array_mut()
                .context("CAM operation list missing")?,
            index,
        )
    } else {
        (
            document["setups"]
                .as_array_mut()
                .context("CAM setup list missing")?,
            row.setup,
        )
    };
    ensure!(
        index > 0 && index < rows.len(),
        "Run the reorder fixture after Duplicate, with its copy selected after a predecessor"
    );
    rows.swap(index, index - 1);
    Ok(())
}

fn assert_selected(c: &mut Client, row: &Row) -> Result<()> {
    let view = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&view).any(|control| {
            control["surface"] == "cam/document"
                && control["label"] == row.label
                && control["selected"] == true
        }),
        "Reorder or its history lost the selected CAM identity {}",
        row.label
    );
    Ok(())
}

fn check(c: &mut Client, out: &Path, operation: bool) -> Result<()> {
    let baseline = document(c)?;
    let row = selected(c, &baseline, operation)?;
    let before_model = c.call("cad_project_model", json!({}))?;
    let mut expected_model: Value = serde_json::from_str(
        before_model
            .as_str()
            .context("Project export was not JSON text")?,
    )?;
    let mut expected = baseline.clone();
    swap_up(&mut expected, &row)?;
    swap_up(&mut expected_model["cam"], &row)?;

    control_in(c, "cam/document", "Move up", None)?;
    ensure!(
        document(c)? == expected,
        "Move up changed more than list order, including keyed intent or generation stamps"
    );
    ensure!(
        project_model(c)? == expected_model,
        "Move up changed project data outside the exact CAM list permutation"
    );
    let moved_model = c.call("cad_project_model", json!({}))?;
    history(c, &before_model, &moved_model)?;
    assert_selected(c, &row)?;
    capture(
        c,
        out,
        if operation {
            "cam-operation-reorder"
        } else {
            "cam-setup-reorder"
        },
    )?;

    control_in(c, "cam/document", "Move down", None)?;
    ensure!(
        document(c)? == baseline && c.call("cad_project_model", json!({}))? == before_model,
        "Move down did not restore the exact initial project and CAM order"
    );
    history(c, &moved_model, &before_model)?;
    assert_selected(c, &row)?;
    Ok(())
}

/// Call immediately after operation Duplicate, before deleting its selected copy.
pub(super) fn check_operation(c: &mut Client, out: &Path) -> Result<()> {
    check(c, out, true)
}

/// Call immediately after setup Duplicate, before deleting its selected copy.
pub(super) fn check_setup(c: &mut Client, out: &Path) -> Result<()> {
    check(c, out, false)
}
