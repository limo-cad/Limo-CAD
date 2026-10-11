//! One focus-only click on an observed, exact native caption. Ordinary client
//! input retains the foreground guard. Native checks are cooperative snapshots.

use std::time::{Duration, Instant};

use super::super::caption::{Geometry, Hit};
use super::*;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_LBUTTON;

const PROOF_DEADLINE: Duration = Duration::from_secs(1);
const HIT_TIMEOUT_MS: u32 = 50;
// TITLEBARINFO.rgstate values documented by Winuser/MSAA; no text is queried.
const TITLEBAR_UNAVAILABLE: u32 = 0x0000_0001 | 0x0000_8000 | 0x0001_0000;
const MENU_FLAGS: u32 = GUI_INMENUMODE | GUI_POPUPMENUMODE | GUI_SYSTEMMENUMODE;

pub(super) struct Proof {
    hwnd: usize,
    pid: u32,
    geometry: Geometry,
    point: [i32; 2],
    sampled_ms: u64,
}

impl Proof {
    pub(super) fn snapshot(&self) -> Value {
        json!({"available":true,"target_hwnd":self.hwnd,"root_hwnd":self.hwnd,"pid":self.pid,
            "point":self.point,"coordinate_space":"physical_screen_pixels",
            "window_screen_rect":self.geometry.window,"client_screen_rect":self.geometry.client,
            "titlebar_screen_rect":self.geometry.title,"dpi":self.geometry.dpi,
            "sampled_ms":self.sampled_ms,"hit_test":"HTCAPTION",
            "scope":"Focus-only native caption proof; not ordinary client coordinates or a guarantee against later window/input changes."})
    }
}

fn geometry(hwnd: HWND) -> Result<Geometry, String> {
    let _dpi = DpiGuard::enter()?;
    if unsafe { IsWindowVisible(hwnd) } == 0
        || unsafe { IsWindowEnabled(hwnd) } == 0
        || unsafe { IsIconic(hwnd) } != 0
        || (unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32 & WS_CAPTION) != WS_CAPTION
        || (unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_NOACTIVATE) != 0
    {
        return Err("CAD has no enabled, visible, nonminimized native caption".into());
    }
    if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 {
        return Err("Native caption recovery does not qualify swapped mouse buttons".into());
    }
    let mut window = RECT::default();
    let mut title = TITLEBARINFO {
        cbSize: size_of::<TITLEBARINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetWindowRect(hwnd, &mut window) } == 0
        || unsafe { GetTitleBarInfo(hwnd, &mut title) } == 0
        || title.rgstate[0] & TITLEBAR_UNAVAILABLE != 0
    {
        return Err("Cannot qualify the native CAD title bar".into());
    }
    let client = client_bounds(hwnd)?;
    let geometry = Geometry {
        window: [window.left, window.top, window.right, window.bottom],
        client: [
            client[0],
            client[1],
            client[0]
                .checked_add(client[2])
                .ok_or("Client X overflow")?,
            client[1]
                .checked_add(client[3])
                .ok_or("Client Y overflow")?,
        ],
        title: [
            title.rcTitleBar.left,
            title.rcTitleBar.top,
            title.rcTitleBar.right,
            title.rcTitleBar.bottom,
        ],
        dpi: unsafe { GetDpiForWindow(hwnd) },
    };
    if geometry.dpi == 0 {
        return Err("Cannot qualify native caption DPI".into());
    }
    Ok(geometry)
}

