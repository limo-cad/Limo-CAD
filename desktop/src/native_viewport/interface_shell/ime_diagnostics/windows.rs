//! Read the actual Winit window's input context. Never activates a source or
//! changes an IMM/TSF context; enabled only by the disposable-runner observer.
use bevy::prelude::Entity;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde_json::{json, Value};
use windows::Win32::{
    Foundation::HWND,
    System::{
        Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        Threading::GetCurrentThreadId,
    },
    UI::{
        Input::{
            Ime::{
                ImmGetContext, ImmGetConversionStatus, ImmGetOpenStatus, ImmReleaseContext,
                IME_CONVERSION_MODE, IME_SENTENCE_MODE,
            },
            KeyboardAndMouse::{GetFocus, GetKeyboardLayout},
        },
        TextServices::{
            CLSID_TF_InputProcessorProfiles, ITfInputProcessorProfileMgr, GUID_TFCAT_TIP_KEYBOARD,
            TF_INPUTPROCESSORPROFILE,
        },
        WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    },
};

pub(super) fn input_context(entity: Entity) -> Value {
    bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        let Some(window) = windows.get_window(entity) else { return Value::Null; };
        let Ok(handle) = window.window_handle() else { return Value::Null; };
        let RawWindowHandle::Win32(raw) = handle.as_raw() else { return Value::Null; };
        let hwnd = HWND(raw.hwnd.get() as *mut std::ffi::c_void);


        unsafe {
            let mut owner = 0;
            let thread = GetWindowThreadProcessId(hwnd, Some(&mut owner));
            if thread != GetCurrentThreadId() || owner != std::process::id() {
                return json!({"error":"input context sampled outside the owned window thread",
                    "window":raw.hwnd.get(), "window_thread":thread, "pid":owner});
            }
            let layout = GetKeyboardLayout(thread);
            let context = ImmGetContext(hwnd);
            let mut conversion = IME_CONVERSION_MODE::default();
            let mut sentence = IME_SENTENCE_MODE::default();
            let imm = if context.0.is_null() {
                json!({"context_present":false})
            } else {
                let conversion_read = ImmGetConversionStatus(context, Some(&mut conversion), Some(&mut sentence)).as_bool();
                let open = ImmGetOpenStatus(context).as_bool();
                let released = ImmReleaseContext(hwnd, context).as_bool();
                json!({"context_present":true, "open":open, "conversion_read":conversion_read,
                    "conversion":conversion.0, "sentence":sentence.0, "context_released":released})
            };
            let manager: windows::core::Result<ITfInputProcessorProfileMgr> =
                CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER);
            let profile = match manager {
                Ok(manager) => {
                    let mut profile = TF_INPUTPROCESSORPROFILE::default();
                    match manager.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut profile) {
                        Ok(()) => json!({"type":profile.dwProfileType, "language":profile.langid,
                            "class_id":format!("{:?}", profile.clsid), "profile_id":format!("{:?}", profile.guidProfile),
                            "flags":profile.dwFlags}),
                        Err(error) => json!({"error":error.to_string(), "hresult":format!("{:08X}", error.code().0)}),
                    }
                }
                Err(error) => json!({"error":error.to_string(), "hresult":format!("{:08X}", error.code().0)}),
            };
            json!({"window":raw.hwnd.get(), "window_thread":thread, "pid":owner,
                "foreground":GetForegroundWindow() == hwnd, "focused":GetFocus() == hwnd,
                "layout":layout.0 as usize, "language":layout.0 as usize & 0xffff,
                "imm":imm, "active_profile":profile})
        }
    })
}
