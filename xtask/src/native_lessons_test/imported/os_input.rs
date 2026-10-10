//! Desktop-input lessons drive the real script chooser. The default harness
//! keeps the typed path so a headless run never opens a modal.
use crate::replay::Client;
#[cfg(not(windows))]
use anyhow::bail;
use anyhow::{ensure, Context, Result};
use std::path::Path;
#[cfg(not(windows))]
use std::{
    io::Write,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) fn enabled() -> bool {
    std::env::var("LIMO_CAD_NATIVE_SCRIPT_INPUT").as_deref() == Ok("1")
}

pub(super) fn complete_dialog(
    client: &mut Client,
    out: &Path,
    title: &str,
    path: &str,
) -> Result<()> {
    #[cfg(windows)]
    {
        crate::native_platform_test::require_windows_harness()?;
        let inspected = client.call("cad_interface", serde_json::json!({"action":"inspect"}))?;
        let pid = u32::try_from(
            inspected["native_window"]["pid"]
                .as_u64()
                .context("The current build does not publish its native CAD owner; enable native-computer-control")?,
        )?;
        if let Some(expected) = std::env::var_os("LIMO_CAD_NATIVE_OWNED_PID") {
            ensure!(
                expected
                    .to_str()
                    .context("LIMO_CAD_NATIVE_OWNED_PID")?
                    .parse::<u32>()?
                    == pid,
                "Script dialog inspection belongs to a different native host"
            );
        }
        crate::native_platform_test::Driver::new(pid, out)?.complete_dialog(title, path)
    }
    #[cfg(not(windows))]
    {
        let _ = (client, out);
        std::env::set_var("LIMO_CAD_SCRIPT_DIALOG_TITLE", title);
        invoke("script-dialog", Some(path), Duration::from_secs(30))
    }
}

#[cfg(not(windows))]
fn invoke(operation: &str, input: Option<&str>, timeout: Duration) -> Result<()> {
    let pid = std::env::var("LIMO_CAD_NATIVE_OWNED_PID")
        .context("LIMO_CAD_NATIVE_OWNED_PID")?
        .parse::<u32>()
        .context("LIMO_CAD_NATIVE_OWNED_PID")?;
    let mut command = helper(pid, operation)?;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().context("Start script chooser helper")?;
    if let Some(input) = input {
        child
            .stdin
            .take()
            .context("script chooser helper stdin")?
            .write_all(input.as_bytes())?;
    } else {
        drop(child.stdin.take());
    }
    let deadline = Instant::now() + timeout;
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Script chooser helper {operation} exceeded {timeout:?}");
        }
        thread::sleep(Duration::from_millis(25));
    }
    let output = child.wait_with_output()?;
    ensure!(
        output.status.success(),
        "Script chooser helper {operation} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[cfg(not(windows))]
fn helper(pid: u32, operation: &str) -> Result<Command> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("platform");
    #[cfg(target_os = "linux")]
    {
        let mut command = Command::new("bash");
        command
            .arg(root.join("native-input-linux.sh"))
            .arg(pid.to_string())
            .arg(operation);
        Ok(command)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (root, pid, operation);
        bail!("Script OS chooser input is not built for this OS");
    }
}
