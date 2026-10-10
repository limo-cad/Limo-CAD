//! Read-only ownership checks for disposable X11 qualification.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct OwnedCommand(Child);
impl Drop for OwnedCommand {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn proc_parent(stat: &str) -> Result<u32> {
    stat.rsplit_once(')')
        .and_then(|(_, tail)| tail.split_whitespace().nth(1))
        .context("Missing process parent")?
        .parse()
        .context("Invalid process parent")
}

fn bounded_file(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(16_385).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 16_384, "Process record exceeds its bound");
    Ok(bytes)
}

pub(crate) fn verify_private_display() -> Result<u32> {
    ensure!(
        cfg!(target_os = "linux"),
        "An owned Linux Xvfb display is required"
    );
    let display = std::env::var("DISPLAY").context("DISPLAY is missing")?;
    let number = regex::Regex::new(r"^:(\d+)(?:\.\d+)?$")?
        .captures(&display)
        .context("A local private Xvfb display is required")?[1]
        .parse::<u32>()?;
    let pid = String::from_utf8(bounded_file(format!("/tmp/.X{number}-lock"))?)?
        .trim()
        .parse::<u32>()?;
    ensure!(pid > 1, "Invalid Xvfb PID");
    let command = bounded_file(format!("/proc/{pid}/cmdline"))?;
    let executable =
        std::str::from_utf8(command.split(|byte| *byte == 0).next().unwrap_or_default())?;
    ensure!(
        Path::new(executable)
            .file_name()
            .is_some_and(|name| name == "Xvfb"),
        "The display is not Xvfb"
    );
    let parent = |pid| -> Result<u32> {
        proc_parent(&String::from_utf8(bounded_file(format!(
            "/proc/{pid}/stat"
        ))?)?)
    };
    let server_parent = parent(pid)?;
    let mut ancestors = HashSet::new();
    let mut ancestor = std::process::id();
    while ancestor > 1 && ancestors.insert(ancestor) {
        ensure!(ancestors.len() <= 256, "Process ancestry exceeds its bound");
        ancestor = parent(ancestor)?;
    }
    ensure!(
        ancestors.contains(&server_parent),
        "Xvfb does not belong to this fixture's process tree"
    );
    Ok(pid)
}

pub(crate) fn output(command: &mut Command, budget: Duration) -> Result<String> {
    ensure!(
        !budget.is_zero(),
        "Read-only command has no remaining budget"
    );
    let deadline = Instant::now() + budget;
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = OwnedCommand(
        command
            .stdin(Stdio::null())
            .stdout(stdout.try_clone()?)
            .stderr(stderr.try_clone()?)
            .spawn()
            .with_context(|| format!("Start {command:?}"))?,
    );
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            anyhow::bail!("Read-only command exceeded its budget: {command:?}");
        }
        thread::sleep(Duration::from_millis(10));
    };
    fn read(file: &mut fs::File) -> Result<String> {
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(65_537).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= 65_536,
            "Read-only command output exceeds its bound"
        );
        Ok(String::from_utf8(bytes)?.trim().to_owned())
    }
    ensure!(
        status.success(),
        "{command:?} failed: {}",
        read(&mut stderr)?
    );
    read(&mut stdout)
}

pub(crate) fn observe_focus(pid: u32, deadline: Instant) -> Result<Value> {
    ensure!(pid > 1, "An owned process ID is required");
    let xvfb = verify_private_display()?;
    observe_focus_read(pid, xvfb, |command: &mut Command| {
        let budget = deadline.saturating_duration_since(Instant::now());
        ensure!(
            !budget.is_zero(),
            "Owned focus observation exceeded its budget"
        );
        output(command, budget)
    })
}

fn observe_focus_read(
    pid: u32,
    xvfb: u32,
    mut read: impl FnMut(&mut Command) -> Result<String>,
) -> Result<Value> {
    let property = read(Command::new("xprop").args(["-root", "_NET_ACTIVE_WINDOW"]))?;
    let window =
        regex::Regex::new(r"^_NET_ACTIVE_WINDOW\(WINDOW\): window id # (0x[0-9a-fA-F]+)$")?
            .captures(&property)
            .context("The window manager did not publish an active window")?[1]
            .trim_start_matches("0x")
            .to_owned();
    let active = u64::from_str_radix(&window, 16)?;
    let (active_pid, focused, focused_pid) = if active == 0 {
        (None, None, None)
    } else {
        let active_pid = read(Command::new("xdotool").args(["getwindowpid", &active.to_string()]))?
            .parse::<u32>()?;
        let focused = read(Command::new("xdotool").arg("getwindowfocus"))?.parse::<u64>()?;
        let focused_pid =
            read(Command::new("xdotool").args(["getwindowpid", &focused.to_string()]))?
                .parse::<u32>()?;
        (Some(active_pid), Some(focused), Some(focused_pid))
    };
    Ok(
        json!({"source":"X11 window manager and input focus","xvfb_pid":xvfb,"pid":pid,
        "active_window":active,"active_pid":active_pid,"focused_window":focused,
        "focused_pid":focused_pid,"owned_focus":active_pid==Some(pid)&&focused_pid==Some(pid)}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proc_parent_handles_spaces_and_parentheses_in_process_names() {
        assert_eq!(proc_parent("10 (fixture (owned)) S 42 0 0").unwrap(), 42);
        assert!(proc_parent("10 (fixture) S invalid").is_err());
        assert!(proc_parent("10 (fixture)").is_err());
    }

    #[test]
    fn focus_requires_both_os_window_pids_and_never_queries_window_zero() {
        for (active_pid, focused_pid, expected) in
            [(123, 123, true), (456, 123, false), (123, 456, false)]
        {
            let mut output = std::collections::VecDeque::from([
                "_NET_ACTIVE_WINDOW(WINDOW): window id # 0x10".to_owned(),
                active_pid.to_string(),
                "17".to_owned(),
                focused_pid.to_string(),
            ]);
            let mut calls = Vec::new();
            let observed = observe_focus_read(123, 99, |command| {
                calls.push((
                    command.get_program().to_string_lossy().into_owned(),
                    command
                        .get_args()
                        .map(|arg| arg.to_string_lossy().into_owned())
                        .collect::<Vec<_>>(),
                ));
                Ok(output.pop_front().expect("unexpected OS query"))
            })
            .unwrap();
            assert_eq!(observed["owned_focus"], expected);
            assert_eq!(
                calls,
                [
                    (
                        "xprop".into(),
                        vec!["-root".into(), "_NET_ACTIVE_WINDOW".into()]
                    ),
                    ("xdotool".into(), vec!["getwindowpid".into(), "16".into()]),
                    ("xdotool".into(), vec!["getwindowfocus".into()]),
                    ("xdotool".into(), vec!["getwindowpid".into(), "17".into()]),
                ]
            );
        }
        let mut queries = 0;
        let observed = observe_focus_read(123, 99, |_| {
            queries += 1;
            Ok("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x0".into())
        })
        .unwrap();
        assert_eq!(observed["owned_focus"], false);
        assert_eq!(queries, 1);
        assert!(observe_focus_read(123, 99, |_| Ok("invalid property".into())).is_err());
    }
}
