use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine;
use enigo::{Axis, Button, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use serde::Deserialize;
use serde_json::{json, Value};
use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HWND, LPARAM, POINT, RECT, WAIT_TIMEOUT,
};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForWindow, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, IsWindowEnabled};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::capture;
use crate::{build_pair, session};

const OBSERVATION_MS: u64 = 60_000;
// Conservative queue pacing/stability sampling, not text-consumption proof
// or a guarantee that another application cannot subsequently take focus.
const NATIVE_TEXT_INTERVAL_MS: u64 = 20;
const FOCUS_STABILITY_SAMPLES: usize = 5;
const CAD_GESTURE_CAPTURE_ERROR: &str = "CAD input is captured by an existing gesture";
// Bound read-only queue-settling checks; this does not qualify gesture timing.
const POINTER_RELEASE_CAPTURE_WAIT_MS: u64 = 100;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    action: String,
    session_id: Option<String>,
    observation: Option<String>,
    point: Option<[i32; 2]>,
    to: Option<[i32; 2]>,
    button: Option<String>,
    delta: Option<i32>,
    key: Option<String>,
    text: Option<String>,
    modifiers: Option<Vec<String>>,
    path: Option<Vec<Waypoint>>,
    cancel: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Waypoint {
    point: [i32; 2],
    #[serde(default)]
    hold_ms: u32,
}

struct Observation {
    token: String,
    expires_ms: u64,
    owner: Value,
    main_hwnd: usize,
    hwnd: usize,
    native_dialog: bool,
    bounds: Option<[i32; 4]>,
    layout: Option<Value>,
    editable_focus: bool,
    process: DesktopProcess,
}

/// Holding the process object keeps a recycled PID from inheriting a token.
struct DesktopProcess(usize);
impl DesktopProcess {
    fn open(pid: u32) -> Result<Self, String> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return Err(format!(
                "Cannot retain CAD process ownership: {}",
                std::io::Error::last_os_error()
            ));
        }
        let process = Self(handle as usize);
        process.verify()?;
        Ok(process)
    }

    fn verify(&self) -> Result<(), String> {
        if unsafe { WaitForSingleObject(self.0 as HANDLE, 0) } == WAIT_TIMEOUT {
            Ok(())
        } else {
            Err("Observed CAD process has exited or is unavailable".into())
        }
    }
}
impl Drop for DesktopProcess {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0 as HANDLE);
        }
    }
}

#[derive(Default)]
pub(crate) struct ComputerControl {
    observation: Option<Observation>,
}

