//! Finite owned-window reproduction for #383. Captures are API-driven and may
//! request a redraw; this does not qualify passive wake delivery or OS input.
use crate::{
    native_fixture::{browser_select, capture, control, edit_feature, ui},
    native_platform_test::{wait_for_interface, wait_for_owned_window},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, process::Command, time::Duration};

fn planes(c: &mut Client) -> Result<Value> {
    c.call("construction_plane_definitions", json!({}))
}

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let (mut server, mut out, mut revision) = (None, None, None);
    while let Some(arg) = args.next() {
        let value = args.next().context("Missing option value")?;
        match arg.as_str() {
            "--server" if server.is_none() => server = Some(PathBuf::from(value)),
            "--out" if out.is_none() => out = Some(PathBuf::from(value)),
            "--expected-revision" if revision.is_none() => revision = Some(value),
            _ => anyhow::bail!("Unknown or duplicate option {arg}"),
        }
    }
    let server = server.context("Use --server")?.canonicalize()?;
    let out = out.context("Use --out")?;
    let revision = revision.context("Use --expected-revision for the packaged source")?;
    ensure!(
        revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit()),
        "Expected a full source revision"
    );
    ensure!(
        out.is_absolute() && (!out.exists() || fs::read_dir(&out)?.next().is_none()),
        "Choose a fresh absolute evidence directory"
    );
    fs::create_dir_all(&out)?;
    let sessions = out.join("sessions");
    limo_cad_session_storage::create_registry(&sessions)?;
    let config = out.join("config");
    fs::create_dir(&config)?;
    fs::write(
        config.join("app-preferences.json"),
        br#"{"schema_version":1,"theme":"light","locale":"en","ui_scale":1.0}"#,
    )?;
    let mut command = Command::new(&server);
    command
        .current_dir(&out)
        .env("LIMO_CAD_SESSION_DIR", &sessions)
        .env("LIMO_CAD_CONFIG_DIR", &config);
    let mut c = Client::start_command_logged(command, Some(Duration::from_secs(45)), &out)?;
    let result = (|| {
        let session = wait_for_owned_window(&mut c, &sessions)?;
        c.call("cad_attach", json!({"session_id":session}))?;
        wait_for_interface(&mut c, &session)?;
        let inspected = ui(&mut c, json!({"action":"inspect"}))?;
        let build = inspected["desktop_build"].clone();
        ensure!(
            build["revision"] == revision && build["modified"] == false,
            "Packaged source identity mismatch: {build}"
        );
        let document = c.call("cad_document", json!({}))?;
        ensure!(
            document["features"].as_array().is_some_and(Vec::is_empty),
            "Owned document must be blank"
        );
        // Keep the original owned blank tab so Close/Open exercises a cold
        // document without closing the last native window.
        let created = control(&mut c, "New design", None)?;
        let working = created["active_session_id"]
            .as_str()
            .context("New design session")?;
        ensure!(working != session, "New design reused the blank tab");
        c.call("cad_attach", json!({"session_id":working}))?;
        wait_for_interface(&mut c, working)?;
        control(&mut c, "Isometric", None)?;
        capture(&mut c, &out, "00-empty")?;
        control(&mut c, "Offset Plane", None)?;
        browser_select(&mut c, "Origin", "XY")?;
        control(&mut c, "Offset distance", Some("25"))?;
        capture(&mut c, &out, "01-preview")?;
        control(&mut c, "Apply Offset Plane", None)?;
        // The first request after Apply is capture: no unrelated model edit,
        // camera operation or explicit wait is inserted to make the plane appear.
        capture(&mut c, &out, "02-immediate-after-apply")?;
        let first = planes(&mut c)?;
        ensure!(
            first.as_array().is_some_and(|v| v.len() == 1),
            "Expected one offset plane"
        );
        ensure!(
            first[0]["basis"]["origin"][2] == 25.,
            "Wrong initial plane: {first}"
        );
        let name = first[0]["name"].as_str().context("Plane name")?.to_owned();
        let id = first[0]["datum_id"].clone();
        edit_feature(&mut c, &name, false)?;
        control(&mut c, "Offset distance", Some("35"))?;
        control(&mut c, "Apply Offset Plane", None)?;
        capture(&mut c, &out, "03-immediate-after-edit")?;
        let edited = planes(&mut c)?;
        ensure!(
            edited[0]["datum_id"] == id && edited[0]["basis"]["origin"][2] == 35.,
            "Edit lost plane identity or position"
        );
        control(&mut c, "Undo", None)?;
        capture(&mut c, &out, "04-undo")?;
        ensure!(
            planes(&mut c)? == first,
            "Undo did not restore the initial plane"
        );
        control(&mut c, "Redo", None)?;
        capture(&mut c, &out, "05-redo")?;
        ensure!(
            planes(&mut c)? == edited,
            "Redo did not restore the edited plane"
        );
        for (visible, image) in [(false, "06-hidden"), (true, "07-shown")] {
            c.call(
                "construction_set_visibility",
                json!({"visible":visible,"datum_plane_ids":[id],"sketch_names":[]}),
            )?;
            capture(&mut c, &out, image)?;
            ensure!(
                planes(&mut c)? == edited,
                "Visibility changed plane geometry"
            );
        }
        let project = out.join("offset-plane.limo");
        ui(
            &mut c,
            json!({"action":"file","command":"save","path":project}),
        )?;
        let closed = ui(&mut c, json!({"action":"file","command":"close"}))?;
        ensure!(
            closed["active_session_id"] == session,
            "Close did not return to the owned blank tab"
        );
        c.call(
            "cad_attach",
            json!({"session_id":closed["active_session_id"]}),
        )?;
        let opened = ui(
            &mut c,
            json!({"action":"file","command":"open","path":project}),
        )?;
        c.call(
            "cad_attach",
            json!({"session_id":opened["active_session_id"]}),
        )?;
        ensure!(
            planes(&mut c)? == edited,
            "Cold document reopen changed the plane"
        );
        capture(&mut c, &out, "08-reopened")?;
        Ok::<_, anyhow::Error>(
            json!({"passed":true,"source":build,"platform":std::env::consts::OS,
            "server":server,"owned_pid":c.process_id(),"project":project,"plane":edited,
            "scope":"API-driven native controls and rendered captures; manual image inspection required",
            "not_proven":["passive redraw/wake delivery","OS pointer or keyboard delivery","Kubuntu","original reporter sequence"]}),
        )
    })();
    let report = match &result {
        Ok(value) => value.clone(),
        Err(error) => {
            json!({"passed":false,"error":format!("{error:#}"),"expected_revision":revision})
        }
    };
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    result.map(|_| ())
}
