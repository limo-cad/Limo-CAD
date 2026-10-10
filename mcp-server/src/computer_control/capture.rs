//! One-frame native window capture in a bounded child of the same CAD executable.
use std::io::{Read, Write};
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{ImageEncoder, ImageEncoderPixelFormat, ImageFormat};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window;
use windows_sys::Win32::Foundation::{
    CloseHandle, FILETIME, HANDLE, HWND, RECT, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
    CREATE_NO_WINDOW, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};
use windows_sys::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
};

const REQUEST_ENV: &str = "LIMO_CAD_NATIVE_CAPTURE_WORKER";
const DEADLINE: Duration = Duration::from_secs(3);
const MAX_PIXELS: u64 = 16_777_216;
const MAX_RESPONSE: u64 = 96 * 1024 * 1024;

pub(super) struct CapturedWindow {
    pub(super) png: Vec<u8>,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) screen_bounds: [i32; 4],
    pub(super) cleanup: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct Target {
    hwnd: usize,
    pid: u32,
    process_created: u64,
    window_bounds: [i32; 4],
    frame_bounds: Option<[i32; 4]>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum Response {
    Captured {
        png_base64: String,
        width: u32,
        height: u32,
        screen_bounds: [i32; 4],
    },
    Error {
        message: String,
    },
    Cleanup {
        error: Option<String>,
    },
}

/// Capture only the already qualified CAD HWND. Library startup and shutdown
/// execute in an owned child, whose deadline cannot terminate the live desktop.
pub(super) fn png(hwnd: usize) -> Result<CapturedWindow, String> {
    let started = Instant::now();
    let target = snapshot(hwnd)?;
    let request = serde_json::to_string(&target).map_err(|error| error.to_string())?;
    let mut child = Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
        .arg("--headless")
        .env(REQUEST_ENV, request)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| format!("Could not start native capture worker: {error}"))?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            stop_child(&mut child)?;
            return Err("Native capture worker did not provide stdout".into());
        }
    };
    let reader = match thread::Builder::new()
        .name("cad-native-capture-output".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .take(MAX_RESPONSE + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| format!("Could not read native capture: {error}"))?;
            if bytes.len() as u64 > MAX_RESPONSE {
                return Err("Native capture exceeded the response limit".to_string());
            }
            Ok(bytes)
        }) {
        Ok(reader) => reader,
        Err(error) => {
            stop_child(&mut child)?;
            return Err(format!("Could not read native capture worker: {error}"));
        }
    };
    let mut stopped = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < DEADLINE => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                stopped = Some(stop_child(&mut child)?);
                break Err("Native capture exceeded its three-second deadline".to_string());
            }
            Err(error) => {
                stopped = Some(stop_child(&mut child)?);
                break Err(format!("Could not wait for native capture worker: {error}"));
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| "Native capture output reader failed".to_string())?;
    let bytes = bytes?;
    let mut replies = bytes.split_inclusive(|byte| *byte == b'\n');
    let first = replies.next().filter(|line| line.ends_with(b"\n"));
    let first = match first {
        Some(first) => first,
        None => {
            return Err(match status {
                Err(error) => error,
                Ok(status) => {
                    format!("Native capture worker exited with {status} without a completed reply")
                }
            });
        }
    };
    if snapshot(hwnd)? != target {
        return Err("Native capture target changed during capture; observe again".into());
    }
    match serde_json::from_slice::<Response>(first)
        .map_err(|error| format!("Invalid native capture response: {error}"))?
    {
        Response::Error { message } => Err(message),
        Response::Cleanup { .. } => Err("Native capture returned cleanup without an image".into()),
        Response::Captured {
            png_base64,
            width,
            height,
            screen_bounds,
        } => {
            validate_dimensions(width, height)?;
            if matching_bounds(&target, width, height)? != screen_bounds {
                return Err("Native capture returned inconsistent screen bounds".into());
            }
            let png = base64::engine::general_purpose::STANDARD
                .decode(png_base64)
                .map_err(|error| format!("Invalid native capture PNG: {error}"))?;
            if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err("Native capture did not return a PNG".into());
            }
            Ok(CapturedWindow {
                png,
                width,
                height,
                screen_bounds,
                cleanup: cleanup_status(replies.next(), &status, stopped.as_ref()),
            })
        }
    }
}