impl ComputerControl {
    pub(crate) fn call(
        &mut self,
        arguments: &Value,
        attached: Option<&str>,
    ) -> Result<Value, String> {
        let request: Request = serde_json::from_value(arguments.clone())
            .map_err(|error| format!("Invalid computer control request: {error}"))?;
        if request.action == "observe" {
            self.observation = None;
            let session_id =
                request.session_id.as_deref().or(attached).ok_or(
                    "Computer control needs an explicit or attached active desktop session",
                )?;
            let owner = session::computer_control_owner(session_id)?;
            let pid = owner["pid"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
                .ok_or("Owner has no PID")?;
            let process = DesktopProcess::open(pid)?;
            let inspect = inspect_desktop(session_id, false)?;
            let main = main_from_inspection(&owner, &inspect)?;
            let target = target_window(&owner, main)?;
            let hwnd = target.window;
            let native = target
                .native_dialog
                .then(|| native_layout(hwnd))
                .transpose()?;
            let captured = target
                .native_dialog
                .then(|| capture::png(hwnd as usize))
                .transpose()?;
            process.verify()?;
            if target.native_dialog
                && main_from_inspection(&owner, &inspect_desktop(session_id, false)?)? != main
            {
                return Err(
                    "Bevy primary window changed during native capture; observe again".into(),
                );
            }
            if session::computer_control_owner(session_id)? != owner
                || target_window(&owner, main)? != target
                || native
                    .as_ref()
                    .is_some_and(|expected| native_layout(hwnd).as_ref() != Ok(expected))
            {
                return Err(
                    "CAD owner or native dialog changed during observation; observe again".into(),
                );
            }
            let presented = (target.native_dialog || inspect["presented"] == true)
                && unsafe { IsIconic(hwnd) } == 0;
            let bounds = if presented {
                Some(client_bounds(hwnd)?)
            } else {
                None
            };
            if let (Some(captured), Some(client)) = (&captured, bounds) {
                let image = captured.screen_bounds;
                // A dialog's first compositor frame can contain only its title
                // bar. Qualify coordinates and edit focus only after its entire
                // current client area is visible in the captured frame.
                if image[0] > client[0]
                    || image[1] > client[1]
                    || i64::from(image[0]) + i64::from(image[2])
                        < i64::from(client[0]) + i64::from(client[2])
                    || i64::from(image[1]) + i64::from(image[3])
                        < i64::from(client[1]) + i64::from(client[3])
                {
                    return Err(
                        "Native dialog capture does not cover its client area; observe again"
                            .into(),
                    );
                }
            }
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let token = format!(
                "computer-{}-{}-{}",
                std::process::id(),
                session::now_ms(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let expires_ms = session::now_ms().saturating_add(OBSERVATION_MS);
            let focused = &inspect["ui"]["focused_control"];
            let editable_focus = presented
                && if let Some(native) = &native {
                    native["focused_editable"] == true
                } else {
                    inspect["ui"]["surfaces"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .flat_map(|surface| surface["controls"].as_array().into_iter().flatten())
                        .any(|control| {
                            control["id"] == *focused
                                && matches!(
                                    control["role"].as_str(),
                                    Some("textbox" | "multiline_textbox")
                                )
                                && control["read_only"] != true
                                && control["disabled"] != true
                        })
                };
            self.observation = Some(Observation {
                token: token.clone(),
                expires_ms,
                owner: owner.clone(),
                main_hwnd: target.main as usize,
                hwnd: hwnd as usize,
                native_dialog: target.native_dialog,
                bounds,
                layout: presented.then(|| native.clone().unwrap_or_else(|| layout(&inspect))),
                editable_focus,
                process,
            });
            let screen_bounds = bounds.map(
                |bounds| json!({"x":bounds[0],"y":bounds[1],"width":bounds[2],"height":bounds[3]}),
            );
            let scale = bounds.filter(|_| !target.native_dialog).map(|bounds| {
                [
                    bounds[2] as f64
                        / inspect["ui"]["client"]["width"]
                            .as_f64()
                            .unwrap_or(bounds[2] as f64),
                    bounds[3] as f64
                        / inspect["ui"]["client"]["height"]
                            .as_f64()
                            .unwrap_or(bounds[3] as f64),
                ]
            });
            let inspection = if target.native_dialog {
                json!({"status":inspect["status"],"presented":true,"build_pair":inspect["build_pair"],
                    "native_dialog":native,"host_render_status":inspect["render_status"]})
            } else if presented {
                inspect
            } else {
                json!({"status":inspect["status"],"presented":false,"build_pair":inspect["build_pair"],
                    "render_status":inspect["render_status"],"hint":"Owner-only focus observation; no rendered controls or pointer coordinates are qualified."})
            };
            let image = captured.map(|captured| {
                let offset = bounds.map(|bounds| {
                    [
                        bounds[0] - captured.screen_bounds[0],
                        bounds[1] - captured.screen_bounds[1],
                    ]
                });
                json!({"png_base64":base64::engine::general_purpose::STANDARD.encode(captured.png),
                    "width":captured.width,"height":captured.height,
                    "screen_bounds":captured.screen_bounds,"client_to_image_offset":offset,
                    "cleanup":captured.cleanup})
            });
            return Ok(
                json!({"status":"observed","observation":token,"expires_ms":expires_ms,
                "owner":owner,"window_handle":hwnd as usize,"executable":std::env::current_exe().map_err(|e|e.to_string())?,
                "main_window_handle":target.main as usize,"target_kind":if target.native_dialog {"native_dialog"} else {"bevy_window"},
                "native_window_diagnostics":window_diagnostics(pid),
                "presented":presented,"focus_only":!presented,"minimized":unsafe { IsIconic(hwnd) } != 0,
                "client_screen_bounds":screen_bounds,
                "coordinate_space":"physical_client_pixels","dpi":unsafe { GetDpiForWindow(hwnd) },
                "interface_to_physical_scale":scale,
                "text_support":if target.native_dialog {"printable_unicode"} else {"printable_bmp"},
                "foreground":unsafe { GetForegroundWindow() == hwnd },"editable_focus":editable_focus,"inspection":inspection,"image":image,
                "hint":if target.native_dialog {"The attached image shows the owned native dialog. Subtract image.client_to_image_offset from image pixel points to obtain physical client coordinates. Focus if needed, observe again, send one input and observe its visible result."} else {"Use cad_interface capture for the actual rendered image. Focus if needed, observe again, then send one input and observe its visible result."}}),
            );
        }
        let observed = self
            .observation
            .take()
            .ok_or("Observe the CAD window before sending input")?;
        if request.observation.as_deref() != Some(&observed.token)
            || session::now_ms() > observed.expires_ms
        {
            return Err(
                "Computer observation is missing, expired or already consumed; observe again"
                    .into(),
            );
        }
        let session_id = observed.owner["session_id"]
            .as_str()
            .ok_or("Observation has no session")?;
        observed.process.verify()?;
        if request
            .session_id
            .as_deref()
            .is_some_and(|id| id != session_id)
            || session::computer_control_owner(session_id)? != observed.owner
        {
            return Err("Active desktop document changed; observe again before input".into());
        }
        let focus = request.action == "focus";
        let current = inspect_desktop(session_id, !focus && !observed.native_dialog)?;
        let main = main_from_inspection(&observed.owner, &current)?;
        let target = target_window(&observed.owner, main)?;
        let hwnd = target.window;
        if hwnd as usize != observed.hwnd
            || target.main as usize != observed.main_hwnd
            || target.native_dialog != observed.native_dialog
        {
            return Err("CAD window was replaced, moved or resized; observe again".into());
        }
        if !focus && observed.bounds.is_none() {
            return Err("This observation qualifies focus only; restore/focus CAD and observe a presented frame before input".into());
        }
        if let Some(bounds) = observed.bounds {
            if client_bounds(hwnd)? != bounds {
                return Err("CAD window moved or resized; observe again".into());
            }
        }
        if let Some(expected) = &observed.layout {
            let current_layout = if observed.native_dialog {
                native_layout(hwnd)?
            } else {
                layout(&current)
            };
            if current_layout != *expected {
                if observed.native_dialog {
                    return Err(format!(
                        "Native CAD dialog changed ({}); observe again before input",
                        native_layout_difference(expected, &current_layout)
                    ));
                }
                return Err(
                    "Rendered controls or camera changed; observe again before input".into(),
                );
            }
        }
        if session::computer_control_owner(session_id)? != observed.owner {
            return Err("Desktop changed while checking input guards; observe again".into());
        }
        if focus {
            let accepted = unsafe {
                if IsIconic(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                SetForegroundWindow(hwnd) != 0
            };
            // Activation across input queues is asynchronous. Wait for the
            // target to process that one request before checking foreground.
            // https://devblogs.microsoft.com/oldnewthing/20161118-00/?p=94745
            let mut result = 0;
            let acknowledged = unsafe {
                SendMessageTimeoutW(
                    hwnd,
                    WM_NULL,
                    0,
                    0,
                    SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                    5000,
                    &mut result,
                ) != 0
            };
            observed.process.verify()?;
            if session::computer_control_owner(session_id)? != observed.owner
                || target_window(&observed.owner, main)? != target
                || session::now_ms() > observed.expires_ms
            {
                return Err("CAD owner or window changed while activating; observe again".into());
            }
            if !acknowledged {
                return Err(
                    "CAD did not acknowledge foreground activation within 5s; no input was sent"
                        .into(),
                );
            }
            if unsafe { GetForegroundWindow() } != hwnd || unsafe { IsIconic(hwnd) } != 0 {
                return Err(json!({"code":"computer_control_foreground_denied",
                    "message":"Windows did not activate the owned CAD window; no input was sent",
                    "activation_accepted":accepted,"activation_acknowledged":acknowledged,
                    "expected_hwnd":hwnd as usize,
                    "foreground_hwnd":unsafe { GetForegroundWindow() } as usize,
                    "minimized":unsafe { IsIconic(hwnd) } != 0})
                .to_string());
            }
            guard_foreground(hwnd, false, observed.native_dialog)?;
            for _ in 0..FOCUS_STABILITY_SAMPLES {
                std::thread::sleep(std::time::Duration::from_millis(NATIVE_TEXT_INTERVAL_MS));
                observed.process.verify()?;
                if session::computer_control_owner(session_id)? != observed.owner
                    || target_window(&observed.owner, main)? != target
                {
                    return Err(
                        "CAD owner or window changed during activation; observe again".into(),
                    );
                }
                if unsafe { GetForegroundWindow() } != hwnd || unsafe { IsIconic(hwnd) } != 0 {
                    return Err("CAD foreground activation was lost during stability sampling; no input was sent".into());
                }
            }
            return Ok(
                json!({"status":"focused","owner":observed.owner,"observation_consumed":true,
                "activation_accepted":accepted,"activation_acknowledged":acknowledged,
                "foreground_stability_ms":NATIVE_TEXT_INTERVAL_MS * FOCUS_STABILITY_SAMPLES as u64,
                "hint":"Foreground ownership held during bounded sampling only. Observe again before sending mouse or keyboard input."}),
            );
        }
        guard_foreground(hwnd, false, observed.native_dialog)?;
        guard_held_input()?;
        let plan = plan(&request, &observed, hwnd)?;
        guard_foreground(hwnd, false, observed.native_dialog)?;
        if Some(client_bounds(hwnd)?) != observed.bounds
            || session::computer_control_owner(session_id)? != observed.owner
            || session::now_ms() > observed.expires_ms
        {
            return Err("Desktop moved or changed immediately before input; observe again".into());
        }
        observed.process.verify()?;
        let backend = if request.action == "move" {
            "verified_win32_cursor"
        } else {
            "enigo"
        };
        let mut driver = InputDriver::new()?;
        let mut completed = 0;
        let mut pointer = None;
        let mut pointer_start = None;
        let planned = plan.iter().filter(|step| step.is_input()).count();
        let mut native_text = if observed.native_dialog && request.action == "text" {
            guard_native_text_focus(&observed, hwnd)?;
            Some(NativeTextVerification::new(&observed, hwnd)?)
        } else {
            None
        };
        for step in &plan {
            let guard = guard_action(&observed, hwnd, driver.holds_button(), completed == 0)
                .or_else(|error| {
                    if !observed.native_dialog
                        && driver.is_post_pointer_modifier_release(*step)
                        && error == CAD_GESTURE_CAPTURE_ERROR
                    {
                        wait_for_pointer_capture_clear(&observed, hwnd)
                    } else {
                        Err(error)
                    }
                })
                .and_then(|()| match *step {
                    Step::Move(point) => guard_pointer(point, hwnd),
                    Step::Button(_, _) | Step::Scroll(_) | Step::Pause(_) => guard_cursor(
                        pointer.ok_or("Pointer input has no planned position")?,
                        hwnd,
                    ),
                    Step::Key(_, _) if driver.holds_button() => guard_cursor(
                        pointer.ok_or("Pointer input has no planned position")?,
                        hwnd,
                    ),
                    Step::Text(_) if observed.native_dialog => {
                        guard_native_text_focus(&observed, hwnd)
                    }
                    _ => Ok(()),
                });
            if let Err(error) = guard {
                if completed == 0 {
                    return Err(error);
                }
                let cleanup_errors = driver.release_all();
                return Ok(json!({"status":"input_incomplete","action":request.action,
                    "backend":backend,"completed_primitives":completed,"planned_primitives":planned,
                    "failed_primitive":step.kind(),"error":error,"cleanup_errors":cleanup_errors,
                    "input_may_have_been_inserted":true,"owner":observed.owner,
                    "observation_consumed":true,"hint":"Input stopped when an ownership, focus or visibility guard changed. Observe and capture the result; do not blindly retry."}));
            }
            if let Err(error) = driver.apply(*step) {
                let cleanup_errors = driver.release_all();
                return Ok(json!({"status":"input_incomplete","action":request.action,
                    "backend":backend,"completed_primitives":completed,"planned_primitives":planned,
                    "failed_primitive":step.kind(),"error":error,"cleanup_errors":cleanup_errors,
                    "input_may_have_been_inserted":true,"owner":observed.owner,
                    "observation_consumed":true,"hint":"The input backend cannot report how many events a failed primitive inserted. Owned held keys/buttons received one release attempt. Observe and capture the result; do not blindly retry."}));
            }
            if let Step::Move(point) = *step {
                pointer = Some(point);
                pointer_start.get_or_insert(point);
            }
            completed += usize::from(step.is_input());
            if observed.native_dialog && matches!(step, Step::Text(_)) {
                if let (Some(verification), Step::Text(text)) = (&mut native_text, step) {
                    verification.prefix.extend(text.encode_utf16());
                    if let Err(error) = verify_native_text(&observed, hwnd, verification, false) {
                        let cleanup_errors = driver.release_all();
                        return Ok(json!({"status":"input_incomplete","action":request.action,
                            "backend":backend,"completed_primitives":completed,"planned_primitives":planned,
                            "verified_text_scalars":verification.verified_scalars,
                            "failed_primitive":"native_text_readback","error":error,"cleanup_errors":cleanup_errors,
                            "input_may_have_been_inserted":true,"owner":observed.owner,
                            "observation_consumed":true,"hint":"Native edit readback did not confirm the queued scalar. Observe the actual text; do not submit or blindly retry."}));
                    }
                    verification.verified_scalars += 1;
                }
            }
        }
        if observed.native_dialog && request.action == "text" {
            if let Err(error) = guard_action(&observed, hwnd, false, false)
                .and_then(|()| guard_native_text_focus(&observed, hwnd))
                .and_then(|()| {
                    verify_native_text(&observed, hwnd, native_text.as_ref().unwrap(), true)
                })
            {
                let cleanup_errors = driver.release_all();
                return Ok(json!({"status":"input_incomplete","action":request.action,
                    "backend":backend,"completed_primitives":completed,"planned_primitives":planned,
                    "failed_primitive":"final_text_guard","error":error,"cleanup_errors":cleanup_errors,
                    "input_may_have_been_inserted":true,"owner":observed.owner,
                    "observation_consumed":true,"hint":"Final native edit ownership or exact text replacement could not be confirmed. Observe and capture the actual result before any further input."}));
            }
        }
        let client_point = |point: [i32; 2]| {
            observed
                .bounds
                .map(|bounds| [point[0] - bounds[0], point[1] - bounds[1]])
        };
        Ok(
            json!({"status":"input_sent","action":request.action,"backend":backend,
            "completed_primitives":completed,"owner":observed.owner,
            "mouse_button_primitives":plan.iter().filter(|step| matches!(step, Step::Button(_, _))).count(),
            "keyboard_primitives":plan.iter().filter(|step| matches!(step, Step::Key(_, _) | Step::Text(_))).count(),
            "native_text_verified_scalars":native_text.as_ref().map(|verification| verification.verified_scalars),
            "native_text_verification":native_text.as_ref().map(|_| "Exact edit text replacement verified by bounded read-only Win32 readback; this does not verify saving or CAD behavior."),
            "pointer_start_physical_client":pointer_start.and_then(client_point),
            "pointer_end_physical_client":pointer.and_then(client_point),
            "pointer_verification":"Cursor checked after movement and before pointer primitives; coordinates describe input, not the resulting product state.",
            "observation_consumed":true,"hint":"OS insertion does not confirm product behavior. Observe and capture before the next action; do not blindly retry."}),
        )
    }
}

fn inspect_desktop(session_id: &str, require_presented: bool) -> Result<Value, String> {
    let mut result =
        session::request_ui(&json!({"action":"inspect","session_id":session_id}), None)?;
    build_pair::decorate(&mut result);
    if result["status"] != "applied"
        || (require_presented && result["presented"] != true)
        || result["build_pair"]["status"] != "matched"
    {
        return Err(json!({"code":"computer_control_not_ready","inspection":result,
            "hint":"The current CAD window must have the same clean build as this MCP process. Pointer and keyboard input additionally require a presented frame; focus can restore a retained window."}).to_string());
    }
    Ok(result)
}

fn layout(inspect: &Value) -> Value {
    let mut ui = inspect["ui"].clone();
    let focused = ui["focused_control"].clone();
    if let Some(surfaces) = ui["surfaces"].as_array_mut() {
        for surface in surfaces {
            if let Some(controls) = surface["controls"].as_array_mut() {
                for control in controls {
                    let is_focused = control["id"] == focused;
                    if let Some(map) = control.as_object_mut() {
                        map.remove("id");
                        map.insert("focused".into(), json!(is_focused));
                    }
                }
            }
        }
    }
    if let Some(map) = ui.as_object_mut() {
        map.remove("focused_control");
        map.remove("unlabeled_controls");
        map.remove("ime_diagnostics");
    }
    json!({"ui":ui,"view_state":inspect["view_state"]})
}

#[derive(Clone, Copy, PartialEq)]
struct Target {
    main: HWND,
    window: HWND,
    native_dialog: bool,
}

fn target_window(owner: &Value, published_main: usize) -> Result<Target, String> {
    let main = main_window(owner, published_main)?;
    let foreground = unsafe { GetAncestor(GetForegroundWindow(), GA_ROOT) };
    let mut popup = unsafe { GetLastActivePopup(main) };
    for _ in 0..16 {
        let next = unsafe { GetLastActivePopup(popup) };
        if next == popup || next.is_null() {
            break;
        }
        popup = next;
    }
    for window in [foreground, popup] {
        if owned_dialog(main, window)
            && unsafe { IsWindowVisible(window) } != 0
            && unsafe { IsWindowEnabled(window) } != 0
            && unsafe { IsIconic(window) } == 0
            && window_class(window)? == "#32770"
        {
            return Ok(Target {
                main,
                window,
                native_dialog: true,
            });
        }
    }
    if unsafe { IsWindowEnabled(main) } == 0 {
        return Err(
            "CAD is blocked by a modal window without a qualified same-process owner chain".into(),
        );
    }
    Ok(Target {
        main,
        window: main,
        native_dialog: false,
    })
}

fn owned_dialog(main: HWND, window: HWND) -> bool {
    if window.is_null() || window == main {
        return false;
    }
    let mut main_pid = 0;
    unsafe {
        GetWindowThreadProcessId(main, &mut main_pid);
    }
    let mut cursor = window;
    for _ in 0..16 {
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(cursor, &mut pid);
        }
        if pid != main_pid || pid == 0 {
            return false;
        }
        let owner = unsafe { GetWindow(cursor, GW_OWNER) };
        if owner == main {
            return true;
        }
        if owner.is_null() || owner == cursor {
            return false;
        }
        cursor = owner;
    }
    false
}

fn window_diagnostics(pid: u32) -> Value {
    let _dpi = match DpiGuard::enter() {
        Ok(guard) => guard,
        Err(error) => return json!({"error":error,"diagnostics_only":true}),
    };
    struct Search {
        pid: u32,
        windows: Vec<Value>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, pointer: LPARAM) -> i32 {
        let search = &mut *(pointer as *mut Search);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == search.pid {
            let mut caption = [0u16; 512];
            let length = GetWindowTextW(hwnd, caption.as_mut_ptr(), caption.len() as i32);
            let mut rect = RECT::default();
            let rect = (GetWindowRect(hwnd, &mut rect) != 0).then_some([
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
            ]);
            search.windows.push(json!({"hwnd":hwnd as usize,"pid":pid,
                "class":window_class(hwnd).unwrap_or_else(|error| error),
                "caption":String::from_utf16_lossy(&caption[..length.max(0) as usize]),
                "visible":IsWindowVisible(hwnd) != 0,"enabled":IsWindowEnabled(hwnd) != 0,
                "owner_hwnd":GetWindow(hwnd, GW_OWNER) as usize,"physical_screen_rect":rect}));
        }
        1
    }
    let mut search = Search {
        pid,
        windows: Vec::new(),
    };
    if unsafe { EnumWindows(Some(visit), &mut search as *mut Search as LPARAM) } == 0 {
        return json!({"error":format!("Could not enumerate CAD windows: {}",std::io::Error::last_os_error()),
            "diagnostics_only":true,"windows":search.windows});
    }
    json!({"diagnostics_only":true,"windows":search.windows})
}

fn main_from_inspection(owner: &Value, inspect: &Value) -> Result<usize, String> {
    let published = &inspect["native_window"];
    let pid = owner["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or("Owner has no PID")?;
    if published["pid"] != owner["pid"] || published["window_id"] != owner["window_id"] {
        return Err(json!({"code":"computer_control_main_window_not_published",
            "message":"The active CAD inspection must publish its exact Bevy primary HWND and owner",
            "expected_owner":owner,"native_window":published,
            "diagnostics":window_diagnostics(pid)}).to_string());
    }
    published["hwnd"]
        .as_u64()
        .and_then(|hwnd| usize::try_from(hwnd).ok())
        .filter(|hwnd| *hwnd != 0)
        .ok_or_else(|| {
            json!({"code":"computer_control_main_window_not_published",
            "message":"CAD inspection did not publish a valid primary HWND",
            "native_window":published,"diagnostics":window_diagnostics(pid)})
            .to_string()
        })
}

fn main_window(owner: &Value, published: usize) -> Result<HWND, String> {
    let pid = owner["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or("Owner has no PID")?;
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    if !same_path(&process_image(pid)?, &current)? {
        return Err(
            "GUI and MCP executable paths differ; restart both from the canonical installed binary"
                .into(),
        );
    }
    let hwnd = published as HWND;
    let mut actual_pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut actual_pid);
    }
    if unsafe { IsWindow(hwnd) } == 0
        || actual_pid != pid
        || unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd
        || unsafe { IsWindowVisible(hwnd) } == 0
    {
        return Err(json!({"code":"computer_control_main_window_mismatch",
            "message":"Published Bevy primary HWND is no longer an owned visible top-level CAD window",
            "published_hwnd":published,"expected_pid":pid,"actual_pid":actual_pid,
            "diagnostics":window_diagnostics(pid)}).to_string());
    }
    Ok(hwnd)
}

fn window_class(hwnd: HWND) -> Result<String, String> {
    let mut class = [0u16; 256];
    let length = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
    if length <= 0 {
        return Err("Cannot identify the native CAD control class".into());
    }
    String::from_utf16(&class[..length as usize]).map_err(|error| error.to_string())
}

fn native_layout(hwnd: HWND) -> Result<Value, String> {
    struct Children {
        controls: Vec<Value>,
        error: Option<String>,
    }
    unsafe extern "system" fn visit(child: HWND, pointer: LPARAM) -> i32 {
        let children = &mut *(pointer as *mut Children);
        if children.controls.len() >= 1024 {
            children.error = Some("Native CAD dialog has too many controls to qualify".into());
            return 0;
        }
        if IsWindowVisible(child) == 0 {
            return 1;
        }
        let mut rect = RECT::default();
        if GetWindowRect(child, &mut rect) == 0 {
            children.error = Some("Native CAD dialog changed during control observation".into());
            return 0;
        }
        let Ok(class) = window_class(child) else {
            children.error = Some("Native CAD dialog changed during control observation".into());
            return 0;
        };
        let mut caption = [0u16; 512];
        let length = GetWindowTextW(child, caption.as_mut_ptr(), caption.len() as i32);
        children
            .controls
            .push(json!({"window_handle":child as usize,"class":class,
            "caption":String::from_utf16_lossy(&caption[..length.max(0) as usize]),
            "screen_bounds":[rect.left,rect.top,rect.right-rect.left,rect.bottom-rect.top],
            "enabled":IsWindowEnabled(child) != 0,
            "style":GetWindowLongPtrW(child, GWL_STYLE)}));
        1
    }
    let _dpi = DpiGuard::enter()?;
    let mut children = Children {
        controls: Vec::new(),
        error: None,
    };
    unsafe {
        EnumChildWindows(hwnd, Some(visit), &mut children as *mut Children as LPARAM);
    }
    if let Some(error) = children.error {
        return Err(error);
    }
    children
        .controls
        .sort_by_key(|control| control["window_handle"].as_u64());
    let mut pid = 0;
    let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    let mut info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetGUIThreadInfo(thread, &mut info) } == 0 {
        return Err("Cannot observe native CAD dialog focus".into());
    }
    let focus = info.hwndFocus;
    let mut focus_pid = 0;
    unsafe {
        GetWindowThreadProcessId(focus, &mut focus_pid);
    }
    let editable = !focus.is_null()
        && focus_pid == pid
        && unsafe { GetAncestor(focus, GA_ROOT) } == hwnd
        && unsafe { IsWindowEnabled(focus) } != 0
        && unsafe { IsWindowVisible(focus) } != 0
        && (unsafe { GetWindowLongPtrW(focus, GWL_STYLE) } & ES_READONLY as isize) == 0
        && window_class(focus).is_ok_and(|class| {
            class.eq_ignore_ascii_case("Edit") || class.to_ascii_uppercase().starts_with("RICHEDIT")
        });
    Ok(
        json!({"class":window_class(hwnd)?,"controls":children.controls,
        "focused_control":focus as usize,"focused_editable":editable}),
    )
}

fn native_layout_difference(expected: &Value, current: &Value) -> String {
    let bounded = |value: &Value| {
        let rendered = value.to_string();
        let mut summary: String = rendered.chars().take(80).collect();
        if rendered.chars().count() > 80 {
            summary.push_str("...");
        }
        summary
    };
    let changed = |path: &str, before: &Value, after: &Value| {
        format!("{path}: {} -> {}", bounded(before), bounded(after))
    };
    for field in ["focused_control", "focused_editable", "class"] {
        if expected[field] != current[field] {
            return changed(field, &expected[field], &current[field]);
        }
    }
    let Some(before) = expected["controls"].as_array() else {
        return "observed controls unavailable".into();
    };
    let Some(after) = current["controls"].as_array() else {
        return "current controls unavailable".into();
    };
    if before.len() != after.len() {
        return format!("control count: {} -> {}", before.len(), after.len());
    }
    for (index, (old, new)) in before.iter().zip(after).enumerate() {
        for field in [
            "window_handle",
            "caption",
            "screen_bounds",
            "style",
            "enabled",
            "class",
        ] {
            if old[field] != new[field] {
                return changed(
                    &format!("control[{index}].{field}"),
                    &old[field],
                    &new[field],
                );
            }
        }
    }
    "other native layout metadata changed".into()
}

fn process_image(pid: u32) -> Result<PathBuf, String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(format!(
            "Cannot verify CAD process {pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    let success =
        unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) };
    let error = std::io::Error::last_os_error();
    unsafe {
        CloseHandle(process);
    }
    if success == 0 {
        return Err(format!("Cannot read CAD executable: {error}"));
    }
    Ok(PathBuf::from(
        String::from_utf16(&buffer[..length as usize]).map_err(|e| e.to_string())?,
    ))
}

fn same_path(left: &Path, right: &Path) -> Result<bool, String> {
    let canonical = |path: &Path| {
        std::fs::canonicalize(path)
            .map(|path| path.to_string_lossy().to_lowercase())
            .map_err(|e| e.to_string())
    };
    Ok(canonical(left)? == canonical(right)?)
}

struct DpiGuard(DPI_AWARENESS_CONTEXT);
impl DpiGuard {
    fn enter() -> Result<Self, String> {
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if previous.is_null() {
            return Err("Cannot establish physical screen coordinates".into());
        }
        Ok(Self(previous))
    }
}
impl Drop for DpiGuard {
    fn drop(&mut self) {
        unsafe {
            SetThreadDpiAwarenessContext(self.0);
        }
    }
}

fn client_bounds(hwnd: HWND) -> Result<[i32; 4], String> {
    let _dpi = DpiGuard::enter()?;
    let mut rect = RECT::default();
    let mut origin = POINT::default();
    if unsafe { GetClientRect(hwnd, &mut rect) } == 0
        || unsafe { ClientToScreen(hwnd, &mut origin) } == 0
    {
        return Err("Cannot read the CAD client rectangle".into());
    }
    let bounds = [
        origin.x,
        origin.y,
        rect.right - rect.left,
        rect.bottom - rect.top,
    ];
    if bounds[2] <= 0 || bounds[3] <= 0 {
        return Err("CAD client has no drawable area".into());
    }
    Ok(bounds)
}

fn guard_foreground(hwnd: HWND, owns_capture: bool, native_dialog: bool) -> Result<(), String> {
    if unsafe { GetForegroundWindow() } != hwnd {
        return Err("CAD is not the foreground window; focus it and observe again".into());
    }
    let mut pid = 0;
    let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    let mut info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetGUIThreadInfo(thread, &mut info) } == 0 {
        return Err("Cannot inspect CAD native input ownership".into());
    }
    if !info.hwndMenuOwner.is_null()
        && (!native_dialog || !target_contains(hwnd, info.hwndMenuOwner))
    {
        return Err("CAD input is captured by a native menu".into());
    }
    if !info.hwndFocus.is_null()
        && if native_dialog {
            !target_contains(hwnd, info.hwndFocus)
        } else {
            (unsafe { GetAncestor(info.hwndFocus, GA_ROOT) }) != hwnd
        }
    {
        return Err("CAD native input focus belongs to another window".into());
    }
    if !info.hwndCapture.is_null() {
        if !target_contains(hwnd, info.hwndCapture) {
            return Err("Pointer capture belongs to another window".into());
        }
        if !(owns_capture || native_dialog) {
            return Err(CAD_GESTURE_CAPTURE_ERROR.into());
        }
    }
    Ok(())
}

fn wait_for_pointer_capture_clear(observed: &Observation, hwnd: HWND) -> Result<(), String> {
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_millis(POINTER_RELEASE_CAPTURE_WAIT_MS);
    loop {
        // No injected input or capture allowance: every check keeps the same
        // process, owner, bounds, foreground, focus and native-menu fences.
        match guard_action(observed, hwnd, false, false) {
            Ok(()) => return Ok(()),
            Err(error) if error == CAD_GESTURE_CAPTURE_ERROR => {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err(format!(
                        "{error} after a bounded mouse-up capture-clear wait"
                    ));
                }
                std::thread::sleep(remaining.min(std::time::Duration::from_millis(10)));
            }
            Err(error) => return Err(error),
        }
    }
}