fn hit(hwnd: HWND, pid: u32, point: [i32; 2], deadline: Instant) -> Result<Hit, String> {
    let _dpi = DpiGuard::enter()?;
    let mut actual_pid = 0;
    let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut actual_pid) };
    if thread == 0 || thread == unsafe { GetCurrentThreadId() } || actual_pid != pid {
        return Err("Cannot use a bounded cross-thread caption hit test".into());
    }
    let x = i16::try_from(point[0]).map_err(|_| "Caption X exceeds native hit-test range")?;
    let y = i16::try_from(point[1]).map_err(|_| "Caption Y exceeds native hit-test range")?;
    let packed = (u32::from(x as u16) | (u32::from(y as u16) << 16)) as i32 as LPARAM;
    let at = POINT {
        x: point[0],
        y: point[1],
    };
    if unsafe { WindowFromPoint(at) } != hwnd {
        return Err("Native CAD caption is covered by another window".into());
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err("Native caption proof exceeded its bounded deadline".into());
    }
    let timeout = remaining.as_millis().min(u128::from(HIT_TIMEOUT_MS)).max(1) as u32;
    let mut result = 0;
    if unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_NCHITTEST,
            0,
            packed,
            SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
            timeout,
            &mut result,
        )
    } == 0
        || Instant::now() >= deadline
    {
        return Err("Native caption hit test failed or timed out".into());
    }
    let window = unsafe { WindowFromPoint(at) };
    let root = unsafe { GetAncestor(window, GA_ROOT) };
    let mut hit_pid = 0;
    unsafe {
        GetWindowThreadProcessId(window, &mut hit_pid);
    }
    Ok(Hit {
        window: window as usize,
        root: root as usize,
        pid: hit_pid,
        code: result,
    })
}

fn patch(proof: &Proof, hwnd: HWND, pid: u32, deadline: Instant) -> Result<(), String> {
    if geometry(hwnd)? != proof.geometry {
        return Err("Observed native caption/window geometry changed; observe again".into());
    }
    let points = proof
        .geometry
        .patch(proof.point)
        .ok_or("Native caption has no qualified nonclient margin")?;
    let mut hits = [Hit::default(); 5];
    for (index, point) in points.into_iter().enumerate() {
        hits[index] = hit(hwnd, pid, point, deadline)?;
    }
    if !proof
        .geometry
        .qualifies(proof.point, &hits, hwnd as usize, pid)
    {
        return Err("Pointer is not exclusively over the exact owned native HTCAPTION".into());
    }
    if geometry(hwnd)? != proof.geometry {
        return Err("Native caption moved while hit testing; observe again".into());
    }
    Ok(())
}

pub(super) fn observe(hwnd: HWND, pid: u32) -> Result<Proof, String> {
    let deadline = Instant::now() + PROOF_DEADLINE;
    let geometry = geometry(hwnd)?;
    for point in geometry.candidates() {
        let proof = Proof {
            hwnd: hwnd as usize,
            pid,
            geometry,
            point,
            sampled_ms: session::now_ms(),
        };
        if patch(&proof, hwnd, pid, deadline).is_ok() {
            return Ok(proof);
        }
        if Instant::now() >= deadline {
            break;
        }
    }
    Err("No exposed native caption point passed exact ownership and HTCAPTION checks".into())
}

fn scope(observed: &Observation, target: Target, proof: &Proof) -> Result<(), String> {
    owner_scope(observed, target, proof)?;
    main_scope(observed, target, proof.geometry)
}

fn main_scope(observed: &Observation, target: Target, expected: Geometry) -> Result<(), String> {
    observed.process.verify()?;
    let session_id = observed.owner["session_id"]
        .as_str()
        .ok_or("Observation has no session")?;
    if observed.native_dialog
        || observed.main_hwnd != target.main as usize
        || observed.hwnd != target.window as usize
        || observed.bounds.is_none()
        || observed.layout.is_none()
        || target.native_dialog
        || target.main != target.window
        || session::now_ms() > observed.expires_ms
        || session::computer_control_owner(session_id)? != observed.owner
    {
        return Err("Native caption focus requires the unchanged presented main CAD owner".into());
    }
    let current = inspect_desktop(session_id, true)?;
    let main = main_from_inspection(&observed.owner, &current)?;
    if target_window(&observed.owner, main)? != target
        || Some(client_bounds(target.window)?) != observed.bounds
        || observed.layout.as_ref() != Some(&layout(&current))
        || geometry(target.window)? != expected
        || session::computer_control_owner(session_id)? != observed.owner
        || session::now_ms() > observed.expires_ms
    {
        return Err("CAD owner, modal, layout or native caption changed; observe again".into());
    }
    Ok(())
}