fn cleanup_status(
    reply: Option<&[u8]>,
    worker: &Result<ExitStatus, String>,
    stopped: Option<&StopOutcome>,
) -> Value {
    let session = match reply.filter(|line| line.ends_with(b"\n")) {
        Some(reply) => match serde_json::from_slice::<Response>(reply) {
            Ok(Response::Cleanup { error: None }) => json!({"status":"stopped"}),
            Ok(Response::Cleanup { error: Some(error) }) => {
                json!({"status":"stop_failed","message":error})
            }
            _ => json!({"status":"unconfirmed","message":"Invalid capture cleanup receipt"}),
        },
        None => {
            json!({"status":"unconfirmed","message":"Capture worker did not complete its cleanup receipt"})
        }
    };
    let worker = match worker {
        Ok(status) => {
            json!({"status":if status.success() {"exited"} else {"exit_failed"},"exit":status.to_string()})
        }
        Err(error) => match stopped {
            Some(StopOutcome::Terminated(status)) => {
                json!({"status":"terminated","exit":status.to_string(),"message":error})
            }
            Some(StopOutcome::Exited(status)) => {
                json!({"status":if status.success() {"exited"} else {"exit_failed"},"exit":status.to_string(),"wait_error":error})
            }
            None => json!({"status":"unconfirmed","message":error}),
        },
    };
    json!({"session":session,"worker":worker,"deadline_ms":DEADLINE.as_millis()})
}

enum StopOutcome {
    Exited(ExitStatus),
    Terminated(ExitStatus),
}

fn stop_child(child: &mut Child) -> Result<StopOutcome, String> {
    if let Some(status) = child
        .try_wait()
        .map_err(|error| format!("Could not inspect native capture worker: {error}"))?
    {
        return Ok(StopOutcome::Exited(status));
    }
    if let Err(error) = child.kill() {
        return match child.try_wait() {
            Ok(Some(status)) => Ok(StopOutcome::Exited(status)),
            _ => Err(format!(
                "Could not terminate owned native capture worker: {error}"
            )),
        };
    }
    if unsafe { WaitForSingleObject(child.as_raw_handle() as HANDLE, 500) } != WAIT_OBJECT_0 {
        return Err(
            "Windows did not confirm native capture worker termination within 500ms".into(),
        );
    }
    child
        .wait()
        .map(StopOutcome::Terminated)
        .map_err(|error| format!("Could not reap native capture worker: {error}"))
}

/// Called before MCP initialization by the same executable's --headless path.
/// The request bypasses kernel/document creation and returns only capture data.
pub(crate) fn run_worker_if_requested() -> Option<Result<(), String>> {
    let request = std::env::var_os(REQUEST_ENV)?;
    std::env::remove_var(REQUEST_ENV);
    let result = request
        .to_str()
        .ok_or_else(|| "Native capture request is not Unicode".to_string())
        .and_then(|request| {
            serde_json::from_str::<Target>(request)
                .map_err(|error| format!("Invalid native capture request: {error}"))
        })
        .and_then(capture);
    Some(match result {
        Ok(()) => Ok(()),
        Err(message) => publish(&Response::Error { message }),
    })
}

fn publish(response: &Response) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, response)
        .map_err(|error| format!("Could not write native capture: {error}"))?;
    stdout.write_all(b"\n").map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())
}

fn capture(target: Target) -> Result<(), String> {
    if snapshot(target.hwnd)? != target {
        return Err("Native capture target changed before capture".into());
    }
    let (sender, receiver) = mpsc::channel();
    let settings = Settings::new(
        Window::from_raw_hwnd(target.hwnd as *mut std::ffi::c_void),
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        sender,
    );
    let control = OneFrame::start_free_threaded(settings)
        .map_err(|error| format!("Could not start Windows capture: {error}"))?;
    let frame = receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|error| format!("Windows did not provide a capture frame: {error}"))
        .flatten()
        .and_then(|(png, width, height)| {
            if snapshot(target.hwnd)? != target {
                return Err("Native capture target changed while capturing".into());
            }
            let screen_bounds = matching_bounds(&target, width, height)?;
            Ok(Response::Captured {
                png_base64: base64::engine::general_purpose::STANDARD.encode(png),
                width,
                height,
                screen_bounds,
            })
        });
    let response = match frame {
        Ok(response) => response,
        Err(message) => Response::Error { message },
    };
    let published = publish(&response);
    let stopped = control
        .stop()
        .map_err(|error| format!("Could not stop Windows capture: {error}"));
    published?;
    publish(&Response::Cleanup {
        error: stopped.err(),
    })
}

type EncodedFrame = Result<(Vec<u8>, u32, u32), String>;

struct OneFrame {
    sender: Option<Sender<EncodedFrame>>,
}

impl GraphicsCaptureApiHandler for OneFrame {
    type Flags = Sender<EncodedFrame>;
    type Error = String;

    fn new(context: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            sender: Some(context.flags),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame<'_>,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if let Some(sender) = self.sender.take() {
            let result = encode_frame(frame);
            let _ = sender.send(result);
        }
        control.stop();
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Err(
                "Native capture target closed before its first frame".into()
            ));
        }
        Ok(())
    }
}