fn guard_action(
    observed: &Observation,
    hwnd: HWND,
    owns_capture: bool,
    first: bool,
) -> Result<(), String> {
    observed.process.verify()?;
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
    }
    if unsafe { IsWindow(hwnd) } == 0
        || unsafe { IsWindowEnabled(hwnd) } == 0
        || unsafe { IsWindowVisible(hwnd) } == 0
        || unsafe { IsIconic(hwnd) } != 0
        || observed.owner["pid"].as_u64() != Some(pid as u64)
        || (observed.native_dialog && !owned_dialog(observed.main_hwnd as HWND, hwnd))
        || (!observed.native_dialog && observed.main_hwnd != hwnd as usize)
        || session::now_ms() > observed.expires_ms
        || Some(client_bounds(hwnd)?) != observed.bounds
    {
        return Err("Observed CAD window is no longer available at its qualified bounds".into());
    }
    let session_id = observed.owner["session_id"]
        .as_str()
        .ok_or("Observation has no session")?;
    let current = session::computer_control_owner(session_id)?;
    if (first && current != observed.owner)
        || [
            "session_id",
            "window_id",
            "document_id",
            "process_instance_id",
            "pid",
        ]
        .iter()
        .any(|field| current[*field] != observed.owner[*field])
    {
        return Err("Active CAD document or window owner changed during input".into());
    }
    guard_foreground(hwnd, owns_capture, observed.native_dialog)
}

