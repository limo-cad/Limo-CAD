//! Read-only completion observations after the one production UI request.
use super::*;

const SETTLE_BUDGET: Duration = Duration::from_secs(5);

fn remaining(deadline: Instant) -> Result<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    ensure!(
        !remaining.is_zero(),
        "Read-only switching observation exceeded its 5s budget"
    );
    Ok(remaining)
}

fn read(client: &mut Client, name: &str, arguments: Value, deadline: Instant) -> Result<Value> {
    let result = client.rpc_with_timeout(
        "tools/call",
        json!({"name":name,"arguments":arguments}),
        remaining(deadline)?,
    )?;
    let result = Client::decode_call_result(name, result)?;
    if name == "cad_interface" {
        ensure!(
            result["status"] == "applied",
            "Interface observation failed: {result}"
        );
    }
    Ok(result)
}

fn same_owner(observed: &Value, requested: &Value) -> bool {
    observed["active_session_id"].as_str().is_some()
        && observed["active_session_id"] == requested["active_session_id"]
        && observed["attached_session_id"] == requested["attached_session_id"]
}

fn focused(observed: &Value, os: &Value, requested: &Value) -> bool {
    same_owner(observed, requested)
        && (observed["value"]["focused"] == true || observed["window"]["focused"] == true)
        && os["owned_focus"] == true
}

pub(super) fn foreground(client: &mut Client, requested: &Value) -> Result<Value> {
    let deadline = Instant::now() + SETTLE_BUDGET;
    let mut observations = 0;
    loop {
        let observed = read(
            client,
            "cad_interface",
            json!({"action":"window","mode":"inspect"}),
            deadline,
        )?;
        ensure!(
            same_owner(&observed, requested),
            "Foreground observation changed document owner"
        );
        let os = crate::linux_fixture::observe_focus(client.process_id(), deadline)?;
        observations += 1;
        if focused(&observed, &os, requested) {
            return Ok(
                json!({"observations":observations,"window":observed["window"],
                "value":observed["value"],"active_session_id":observed["active_session_id"],"os":os}),
            );
        }
        ensure!(
            Instant::now() < deadline,
            "Owned focus did not settle within 5s: value={}, window={}, os={os}",
            observed["value"],
            observed["window"]
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn selected_sheet(observed: &Value, expected: &Value) -> bool {
    let sheet = expected["drawings"]["sheets"]
        .as_array()
        .and_then(|sheets| {
            sheets
                .iter()
                .find(|s| s["id"] == expected["drawings"]["active_sheet_id"])
        });
    let Some(name) = sheet.and_then(|sheet| sheet["name"].as_str()) else {
        return false;
    };
    let mut fields =
        controls(observed).filter(|c| c["label"] == "Sheet name" && c["role"] == "textbox");
    fields.next().is_some_and(|field| field["value"] == name) && fields.next().is_none()
}

pub(super) fn navigation(
    options: &Options,
    host: &mut Host,
    requested: &Value,
    instance: usize,
    cycle: usize,
    n: usize,
) -> Result<Value> {
    let deadline = Instant::now() + SETTLE_BUDGET;
    let mut observations = 0;
    loop {
        let observed = read(
            &mut host.client,
            "cad_interface",
            json!({"action":"inspect"}),
            deadline,
        )?;
        ensure!(
            same_owner(&observed, requested),
            "Navigation observation changed document owner"
        );
        let text = read(&mut host.client, "cad_project_model", json!({}), deadline)?;
        let current: Value = serde_json::from_str(text.as_str().context("Project model missing")?)?;
        observations += 1;
        let selected = !options.sheets || selected_sheet(&observed, &host.models[n]);
        if current == host.models[n] && selected {
            return Ok(
                json!({"observations":observations,"exact_expected_model_preserved":true,
                "active_session_id":observed["active_session_id"],"selected_sheet_verified":options.sheets}),
            );
        }
        let pending = endpoint_only(&current, &host.models);
        if !pending || Instant::now() >= deadline {
            fs::write(
                options
                    .out
                    .join(format!("changed-{instance}-{cycle}-{n}.json")),
                serde_json::to_vec_pretty(&current)?,
            )?;
            fs::write(
                options
                    .out
                    .join(format!("unsettled-ui-{instance}-{cycle}-{n}.json")),
                serde_json::to_vec_pretty(&observed)?,
            )?;
            bail!("Switch did not reach the exact expected model and selected sheet in instance {instance}, cycle {cycle}, target {n}: endpoint_only={pending}, selected_sheet={selected}, observations={observations}");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn endpoint_only(current: &Value, models: &[Value; 2]) -> bool {
    current == &models[0] || current == &models[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreground_requires_current_application_and_os_focus_for_the_same_owner() {
        let request = json!({"active_session_id":"owned","attached_session_id":"owned","value":{"focused":false}});
        let mut observed = request.clone();
        let os = json!({"owned_focus":true});
        assert!(!focused(&observed, &os, &request));
        observed["value"]["focused"] = json!(true);
        assert!(focused(&observed, &os, &request));
        assert!(!focused(&observed, &json!({"owned_focus":false}), &request));
        observed["active_session_id"] = json!("different");
        assert!(!focused(&observed, &os, &request));
        let window_receipt = json!({"active_session_id":"owned","attached_session_id":"owned","window":{"focused":true}});
        assert!(focused(&window_receipt, &os, &request));
    }
    #[test]
    fn stale_sheet_receipt_cannot_certify_the_new_sheet() {
        let expected = json!({"drawings":{"active_sheet_id":2,"sheets":[{"id":1,"name":"Sparse"},{"id":2,"name":"Dense"}]}});
        let mut observed = json!({"ui":{"surfaces":[{"controls":[{"label":"Sheet name","role":"textbox","value":"Sparse"}]}]}});
        assert!(!selected_sheet(&observed, &expected));
        observed["ui"]["surfaces"][0]["controls"][0]["value"] = json!("Dense");
        assert!(selected_sheet(&observed, &expected));
        observed["ui"]["surfaces"][0]["controls"][0]["role"] = json!("button");
        assert!(!selected_sheet(&observed, &expected));
    }
    #[test]
    fn waiting_permits_only_exact_navigation_endpoints_not_unrelated_model_changes() {
        let first = json!({"drawings":{"active_sheet_id":1,"sheets":[1,2]},"geometry":[3],"cam":{"setups":[]},"document":{"features":[4]}});
        let mut second = first.clone();
        second["drawings"]["active_sheet_id"] = json!(2);
        let endpoints = [first.clone(), second.clone()];
        assert!(endpoint_only(&first, &endpoints));
        assert!(endpoint_only(&second, &endpoints));
        for pointer in [
            "/drawings/sheets",
            "/geometry",
            "/cam/setups",
            "/document/features",
        ] {
            let mut changed = second.clone();
            *changed.pointer_mut(pointer).unwrap() = json!([999]);
            assert!(!endpoint_only(&changed, &endpoints), "{pointer}");
        }
    }
}