// Exposure is not activation or input. Windows may still refuse the Z-order
// request; the consumed observation never qualifies a click after this change.
fn expose(observed: &Observation, target: Target) -> Result<Value, String> {
    let expected = geometry(target.window)?;
    let pid = observed.owner["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or("Owner has no PID")?;
    main_scope(observed, target, expected)?;
    let foreground = input_ownership(target.window, pid, false, std::ptr::null_mut())?;
    main_scope(observed, target, expected)?;
    if input_ownership(target.window, pid, false, std::ptr::null_mut())? != foreground {
        return Err("Foreground changed before CAD exposure; observe again".into());
    }
    main_scope(observed, target, expected)?;
    if unsafe { GetForegroundWindow() } != foreground {
        return Err("Foreground changed immediately before CAD exposure; observe again".into());
    }
    // ASYNC avoids waiting indefinitely on the owning GUI thread. NOACTIVATE
    // keeps foreground ownership unchanged; never use TOPMOST or attach queues.
    // https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-setwindowpos
    let requested = unsafe {
        SetWindowPos(
            target.window,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER | SWP_ASYNCWINDOWPOS,
        ) != 0
    };
    Ok(exposure_response(&observed.owner, requested))
}

fn exposure_response(owner: &Value, requested: bool) -> Value {
    json!({"status":"not_focused","action":"focus","method":"nonactivating_window_exposure",
        "owner":owner,"exposure_requested":requested,"input_sent":false,
        "foreground_verified":false,"observation_consumed":true,
        "hint":"A best-effort nonactivating CAD exposure request was attempted. It may be refused or still queued. Observe again; only a fresh qualified exposed native caption can permit the existing focus click. Ordinary background input remains refused."})
}

fn owner_scope(observed: &Observation, target: Target, proof: &Proof) -> Result<(), String> {
    observed.process.verify()?;
    let session_id = observed.owner["session_id"]
        .as_str()
        .ok_or("Observation has no session")?;
    if proof.hwnd != target.window as usize
        || proof.pid as u64 != observed.owner["pid"].as_u64().unwrap_or(0)
        || observed.main_hwnd != target.main as usize
        || observed.hwnd != target.window as usize
        || session::now_ms() > observed.expires_ms
        || session::computer_control_owner(session_id)? != observed.owner
        || target_window(&observed.owner, target.main as usize)? != target
    {
        return Err("Observed native caption process/document/window owner changed".into());
    }
    Ok(())
}

fn gui_state(hwnd: HWND, pid: Option<u32>) -> Result<GUITHREADINFO, String> {
    let mut actual_pid = 0;
    let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut actual_pid) };
    let mut info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    if hwnd.is_null()
        || thread == 0
        || actual_pid == 0
        || pid.is_some_and(|expected| expected != actual_pid)
        || unsafe { GetGUIThreadInfo(thread, &mut info) } == 0
    {
        return Err("Cannot qualify current native caption input ownership".into());
    }
    Ok(info)
}

fn no_menu(info: &GUITHREADINFO) -> Result<(), String> {
    if !info.hwndMenuOwner.is_null() || info.flags & MENU_FLAGS != 0 {
        return Err("Native caption activation is blocked by an active menu".into());
    }
    Ok(())
}

fn input_ownership(
    hwnd: HWND,
    pid: u32,
    releasing: bool,
    initial_foreground: HWND,
) -> Result<HWND, String> {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null()
        || (!releasing && foreground == hwnd)
        || (releasing && foreground != hwnd && foreground != initial_foreground)
    {
        return Err("Foreground changed before native caption input; observe again".into());
    }
    let own = gui_state(hwnd, Some(pid))?;
    no_menu(&own)?;
    if (!own.hwndCapture.is_null() && (!releasing || own.hwndCapture != hwnd))
        || (!own.hwndMoveSize.is_null() && (!releasing || own.hwndMoveSize != hwnd))
        || (own.flags & GUI_INMOVESIZE != 0 && (!releasing || own.hwndMoveSize != hwnd))
        || (!own.hwndActive.is_null() && own.hwndActive != hwnd)
    {
        return Err("CAD has a pre-existing native capture or move/size gesture".into());
    }
    if !own.hwndFocus.is_null() {
        let mut focus_pid = 0;
        unsafe {
            GetWindowThreadProcessId(own.hwndFocus, &mut focus_pid);
        }
        if focus_pid != pid || unsafe { GetAncestor(own.hwndFocus, GA_ROOT) } != hwnd {
            return Err("CAD native focus belongs to another window".into());
        }
    }
    let active = gui_state(foreground, None)?;
    no_menu(&active)?;
    if active.hwndActive != foreground
        || (foreground != hwnd
            && (!active.hwndCapture.is_null()
                || !active.hwndMoveSize.is_null()
                || active.flags & GUI_INMOVESIZE != 0))
        || unsafe { GetForegroundWindow() } != foreground
    {
        return Err("Foreground input is captured, moving or changed during qualification".into());
    }
    for key in 1..=254 {
        // Only the matching release of this attempted caption press may see Left held.
        if !(releasing && key == i32::from(VK_LBUTTON)) && unsafe { GetAsyncKeyState(key) } < 0 {
            return Err("A physical key/button is held; native caption focus was refused".into());
        }
    }
    Ok(foreground)
}