use super::native_text::{NativeEditText, SettledEdit};

struct NativeTextVerification {
    prefix: Vec<u16>,
    suffix: Vec<u16>,
    verified_scalars: usize,
}

impl NativeTextVerification {
    fn new(observed: &Observation, hwnd: HWND) -> Result<Self, String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
        let (edit, _) = read_guarded_native_edit(observed, hwnd, deadline)?;
        if String::from_utf16(&edit.text[..edit.start]).is_err()
            || String::from_utf16(&edit.text[edit.end..]).is_err()
        {
            return Err(
                "Native CAD edit selection splits a Unicode scalar; no input was sent".into(),
            );
        }
        Ok(Self {
            prefix: edit.text[..edit.start].to_vec(),
            suffix: edit.text[edit.end..].to_vec(),
            verified_scalars: 0,
        })
    }
}

enum NativeEditReadError {
    Unavailable(String),
    Unstable(String),
}

impl From<String> for NativeEditReadError {
    fn from(error: String) -> Self {
        Self::Unavailable(error)
    }
}

fn read_native_edit(
    observed: &Observation,
    deadline: std::time::Instant,
) -> Result<NativeEditText, NativeEditReadError> {
    let edit = observed
        .layout
        .as_ref()
        .and_then(|layout| layout["focused_control"].as_u64())
        .ok_or_else(|| {
            NativeEditReadError::Unavailable("Native text observation has no qualified edit".into())
        })? as usize as HWND;
    let read = |message: u32, wparam: usize, lparam: isize| -> Result<usize, String> {
        let timeout = deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_millis()
            .min(100) as u32;
        if timeout == 0 {
            return Err("Native CAD edit readback deadline elapsed".into());
        }
        let mut result = 0;
        if unsafe {
            SendMessageTimeoutW(
                edit,
                message,
                wparam,
                lparam,
                SMTO_ABORTIFHUNG,
                timeout,
                &mut result,
            )
        } == 0
        {
            return Err("Native CAD edit readback timed out or was unavailable".into());
        }
        Ok(result)
    };
    const MAX_NATIVE_EDIT_UNITS: usize = 4096;
    let length = read(WM_GETTEXTLENGTH, 0, 0)?;
    if length > MAX_NATIVE_EDIT_UNITS {
        return Err(NativeEditReadError::Unavailable(
            "Native CAD edit text exceeds the bounded readback scope".into(),
        ));
    }
    let before_selection = read(0x00B0, 0, 0)?;
    let mut text = vec![0u16; MAX_NATIVE_EDIT_UNITS + 1];
    let copied = read(WM_GETTEXT, text.len(), text.as_mut_ptr() as isize)?;
    if copied > MAX_NATIVE_EDIT_UNITS {
        return Err(NativeEditReadError::Unavailable(
            "Native CAD edit copy exceeds the bounded readback scope".into(),
        ));
    }
    text.truncate(copied);
    // EM_GETSEL is a system edit message (0x00B0). With both pointer
    // arguments null its packed return suffices for our <=4096-unit scope.
    let selection = read(0x00B0, 0, 0)?;
    let after_length = read(WM_GETTEXTLENGTH, 0, 0)?;
    let start = selection & 0xffff;
    let end = (selection >> 16) & 0xffff;
    let unicode_valid = String::from_utf16(&text).is_ok();
    if length != copied
        || copied != after_length
        || before_selection != selection
        || start > end
        || end > text.len()
        || !unicode_valid
    {
        return Err(NativeEditReadError::Unstable(format!(
            "Native CAD edit snapshot could not be qualified (UTF-16 lengths before/copied/after {length}/{copied}/{after_length}, selection before {}..{}, after {start}..{end}, Unicode valid {unicode_valid})",
            before_selection & 0xffff, (before_selection >> 16) & 0xffff
        )));
    }
    Ok(NativeEditText { text, start, end })
}

