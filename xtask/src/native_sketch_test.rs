//! Exercises rendered native controls and the canvas through MCP. The caller
//! supplies one blank development document; this fixture never launches a GUI,
//! closes a window, replaces a document or discards another person's work.
use crate::native_fixture::{click, control, controls, sketch, start, ui, Fixture};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::fs;

fn dimension(client: &mut Client) -> Result<Value> {
    let inspected = ui(client, json!({"action":"inspect"}))?;
    let labels: Vec<_> = controls(&inspected)
        .filter_map(|c| c["label"].as_str())
        .filter(|label| label.starts_with("Edit dimension "))
        .collect();
    ensure!(
        labels.len() == 1,
        "Expected one visible dimension, got {labels:?}"
    );
    control(client, labels[0], None)
}

fn drawing_sizes(client: &mut Client, out: &std::path::Path) -> Result<()> {
    let initial = sketch(client)?;
    control(client, "Rectangle", None)?;
    click(client, [-30., -20.], false)?;
    control(client, "Drawing width", Some("60 mm"))?;
    control(client, "Drawing height", Some("40"))?;
    ui(
        client,
        json!({"action":"viewport","gesture":"move","world":[10.,10.,0.]}),
    )?;
    let layout = ui(client, json!({"action":"inspect"}))?;
    let palette_x = controls(&layout)
        .find(|c| c["label"] == "Sketch Palette")
        .and_then(|c| c["bounds"]["x"].as_f64())
        .context("Palette bounds missing")?;
    for field in controls(&layout).filter(|c| {
        c["label"]
            .as_str()
            .is_some_and(|label| label.starts_with("Drawing "))
    }) {
        let bounds = &field["bounds"];
        ensure!(
            bounds["x"].as_f64().context("Missing field x")?
                + bounds["width"].as_f64().context("Missing field width")?
                <= palette_x,
            "Drawing field overlaps the Sketch Palette: {field}"
        );
    }
    ui(
        client,
        json!({"action":"capture","path":out.join("drawing-size-preview.png")}),
    )?;
    ensure!(
        sketch(client)?["entities"] == initial["entities"],
        "Size preview mutated the sketch"
    );
    click(client, [10., 10.], false)?;
    let rectangle = sketch(client)?;
    ensure!(
        rectangle["dimensions"]
            .as_array()
            .is_some_and(|d| d.len() == 2),
        "Typed rectangle dimensions missing"
    );
    for expected in [60., 40.] {
        ensure!(
            rectangle["dimensions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["value"]
                    .as_f64()
                    .is_some_and(|v| (v - expected).abs() < 1e-6)),
            "Rectangle size {expected} was not applied"
        );
    }
    control(client, "Select", None)?;
    control(client, "Undo", None)?;
    ensure!(
        sketch(client)?["entities"] == initial["entities"],
        "Typed rectangle was not a single undo"
    );
    control(client, "Circle", None)?;
    click(client, [0., 0.], false)?;
    control(client, "Drawing diameter", Some("1 in"))?;
    click(client, [10., 10.], false)?;
    let circle = sketch(client)?;
    ensure!(
        circle["entities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "circle"
                && e["radius"]
                    .as_f64()
                    .is_some_and(|r| (r - 12.7).abs() < 1e-6)),
        "Typed inch diameter did not reach geometry"
    );
    control(client, "Select", None)?;
    control(client, "Undo", None)?;
    ensure!(
        sketch(client)?["entities"] == initial["entities"],
        "Typed circle undo failed"
    );
    println!("PASS: native on-canvas sizes, units, driving dimensions and undo");
    Ok(())
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let Fixture {
        mut client,
        server,
        session,
        out,
        project,
        capture,
        report,
    } = start(args, "native-sketch")?;
    crate::native_fixture::begin_sketch(&mut client, "XY")?;
    let palette = ui(&mut client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&palette).any(|c| c["label"] == "Sketch Palette" && c["expanded"] == true),
        "Sketch palette is missing"
    );
    for label in ["Sketch Grid", "Points"] {
        control(&mut client, label, None)?;
        let inspected = ui(&mut client, json!({"action":"inspect"}))?;
        ensure!(
            controls(&inspected).any(|c| c["label"] == label && c["value"] == false),
            "{label} did not turn off"
        );
        control(&mut client, label, None)?;
    }
    control(&mut client, "Snap", None)?;
    ensure!(
        sketch(&mut client)?["grid_snap"] == false,
        "Snap did not reach the engine"
    );
    control(&mut client, "Snap", None)?;
    ensure!(
        sketch(&mut client)?["grid_snap"] == true,
        "Snap was not restored"
    );
    control(&mut client, "Sketch Palette", None)?;
    let collapsed = ui(&mut client, json!({"action":"inspect"}))?;
    ensure!(
        !controls(&collapsed).any(|c| c["label"] == "Snap"),
        "Collapsed palette still exposes its controls"
    );
    control(&mut client, "Sketch Palette", None)?;
    control(&mut client, "Return to Flat View", None)?;
    drawing_sizes(&mut client, &out)?;
    control(&mut client, "Rectangle", None)?;
    click(&mut client, [-30., -20.], false)?;
    click(&mut client, [30., 20.], false)?;
    control(&mut client, "Select", None)?;
    ui(
        &mut client,
        json!({"action":"viewport","gesture":"move","world":[0.,-20.,0.]}),
    )?;
    let original = sketch(&mut client)?;
    ensure!(
        original["entities"]
            .as_array()
            .context("No sketch geometry")?
            .len()
            == 8,
        "Rectangle did not create four points and four lines"
    );

    click(&mut client, [0., -20.], false)?;
    click(&mut client, [30., 0.], true)?;
    control(&mut client, "Fillet", None)?;
    control(&mut client, "Radius", Some("5"))?;
    ensure!(
        sketch(&mut client)?["entities"] == original["entities"],
        "Fillet preview changed the model before Apply"
    );
    ui(
        &mut client,
        json!({"action":"capture","path":out.join("fillet-preview.png")}),
    )?;
    control(&mut client, "Apply Fillet", None)?;
    let rounded = sketch(&mut client)?;
    ensure!(
        rounded["entities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "arc" && e["radius"] == 5.),
        "Fillet did not create the requested arc"
    );
    control(&mut client, "Undo", None)?;
    ensure!(
        sketch(&mut client)?["entities"] == original["entities"],
        "Fillet Undo did not restore the rectangle"
    );
    println!("PASS: rendered fillet controls, selection and atomic undo");

    click(&mut client, [0., -20.], false)?;
    control(&mut client, "Rectangular Pattern", None)?;
    control(&mut client, "Count", Some("3"))?;
    control(&mut client, "Spacing", Some("10"))?;
    control(&mut client, "Apply Rectangular Pattern", None)?;
    let patterned = sketch(&mut client)?;
    ensure!(
        patterned["entities"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "line")
            .count()
            == 6,
        "Pattern did not add two line instances"
    );
    control(&mut client, "Undo", None)?;
    ensure!(
        sketch(&mut client)?["entities"] == original["entities"],
        "Pattern Undo changed source geometry"
    );
    println!("PASS: rendered pattern fields and atomic undo");

    for (label, menu, field, value, pair) in [
        ("Chamfer", Some("EDIT tools"), Some("Distance"), "3", true),
        ("Offset", None, Some("Distance"), "3", false),
        ("Move/Copy", None, Some("X distance"), "5", false),
        (
            "Scale",
            Some("EDIT tools"),
            Some("Scale factor"),
            "2",
            false,
        ),
        ("Mirror", None, None, "", true),
        ("Circular Pattern", None, Some("Count"), "4", false),
        ("Polygon", Some("DRAW tools"), Some("Sides"), "5", false),
    ] {
        control(&mut client, "Select", None)?;
        if label != "Polygon" {
            click(&mut client, [0., -20.], false)?;
        }
        if pair {
            click(&mut client, [30., 0.], true)?;
        }
        if let Some(menu) = menu {
            control(&mut client, menu, None)?;
        }
        control(&mut client, label, None)?;
        if label == "Offset" {
            click(&mut client, [0., -24.], false)?;
        }
        if label == "Move/Copy" {
            control(&mut client, "Create copy", None)?;
        }
        if let Some(field) = field {
            control(&mut client, field, Some(value))?;
        }
        control(&mut client, &format!("Apply {label}"), None)?;
        ensure!(
            sketch(&mut client)?["entities"] != original["entities"],
            "{label} did not change geometry"
        );
        control(&mut client, "Undo", None)?;
        ensure!(
            sketch(&mut client)?["entities"] == original["entities"],
            "{label} Undo did not restore geometry"
        );
        println!("PASS: {label} through rendered controls and undo");
    }

    click(&mut client, [0., -20.], false)?;
    control(&mut client, "Sketch Dimension", None)?;
    click(&mut client, [0., -28.], false)?;
    control(&mut client, "Dimension value", Some("70"))?;
    control(&mut client, "Apply Dimension", None)?;
    ensure!(
        sketch(&mut client)?["dimensions"][0]["value"] == 70.,
        "Dimension did not drive the model"
    );
    dimension(&mut client)?;
    control(&mut client, "Dimension value", Some("80"))?;
    control(&mut client, "Apply Dimension", None)?;
    ensure!(
        sketch(&mut client)?["dimensions"][0]["value"] == 80.,
        "Visible dimension label did not edit the model"
    );
    dimension(&mut client)?;
    control(&mut client, "Toggle Driving / Reference", None)?;
    ensure!(
        sketch(&mut client)?["dimensions"][0]["mode"] == "reference",
        "Dimension remained driving"
    );
    dimension(&mut client)?;
    let reference = ui(&mut client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&reference).any(|c| c["label"] == "Dimension value" && c["read_only"] == true),
        "Reference measurement must be read only"
    );
    let target = controls(&reference)
        .find(|c| c["label"] == "Dimension value")
        .unwrap()["id"]
        .clone();
    let rejected = client.call(
        "cad_interface",
        json!({"action":"set_value","target":target,"value":"90"}),
    );
    ensure!(
        rejected.is_err(),
        "A reference measurement accepted an edit"
    );
    ensure!(
        sketch(&mut client)?["dimensions"][0]["value"] == 80.,
        "Rejected reference edit changed geometry"
    );
    control(&mut client, "Toggle Driving / Reference", None)?;
    ensure!(
        sketch(&mut client)?["dimensions"][0]["mode"] == "driving",
        "Dimension did not return to driving"
    );
    println!("PASS: visible dimension create/edit/reference/driving");

    let geometry = sketch(&mut client)?["entities"].clone();
    for (toggle, prefix) in [
        ("Dimensions", "Edit dimension "),
        ("Constraints", "Constraint "),
    ] {
        control(&mut client, toggle, None)?;
        let hidden = ui(&mut client, json!({"action":"inspect"}))?;
        ensure!(
            !controls(&hidden).any(|c| c["label"].as_str().is_some_and(|s| s.starts_with(prefix))),
            "Hidden {toggle} still expose selectable annotations"
        );
        control(&mut client, toggle, None)?;
        let restored = ui(&mut client, json!({"action":"inspect"}))?;
        ensure!(
            controls(&restored).any(|c| c["label"].as_str().is_some_and(|s| s.starts_with(prefix))),
            "{toggle} annotations were not restored"
        );
    }
    control(&mut client, "ISO Dimension Style", None)?;
    ensure!(
        sketch(&mut client)?["dimension_style"] == "iso",
        "ISO style did not reach the engine"
    );
    control(&mut client, "ISO Dimension Style", None)?;
    ensure!(
        sketch(&mut client)?["dimension_style"] == "aligned",
        "Aligned style was not restored"
    );
    ensure!(
        sketch(&mut client)?["entities"] == geometry,
        "Palette changes altered geometry"
    );
    println!("PASS: palette collapse, display toggles, snap and dimension style");

    let before = sketch(&mut client)?;
    let inspected = ui(&mut client, json!({"action":"inspect"}))?;
    let label = controls(&inspected)
        .filter_map(|c| c["label"].as_str())
        .find(|s| s.starts_with("Constraint "))
        .context("No visible constraint markers")?
        .to_owned();
    control(&mut client, &label, None)?;
    control(&mut client, "Delete Constraint", None)?;
    ensure!(
        sketch(&mut client)?["constraints"]
            .as_array()
            .unwrap()
            .len()
            + 1
            == before["constraints"].as_array().unwrap().len(),
        "Constraint inspector did not delete its relation"
    );
    control(&mut client, "Undo", None)?;
    ensure!(
        sketch(&mut client)?["constraints"] == before["constraints"],
        "Undo did not restore the deleted constraint"
    );

    control(&mut client, "Fillet", None)?;
    let inspected = control(&mut client, "Radius", Some("invalid_expression"))?;
    let target = controls(&inspected)
        .find(|c| c["label"] == "Radius")
        .context("Missing Radius field")?["id"]
        .clone();
    ui(
        &mut client,
        json!({"action":"key","target":target,"key":"Escape"}),
    )?;
    ensure!(
        !controls(&ui(&mut client, json!({"action":"inspect"}))?)
            .any(|c| c["label"] == "Apply Fillet"),
        "Escape left the form open"
    );
    ensure!(
        sketch(&mut client)?["entities"] == before["entities"],
        "Cancel changed geometry"
    );
    println!("PASS: constraint inspector, undo and Escape from an invalid expression");

    let final_sketch = sketch(&mut client)?;
    ui(&mut client, json!({"action":"capture","path":capture}))?;
    control(&mut client, "Finish sketch", None)?;
    ui(
        &mut client,
        json!({"action":"file","command":"save","path":project}),
    )?;
    ensure!(
        project.is_file() && capture.is_file(),
        "Evidence files were not produced"
    );
    fs::write(
        &report,
        serde_json::to_vec_pretty(&json!({"status":"passed","server":server,"session":session,
        "checks":["nine_modify_forms_and_undo","dimension_edit_modes","constraint_delete_undo","escape_cancel","sketch_palette","drawing_sizes","render_capture","save"],
        "sketch":final_sketch,"project":project,"capture":capture}))?,
    )?;
    println!("PASS: native sketch saved; report {}", report.display());
    Ok(())
}
