//! Opt-in, own-window message evidence before Winit keyboard assembly.
//! Never records text, consumes a message, or certifies input delivery.
use std::cell::RefCell;

use bevy::prelude::Entity;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde_json::{json, Value};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GetWindowThreadProcessId, WM_KEYDOWN, WM_KEYUP, WM_NCDESTROY, WM_SYSKEYDOWN,
            WM_SYSKEYUP,
        },
    },
};

const LIMIT: usize = 32;
const SUBCLASS_ID: usize = 0x4c434b44;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Record {
    sequence: u64,
    message: u32,
    key: u32,
    lparam: u32,
}

struct Trace {
    hwnd: usize,
    thread: u32,
    sequence: u64,
    records: [Option<Record>; LIMIT],
}

impl Trace {
    fn new(hwnd: usize, thread: u32) -> Self {
        Self {
            hwnd,
            thread,
            sequence: 0,
            records: [None; LIMIT],
        }
    }

    fn record(&mut self, hwnd: usize, message: u32, key: usize, lparam: isize) {
        if hwnd != self.hwnd
            || !matches!(message, WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP)
            || !matches!(key, 0x4e | 0x4f | 0x53 | 0x57 | 0x50 | 0x10..=0x12 | 0x5b..=0x5c | 0xa0..=0xa5)
        {
            return;
        }
        self.sequence = self.sequence.saturating_add(1);
        self.records.rotate_left(1);
        self.records[LIMIT - 1] = Some(Record {
            sequence: self.sequence,
            message,
            key: key as u32,
            lparam: lparam as u32,
        });
    }
}

thread_local! {
    // No pointer to Rust state is handed to Win32. Callback storage is bounded,
    // allocation-free and accessed only on the HWND's UI thread.
    static TRACE: RefCell<Option<Trace>> = const { RefCell::new(None) };
}

fn observe_message(hwnd: usize, message: u32, key: usize, lparam: isize) {
    let _ = TRACE.try_with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            if message == WM_NCDESTROY {
                if slot.as_ref().is_some_and(|trace| trace.hwnd == hwnd) {
                    *slot = None;
                }
            } else if let Some(trace) = slot.as_mut() {
                trace.record(hwnd, message, key, lparam);
            }
        }
    });
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    observe_message(hwnd.0 as usize, message, wparam.0, lparam.0);
    if message == WM_NCDESTROY {
        // Win32 requires this on destruction; no Rust borrow survives forwarding.
        let _ = unsafe { RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID) };
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

fn owned_hwnd(entity: Entity) -> Option<(HWND, u32)> {
    bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        let window = windows.get_window(entity)?;
        let handle = window.window_handle().ok()?;
        let RawWindowHandle::Win32(raw) = handle.as_raw() else {
            return None;
        };
        let hwnd = HWND(raw.hwnd.get() as *mut std::ffi::c_void);
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        (thread == unsafe { GetCurrentThreadId() } && pid == std::process::id())
            .then_some((hwnd, thread))
    })
}

/// Called on the existing native input system's UI thread. Registration is
/// confined to the current primary HWND; replacing it retires previous evidence.
pub(super) fn reconcile(entity: Option<Entity>) {
    let enabled = std::env::var("LIMO_CAD_FILE_SHORTCUT_DIAGNOSTICS").as_deref() == Ok("1");
    let target = entity.filter(|_| enabled).and_then(owned_hwnd);
    let old = TRACE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|trace| {
            target.is_some_and(|(hwnd, thread)| {
                trace.hwnd == hwnd.0 as usize && trace.thread == thread
            })
        }) {
            return None;
        }
        slot.take()
    });
    if let Some(old) = old {
        let hwnd = HWND(old.hwnd as *mut std::ffi::c_void);
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if thread == unsafe { GetCurrentThreadId() } && pid == std::process::id() {
            let _ = unsafe { RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID) };
        }
    }
    let Some((hwnd, thread)) = target else {
        return;
    };
    // Already registered traces are left intact, including their bounded history.
    let present = TRACE.with(|slot| slot.borrow().is_some());
    if present {
        return;
    }
    if unsafe { SetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, 0) }.as_bool() {
        TRACE.with(|slot| *slot.borrow_mut() = Some(Trace::new(hwnd.0 as usize, thread)));
    }
}

pub(crate) fn snapshot(hwnd: usize) -> Value {
    TRACE.with(|slot| {
        let slot = slot.borrow();
        let Some(trace) = slot.as_ref().filter(|trace| trace.hwnd == hwnd) else {
            return Value::Null;
        };
        let events: Vec<_> = trace.records.iter().flatten().map(|record| json!({
            "sequence":record.sequence,"message":record.message,"virtual_key":record.key,
            "lparam_bits":record.lparam,"repeat_count":record.lparam & 0xffff,
            "scan_code":(record.lparam >> 16) & 0xff,
            "extended":record.lparam & (1 << 24) != 0,
            "alt_context":record.lparam & (1 << 29) != 0,
            "previous_state":record.lparam & (1 << 30) != 0,
            "transition_state":record.lparam & (1 << 31) != 0
        })).collect();
        json!({"hwnd":trace.hwnd,"window_thread":trace.thread,"pid":std::process::id(),
            "event_limit":LIMIT,"sequence":trace.sequence,"events":events,
            "source":"Own CAD HWND messages before Winit assembly; every message forwarded unchanged. Only N/O/S/W/P and modifier virtual-key messages, including unmodified keys; no character-message payloads. This does not prove shortcut dispatch or physical keyboard behavior."})
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_key_trace_bounds_history_excludes_text_and_retires_exact_window() {
        TRACE.with(|slot| *slot.borrow_mut() = Some(Trace::new(10, 20)));
        // Exercise the exact callback observation boundary, including foreign
        // HWNDs, unknown keys and WM_CHAR carrying a file-command letter.
        observe_message(11, WM_KEYDOWN, 0x53, 1);
        observe_message(10, 0x0102, 0x53, 1);
        observe_message(10, WM_KEYDOWN, 0x41, 1);
        assert_eq!(snapshot(10)["sequence"], 0);
        for _ in 0..40 {
            observe_message(10, WM_KEYDOWN, 0x53, 0x001f0001);
        }
        observe_message(10, WM_KEYUP, 0x53, 0xc01f0001u32 as isize);
        let trace = snapshot(10);
        let events = trace["events"].as_array().unwrap();
        assert_eq!(events.len(), LIMIT);
        assert_eq!(events[0]["sequence"], 10);
        assert_eq!(events[31]["previous_state"], true);
        assert_eq!(events[31]["transition_state"], true);
        assert_eq!(events[31]["scan_code"], 0x1f);
        assert!(snapshot(11).is_null());
        observe_message(11, WM_NCDESTROY, 0, 0);
        assert_eq!(snapshot(10)["sequence"], 41);
        observe_message(10, WM_NCDESTROY, 0, 0);
        assert!(snapshot(10).is_null());
        TRACE.with(|slot| *slot.borrow_mut() = Some(Trace::new(11, 20)));
        observe_message(10, WM_KEYDOWN, 0x53, 1);
        assert_eq!(snapshot(11)["sequence"], 0);
        observe_message(11, WM_NCDESTROY, 0, 0);
    }
}