fn read_guarded_native_edit(
    observed: &Observation,
    hwnd: HWND,
    deadline: std::time::Instant,
) -> Result<(NativeEditText, bool), String> {
    let mut retried_unstable = false;
    loop {
        guard_action(observed, hwnd, false, false)?;
        guard_native_text_focus(observed, hwnd)?;
        let readback = read_native_edit(observed, deadline);
        guard_action(observed, hwnd, false, false)?;
        guard_native_text_focus(observed, hwnd)?;
        match readback {
            Ok(edit) => return Ok((edit, retried_unstable)),
            Err(NativeEditReadError::Unavailable(error)) => return Err(error),
            Err(NativeEditReadError::Unstable(error)) => {
                retried_unstable = true;
                if std::time::Instant::now() >= deadline {
                    return Err(error);
                }
            }
        }
        // Retry only the read-only snapshot, never the queued keyboard input.
        std::thread::sleep(std::time::Duration::from_millis(NATIVE_TEXT_INTERVAL_MS));
    }
}

fn verify_native_text(
    observed: &Observation,
    hwnd: HWND,
    expected: &NativeTextVerification,
    final_read: bool,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let deadline = started + std::time::Duration::from_millis(750);
    let mut settled = SettledEdit::default();
    loop {
        let (current, retried_unstable) = read_guarded_native_edit(observed, hwnd, deadline)?;
        if retried_unstable {
            settled.reset();
        }
        let acceptable = current.matches(&expected.prefix, &expected.suffix, final_read);
        if settled.observe(started.elapsed(), &current, acceptable) {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "Native edit text/selection did not settle at the requested replacement (expected UTF-16 length {}, actual {}, selection {}..{}); no further input was sent",
                expected.prefix.len() + expected.suffix.len(), current.text.len(), current.start, current.end
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(NATIVE_TEXT_INTERVAL_MS));
    }
}

