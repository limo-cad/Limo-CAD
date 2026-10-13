//! Exercise preset editing and explicit operation copies through retained UI.
use super::*;

pub(super) fn check(c: &mut Client, out: &std::path::Path) -> Result<()> {
    let baseline = document(c)?;
    let before = c.call("cad_project_model", json!({}))?;
    field(c, "Tool section", "presets")?;
    control(c, "Add preset", None)?;
    field(c, "Preset name", "Native finish")?;
    field(c, "Preset spindle (rpm)", "14500")?;
    field(c, "Preset cutting feed (mm/min)", "900")?;
    field(c, "Preset plunge feed (mm/min)", "90")?;
    field(c, "Preset coolant", "mist")?;
    control(c, "Apply", None)?;
    let added = document(c)?;
    let mut expected = baseline.clone();
    expected["tools"][0]["cutting_presets"] = json!([{
        "name":"Native finish",
        "cutting":{"spindle_rpm":14500,"feed_xy":900.,"feed_z":90.,"coolant":"mist"}
    }]);
    ensure!(
        added == expected,
        "Adding a named preset changed unrelated CAM data"
    );
    let after = c.call("cad_project_model", json!({}))?;
    history(c, &before, &after)?;

    control(c, "Copy preset", None)?;
    control(c, "Apply", None)?;
    let copied = document(c)?;
    let rows = copied["tools"][0]["cutting_presets"]
        .as_array()
        .context("Preset rows")?;
    ensure!(
        rows.len() == 2
            && rows[1]["name"] == "Native finish copy"
            && rows[1]["cutting"] == rows[0]["cutting"],
        "Preset copy lost the named canonical cutting data"
    );
    let copied_model = c.call("cad_project_model", json!({}))?;
    history(c, &after, &copied_model)?;
    field(c, "Cutting preset", "1")?;
    control(c, "Remove preset", None)?;
    control(c, "Apply", None)?;
    ensure!(
        document(c)? == added,
        "Removing the selected copy changed another preset"
    );
    let removed_model = c.call("cad_project_model", json!({}))?;
    history(c, &copied_model, &removed_model)?;

    field(c, "Preset name", "")?;
    rejected(c, "Apply")?;
    ensure!(
        document(c)? == added,
        "Invalid preset name changed the document"
    );
    control(c, "Reset", None)?;
    capture(c, out, "cam-cutting-presets")?;

    control(c, "Toolpaths", None)?;
    field(c, "Operation section", "parameters")?;
    field(c, "Copy tool cutting preset", "0")?;
    control(c, "Apply", None)?;
    let applied = document(c)?;
    let mut expected = added.clone();
    expected["setups"][0]["operations"][0]["cutting"] =
        added["tools"][0]["cutting_presets"][0]["cutting"].clone();
    ensure!(
        applied == expected,
        "Explicit preset copy changed more than operation cutting data"
    );
    let applied_model = c.call("cad_project_model", json!({}))?;
    history(c, &removed_model, &applied_model)?;
    capture(c, out, "cam-operation-cutting-preset")?;
    control(c, "Undo", None)?;
    ensure!(
        document(c)? == added,
        "Undo did not restore the prior programmed cutting data"
    );
    control(c, "Project tools", None)?;
    field(c, "Tool section", "tool")?;
    Ok(())
}
