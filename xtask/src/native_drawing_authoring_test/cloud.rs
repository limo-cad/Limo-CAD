//! Loaded-cloud controls are separate from the opt-in actual paper gestures.
use super::*;

pub(super) fn open(c: &mut Client) -> Result<Value> {
    control(c, "More dimensions", None)?;
    control(c, "Revision Cloud", None)
}

pub(in super::super) fn exercise_blank(
    c: &mut Client,
    out: &Path,
    server: &str,
    physical: bool,
) -> Result<Value> {
    let mut prepared = model(c)?;
    ensure!(
        prepared["drawings"]["sheets"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "Cloud fixture requires a fresh blank drawing"
    );
    prepared["drawings"]["sheets"] = json!([
        {"id":1,"name":"Cloud paper","format":"a4","orientation":"landscape","standard":"ansi",
        "title_block":{"title":"Preserved cloud intent","revision":"c\u{03c9}","checked_by":"Saved checker"},
        "tolerance_note":{"preset":"custom","custom":"Saved tolerance text"},
        "release":{"status":"released","released_revision":"c\u{03c9}","released_at":"2026-09-27"},
        "views":[],"annotations":[
            {"kind":"note","id":1,"text":"Saved \u{03a9} note","position":[170.25,25.125]},
            {"kind":"revision_cloud","id":2,"revision":"a\u{03c9}","points":[
                [35.125,60.375],[60.25,55.875],[95.625,68.125],[105.375,94.625],
                [84.125,120.375],[52.875,115.625],[28.375,89.875]]}]},
        {"id":2,"name":"Preserved sheet","format":"a3","orientation":"portrait",
        "annotations":[{"kind":"note","id":3,"text":"Other sheet intent","position":[40.125,30.375]}]}]);
    prepared["drawings"]["active_sheet_id"] = json!(1);
    prepared["drawings"]["next_sheet_id"] = json!(3);
    prepared["drawings"]["next_view_id"] = json!(1);
    prepared["drawings"]["next_annotation_id"] = json!(4);
    c.call(
        "cad_load_project_model",
        json!({"model_json":serde_json::to_string(&prepared)?}),
    )?;
    control(c, "Switch workspace", None)?;
    control(c, "Drawing", None)?;
    control(c, "Fit sheet", None)?;
    let baseline = model(c)?;
    ensure!(
        annotations(&baseline)?[1] == prepared["drawings"]["sheets"][0]["annotations"][1],
        "Loading the fixture changed saved cloud revision or fractional vertices"
    );
    for (key, value) in prepared["drawings"]["sheets"][0]["title_block"]
        .as_object()
        .unwrap()
    {
        ensure!(
            &baseline["drawings"]["sheets"][0]["title_block"][key] == value,
            "Loading the fixture changed saved title-block intent: {key}"
        );
    }
    ensure!(
        baseline["drawings"]["sheets"][0]["standard"] == "ansi"
            && baseline["drawings"]["sheets"][0]["tolerance_note"]
                == prepared["drawings"]["sheets"][0]["tolerance_note"]
            && baseline["drawings"]["sheets"][0]["release"]
                == prepared["drawings"]["sheets"][0]["release"],
        "Loading the fixture changed presentation or released receipt"
    );
    std::fs::write(
        out.join("cloud-baseline-model.json"),
        serde_json::to_vec_pretty(&baseline)?,
    )?;
    capture(c, out, "cloud-loaded")?;
    open(c)?;
    ensure!(
        model(c)? == baseline,
        "Choosing cloud tool on a sheet without views changed the model"
    );
    control(c, "Sheet setup", None)?;
    control(c, "Edit annotation 2", None)?;
    control(c, "Apply annotation", None)?;
    ensure!(
        model(c)? == baseline,
        "No-op Apply changed saved lowercase revision or released receipt"
    );
    field(c, "Revision", "discarded")?;
    ensure!(
        model(c)? == baseline,
        "Cloud field draft changed the saved record"
    );
    control(c, "Reset annotation", None)?;
    ensure!(model(c)? == baseline, "Reset changed loaded cloud intent");
    field(c, "Revision", " \t")?;
    ensure!(
        control(c, "Apply annotation", None).is_err(),
        "Empty revision was accepted"
    );
    ensure!(
        model(c)? == baseline,
        "Invalid revision changed shared intent"
    );
    control(c, "Reset annotation", None)?;
    field(c, "Revision", "b\u{00df}\u{03c9}")?;
    control(c, "Apply annotation", None)?;
    let edited = model(c)?;
    ensure!(
        edited == replace_expected(&baseline, 2, |a| a["revision"] = json!("BSS\u{03a9}")),
        "Revision edit changed arbitrary vertices, paper presentation or unrelated intent"
    );
    history(c, &baseline, &edited)?;
    control(c, "Edit annotation 2", None)?;
    capture(c, out, "cloud-edited")?;
    curved::save_exact(c, out, "cloud-edited", &edited)?;
    control(c, "Apply annotation", None)?;
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == baseline,
        "No-op or invalid Apply inserted a history entry"
    );
    control(c, "Redo", None)?;
    control(c, "Edit annotation 2", None)?;
    control(c, "Delete annotation", None)?;
    let deleted = model(c)?;
    let mut expected = edited.clone();
    expected["drawings"]["sheets"][0]["annotations"]
        .as_array_mut()
        .unwrap()
        .retain(|a| a["id"] != 2);
    ensure!(
        deleted == expected,
        "Cloud delete changed another annotation, ID counter or sheet"
    );
    history(c, &edited, &deleted)?;
    capture(c, out, "cloud-deleted")?;
    control(c, "Undo", None)?;
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == baseline,
        "Cloud edit/delete did not restore the exact issued baseline"
    );
    let physical_result = if physical {
        Some(desktop::exercise_cloud(c, out, server, &baseline)?)
    } else {
        None
    };
    ensure!(
        model(c)? == baseline,
        "Cloud fixture did not restore the complete baseline"
    );
    curved::save_exact(c, out, "cloud-restored", &baseline)?;
    capture(c, out, "cloud-restored")?;
    Ok(
        json!({"status":"passed","published_control_edit_reset_validation_delete":true,
        "seven_loaded_vertices_exactly_preserved":true,"exact_history_archive_release_presentation":true,
        "physical":physical_result,"captures":["cloud-loaded.png","cloud-edited.png","cloud-deleted.png","cloud-restored.png"],
        "not_proven_without_physical_mode":["Triangle and quadrilateral paper authoring","Cloud path and label drags","Escape cancels physical gestures"],
        "not_proven":["macOS input","IME","Drawing export"]}),
    )
}