fn guard_native_text_focus(observed: &Observation, hwnd: HWND) -> Result<(), String> {
    let expected = observed
        .layout
        .as_ref()
        .and_then(|layout| layout["focused_control"].as_u64())
        .ok_or("Native text observation has no qualified edit focus")?;
    let mut pid = 0;
    let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    let mut info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetGUIThreadInfo(thread, &mut info) } == 0 {
        return Err("Cannot verify native CAD edit focus during text input".into());
    }
    let focus = info.hwndFocus;
    let mut focus_pid = 0;
    unsafe {
        GetWindowThreadProcessId(focus, &mut focus_pid);
    }
    if focus.is_null()
        || focus as usize as u64 != expected
        || focus_pid != pid
        || unsafe { GetAncestor(focus, GA_ROOT) } != hwnd
        || unsafe { IsWindowEnabled(focus) } == 0
        || unsafe { IsWindowVisible(focus) } == 0
        || (unsafe { GetWindowLongPtrW(focus, GWL_STYLE) } & ES_READONLY as isize) != 0
        || !window_class(focus).is_ok_and(|class| {
            class.eq_ignore_ascii_case("Edit") || class.to_ascii_uppercase().starts_with("RICHEDIT")
        })
    {
        return Err("The observed native CAD edit no longer owns writable text focus".into());
    }
    Ok(())
}

fn guard_held_input() -> Result<(), String> {
    for key in 1..=254 {
        if unsafe { GetAsyncKeyState(key) } < 0 {
            return Err(
                "A physical key or mouse button is held; release it before computer control".into(),
            );
        }
    }
    Ok(())
}

