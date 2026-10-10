//! Actual Windows UIA calls into the production AccessKit provider of our child.
//! No keyboard, mouse or clipboard input is needed for this scenario.
use super::*;

fn automation(driver: &Driver, out: &Path, name: &str, request: Value) -> Result<Value> {
    let value: Value =
        serde_json::from_str(&driver.invoke("accessibility", Some(&request.to_string()))?)?;
    fs::write(
        out.join(format!("{name}.json")),
        serde_json::to_vec_pretty(&value)?,
    )?;
    ensure!(
        value["owned_pid"] == driver.pid,
        "UIA returned a foreign process"
    );
    Ok(value)
}

pub(super) fn exercise(
    client: &mut Client,
    driver: &Driver,
    out: &Path,
    session: &str,
) -> Result<Value> {
    let before = client.call("cad_project_model", json!({}))?;
    let tree = automation(driver, out, "uia-startup", json!({"operation":"inspect"}))?;
    ensure!(
        tree["controls"].as_array().is_some_and(|nodes| nodes
            .iter()
            .any(|node| node["name"] == "File" && node["enabled"] == true)),
        "UIA has no named File control"
    );
    automation(
        driver,
        out,
        "uia-file",
        json!({"operation":"invoke","label":"File"}),
    )?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let frame = ui(client, json!({"action":"inspect"}))?;
        if controls(&frame).any(|c| c["label"] == "Rename Project…") {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "UIA File action did not open its live menu"
        );
        thread::sleep(Duration::from_millis(50));
    }
    automation(
        driver,
        out,
        "uia-rename",
        json!({"operation":"invoke","label":"Rename Project…"}),
    )?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let frame = ui(client, json!({"action":"inspect"}))?;
        if controls(&frame).any(|c| c["label"] == "Project name") {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "UIA Rename did not expose its editor"
        );
        thread::sleep(Duration::from_millis(50));
    }
    automation(
        driver,
        out,
        "uia-value",
        json!({"operation":"set_value","label":"Project name","value":"UIA Café 零件"}),
    )?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let frame = ui(client, json!({"action":"inspect"}))?;
        if controls(&frame).any(|c| c["label"] == "Project name" && c["value"] == "UIA Café 零件")
        {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "UIA ValuePattern did not update the actual editor"
        );
        thread::sleep(Duration::from_millis(50));
    }
    automation(
        driver,
        out,
        "uia-cancel",
        json!({"operation":"invoke","label":"Cancel"}),
    )?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let frame = ui(client, json!({"action":"inspect"}))?;
        if !controls(&frame).any(|c| c["label"] == "Project name") {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "UIA Cancel did not retire the editor"
        );
        thread::sleep(Duration::from_millis(50));
    }
    ensure!(
        client.call("cad_project_model", json!({}))? == before,
        "Assistive editing/cancellation changed the document"
    );
    capture(client, out, "uia-restored")?;
    Ok(
        json!({"status":"passed","event_source":"Windows UI Automation / production AccessKit",
        "owned_pid":driver.pid,"session":session,"named_roles":true,
        "invoke_file_and_rename":true,"unicode_value_pattern":true,"cancel_preserves_exact_document":true,
        "not_tested":["screen reader speech","physical keyboard","other platform accessibility adapters"]}),
    )
}
