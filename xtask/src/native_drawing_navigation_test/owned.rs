//! Disposable owned-window launcher shared with the established platform
//! input handshake. Drawing and CAM fixtures retain the same exact PID proof.
use super::*;
use anyhow::bail;
use std::{ffi::OsString, process::Command};

struct PrivateEnvironment(Vec<(&'static str, Option<OsString>)>);
#[derive(Clone, Copy, PartialEq)]
enum Fixture {
    Drawing,
    DrawingOutput,
    Centers,
    Hole,
    Scripts,
    Mechanism,
    Cam,
    Chamfer,
    Cloud,
    CamGeometry,
}
pub(super) fn verify_display() -> Result<()> {
    crate::linux_fixture::verify_private_display().map(|_| ())
}
impl PrivateEnvironment {
    fn set(root: &Path, fixture: Fixture) -> Self {
        let values = [
            ("LIMO_CAD_SESSION_DIR", root.join("sessions")),
            ("LIMO_CAD_CONFIG_DIR", root.join("config")),
        ];
        let mut saved = Vec::new();
        for (key, value) in values {
            saved.push((key, std::env::var_os(key)));
            std::env::set_var(key, value);
        }
        let flags: &[&str] = match fixture {
            Fixture::Drawing | Fixture::DrawingOutput | Fixture::Scripts => &[],
            Fixture::Hole => &["LIMO_CAD_NATIVE_HOLE_INPUT"],
            Fixture::Cam => &[
                "LIMO_CAD_NATIVE_CAM_ROW_INPUT",
                "LIMO_CAD_NATIVE_CAM_WCS_INPUT",
            ],
            Fixture::Chamfer => &[
                "LIMO_CAD_NATIVE_CHAMFER_ONLY",
                "LIMO_CAD_NATIVE_CHAMFER_INPUT",
            ],
            Fixture::Cloud => &["LIMO_CAD_NATIVE_CLOUD_ONLY", "LIMO_CAD_NATIVE_CLOUD_INPUT"],
            Fixture::Centers => &[
                "LIMO_CAD_NATIVE_CENTERS_ONLY",
                "LIMO_CAD_NATIVE_CENTERS_INPUT",
            ],
            Fixture::CamGeometry => &["LIMO_CAD_NATIVE_CAM_PICK_INPUT"],
            Fixture::Mechanism => &["LIMO_CAD_NATIVE_MECHANISM_INPUT"],
        };
        for &key in flags {
            saved.push((key, std::env::var_os(key)));
            std::env::set_var(key, "1");
        }
        Self(saved)
    }
}
impl Drop for PrivateEnvironment {
    fn drop(&mut self) {
        for (key, prior) in self.0.drain(..) {
            if let Some(value) = prior {
                std::env::set_var(key, value)
            } else {
                std::env::remove_var(key)
            }
        }
    }
}

pub(in super::super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    run_fixture(args, Fixture::Drawing)
}

pub(in super::super) fn run_cam(args: impl Iterator<Item = String>) -> Result<()> {
    run_fixture(args, Fixture::Cam)
}

pub(in super::super) fn run_chamfer(args: impl Iterator<Item = String>) -> Result<()> {
    run_fixture(args, Fixture::Chamfer)
}

pub(in super::super) fn run_cloud(args: impl Iterator<Item = String>) -> Result<()> {
    run_fixture(args, Fixture::Cloud)
}

pub(in super::super) fn run_output(args: impl Iterator<Item = String>) -> Result<()> {
    run_fixture(args, Fixture::DrawingOutput)
}

pub(in super::super) fn run_centers(args: impl Iterator<Item = String>) -> Result<()> {
    run_fixture(args, Fixture::Centers)
}

pub(in super::super) fn run_hole(args: impl Iterator<Item = String>) -> Result<()> {
    run_fixture(args, Fixture::Hole)
}

pub(in super::super) fn run_scripts(args: impl Iterator<Item = String>) -> Result<()> {
    run_fixture(args, Fixture::Scripts)
}

pub(in super::super) fn run_mechanism(args: impl Iterator<Item = String>) -> Result<()> {
    run_fixture(args, Fixture::Mechanism)
}

pub(in super::super) fn run_cam_geometry(args: impl Iterator<Item = String>) -> Result<()> {
    run_fixture(args, Fixture::CamGeometry)
}

fn run_fixture(mut args: impl Iterator<Item = String>, fixture: Fixture) -> Result<()> {
    let mut server = None;
    let mut out = None;
    let mut desktop = false;
    let mut authoring = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--server" => {
                server = Some(PathBuf::from(args.next().context("Missing --server path")?))
            }
            "--out" => out = Some(PathBuf::from(args.next().context("Missing --out path")?)),
            "--desktop-input" => desktop = true,
            "--authoring-input" => authoring = true,
            _ => bail!("Unknown owned native input option {arg}"),
        }
    }
    ensure!(
        desktop && cfg!(target_os = "linux"),
        "Use --desktop-input only on the disposable Linux Xvfb runner"
    );
    ensure!(
        fixture == Fixture::Drawing || !authoring,
        "Only drawing navigation accepts --authoring-input"
    );
    verify_display()?;
    let server = server
        .context("Use --server for the native desktop binary")?
        .canonicalize()?;
    let out = out.context("Use --out for an empty evidence root")?;
    ensure!(
        out.is_absolute() && (!out.exists() || fs::read_dir(&out)?.next().is_none()),
        "Choose a fresh absolute evidence root; preserve previous results"
    );
    fs::create_dir_all(&out)?;
    let out = out.canonicalize()?;
    let _environment = PrivateEnvironment::set(&out, fixture);
    let sessions = out.join("sessions");
    limo_cad_session_storage::create_registry(&sessions)?;
    let result = (|| {
        let mut command = Command::new(&server);
        command.current_dir(&sessions);
        let mut host = if std::env::var("LIMO_CAD_NATIVE_PAPER_DIAGNOSTICS").as_deref() == Ok("1") {
            Client::start_command_logged(command, Some(Duration::from_secs(45)), &out)?
        } else {
            Client::start_command(command, Some(Duration::from_secs(45)))?
        };
        fs::write(
            out.join("host.json"),
            serde_json::to_vec_pretty(&json!({"pid":host.process_id(),"exe":server}))?,
        )?;
        let session = crate::native_platform_test::wait_for_owned_window(&mut host, &sessions)?;
        host.call("cad_attach", json!({"session_id":session}))?;
        crate::native_platform_test::wait_for_interface(&mut host, &session)?;
        fs::write(out.join("session.txt"), &session)?;
        let mut fixture_args = vec![
            "--server".into(),
            server.to_string_lossy().into_owned(),
            "--session".into(),
            session,
            "--out".into(),
            out.join("evidence").to_string_lossy().into_owned(),
        ];
        if fixture == Fixture::Drawing || fixture == Fixture::Hole {
            fixture_args.push("--desktop-input".into());
        }
        match fixture {
            Fixture::Mechanism => {
                std::env::set_var("LIMO_CAD_NATIVE_MECHANISM_INPUT", "1");
            }
            Fixture::Chamfer => {
                std::env::set_var("LIMO_CAD_NATIVE_CHAMFER_ONLY", "1");
                std::env::set_var("LIMO_CAD_NATIVE_CHAMFER_INPUT", "1");
            }
            Fixture::Cloud => {
                std::env::set_var("LIMO_CAD_NATIVE_CLOUD_ONLY", "1");
                std::env::set_var("LIMO_CAD_NATIVE_CLOUD_INPUT", "1");
            }
            Fixture::Centers => {
                std::env::set_var("LIMO_CAD_NATIVE_CENTERS_ONLY", "1");
                std::env::set_var("LIMO_CAD_NATIVE_CENTERS_INPUT", "1");
            }
            Fixture::Cam => {
                std::env::set_var("LIMO_CAD_NATIVE_CAM_ROW_INPUT", "1");
                std::env::set_var("LIMO_CAD_NATIVE_CAM_WCS_INPUT", "1");
            }
            Fixture::CamGeometry => {
                std::env::set_var("LIMO_CAD_NATIVE_CAM_PICK_INPUT", "1");
            }
            Fixture::Hole => {
                std::env::set_var("LIMO_CAD_NATIVE_HOLE_INPUT", "1");
            }
            Fixture::Drawing | Fixture::DrawingOutput | Fixture::Scripts => {}
        }
        if authoring {
            fixture_args.push("--authoring-input".into());
        }
        match fixture {
            Fixture::Drawing => {
                crate::native_drawing_annotations_test::run_navigation(fixture_args.into_iter())?
            }
            Fixture::DrawingOutput => crate::native_drawing_test::run(fixture_args.into_iter())?,
            Fixture::Hole => crate::native_drawing_hole_test::run(fixture_args.into_iter())?,
            Fixture::Scripts => crate::native_lessons_test::run(fixture_args.into_iter())?,
            Fixture::Mechanism => crate::native_mechanism_test::run(fixture_args.into_iter())?,
            Fixture::Cam => crate::native_cam_test::run(fixture_args.into_iter())?,
            Fixture::Chamfer | Fixture::Cloud | Fixture::Centers => {
                crate::native_drawing_annotations_test::run_authoring(fixture_args.into_iter())?
            }
            Fixture::CamGeometry => crate::native_cam_geometry_test::run(fixture_args.into_iter())?,
        }
        ensure!(
            host.is_running()?,
            "Owned native host exited during input validation"
        );
        let evidence = match fixture {
            Fixture::Drawing => "evidence/native-drawing-navigation.json",
            Fixture::DrawingOutput => "evidence/native-drawing.json",
            Fixture::Hole => "evidence/native-drawing-hole.json",
            Fixture::Scripts => "evidence/native-lessons.json",
            Fixture::Mechanism => "evidence/native-mechanism.json",
            Fixture::Cam => "evidence/native-cam.json",
            Fixture::Chamfer | Fixture::Cloud | Fixture::Centers => {
                "evidence/native-drawing-authoring.json"
            }
            Fixture::CamGeometry => "evidence/native-cam-geometry.json",
        };
        Ok::<_, anyhow::Error>(
            json!({"status":"passed","platform":std::env::consts::OS,"pid":host.process_id(),
            "evidence":evidence,
            "annotation_os_input":authoring,"cam_row_os_input":fixture == Fixture::Cam,
            "cam_wcs_os_input":fixture == Fixture::Cam,"chamfer_os_input":fixture == Fixture::Chamfer,
            "cloud_os_input":fixture == Fixture::Cloud,
            "drawing_output":fixture == Fixture::DrawingOutput,
            "center_authoring":fixture == Fixture::Centers,"center_os_input":fixture == Fixture::Centers,
            "hole_authoring":fixture == Fixture::Hole,
            "hole_os_input":fixture == Fixture::Hole && std::env::var("LIMO_CAD_NATIVE_HOLE_INPUT").as_deref() == Ok("1"),
            "scripts_workflow":fixture == Fixture::Scripts,"scripts_os_input":false,
            "mechanism_os_input":fixture == Fixture::Mechanism,
            "drawing_save_dialog_os_input":false,
            "cam_geometry_os_input":fixture == Fixture::CamGeometry,
            "not_proven":["Touchpad pinch","Monitor DPI transition","Wayland","macOS paper gestures"]}),
        )
    })();
    let report = match &result {
        Ok(report) => report.clone(),
        Err(error) => {
            json!({"status":"failed","error":format!("{error:#}"),"platform":std::env::consts::OS})
        }
    };
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    result.map(|_| ())
}
