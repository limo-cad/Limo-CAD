//! Cross-platform 3Dconnexion adapter. macOS prefers the installed 3DxWare
//! client framework so the official driver remains the sole owner of the
//! physical device; raw HID stays available as the Windows/macOS fallback.
//! Both transports deliver the same typed events to a host-owned sink. The
//! native controller routes those events to the focused document camera.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use serde::Serialize;

mod raw;
#[cfg(test)]
pub(crate) use raw::motion_from_report;

const CURRENT_VENDOR_ID: u16 = 0x256f;
const LEGACY_VENDOR_ID: u16 = 0x046d;
const GENERIC_DESKTOP_USAGE_PAGE: u16 = 0x01;
const MULTI_AXIS_CONTROLLER_USAGE: u16 = 0x08;
const RAW_HID_EMIT_INTERVAL: Duration = Duration::from_millis(16);
const RAW_HID_READ_TIMEOUT_MS: i32 = 8;

#[derive(Debug, Clone, Serialize)]
pub struct SixDofMouseInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub product_name: String,
    pub serial_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MotionPacket {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation: Option<[i16; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<[i16; 3]>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub(crate) struct ButtonPacket {
    pub button: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SixDofEvent {
    Motion(MotionPacket),
    Button(ButtonPacket),
    Error(String),
}

/// Called on the HID worker or installed-driver callback thread. A sink must
/// enqueue promptly and must not call connect/disconnect from that callback.
/// Its host owns focus/document gating and camera updates; transport packets
/// keep the existing device axis order and scale.
pub(crate) type SixDofEventSink = Arc<dyn Fn(SixDofEvent) + Send + Sync>;

struct RawHidWorker {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for RawHidWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

enum SixDofConnection {
    RawHid(RawHidWorker),
    #[cfg(target_os = "macos")]
    InstalledDriver(mac_driver::Connection),
}

impl SixDofConnection {
    fn close(self) {
        match self {
            Self::RawHid(worker) => drop(worker),
            #[cfg(target_os = "macos")]
            Self::InstalledDriver(connection) => drop(connection),
        }
    }
}

#[derive(Default)]
pub struct SixDofMouseState {
    operation: Mutex<()>,
    connection: Mutex<Option<SixDofConnection>>,
}

impl SixDofMouseState {
    /// Opens a device only for an explicit host connection request. HID
    /// discovery, driver registration and replacing an old connection can
    /// block; a native host should call this from its connection worker.
    pub(crate) fn connect(&self, sink: SixDofEventSink) -> Result<SixDofMouseInfo, String> {
        self.connect_with(|| open_connection(sink))
    }

    fn connect_with(
        &self,
        open: impl FnOnce() -> Result<(SixDofConnection, SixDofMouseInfo), String>,
    ) -> Result<SixDofMouseInfo, String> {
        let _operation = self
            .operation
            .lock()
            .map_err(|_| "six-dof mouse operation mutex poisoned".to_string())?;
        self.stop()?;
        let (connection, info) = open()?;
        *self
            .connection
            .lock()
            .map_err(|_| "six-dof mouse worker mutex poisoned".to_string())? = Some(connection);
        Ok(info)
    }

    /// Stops delivery and joins the raw worker before returning. As with
    /// connect, call from a host worker rather than the render/input thread.
    pub(crate) fn disconnect(&self) -> Result<(), String> {
        let _operation = self
            .operation
            .lock()
            .map_err(|_| "six-dof mouse operation mutex poisoned".to_string())?;
        self.stop()
    }

    fn stop(&self) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "six-dof mouse worker mutex poisoned".to_string())?
            .take();
        if let Some(connection) = connection {
            connection.close();
        }
        Ok(())
    }
}

impl Drop for SixDofMouseState {
    fn drop(&mut self) {
        let connection = self
            .connection
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(connection) = connection {
            connection.close();
        }
    }
}

fn supported_descriptor(vendor_id: u16, usage_page: u16, usage: u16, product_name: &str) -> bool {
    if usage_page != GENERIC_DESKTOP_USAGE_PAGE || usage != MULTI_AXIS_CONTROLLER_USAGE {
        return false;
    }
    if vendor_id == CURRENT_VENDOR_ID {
        return true;
    }
    if vendor_id != LEGACY_VENDOR_ID {
        return false;
    }
    let product = product_name.to_ascii_lowercase();
    product.contains("space") || product.contains("3dconnexion") || product.contains("cadman")
}

fn supported_device(info: &hidapi::DeviceInfo) -> bool {
    supported_descriptor(
        info.vendor_id(),
        info.usage_page(),
        info.usage(),
        info.product_string().unwrap_or_default(),
    )
}

fn vector(data: &[u8], offset: usize) -> Option<[i16; 3]> {
    if data.len() < offset + 6 {
        return None;
    }
    Some([
        i16::from_le_bytes([data[offset], data[offset + 1]]),
        i16::from_le_bytes([data[offset + 2], data[offset + 3]]),
        i16::from_le_bytes([data[offset + 4], data[offset + 5]]),
    ])
}

fn open_connection(sink: SixDofEventSink) -> Result<(SixDofConnection, SixDofMouseInfo), String> {
    #[cfg(target_os = "macos")]
    let installed_driver_error = match mac_driver::Connection::connect(sink.clone()) {
        Ok((connection, info)) => {
            return Ok((SixDofConnection::InstalledDriver(connection), info));
        }
        Err(error) => Some(error),
    };
    let api = hidapi::HidApi::new().map_err(|error| error.to_string())?;
    let info = api
        .device_list()
        .find(|candidate| supported_device(candidate))
        .ok_or_else(|| {
            #[cfg(target_os = "macos")]
            {
                format!(
                    "The installed 3Dconnexion driver could not be used ({}) and no raw multi-axis device was found.",
                    installed_driver_error.as_deref().unwrap_or("unknown error"),
                )
            }
            #[cfg(not(target_os = "macos"))]
            {
                "No supported 3D mouse was found.".to_string()
            }
        })?;
    let result = SixDofMouseInfo {
        vendor_id: info.vendor_id(),
        product_id: info.product_id(),
        product_name: info.product_string().unwrap_or("3D mouse").to_string(),
        serial_number: info.serial_number().map(str::to_string),
    };
    let device = info.open_device(&api).map_err(|error| error.to_string())?;
    let worker = raw::spawn(
        move |buffer, timeout| {
            device
                .read_timeout(buffer, timeout)
                .map_err(|error| error.to_string())
        },
        sink,
    )?;
    Ok((SixDofConnection::RawHid(worker), result))
}

#[cfg(target_os = "macos")]
mod mac_driver {
    use std::ffi::{c_char, c_int, c_void, CStr, CString};
    use std::sync::{Mutex, OnceLock};

    use super::*;

    const FRAMEWORK_PATH: &str =
        "/Library/Frameworks/3DconnexionClient.framework/3DconnexionClient";
    const RTLD_NOW: c_int = 0x2;
    const CONNEXION_MESSAGE_DEVICE_STATE: u32 = 0x3364_5352;
    const CONNEXION_COMMAND_HANDLE_BUTTONS: u16 = 2;
    const CONNEXION_COMMAND_HANDLE_AXIS: u16 = 3;
    const CONNEXION_CLIENT_MODE_TAKE_OVER: u16 = 1;
    const CONNEXION_MASK_ALL: u32 = 0x3fff;
    const CONNEXION_MASK_ALL_BUTTONS: u32 = 0xffff_ffff;
    const LIMO_CAD_SIGNATURE: u32 = u32::from_be_bytes(*b"NBCD");

    type MessageHandler =
        unsafe extern "C" fn(product_id: u32, message_type: u32, argument: *mut c_void);
    type AddedHandler = unsafe extern "C" fn(product_id: u32);
    type RemovedHandler = unsafe extern "C" fn(product_id: u32);
    type SetHandlers = unsafe extern "C" fn(
        Option<MessageHandler>,
        Option<AddedHandler>,
        Option<RemovedHandler>,
        bool,
    ) -> i16;
    type CleanupHandlers = unsafe extern "C" fn();
    type RegisterClient =
        unsafe extern "C" fn(signature: u32, name: *mut u8, mode: u16, mask: u32) -> u16;
    type SetButtonMask = unsafe extern "C" fn(client_id: u16, mask: u32);
    type UnregisterClient = unsafe extern "C" fn(client_id: u16);

    extern "C" {
        fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
        fn dlerror() -> *const c_char;
    }

    #[repr(C, packed(2))]
    #[derive(Clone, Copy)]
    struct ConnexionDeviceState {
        version: u16,
        client: u16,
        command: u16,
        param: i16,
        value: i32,
        time: u64,
        report: [u8; 8],
        buttons8: u16,
        axis: [i16; 6],
        address: u16,
        buttons: u32,
    }

    impl ConnexionDeviceState {
        fn buttons(&self) -> u32 {
            unsafe { std::ptr::addr_of!(self.buttons).read_unaligned() }
        }
    }

    unsafe fn copy_device_state(argument: *const c_void) -> Option<ConnexionDeviceState> {
        if argument.is_null() {
            return None;
        }

        Some(unsafe { std::ptr::read_unaligned(argument.cast::<ConnexionDeviceState>()) })
    }

    struct CallbackState {
        sink: SixDofEventSink,
        client_id: u16,
        previous_buttons: u32,
    }

    static CALLBACK_STATE: OnceLock<Mutex<Option<CallbackState>>> = OnceLock::new();

    fn callback_state() -> &'static Mutex<Option<CallbackState>> {
        CALLBACK_STATE.get_or_init(|| Mutex::new(None))
    }

    fn canonical_motion(device: &ConnexionDeviceState) -> MotionPacket {
        let axis = device.axis;
        MotionPacket {
            translation: Some([axis[0], axis[1], axis[2]]),
            rotation: Some([axis[3], axis[4], axis[5]]),
        }
    }

    unsafe extern "C" fn message_handler(
        _product_id: u32,
        message_type: u32,
        argument: *mut c_void,
    ) {
        if message_type != CONNEXION_MESSAGE_DEVICE_STATE {
            return;
        }
        let Some(device) = (unsafe { copy_device_state(argument) }) else {
            return;
        };
        let Ok(mut guard) = callback_state().lock() else {
            return;
        };
        let Some(callback) = guard.as_mut() else {
            return;
        };
        callback.receive(&device);
    }

    impl CallbackState {
        fn receive(&mut self, device: &ConnexionDeviceState) {
            if device.client != self.client_id {
                return;
            }
            if device.command == CONNEXION_COMMAND_HANDLE_AXIS {
                (self.sink)(SixDofEvent::Motion(canonical_motion(device)));
            }
            let buttons = device.buttons();
            if device.command == CONNEXION_COMMAND_HANDLE_BUTTONS
                || buttons != self.previous_buttons
            {
                let newly_pressed = buttons & !self.previous_buttons;
                self.previous_buttons = buttons;
                for index in 0..32 {
                    if newly_pressed & (1 << index) != 0 {
                        (self.sink)(SixDofEvent::Button(ButtonPacket { button: index + 1 }));
                    }
                }
            }
        }
    }

    unsafe fn last_dl_error() -> String {
        let error = unsafe { dlerror() };
        if error.is_null() {
            "unknown dynamic-loader error".to_string()
        } else {
            unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned()
        }
    }

    unsafe fn load_symbol<T: Copy>(handle: *mut c_void, name: &'static [u8]) -> Result<T, String> {
        let symbol = unsafe { dlsym(handle, name.as_ptr().cast()) };
        if symbol.is_null() {
            let label = CStr::from_bytes_with_nul(name)
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "unknown symbol".to_string());
            return Err(format!(
                "3Dconnexion driver symbol {label} is unavailable: {}",
                unsafe { last_dl_error() },
            ));
        }
        Ok(unsafe { std::mem::transmute_copy(&symbol) })
    }

    struct Api {
        handle: usize,
        set_handlers: SetHandlers,
        cleanup_handlers: CleanupHandlers,
        register_client: RegisterClient,
        set_button_mask: SetButtonMask,
        unregister_client: UnregisterClient,
    }

    impl Api {
        fn load() -> Result<Self, String> {
            let path = CString::new(FRAMEWORK_PATH).expect("framework path contains no NUL");
            let handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW) };
            if handle.is_null() {
                return Err(format!(
                    "3Dconnexion driver framework is unavailable: {}",
                    unsafe { last_dl_error() },
                ));
            }
            let symbols = unsafe {
                (|| {
                    Ok(Self {
                        handle: handle as usize,
                        set_handlers: load_symbol(handle, b"SetConnexionHandlers\0")?,
                        cleanup_handlers: load_symbol(handle, b"CleanupConnexionHandlers\0")?,
                        register_client: load_symbol(handle, b"RegisterConnexionClient\0")?,
                        set_button_mask: load_symbol(handle, b"SetConnexionClientButtonMask\0")?,
                        unregister_client: load_symbol(handle, b"UnregisterConnexionClient\0")?,
                    })
                })()
            };
            if symbols.is_err() {
                unsafe {
                    dlclose(handle);
                }
            }
            symbols
        }
    }

    pub struct Connection {
        api: Api,
        client_id: u16,
    }

    impl Connection {
        pub fn connect(sink: SixDofEventSink) -> Result<(Self, SixDofMouseInfo), String> {
            let api = Api::load()?;
            {
                let mut callback = callback_state()
                    .lock()
                    .map_err(|_| "3Dconnexion callback mutex poisoned".to_string())?;
                if callback.is_some() {
                    unsafe {
                        dlclose(api.handle as *mut c_void);
                    }
                    return Err("A 3Dconnexion driver client is already active.".to_string());
                }
                *callback = Some(CallbackState {
                    sink,
                    client_id: 0,
                    previous_buttons: 0,
                });
            }

            let handler_result =
                unsafe { (api.set_handlers)(Some(message_handler), None, None, true) };
            if handler_result != 0 {
                *callback_state()
                    .lock()
                    .map_err(|_| "3Dconnexion callback mutex poisoned".to_string())? = None;
                unsafe {
                    dlclose(api.handle as *mut c_void);
                }
                return Err(format!(
                    "3Dconnexion driver rejected its message handler ({handler_result})."
                ));
            }

            let mut name = b"Limo CAD\0".to_vec();
            let client_id = unsafe {
                (api.register_client)(
                    LIMO_CAD_SIGNATURE,
                    name.as_mut_ptr(),
                    CONNEXION_CLIENT_MODE_TAKE_OVER,
                    CONNEXION_MASK_ALL,
                )
            };
            if client_id == 0 {
                unsafe {
                    (api.cleanup_handlers)();
                }
                *callback_state()
                    .lock()
                    .map_err(|_| "3Dconnexion callback mutex poisoned".to_string())? = None;
                unsafe {
                    dlclose(api.handle as *mut c_void);
                }
                return Err("3Dconnexion driver did not register Limo CAD.".to_string());
            }
            unsafe {
                (api.set_button_mask)(client_id, CONNEXION_MASK_ALL_BUTTONS);
            }
            if let Some(callback) = callback_state()
                .lock()
                .map_err(|_| "3Dconnexion callback mutex poisoned".to_string())?
                .as_mut()
            {
                callback.client_id = client_id;
            }
            eprintln!("3Dconnexion installed-driver client registered (client_id={client_id})");

            Ok((
                Self { api, client_id },
                SixDofMouseInfo {
                    vendor_id: CURRENT_VENDOR_ID,
                    product_id: 0,
                    product_name: "3Dconnexion SpaceMouse (installed driver)".to_string(),
                    serial_number: None,
                },
            ))
        }
    }

    impl Drop for Connection {
        fn drop(&mut self) {
            unsafe {
                (self.api.unregister_client)(self.client_id);
                (self.api.cleanup_handlers)();
            }
            if let Ok(mut callback) = callback_state().lock() {
                *callback = None;
            }
            unsafe {
                dlclose(self.api.handle as *mut c_void);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn installed_driver_dispatches_only_its_client_and_preserves_button_edges() {
            let events = Arc::new(Mutex::new(Vec::new()));
            let output = events.clone();
            let mut callback = CallbackState {
                sink: Arc::new(move |event| output.lock().unwrap().push(event)),
                client_id: 17,
                previous_buttons: 0,
            };
            let mut device = ConnexionDeviceState {
                version: 0,
                client: 18,
                command: CONNEXION_COMMAND_HANDLE_AXIS,
                param: 0,
                value: 0,
                time: 0,
                report: [0; 8],
                buttons8: 0,
                axis: [10, -20, 30, -40, 50, -60],
                address: 0,
                buttons: 0x8000_0001,
            };
            callback.receive(&device);
            assert!(events.lock().unwrap().is_empty());
            device.client = 17;
            callback.receive(&device);
            device.command = CONNEXION_COMMAND_HANDLE_BUTTONS;
            callback.receive(&device);
            device.buttons = 0;
            callback.receive(&device);
            device.buttons = 1;
            callback.receive(&device);
            assert_eq!(
                *events.lock().unwrap(),
                vec![
                    SixDofEvent::Motion(MotionPacket {
                        translation: Some([10, -20, 30]),
                        rotation: Some([-40, 50, -60])
                    }),
                    SixDofEvent::Button(ButtonPacket { button: 1 }),
                    SixDofEvent::Button(ButtonPacket { button: 32 }),
                    SixDofEvent::Button(ButtonPacket { button: 1 }),
                ]
            );
        }

        #[test]
        fn connexion_device_state_matches_the_installed_framework_abi() {
            assert_eq!(std::mem::size_of::<ConnexionDeviceState>(), 48);
            assert_eq!(std::mem::align_of::<ConnexionDeviceState>(), 2);
            assert_eq!(std::mem::offset_of!(ConnexionDeviceState, version), 0);
            assert_eq!(std::mem::offset_of!(ConnexionDeviceState, client), 2);
            assert_eq!(std::mem::offset_of!(ConnexionDeviceState, command), 4);
            assert_eq!(std::mem::offset_of!(ConnexionDeviceState, param), 6);
            assert_eq!(std::mem::offset_of!(ConnexionDeviceState, value), 8);
            assert_eq!(std::mem::offset_of!(ConnexionDeviceState, time), 12);
            assert_eq!(std::mem::offset_of!(ConnexionDeviceState, report), 20);
            assert_eq!(std::mem::offset_of!(ConnexionDeviceState, buttons8), 28);
            assert_eq!(std::mem::offset_of!(ConnexionDeviceState, axis), 30);
            assert_eq!(std::mem::offset_of!(ConnexionDeviceState, address), 42);
            assert_eq!(std::mem::offset_of!(ConnexionDeviceState, buttons), 44);
        }

        #[test]
        fn mac_driver_preserves_the_documented_axis_order() {
            let device = ConnexionDeviceState {
                version: 0,
                client: 0,
                command: CONNEXION_COMMAND_HANDLE_AXIS,
                param: 0,
                value: 0,
                time: 0,
                report: [0; 8],
                buttons8: 0,
                axis: [10, 20, 30, 40, 50, 60],
                address: 0,
                buttons: 0,
            };
            assert_eq!(
                canonical_motion(&device),
                MotionPacket {
                    translation: Some([10, 20, 30]),
                    rotation: Some([40, 50, 60]),
                }
            );
        }

        #[test]
        fn mac_driver_copies_misaligned_callback_records_before_reading_them() {
            let mut storage = [0_u8; std::mem::size_of::<ConnexionDeviceState>() + 2];
            let base = storage.as_mut_ptr();
            let offset = if (base as usize).is_multiple_of(2) {
                1
            } else {
                0
            };
            let report = unsafe { base.add(offset) };
            assert_ne!((report as usize) % 2, 0);

            let write = |field_offset: usize, bytes: &[u8]| unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    report.add(field_offset),
                    bytes.len(),
                );
            };
            write(2, &17_u16.to_ne_bytes());
            write(4, &CONNEXION_COMMAND_HANDLE_AXIS.to_ne_bytes());
            for (index, value) in [10_i16, 20, 30, 40, 50, 60].into_iter().enumerate() {
                write(30 + index * 2, &value.to_ne_bytes());
            }
            write(44, &0x8000_0005_u32.to_ne_bytes());

            let device = unsafe { copy_device_state(report.cast()) }
                .expect("a non-null callback record should be copied");
            assert_eq!(device.client, 17);
            assert_eq!(device.command, CONNEXION_COMMAND_HANDLE_AXIS);
            assert_eq!(device.buttons(), 0x8000_0005);
            assert_eq!(
                canonical_motion(&device),
                MotionPacket {
                    translation: Some([10, 20, 30]),
                    rotation: Some([40, 50, 60]),
                }
            );
        }

        #[test]
        fn mac_driver_rejects_a_null_callback_record() {
            assert!(unsafe { copy_device_state(std::ptr::null()) }.is_none());
        }
    }
}

#[cfg(test)]
mod tests;
