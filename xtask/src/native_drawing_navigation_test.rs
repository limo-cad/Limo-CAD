//! Real Winit gestures on a private, PID-verified fixture window.
//! JSON preserves intent; completed paper layout proves zoom, pixels prove pan.
use crate::{
    native_fixture::{capture, control, controls, ui},
    native_platform_test::Driver,
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
mod owned;
pub(super) use owned::run as run_owned;
pub(super) use owned::run_cam as run_cam_owned;
pub(super) use owned::run_cam_geometry as run_cam_geometry_owned;
pub(super) use owned::run_centers as run_centers_owned;
pub(super) use owned::run_chamfer as run_chamfer_owned;
pub(super) use owned::run_cloud as run_cloud_owned;
pub(super) use owned::run_hole as run_hole_owned;
pub(super) use owned::run_mechanism as run_mechanism_owned;
pub(super) use owned::run_output as run_output_owned;
pub(super) use owned::run_scripts as run_scripts_owned;

fn inspect(client: &mut Client) -> Result<Value> {
    ui(client, json!({"action":"inspect"}))
}
fn fitted(state: &Value) -> Result<bool> {
    controls(state)
        .find(|c| c["label"] == "Fit sheet")
        .and_then(|c| c["selected"].as_bool())
        .context("Drawing Fit state missing")
}
fn positive(value: &Value) -> Result<f64> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && *v > 0.)
        .context("Paper navigation measurement missing or non-positive")
}
/// Fit is an independent state assertion. The zoom oracle uses both the
/// presentation transform and the completed Bevy layout, not the toggle.
fn zoom_evidence(before: &Value, after: &Value, inward: bool) -> Result<Value> {
    let a = &before["paper_navigation"];
    let b = &after["paper_navigation"];
    ensure!(
        a["owner"].is_object()
            && a["owner"] == b["owner"]
            && a["sheet_id"].as_u64().is_some()
            && a["sheet_id"] == b["sheet_id"]
            && a["sheet_mm"] == b["sheet_mm"]
            && a["client_size"] == b["client_size"],
        "Paper owner geometry or client changed during zoom"
    );
    let ratio = positive(&b["paper_scale"])? / positive(&a["paper_scale"])?;
    ensure!(
        if inward {
            (1.02..3.).contains(&ratio)
        } else {
            (0.3..0.98).contains(&ratio)
        },
        "Paper scale did not change in the requested bounded direction (ratio {ratio})"
    );
    let mut rendered_ratios = [0.; 2];
    for axis in 0..2 {
        let extent = |view: &Value| -> Result<f64> {
            let size = positive(&view["rendered_size_px"][axis])?;
            let dpi = positive(&view["render_scale"])?;
            let mm = positive(&view["sheet_mm"][axis])?;
            let scale = positive(&view["paper_scale"])?;
            ensure!(
                (size / dpi - mm * scale).abs() <= 1.,
                "Completed paper layout disagrees with its presentation transform"
            );
            Ok(size / dpi)
        };
        rendered_ratios[axis] = extent(b)? / extent(a)?;
        ensure!(
            (rendered_ratios[axis] - ratio).abs() < 0.005,
            "Zoom state changed without the rendered paper extent"
        );
    }
    Ok(json!({"scale_ratio":ratio,"rendered_extent_ratios":rendered_ratios}))
}
pub(super) fn canvas(state: &Value) -> Result<&Value> {
    state["ui"]["canvases"]
        .as_array()
        .and_then(|c| c.iter().find(|c| c["name"] == "drawing"))
        .filter(|c| {
            ["x", "y", "width", "height"]
                .iter()
                .all(|key| c[key].as_f64().is_some_and(f64::is_finite))
        })
        .filter(|c| c["width"].as_f64().unwrap() > 0. && c["height"].as_f64().unwrap() > 0.)
        .context("Visible drawing canvas missing")
}
pub(super) fn owned_pid(out: &Path, session: &str, server: &str) -> Result<u32> {
    let root = out
        .parent()
        .context("Owned evidence root")?
        .canonicalize()?;
    let sessions = PathBuf::from(
        std::env::var_os("LIMO_CAD_SESSION_DIR").context("Private session registry required")?,
    )
    .canonicalize()?;
    ensure!(
        sessions == root.join("sessions"),
        "Drawing input requires the fixture's private session registry"
    );
    let launch: Value = serde_json::from_slice(&fs::read(root.join("host.json"))?)?;
    ensure!(
        PathBuf::from(launch["exe"].as_str().context("Owned launcher binary")?).canonicalize()?
            == Path::new(server).canonicalize()?,
        "Owned launch binary does not match fixture server"
    );
    let pid = u32::try_from(launch["pid"].as_u64().context("Owned launcher PID")?)?;
    let mut matches = 0;
    for entry in fs::read_dir(sessions.join("_ui/processes"))? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let registry: Value = serde_json::from_slice(&fs::read(path)?)?;
        if registry["pid"] == pid
            && registry["windows"]
                .as_array()
                .is_some_and(|windows| windows.iter().any(|w| w["active_session_id"] == session))
        {
            matches += 1;
        }
    }
    ensure!(
        matches == 1,
        "Session is not uniquely owned by the fixture launch PID"
    );
    Ok(pid)
}
pub(super) fn exercise(
    client: &mut Client,
    out: &Path,
    session: &str,
    server: &str,
    segments: usize,
    desktop_input: bool,
) -> Result<Value> {
    ensure!(
        cfg!(target_os = "windows") || cfg!(target_os = "linux"),
        "Real drawing input requires Windows or disposable Linux Xvfb"
    );
    let current = inspect(client)?;
    fs::write(
        out.join("navigation-owner.json"),
        serde_json::to_vec_pretty(&current)?,
    )?;
    let active_session = current["active_session_id"]
        .as_str()
        .context("Current native session missing")?;
    let pid = owned_pid(out, active_session, server)?;
    let before = client.call("cad_project_model", json!({}))?;
    control(client, "Fit sheet", None)?;
    let fit = inspect(client)?;
    ensure!(fitted(&fit)?, "Fit did not become selected");
    let fit_capture = ui(
        client,
        json!({"action":"capture","path":out.join("dense-fit.png")}),
    )?;
    if std::env::var("LIMO_CAD_NATIVE_PAPER_DIAGNOSTICS").as_deref() == Ok("1") {
        fs::write(
            out.join("dense-fit-capture.json"),
            serde_json::to_vec_pretty(&fit_capture)?,
        )?;
        ensure!(fit_capture["value"]["paper_probe"]["fitted_paper_white"]==true,
            "Fitted paper is not visible at the known 3 mm blank margin; dense-fit.png and paper diagnostics were retained");
    }
    control(client, "Zoom drawing in", None)?;
    let button_zoom = inspect(client)?;
    let button_in_evidence = zoom_evidence(&fit, &button_zoom, true)?;
    ensure!(
        !fitted(&inspect(client)?)?,
        "Zoom buttons did not leave fitted mode"
    );
    capture(client, out, "dense-button-zoom")?;
    control(client, "Zoom drawing out", None)?;
    let button_out_evidence = zoom_evidence(&button_zoom, &inspect(client)?, false)?;
    capture(client, out, "dense-button-out")?;
    control(client, "Fit sheet", None)?;
    let restored = inspect(client)?;
    ensure!(
        fitted(&restored)?
            && canvas(&restored)? == canvas(&fit)?
            && restored["paper_navigation"] == fit["paper_navigation"],
        "Fit did not restore the same paper bounds"
    );
    ensure!(
        client.call("cad_project_model", json!({}))? == before,
        "MCP paper navigation changed saved intent"
    );
    fs::write(
        out.join("navigation-mcp.json"),
        serde_json::to_vec_pretty(&json!({
            "mcp_fit_zoom_passed":true,"model_exactly_preserved":true,"dense_view_count":20,"visible_segments":segments,
            "os_input":if desktop_input {"pending; a later helper failure must not be reported as a pass"} else {"not attempted (--mcp-only)"},"fit_canvas":canvas(&fit)?,
            "button_zoom_in":button_in_evidence,"button_zoom_out":button_out_evidence
        }))?,
    )?;
    if !desktop_input {
        return Ok(
            json!({"mcp_fit_zoom_passed":true,"dense_view_count":20,"visible_segments":segments,
            "model_exactly_preserved":true,"os_input":"not attempted (--mcp-only)","pan_inverse_pixels_exact":null,
            "captures":["dense-fit.png","dense-button-zoom.png","dense-button-out.png"],
            "not_proven":["OS wheel","OS middle pan","macOS/Linux paper gestures","monitor DPI transition","touchpad hardware"]}),
        );
    }
    if cfg!(target_os = "linux") {
        owned::verify_display()?;
    }
    let driver = Driver::new(pid, out)?;
    driver.event("focus")?;
    let bounds = canvas(&restored)?;
    let point = [
        bounds["x"].as_f64().unwrap() + bounds["width"].as_f64().unwrap() * 0.5,
        bounds["y"].as_f64().unwrap() + bounds["height"].as_f64().unwrap() * 0.5,
    ];
    let mut input_evidence = Vec::new();
    let wheel_calls = 1;
    let mut wheel_requests = Vec::new();
    for invocation in 1..=wheel_calls {
        let request = json!({"x":point[0],"y":point[1],"notches":1,"ctrl":true,"client":restored["ui"]["client"]});
        let wheel_evidence = driver.invoke("drawing-wheel", Some(&request.to_string()))?;
        let receipt = if wheel_evidence.trim().is_empty() {
            Value::Null
        } else {
            let receipt = serde_json::from_str::<Value>(&wheel_evidence)?;
            input_evidence.push(receipt.clone());
            receipt
        };
        wheel_requests.push(json!({"invocation":invocation,"request":request,"receipt":receipt}));
    }
    let wheel_intent = json!({"total_notches":wheel_calls,"invocations":wheel_requests});
    fs::write(
        out.join("navigation-wheel-input.json"),
        serde_json::to_vec_pretty(&wheel_intent)?,
    )?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let wheel_zoom_evidence = loop {
        let observed = inspect(client)?;
        fs::write(
            out.join("navigation-wheel-layout.json"),
            serde_json::to_vec_pretty(&observed["paper_navigation"])?,
        )?;
        if let Ok(evidence) = zoom_evidence(&restored, &observed, true) {
            ensure!(!fitted(&observed)?, "Wheel zoom retained Fit state");
            break evidence;
        }
        ensure!(
            Instant::now() < deadline,
            "One actual OS Ctrl-wheel did not enlarge the completed paper layout; no input retry was sent"
        );
        thread::sleep(Duration::from_millis(30));
    };
    capture(client, out, "dense-os-wheel")?;
    // One notch creates only a small overflow. Keep the inverse pan inside
    // that range; clamping a long drag is a different behavior.
    let target = [point[0] - 8., point[1] - 6.];
    let pan_evidence=driver.invoke(
        "drawing-pan",
        Some(&json!({"x":point[0],"y":point[1],"to_x":target[0],"to_y":target[1],"client":restored["ui"]["client"]}).to_string()),
    )?;
    if !pan_evidence.trim().is_empty() {
        input_evidence.push(serde_json::from_str::<Value>(&pan_evidence)?);
    }
    capture(client, out, "dense-os-pan")?;
    let back_evidence=driver.invoke(
        "drawing-pan",
        Some(&json!({"x":target[0],"y":target[1],"to_x":point[0],"to_y":point[1],"client":restored["ui"]["client"]}).to_string()),
    )?;
    if !back_evidence.trim().is_empty() {
        input_evidence.push(serde_json::from_str::<Value>(&back_evidence)?);
    }
    fs::write(
        out.join("navigation-os-input.json"),
        serde_json::to_vec_pretty(&input_evidence)?,
    )?;
    capture(client, out, "dense-os-pan-back")?;
    ensure!(
        fs::read(out.join("dense-os-pan.png"))? != fs::read(out.join("dense-os-wheel.png"))?,
        "Middle pan did not change captured pixels"
    );
    ensure!(
        fs::read(out.join("dense-os-pan-back.png"))? == fs::read(out.join("dense-os-wheel.png"))?,
        "Inverse middle pan did not restore exact captured pixels"
    );
    control(client, "Fit sheet", None)?;
    capture(client, out, "dense-fit-restored")?;
    let final_fit = inspect(client)?;
    ensure!(
        fitted(&final_fit)? && final_fit["paper_navigation"] == fit["paper_navigation"],
        "Fit did not reset after actual pan"
    );
    ensure!(
        client.call("cad_project_model", json!({}))? == before,
        "Paper navigation changed document or drawing intent"
    );
    Ok(
        json!({"actual_input":if cfg!(target_os="linux"){"X11 XTEST wheel/middle button and pointer movement into real Winit"}else{"Rust/Enigo MCP wheel/middle button and OS cursor movement into real Winit"},
        "initial_session":session,"active_session":active_session,"dense_view_count":20,"visible_segments":segments,"model_exactly_preserved":true,"pan_inverse_pixels_exact":true,"input_evidence":input_evidence,"wheel_intent":wheel_intent,"wheel_zoom":wheel_zoom_evidence,
        "captures":["dense-fit.png","dense-button-zoom.png","dense-button-out.png","dense-os-wheel.png","dense-os-pan.png","dense-os-pan-back.png","dense-fit-restored.png"],
        "not_proven":["Other OS paper gestures","monitor DPI transition","touchpad hardware","Wayland"]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn layout(scale: f64, dpi: f64) -> Value {
        json!({"paper_navigation":{"owner":{"window_id":"fixture","document_id":"paper","epoch":1},"sheet_id":7,"paper_scale":scale,"sheet_mm":[297.,210.],
            "rendered_size_px":[297.*scale*dpi,210.*scale*dpi],"render_scale":dpi,
            "client_size":[1360.,860.]}})
    }
    #[test]
    fn zoom_requires_completed_layout_and_rejects_state_only_and_dpi_changes() {
        let before = layout(2., 1.);
        let zoomed = layout(2.24, 1.);
        zoom_evidence(&before, &zoomed, true).unwrap();
        zoom_evidence(&zoomed, &before, false).unwrap();
        let mut fit_only = before.clone();
        fit_only["selected"] = json!(false);
        assert!(zoom_evidence(&before, &fit_only, true).is_err());
        let mut state_only = before.clone();
        state_only["paper_navigation"]["paper_scale"] = json!(2.24);
        assert!(zoom_evidence(&before, &state_only, true).is_err());
        assert!(zoom_evidence(&before, &layout(2., 2.), true).is_err());
        assert!(zoom_evidence(&before, &zoomed, false).is_err());
        for (field, changed) in [
            ("owner", json!({"epoch":2})),
            ("sheet_id", json!(8)),
            ("client_size", json!([1920., 1080.])),
        ] {
            let mut replaced = zoomed.clone();
            replaced["paper_navigation"][field] = changed;
            assert!(zoom_evidence(&before, &replaced, true).is_err());
        }
        for field in [
            "paper_scale",
            "sheet_mm",
            "rendered_size_px",
            "render_scale",
        ] {
            let mut missing = zoomed.clone();
            missing["paper_navigation"][field] = Value::Null;
            assert!(zoom_evidence(&before, &missing, true).is_err());
        }
    }
    #[test]
    fn canvas_requires_the_flat_published_rectangle() {
        let published = json!({"name":"drawing","x":355.96,"y":132.,"width":888.08,"height":628.});
        assert_eq!(
            canvas(&json!({"ui":{"canvases":[published.clone()]}})).unwrap(),
            &published
        );
        for invalid in [
            json!({"name":"drawing","bounds":published}),
            json!({"name":"drawing"}),
            json!({"name":"drawing","x":0.,"y":0.,"width":0.,"height":100.}),
        ] {
            assert!(canvas(&json!({"ui":{"canvases":[invalid]}})).is_err());
        }
    }
}