fn encode_frame(frame: &mut Frame<'_>) -> EncodedFrame {
    let (width, height) = (frame.width(), frame.height());
    validate_dimensions(width, height)?;
    if frame.color_format() != ColorFormat::Rgba8 {
        return Err("Native capture returned an unexpected pixel format".into());
    }
    let buffer = frame.buffer().map_err(|error| error.to_string())?;
    let mut packed = Vec::new();
    let rgba = buffer.as_nopadding_buffer(&mut packed);
    if rgba.len() as u64 != u64::from(width) * u64::from(height) * 4 {
        return Err("Native capture returned an incomplete pixel buffer".into());
    }
    let bytes = ImageEncoder::new(ImageFormat::Png, ImageEncoderPixelFormat::Rgba8)
        .and_then(|encoder| encoder.encode(rgba, width, height))
        .map_err(|error| format!("Could not encode native capture PNG: {error}"))?;
    Ok((bytes, width, height))
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        Err("Native capture dimensions exceed the supported window size".into())
    } else {
        Ok(())
    }
}

fn matching_bounds(target: &Target, width: u32, height: u32) -> Result<[i32; 4], String> {
    [target.frame_bounds, Some(target.window_bounds)]
        .into_iter()
        .flatten()
        .find(|bounds| {
            i64::from(bounds[2]) == i64::from(width) && i64::from(bounds[3]) == i64::from(height)
        })
        .ok_or_else(|| {
            "Native capture dimensions do not match verified physical window bounds".into()
        })
}

fn snapshot(hwnd: usize) -> Result<Target, String> {
    let _dpi = DpiContext::enter()?;
    let hwnd = hwnd as HWND;
    if unsafe { IsWindow(hwnd) } == 0
        || unsafe { IsWindowVisible(hwnd) } == 0
        || unsafe { IsIconic(hwnd) } != 0
    {
        return Err("Native capture requires an existing visible CAD window".into());
    }
    let mut pid = 0;
    if unsafe { GetWindowThreadProcessId(hwnd, &mut pid) } == 0 || pid == 0 {
        return Err("Could not identify the native capture owner".into());
    }
    let process = Process(unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    });
    if process.0.is_null() || unsafe { WaitForSingleObject(process.0, 0) } != WAIT_TIMEOUT {
        return Err("Native capture owner is unavailable".into());
    }
    let mut path = vec![0u16; 32768];
    let mut length = path.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process.0, 0, path.as_mut_ptr(), &mut length) } == 0 {
        return Err("Could not verify native capture executable".into());
    }
    let path = PathBuf::from(
        String::from_utf16(&path[..length as usize]).map_err(|error| error.to_string())?,
    );
    let owner_path = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let own_path = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|error| error.to_string())?;
    if !owner_path
        .to_string_lossy()
        .eq_ignore_ascii_case(&own_path.to_string_lossy())
    {
        return Err("Native capture owner must use this exact CAD executable".into());
    }
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetProcessTimes(process.0, &mut created, &mut exited, &mut kernel, &mut user) } == 0
    {
        return Err("Could not verify native capture process lifetime".into());
    }
    let mut window = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut window) } == 0 {
        return Err("Could not read native capture window bounds".into());
    }
    let window_bounds = bounds(window)?;
    validate_dimensions(window_bounds[2] as u32, window_bounds[3] as u32)?;
    let mut frame = RECT::default();
    let frame_bounds = if unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut frame as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
    } >= 0
    {
        Some(bounds(frame)?)
    } else {
        None
    };
    Ok(Target {
        hwnd: hwnd as usize,
        pid,
        process_created: (u64::from(created.dwHighDateTime) << 32)
            | u64::from(created.dwLowDateTime),
        window_bounds,
        frame_bounds,
    })
}

fn bounds(rect: RECT) -> Result<[i32; 4], String> {
    let width = rect.right.checked_sub(rect.left);
    let height = rect.bottom.checked_sub(rect.top);
    match (width, height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            Ok([rect.left, rect.top, width, height])
        }
        _ => Err("Native capture window has invalid bounds".into()),
    }
}

struct Process(HANDLE);

impl Drop for Process {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

struct DpiContext(DPI_AWARENESS_CONTEXT);

impl DpiContext {
    fn enter() -> Result<Self, String> {
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if previous.is_null() {
            Err("Could not enter physical-pixel native capture coordinates".into())
        } else {
            Ok(Self(previous))
        }
    }
}

impl Drop for DpiContext {
    fn drop(&mut self) {
        unsafe { SetThreadDpiAwarenessContext(self.0) };
    }
}
