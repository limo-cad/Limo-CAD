//! Only windows launched by this scenario may receive native close or cleanup.
use super::*;
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};

struct OwnedWindow {
    pid: Pid,
    start: u64,
    session: Option<String>,
    system: System,
}
impl OwnedWindow {
    fn capture(pid: u32, parent: u32, session: Option<String>) -> Result<Self> {
        ensure!(pid > 0 && pid != parent, "Invalid owned desktop PID");
        let pid = Pid::from_u32(pid);
        let parent = Pid::from_u32(parent);
        let mut system = System::new();
        refresh(&mut system, pid);
        let process = system
            .process(pid)
            .context("Launched desktop process is absent")?;
        ensure!(
            process.parent() == Some(parent),
            "Launched PID is not a child of this MCP transport"
        );
        let start = process.start_time();
        Ok(Self {
            pid,
            start,
            session,
            system,
        })
    }
    fn alive(&mut self) -> bool {
        refresh(&mut self.system, self.pid);
        self.system.process(self.pid).is_some_and(|p| {
            p.start_time() == self.start
                && !matches!(p.status(), ProcessStatus::Zombie | ProcessStatus::Dead)
        })
    }
    fn exited(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        while self.alive() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        ensure!(!self.alive(), "Owned CAD process {} did not exit", self.pid);
        Ok(())
    }
}
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        if self.alive() {
            if let Some(process) = self.system.process(self.pid) {
                let _ = process.kill();
            }
        }
    }
}
fn refresh(system: &mut System, pid: Pid) {
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
}
fn launch(s: &mut Scenario) -> Result<OwnedWindow> {
    let launch=s.call("cad_interface",json!({"action":"launch","executable":s.options.get("--desktop").context("Use --desktop PATH")?}))?;
    let pid = u32::try_from(
        launch["pid"]
            .as_u64()
            .context("Launch must identify its process")?,
    )?;
    let session = launch["session_id"].as_str().map(str::to_owned);
    let owned = OwnedWindow::capture(pid, s.client.process_id(), session.clone())?;
    ensure!(
        launch["status"] == "ready",
        "Owned desktop did not become ready: {launch}"
    );
    s.session = session;
    Ok(owned)
}
fn click(s: &mut Scenario, owned: &mut OwnedWindow, label: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        ensure!(
            owned.alive(),
            "Owned desktop exited while waiting for {label}"
        );
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(!remaining.is_zero(), "Timed out waiting for {label}");
        let raw=s.client.rpc_with_timeout("tools/call",json!({"name":"cad_interface","arguments":{"action":"inspect","session_id":owned.session}}),remaining)?;
        let state = Client::decode_call_result("cad_interface", raw)?;
        ensure!(state["status"] == "applied", "Inspection failed: {state}");
        let found: Vec<_> = controls_in(&state)
            .filter(|c| c["label"] == label && c["disabled"] != true)
            .collect();
        ensure!(found.len() <= 1, "Ambiguous enabled {label}");
        if let Some(row) = found.first() {
            ensure!(owned.alive(), "Owned desktop exited before click");
            let remaining = deadline.saturating_duration_since(Instant::now());
            ensure!(!remaining.is_zero(), "Timed out before {label} click");
            let raw=s.client.rpc_with_timeout("tools/call",json!({"name":"cad_interface","arguments":{"action":"click","target":row["id"],"session_id":owned.session}}),remaining)?;
            let result = Client::decode_call_result("cad_interface", raw)?;
            ensure!(result["status"] == "applied", "Click failed: {result}");
            s.report["calls"].as_array_mut().unwrap().push(json!({
                "name":"cad_interface","label":label,"status":result["status"],
                "awaiting_input":result["awaiting_input"],"active_session_id":result["active_session_id"]
            }));
            if let Some(session) = result["active_session_id"].as_str() {
                s.session = Some(session.into());
                owned.session = Some(session.into());
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50).min(remaining));
    }
}
fn dirty(s: &mut Scenario) -> Result<()> {
    s.call(
        "sketch_begin",
        json!({"plane":{"type":"origin_plane","plane":"xy"}}),
    )?;
    s.call("sketch_add_rectangle_locked",json!({"mode":"two_point","anchor":{"x":0,"y":0},"corner_hint":{"x":20,"y":10},"width_mm":20,"height_mm":10,"ctrl_held":true}))?;
    s.call("sketch_finish", json!({}))?;
    Ok(())
}
fn menu_exit(s: &mut Scenario, owned: &mut OwnedWindow) -> Result<()> {
    click(s, owned, "File")?;
    click(s, owned, "Exit")
}
fn cleanup(s: &mut Scenario, owned: &mut OwnedWindow) -> Result<()> {
    if !owned.alive() {
        return Ok(());
    }
    let result = (|| -> Result<()> {
        let session = owned
            .session
            .clone()
            .context("Owned desktop has no cleanup session")?;
        s.client.rpc_with_timeout("tools/call",json!({"name":"cad_interface","arguments":{"action":"window","mode":"close","session_id":session}}),Duration::from_secs(2)).and_then(|raw|Client::decode_call_result("cad_interface",raw))?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while owned.alive() && Instant::now() < deadline {
            let raw=s.client.rpc_with_timeout("tools/call",json!({"name":"cad_interface","arguments":{"action":"inspect","session_id":session}}),deadline.saturating_duration_since(Instant::now()))?;
            let state = Client::decode_call_result("cad_interface", raw)?;
            let discard: Vec<_> = controls_in(&state)
                .filter(|r| {
                    ["Don't Save", "Discard changes and close"]
                        .iter()
                        .any(|label| r["label"] == *label)
                        && r["disabled"] != true
                })
                .collect();
            ensure!(discard.len() <= 1, "Ambiguous owned discard prompt");
            if let Some(row) = discard.first() {
                let raw=s.client.rpc_with_timeout("tools/call",json!({"name":"cad_interface","arguments":{"action":"click","target":row["id"],"session_id":session}}),Duration::from_secs(2))?;
                Client::decode_call_result("cad_interface", raw)?;
                return owned.exited(Duration::from_secs(2));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        owned.exited(Duration::from_millis(50))
    })();
    if !owned.alive() {
        return Ok(());
    }
    s.report["cases"].as_array_mut().unwrap().push(json!({"pid":owned.pid.as_u32(),"cleanup":true,"forced":true,"passed":false,"error":result.as_ref().err().map(|e|format!("{e:#}"))}));
    bail!("Guarded cleanup failed for owned process {}", owned.pid)
}
pub(super) fn run(s: &mut Scenario) -> Result<()> {
    ensure!(
        s.options.contains_key("--desktop") && s.session.is_none(),
        "Exit scenario requires --desktop PATH and no existing --session"
    );
    let mut cases = vec![
        "mcp-clean",
        "mcp-background",
        "menu-clean",
        "cancel-discard",
        "save-exit",
    ];
    if cfg!(windows) {
        cases.push("native-close");
    }
    if let Some(selected) = s.options.get("--case") {
        ensure!(
            cases.contains(&selected.as_str()),
            "Unknown exit case {selected}"
        );
        cases.retain(|case| *case == selected);
    }
    for test in cases {
        let mut owned = launch(s)?;
        let pid = owned.pid.as_u32();
        let result = (|| -> Result<()> {
            match test {
                "mcp-clean" => {
                    s.ui(json!({"action":"window","mode":"close"}))?;
                }
                "mcp-background" => {
                    s.ui(json!({"action":"window","mode":"background"}))?;
                    s.ui(json!({"action":"window","mode":"close"}))?;
                }
                "menu-clean" => menu_exit(s, &mut owned)?,
                "cancel-discard" => {
                    dirty(s)?;
                    let before = s.call("cad_project_model", json!({}))?;
                    s.ui(json!({"action":"window","mode":"close"}))?;
                    click(s, &mut owned, "Keep working")?;
                    ensure!(
                        owned.alive() && s.call("cad_project_model", json!({}))? == before,
                        "Cancel failed to preserve work"
                    );
                    menu_exit(s, &mut owned)?;
                    click(s, &mut owned, "Discard changes and close")?;
                }
                "save-exit" => {
                    let directory = tempfile::Builder::new()
                        .prefix("limo-cad-exit-")
                        .tempdir()?;
                    let path = directory.path().join("saved.limo");
                    s.ui(json!({"action":"file","command":"save","path":path}))?;
                    dirty(s)?;
                    let expected = s.model()?;
                    s.ui(json!({"action":"window","mode":"close"}))?;
                    click(s, &mut owned, "Save all and close")?;
                    owned.exited(Duration::from_secs(10))?;
                    let mut reopened = launch(s)?;
                    let restored = (|| -> Result<()> {
                        s.ui(json!({"action":"file","command":"open","path":path}))?;
                        reopened.session = s.session.clone();
                        ensure!(
                            s.model()? == expected,
                            "Save on exit did not preserve the complete model"
                        );
                        s.ui(json!({"action":"window","mode":"close"}))?;
                        reopened.exited(Duration::from_secs(10))
                    })();
                    let clean = cleanup(s, &mut reopened);
                    restored?;
                    clean?;
                }
                "native-close" => native_close(&mut owned)?,
                _ => unreachable!(),
            }
            owned.exited(Duration::from_secs(10))
        })();
        let clean = cleanup(s, &mut owned);
        s.report["cases"].as_array_mut().unwrap().push(json!({"test":test,"pid":pid,"passed":result.is_ok() && clean.is_ok(),"error":result.as_ref().err().map(|e|format!("{e:#}"))}));
        result?;
        clean?;
        s.check(test);
    }
    Ok(())
}

#[cfg(not(windows))]
fn native_close(_: &mut OwnedWindow) -> Result<()> {
    bail!("Native WM_CLOSE is a Windows scenario")
}
#[cfg(windows)]
fn native_close(owned: &mut OwnedWindow) -> Result<()> {
    use windows::core::BOOL;
    use windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowLongPtrW, GetWindowThreadProcessId, IsWindowVisible,
            PostMessageW, GWLP_HWNDPARENT, GWL_EXSTYLE, WM_CLOSE, WS_EX_TOOLWINDOW,
        },
    };
    struct Search {
        pid: u32,
        windows: Vec<HWND>,
    }
    unsafe extern "system" fn visit(window: HWND, data: LPARAM) -> BOOL {
        let search = unsafe { &mut *(data.0 as *mut Search) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, Some(&mut pid));
        }
        if pid == search.pid
            && unsafe { IsWindowVisible(window) }.as_bool()
            && unsafe { GetWindowLongPtrW(window, GWLP_HWNDPARENT) } == 0
            && unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } & WS_EX_TOOLWINDOW.0 as isize == 0
        {
            search.windows.push(window);
        }
        BOOL(1)
    }
    ensure!(owned.alive(), "Owned desktop exited before WM_CLOSE");
    let mut search = Search {
        pid: owned.pid.as_u32(),
        windows: vec![],
    };

    unsafe {
        EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize))?;
    }
    ensure!(
        search.windows.len() == 1,
        "Expected one owned visible CAD window, found {}",
        search.windows.len()
    );
    ensure!(
        owned.alive(),
        "Owned process identity changed before WM_CLOSE"
    );
    unsafe {
        PostMessageW(Some(search.windows[0]), WM_CLOSE, WPARAM(0), LPARAM(0))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_refuses_a_process_that_the_transport_did_not_launch() {
        assert!(OwnedWindow::capture(std::process::id(), std::process::id() + 1, None).is_err());
    }
}