fn cursor(point: [i32; 2]) -> Result<(), String> {
    let _dpi = DpiGuard::enter()?;
    let mut current = POINT::default();
    if unsafe { GetCursorPos(&mut current) } == 0 || [current.x, current.y] != point {
        return Err("Cursor left the qualified native caption point".into());
    }
    Ok(())
}

fn denied(observed: &Observation, reason: String, accepted: bool, acknowledged: bool) -> String {
    json!({"code":"computer_control_foreground_denied","message":reason,
        "activation_accepted":accepted,"activation_acknowledged":acknowledged,
        "expected_hwnd":observed.hwnd,"foreground_hwnd":unsafe { GetForegroundWindow() } as usize,
        "input_sent":false,"observation_consumed":true,
        "hint":"No proven caption click was inserted. Expose/focus the owned CAD window and observe again; no client click or activation trick is attempted."}).to_string()
}

fn incomplete(
    observed: &Observation,
    proof: &Proof,
    driver: &mut InputDriver,
    completed: usize,
    attempted: usize,
    failed: &str,
    error: String,
) -> Value {
    let cleanup = driver.release_all();
    json!({"status":"input_incomplete","action":"focus","method":"verified_native_caption_click",
        "backend":"verified_win32_cursor_and_enigo","owner":observed.owner,
        "activation_accepted":false,"activation_acknowledged":true,
        "completed_primitives":completed,"attempted_primitives":attempted,"planned_primitives":3,
        "failed_primitive":failed,"error":error,"cleanup_errors":cleanup,
        "input_may_have_been_inserted":true,"native_caption_focus":proof.snapshot(),
        "foreground_hwnd":unsafe { GetForegroundWindow() } as usize,"observation_consumed":true,
        "hint":"Caption input stopped; one release was attempted for any owned attempted press. Observe the actual window/result before further input; do not blindly retry."})
}