fn screen_point(point: [i32; 2], observed: &Observation, hwnd: HWND) -> Result<[i32; 2], String> {
    let bounds = observed
        .bounds
        .ok_or("This observation has no qualified pointer coordinates")?;
    if point[0] < 0 || point[1] < 0 || point[0] >= bounds[2] || point[1] >= bounds[3] {
        return Err("Pointer point is outside the observed CAD client rectangle".into());
    }
    let screen = [
        bounds[0]
            .checked_add(point[0])
            .ok_or("Pointer X overflow")?,
        bounds[1]
            .checked_add(point[1])
            .ok_or("Pointer Y overflow")?,
    ];
    guard_pointer(screen, hwnd)?;
    Ok(screen)
}

fn guard_pointer(screen: [i32; 2], hwnd: HWND) -> Result<(), String> {
    let _dpi = DpiGuard::enter()?;
    let target = unsafe {
        WindowFromPoint(POINT {
            x: screen[0],
            y: screen[1],
        })
    };
    if !target_contains(hwnd, target) {
        return Err("Pointer target is occluded by another window".into());
    }
    Ok(())
}

fn target_contains(target: HWND, window: HWND) -> bool {
    let mut target_pid = 0;
    unsafe {
        GetWindowThreadProcessId(target, &mut target_pid);
    }
    let mut cursor = unsafe { GetAncestor(window, GA_ROOT) };
    for _ in 0..16 {
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(cursor, &mut pid);
        }
        if cursor.is_null() || pid == 0 || pid != target_pid {
            return false;
        }
        if cursor == target {
            return true;
        }
        let owner = unsafe { GetWindow(cursor, GW_OWNER) };
        let root = unsafe { GetAncestor(owner, GA_ROOT) };
        if root == cursor {
            return false;
        }
        cursor = root;
    }
    false
}

fn guard_cursor(expected: [i32; 2], hwnd: HWND) -> Result<(), String> {
    let _dpi = DpiGuard::enter()?;
    let mut actual = POINT::default();
    if unsafe { GetCursorPos(&mut actual) } == 0 || [actual.x, actual.y] != expected {
        return Err("Cursor moved away from its qualified CAD target".into());
    }
    guard_pointer(expected, hwnd)
}

