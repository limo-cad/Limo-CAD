//! Published table controls on the existing real-solid annotation fixture.
//! Exact models, history and archives prove that rows remain shared DTOs.
use super::*;

fn sheet(model: &Value) -> &Value {
    let id = &model["drawings"]["active_sheet_id"];
    model["drawings"]["sheets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sheet| &sheet["id"] == id)
        .unwrap()
}
fn sheet_mut(model: &mut Value) -> &mut Value {
    let id = model["drawings"]["active_sheet_id"].clone();
    model["drawings"]["sheets"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|sheet| sheet["id"] == id)
        .unwrap()
}
fn ordinary_edit(before: &Value, change: impl FnOnce(&mut Value)) -> Value {
    let mut expected = before.clone();
    let current = sheet_mut(&mut expected);
    change(current);
    if current["release"]["status"] == "released" {
        current["release"]["status"] = json!("draft");
    }
    expected
}
fn row_mut<'a>(model: &'a mut Value, collection: &str, id: u64) -> &'a mut Value {
    sheet_mut(model)[collection]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == id)
        .unwrap()
}
fn image(c: &mut Client, out: &Path, names: &mut Vec<String>, stage: &str) -> Result<()> {
    let name = format!("author-table-{stage}");
    capture(c, out, &name)?;
    names.push(format!("{name}.png"));
    Ok(())
}
fn commit(
    c: &mut Client,
    out: &Path,
    stage: &str,
    command: &str,
    before: &Value,
    expected: &Value,
    changes: &mut usize,
) -> Result<Value> {
    control(c, command, None).with_context(|| format!("Table {stage}: {command}"))?;
    let actual = model(c)?;
    std::fs::write(
        out.join(format!("author-table-{stage}-expected.json")),
        serde_json::to_vec_pretty(expected)?,
    )?;
    std::fs::write(
        out.join(format!("author-table-{stage}-actual.json")),
        serde_json::to_vec_pretty(&actual)?,
    )?;
    ensure!(
        &actual == expected,
        "Table {stage} changed unexpected model intent; exact evidence saved"
    );
    ensure!(
        &actual != before,
        "Table {stage} did not create its expected change"
    );
    history(c, before, &actual)
        .with_context(|| format!("Table {stage}: exact one-entry history"))?;
    *changes += 1;
    Ok(actual)
}
fn revision_fields(c: &mut Client, code: &str, description: &str) -> Result<()> {
    for (label, value) in [
        ("Revision code", code),
        ("Description", description),
        ("Date", "2026-09-27"),
        ("Status", "draft"),
        ("Drawn by", "Native QA"),
        ("Checked by", "CHECK"),
        ("Approved by", "APPROVE"),
        ("Change order", "ECO-124"),
    ] {
        field(c, label, value)?;
    }
    Ok(())
}
fn revision(id: u64, code: &str, description: &str) -> Value {
    json!({"id":id,"revision":code,"description":description,"date":"2026-09-27",
        "status":"draft","author":"Native QA","checked_by":"CHECK",
        "approved_by":"APPROVE","change_order":"ECO-124"})
}
fn choose(c: &mut Client, table: &str, row_label: &str, id: u64) -> Result<()> {
    control(c, table, None)?;
    control(c, row_label, Some(&id.to_string()))?;
    Ok(())
}
pub(super) fn exercise(c: &mut Client, out: &Path, baseline: &Value) -> Result<Value> {
    ensure!(
        &model(c)? == baseline,
        "Table fixture did not receive the exact authoring baseline"
    );
    let bom_id = sheet(baseline)["bom"]
        .as_array()
        .context("Existing BOM")?
        .first()
        .context("Existing linked BOM row")?["id"]
        .as_u64()
        .context("BOM ID")?;
    let balloon_ids = sheet(baseline)["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["kind"] == "item_balloon" && a["bom_item_id"] == bom_id)
        .map(|a| a["id"].clone())
        .collect::<Vec<_>>();
    ensure!(
        !balloon_ids.is_empty(),
        "Table fixture requires the existing sheet-6 item balloon"
    );
    let mut changes = 0;
    let mut captures = Vec::new();
    let mut current = baseline.clone();
    let revision_id = current["drawings"]["next_revision_id"]
        .as_u64()
        .context("Revision counter")?;

    control(c, "Revisions", None)?;
    control(c, "Add row", None)?;
    revision_fields(c, "TABLE-B", "Native revision draft")?;
    ensure!(
        model(c)? == current,
        "Staged revision wrote the shared document"
    );
    image(c, out, &mut captures, "revision-staged")?;
    control(c, "Cancel", None)?;
    ensure!(
        model(c)? == current,
        "Cancelling a new revision consumed an ID or history"
    );
    control(c, "Add row", None)?;
    revision_fields(c, "TABLE-B", "Native revision draft")?;
    let mut expected = ordinary_edit(&current, |s| {
        s["revisions"].as_array_mut().unwrap().push(revision(
            revision_id,
            "TABLE-B",
            "Native revision draft",
        ));
        s["title_block"]["revision"] = json!("TABLE-B");
    });
    expected["drawings"]["next_revision_id"] = json!(revision_id + 1);
    current = commit(
        c,
        out,
        "revision-create",
        "Apply",
        &current,
        &expected,
        &mut changes,
    )?;
    choose(c, "Revisions", "Revision row", revision_id)?;
    field(c, "Description", "Reviewed native table")?;
    field(c, "Status", "in_review")?;
    ensure!(model(c)? == current, "Revision edit committed before Apply");
    expected = ordinary_edit(&current, |_| {});
    row_mut(&mut expected, "revisions", revision_id)["description"] =
        json!("Reviewed native table");
    row_mut(&mut expected, "revisions", revision_id)["status"] = json!("in_review");
    current = commit(
        c,
        out,
        "revision-edit",
        "Apply",
        &current,
        &expected,
        &mut changes,
    )?;
    choose(c, "Revisions", "Revision row", revision_id)?;
    field(c, "Status", "released")?;
    expected = current.clone();
    row_mut(&mut expected, "revisions", revision_id)["status"] = json!("released");
    sheet_mut(&mut expected)["release"] =
        json!({"status":"released","released_revision":"TABLE-B","released_at":"2026-09-27"});
    current = commit(
        c,
        out,
        "revision-release",
        "Apply",
        &current,
        &expected,
        &mut changes,
    )?;
    choose(c, "Revisions", "Revision row", revision_id)?;
    let state = ui(c, json!({"action":"inspect"}))?;
    let code = controls(&state)
        .find(|r| r["label"] == "Revision code")
        .context("Issued revision code field")?;
    let delete = controls(&state)
        .find(|r| r["label"] == "Delete row")
        .context("Issued revision delete control")?;
    ensure!(
        code["read_only"] == true && delete["disabled"] == true,
        "Issued revision controls are editable"
    );
    ensure!(
        ui(
            c,
            json!({"action":"set_value","target":code["id"],"value":"ILLEGAL"})
        )
        .is_err(),
        "Issued revision accepted a field edit"
    );
    ensure!(
        ui(c, json!({"action":"click","target":delete["id"]})).is_err(),
        "Issued revision accepted deletion"
    );
    ensure!(
        model(c)? == current,
        "Issued revision rejection changed document/history"
    );
    image(c, out, &mut captures, "revision-issued")?;
    curved::save_exact(c, out, "table-revision-issued", &current)?;

    choose(c, "Bill of materials", "BOM row", bom_id)?;
    field(c, "Quantity", "0")?;
    ensure!(
        control(c, "Apply", None).is_err(),
        "BOM accepted zero quantity"
    );
    ensure!(
        model(c)? == current,
        "Invalid BOM edit changed the document"
    );
    control(c, "Reset", None)?;
    for (label, value) in [
        ("Item number", "7A"),
        ("Part number", "PART-TABLE"),
        ("Description", "Fractional native BOM"),
        ("Quantity", "2.5"),
        ("Material", "Al"),
        ("Finish", "Anodized"),
    ] {
        field(c, label, value)?;
    }
    ensure!(model(c)? == current, "BOM edit committed before Apply");
    expected = ordinary_edit(&current, |_| {});
    let row = row_mut(&mut expected, "bom", bom_id);
    for (key, value) in [
        ("item_number", json!("7A")),
        ("part_number", json!("PART-TABLE")),
        ("description", json!("Fractional native BOM")),
        ("quantity", json!(2.5)),
        ("material", json!("Al")),
        ("finish", json!("Anodized")),
    ] {
        row[key] = value;
    }
    current = commit(
        c,
        out,
        "bom-stable-id",
        "Apply",
        &current,
        &expected,
        &mut changes,
    )?;
    ensure!(
        sheet(&current)["release"]["status"] == "draft"
            && sheet(&current)["release"]["released_revision"] == "TABLE-B"
            && sheet(&current)["revisions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == revision_id && r["status"] == "released"),
        "Ordinary edit lost the issued revision or its receipt"
    );

    control(c, "Bill of materials", None)?;
    control(c, "Add row", None)?;
    field(c, "Description", "Cancelled BOM row")?;
    control(c, "Cancel", None)?;
    ensure!(
        model(c)? == current,
        "Cancelled BOM row consumed an ID or history"
    );
    let new_bom_id = current["drawings"]["next_bom_item_id"].as_u64().unwrap();
    control(c, "Add row", None)?;
    for (label, value) in [
        ("Item number", "8"),
        ("Part number", "PART-8"),
        ("Description", "Added native row"),
        ("Body", ""),
        ("Quantity", "0.25"),
        ("Material", "Steel"),
        ("Finish", "Plain"),
    ] {
        field(c, label, value)?;
    }
    expected = ordinary_edit(&current, |s| {
        s["bom"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":new_bom_id,"item_number":"8",
            "body_id":null,"part_number":"PART-8","description":"Added native row",
            "quantity":0.25,"material":"Steel","finish":"Plain"}));
    });
    expected["drawings"]["next_bom_item_id"] = json!(new_bom_id + 1);
    current = commit(
        c,
        out,
        "bom-create",
        "Apply",
        &current,
        &expected,
        &mut changes,
    )?;
    choose(c, "Bill of materials", "BOM row", new_bom_id)?;
    expected = ordinary_edit(&current, |s| {
        s["bom"]
            .as_array_mut()
            .unwrap()
            .retain(|r| r["id"] != new_bom_id)
    });
    current = commit(
        c,
        out,
        "bom-delete-unlinked",
        "Delete row",
        &current,
        &expected,
        &mut changes,
    )?;

    for (table, row_label, position, moved, defaults) in [
        (
            "Revisions",
            "Revision row",
            "revision_table_position",
            [15., 105.],
            [10., 10.],
        ),
        (
            "Bill of materials",
            "BOM row",
            "bom_table_position",
            [145., 105.],
            [10., 45.],
        ),
    ] {
        let tag = if table == "Revisions" {
            "revision"
        } else {
            "bom"
        };
        choose(c, table, row_label, 0)?;
        field(c, "Show table", "shown")?;
        field(c, "Table X (mm)", &moved[0].to_string())?;
        field(c, "Table Y (mm)", &moved[1].to_string())?;
        ensure!(model(c)? == current, "Table move committed before Apply");
        expected = ordinary_edit(&current, |s| s[position] = json!(moved));
        current = commit(
            c,
            out,
            &format!("{tag}-move"),
            "Apply",
            &current,
            &expected,
            &mut changes,
        )?;
        image(c, out, &mut captures, &format!("{tag}-moved"))?;
        choose(c, table, row_label, 0)?;
        field(c, "Show table", "hidden")?;
        expected = ordinary_edit(&current, |s| s[position] = Value::Null);
        current = commit(
            c,
            out,
            &format!("{tag}-hide"),
            "Apply",
            &current,
            &expected,
            &mut changes,
        )?;
        image(c, out, &mut captures, &format!("{tag}-hidden"))?;
        choose(c, table, row_label, 0)?;
        field(c, "Show table", "shown")?;
        expected = ordinary_edit(&current, |s| s[position] = json!(defaults));
        current = commit(
            c,
            out,
            &format!("{tag}-show-default"),
            "Apply",
            &current,
            &expected,
            &mut changes,
        )?;
    }
    image(c, out, &mut captures, "defaults-and-linked-balloon")?;
    curved::save_exact(c, out, "table-visible", &current)?;
    choose(c, "Bill of materials", "BOM row", bom_id)?;
    expected = ordinary_edit(&current, |s| {
        s["bom"]
            .as_array_mut()
            .unwrap()
            .retain(|r| r["id"] != bom_id);
        s["annotations"]
            .as_array_mut()
            .unwrap()
            .retain(|a| !(a["kind"] == "item_balloon" && a["bom_item_id"] == bom_id));
    });
    current = commit(
        c,
        out,
        "bom-delete-linked",
        "Delete row",
        &current,
        &expected,
        &mut changes,
    )?;
    image(c, out, &mut captures, "linked-balloon-deleted")?;
    curved::save_exact(c, out, "table-linked-delete", &current)?;

    for _ in 0..changes {
        control(c, "Undo", None)?;
    }
    ensure!(
        &model(c)? == baseline,
        "Table fixture did not restore the exact 24-annotation baseline"
    );
    control(c, "Sheet setup", None)?;
    let report = json!({"shared_table_controls_passed":true,"exact_history_archive_passed":true,
        "accepted_document_changes":changes,"staged_cancel_preserves_ids":true,
        "released_revision_read_only":true,"fractional_quantities":[2.5,0.25],
        "stable_bom_id":bom_id,"linked_balloon_ids":balloon_ids,
        "same_sheet_linked_delete_exact":true,"other_sheets_and_solid_preserved":true,
        "restored_exact_baseline":true,"captures":captures,
        "not_proven":["Physical table field typing","Physical table placement drag"]});
    std::fs::write(
        out.join("author-tables.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