pub(super) fn recover(
    observed: &Observation,
    target: Target,
    accepted: bool,
    acknowledged: bool,
) -> Result<Value, String> {
    if accepted || !acknowledged || observed.native_dialog {
        return Err(denied(
            observed,
            "No refused, acknowledged main-window activation qualifies caption recovery".into(),
            accepted,
            acknowledged,
        ));
    }
    let Some(proof) = observed.caption.as_ref() else {
        return expose(observed, target)
            .map_err(|error| denied(observed, error, accepted, acknowledged));
    };
    let pid = observed.owner["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or("Owner has no PID")?;
    let mut driver =
        InputDriver::new().map_err(|error| denied(observed, error, accepted, acknowledged))?;
    let mut completed = 0;
    let mut attempted = 0;
    let mut initial_foreground = std::ptr::null_mut();
    for step in [
        Step::Move(proof.point),
        Step::Button(Button::Left, Direction::Press),
        Step::Button(Button::Left, Direction::Release),
    ] {
        let releasing = matches!(step, Step::Button(_, Direction::Release));
        let deadline = Instant::now() + PROOF_DEADLINE;
        let guard = scope(observed, target, proof)
            .and_then(|()| {
                let foreground =
                    input_ownership(target.window, pid, releasing, initial_foreground)?;
                if completed == 0 {
                    initial_foreground = foreground;
                } else if !releasing && foreground != initial_foreground {
                    return Err("Foreground changed after caption pointer movement".into());
                }
                Ok(())
            })
            .and_then(|()| patch(proof, target.window, pid, deadline))
            .and_then(|()| {
                if completed == 0 {
                    Ok(())
                } else {
                    cursor(proof.point)
                }
            })
            .and_then(|()| {
                let current = input_ownership(target.window, pid, releasing, initial_foreground)?;
                if !releasing && current != initial_foreground {
                    return Err("Foreground changed immediately before caption input".into());
                }
                owner_scope(observed, target, proof)
            });
        if let Err(error) = guard {
            return if attempted == 0 {
                Err(denied(observed, error, accepted, acknowledged))
            } else {
                Ok(incomplete(
                    observed,
                    proof,
                    &mut driver,
                    completed,
                    attempted,
                    step.kind(),
                    error,
                ))
            };
        }
        attempted += 1;
        if let Err(error) = driver.apply(step) {
            return Ok(incomplete(
                observed,
                proof,
                &mut driver,
                completed,
                attempted,
                step.kind(),
                error,
            ));
        }
        completed += 1;
    }
    let mut result = 0;
    let click_acknowledged = unsafe {
        SendMessageTimeoutW(
            target.window,
            WM_NULL,
            0,
            0,
            SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
            5000,
            &mut result,
        )
    } != 0;
    let final_guard = if !click_acknowledged {
        Err("CAD did not acknowledge the caption click within 5s".into())
    } else {
        scope(observed, target, proof).and_then(|()| guard_foreground(target.window, false, false))
    };
    if let Err(error) = final_guard {
        return Ok(incomplete(
            observed,
            proof,
            &mut driver,
            completed,
            attempted,
            "caption_acknowledgment",
            error,
        ));
    }
    let stability_started = Instant::now();
    for _ in 0..FOCUS_STABILITY_SAMPLES {
        std::thread::sleep(Duration::from_millis(NATIVE_TEXT_INTERVAL_MS));
        if let Err(error) = owner_scope(observed, target, proof)
            .and_then(|()| {
                if geometry(target.window)? == proof.geometry {
                    Ok(())
                } else {
                    Err("Native caption/window geometry changed during focus sampling".into())
                }
            })
            .and_then(|()| guard_foreground(target.window, false, false))
        {
            return Ok(incomplete(
                observed,
                proof,
                &mut driver,
                completed,
                attempted,
                "foreground_sampling",
                error,
            ));
        }
    }
    if let Err(error) =
        scope(observed, target, proof).and_then(|()| guard_foreground(target.window, false, false))
    {
        return Ok(incomplete(
            observed,
            proof,
            &mut driver,
            completed,
            attempted,
            "final_focus_guard",
            error,
        ));
    }
    Ok(
        json!({"status":"focused","method":"verified_native_caption_click","owner":observed.owner,
        "activation_accepted":accepted,"activation_acknowledged":acknowledged,
        "caption_click_acknowledged":click_acknowledged,"completed_primitives":completed,
        "mouse_button_primitives":2,"keyboard_primitives":0,"native_caption_focus":proof.snapshot(),
        "foreground_stability_ms":stability_started.elapsed().as_millis(),
        "foreground_stability_samples":FOCUS_STABILITY_SAMPLES,
        "observation_consumed":true,
        "hint":"One guarded native caption click was inserted. Foreground held during bounded sampling only; observe again before ordinary input. This does not prove product effects or future ownership."}),
    )
}

#[cfg(test)]
mod exposure_tests {
    use super::*;

    #[test]
    fn queued_or_refused_exposure_never_certifies_focus_or_input() {
        let owner = json!({"pid":42,"session_id":"current-document","epoch":9});
        for requested in [true, false] {
            let receipt = exposure_response(&owner, requested);
            assert_eq!(receipt["owner"], owner);
            assert_eq!(receipt["exposure_requested"], requested);
            assert_eq!(receipt["status"], "not_focused");
            assert_eq!(receipt["foreground_verified"], false);
            assert_eq!(receipt["input_sent"], false);
            assert_eq!(receipt["observation_consumed"], true);
            assert!(receipt.get("native_caption_focus").is_none());
            assert!(receipt.get("completed_primitives").is_none());
        }
    }
}