/// Enigo 0.6.1 absolute movement normalizes against the primary monitor only.
/// Native physical positioning preserves negative and mixed-DPI monitor coordinates.
fn position_cursor(screen: [i32; 2]) -> Result<(), String> {
    let _dpi = DpiGuard::enter()?;
    let mut actual = POINT::default();
    if unsafe { SetCursorPos(screen[0], screen[1]) } == 0
        || unsafe { GetCursorPos(&mut actual) } == 0
        || [actual.x, actual.y] != screen
    {
        return Err("Windows did not position the cursor at the qualified CAD point".into());
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Step<'a> {
    Move([i32; 2]),
    Button(Button, Direction),
    Key(Key, Direction),
    Scroll(i32),
    Text(&'a str),
    Pause(u64),
}

impl Step<'_> {
    fn kind(&self) -> &'static str {
        match self {
            Self::Move(_) => "pointer_move",
            Self::Button(_, Direction::Press) => "button_press",
            Self::Button(_, _) => "button_release",
            Self::Key(_, Direction::Press) => "key_press",
            Self::Key(_, _) => "key_release",
            Self::Scroll(_) => "wheel",
            Self::Text(_) => "text",
            Self::Pause(_) => "gesture_dwell",
        }
    }

    fn is_input(&self) -> bool {
        !matches!(self, Self::Pause(_))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Held {
    Button(Button),
    Key(Key),
}

/// Track attempted presses too: a failed Enigo call may already have inserted input.
struct InputDriver {
    enigo: Enigo,
    held: Vec<Held>,
    completed_pointer_up: bool,
}

impl InputDriver {
    fn new() -> Result<Self, String> {
        let settings = Settings {
            release_keys_when_dropped: false,
            ..Default::default()
        };
        Ok(Self {
            enigo: Enigo::new(&settings).map_err(|error| error.to_string())?,
            held: Vec::new(),
            completed_pointer_up: false,
        })
    }

    fn holds_button(&self) -> bool {
        self.held.iter().any(|held| matches!(held, Held::Button(_)))
    }

    fn is_post_pointer_modifier_release(&self, step: Step<'_>) -> bool {
        matches!(step, Step::Key(key @ (Key::Control | Key::Shift), Direction::Release)
            if self.completed_pointer_up && !self.holds_button() && self.held.contains(&Held::Key(key)))
    }

    fn apply(&mut self, step: Step<'_>) -> Result<(), String> {
        let held = match step {
            Step::Button(button, direction) => Some((Held::Button(button), direction)),
            Step::Key(key, direction) => Some((Held::Key(key), direction)),
            _ => None,
        };
        if let Some((held, Direction::Press)) = held {
            self.held.push(held);
            if matches!(held, Held::Button(_)) {
                self.completed_pointer_up = false;
            }
        }
        match step {
            Step::Move(point) => position_cursor(point)?,
            Step::Button(button, direction) => self
                .enigo
                .button(button, direction)
                .map_err(|error| error.to_string())?,
            Step::Key(key, direction) => self
                .enigo
                .key(key, direction)
                .map_err(|error| error.to_string())?,
            Step::Scroll(notches) => self
                .enigo
                .scroll(notches, Axis::Vertical)
                .map_err(|error| error.to_string())?,
            Step::Text(text) => self.enigo.text(text).map_err(|error| error.to_string())?,
            Step::Pause(ms) => std::thread::sleep(std::time::Duration::from_millis(ms)),
        }
        if let Some((held, Direction::Release)) = held {
            if matches!(held, Held::Button(_)) {
                self.completed_pointer_up = true;
            }
            self.held.retain(|candidate| *candidate != held);
        }
        Ok(())
    }

    fn release_all(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        for held in self.held.drain(..).rev() {
            let result = match held {
                Held::Button(button) => self.enigo.button(button, Direction::Release),
                Held::Key(key) => self.enigo.key(key, Direction::Release),
            };
            if let Err(error) = result {
                errors.push(error.to_string());
            }
        }
        errors
    }
}

impl Drop for InputDriver {
    fn drop(&mut self) {
        let _ = self.release_all();
    }
}

fn plan<'a>(
    request: &'a Request,
    observed: &Observation,
    hwnd: HWND,
) -> Result<Vec<Step<'a>>, String> {
    let mut steps = Vec::new();
    if request.action != "drag"
        && (request.path.is_some() || request.to.is_some() || request.cancel.is_some())
    {
        return Err("Only drag accepts path, to or cancel".into());
    }
    if request.modifiers.is_some()
        && !matches!(
            request.action.as_str(),
            "click" | "double_click" | "drag" | "wheel"
        )
    {
        return Err("Pointer modifiers require click, double_click, drag or wheel".into());
    }
    match request.action.as_str() {
        "move" => {
            if request.button.is_some()
                || request.delta.is_some()
                || request.key.is_some()
                || request.text.is_some()
            {
                return Err(
                    "Pointer-only move accepts point, without button, delta, key or text".into(),
                );
            }
            let point = request
                .point
                .ok_or("Pointer input needs point in physical client pixels")?;
            steps.push(Step::Move(screen_point(point, observed, hwnd)?));
        }
        "click" | "double_click" | "drag" | "wheel" => {
            let point = request
                .point
                .ok_or("Pointer input needs point in physical client pixels")?;
            steps.push(Step::Move(screen_point(point, observed, hwnd)?));
            let modifiers = request
                .modifiers
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|modifier| match modifier.as_str() {
                    "Ctrl" => Ok(Key::Control),
                    "Shift" => Ok(Key::Shift),
                    _ => Err("Pointer modifiers must be Ctrl or Shift"),
                })
                .collect::<Result<Vec<_>, _>>()?;
            if modifiers.len() > 2
                || modifiers
                    .iter()
                    .enumerate()
                    .any(|(index, key)| modifiers[..index].contains(key))
            {
                return Err("Pointer modifiers must be unique Ctrl or Shift keys".into());
            }
            for modifier in &modifiers {
                steps.push(Step::Key(*modifier, Direction::Press));
            }
            if request.action == "wheel" {
                let delta = request
                    .delta
                    .filter(|delta| {
                        *delta != 0 && (-1200..=1200).contains(delta) && *delta % 120 == 0
                    })
                    .ok_or("Wheel delta must be a nonzero multiple of 120 from -1200 to 1200")?;
                steps.push(Step::Scroll(-delta / 120));
            } else {
                let button = match request.button.as_deref().unwrap_or("left") {
                    "left" => Button::Left,
                    "middle" => Button::Middle,
                    "right" => Button::Right,
                    _ => return Err("Mouse button must be left, middle or right".into()),
                };
                steps.push(Step::Button(button, Direction::Press));
                if request.action == "drag" {
                    let path: Vec<([i32; 2], u32)> =
                        match (&request.path, request.to) {
                            (Some(path), None) => path
                                .iter()
                                .map(|waypoint| (waypoint.point, waypoint.hold_ms))
                                .collect(),
                            (None, Some(to)) => vec![(to, 0)],
                            _ => return Err(
                                "Drag requires exactly one endpoint to or bounded waypoint path"
                                    .into(),
                            ),
                        };
                    if path.is_empty()
                        || path.len() > 8
                        || path.iter().any(|(_, hold)| *hold > 800)
                        || path.iter().map(|(_, hold)| *hold).sum::<u32>() > 1600
                    {
                        return Err("Drag accepts 1-8 waypoints, each holding 0-800ms, at most 1600ms total dwell".into());
                    }
                    let mut from = point;
                    for (to, hold) in path {
                        screen_point(to, observed, hwnd)?;
                        for step in 1..=6 {
                            let at = [
                                from[0] + ((to[0] as i64 - from[0] as i64) * step / 6) as i32,
                                from[1] + ((to[1] as i64 - from[1] as i64) * step / 6) as i32,
                            ];
                            steps.push(Step::Move(screen_point(at, observed, hwnd)?));
                            pause(&mut steps, 35);
                        }
                        pause(&mut steps, u64::from(hold));
                        from = to;
                    }
                    if request.cancel == Some(true) {
                        for modifier in modifiers.iter().rev() {
                            steps.push(Step::Key(*modifier, Direction::Release));
                        }
                        steps.push(Step::Key(Key::Escape, Direction::Press));
                        steps.push(Step::Key(Key::Escape, Direction::Release));
                        pause(&mut steps, 100);
                    }
                }
                steps.push(Step::Button(button, Direction::Release));
                if request.action == "double_click" {
                    steps.push(Step::Button(button, Direction::Press));
                    steps.push(Step::Button(button, Direction::Release));
                }
            }
            if request.cancel != Some(true) {
                for modifier in modifiers.into_iter().rev() {
                    steps.push(Step::Key(modifier, Direction::Release));
                }
            }
        }
        "key" => {
            let chord = request.key.as_deref().ok_or("Key input requires key")?;
            let mut parts: Vec<_> = chord.split('+').collect();
            let key = key_code(parts.pop().unwrap_or_default())?;
            let mut modifiers = Vec::new();
            for part in parts {
                let modifier = match part {
                    "Ctrl" => Key::Control,
                    "Shift" => Key::Shift,
                    _ => return Err("Only Ctrl and Shift key modifiers are supported".into()),
                };
                if modifiers.contains(&modifier) {
                    return Err("Duplicate key modifier".into());
                }
                modifiers.push(modifier);
                steps.push(Step::Key(modifier, Direction::Press));
            }
            steps.push(Step::Key(key, Direction::Press));
            steps.push(Step::Key(key, Direction::Release));
            for modifier in modifiers.into_iter().rev() {
                steps.push(Step::Key(modifier, Direction::Release));
            }
        }
        "text" => {
            if !observed.editable_focus {
                return Err("Observe and focus an editable CAD text control before typing".into());
            }
            let text = request.text.as_deref().ok_or("Text input requires text")?;
            if text.is_empty() || text.chars().count() > 512 || text.chars().any(char::is_control) {
                return Err("Text must contain 1-512 printable Unicode characters".into());
            }
            if !observed.native_dialog {
                if let Some(character) = text.chars().find(|character| *character as u32 > 0xffff) {
                    return Err(json!({
                        "code":"computer_control_unsupported_bevy_text",
                        "message":format!("Bevy field input does not support U+{:X}: pinned Winit 0.30.13 cannot assemble Enigo 0.6.1 surrogate packet text. No input was sent. Use printable BMP characters for this Bevy field; native dialog text supports non-BMP Unicode.", character as u32),
                        "target_kind":"bevy_window",
                        "input_sent":false,
                        "observation_consumed":true,
                    }).to_string());
                }
            }
            if observed.native_dialog {
                for (start, character) in text.char_indices() {
                    steps.push(Step::Text(&text[start..start + character.len_utf8()]));
                }
            } else {
                steps.push(Step::Text(text));
            }
        }
        _ => return Err("Unknown computer control action".into()),
    }
    Ok(steps)
}

fn pause(steps: &mut Vec<Step<'_>>, mut ms: u64) {
    while ms > 0 {
        let part = ms.min(20);
        steps.push(Step::Pause(part));
        ms -= part;
    }
}

fn key_code(key: &str) -> Result<Key, String> {
    let code = match key {
        "Enter" => Key::Return,
        "Escape" => Key::Escape,
        "Tab" => Key::Tab,
        "Backspace" => Key::Backspace,
        "Delete" => Key::Delete,
        "Insert" => Key::Insert,
        "ArrowLeft" => Key::LeftArrow,
        "ArrowRight" => Key::RightArrow,
        "ArrowUp" => Key::UpArrow,
        "ArrowDown" => Key::DownArrow,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "Space" => Key::Space,
        "F1" => Key::F1,
        "F2" => Key::F2,
        "F3" => Key::F3,
        "F4" => Key::F4,
        "F5" => Key::F5,
        "F6" => Key::F6,
        "F7" => Key::F7,
        "F8" => Key::F8,
        "F9" => Key::F9,
        "F10" => Key::F10,
        "F11" => Key::F11,
        "F12" => Key::F12,
        _ if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() => {
            Key::Other(key.as_bytes()[0].to_ascii_uppercase() as u32)
        }
        _ => {
            return Err(
                "Unsupported key; use a named navigation key, F1-F12, letter or digit".into(),
            )
        }
    };
    Ok(code)
}
