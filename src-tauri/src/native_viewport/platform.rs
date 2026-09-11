#[cfg(target_os = "linux")]
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
#[cfg(target_os = "linux")]
use bevy::render::settings::{RenderCreation, WgpuFeatures, WgpuSettings, WgpuSettingsPriority};
use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::RenderLayers, ClearColorConfig},
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::Indices,
    prelude::*,
    render::{render_resource::PrimitiveTopology, RenderPlugin},
    text::FontWeight,
    ui::UiTransform,
    window::{
        ExitCondition, PresentMode, PrimaryWindow, RawHandleWrapper, RawHandleWrapperHolder,
        WindowPlugin, WindowResized, WindowResolution, WindowScaleFactorChanged, WindowWrapper,
    },
};
use nbcad_core::{BodyAppearance, PlaneBasis};
use nbcad_sketch::{BodyPoseDto, EntityDto, InstanceBodyPoseDto, SketchDto, Vec2 as SketchVec2};
use nbcad_solid::{
    BodyDto, DatumPlaneDefinitionDto, FaceDto, Point2Dto, ProfileCatalogItemDto, ProfileLoopDto,
    SketchPointKindDto, SketchPointRefDto, SolidSceneDto,
};
#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindow, NSWindowOrderingMode};
#[cfg(target_os = "macos")]
use objc2_core_graphics::CGMutablePath;
#[cfg(target_os = "macos")]
use objc2_foundation::{NSPoint, NSRect, NSSize};
#[cfg(target_os = "macos")]
use objc2_quartz_core::{kCAFillRuleEvenOdd, CAShapeLayer};
#[cfg(target_os = "macos")]
use raw_window_handle::{AppKitDisplayHandle, AppKitWindowHandle};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
#[cfg(target_os = "windows")]
use raw_window_handle::{Win32WindowHandle, WindowsDisplayHandle};
#[cfg(target_os = "linux")]
use raw_window_handle::{XlibDisplayHandle, XlibWindowHandle};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::ptr::NonNull;
#[cfg(target_os = "linux")]
use std::time::Duration;
use std::{
    any::Any,
    collections::HashMap,
    ffi::c_void,
    num::NonZeroU32,
    panic::AssertUnwindSafe,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
#[cfg(target_os = "windows")]
use std::{num::NonZeroIsize, sync::OnceLock};
use tauri::Manager;
#[cfg(target_os = "windows")]
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
#[cfg(target_os = "windows")]
use windows_core_webview2::PCWSTR;
#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::{
        GetLastError, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
    },
    Graphics::Gdi::{
        CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, ScreenToClient, SetWindowRgn,
        RGN_DIFF,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{
            GetKeyState, ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
            VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
        },
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, GetClientRect, GetParent, GetWindowLongPtrW,
            RegisterClassW, SetWindowLongPtrW, SetWindowPos, ShowWindow, CS_DBLCLKS, CS_OWNDC,
            GWLP_USERDATA, HTCLIENT, HWND_TOP, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_SHOWWINDOW,
            SW_HIDE, SW_SHOWNA, WM_CANCELMODE, WM_CAPTURECHANGED, WM_ERASEBKGND, WM_LBUTTONDBLCLK,
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP,
            WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_NCHITTEST,
            WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN,
            WS_CLIPSIBLINGS, WS_EX_NOACTIVATE, WS_EX_NOPARENTNOTIFY,
        },
    },
};
#[cfg(target_os = "linux")]
use {
    glib::{
        prelude::*,
        translate::{from_glib_borrow, IntoGlib, ToGlibPtr},
    },
    gtk::prelude::*,
};

use super::profile_outline::{base_curve_remainder, profile_outline_segments, BaseCurveRemainder};
use super::ui::{
    self, HudAxisLabel, HudAxisMark, NativeHudRoot, ViewportUiAssets, ViewportUiTheme,
};
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux", test))]
use super::ViewportRect;
use super::{
    NativePick, NativePickPurpose, NativeViewportMetrics, ViewportAnnotationKind, ViewportCamera,
    ViewportHud, ViewportLayout, ViewportLinePattern, ViewportMode, ViewportModel,
    ViewportOriginPlane, ViewportPalette, ViewportPresentation, ViewportPreview, ViewportSnapKind,
    ViewportSnapMarker,
};
use crate::state::BOOTSTRAP_SESSION_ID;

#[path = "script_preview.rs"]
pub(crate) mod script_preview;

#[cfg(target_os = "linux")]
const INITIAL_PHYSICAL_SIZE: u32 = 1;
#[cfg(not(target_os = "linux"))]
const INITIAL_PHYSICAL_SIZE: u32 = 32;
/// Base mesh size; a camera-aware transform keeps its screen footprint stable.
const REFERENCE_PLANE_HALF_SIZE: f32 = 50.0;
const REFERENCE_PLANE_SCREEN_FRACTION: f32 = 0.32;
const SKETCH_LINE_WIDTH: f32 = 1.25;
const FINISHED_SKETCH_OFFSET: f32 = 0.05;
const PROFILE_PICK_OFFSET: f32 = 0.08;
// Direct line feedback must occupy the exact profile-perimeter geometry. If
// it is even slightly behind that perimeter, selecting a profile edge as a
// Revolve axis records the right state but leaves no visible selected line.
const DIRECT_PICK_FEEDBACK_OFFSET: f32 = PROFILE_PICK_OFFSET;
// Gizmo widths are expressed in physical pixels after
// `configure_viewport_line_widths` applies the backing scale. A base width of
// one therefore renders as roughly one logical desktop pixel on Retina while
// remaining visible on a 1x display. Do not quarter this value: that produces
// a sub-pixel stroke which Metal can effectively hide.
const PICK_FEEDBACK_LINE_WIDTH: f32 = 1.0;
// A selected/hovered finished-sketch curve is already reinforced by a
// high-contrast state color. Keep this direct replacement slightly finer
// than the general pick overlay so it does not visually thicken profile
// geometry, while staying above Metal's unreliable quarter-pixel range.
const DIRECT_PICK_FEEDBACK_LINE_WIDTH: f32 = 0.75;
// A profile border is one fine stroke, not a colored line inside a halo.
// Keep its width separate so changes cannot resize point/origin feedback.
const PROFILE_BORDER_LINE_WIDTH: f32 = 0.75;
// Bevy's meaningful 3D gizmo depth-bias domain is [-1, 1], where -1 is
// always in front. Values outside that domain can clip the entire gizmo pass
// on Metal. Keep ordinary sketch references behind the two interaction
// layers, and keep each narrow color stroke in front of its wider halo.
const PICK_FEEDBACK_DEPTH_BIAS: f32 = -0.98;
const PICK_FEEDBACK_HALO_LINE_WIDTH: f32 = 2.0;
const PICK_FEEDBACK_HALO_DEPTH_BIAS: f32 = -0.96;
// A directly hovered/selected curve must remain in front of a selected
// profile perimeter when they are the exact same geometry. A later draw call
// in the same gizmo group is not sufficient because the two strokes are
// batched with the same depth bias.
const DIRECT_PICK_FEEDBACK_DEPTH_BIAS: f32 = -1.0;
const VIEWPORT_LINE_REFERENCE_DIAGONAL: f32 = 1200.0;
const VIEWPORT_LINE_SCALE_MIN: f32 = 0.9;
const VIEWPORT_LINE_SCALE_MAX: f32 = 1.6;
/// Below this projected radius, per-edge gizmos cost more CPU/GPU bandwidth
/// than the outline information they can convey. Bevy still renders the
/// retained shaded mesh and selected/hovered geometry always bypasses LOD.
const OCCURRENCE_EDGE_LOD_MIN_RADIUS_PX: f32 = 3.0;
const SKETCH_DEPTH_BIAS: f32 = -0.90;
const SKETCH_POINT_OUTLINE_WIDTH: f32 = 2.0;
const SKETCH_POINT_OUTLINE_DEPTH_BIAS: f32 = -0.89;
const SKETCH_POINT_RADIUS_PX: f32 = 2.5;
const SKETCH_POINT_OUTLINE_RADIUS_PX: f32 = 3.25;
const HIGHLIGHT_LINE_WIDTH: f32 = 2.0;
const SNAP_MARKER_HALF_SIZE_PX: f32 = 6.0;

#[cfg(target_os = "macos")]
const NATIVE_BACKEND: &str = "Bevy 0.19 / wgpu Metal / embedded NSView";
#[cfg(target_os = "windows")]
const NATIVE_BACKEND: &str = "Bevy 0.19 / wgpu DX12-Vulkan / embedded HWND";
#[cfg(target_os = "linux")]
const NATIVE_BACKEND: &str = "Bevy 0.19 / wgpu Vulkan / embedded GTK X11-XWayland DrawingArea";

#[derive(Default)]
struct NativePointers {
    webview: AtomicUsize,
    viewport: AtomicUsize,
    window: AtomicUsize,
}

#[derive(Default)]
struct MetricsState {
    ready: bool,
    startup_error: Option<String>,
    display_backend: String,
    ci_probe_written: bool,
    probe_count: u64,
    logical_width: f64,
    logical_height: f64,
    scale_factor: f64,
    physical_width: u32,
    physical_height: u32,
    rendered_frames: u64,
    wakeups: u64,
    total_frame_ms: f64,
    last_pointer_latency_ms: f64,
    body_count: usize,
    triangle_count: usize,
}

enum RenderCommand {
    Resize {
        logical_width: f64,
        logical_height: f64,
        scale_factor: f64,
        palette: ViewportPalette,
        hud: ViewportHud,
    },
    Model(ViewportModel),
    RebindModelSession {
        from: String,
        to: String,
    },
    DropModelSession(String),
    Camera(ViewportCamera),
    Preview(ViewportPreview),
    Presentation(ViewportPresentation),
}

#[derive(Default)]
struct PendingRenderCommands {
    resize: Option<(f64, f64, f64, ViewportPalette, ViewportHud)>,
    model: Option<ViewportModel>,
    rebind_model_sessions: Vec<(String, String)>,
    drop_model_sessions: Vec<String>,
    camera: Option<ViewportCamera>,
    preview: Option<ViewportPreview>,
    presentation: Option<ViewportPresentation>,
    scheduled: bool,
}

struct MainThreadRenderRuntime {
    app: bevy::app::App,
    model: ViewportModel,
    camera: ViewportCamera,
    logical_size: (f32, f32),
    scale_factor: f32,
    session_aliases: HashMap<String, String>,
}

struct PickState {
    scene: SolidSceneDto,
    body_poses: Vec<BodyPoseDto>,
    instance_body_poses: Vec<InstanceBodyPoseDto>,
    camera: ViewportCamera,
    logical_size: (f32, f32),
    hidden_body_ids: Vec<u64>,
}

impl Default for PickState {
    fn default() -> Self {
        Self {
            scene: SolidSceneDto::default(),
            body_poses: Vec::new(),
            instance_body_poses: Vec::new(),
            camera: ViewportCamera::default(),
            logical_size: (1.0, 1.0),
            hidden_body_ids: Vec::new(),
        }
    }
}

pub struct PlatformNativeViewport {
    app: tauri::AppHandle,
    runtime: Arc<AtomicUsize>,
    pending: Arc<Mutex<PendingRenderCommands>>,
    pick_state: Arc<Mutex<PickState>>,
    layout_revision: Arc<AtomicU64>,
    last_layout: Arc<Mutex<Option<ViewportLayout>>>,
    suspended: Arc<AtomicBool>,
    pointers: Arc<NativePointers>,
    metrics: Arc<Mutex<MetricsState>>,
}

impl PlatformNativeViewport {
    pub fn install(app: &mut tauri::App) -> Result<Self, String> {
        let main_window = app
            .get_webview_window("main")
            .ok_or_else(|| "main Tauri webview window is missing".to_string())?;
        let app_handle = app.handle().clone();
        let runtime = Arc::new(AtomicUsize::new(0));
        let pending = Arc::new(Mutex::new(PendingRenderCommands::default()));
        let pick_state = Arc::new(Mutex::new(PickState::default()));
        let layout_revision = Arc::new(AtomicU64::new(0));
        let last_layout = Arc::new(Mutex::new(None));
        let suspended = Arc::new(AtomicBool::new(false));
        let pointers = Arc::new(NativePointers::default());
        let metrics = Arc::new(Mutex::new(MetricsState::default()));
        let install_pointers = pointers.clone();
        let install_metrics = metrics.clone();
        let install_runtime = runtime.clone();
        let install_pending = pending.clone();
        #[cfg(target_os = "linux")]
        let install_last_layout = last_layout.clone();

        main_window
            .with_webview(move |platform| {
                // Tauri guarantees this closure runs on the native UI thread.
                #[cfg(target_os = "macos")]
                let marker =
                    MainThreadMarker::new().expect("Tauri with_webview must run on main thread");
                #[cfg(target_os = "macos")]
                let result = unsafe {
                    install_native_views(
                        marker,
                        platform.inner(),
                        platform.ns_window(),
                        install_pointers.clone(),
                    )
                };
                #[cfg(target_os = "windows")]
                let result = unsafe {
                    let controller = platform.controller();
                    let core_webview = controller.CoreWebView2().map_err(|error| {
                        format!("WebView2 did not expose its page interface: {error}")
                    });
                    let mut webview_hwnd = Default::default();
                    controller
                        .ParentWindow(&mut webview_hwnd)
                        .map_err(|error| {
                            format!("WebView2 did not expose its container HWND: {error}")
                        })
                        .and_then(|_| core_webview)
                        .and_then(|core_webview| {
                            install_native_views(
                                webview_hwnd.0,
                                install_pointers.clone(),
                                core_webview,
                            )
                        })
                };
                #[cfg(target_os = "linux")]
                let result = unsafe {
                    install_native_views(
                        platform.inner(),
                        install_pointers.clone(),
                        install_runtime.clone(),
                        install_pending.clone(),
                        install_metrics.clone(),
                        install_last_layout.clone(),
                    )
                };

                let (view_handle, scale_factor) = match result {
                    Ok(value) => value,
                    Err(error) => {
                        record_startup_failure(
                            &install_metrics,
                            format!("native viewport installation failed: {error}"),
                        );
                        return;
                    }
                };
                let display_backend = view_handle.display_backend_name().to_string();

                // Renderer initialization can panic inside a platform backend before
                // Tauri has a chance to surface an IPC error. Keep the React shell
                // alive and expose the real cause through metrics/CI instead of
                // silently leaving an empty viewport.
                let initialized = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    let bevy_app = build_bevy_app(view_handle, scale_factor as f32)?;
                    let render_runtime = Box::new(MainThreadRenderRuntime {
                        app: bevy_app,
                        model: ViewportModel {
                            session_id: BOOTSTRAP_SESSION_ID.to_string(),
                            geometry_revision: 0,
                            scene: SolidSceneDto::default(),
                            active_sketch: None,
                            finished_sketches: Vec::new(),
                            datum_planes: Vec::new(),
                            profile_catalog: Vec::new(),
                            body_appearances: Vec::new(),
                            body_poses: Vec::new(),
                            instance_body_poses: Vec::new(),
                        },
                        camera: ViewportCamera::default(),
                        logical_size: (1.0, 1.0),
                        scale_factor: scale_factor as f32,
                        session_aliases: HashMap::new(),
                    });
                    // Linux must not create a Vulkan surface for GTK's 1x1
                    // placeholder drawable. Its first Bevy update is driven
                    // by the server-confirmed ConfigureNotify below. macOS
                    // and Windows keep their established eager bootstrap.
                    #[cfg(target_os = "linux")]
                    let render_runtime = render_runtime;
                    #[cfg(not(target_os = "linux"))]
                    let mut render_runtime = render_runtime;
                    #[cfg(not(target_os = "linux"))]
                    render_frames(&mut render_runtime.app, 2, &install_metrics);
                    Ok::<_, String>(render_runtime)
                }));
                let render_runtime = match initialized {
                    Ok(Ok(runtime)) => runtime,
                    Ok(Err(error)) => {
                        record_startup_failure(
                            &install_metrics,
                            format!("native Bevy viewport failed to initialize: {error}"),
                        );
                        return;
                    }
                    Err(payload) => {
                        record_startup_failure(
                            &install_metrics,
                            format!(
                                "native Bevy viewport panicked during initialization: {}",
                                panic_message(payload)
                            ),
                        );
                        return;
                    }
                };

                // The Bevy App and its native surface stay on the native UI
                // thread. The allocation lives for the process and is
                // dereferenced only by run_on_main_thread closures.
                let runtime_pointer = Box::into_raw(render_runtime) as usize;
                install_runtime.store(runtime_pointer, Ordering::Release);
                if let Ok(mut current) = install_metrics.lock() {
                    current.ready = true;
                    current.scale_factor = scale_factor;
                    current.display_backend = display_backend;
                }
                eprintln!(
                    "native Bevy viewport installed ({NATIVE_BACKEND}, {scale_factor:.2}x scale)"
                );
                // On Linux the bootstrap scene is commonly queued before GTK
                // has allocated the embedded X11 child. Keep it coalesced
                // until the first real ConfigureNotify; draining here would
                // still create a FIFO swapchain for the 1x1 placeholder.
                #[cfg(not(target_os = "linux"))]
                drain_render_commands(&install_runtime, &install_pending, &install_metrics);
            })
            .map_err(|error| format!("could not access the native webview: {error}"))?;

        Ok(Self {
            app: app_handle,
            runtime,
            pending,
            pick_state,
            layout_revision,
            last_layout,
            suspended,
            pointers,
            metrics,
        })
    }

    pub fn set_layout(&self, app: &tauri::AppHandle, layout: ViewportLayout) -> Result<(), String> {
        if layout.revision > 0 {
            let previous = self
                .layout_revision
                .fetch_max(layout.revision, Ordering::AcqRel);
            if layout.revision < previous {
                return Ok(());
            }
        }
        if let Ok(mut state) = self.pick_state.lock() {
            state.logical_size = (
                layout.viewport.width.max(1.0) as f32,
                layout.viewport.height.max(1.0) as f32,
            );
        }
        if let Ok(mut last_layout) = self.last_layout.lock() {
            *last_layout = Some(layout.clone());
        }
        if self.suspended.load(Ordering::Acquire) {
            return Ok(());
        }
        let pointers = self.pointers.clone();
        let runtime = self.runtime.clone();
        let pending = self.pending.clone();
        let metrics = self.metrics.clone();
        app.run_on_main_thread(move || {
            let webview_pointer = pointers.webview.load(Ordering::Acquire);
            let viewport_pointer = pointers.viewport.load(Ordering::Acquire);
            let window_pointer = pointers.window.load(Ordering::Acquire);
            if webview_pointer == 0 || viewport_pointer == 0 || window_pointer == 0 {
                return;
            }

            let scale_factor = unsafe {
                apply_native_layout(webview_pointer, viewport_pointer, window_pointer, &layout)
            };
            #[cfg(target_os = "linux")]
            {
                // A DOM layout is a requested GTK allocation, not proof that
                // X11 has resized the child drawable. Rendering the requested
                // extent early violates Vulkan's currentExtent contract and
                // can permanently blacken the swapchain. A matching layout is
                // still a useful explicit redraw (palette/HUD/full-screen
                // transitions); actual size changes are driven exclusively by
                // the viewport's server-confirmed configure event below.
                let pointer = runtime.load(Ordering::Acquire);
                let matches_configured_size = if pointer == 0 {
                    false
                } else {
                    let render_runtime = unsafe { &*(pointer as *const MainThreadRenderRuntime) };
                    (render_runtime.logical_size.0 - layout.viewport.width as f32).abs() <= 0.01
                        && (render_runtime.logical_size.1 - layout.viewport.height as f32).abs()
                            <= 0.01
                };
                if matches_configured_size {
                    push_render_command(
                        &pending,
                        RenderCommand::Resize {
                            logical_width: layout.viewport.width.max(1.0),
                            logical_height: layout.viewport.height.max(1.0),
                            scale_factor,
                            palette: layout.palette,
                            hud: layout.hud,
                        },
                    );
                    drain_render_commands(&runtime, &pending, &metrics);
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                push_render_command(
                    &pending,
                    RenderCommand::Resize {
                        logical_width: layout.viewport.width.max(1.0),
                        logical_height: layout.viewport.height.max(1.0),
                        scale_factor,
                        palette: layout.palette,
                        hud: layout.hud,
                    },
                );
                drain_render_commands(&runtime, &pending, &metrics);
            }
        })
        .map_err(|error| format!("could not schedule native viewport layout: {error}"))
    }

    pub fn set_suspended(&self, app: &tauri::AppHandle, suspended: bool) -> Result<(), String> {
        self.suspended.store(suspended, Ordering::Release);
        let pointers = self.pointers.clone();
        let layout = self
            .last_layout
            .lock()
            .map_err(|_| "native viewport layout lock poisoned".to_string())?
            .clone();
        app.run_on_main_thread(move || {
            let webview_pointer = pointers.webview.load(Ordering::Acquire);
            let viewport_pointer = pointers.viewport.load(Ordering::Acquire);
            let window_pointer = pointers.window.load(Ordering::Acquire);
            if webview_pointer == 0 || viewport_pointer == 0 || window_pointer == 0 {
                return;
            }
            unsafe {
                set_native_viewport_suspended(
                    webview_pointer,
                    viewport_pointer,
                    window_pointer,
                    suspended,
                    layout.as_ref(),
                );
            }
        })
        .map_err(|error| format!("could not suspend native viewport: {error}"))
    }

    pub fn sync_model(&self, model: ViewportModel) -> Result<(), String> {
        if let Ok(mut state) = self.pick_state.lock() {
            state.scene = model.scene.clone();
            state.body_poses = model.body_poses.clone();
            state.instance_body_poses = model.instance_body_poses.clone();
        }
        self.enqueue(RenderCommand::Model(model))
    }

    pub fn drop_model_session(&self, session_id: String) -> Result<(), String> {
        self.enqueue(RenderCommand::DropModelSession(session_id))
    }

    pub fn rebind_model_session(&self, from: String, to: String) -> Result<(), String> {
        self.enqueue(RenderCommand::RebindModelSession { from, to })
    }

    pub fn set_camera(&self, camera: ViewportCamera) -> Result<(), String> {
        if let Ok(mut state) = self.pick_state.lock() {
            state.camera = camera;
        }
        self.enqueue(RenderCommand::Camera(camera))
    }

    pub fn set_preview(&self, preview: ViewportPreview) -> Result<(), String> {
        const MAX_LINE_FLOATS: usize = 6 * 65_536;
        const MAX_POINT_FLOATS: usize = 3 * 32_768;
        const MAX_TRIANGLE_FLOATS: usize = 9 * 65_536;
        const MAX_ARROWS: usize = 256;
        const MAX_ANNOTATIONS: usize = 2_048;
        let line_floats = preview
            .lines
            .iter()
            .map(|layer| layer.segments.len())
            .sum::<usize>();
        let point_floats = preview
            .points
            .iter()
            .map(|layer| layer.positions.len())
            .sum::<usize>();
        let triangle_floats = preview
            .triangles
            .iter()
            .map(|layer| layer.positions.len())
            .sum::<usize>();
        if preview.lines.len() > 128
            || preview.points.len() > 128
            || preview.triangles.len() > 128
            || preview.arrows.len() > MAX_ARROWS
            || line_floats > MAX_LINE_FLOATS
            || point_floats > MAX_POINT_FLOATS
            || triangle_floats > MAX_TRIANGLE_FLOATS
            || preview.annotations.len() > MAX_ANNOTATIONS
            || preview
                .annotations
                .iter()
                .any(|annotation| annotation.text.len() > 128)
        {
            return Err("native transient presentation is too large".to_string());
        }
        self.enqueue(RenderCommand::Preview(preview))
    }

    pub fn set_presentation(&self, presentation: ViewportPresentation) -> Result<(), String> {
        if let Ok(mut state) = self.pick_state.lock() {
            state.hidden_body_ids = presentation.hidden_body_ids.clone();
            state.body_poses = presentation.body_poses.clone();
            state.instance_body_poses = presentation.instance_body_poses.clone();
        }
        self.enqueue(RenderCommand::Presentation(presentation))
    }

    pub fn pick(
        &self,
        x: f32,
        y: f32,
        camera: Option<ViewportCamera>,
        logical_size: Option<(f32, f32)>,
        purpose: NativePickPurpose,
    ) -> Result<Option<NativePick>, String> {
        let started = Instant::now();
        let result = {
            let state = self
                .pick_state
                .lock()
                .map_err(|_| "native viewport pick state lock poisoned".to_string())?;
            pick_occt_scene(
                &state.scene,
                camera.unwrap_or(state.camera),
                logical_size.unwrap_or(state.logical_size),
                x,
                y,
                &state.hidden_body_ids,
                &state.body_poses,
                &state.instance_body_poses,
                purpose,
            )
        };
        if let Ok(mut current) = self.metrics.lock() {
            current.last_pointer_latency_ms = started.elapsed().as_secs_f64() * 1_000.0;
        }
        Ok(result)
    }

    fn enqueue(&self, command: RenderCommand) -> Result<(), String> {
        let should_schedule = push_render_command(&self.pending, command);
        if !should_schedule {
            return Ok(());
        }
        let runtime = self.runtime.clone();
        let pending = self.pending.clone();
        let metrics = self.metrics.clone();
        if let Err(error) = self.app.run_on_main_thread(move || {
            drain_render_commands(&runtime, &pending, &metrics);
        }) {
            if let Ok(mut pending) = self.pending.lock() {
                pending.scheduled = false;
            }
            return Err(format!("could not schedule native Bevy update: {error}"));
        }
        Ok(())
    }

    pub fn metrics(&self) -> NativeViewportMetrics {
        let mut metrics = self.metrics.lock().expect("viewport metrics lock poisoned");
        if metrics.probe_count == 0 {
            eprintln!(
                "React native-viewport bridge connected (ready={})",
                metrics.ready
            );
        }
        metrics.probe_count += 1;
        NativeViewportMetrics {
            available: true,
            ready: metrics.ready,
            startup_error: metrics.startup_error.clone(),
            backend: NATIVE_BACKEND.to_string(),
            logical_width: metrics.logical_width,
            logical_height: metrics.logical_height,
            scale_factor: metrics.scale_factor,
            physical_width: metrics.physical_width,
            physical_height: metrics.physical_height,
            rendered_frames: metrics.rendered_frames,
            wakeups: metrics.wakeups,
            average_frame_ms: if metrics.rendered_frames == 0 {
                0.0
            } else {
                metrics.total_frame_ms / metrics.rendered_frames as f64
            },
            last_pointer_latency_ms: metrics.last_pointer_latency_ms,
            body_count: metrics.body_count,
            triangle_count: metrics.triangle_count,
        }
    }
}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown renderer panic".to_string()
    }
}

/// Writes only when explicitly requested by a development/CI environment.
/// This contains renderer readiness metadata, never pointer or model data.
fn write_ci_probe(metrics: &MetricsState, status: &str) -> Result<(), String> {
    let Some(path) = std::env::var_os("NBCAD_VIEWPORT_PROBE_FILE") else {
        return Ok(());
    };
    let payload = serde_json::json!({
        "status": status,
        "backend": NATIVE_BACKEND,
        "displayBackend": metrics.display_backend,
        "error": metrics.startup_error,
        "logicalWidth": metrics.logical_width,
        "logicalHeight": metrics.logical_height,
        "scaleFactor": metrics.scale_factor,
        "physicalWidth": metrics.physical_width,
        "physicalHeight": metrics.physical_height,
        "renderedFrames": metrics.rendered_frames,
    });
    let bytes = serde_json::to_vec_pretty(&payload)
        .map_err(|error| format!("could not encode native viewport probe: {error}"))?;
    std::fs::write(path, bytes)
        .map_err(|error| format!("could not write native viewport probe: {error}"))
}

fn record_startup_failure(metrics: &Arc<Mutex<MetricsState>>, error: String) {
    eprintln!("{error}");
    if let Ok(mut current) = metrics.lock() {
        current.startup_error = Some(error);
        current.ready = false;
        if !current.ci_probe_written {
            match write_ci_probe(&current, "error") {
                Ok(()) => current.ci_probe_written = true,
                Err(probe_error) => eprintln!("{probe_error}"),
            }
        }
    }
}

fn maybe_write_ready_probe(metrics: &Arc<Mutex<MetricsState>>) {
    let Ok(mut current) = metrics.lock() else {
        return;
    };
    if current.ci_probe_written
        || !current.ready
        || current.startup_error.is_some()
        || current.physical_width < 2
        || current.physical_height < 2
        || current.rendered_frames < 2
    {
        return;
    }
    match write_ci_probe(&current, "ready") {
        Ok(()) => current.ci_probe_written = true,
        Err(error) => eprintln!("{error}"),
    }
}

/// Installs a sibling NSView directly below the WKWebView. The NSWindow stays
/// opaque; only the WKWebView's layer gets a viewport-shaped mask.
#[cfg(target_os = "macos")]
unsafe fn install_native_views(
    marker: MainThreadMarker,
    webview_pointer: *mut c_void,
    window_pointer: *mut c_void,
    pointers: Arc<NativePointers>,
) -> Result<(NativeViewHandle, f64), String> {
    if webview_pointer.is_null() || window_pointer.is_null() {
        return Err("Tauri returned a null AppKit handle".to_string());
    }

    let webview = unsafe { &*webview_pointer.cast::<NSView>() };
    let ns_window = unsafe { &*window_pointer.cast::<NSWindow>() };
    let parent = unsafe { webview.superview() }
        .ok_or_else(|| "WKWebView is not attached to an NSView hierarchy".to_string())?;

    // Tauri/Wry may expose the main WKWebView through its child-view path,
    // whose default mask only anchors the top edge. The CAD host requires the
    // WebView and its container to follow every live resize and native
    // full-screen transition, including while the Bevy viewport is unmounted.
    let flexible =
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable;
    parent.setAutoresizingMask(parent.autoresizingMask() | flexible);
    webview.setAutoresizingMask(webview.autoresizingMask() | flexible);
    webview.setWantsLayer(true);
    let viewport = NSView::new(marker);
    viewport.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)));
    viewport.setWantsLayer(true);
    viewport.setHidden(true);
    parent.addSubview_positioned_relativeTo(&viewport, NSWindowOrderingMode::Below, Some(webview));

    let view_pointer = (&*viewport as *const NSView) as usize;
    pointers
        .webview
        .store(webview_pointer as usize, Ordering::Release);
    pointers.viewport.store(view_pointer, Ordering::Release);
    pointers
        .window
        .store(window_pointer as usize, Ordering::Release);

    Ok((
        NativeViewHandle::appkit(view_pointer),
        ns_window.backingScaleFactor(),
    ))
}

#[cfg(target_os = "macos")]
unsafe fn apply_native_layout(
    webview_pointer: usize,
    viewport_pointer: usize,
    window_pointer: usize,
    layout: &ViewportLayout,
) -> f64 {
    let webview = unsafe { &*(webview_pointer as *const NSView) };
    let viewport = unsafe { &*(viewport_pointer as *const NSView) };
    let ns_window = unsafe { &*(window_pointer as *const NSWindow) };
    let webview_bounds = webview.bounds();
    let Some(parent) = (unsafe { webview.superview() }) else {
        viewport.setHidden(true);
        return ns_window.backingScaleFactor();
    };

    // AppKit can replace the WKWebView's container during native full-screen
    // transitions. Keep the Metal sibling attached to the WebView's current
    // parent; applying a frame converted for a different parent can otherwise
    // expand the viewport across the whole application shell after exit.
    let parent_pointer = (&*parent as *const NSView) as usize;
    let viewport_parent_pointer = unsafe { viewport.superview() }
        .as_deref()
        .map(|view| (view as *const NSView) as usize);
    if viewport_parent_pointer != Some(parent_pointer) {
        viewport.removeFromSuperview();
        parent.addSubview_positioned_relativeTo(
            viewport,
            NSWindowOrderingMode::Below,
            Some(webview),
        );
    }

    let viewport_in_webview = dom_rect_to_view_rect(webview, layout.viewport);
    let viewport_in_parent = webview.convertRect_toView(viewport_in_webview, Some(&parent));
    viewport.setFrame(viewport_in_parent);
    let viewport_visible = layout.viewport.width >= 2.0 && layout.viewport.height >= 2.0;
    viewport.setHidden(!viewport_visible);
    if std::env::var_os("NBCAD_NATIVE_LAYOUT_DEBUG").is_some() {
        eprintln!(
            "native viewport layout: bounds={webview_bounds:?} safe={:?} dom={:?} appkit={viewport_in_webview:?}",
            webview.safeAreaRect(),
            layout.viewport,
        );
    }

    if let Some(webview_layer) = webview.layer() {
        // Drawing (and future non-3D workspaces) unmount the DOM viewport.
        // Leaving an old even-odd mask installed clips the resized WebView to
        // its previous window bounds, producing black strips after entering
        // full screen. With no viewport hole, no mask is needed at all.
        if !viewport_visible {
            webview_layer.setMask(None);
            return ns_window.backingScaleFactor();
        }
        let layer_bounds = webview_layer.bounds();
        let mask = CAShapeLayer::layer();
        mask.setFrame(layer_bounds);

        let path = CGMutablePath::new();
        let outer = webview.convertRectToLayer(webview_bounds);
        let hole = webview.convertRectToLayer(viewport_in_webview);
        unsafe {
            CGMutablePath::add_rect(Some(&path), std::ptr::null(), outer);
            CGMutablePath::add_rect(Some(&path), std::ptr::null(), hole);
        }

        // Overlay rectangles are clipped to the viewport hole before being
        // toggled back on. React intentionally sends non-overlapping islands.
        for overlay in &layout.overlays {
            if let Some(intersection) = intersect_rect(*overlay, layout.viewport) {
                let overlay_rect =
                    webview.convertRectToLayer(dom_rect_to_view_rect(webview, intersection));
                unsafe {
                    if intersection.corner_radius > 0.5 {
                        CGMutablePath::add_rounded_rect(
                            Some(&path),
                            std::ptr::null(),
                            overlay_rect,
                            intersection.corner_radius,
                            intersection.corner_radius,
                        );
                    } else {
                        CGMutablePath::add_rect(Some(&path), std::ptr::null(), overlay_rect);
                    }
                }
            }
        }

        mask.setPath(Some(&path));
        mask.setFillRule(kCAFillRuleEvenOdd);
        unsafe {
            webview_layer.setMask(Some(&mask));
        }
    }

    ns_window.backingScaleFactor()
}

#[cfg(target_os = "macos")]
unsafe fn set_native_viewport_suspended(
    webview_pointer: usize,
    viewport_pointer: usize,
    window_pointer: usize,
    suspended: bool,
    layout: Option<&ViewportLayout>,
) {
    let webview = unsafe { &*(webview_pointer as *const NSView) };
    let viewport = unsafe { &*(viewport_pointer as *const NSView) };
    if suspended {
        viewport.setHidden(true);
        if let Some(layer) = webview.layer() {
            layer.setMask(None);
        }
    } else if let Some(layout) = layout {
        let _ = unsafe {
            apply_native_layout(webview_pointer, viewport_pointer, window_pointer, layout)
        };
    }
}

#[cfg(target_os = "macos")]
fn dom_rect_to_view_rect(view: &NSView, rect: ViewportRect) -> NSRect {
    let bounds = view.bounds();
    // `getBoundingClientRect()` is relative to WebKit's unobscured content
    // viewport. In a normal macOS window the WKWebView can still extend under
    // the title bar, so its NSView bounds are taller than that DOM viewport by
    // the title-bar safe-area inset. Full screen has no such inset. Mapping
    // against the safe-area rect keeps both the Metal sibling and every DOM
    // mask island on the same origin through window/full-screen transitions.
    let content = view.safeAreaRect();
    dom_rect_to_content_rect(bounds, content, view.isFlipped(), rect)
}

#[cfg(target_os = "macos")]
fn dom_rect_to_content_rect(
    bounds: NSRect,
    content: NSRect,
    flipped: bool,
    rect: ViewportRect,
) -> NSRect {
    let content = if content.size.width > 0.0 && content.size.height > 0.0 {
        content
    } else {
        bounds
    };
    let y = if flipped {
        content.origin.y + rect.y
    } else {
        content.origin.y + content.size.height - rect.y - rect.height
    };
    NSRect::new(
        NSPoint::new(content.origin.x + rect.x, y),
        NSSize::new(rect.width.max(0.0), rect.height.max(0.0)),
    )
}

#[cfg(target_os = "windows")]
static WINDOWS_VIEWPORT_CLASS: OnceLock<Result<(), u32>> = OnceLock::new();

#[cfg(target_os = "windows")]
const WINDOWS_WM_MOUSELEAVE: u32 = 0x02A3;
#[cfg(target_os = "windows")]
const WINDOWS_INPUT_PREFIX: &str = "__nbcad_native_input__|";

#[cfg(target_os = "windows")]
struct WindowsInputBridge {
    webview: ICoreWebView2,
    tracking_mouse_leave: bool,
    last_x: i32,
    last_y: i32,
}

#[cfg(target_os = "windows")]
impl WindowsInputBridge {
    fn post(
        &self,
        kind: char,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        button: i32,
        buttons: u32,
        delta: i32,
    ) {
        let payload = format!(
            "{WINDOWS_INPUT_PREFIX}{kind}|{x}|{y}|{width}|{height}|{button}|{buttons}|{}|{delta}",
            windows_input_modifiers()
        );
        let wide = payload
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        if let Err(error) = unsafe { self.webview.PostWebMessageAsString(PCWSTR(wide.as_ptr())) } {
            if std::env::var_os("NBCAD_NATIVE_INPUT_DEBUG").is_some() {
                eprintln!("native viewport input bridge failed: {error}");
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_input_modifiers() -> u32 {
    let pressed = |key: u16| unsafe { GetKeyState(key as i32) } < 0;
    u32::from(pressed(VK_SHIFT))
        | (u32::from(pressed(VK_CONTROL)) << 1)
        | (u32::from(pressed(VK_MENU)) << 2)
        | (u32::from(pressed(VK_LWIN) || pressed(VK_RWIN)) << 3)
}

#[cfg(target_os = "windows")]
fn windows_dom_buttons(wparam: WPARAM) -> u32 {
    let keys = wparam as u32;
    u32::from(keys & 0x0001 != 0)
        | (u32::from(keys & 0x0002 != 0) << 1)
        | (u32::from(keys & 0x0010 != 0) << 2)
        | (u32::from(keys & 0x0020 != 0) << 3)
        | (u32::from(keys & 0x0040 != 0) << 4)
}

#[cfg(target_os = "windows")]
fn windows_lparam_point(lparam: LPARAM) -> POINT {
    let packed = lparam as u32;
    POINT {
        x: packed as u16 as i16 as i32,
        y: (packed >> 16) as u16 as i16 as i32,
    }
}

#[cfg(target_os = "windows")]
unsafe fn post_windows_input(
    hwnd: HWND,
    kind: char,
    point: POINT,
    button: i32,
    buttons: u32,
    delta: i32,
) {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WindowsInputBridge;
    if pointer.is_null() {
        return;
    }
    let mut bounds = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut bounds) } == 0 {
        return;
    }
    let bridge = unsafe { &mut *pointer };
    bridge.last_x = point.x;
    bridge.last_y = point.y;
    bridge.post(
        kind,
        point.x,
        point.y,
        (bounds.right - bounds.left).max(1),
        (bounds.bottom - bounds.top).max(1),
        button,
        buttons,
        delta,
    );
}

#[cfg(target_os = "windows")]
unsafe fn post_windows_cancel(hwnd: HWND) {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WindowsInputBridge;
    if pointer.is_null() {
        return;
    }
    let bridge = unsafe { &*pointer };
    unsafe {
        post_windows_input(
            hwnd,
            'c',
            POINT {
                x: bridge.last_x,
                y: bridge.last_y,
            },
            -1,
            0,
            0,
        )
    };
}

#[cfg(target_os = "windows")]
unsafe fn begin_windows_mouse_leave_tracking(hwnd: HWND) {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WindowsInputBridge;
    if pointer.is_null() || unsafe { &*pointer }.tracking_mouse_leave {
        return;
    }
    let mut tracking = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    if unsafe { TrackMouseEvent(&mut tracking) } != 0 {
        unsafe { &mut *pointer }.tracking_mouse_leave = true;
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn windows_viewport_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // WebView2's actual renderer HWND lives on another UI thread, so
        // HTTRANSPARENT cannot walk from this sibling all the way to the DOM.
        // Own the hit and relay it through CoreWebView2; the page dispatches it
        // onto the existing transparent interaction surface.
        WM_NCHITTEST => HTCLIENT as LRESULT,
        WM_MOUSEMOVE => {
            unsafe { begin_windows_mouse_leave_tracking(hwnd) };
            unsafe {
                post_windows_input(
                    hwnd,
                    'm',
                    windows_lparam_point(lparam),
                    -1,
                    windows_dom_buttons(wparam),
                    0,
                )
            };
            0
        }
        WM_LBUTTONDOWN | WM_MBUTTONDOWN | WM_RBUTTONDOWN | WM_LBUTTONDBLCLK | WM_MBUTTONDBLCLK
        | WM_RBUTTONDBLCLK => {
            unsafe {
                SetCapture(hwnd);
            }
            let button = match message {
                WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => 0,
                WM_MBUTTONDOWN | WM_MBUTTONDBLCLK => 1,
                _ => 2,
            };
            let kind = if matches!(
                message,
                WM_LBUTTONDBLCLK | WM_MBUTTONDBLCLK | WM_RBUTTONDBLCLK
            ) {
                'b'
            } else {
                'd'
            };
            unsafe {
                post_windows_input(
                    hwnd,
                    kind,
                    windows_lparam_point(lparam),
                    button,
                    windows_dom_buttons(wparam),
                    0,
                )
            };
            0
        }
        WM_LBUTTONUP | WM_MBUTTONUP | WM_RBUTTONUP => {
            let button = match message {
                WM_LBUTTONUP => 0,
                WM_MBUTTONUP => 1,
                _ => 2,
            };
            let buttons = windows_dom_buttons(wparam);
            unsafe {
                post_windows_input(hwnd, 'u', windows_lparam_point(lparam), button, buttons, 0)
            };
            if buttons == 0 {
                unsafe {
                    ReleaseCapture();
                }
            }
            0
        }
        WM_MOUSEWHEEL => {
            let mut point = windows_lparam_point(lparam);
            unsafe {
                ScreenToClient(hwnd, &mut point);
            }
            let delta = ((wparam >> 16) as u16 as i16) as i32;
            unsafe {
                post_windows_input(
                    hwnd,
                    'v',
                    point,
                    -1,
                    windows_dom_buttons(wparam & 0xffff),
                    delta,
                )
            };
            0
        }
        // Wheel tilt is intentionally consumed. Only the center-wheel press
        // participates in CAD navigation on Windows.
        WM_MOUSEHWHEEL => 0,
        WM_CANCELMODE | WM_CAPTURECHANGED => {
            unsafe { post_windows_cancel(hwnd) };
            0
        }
        WINDOWS_WM_MOUSELEAVE => {
            let pointer =
                unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WindowsInputBridge;
            if !pointer.is_null() {
                unsafe { &mut *pointer }.tracking_mouse_leave = false;
            }
            unsafe { post_windows_input(hwnd, 'l', POINT::default(), -1, 0, 0) };
            0
        }
        // The swapchain owns every visible pixel; suppress background erases
        // that would otherwise flash while resizing.
        WM_ERASEBKGND => 1,
        WM_NCDESTROY => {
            let pointer =
                unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WindowsInputBridge;
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            if !pointer.is_null() {
                drop(unsafe { Box::from_raw(pointer) });
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

#[cfg(target_os = "windows")]
fn register_windows_viewport_class() -> Result<(), String> {
    let registration = WINDOWS_VIEWPORT_CLASS.get_or_init(|| unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        if module.is_null() {
            return Err(GetLastError());
        }
        let class = WNDCLASSW {
            style: CS_OWNDC | CS_DBLCLKS,
            lpfnWndProc: Some(windows_viewport_proc),
            hInstance: module,
            lpszClassName: windows_sys::w!("noBS.CAD.BevyViewport"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            let error = GetLastError();
            if error != ERROR_CLASS_ALREADY_EXISTS {
                return Err(error);
            }
        }
        Ok(())
    });
    match *registration {
        Ok(()) => Ok(()),
        Err(code) => Err(format!(
            "could not register the native viewport window class (Win32 error {code})"
        )),
    }
}

/// Installs an opaque Win32 child above Wry's WebView2 container. Its window
/// region is cut around visible DOM overlay islands, while viewport input is
/// relayed to the page through CoreWebView2. This avoids both transparent
/// top-level windows and a transparent WebView2 compositor.
#[cfg(target_os = "windows")]
unsafe fn install_native_views(
    webview_pointer: *mut c_void,
    pointers: Arc<NativePointers>,
    core_webview: ICoreWebView2,
) -> Result<(NativeViewHandle, f64), String> {
    if webview_pointer.is_null() {
        return Err("WebView2 returned a null container HWND".to_string());
    }
    let webview = webview_pointer as HWND;
    let window = unsafe { GetParent(webview) };
    if window.is_null() {
        return Err("WebView2 container is not attached to a Win32 parent".to_string());
    }
    register_windows_viewport_class()?;

    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    if module.is_null() {
        return Err(format!(
            "could not resolve the application module (Win32 error {})",
            unsafe { GetLastError() }
        ));
    }
    let viewport = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_NOPARENTNOTIFY,
            windows_sys::w!("noBS.CAD.BevyViewport"),
            std::ptr::null(),
            WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            0,
            0,
            1,
            1,
            window,
            std::ptr::null_mut(),
            module,
            std::ptr::null(),
        )
    };
    if viewport.is_null() {
        return Err(format!(
            "could not create the native viewport HWND (Win32 error {})",
            unsafe { GetLastError() }
        ));
    }
    let input_bridge = Box::new(WindowsInputBridge {
        webview: core_webview,
        tracking_mouse_leave: false,
        last_x: 0,
        last_y: 0,
    });
    unsafe {
        SetWindowLongPtrW(
            viewport,
            GWLP_USERDATA,
            Box::into_raw(input_bridge) as isize,
        );
    }

    pointers.webview.store(webview as usize, Ordering::Release);
    pointers
        .viewport
        .store(viewport as usize, Ordering::Release);
    pointers.window.store(window as usize, Ordering::Release);

    Ok((
        NativeViewHandle::win32(viewport as usize),
        windows_scale_factor(window),
    ))
}

#[cfg(target_os = "windows")]
fn windows_scale_factor(window: HWND) -> f64 {
    let dpi = unsafe { GetDpiForWindow(window) };
    if dpi == 0 {
        1.0
    } else {
        dpi as f64 / 96.0
    }
}

#[cfg(target_os = "windows")]
unsafe fn apply_native_layout(
    _webview_pointer: usize,
    viewport_pointer: usize,
    window_pointer: usize,
    layout: &ViewportLayout,
) -> f64 {
    let viewport = viewport_pointer as HWND;
    let window = window_pointer as HWND;
    let scale_factor = windows_scale_factor(window);
    let rect = layout.viewport;
    let visible = rect.width >= 2.0 && rect.height >= 2.0;
    if !visible {
        unsafe {
            ShowWindow(viewport, SW_HIDE);
        }
        return scale_factor;
    }

    let x = (rect.x * scale_factor).round() as i32;
    let y = (rect.y * scale_factor).round() as i32;
    let width = (rect.width * scale_factor).round().max(1.0) as i32;
    let height = (rect.height * scale_factor).round().max(1.0) as i32;
    let positioned = unsafe {
        SetWindowPos(
            viewport,
            HWND_TOP,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
        )
    };
    if positioned == 0 {
        eprintln!("native viewport resize failed (Win32 error {})", unsafe {
            GetLastError()
        });
    }

    apply_windows_viewport_region(viewport, layout, width, height, scale_factor);
    unsafe {
        ShowWindow(viewport, SW_SHOWNA);
    }
    scale_factor
}

#[cfg(target_os = "windows")]
unsafe fn set_native_viewport_suspended(
    webview_pointer: usize,
    viewport_pointer: usize,
    window_pointer: usize,
    suspended: bool,
    layout: Option<&ViewportLayout>,
) {
    let viewport = viewport_pointer as HWND;
    if suspended {
        unsafe {
            ShowWindow(viewport, SW_HIDE);
        }
    } else if let Some(layout) = layout {
        let _ = unsafe {
            apply_native_layout(webview_pointer, viewport_pointer, window_pointer, layout)
        };
    }
}

#[cfg(target_os = "windows")]
fn apply_windows_viewport_region(
    viewport: HWND,
    layout: &ViewportLayout,
    width: i32,
    height: i32,
    scale_factor: f64,
) {
    let region = unsafe { CreateRectRgn(0, 0, width, height) };
    if region.is_null() {
        return;
    }

    for overlay in &layout.overlays {
        let Some(intersection) = intersect_rect(*overlay, layout.viewport) else {
            continue;
        };
        let left = ((intersection.x - layout.viewport.x) * scale_factor)
            .floor()
            .clamp(0.0, width as f64) as i32;
        let top = ((intersection.y - layout.viewport.y) * scale_factor)
            .floor()
            .clamp(0.0, height as f64) as i32;
        let right = ((intersection.x + intersection.width - layout.viewport.x) * scale_factor)
            .ceil()
            .clamp(0.0, width as f64) as i32;
        let bottom = ((intersection.y + intersection.height - layout.viewport.y) * scale_factor)
            .ceil()
            .clamp(0.0, height as f64) as i32;
        if right <= left || bottom <= top {
            continue;
        }
        let corner_diameter = (intersection.corner_radius * 2.0 * scale_factor).round() as i32;
        let overlay_region = if corner_diameter > 1 {
            unsafe {
                CreateRoundRectRgn(left, top, right, bottom, corner_diameter, corner_diameter)
            }
        } else {
            unsafe { CreateRectRgn(left, top, right, bottom) }
        };
        if overlay_region.is_null() {
            continue;
        }
        unsafe {
            CombineRgn(region, region, overlay_region, RGN_DIFF);
            DeleteObject(overlay_region);
        }
    }

    // SetWindowRgn takes ownership on success.
    if unsafe { SetWindowRgn(viewport, region, 1) } == 0 {
        unsafe {
            DeleteObject(region);
        }
    }
}

/// Installs an opaque GTK/X11 child above WebKitGTK inside Tauri's existing
/// window. Its X11 shape is cut around visible DOM overlay islands, matching
/// the Windows ownership boundary without relying on WebKitGTK's accelerated
/// transparency path. Input passes through to the webview underneath, so the
/// DOM remains the accessibility and interaction owner.
#[cfg(target_os = "linux")]
unsafe fn disconnect_latest_instance_handler(
    webview: &webkit2gtk::WebView,
    signal_name: &'static [u8],
) -> bool {
    let signal_id = unsafe {
        glib::gobject_ffi::g_signal_lookup(signal_name.as_ptr().cast(), webview.type_().into_glib())
    };
    if signal_id == 0 {
        return false;
    }

    // Handler ids increase as handlers are connected. Wry attaches its
    // undecorated-resize hooks after WebKit has installed the handlers that
    // dispatch DOM input, so the newest instance handler is the one to
    // replace. Temporarily block matches while enumerating them; otherwise
    // g_signal_handler_find would return the same handler every time.
    let instance = webview.as_ptr().cast();
    let mask = glib::gobject_ffi::G_SIGNAL_MATCH_ID | glib::gobject_ffi::G_SIGNAL_MATCH_UNBLOCKED;
    let mut handler_ids = Vec::new();
    loop {
        let handler_id = unsafe {
            glib::gobject_ffi::g_signal_handler_find(
                instance,
                mask,
                signal_id,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if handler_id == 0 {
            break;
        }
        unsafe { glib::gobject_ffi::g_signal_handler_block(instance, handler_id) };
        handler_ids.push(handler_id);
    }

    let latest = handler_ids.iter().copied().max();
    for handler_id in handler_ids {
        if Some(handler_id) == latest {
            unsafe { glib::gobject_ffi::g_signal_handler_disconnect(instance, handler_id) };
        } else {
            unsafe { glib::gobject_ffi::g_signal_handler_unblock(instance, handler_id) };
        }
    }
    latest.is_some()
}

#[cfg(target_os = "linux")]
unsafe fn replace_tauri_gtk_resize_handler(webview: &webkit2gtk::WebView) {
    // Tauri's Linux resize hook walks `webview -> GtkBox -> GtkWindow` and
    // unwraps the final cast. The native viewport deliberately inserts a
    // GtkOverlay between the box and webview, so leaving that hook installed
    // makes every primary-button press panic before GTK can dispatch it to
    // the application. Remove the hook that Wry attached at construction and
    // replace it with the same edge-resize behavior using Widget::toplevel(),
    // which remains valid after reparenting.
    if !unsafe { disconnect_latest_instance_handler(webview, b"button-press-event\0") } {
        eprintln!("WebKitGTK had no instance button resize handler to replace");
    }
    if !unsafe { disconnect_latest_instance_handler(webview, b"touch-event\0") } {
        eprintln!("WebKitGTK had no instance touch resize handler to replace");
    }

    webview.connect_button_press_event(|webview, event| {
        if event.button() != 1 {
            return glib::Propagation::Proceed;
        }
        let Some(window) = webview
            .toplevel()
            .and_then(|widget| widget.downcast::<gtk::Window>().ok())
        else {
            return glib::Propagation::Proceed;
        };
        if window.is_decorated() || !window.is_resizable() || window.is_maximized() {
            return glib::Propagation::Proceed;
        }
        let Some(gdk_window) = window.window() else {
            return glib::Propagation::Proceed;
        };
        let (root_x, root_y) = event.root();
        let (window_x, window_y) = gdk_window.position();
        let x = root_x - window_x as f64;
        let y = root_y - window_y as f64;
        let width = gdk_window.width() as f64;
        let height = gdk_window.height() as f64;
        let inset = (window.scale_factor() * 5) as f64;
        let west = x < inset;
        let east = x >= width - inset;
        let north = y < inset;
        let south = y >= height - inset;
        let edge = match (west, east, north, south) {
            (true, _, true, _) => Some(gtk::gdk::WindowEdge::NorthWest),
            (_, true, true, _) => Some(gtk::gdk::WindowEdge::NorthEast),
            (true, _, _, true) => Some(gtk::gdk::WindowEdge::SouthWest),
            (_, true, _, true) => Some(gtk::gdk::WindowEdge::SouthEast),
            (true, _, _, _) => Some(gtk::gdk::WindowEdge::West),
            (_, true, _, _) => Some(gtk::gdk::WindowEdge::East),
            (_, _, true, _) => Some(gtk::gdk::WindowEdge::North),
            (_, _, _, true) => Some(gtk::gdk::WindowEdge::South),
            _ => None,
        };
        if let Some(edge) = edge {
            gdk_window.begin_resize_drag(
                edge,
                1,
                root_x.round() as i32,
                root_y.round() as i32,
                event.time(),
            );
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
}

#[cfg(target_os = "linux")]
unsafe fn install_native_views(
    webview: webkit2gtk::WebView,
    pointers: Arc<NativePointers>,
    runtime: Arc<AtomicUsize>,
    pending: Arc<Mutex<PendingRenderCommands>>,
    metrics: Arc<Mutex<MetricsState>>,
    last_layout: Arc<Mutex<Option<ViewportLayout>>>,
) -> Result<(NativeViewHandle, f64), String> {
    let parent = webview
        .parent()
        .and_then(|widget| widget.downcast::<gtk::Box>().ok())
        .ok_or_else(|| "WebKitGTK is not attached to Tauri's GTK box".to_string())?;
    let child_index = parent
        .children()
        .iter()
        .position(|child| child == webview.upcast_ref::<gtk::Widget>())
        .unwrap_or(0) as i32;

    unsafe { replace_tauri_gtk_resize_handler(&webview) };

    // Hold a strong reference to the webview while it is temporarily removed
    // from Wry's default box and moved into the overlay.
    parent.remove(&webview);
    let overlay = gtk::Overlay::new();
    let fixed = gtk::Fixed::new();
    let viewport = gtk::DrawingArea::new();
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);
    fixed.set_hexpand(true);
    fixed.set_vexpand(true);
    viewport.set_app_paintable(true);
    viewport.set_can_focus(false);
    // GTK's Cairo backing store otherwise repaints this X11 child after wgpu
    // presents, replacing the Bevy frame with the widget's theme background.
    // The DrawingArea is an external Vulkan target and Bevy is its sole
    // painter, matching Tao's X11 setup for externally rendered windows.
    unsafe {
        let widget = viewport.upcast_ref::<gtk::Widget>();
        gtk::ffi::gtk_widget_set_double_buffered(widget.to_glib_none().0, 0);
    }
    viewport.set_size_request(INITIAL_PHYSICAL_SIZE as i32, INITIAL_PHYSICAL_SIZE as i32);
    // `size-allocate` and `draw` describe GTK's requested/cached geometry.
    // Vulkan's X11 surface capabilities do not change until the server emits
    // ConfigureNotify. Make that event the single authority for swapchain
    // dimensions and coalesce bursts to the newest confirmed extent.
    let configured_size = Arc::new(AtomicU64::new(0));
    let render_scheduled = Arc::new(AtomicBool::new(false));
    viewport.connect_configure_event(move |viewport, event| {
        let (logical_width, logical_height) = event.size();
        // GTK realizes the DrawingArea at its 1x1 placeholder allocation
        // before React supplies the actual viewport rectangle. Creating or
        // updating a FIFO swapchain for that transient drawable can consume
        // every image before X11 has a presentable window. Wait for a real
        // allocation; the later ConfigureNotify is the authoritative first
        // render and resize signal.
        if logical_width < 32 || logical_height < 32 {
            return false;
        }
        let size_key = (u64::from(logical_width.max(1)) << 32) | u64::from(logical_height.max(1));
        configured_size.store(size_key, Ordering::Release);
        if render_scheduled.swap(true, Ordering::AcqRel) {
            return false;
        }

        let runtime = runtime.clone();
        let pending = pending.clone();
        let metrics = metrics.clone();
        let last_layout = last_layout.clone();
        let configured_size = configured_size.clone();
        let render_scheduled = render_scheduled.clone();
        let viewport = viewport.clone();
        let scale_factor = f64::from(viewport.scale_factor().max(1));
        // ConfigureNotify is emitted while GTK may still have a queued Cairo
        // paint for this child. Present on the next frame boundary so GTK has
        // finished its layout/draw pass before Bevy owns the X11 drawable.
        glib::timeout_add_local_once(Duration::from_millis(16), move || {
            let size_key = configured_size.load(Ordering::Acquire);
            let logical_width = (size_key >> 32) as u32;
            let logical_height = size_key as u32;
            if let Some(layout) = last_layout.lock().ok().and_then(|current| current.clone()) {
                if let Some(window) = viewport.window() {
                    apply_linux_viewport_region(
                        &window,
                        &layout,
                        logical_width.max(1) as i32,
                        logical_height.max(1) as i32,
                    );
                }
                push_render_command(
                    &pending,
                    RenderCommand::Resize {
                        logical_width: f64::from(logical_width.max(1)),
                        logical_height: f64::from(logical_height.max(1)),
                        scale_factor,
                        palette: layout.palette,
                        hud: layout.hud,
                    },
                );
                drain_render_commands(&runtime, &pending, &metrics);
            }
            render_scheduled.store(false, Ordering::Release);
        });
        false
    });
    viewport.connect_draw(|_, _| glib::Propagation::Stop);
    fixed.put(&viewport, 0, 0);

    webview.set_hexpand(true);
    webview.set_vexpand(true);
    overlay.add(&webview);
    overlay.add_overlay(&fixed);
    overlay.set_overlay_pass_through(&fixed, true);
    parent.pack_start(&overlay, true, true, 0);
    parent.reorder_child(&overlay, child_index);
    overlay.show_all();
    viewport.realize();
    viewport.map();

    let gdk_window = viewport
        .window()
        .ok_or_else(|| "GTK did not realize a native viewport window".to_string())?;
    // Bevy receives pointer input through the React bridge. Its native X11
    // child must therefore be input-transparent; otherwise that child owns
    // every click inside the viewport rectangle and DOM controls revealed by
    // the X11 shape cutouts become inert.
    gdk_window.set_pass_through(true);
    let gdk_display = gdk_window.display();
    let backend_name = gdk_display.type_().name();
    if !backend_name.contains("X11") {
        return Err(format!(
            "unsupported GTK display backend {backend_name}; the embedded viewport requires X11 or XWayland"
        ));
    }
    let xid = unsafe { gdk_x11_sys::gdk_x11_window_get_xid(gdk_window.as_ptr().cast()) };
    let display = unsafe { gdk_x11_sys::gdk_x11_display_get_xdisplay(gdk_display.as_ptr().cast()) };
    if xid == 0 {
        return Err("GDK X11 viewport window has no XID".to_string());
    }
    let view_handle = NativeViewHandle::xlib(
        xid as u64,
        NonNull::new(display.cast())
            .ok_or_else(|| "GDK X11 display is not available".to_string())?,
    );

    pointers
        .webview
        .store(webview.as_ptr() as usize, Ordering::Release);
    pointers
        .viewport
        .store(viewport.as_ptr() as usize, Ordering::Release);
    pointers
        .window
        .store(fixed.as_ptr() as usize, Ordering::Release);

    Ok((view_handle, viewport.scale_factor().max(1) as f64))
}

#[cfg(target_os = "linux")]
unsafe fn apply_native_layout(
    webview_pointer: usize,
    viewport_pointer: usize,
    window_pointer: usize,
    layout: &ViewportLayout,
) -> f64 {
    let webview = unsafe {
        from_glib_borrow::<_, webkit2gtk::WebView>(
            webview_pointer as *mut webkit2gtk::ffi::WebKitWebView,
        )
    };
    let viewport = unsafe {
        from_glib_borrow::<_, gtk::DrawingArea>(viewport_pointer as *mut gtk::ffi::GtkDrawingArea)
    };
    let fixed =
        unsafe { from_glib_borrow::<_, gtk::Fixed>(window_pointer as *mut gtk::ffi::GtkFixed) };
    let scale_factor = viewport.scale_factor().max(1) as f64;
    let rect = layout.viewport;
    if rect.width < 2.0 || rect.height < 2.0 {
        // Keep the Vulkan child mapped, sized, and in the same X11 stacking
        // order so its swapchain remains presentable. Moving it outside the
        // parent allocation exposes the opaque WebView for Drawing without
        // relying on transparent GTK compositing or an empty XShape region.
        park_linux_viewport(&webview, &viewport, &fixed);
        return scale_factor;
    }

    // DOM client coordinates and GTK allocations are both logical pixels;
    // wgpu applies the widget scale factor when it configures the surface.
    let width = rect.width.round().max(1.0) as i32;
    let height = rect.height.round().max(1.0) as i32;
    fixed.move_(&*viewport, rect.x.round() as i32, rect.y.round() as i32);
    viewport.set_size_request(width, height);
    viewport.show();
    viewport.queue_resize();
    if let Some(window) = viewport.window() {
        webview.queue_draw();
        apply_linux_viewport_region(&window, layout, width, height);
        // `GtkFixed::move_` updates the X11 child allocation asynchronously.
        // When the viewport returns from Drawing, GTK first carries forward
        // the empty clip it computed while the child was offscreen. Restore
        // the application-owned overlay cutouts after that allocation pass;
        // doing this synchronously is overwritten by GTK a few milliseconds
        // later and leaves an otherwise healthy Vulkan surface invisible.
        let deferred_window = window.clone();
        let deferred_layout = layout.clone();
        glib::timeout_add_local_once(Duration::from_millis(16), move || {
            apply_linux_viewport_region(&deferred_window, &deferred_layout, width, height);
            deferred_window.invalidate_rect(None, false);
        });
    }
    scale_factor
}

#[cfg(target_os = "linux")]
fn park_linux_viewport(
    webview: &webkit2gtk::WebView,
    viewport: &gtk::DrawingArea,
    fixed: &gtk::Fixed,
) {
    // Hiding or unmapping the DrawingArea destroys wgpu's presentation path
    // on some X11 drivers. Keep the child mapped and park it beyond the
    // GtkFixed allocation while a non-3D workspace or native dialog owns the
    // window, then expose the opaque WebView immediately.
    let hidden_x = -viewport.allocated_width().max(1) - 16;
    fixed.move_(viewport, hidden_x, 0);
    viewport.show();
    webview.queue_draw();
}

#[cfg(target_os = "linux")]
fn apply_linux_viewport_region(
    window: &gdk::Window,
    layout: &ViewportLayout,
    width: i32,
    height: i32,
) {
    use gdk::cairo::{RectangleInt, Region};

    let visible = Region::create_rectangle(&RectangleInt::new(0, 0, width, height));
    for overlay in &layout.overlays {
        let Some(intersection) = intersect_rect(*overlay, layout.viewport) else {
            continue;
        };
        let left = (intersection.x - layout.viewport.x)
            .floor()
            .clamp(0.0, f64::from(width)) as i32;
        let top = (intersection.y - layout.viewport.y)
            .floor()
            .clamp(0.0, f64::from(height)) as i32;
        let right = (intersection.x + intersection.width - layout.viewport.x)
            .ceil()
            .clamp(0.0, f64::from(width)) as i32;
        let bottom = (intersection.y + intersection.height - layout.viewport.y)
            .ceil()
            .clamp(0.0, f64::from(height)) as i32;
        if right <= left || bottom <= top {
            continue;
        }

        let radius = intersection
            .corner_radius
            .round()
            .clamp(0.0, f64::from((right - left).min(bottom - top)) / 2.0)
            as i32;
        let cutout = if radius <= 1 {
            Region::create_rectangle(&RectangleInt::new(left, top, right - left, bottom - top))
        } else {
            rounded_linux_region(left, top, right, bottom, radius)
        };
        if let Err(error) = visible.subtract(&cutout) {
            eprintln!("could not clip a DOM overlay from the Linux viewport: {error}");
        }
    }
    window.shape_combine_region(Some(&visible), 0, 0);
}

#[cfg(target_os = "linux")]
fn rounded_linux_region(
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    radius: i32,
) -> gdk::cairo::Region {
    use gdk::cairo::{RectangleInt, Region};

    // Cairo regions are integer rectangles. Build one scan line per corner
    // row so rounded DOM dialogs retain their true silhouette instead of
    // exposing rectangular flashes around their transparent corner pixels.
    let mut rows = Vec::with_capacity((bottom - top).max(0) as usize);
    let radius_f = f64::from(radius);
    for y in top..bottom {
        let local_y = y - top;
        let from_nearest_edge = if local_y < radius {
            local_y
        } else if local_y >= bottom - top - radius {
            bottom - top - 1 - local_y
        } else {
            radius
        };
        let inset = if from_nearest_edge >= radius {
            0
        } else {
            let dy = radius_f - (f64::from(from_nearest_edge) + 0.5);
            (radius_f - (radius_f * radius_f - dy * dy).max(0.0).sqrt()).floor() as i32
        };
        let row_left = left + inset;
        let row_right = right - inset;
        if row_right > row_left {
            rows.push(RectangleInt::new(row_left, y, row_right - row_left, 1));
        }
    }
    Region::create_rectangles(&rows)
}

#[cfg(target_os = "linux")]
unsafe fn set_native_viewport_suspended(
    webview_pointer: usize,
    viewport_pointer: usize,
    window_pointer: usize,
    suspended: bool,
    layout: Option<&ViewportLayout>,
) {
    let webview = unsafe {
        from_glib_borrow::<_, webkit2gtk::WebView>(
            webview_pointer as *mut webkit2gtk::ffi::WebKitWebView,
        )
    };
    let viewport = unsafe {
        from_glib_borrow::<_, gtk::DrawingArea>(viewport_pointer as *mut gtk::ffi::GtkDrawingArea)
    };
    let fixed =
        unsafe { from_glib_borrow::<_, gtk::Fixed>(window_pointer as *mut gtk::ffi::GtkFixed) };
    if suspended {
        park_linux_viewport(&webview, &viewport, &fixed);
    } else if let Some(layout) = layout {
        let _ = unsafe {
            apply_native_layout(webview_pointer, viewport_pointer, window_pointer, layout)
        };
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux", test))]
fn intersect_rect(a: ViewportRect, b: ViewportRect) -> Option<ViewportRect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > left && bottom > top).then_some(ViewportRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
        corner_radius: if left == a.x
            && top == a.y
            && right == a.x + a.width
            && bottom == a.y + a.height
        {
            a.corner_radius
                .min((right - left) / 2.0)
                .min((bottom - top) / 2.0)
        } else {
            0.0
        },
    })
}

#[derive(Debug, Clone, Copy)]
struct NativeViewHandle {
    window: RawWindowHandle,
    display: RawDisplayHandle,
}

unsafe impl Send for NativeViewHandle {}
unsafe impl Sync for NativeViewHandle {}

impl NativeViewHandle {
    fn display_backend_name(&self) -> &'static str {
        match self.display {
            RawDisplayHandle::AppKit(_) => "appkit",
            RawDisplayHandle::Windows(_) => "win32",
            RawDisplayHandle::Xlib(_) => "x11",
            _ => "unknown",
        }
    }

    #[cfg(target_os = "macos")]
    fn appkit(view_pointer: usize) -> Self {
        let pointer =
            NonNull::new(view_pointer as *mut c_void).expect("NSView handle cannot be null");
        Self {
            window: RawWindowHandle::AppKit(AppKitWindowHandle::new(pointer)),
            display: RawDisplayHandle::AppKit(AppKitDisplayHandle::new()),
        }
    }

    #[cfg(target_os = "windows")]
    fn win32(view_pointer: usize) -> Self {
        let pointer =
            NonZeroIsize::new(view_pointer as isize).expect("viewport HWND cannot be null");
        let module = unsafe { GetModuleHandleW(std::ptr::null()) };
        let mut handle = Win32WindowHandle::new(pointer);
        // Vulkan requires this field. Without it wgpu can create only a DX12
        // surface, which excludes adapters exposed only through Vulkan.
        handle.hinstance = NonZeroIsize::new(module as isize);
        Self {
            window: RawWindowHandle::Win32(handle),
            display: RawDisplayHandle::Windows(WindowsDisplayHandle::new()),
        }
    }

    #[cfg(target_os = "linux")]
    fn xlib(window: u64, display: NonNull<c_void>) -> Self {
        Self {
            window: RawWindowHandle::Xlib(XlibWindowHandle::new(window as _)),
            display: RawDisplayHandle::Xlib(XlibDisplayHandle::new(Some(display), 0)),
        }
    }
}

impl HasWindowHandle for NativeViewHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Ok(unsafe { WindowHandle::borrow_raw(self.window) })
    }
}

impl HasDisplayHandle for NativeViewHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(unsafe { DisplayHandle::borrow_raw(self.display) })
    }
}

#[derive(Resource, Default)]
struct ModelResource {
    session_id: String,
    geometry_revision: u64,
    scene: SolidSceneDto,
    active_sketch: Option<SketchDto>,
    finished_sketches: Vec<SketchDto>,
    datum_planes: Vec<DatumPlaneDefinitionDto>,
    profile_catalog: Vec<ProfileCatalogItemDto>,
    body_appearances: Vec<BodyAppearance>,
    body_poses: Vec<BodyPoseDto>,
    instance_body_poses: Vec<InstanceBodyPoseDto>,
    instance_revision: u64,
    revision: u64,
}

#[derive(Resource, Default)]
struct ModelGeometryCache(HashMap<String, (u64, u64)>);

#[derive(Resource)]
struct CameraResource {
    camera: ViewportCamera,
    revision: u64,
}

#[derive(Resource, Default)]
struct PreviewResource {
    value: ViewportPreview,
    revision: u64,
    /// Changes only when GPU mesh content changes. Screen annotations and
    /// sketch gizmos may update at camera frequency without reallocating the
    /// retained profile/tool meshes.
    mesh_revision: u64,
}

#[derive(Resource, Clone, Copy, Default)]
struct PaletteResource(ViewportPalette);

#[derive(Resource)]
struct HudResource {
    hud: ViewportHud,
    revision: u64,
}

#[derive(Resource, Clone, Copy)]
struct ViewportSizeResource {
    logical_width: f32,
    logical_height: f32,
}

impl Default for ViewportSizeResource {
    fn default() -> Self {
        Self {
            logical_width: INITIAL_PHYSICAL_SIZE as f32,
            logical_height: INITIAL_PHYSICAL_SIZE as f32,
        }
    }
}

#[derive(Resource, Default)]
struct PresentationResource(ViewportPresentation);

impl Default for HudResource {
    fn default() -> Self {
        Self {
            hud: ViewportHud::default(),
            revision: 1,
        }
    }
}

impl Default for CameraResource {
    fn default() -> Self {
        Self {
            camera: ViewportCamera::default(),
            revision: 1,
        }
    }
}

#[derive(Resource, Default)]
struct RenderedRevisions {
    model: u64,
    camera: u64,
    hud: u64,
    annotations: u64,
    preview_meshes: u64,
}

#[derive(Component)]
struct NativeCadBody {
    body_id: u64,
    occurrence_id: Option<u64>,
}

#[derive(Component)]
struct NativeCadFace {
    body_id: u64,
    occurrence_id: Option<u64>,
    face_id: u64,
    boundary: Vec<(Vec3, Vec3)>,
}

#[derive(Component)]
struct NativeCadFaceOverlay {
    body_id: u64,
    occurrence_id: Option<u64>,
}

#[derive(Component)]
struct NativeModelGeometry {
    session_id: String,
    geometry_revision: u64,
    instance_revision: u64,
}

#[derive(Component)]
struct NativeCadCamera;

/// Common marker for the model camera and the depth-independent transient
/// overlay camera. Both always receive exactly the same projection.
#[derive(Component)]
struct NativeViewportCamera;

#[derive(Component)]
struct NativeOverlayCamera;

#[derive(Component)]
struct NativePreviewMesh;

#[derive(Clone, Copy)]
enum NativePreviewArrowPartKind {
    Shaft,
    Head,
    Base,
}

/// Semantic arrow data retained on each unit primitive. Camera movement only
/// updates these transforms; it never allocates a new Mesh or Material.
#[derive(Component, Clone, Copy)]
struct NativePreviewArrowPart {
    start: Vec3,
    end: Vec3,
    width: f32,
    kind: NativePreviewArrowPartKind,
}

#[derive(Component)]
struct CadKeyLight;

#[derive(Component)]
struct CadFillLight;

#[derive(Component)]
struct NativeDatumPlane {
    datum_id: u64,
}

#[derive(Component, Clone, Copy)]
struct NativeOriginPlane {
    plane: ViewportOriginPlane,
}

#[derive(Component)]
struct NativeAnnotationRoot;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadHighlightGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadSketchGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadPickFeedbackGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadDirectPickFeedbackGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadProfileBorderGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadPickFeedbackHaloGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadSketchPointOutlineGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadSketchPointGizmos;

fn cad_render_plugin() -> RenderPlugin {
    let render_plugin = RenderPlugin {
        // This renderer advances only in response to bridge commands. On
        // Windows, asynchronously compiled PBR/UI pipelines can otherwise
        // finish after the two-frame render burst and remain invisible
        // until unrelated input happens to wake Bevy again. Blocking the
        // pipeline queue keeps origin-plane fills and native HUD chrome
        // deterministic. Bevy ignores this setting on macOS.
        synchronous_pipeline_compilation: true,
        ..default()
    };
    #[cfg(target_os = "linux")]
    let render_plugin = RenderPlugin {
        // Bevy's native default requires adapter-specific texture format
        // features. They are optional for this viewport and are not exposed
        // by every conforming Vulkan adapter (notably Mesa lavapipe and some
        // older integrated GPUs). Stay within the WebGPU portability profile
        // so startup can fall back to those adapters without weakening the
        // Vulkan-only production backend.
        render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
            priority: WgpuSettingsPriority::WebGPU,
            features: WgpuFeatures::empty(),
            ..default()
        })),
        ..render_plugin
    };
    render_plugin
}

fn build_bevy_app(
    view_handle: NativeViewHandle,
    scale_factor: f32,
) -> Result<bevy::app::App, String> {
    let mut app = bevy::app::App::new();
    #[cfg(target_os = "linux")]
    let present_mode = PresentMode::Fifo;
    #[cfg(not(target_os = "linux"))]
    let present_mode = PresentMode::AutoNoVsync;
    let plugins = DefaultPlugins
        .build()
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "noBS CAD embedded viewport".to_string(),
                resolution: WindowResolution::new(INITIAL_PHYSICAL_SIZE, INITIAL_PHYSICAL_SIZE)
                    .with_scale_factor_override(scale_factor.max(1.0)),
                visible: true,
                // FIFO is the only Vulkan presentation mode guaranteed on
                // every Linux surface. Event-driven rendering still sleeps at
                // idle, while avoiding resize-loss behavior seen with
                // mailbox/immediate surfaces on X11 software and older GPUs.
                present_mode,
                desired_maximum_frame_latency: NonZeroU32::new(2),
                ..default()
            }),
            primary_cursor_options: None,
            exit_condition: ExitCondition::DontExit,
            close_when_requested: false,
        })
        .set(cad_render_plugin());
    #[cfg(target_os = "linux")]
    let plugins = plugins.disable::<PipelinedRenderingPlugin>();
    app.add_plugins(plugins);

    let (window_entity, holder) = {
        let world = app.world_mut();
        let mut query =
            world.query_filtered::<(Entity, &RawHandleWrapperHolder), With<PrimaryWindow>>();
        let (entity, holder) = query
            .single(world)
            .map_err(|error| format!("Bevy primary window entity is missing: {error}"))?;
        (entity, holder.clone())
    };

    let wrapped_view = WindowWrapper::new(view_handle);
    let raw_handle = RawHandleWrapper::new(&wrapped_view)
        .map_err(|error| format!("could not wrap the embedded native view: {error}"))?;
    *holder
        .0
        .lock()
        .map_err(|_| "Bevy raw-window handle lock poisoned".to_string())? =
        Some(raw_handle.clone());
    app.world_mut().entity_mut(window_entity).insert(raw_handle);

    install_cad_scene(&mut app);
    app.finish();
    app.cleanup();
    Ok(app)
}

/// The embedded viewport and immutable script previews run these exact systems
/// in separate worlds. A preview never replaces the live model or camera.
fn install_cad_scene(app: &mut bevy::app::App) {
    app.init_gizmo_group::<CadHighlightGizmos>()
        .init_gizmo_group::<CadSketchGizmos>()
        .init_gizmo_group::<CadPickFeedbackHaloGizmos>()
        .init_gizmo_group::<CadPickFeedbackGizmos>()
        .init_gizmo_group::<CadDirectPickFeedbackGizmos>()
        .init_gizmo_group::<CadProfileBorderGizmos>()
        .init_gizmo_group::<CadSketchPointOutlineGizmos>()
        .init_gizmo_group::<CadSketchPointGizmos>();
    let initial_palette = ViewportPalette::default();
    app.insert_resource(ClearColor(rgb(initial_palette.background)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(1.0, 1.0, 1.0),
            brightness: 900.0,
            ..default()
        })
        .init_resource::<ModelResource>()
        .init_resource::<ModelGeometryCache>()
        .init_resource::<CameraResource>()
        .init_resource::<PreviewResource>()
        .init_resource::<PaletteResource>()
        .init_resource::<HudResource>()
        .init_resource::<ViewportSizeResource>()
        .init_resource::<PresentationResource>()
        .init_resource::<RenderedRevisions>()
        .init_resource::<ViewportUiAssets>()
        .add_systems(Startup, (ui::load_system_font, setup_scene).chain())
        .add_systems(
            Update,
            (
                rebuild_occt_meshes,
                apply_camera,
                resize_reference_planes,
                apply_native_presentation_styles,
                rebuild_native_face_overlays,
                apply_body_poses,
                rebuild_native_preview_meshes,
                update_native_preview_arrows,
                rebuild_native_annotations,
                rebuild_native_hud,
                update_native_hud_orientation,
                draw_cad_gizmos,
            )
                .chain(),
        );
}

#[derive(Resource)]
struct SceneImageTarget(Handle<Image>);

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gizmo_config: ResMut<GizmoConfigStore>,
    target: Option<Res<SceneImageTarget>>,
) {
    configure_viewport_line_widths(
        &mut gizmo_config,
        VIEWPORT_LINE_REFERENCE_DIAGONAL * 0.8,
        VIEWPORT_LINE_REFERENCE_DIAGONAL * 0.6,
        1.0,
    );
    let (highlight_config, _) = gizmo_config.config_mut::<CadHighlightGizmos>();
    highlight_config.depth_bias = -1.0;
    let (sketch_config, _) = gizmo_config.config_mut::<CadSketchGizmos>();
    // Visible sketches are reference graphics, not occluded model edges.
    // Match the browser renderer's depthTest:false contract so a sketch on a
    // face (or behind a body) remains readable until its eye toggle is hidden.
    sketch_config.depth_bias = SKETCH_DEPTH_BIAS;
    let (pick_feedback_config, _) = gizmo_config.config_mut::<CadPickFeedbackGizmos>();
    pick_feedback_config.depth_bias = PICK_FEEDBACK_DEPTH_BIAS;
    let (direct_pick_feedback_config, _) = gizmo_config.config_mut::<CadDirectPickFeedbackGizmos>();
    direct_pick_feedback_config.depth_bias = DIRECT_PICK_FEEDBACK_DEPTH_BIAS;
    let (profile_border_config, _) = gizmo_config.config_mut::<CadProfileBorderGizmos>();
    profile_border_config.depth_bias = PICK_FEEDBACK_DEPTH_BIAS;
    let (pick_halo_config, _) = gizmo_config.config_mut::<CadPickFeedbackHaloGizmos>();
    pick_halo_config.depth_bias = PICK_FEEDBACK_HALO_DEPTH_BIAS;
    let (point_outline_config, _) = gizmo_config.config_mut::<CadSketchPointOutlineGizmos>();
    point_outline_config.depth_bias = SKETCH_POINT_OUTLINE_DEPTH_BIAS;
    let (point_config, _) = gizmo_config.config_mut::<CadSketchPointGizmos>();
    point_config.depth_bias = SKETCH_DEPTH_BIAS;

    let camera = ViewportCamera::default();
    let (key_transform, fill_transform) = camera_relative_light_transforms(camera);
    let model_camera = commands
        .spawn((
            Name::new("React-synchronized CAD camera"),
            NativeViewportCamera,
            NativeCadCamera,
            Camera3d::default(),
            BoxShadowSamples(6),
            Projection::Perspective(PerspectiveProjection {
                fov: camera.vertical_fov_degrees.to_radians(),
                near: 0.1,
                far: 20_000.0,
                ..default()
            }),
            camera_transform(camera),
        ))
        .id();

    let overlay_camera = commands
        .spawn((
            Name::new("CAD transient overlay camera"),
            NativeViewportCamera,
            NativeOverlayCamera,
            IsDefaultUiCamera,
            Camera3d::default(),
            Camera {
                order: 1,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                fov: camera.vertical_fov_degrees.to_radians(),
                near: 0.1,
                far: 20_000.0,
                ..default()
            }),
            camera_transform(camera),
            RenderLayers::layer(1),
        ))
        .id();
    if let Some(target) = target {
        for camera in [model_camera, overlay_camera] {
            commands
                .entity(camera)
                .insert(bevy::camera::RenderTarget::Image(target.0.clone().into()));
        }
    }

    commands.spawn((
        Name::new("CAD key light"),
        CadKeyLight,
        DirectionalLight {
            color: Color::srgb(1.0, 1.0, 1.0),
            illuminance: 2_200.0,
            shadow_maps_enabled: false,
            ..default()
        },
        key_transform,
    ));
    commands.spawn((
        Name::new("CAD fill light"),
        CadFillLight,
        DirectionalLight {
            color: Color::srgb(1.0, 1.0, 1.0),
            illuminance: 2_200.0,
            shadow_maps_enabled: false,
            ..default()
        },
        fill_transform,
    ));

    for (name, basis, color) in origin_plane_bases() {
        let plane = match name {
            "XY" => ViewportOriginPlane::Xy,
            "XZ" => ViewportOriginPlane::Xz,
            _ => ViewportOriginPlane::Yz,
        };
        commands.spawn((
            Name::new(format!("Origin plane {name}")),
            NativeOriginPlane { plane },
            Visibility::Hidden,
            Mesh3d(meshes.add(reference_plane_mesh(&basis, REFERENCE_PLANE_HALF_SIZE))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                cull_mode: None,
                ..default()
            })),
        ));
    }
}

fn rebuild_occt_meshes(
    mut commands: Commands,
    model: Res<ModelResource>,
    mut revisions: ResMut<RenderedRevisions>,
    mut cache: ResMut<ModelGeometryCache>,
    mut existing: Query<(
        Entity,
        &NativeModelGeometry,
        &mut Visibility,
        Option<&Mesh3d>,
        Option<&MeshMaterial3d<StandardMaterial>>,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    palette: Res<PaletteResource>,
) {
    if revisions.model == model.revision {
        return;
    }
    revisions.model = model.revision;

    for (entity, geometry, mut visibility, mesh, material) in &mut existing {
        if geometry.session_id == model.session_id {
            if geometry.geometry_revision != model.geometry_revision
                || geometry.instance_revision != model.instance_revision
            {
                if let Some(mesh) = mesh {
                    meshes.remove(&mesh.0);
                }
                if let Some(material) = material {
                    materials.remove(&material.0);
                }
                commands.entity(entity).despawn();
            }
        } else {
            *visibility = Visibility::Hidden;
        }
    }

    let cache_key = (model.geometry_revision, model.instance_revision);
    if cache.0.get(&model.session_id) == Some(&cache_key) {
        return;
    }
    cache.0.insert(model.session_id.clone(), cache_key);

    for body in &model.scene.bodies {
        let instances = if model.instance_body_poses.is_empty() {
            vec![None]
        } else {
            model
                .instance_body_poses
                .iter()
                .filter(|instance| instance.body_id == body.id && instance.visible)
                .map(|instance| Some(instance.occurrence_id.0))
                .collect::<Vec<_>>()
        };
        if instances.is_empty() {
            continue;
        }
        let mesh_handle = body_mesh(body).map(|mesh| meshes.add(mesh));
        for occurrence_id in instances {
            let transform = instance_body_pose_transform(
                &model.instance_body_poses,
                &model.body_poses,
                body.id.0,
                occurrence_id,
            );
            if let Some(mesh_handle) = &mesh_handle {
                commands.spawn((
                    Name::new(format!(
                        "OCCT body {} occurrence {:?} ({})",
                        body.id.0, occurrence_id, body.name
                    )),
                    NativeCadBody {
                        body_id: body.id.0,
                        occurrence_id,
                    },
                    NativeModelGeometry {
                        session_id: model.session_id.clone(),
                        geometry_revision: model.geometry_revision,
                        instance_revision: model.instance_revision,
                    },
                    Mesh3d(mesh_handle.clone()),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: body_appearance_color(&model, body.id.0, palette.0.body),
                        metallic: 0.0,
                        perceptual_roughness: 0.86,
                        cull_mode: None,
                        ..default()
                    })),
                    transform,
                ));
            }
            for face in &body.faces {
                commands.spawn((
                    Name::new(format!(
                        "OCCT face metadata {} on {} occurrence {:?} ({})",
                        face.id.0, body.id.0, occurrence_id, body.name
                    )),
                    NativeCadFace {
                        body_id: body.id.0,
                        occurrence_id,
                        face_id: face.id.0,
                        boundary: face_boundary_segments(body, face),
                    },
                    NativeModelGeometry {
                        session_id: model.session_id.clone(),
                        geometry_revision: model.geometry_revision,
                        instance_revision: model.instance_revision,
                    },
                    Visibility::Inherited,
                    transform,
                ));
            }
        }
    }

    for plane in &model.datum_planes {
        commands.spawn((
            Name::new(format!("Construction plane {}", plane.name)),
            NativeDatumPlane {
                datum_id: plane.datum_id.0,
            },
            NativeModelGeometry {
                session_id: model.session_id.clone(),
                geometry_revision: model.geometry_revision,
                instance_revision: model.instance_revision,
            },
            Mesh3d(meshes.add(reference_plane_mesh(
                &plane.basis,
                REFERENCE_PLANE_HALF_SIZE,
            ))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(0.85, 0.65, 0.30, 0.08),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                cull_mode: None,
                ..default()
            })),
        ));
    }
}

fn apply_camera(
    camera: Res<CameraResource>,
    image_target: Option<Res<SceneImageTarget>>,
    mut revisions: ResMut<RenderedRevisions>,
    mut query: Query<(&mut Transform, &mut Projection), With<NativeViewportCamera>>,
    mut key_lights: Query<
        &mut Transform,
        (
            With<CadKeyLight>,
            Without<CadFillLight>,
            Without<NativeViewportCamera>,
        ),
    >,
    mut fill_lights: Query<
        &mut Transform,
        (
            With<CadFillLight>,
            Without<CadKeyLight>,
            Without<NativeViewportCamera>,
        ),
    >,
) {
    if revisions.camera == camera.revision {
        return;
    }
    revisions.camera = camera.revision;
    for (mut transform, mut projection) in &mut query {
        *transform = camera_transform(camera.camera);
        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.fov = camera
                .camera
                .vertical_fov_degrees
                .clamp(1.0, 150.0)
                .to_radians();
            if image_target.is_some() {
                let distance = Vec3::from_array(camera.camera.position)
                    .distance(Vec3::from_array(camera.camera.target));
                perspective.near = (distance / 100_000.0).max(0.001);
                perspective.far = (distance * 3.0).max(100.0);
            }
        }
    }
    let (key_transform, fill_transform) = camera_relative_light_transforms(camera.camera);
    for mut transform in &mut key_lights {
        *transform = key_transform;
    }
    for mut transform in &mut fill_lights {
        *transform = fill_transform;
    }
}

/// A neutral, camera-relative two-light studio rig. The equal left/right
/// offsets remove the arbitrary world-side darkening that makes a CAD part
/// appear fixed under a room light while preserving gentle normal cues.
fn camera_relative_light_transforms(camera: ViewportCamera) -> (Transform, Transform) {
    let target = Vec3::from_array(camera.target);
    let eye = Vec3::from_array(camera.position);
    let view = (eye - target).normalize_or_zero();
    let view = if view.length_squared() < 1.0e-8 {
        Vec3::Y
    } else {
        view
    };
    let up_hint = Vec3::from_array(camera.up).normalize_or_zero();
    let up_hint = if up_hint.length_squared() < 1.0e-8 {
        Vec3::Z
    } else {
        up_hint
    };
    let right = view.cross(up_hint).normalize_or_zero();
    let right = if right.length_squared() < 1.0e-8 {
        view.any_orthonormal_vector()
    } else {
        right
    };
    let rig_distance = eye.distance(target).max(100.0);
    let key_position = target + (view + right * 0.28).normalize() * rig_distance;
    let fill_position = target + (view - right * 0.28).normalize() * rig_distance;
    (
        Transform::from_translation(key_position)
            .looking_at(target, stable_view_up(target - key_position, up_hint)),
        Transform::from_translation(fill_position)
            .looking_at(target, stable_view_up(target - fill_position, up_hint)),
    )
}

fn world_per_pixel_at(camera: ViewportCamera, viewport: ViewportSizeResource, origin: Vec3) -> f32 {
    let position = Vec3::from_array(camera.position);
    let forward = (Vec3::from_array(camera.target) - position).normalize_or_zero();
    let depth = (origin - position).dot(forward).max(0.2);
    let height = viewport.logical_height.max(1.0);
    2.0 * depth * (camera.vertical_fov_degrees.to_radians() * 0.5).tan() / height
}

fn viewport_line_logical_scale(logical_width: f32, logical_height: f32) -> f32 {
    if !logical_width.is_finite()
        || !logical_height.is_finite()
        || logical_width <= 0.0
        || logical_height <= 0.0
    {
        return 1.0;
    }
    (logical_width.hypot(logical_height) / VIEWPORT_LINE_REFERENCE_DIAGONAL)
        .clamp(VIEWPORT_LINE_SCALE_MIN, VIEWPORT_LINE_SCALE_MAX)
}

fn viewport_line_raster_scale(logical_width: f32, logical_height: f32, backing_scale: f32) -> f32 {
    viewport_line_logical_scale(logical_width, logical_height)
        * if backing_scale.is_finite() {
            backing_scale.max(0.5)
        } else {
            1.0
        }
}

fn configure_viewport_line_widths(
    gizmo_config: &mut GizmoConfigStore,
    logical_width: f32,
    logical_height: f32,
    backing_scale: f32,
) {
    let scale = viewport_line_raster_scale(logical_width, logical_height, backing_scale);
    gizmo_config.config_mut::<CadHighlightGizmos>().0.line.width = HIGHLIGHT_LINE_WIDTH * scale;
    gizmo_config.config_mut::<CadSketchGizmos>().0.line.width = SKETCH_LINE_WIDTH * scale;
    gizmo_config
        .config_mut::<CadPickFeedbackGizmos>()
        .0
        .line
        .width = PICK_FEEDBACK_LINE_WIDTH * scale;
    gizmo_config
        .config_mut::<CadDirectPickFeedbackGizmos>()
        .0
        .line
        .width = DIRECT_PICK_FEEDBACK_LINE_WIDTH * scale;
    gizmo_config
        .config_mut::<CadProfileBorderGizmos>()
        .0
        .line
        .width = PROFILE_BORDER_LINE_WIDTH * scale;
    gizmo_config
        .config_mut::<CadPickFeedbackHaloGizmos>()
        .0
        .line
        .width = PICK_FEEDBACK_HALO_LINE_WIDTH * scale;
    gizmo_config
        .config_mut::<CadSketchPointOutlineGizmos>()
        .0
        .line
        .width = SKETCH_POINT_OUTLINE_WIDTH * scale;
    gizmo_config
        .config_mut::<CadSketchPointGizmos>()
        .0
        .line
        .width = SKETCH_LINE_WIDTH * scale;
}

fn reference_plane_half_size(
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    origin: Vec3,
) -> f32 {
    world_per_pixel_at(camera, viewport, origin)
        * viewport.logical_width.min(viewport.logical_height).max(1.0)
        * (REFERENCE_PLANE_SCREEN_FRACTION * 0.5)
}

fn reference_plane_transform(origin: Vec3, half_size: f32) -> Transform {
    let scale = (half_size / REFERENCE_PLANE_HALF_SIZE).max(1.0e-6);
    Transform::from_translation(origin * (1.0 - scale)).with_scale(Vec3::splat(scale))
}

fn body_local_bounding_sphere(body: &BodyDto) -> Option<(Vec3, f32)> {
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    let mut count = 0usize;
    for point in body.mesh.positions.chunks_exact(3) {
        let point = Vec3::new(point[0], point[1], point[2]);
        minimum = minimum.min(point);
        maximum = maximum.max(point);
        count += 1;
    }
    if count == 0 {
        return None;
    }
    let center = (minimum + maximum) * 0.5;
    let radius = (maximum - center).length().max(1.0e-5);
    Some((center, radius))
}

fn occurrence_edges_are_visible(
    local_bounds: Option<(Vec3, f32)>,
    transform: &Transform,
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
) -> bool {
    let Some((local_center, local_radius)) = local_bounds else {
        return true;
    };
    let center = transform.transform_point(local_center);
    let radius = local_radius * transform.scale.max_element().abs().max(1.0e-6);
    let eye = Vec3::from_array(camera.position);
    let forward = (Vec3::from_array(camera.target) - eye).normalize_or_zero();
    let up_hint = Vec3::from_array(camera.up).normalize_or_zero();
    let right = forward.cross(up_hint).normalize_or_zero();
    let up = right.cross(forward).normalize_or_zero();
    if forward == Vec3::ZERO || right == Vec3::ZERO || up == Vec3::ZERO {
        return true;
    }
    let delta = center - eye;
    let depth = delta.dot(forward);
    if depth + radius <= 0.0 {
        return false;
    }
    let tangent = (camera.vertical_fov_degrees.to_radians() * 0.5).tan();
    let aspect = viewport.logical_width.max(1.0) / viewport.logical_height.max(1.0);
    if delta.dot(right).abs() > depth.max(0.0) * tangent * aspect + radius
        || delta.dot(up).abs() > depth.max(0.0) * tangent + radius
    {
        return false;
    }
    radius / world_per_pixel_at(camera, viewport, center) >= OCCURRENCE_EDGE_LOD_MIN_RADIUS_PX
}

fn resize_reference_planes(
    camera: Res<CameraResource>,
    viewport: Res<ViewportSizeResource>,
    model: Res<ModelResource>,
    mut origin_planes: Query<&mut Transform, (With<NativeOriginPlane>, Without<NativeDatumPlane>)>,
    mut datum_planes: Query<
        (&NativeDatumPlane, &NativeModelGeometry, &mut Transform),
        (Without<NativeOriginPlane>, Without<NativeCadFace>),
    >,
) {
    let size = *viewport;
    let origin_half_size = reference_plane_half_size(camera.camera, size, Vec3::ZERO);
    for mut transform in &mut origin_planes {
        *transform = reference_plane_transform(Vec3::ZERO, origin_half_size);
    }
    for (plane, geometry, mut transform) in &mut datum_planes {
        if geometry.session_id != model.session_id {
            continue;
        }
        let Some(definition) = model
            .datum_planes
            .iter()
            .find(|candidate| candidate.datum_id.0 == plane.datum_id)
        else {
            continue;
        };
        let origin = basis_vector(definition.basis.origin);
        let half_size = reference_plane_half_size(camera.camera, size, origin);
        *transform = reference_plane_transform(origin, half_size);
    }
}

#[allow(clippy::type_complexity)]
fn apply_native_presentation_styles(
    model: Res<ModelResource>,
    presentation: Res<PresentationResource>,
    palette: Res<PaletteResource>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bodies: Query<
        (
            &NativeCadBody,
            &NativeModelGeometry,
            &MeshMaterial3d<StandardMaterial>,
            &mut Visibility,
        ),
        (
            Without<NativeCadFace>,
            Without<NativeCadFaceOverlay>,
            Without<NativeDatumPlane>,
            Without<NativeOriginPlane>,
        ),
    >,
    mut datum_planes: Query<
        (
            &NativeDatumPlane,
            &NativeModelGeometry,
            &MeshMaterial3d<StandardMaterial>,
            &mut Visibility,
        ),
        (Without<NativeCadFace>, Without<NativeOriginPlane>),
    >,
    mut origin_planes: Query<
        (
            &NativeOriginPlane,
            &MeshMaterial3d<StandardMaterial>,
            &mut Visibility,
        ),
        (Without<NativeCadFace>, Without<NativeDatumPlane>),
    >,
) {
    let state = &presentation.0;
    for (body, geometry, handle, mut visibility) in &mut bodies {
        if geometry.session_id != model.session_id {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = if state.hidden_body_ids.contains(&body.body_id) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        let occurrence_is_selected = state
            .selected_occurrence_id
            .is_none_or(|occurrence_id| body.occurrence_id == Some(occurrence_id));
        let selected_body_index = occurrence_is_selected
            .then(|| {
                state
                    .selected_body_ids
                    .iter()
                    .position(|body_id| *body_id == body.body_id)
            })
            .flatten();
        let color = if selected_body_index == Some(0) {
            rgb(palette.0.body_selected)
        } else if selected_body_index.is_some() {
            rgb(palette.0.body_tool)
        } else {
            body_appearance_color(&model, body.body_id, palette.0.body)
        };
        material.base_color = color;
        material.emissive = if selected_body_index.is_some() {
            color.to_linear() * 0.08
        } else {
            LinearRgba::BLACK
        };
    }

    for (plane, geometry, handle, mut visibility) in &mut datum_planes {
        if geometry.session_id != model.session_id {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = if state.hidden_datum_plane_ids.contains(&plane.datum_id) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if let Some(mut material) = materials.get_mut(&handle.0) {
            let hovered = state.hovered_datum_plane_id == Some(plane.datum_id);
            let selected = state.selected_datum_plane_id == Some(plane.datum_id);
            let color = if selected {
                palette.0.selection
            } else if hovered {
                palette.0.hover
            } else if state.mode == ViewportMode::PickPlane {
                palette.0.active_sketch
            } else {
                [0.85, 0.65, 0.30]
            };
            material.base_color = rgba(
                color,
                if selected {
                    0.30
                } else if hovered {
                    0.24
                } else if state.mode == ViewportMode::PickPlane {
                    0.10
                } else {
                    0.08
                },
            );
        }
    }

    for (plane, handle, mut visibility) in &mut origin_planes {
        let visible = state.mode == ViewportMode::PickPlane;
        *visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if let Some(mut material) = materials.get_mut(&handle.0) {
            let hovered = state.hovered_origin_plane == Some(plane.plane);
            let selected = state.selected_origin_plane == Some(plane.plane);
            let base_color = origin_plane_color(&palette.0, plane.plane);
            material.base_color = rgba(
                base_color,
                if selected {
                    0.34
                } else if hovered {
                    0.28
                } else {
                    0.10
                },
            );
        }
    }
}

fn rebuild_native_face_overlays(
    mut commands: Commands,
    model: Res<ModelResource>,
    presentation: Res<PresentationResource>,
    palette: Res<PaletteResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<
        (Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>),
        With<NativeCadFaceOverlay>,
    >,
    mut last: Local<Option<(String, u64, u64, ViewportPresentation, ViewportPalette)>>,
) {
    let mut overlay_presentation = presentation.0.clone();
    overlay_presentation.body_poses.clear();
    overlay_presentation.instance_body_poses.clear();
    let next = (
        model.session_id.clone(),
        model.geometry_revision,
        model.instance_revision,
        overlay_presentation,
        palette.0,
    );
    if last.as_ref() == Some(&next) {
        return;
    }
    *last = Some(next);

    for (entity, mesh, material) in &existing {
        meshes.remove(&mesh.0);
        materials.remove(&material.0);
        commands.entity(entity).despawn();
    }

    let state = &presentation.0;
    let mut requested = state
        .selected_face_ids
        .iter()
        .copied()
        .map(|face_id| (face_id, true))
        .collect::<Vec<_>>();
    if let Some(face_id) = state.hovered_face_id {
        if !state.selected_face_ids.contains(&face_id) {
            requested.push((face_id, false));
        }
    }

    for (face_id, selected) in requested {
        let Some((body, face)) = model.scene.bodies.iter().find_map(|body| {
            body.faces
                .iter()
                .find(|face| face.id.0 == face_id)
                .map(|face| (body, face))
        }) else {
            continue;
        };
        if state.hidden_body_ids.contains(&body.id.0) {
            continue;
        }
        let Some(mesh) = face_mesh(body, face) else {
            continue;
        };
        let mesh_handle = meshes.add(mesh);
        let color = rgb(if selected {
            palette.0.face_selected
        } else {
            palette.0.face_hover
        });
        let material_handle = materials.add(StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * 0.32,
            metallic: 0.0,
            perceptual_roughness: 0.86,
            cull_mode: None,
            depth_bias: 1.0,
            ..default()
        });
        let occurrences = if model.instance_body_poses.is_empty() {
            vec![None]
        } else {
            model
                .instance_body_poses
                .iter()
                .filter(|instance| {
                    instance.body_id == body.id
                        && instance.visible
                        && if selected {
                            state.selected_occurrence_id.is_none()
                                || state.selected_occurrence_id == Some(instance.occurrence_id.0)
                        } else {
                            state.hovered_occurrence_id.is_none()
                                || state.hovered_occurrence_id == Some(instance.occurrence_id.0)
                        }
                })
                .map(|instance| Some(instance.occurrence_id.0))
                .collect::<Vec<_>>()
        };
        for occurrence_id in occurrences {
            commands.spawn((
                Name::new(format!(
                    "OCCT face highlight {face_id} occurrence {occurrence_id:?}"
                )),
                NativeCadFaceOverlay {
                    body_id: body.id.0,
                    occurrence_id,
                },
                NativeModelGeometry {
                    session_id: model.session_id.clone(),
                    geometry_revision: model.geometry_revision,
                    instance_revision: model.instance_revision,
                },
                Mesh3d(mesh_handle.clone()),
                MeshMaterial3d(material_handle.clone()),
                NotShadowCaster,
                NotShadowReceiver,
                instance_body_pose_transform(
                    &model.instance_body_poses,
                    &model.body_poses,
                    body.id.0,
                    occurrence_id,
                ),
            ));
        }
    }
}

fn apply_body_poses(
    model: Res<ModelResource>,
    mut entities: Query<(
        Option<&NativeCadBody>,
        Option<&NativeCadFace>,
        Option<&NativeCadFaceOverlay>,
        &mut Transform,
    )>,
) {
    if !model.is_changed() {
        return;
    }
    for (body, face, overlay, mut transform) in &mut entities {
        let identity = body
            .map(|body| (body.body_id, body.occurrence_id))
            .or_else(|| face.map(|face| (face.body_id, face.occurrence_id)))
            .or_else(|| overlay.map(|overlay| (overlay.body_id, overlay.occurrence_id)));
        if let Some((body_id, occurrence_id)) = identity {
            *transform = instance_body_pose_transform(
                &model.instance_body_poses,
                &model.body_poses,
                body_id,
                occurrence_id,
            );
        }
    }
}

/// Rebuilds only command-owned transient fills and manipulators. These meshes
/// are intentionally separate from OCCT scene geometry: they may be
/// translucent, depth-independent, and replaced on every debounced edit.
fn rebuild_native_preview_meshes(
    mut commands: Commands,
    preview: Res<PreviewResource>,
    mut revisions: ResMut<RenderedRevisions>,
    existing: Query<(Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>), With<NativePreviewMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if revisions.preview_meshes == preview.mesh_revision {
        return;
    }
    revisions.preview_meshes = preview.mesh_revision;

    for (entity, mesh, material) in &existing {
        meshes.remove(mesh.0.id());
        materials.remove(material.0.id());
        commands.entity(entity).despawn();
    }

    for layer in &preview.value.triangles {
        let positions = layer
            .positions
            .chunks_exact(3)
            .filter_map(|point| {
                point
                    .iter()
                    .all(|value| value.is_finite())
                    .then_some([point[0], point[1], point[2]])
            })
            .collect::<Vec<_>>();
        if positions.len() < 3 || positions.len() % 3 != 0 {
            continue;
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.compute_flat_normals();
        let color = Color::srgba(
            layer.color[0].clamp(0.0, 1.0),
            layer.color[1].clamp(0.0, 1.0),
            layer.color[2].clamp(0.0, 1.0),
            layer.color[3].clamp(0.0, 1.0),
        );
        let mut entity = commands.spawn((
            Name::new("Native command profile/tool fill"),
            NativePreviewMesh,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                depth_bias: 2.0,
                ..default()
            })),
            NotShadowCaster,
            NotShadowReceiver,
        ));
        if layer.xray {
            entity.insert(RenderLayers::layer(1));
        }
    }

    for arrow in &preview.value.arrows {
        let start = Vec3::from_array(arrow.start);
        let end = Vec3::from_array(arrow.end);
        if !start.is_finite() || !end.is_finite() {
            continue;
        }
        let delta = end - start;
        let length = delta.length();
        if !length.is_finite() || length <= 1.0e-5 {
            continue;
        }
        let color = Color::srgba(
            arrow.color[0].clamp(0.0, 1.0),
            arrow.color[1].clamp(0.0, 1.0),
            arrow.color[2].clamp(0.0, 1.0),
            arrow.color[3].clamp(0.0, 1.0),
        );
        let material = materials.add(StandardMaterial {
            base_color: color,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            depth_bias: 4.0,
            ..default()
        });
        let render_layer = arrow.xray.then(|| RenderLayers::layer(1));

        let mut shaft = commands.spawn((
            Name::new("Native Extrude direction shaft"),
            NativePreviewMesh,
            NativePreviewArrowPart {
                start,
                end,
                width: arrow.width,
                kind: NativePreviewArrowPartKind::Shaft,
            },
            Mesh3d(meshes.add(Cylinder::new(1.0, 1.0))),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            NotShadowCaster,
            NotShadowReceiver,
        ));
        if let Some(layer) = render_layer.clone() {
            shaft.insert(layer);
        }

        let mut head = commands.spawn((
            Name::new("Native Extrude direction head"),
            NativePreviewMesh,
            NativePreviewArrowPart {
                start,
                end,
                width: arrow.width,
                kind: NativePreviewArrowPartKind::Head,
            },
            Mesh3d(meshes.add(Cone::new(1.0, 1.0))),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            NotShadowCaster,
            NotShadowReceiver,
        ));
        if let Some(layer) = render_layer.clone() {
            head.insert(layer);
        }

        let mut base = commands.spawn((
            Name::new("Native Extrude direction origin"),
            NativePreviewMesh,
            NativePreviewArrowPart {
                start,
                end,
                width: arrow.width,
                kind: NativePreviewArrowPartKind::Base,
            },
            Mesh3d(meshes.add(Sphere::new(1.0))),
            MeshMaterial3d(material),
            Transform::default(),
            NotShadowCaster,
            NotShadowReceiver,
        ));
        if let Some(layer) = render_layer {
            base.insert(layer);
        }
    }
}

/// Preserve a constant logical-pixel arrow footprint without touching GPU
/// assets. This is intentionally cheap enough to run on every demanded frame.
fn update_native_preview_arrows(
    camera: Res<CameraResource>,
    viewport: Res<ViewportSizeResource>,
    mut arrows: Query<(&NativePreviewArrowPart, &mut Transform)>,
) {
    for (arrow, mut transform) in &mut arrows {
        let delta = arrow.end - arrow.start;
        let length = delta.length();
        if !length.is_finite() || length <= 1.0e-5 {
            *transform = Transform::from_scale(Vec3::ZERO);
            continue;
        }
        let direction = delta / length;
        let center = arrow.start + delta * 0.5;
        let world_per_pixel = world_per_pixel_at(camera.camera, *viewport, center);
        let width = arrow.width.clamp(1.0, 4.0);
        let shaft_radius = (world_per_pixel * width * 0.46).max(length * 0.003);
        let head_length = (world_per_pixel * 11.0)
            .max(length * 0.10)
            .min(length * 0.42);
        let shaft_length = (length - head_length).max(length * 0.05);
        let head_radius = (world_per_pixel * width * 2.2).max(shaft_radius * 2.5);
        let rotation = Quat::from_rotation_arc(Vec3::Y, direction);

        *transform = match arrow.kind {
            NativePreviewArrowPartKind::Shaft => {
                Transform::from_translation(arrow.start + direction * (shaft_length * 0.5))
                    .with_rotation(rotation)
                    .with_scale(Vec3::new(shaft_radius, shaft_length, shaft_radius))
            }
            NativePreviewArrowPartKind::Head => Transform::from_translation(
                arrow.start + direction * (shaft_length + head_length * 0.5),
            )
            .with_rotation(rotation)
            .with_scale(Vec3::new(head_radius, head_length, head_radius)),
            NativePreviewArrowPartKind::Base => {
                Transform::from_translation(arrow.start).with_scale(Vec3::splat(head_radius * 0.54))
            }
        };
    }
}

fn rebuild_native_annotations(
    mut commands: Commands,
    preview: Res<PreviewResource>,
    palette: Res<PaletteResource>,
    assets: Res<ViewportUiAssets>,
    mut revisions: ResMut<RenderedRevisions>,
    existing: Query<Entity, With<NativeAnnotationRoot>>,
    cameras: Query<Entity, With<NativeOverlayCamera>>,
) {
    if revisions.annotations == preview.revision {
        return;
    }
    revisions.annotations = preview.revision;

    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let Ok(camera) = cameras.single() else {
        return;
    };

    for annotation in &preview.value.annotations {
        if (annotation.text.trim().is_empty()
            && annotation.icon.is_none()
            && annotation.tool_icon.is_none())
            || !annotation.screen[0].is_finite()
            || !annotation.screen[1].is_finite()
        {
            continue;
        }
        let constraint = annotation.kind == ViewportAnnotationKind::Constraint;
        let tool = annotation.kind == ViewportAnnotationKind::Tool;
        let selected = annotation.selected;
        // `screen` is the projected center of the browser-side sprite, which
        // is also the pick target. Center every native mark on that point so
        // clicking the visible toolbar-derived glyph hits the same location.
        let annotation_transform = UiTransform::from_translation(Val2::percent(-50.0, -50.0));
        let foreground = if selected {
            rgb(palette.0.ink)
        } else {
            Color::srgba(
                annotation.color[0],
                annotation.color[1],
                annotation.color[2],
                annotation.color[3].clamp(0.0, 1.0),
            )
        };
        let (min_width, min_height, pad_x, pad_y, radius, font_size, border) = if constraint {
            if selected {
                (28.0, 24.0, 2.0, 1.0, 5.0, 11.0, 2.0)
            } else {
                // Existing relations read as quiet marks on the geometry,
                // not as a field of opaque UI buttons.
                (22.0, 22.0, 0.0, 0.0, 0.0, 9.0, 0.0)
            }
        } else if tool {
            (24.0, 24.0, 3.0, 3.0, 5.0, 9.0, 1.0)
        } else if selected {
            (28.0, 20.0, 5.0, 2.0, 5.0, 11.0, 2.0)
        } else {
            (24.0, 18.0, 4.0, 2.0, 5.0, 10.0, 1.0)
        };
        commands
            .spawn((
                Name::new(format!("Native viewport annotation {}", annotation.text)),
                NativeAnnotationRoot,
                UiTargetCamera(camera),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(annotation.screen[0]),
                    top: px(annotation.screen[1]),
                    min_width: px(min_width),
                    min_height: px(min_height),
                    padding: UiRect::axes(px(pad_x), px(pad_y)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(border)),
                    border_radius: BorderRadius::all(px(radius)),
                    ..default()
                },
                annotation_transform,
                BackgroundColor(if selected {
                    rgba(palette.0.accent, 0.92)
                } else if tool {
                    rgba(palette.0.header, 0.78)
                } else if constraint {
                    Color::NONE
                } else {
                    rgba(palette.0.header, 0.90)
                }),
                BorderColor::all(rgba(
                    if selected {
                        palette.0.selection
                    } else {
                        palette.0.ui_edge
                    },
                    if border == 0.0 {
                        0.0
                    } else if selected {
                        1.0
                    } else {
                        0.82
                    },
                )),
                ZIndex(if tool { 22 } else { 18 }),
            ))
            .with_children(|root| {
                if constraint {
                    if let Some(icon) = annotation.icon {
                        ui::spawn_constraint_icon(
                            root,
                            icon,
                            foreground,
                            if selected { 20.0 } else { 18.0 },
                        );
                        return;
                    }
                } else if tool {
                    if let Some(icon) = annotation.icon {
                        ui::spawn_constraint_icon(root, icon, foreground, 17.0);
                        return;
                    }
                    if let Some(icon) = annotation.tool_icon {
                        ui::spawn_tool_icon(root, icon, foreground, 16.0);
                        return;
                    }
                }
                root.spawn((
                    Text::new(annotation.text.clone()),
                    ViewportUiTheme::from_palette(&palette.0).text(
                        &assets,
                        font_size,
                        FontWeight::NORMAL,
                    ),
                    TextColor(foreground),
                ));
            });
    }
}

fn rebuild_native_hud(
    mut commands: Commands,
    hud: Res<HudResource>,
    palette: Res<PaletteResource>,
    assets: Res<ViewportUiAssets>,
    mut revisions: ResMut<RenderedRevisions>,
    existing: Query<Entity, With<NativeHudRoot>>,
    cameras: Query<Entity, With<NativeOverlayCamera>>,
) {
    if revisions.hud == hud.revision {
        return;
    }
    revisions.hud = hud.revision;

    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let Ok(camera) = cameras.single() else {
        return;
    };

    ui::spawn_viewport_hud(&mut commands, camera, &hud.hud, &palette.0, &assets);
}

fn update_native_hud_orientation(
    camera: Res<CameraResource>,
    mut marks: Query<(&HudAxisMark, &mut Node)>,
    mut labels: Query<(&HudAxisLabel, &mut Node), Without<HudAxisMark>>,
) {
    ui::update_orientation_nodes(camera.camera, &mut marks, &mut labels);
}

fn stable_view_up(direction: Vec3, up_hint: Vec3) -> Vec3 {
    let forward = direction.normalize_or_zero();
    if !forward.is_finite() || forward.length_squared() < 1.0e-8 {
        return Vec3::Z;
    }

    // Gram-Schmidt removes any component parallel to the view direction.
    // This remains valid at the exact top/bottom angles where a fixed Z-up
    // fallback would still be parallel and produce a degenerate transform.
    let mut up = up_hint.normalize_or_zero();
    up -= forward * up.dot(forward);
    if !up.is_finite() || up.length_squared() < 1.0e-8 {
        forward.any_orthonormal_vector()
    } else {
        up.normalize()
    }
}

fn camera_transform(camera: ViewportCamera) -> Transform {
    let position = Vec3::from_array(camera.position);
    let mut target = Vec3::from_array(camera.target);
    let direction = target - position;
    if !direction.is_finite() || direction.length_squared() < 1.0e-8 {
        target = position + Vec3::NEG_Z;
    }
    let up = stable_view_up(target - position, Vec3::from_array(camera.up));
    Transform::from_translation(position).looking_at(target, up)
}

fn origin_plane_bases() -> [(&'static str, PlaneBasis, Color); 3] {
    [
        (
            "XY",
            PlaneBasis {
                origin: [0.0, 0.0, 0.0],
                u: [1.0, 0.0, 0.0],
                v: [0.0, 1.0, 0.0],
                normal: [0.0, 0.0, 1.0],
            },
            Color::srgba(0.25, 0.60, 0.94, 0.055),
        ),
        (
            "XZ",
            PlaneBasis {
                origin: [0.0, 0.0, 0.0],
                u: [1.0, 0.0, 0.0],
                v: [0.0, 0.0, 1.0],
                normal: [0.0, -1.0, 0.0],
            },
            Color::srgba(0.31, 0.74, 0.47, 0.050),
        ),
        (
            "YZ",
            PlaneBasis {
                origin: [0.0, 0.0, 0.0],
                u: [0.0, 1.0, 0.0],
                v: [0.0, 0.0, 1.0],
                normal: [1.0, 0.0, 0.0],
            },
            Color::srgba(0.88, 0.36, 0.39, 0.050),
        ),
    ]
}

fn origin_plane_color(palette: &ViewportPalette, plane: ViewportOriginPlane) -> [f32; 3] {
    match plane {
        ViewportOriginPlane::Xy => palette.origin_plane_xy,
        ViewportOriginPlane::Xz => palette.origin_plane_xz,
        ViewportOriginPlane::Yz => palette.origin_plane_yz,
    }
}

fn reference_plane_mesh(basis: &PlaneBasis, half_size: f32) -> Mesh {
    let origin = basis_vector(basis.origin);
    let u = basis_vector(basis.u) * half_size;
    let v = basis_vector(basis.v) * half_size;
    let normal = basis_vector(basis.normal).normalize_or_zero();
    let positions = vec![
        (origin - u - v).to_array(),
        (origin + u - v).to_array(),
        (origin + u + v).to_array(),
        (origin - u + v).to_array(),
    ];
    let normals = vec![normal.to_array(); 4];

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    );
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

fn body_mesh(body: &BodyDto) -> Option<Mesh> {
    if body.mesh.positions.len() < 9
        || body.mesh.positions.len() % 3 != 0
        || body.mesh.normals.len() != body.mesh.positions.len()
        || body
            .mesh
            .indices
            .iter()
            .any(|index| (*index as usize) * 3 + 2 >= body.mesh.positions.len())
    {
        return None;
    }
    let positions = body
        .mesh
        .positions
        .chunks_exact(3)
        .map(|value| [value[0], value[1], value[2]])
        .collect::<Vec<_>>();
    let normals = body
        .mesh
        .normals
        .chunks_exact(3)
        .map(|value| [value[0], value[1], value[2]])
        .collect::<Vec<_>>();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(Indices::U32(body.mesh.indices.clone()));
    Some(mesh)
}

fn body_appearance_color(model: &ModelResource, body_id: u64, fallback: [f32; 3]) -> Color {
    let Some(appearance) = model
        .body_appearances
        .iter()
        .find(|appearance| appearance.body_id.0 == body_id)
    else {
        return rgb(fallback);
    };
    Color::srgb(
        f32::from(appearance.color.r) / 255.0,
        f32::from(appearance.color.g) / 255.0,
        f32::from(appearance.color.b) / 255.0,
    )
}

fn body_pose_transform(poses: &[BodyPoseDto], body_id: u64) -> Transform {
    let Some(pose) = poses.iter().find(|pose| pose.body_id.0 == body_id) else {
        return Transform::IDENTITY;
    };
    let rotation = Quat::from_xyzw(
        pose.rotation[0] as f32,
        pose.rotation[1] as f32,
        pose.rotation[2] as f32,
        pose.rotation[3] as f32,
    )
    .normalize();
    Transform::from_translation(Vec3::new(
        pose.translation[0] as f32,
        pose.translation[1] as f32,
        pose.translation[2] as f32,
    ))
    .with_rotation(rotation)
}

fn same_instance_layout(a: &[InstanceBodyPoseDto], b: &[InstanceBodyPoseDto]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.occurrence_id == b.occurrence_id
                && a.component_id == b.component_id
                && a.body_id == b.body_id
                && a.visible == b.visible
        })
}

fn instance_body_pose_transform(
    instances: &[InstanceBodyPoseDto],
    legacy: &[BodyPoseDto],
    body_id: u64,
    occurrence_id: Option<u64>,
) -> Transform {
    let pose = occurrence_id.and_then(|occurrence_id| {
        instances
            .iter()
            .find(|pose| pose.body_id.0 == body_id && pose.occurrence_id.0 == occurrence_id)
    });
    let Some(pose) = pose else {
        return body_pose_transform(legacy, body_id);
    };
    let rotation = Quat::from_xyzw(
        pose.rotation[0] as f32,
        pose.rotation[1] as f32,
        pose.rotation[2] as f32,
        pose.rotation[3] as f32,
    )
    .normalize();
    Transform::from_translation(Vec3::new(
        pose.translation[0] as f32,
        pose.translation[1] as f32,
        pose.translation[2] as f32,
    ))
    .with_rotation(rotation)
}

fn visible_body_occurrences(model: &ModelResource, body_id: u64) -> Vec<Option<u64>> {
    if model.instance_body_poses.is_empty() {
        return vec![None];
    }
    model
        .instance_body_poses
        .iter()
        .filter(|instance| instance.body_id.0 == body_id && instance.visible)
        .map(|instance| Some(instance.occurrence_id.0))
        .collect()
}

fn face_mesh(body: &BodyDto, face: &FaceDto) -> Option<Mesh> {
    let start = face.first_index as usize;
    let end = start
        .saturating_add(face.index_count as usize)
        .min(body.mesh.indices.len());
    let mut positions = Vec::with_capacity(end.saturating_sub(start));
    let mut normals = Vec::with_capacity(end.saturating_sub(start));
    for vertex in &body.mesh.indices[start..end] {
        let offset = *vertex as usize * 3;
        let position = body.mesh.positions.get(offset..offset + 3)?;
        positions.push([position[0], position[1], position[2]]);
        if let Some(normal) = body.mesh.normals.get(offset..offset + 3) {
            normals.push([normal[0], normal[1], normal[2]]);
        }
    }
    if positions.len() < 3 {
        return None;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    if normals.len() == end.saturating_sub(start) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    } else {
        mesh.compute_flat_normals();
    }
    Some(mesh)
}

fn draw_cad_gizmos(
    mut gizmos: Gizmos,
    mut sketch_gizmos: Gizmos<CadSketchGizmos>,
    mut sketch_point_outlines: Gizmos<CadSketchPointOutlineGizmos>,
    mut sketch_points: Gizmos<CadSketchPointGizmos>,
    mut highlights: Gizmos<CadHighlightGizmos>,
    mut pick_halo: Gizmos<CadPickFeedbackHaloGizmos>,
    mut pick_feedback: Gizmos<CadPickFeedbackGizmos>,
    mut direct_pick_feedback: Gizmos<CadDirectPickFeedbackGizmos>,
    mut profile_borders: Gizmos<CadProfileBorderGizmos>,
    model: Res<ModelResource>,
    camera: Res<CameraResource>,
    viewport: Res<ViewportSizeResource>,
    preview: Res<PreviewResource>,
    palette: Res<PaletteResource>,
    presentation: Res<PresentationResource>,
    face_boundaries: Query<(&NativeCadFace, &NativeModelGeometry)>,
) {
    let state = &presentation.0;
    let fine = rgba(palette.0.grid_fine, 0.28);
    let major = rgba(palette.0.grid_major, 0.48);

    // This viewport renders only when a bridge command marks it dirty. Bevy's
    // gizmo mesh updater drops a config group's asset handle when that group
    // emits no vertices. The next hover/selection then spends its only frame
    // recreating the asset, so the feedback can remain invisible until some
    // later pointer transition happens to render a second frame. Keep every
    // transient interaction group resident with a degenerate transparent
    // segment. It produces no pixels, but makes the first frame after an
    // invalid -> valid picker transition update an existing GPU asset.
    keep_gizmo_asset_resident(&mut highlights);
    keep_gizmo_asset_resident(&mut pick_halo);
    keep_gizmo_asset_resident(&mut pick_feedback);
    keep_gizmo_asset_resident(&mut direct_pick_feedback);
    keep_gizmo_asset_resident(&mut profile_borders);

    if state.mode == ViewportMode::Sketch {
        if let Some(sketch) = &model.active_sketch {
            draw_grid_on_basis(&mut gizmos, &sketch.basis, fine, major);
        }
    } else {
        draw_grid_on_basis(&mut gizmos, &origin_plane_bases()[0].1, fine, major);
    }

    if state.mode == ViewportMode::PickPlane {
        let origin_half_size = reference_plane_half_size(camera.camera, *viewport, Vec3::ZERO);
        for (name, basis, _) in origin_plane_bases() {
            let plane = match name {
                "XY" => ViewportOriginPlane::Xy,
                "XZ" => ViewportOriginPlane::Xz,
                _ => ViewportOriginPlane::Yz,
            };
            let selected = state.selected_origin_plane == Some(plane);
            let hovered = state.hovered_origin_plane == Some(plane);
            let base_color = origin_plane_color(&palette.0, plane);
            let alpha = if selected {
                0.98
            } else if hovered {
                0.92
            } else {
                0.42
            };
            // Origin planes are orientation references. Match main's desktop
            // interaction: hover brightens the permanent blue/green/red
            // plane; it never gains the generic cyan picker outline.
            draw_plane_outline(
                &mut highlights,
                &basis,
                origin_half_size,
                rgba(base_color, alpha),
            );
        }
        gizmos.sphere(
            Vec3::ZERO,
            origin_half_size * 0.0092,
            Color::srgba(0.94, 0.95, 0.98, 0.98),
        );
        let axis_length = origin_half_size * 0.28;
        gizmos.arrow(
            Vec3::ZERO,
            Vec3::X * axis_length,
            Color::srgba(0.88, 0.36, 0.39, 0.98),
        );
        gizmos.arrow(
            Vec3::ZERO,
            Vec3::Y * axis_length,
            Color::srgba(0.35, 0.68, 0.45, 0.98),
        );
        gizmos.arrow(
            Vec3::ZERO,
            Vec3::Z * axis_length,
            Color::srgba(0.26, 0.65, 0.91, 0.98),
        );
    } else if state.mode == ViewportMode::Sketch {
        if let Some(sketch) = &model.active_sketch {
            let origin = basis_vector(sketch.basis.origin);
            gizmos.sphere(origin, 0.38, rgba(palette.0.mute, 0.92));
        }
    }

    for plane in &model.datum_planes {
        if state.hidden_datum_plane_ids.contains(&plane.datum_id.0) {
            continue;
        }
        let hovered = state.hovered_datum_plane_id == Some(plane.datum_id.0);
        let selected = state.selected_datum_plane_id == Some(plane.datum_id.0);
        let origin = basis_vector(plane.basis.origin);
        let half_size = reference_plane_half_size(camera.camera, *viewport, origin);
        if selected || hovered {
            draw_plane_outline(
                &mut highlights,
                &plane.basis,
                half_size,
                rgb(if selected {
                    palette.0.selection
                } else {
                    palette.0.hover
                }),
            );
        } else {
            draw_plane_outline(
                &mut sketch_gizmos,
                &plane.basis,
                half_size,
                if state.mode == ViewportMode::PickPlane {
                    rgba(palette.0.active_sketch, 0.72)
                } else {
                    Color::srgba(0.88, 0.68, 0.32, 0.56)
                },
            );
        }
    }

    for body in &model.scene.bodies {
        if state.hidden_body_ids.contains(&body.id.0) {
            continue;
        }
        let local_bounds = body_local_bounding_sphere(body);
        for occurrence_id in visible_body_occurrences(&model, body.id.0) {
            let occurrence_is_selected = state
                .selected_occurrence_id
                .is_none_or(|selected| occurrence_id == Some(selected));
            let selected_body_index = occurrence_is_selected
                .then(|| {
                    state
                        .selected_body_ids
                        .iter()
                        .position(|body_id| *body_id == body.id.0)
                })
                .flatten();
            let hovered_body = state.hovered_body_id == Some(body.id.0)
                && state
                    .hovered_occurrence_id
                    .is_none_or(|hovered| occurrence_id == Some(hovered));
            let body_transform = instance_body_pose_transform(
                &model.instance_body_poses,
                &model.body_poses,
                body.id.0,
                occurrence_id,
            );
            let draw_default_edges = occurrence_edges_are_visible(
                local_bounds,
                &body_transform,
                camera.camera,
                *viewport,
            );

            if selected_body_index.is_some() || hovered_body {
                let color = if selected_body_index == Some(0) {
                    rgb(palette.0.face_selected)
                } else if selected_body_index.is_some() {
                    rgb(palette.0.edge_selected)
                } else {
                    rgb(palette.0.edge_hover)
                };
                for edge in &body.edges {
                    draw_edge_segments(
                        &mut pick_halo,
                        edge,
                        rgb(palette.0.pick_halo),
                        &body_transform,
                    );
                    draw_edge_segments(&mut pick_feedback, edge, color, &body_transform);
                }
            }

            for edge in &body.edges {
                let selected =
                    occurrence_is_selected && state.selected_edge_ids.contains(&edge.id.0);
                let hovered = state.hovered_edge_id == Some(edge.id.0)
                    && state
                        .hovered_occurrence_id
                        .is_none_or(|hovered| occurrence_id == Some(hovered));
                let eligible = (state.pick_refinable_edges && edge.refinable)
                    || (state.pick_straight_edges && edge_is_straight(edge));
                if !draw_default_edges
                    && !selected
                    && !hovered
                    && !eligible
                    && selected_body_index.is_none()
                    && !hovered_body
                {
                    continue;
                }
                let color = if selected {
                    palette.0.edge_selected
                } else if hovered {
                    palette.0.edge_hover
                } else if eligible {
                    palette.0.finished_sketch
                } else if selected_body_index.is_some() {
                    palette.0.body_selected_edge
                } else {
                    palette.0.edge
                };
                draw_edge_segments(&mut gizmos, edge, rgba(color, 0.92), &body_transform);
                if selected || hovered {
                    draw_edge_segments(
                        &mut pick_halo,
                        edge,
                        rgb(palette.0.pick_halo),
                        &body_transform,
                    );
                    draw_edge_segments(
                        &mut pick_feedback,
                        edge,
                        rgb(if selected {
                            palette.0.edge_selected
                        } else {
                            palette.0.edge_hover
                        }),
                        &body_transform,
                    );
                }
            }
        }
    }

    for (face, geometry) in &face_boundaries {
        if geometry.session_id != model.session_id || state.hidden_body_ids.contains(&face.body_id)
        {
            continue;
        }
        let occurrence_is_selected = state
            .selected_occurrence_id
            .is_none_or(|selected| face.occurrence_id == Some(selected));
        let selected = occurrence_is_selected && state.selected_face_ids.contains(&face.face_id);
        let hovered = state.hovered_face_id == Some(face.face_id)
            && state
                .hovered_occurrence_id
                .is_none_or(|hovered| face.occurrence_id == Some(hovered));
        if !selected && !hovered {
            continue;
        }
        let color = rgb(if selected {
            palette.0.edge_selected
        } else {
            palette.0.edge_hover
        });
        let transform = instance_body_pose_transform(
            &model.instance_body_poses,
            &model.body_poses,
            face.body_id,
            face.occurrence_id,
        );
        for (start, end) in &face.boundary {
            pick_halo.line(
                transform.transform_point(*start),
                transform.transform_point(*end),
                rgb(palette.0.pick_halo),
            );
            pick_feedback.line(
                transform.transform_point(*start),
                transform.transform_point(*end),
                color,
            );
        }
    }

    for sketch in &model.finished_sketches {
        if state.hidden_sketch_names.contains(&sketch.name) {
            continue;
        }
        let curve_color = rgba(palette.0.finished_sketch, 0.58);
        let profile_loops = model
            .profile_catalog
            .iter()
            .filter(|catalog| state.profile_picker_active && catalog.sketch_name == sketch.name)
            .flat_map(|catalog| {
                catalog.profiles.iter().filter(|profile| {
                    let outer_index = if profile.nesting_depth % 2 == 0 {
                        Some(profile.index)
                    } else {
                        profile.parent_index
                    };
                    state.candidate_profiles.iter().any(|reference| {
                        reference.sketch_name == catalog.sketch_name
                            && Some(reference.profile_index) == outer_index
                    })
                })
            })
            .collect::<Vec<_>>();
        let point_color = rgb(palette.0.finished_sketch_point);
        let point_outline_color = rgba(palette.0.finished_sketch_point_outline, 0.96);
        for entity in &sketch.entities {
            let (entity_id, _) = sketch_entity_style(entity);
            let direct_feedback = state
                .selected_finished_sketch_entities
                .iter()
                .any(|candidate| {
                    candidate.sketch_name == sketch.name && candidate.entity_id == entity_id
                })
                || state
                    .hovered_finished_sketch_entity
                    .as_ref()
                    .is_some_and(|candidate| {
                        candidate.sketch_name == sketch.name && candidate.entity_id == entity_id
                    });
            // The direct feedback pass replaces this base stroke. Rendering
            // both colors on the same curve produces a flickering, low-
            // contrast result at oblique camera angles.
            if !direct_feedback {
                draw_base_curve_outside_profiles(
                    &mut sketch_gizmos,
                    &sketch.basis,
                    entity,
                    curve_color,
                    &profile_loops,
                );
            }
            draw_sketch_entity_grips(
                &mut sketch_point_outlines,
                &sketch.basis,
                entity,
                camera.camera,
                *viewport,
                SKETCH_POINT_OUTLINE_RADIUS_PX,
                point_outline_color,
                false,
            );
            draw_sketch_entity_grips(
                &mut sketch_points,
                &sketch.basis,
                entity,
                camera.camera,
                *viewport,
                SKETCH_POINT_RADIUS_PX,
                point_color,
                false,
            );
        }
    }

    // Explicit solid-command profile selection takes priority over body
    // occlusion. This is essential for sketches on mid/offset planes inside a
    // part: the user must see the selectable section before choosing it for
    // Extrude, Revolve, Sweep, or Loft.
    if state.profile_picker_active {
        for catalog in &model.profile_catalog {
            if state.hidden_sketch_names.contains(&catalog.sketch_name) {
                continue;
            }
            // A direct line replaces every coincident profile stroke, not
            // just the finished-sketch base curve. Otherwise the profile's
            // wider candidate/halo/perimeter still surrounds the thin accent
            // and makes selection look covered by the original geometry.
            let replacement_lines = model
                .finished_sketches
                .iter()
                .filter(|sketch| sketch.name == catalog.sketch_name)
                .flat_map(|sketch| &sketch.entities)
                .filter_map(|entity| {
                    let EntityDto::Line { id, start, end, .. } = entity else {
                        return None;
                    };
                    let directly_highlighted = state
                        .selected_finished_sketch_entities
                        .iter()
                        .chain(state.hovered_finished_sketch_entity.iter())
                        .any(|reference| {
                            reference.sketch_name == catalog.sketch_name
                                && reference.entity_id == id.0
                        });
                    directly_highlighted.then_some([
                        Point2Dto::new(start.x, start.y),
                        Point2Dto::new(end.x, end.y),
                    ])
                })
                .collect::<Vec<_>>();
            for profile in catalog
                .profiles
                .iter()
                .filter(|candidate| candidate.nesting_depth % 2 == 0)
            {
                if !state.candidate_profiles.iter().any(|candidate| {
                    candidate.sketch_name == catalog.sketch_name
                        && candidate.profile_index == profile.index
                }) {
                    continue;
                }
                let selected = state.selected_profiles.iter().any(|candidate| {
                    candidate.sketch_name == catalog.sketch_name
                        && candidate.profile_index == profile.index
                });
                let hovered = state.hovered_profile.as_ref().is_some_and(|candidate| {
                    candidate.sketch_name == catalog.sketch_name
                        && candidate.profile_index == profile.index
                });
                let color = if selected {
                    rgb(palette.0.edge_selected)
                } else if hovered {
                    rgb(palette.0.edge_hover)
                } else {
                    rgba(palette.0.finished_sketch, 0.94)
                };
                let mut outline = profile_outline_segments(&profile.points, &replacement_lines);
                for hole in catalog.profiles.iter().filter(|candidate| {
                    candidate.nesting_depth % 2 == 1
                        && candidate.parent_index == Some(profile.index)
                }) {
                    outline.extend(profile_outline_segments(&hole.points, &replacement_lines));
                }
                draw_profile_segments(&mut profile_borders, &catalog.basis, &outline, color);
            }
        }
    }

    // Render direct pick feedback after every candidate layer and in its own
    // front-most gizmo group. Some modeling commands keep a selected profile
    // alive while one of its boundary lines is the axis/path input. Those two
    // strokes are identical in screen space, so source order alone cannot
    // prevent the gold profile perimeter from masking hover/selection.
    for sketch in &model.finished_sketches {
        if state.hidden_sketch_names.contains(&sketch.name) {
            continue;
        }
        for entity in &sketch.entities {
            let (entity_id, _) = sketch_entity_style(entity);
            let selected = state
                .selected_finished_sketch_entities
                .iter()
                .any(|candidate| {
                    candidate.sketch_name == sketch.name && candidate.entity_id == entity_id
                });
            let hovered = state
                .hovered_finished_sketch_entity
                .as_ref()
                .is_some_and(|candidate| {
                    candidate.sketch_name == sketch.name && candidate.entity_id == entity_id
                });
            if selected {
                // The application accent identifies an accepted command
                // input even when it is also one edge of a gold/orange
                // selected profile.
                draw_sketch_curve_at_offset(
                    &mut direct_pick_feedback,
                    &sketch.basis,
                    entity,
                    rgb(palette.0.accent),
                    DIRECT_PICK_FEEDBACK_OFFSET,
                );
            } else if hovered {
                // Hover replaces the exact source curve with one logical
                // pixel of state color. A halo or displaced companion makes
                // normal CAD geometry look materially thicker than it is.
                draw_sketch_curve_at_offset(
                    &mut direct_pick_feedback,
                    &sketch.basis,
                    entity,
                    rgb(palette.0.hover),
                    DIRECT_PICK_FEEDBACK_OFFSET,
                );
            }

            for reference in state.selected_sketch_points.iter().filter(|reference| {
                reference.sketch_name == sketch.name && reference.entity_id == entity_id
            }) {
                if let Some(position) = referenced_sketch_point(entity, reference) {
                    draw_sketch_grip(
                        &mut pick_halo,
                        &sketch.basis,
                        position,
                        camera.camera,
                        *viewport,
                        6.0,
                        rgb(palette.0.pick_halo),
                        false,
                    );
                    draw_sketch_grip(
                        &mut pick_feedback,
                        &sketch.basis,
                        position,
                        camera.camera,
                        *viewport,
                        6.0,
                        rgb(palette.0.edge_selected),
                        false,
                    );
                }
            }
            if let Some(reference) = state.hovered_sketch_point.as_ref().filter(|reference| {
                reference.sketch_name == sketch.name
                    && reference.entity_id == entity_id
                    && !state.selected_sketch_points.contains(reference)
            }) {
                if let Some(position) = referenced_sketch_point(entity, reference) {
                    draw_sketch_grip(
                        &mut pick_halo,
                        &sketch.basis,
                        position,
                        camera.camera,
                        *viewport,
                        7.0,
                        rgb(palette.0.pick_halo),
                        false,
                    );
                    draw_sketch_grip(
                        &mut pick_feedback,
                        &sketch.basis,
                        position,
                        camera.camera,
                        *viewport,
                        7.0,
                        rgb(palette.0.edge_hover),
                        false,
                    );
                }
            }
        }
    }

    let (camera_right, camera_up) = camera_facing_axes(camera.camera);
    if let Some(point) = &state.hovered_surface_point {
        let world = Vec3::new(point.x as f32, point.y as f32, point.z as f32);
        let radius = screen_space_disc_radius(camera.camera, *viewport, world, 6.0);
        draw_filled_disc(
            &mut pick_halo,
            world,
            camera_right,
            camera_up,
            radius,
            rgb(palette.0.pick_halo),
            6,
        );
        draw_filled_disc(
            &mut pick_feedback,
            world,
            camera_right,
            camera_up,
            radius,
            rgb(palette.0.edge_hover),
            6,
        );
    }
    if let Some(point) = &state.selected_surface_point {
        let world = Vec3::new(point.x as f32, point.y as f32, point.z as f32);
        let radius = screen_space_disc_radius(camera.camera, *viewport, world, 7.0);
        draw_filled_disc(
            &mut pick_halo,
            world,
            camera_right,
            camera_up,
            radius,
            rgb(palette.0.pick_halo),
            7,
        );
        draw_filled_disc(
            &mut pick_feedback,
            world,
            camera_right,
            camera_up,
            radius,
            rgb(palette.0.edge_selected),
            7,
        );
    }

    if let Some(sketch) = &model.active_sketch {
        draw_sketch(
            &mut sketch_gizmos,
            sketch,
            camera.camera,
            *viewport,
            3.5,
            |entity| {
                let (_, fully_defined) = sketch_entity_style(entity);
                Some(rgb(if fully_defined {
                    palette.0.defined_sketch
                } else {
                    palette.0.active_sketch
                }))
            },
        );
        draw_sketch(
            &mut pick_feedback,
            sketch,
            camera.camera,
            *viewport,
            5.0,
            |entity| {
                let (id, _) = sketch_entity_style(entity);
                if state.hovered_sketch_entity_id == Some(id) {
                    Some(rgb(palette.0.hover))
                } else {
                    None
                }
            },
        );
        draw_sketch(
            &mut highlights,
            sketch,
            camera.camera,
            *viewport,
            4.5,
            |entity| {
                let (id, _) = sketch_entity_style(entity);
                if state.selected_sketch_entity_ids.contains(&id) {
                    Some(rgb(palette.0.selection))
                } else if state.constraint_related_sketch_entity_ids.contains(&id) {
                    Some(rgb(palette.0.constraint_related))
                } else {
                    None
                }
            },
        );
    }

    for layer in &preview.value.lines {
        let color = Color::srgba(
            layer.color[0],
            layer.color[1],
            layer.color[2],
            layer.color[3].clamp(0.0, 1.0),
        );
        for segment in layer.segments.chunks_exact(6) {
            let start = Vec3::new(segment[0], segment[1], segment[2]);
            let end = Vec3::new(segment[3], segment[4], segment[5]);
            if layer.pattern == ViewportLinePattern::Dotted {
                let delta = end - start;
                let length = delta.length();
                if length <= f32::EPSILON {
                    continue;
                }
                let world_per_pixel =
                    world_per_pixel_at(camera.camera, *viewport, start.lerp(end, 0.5))
                        .max(f32::EPSILON);
                // Tiny screen-space strokes read as dots without relying on
                // a renderer-specific dash shader. Cap the subdivision so a
                // pathological guide cannot degrade pointer latency.
                let dot_length = world_per_pixel * 1.25;
                let requested_period = world_per_pixel * 4.25;
                let direction = delta / length;
                let requested_count = ((length / requested_period).ceil() as usize).max(1);
                let dot_count = requested_count.min(512);
                let period = if requested_count > dot_count {
                    length / dot_count as f32
                } else {
                    requested_period
                };
                for dot_index in 0..dot_count {
                    let distance = dot_index as f32 * period;
                    if distance >= length {
                        break;
                    }
                    let dot_start = start + direction * distance;
                    let dot_end = start + direction * (distance + dot_length).min(length);
                    if layer.width >= 2.0 {
                        highlights.line(dot_start, dot_end, color);
                    } else {
                        gizmos.line(dot_start, dot_end, color);
                    }
                }
            } else if layer.width >= 2.0 {
                highlights.line(start, end, color);
            } else {
                gizmos.line(start, end, color);
            }
        }
    }

    for layer in &preview.value.points {
        let color = Color::srgba(
            layer.color[0],
            layer.color[1],
            layer.color[2],
            layer.color[3].clamp(0.0, 1.0),
        );
        let radius = layer.radius.clamp(0.08, 4.0);
        for point in layer.positions.chunks_exact(3) {
            let center = Vec3::new(point[0], point[1], point[2]);
            let forward = (Vec3::from_array(camera.camera.target)
                - Vec3::from_array(camera.camera.position))
            .normalize_or_zero();
            let up_hint = Vec3::from_array(camera.camera.up).normalize_or_zero();
            let right = forward.cross(up_hint).normalize_or_zero();
            let right = if right == Vec3::ZERO { Vec3::X } else { right };
            let up = right.cross(forward).normalize_or_zero();
            if layer.hollow {
                let ring = (0..24)
                    .map(|index| {
                        let angle = index as f32 / 24.0 * std::f32::consts::TAU;
                        center + right * (angle.cos() * radius) + up * (angle.sin() * radius)
                    })
                    .collect::<Vec<_>>();
                draw_marker_loop(&mut highlights, &ring, color);
            } else {
                // One chord row per logical pixel keeps large markers solid;
                // small ones bottom out at the cheap sketch-grip density.
                let world_per_pixel = world_per_pixel_at(camera.camera, *viewport, center);
                let half_steps = ((radius / world_per_pixel.max(0.001)).ceil() as i32).clamp(4, 96);
                draw_filled_disc(
                    &mut highlights,
                    center,
                    right,
                    up,
                    radius,
                    color,
                    half_steps,
                );
            }
        }
    }

    if let Some(marker) = preview.value.marker {
        draw_snap_marker(
            &mut highlights,
            marker,
            camera.camera,
            *viewport,
            &palette.0,
        );
    }
}

fn keep_gizmo_asset_resident<Config: GizmoConfigGroup>(gizmos: &mut Gizmos<Config>) {
    gizmos.line(Vec3::ZERO, Vec3::ZERO, Color::NONE);
}

fn draw_snap_marker(
    gizmos: &mut Gizmos<CadHighlightGizmos>,
    marker: ViewportSnapMarker,
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    palette: &ViewportPalette,
) {
    let center = Vec3::from_array(marker.position);
    let camera_position = Vec3::from_array(camera.position);
    let forward = (Vec3::from_array(camera.target) - camera_position).normalize_or_zero();
    let camera_up = Vec3::from_array(camera.up).normalize_or_zero();
    let mut right = forward.cross(camera_up).normalize_or_zero();
    if right.length_squared() < 1.0e-8 {
        right = Vec3::X;
    }
    let mut up = right.cross(forward).normalize_or_zero();
    if up.length_squared() < 1.0e-8 {
        up = Vec3::Y;
    }
    let world_per_pixel = world_per_pixel_at(camera, viewport, center);
    let half = world_per_pixel * SNAP_MARKER_HALF_SIZE_PX;
    let point_color = rgba(palette.hover, 1.0);
    let secondary_color = rgba(palette.selection, 1.0);
    let preview_color = rgba(palette.preview, 0.98);

    match marker.kind {
        ViewportSnapKind::Point => {
            draw_marker_loop(
                gizmos,
                &[
                    center - right * half - up * half,
                    center + right * half - up * half,
                    center + right * half + up * half,
                    center - right * half + up * half,
                ],
                point_color,
            );
        }
        ViewportSnapKind::Midpoint | ViewportSnapKind::ReferenceMidpoint => {
            draw_marker_loop(
                gizmos,
                &[
                    center + up * half,
                    center + right * half - up * half,
                    center - right * half - up * half,
                ],
                secondary_color,
            );
        }
        ViewportSnapKind::Origin => {
            draw_marker_loop(
                gizmos,
                &[
                    center + up * half,
                    center + right * half,
                    center - up * half,
                    center - right * half,
                ],
                secondary_color,
            );
            let inner = half * 0.42;
            gizmos.line(
                center - right * inner,
                center + right * inner,
                secondary_color,
            );
            gizmos.line(center - up * inner, center + up * inner, secondary_color);
        }
        ViewportSnapKind::Curve => {
            draw_marker_loop(
                gizmos,
                &[
                    center + up * half,
                    center + right * half,
                    center - up * half,
                    center - right * half,
                ],
                point_color,
            );
        }
        ViewportSnapKind::Grid => {
            let arm = half * 0.72;
            gizmos.line(center - right * arm, center + right * arm, preview_color);
            gizmos.line(center - up * arm, center + up * arm, preview_color);
        }
    }
}

fn draw_marker_loop(gizmos: &mut Gizmos<CadHighlightGizmos>, points: &[Vec3], color: Color) {
    if points.len() < 2 {
        return;
    }
    for index in 0..points.len() {
        gizmos.line(points[index], points[(index + 1) % points.len()], color);
    }
}

fn draw_grid_on_basis(gizmos: &mut Gizmos, basis: &PlaneBasis, fine: Color, major: Color) {
    let origin = basis_vector(basis.origin) - basis_vector(basis.normal) * 0.03;
    let u = basis_vector(basis.u);
    let v = basis_vector(basis.v);
    for index in -30..=30 {
        let coordinate = index as f32 * 5.0;
        let color = if index % 5 == 0 { major } else { fine };
        gizmos.line(
            origin + u * coordinate - v * 150.0,
            origin + u * coordinate + v * 150.0,
            color,
        );
        gizmos.line(
            origin - u * 150.0 + v * coordinate,
            origin + u * 150.0 + v * coordinate,
            color,
        );
    }
    gizmos.line(
        origin - u * 150.0,
        origin + u * 150.0,
        Color::srgba(0.80, 0.25, 0.30, 0.62),
    );
    gizmos.line(
        origin - v * 150.0,
        origin + v * 150.0,
        Color::srgba(0.25, 0.65, 0.38, 0.62),
    );
}

fn draw_plane_outline<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    half_size: f32,
    color: Color,
) {
    let origin = basis_vector(basis.origin);
    let u = basis_vector(basis.u) * half_size;
    let v = basis_vector(basis.v) * half_size;
    let corners = [
        origin - u - v,
        origin + u - v,
        origin + u + v,
        origin - u + v,
    ];
    for index in 0..4 {
        gizmos.line(corners[index], corners[(index + 1) % 4], color);
    }
    gizmos.line(origin - u, origin + u, color.with_alpha(0.46));
    gizmos.line(origin - v, origin + v, color.with_alpha(0.46));
}

fn draw_edge_segments<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    edge: &nbcad_solid::EdgeDto,
    color: Color,
    transform: &Transform,
) {
    for pair in edge.points.windows(2) {
        gizmos.line(
            transform.transform_point(Vec3::new(
                pair[0].x as f32,
                pair[0].y as f32,
                pair[0].z as f32,
            )),
            transform.transform_point(Vec3::new(
                pair[1].x as f32,
                pair[1].y as f32,
                pair[1].z as f32,
            )),
            color,
        );
    }
}

fn edge_is_straight(edge: &nbcad_solid::EdgeDto) -> bool {
    if edge.circle.is_some() || edge.points.len() < 2 {
        return false;
    }
    let first = Vec3::new(
        edge.points[0].x as f32,
        edge.points[0].y as f32,
        edge.points[0].z as f32,
    );
    let last_point = &edge.points[edge.points.len() - 1];
    let last = Vec3::new(
        last_point.x as f32,
        last_point.y as f32,
        last_point.z as f32,
    );
    let direction = last - first;
    let length = direction.length();
    if length <= 1e-6 {
        return false;
    }
    let unit = direction / length;
    edge.points.iter().all(|point| {
        let offset = Vec3::new(point.x as f32, point.y as f32, point.z as f32) - first;
        offset.cross(unit).length() <= (length * 1e-5).max(1e-5)
    })
}

fn face_boundary_segments(body: &BodyDto, face: &FaceDto) -> Vec<(Vec3, Vec3)> {
    let start = face.first_index as usize;
    let end = start
        .saturating_add(face.index_count as usize)
        .min(body.mesh.indices.len());
    triangle_boundary_segments(&body.mesh.positions, &body.mesh.indices[start..end])
}

#[derive(Clone, Copy)]
struct BoundarySegment {
    count: u32,
    start: Vec3,
    end: Vec3,
}

fn triangle_boundary_segments(positions: &[f32], indices: &[u32]) -> Vec<(Vec3, Vec3)> {
    let point = |index: u32| {
        let offset = index as usize * 3;
        let value = positions.get(offset..offset + 3)?;
        Some(Vec3::new(value[0], value[1], value[2]))
    };
    let point_key = |value: Vec3| {
        [
            (value.x * 1_000_000.0).round() as i64,
            (value.y * 1_000_000.0).round() as i64,
            (value.z * 1_000_000.0).round() as i64,
        ]
    };
    let mut segments = HashMap::<([i64; 3], [i64; 3]), BoundarySegment>::new();
    for triangle in indices.chunks_exact(3) {
        for (a, b) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let (Some(start), Some(end)) = (point(a), point(b)) else {
                continue;
            };
            let start_key = point_key(start);
            let end_key = point_key(end);
            if start_key == end_key {
                continue;
            }
            let key = if start_key <= end_key {
                (start_key, end_key)
            } else {
                (end_key, start_key)
            };
            segments
                .entry(key)
                .and_modify(|segment| segment.count += 1)
                .or_insert(BoundarySegment {
                    count: 1,
                    start,
                    end,
                });
        }
    }
    segments
        .into_values()
        .filter_map(|segment| (segment.count == 1).then_some((segment.start, segment.end)))
        .collect()
}

fn sketch_entity_style(entity: &EntityDto) -> (u64, bool) {
    match entity {
        EntityDto::Point {
            id, fully_defined, ..
        }
        | EntityDto::Line {
            id, fully_defined, ..
        }
        | EntityDto::Arc {
            id, fully_defined, ..
        }
        | EntityDto::Circle {
            id, fully_defined, ..
        }
        | EntityDto::Spline {
            id, fully_defined, ..
        } => (id.0, *fully_defined),
    }
}

fn draw_sketch<Config, ColorFor>(
    gizmos: &mut Gizmos<Config>,
    sketch: &SketchDto,
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    point_radius_px: f32,
    mut color_for: ColorFor,
) where
    Config: GizmoConfigGroup,
    ColorFor: FnMut(&EntityDto) -> Option<Color>,
{
    for entity in &sketch.entities {
        let Some(color) = color_for(entity) else {
            continue;
        };
        draw_sketch_curve(gizmos, &sketch.basis, entity, color);
        draw_sketch_entity_grips(
            gizmos,
            &sketch.basis,
            entity,
            camera,
            viewport,
            point_radius_px,
            color,
            !sketch_entity_style(entity).1,
        );
    }
}

fn draw_base_curve_outside_profiles<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    entity: &EntityDto,
    color: Color,
    profiles: &[&ProfileLoopDto],
) {
    match base_curve_remainder(entity, profiles) {
        BaseCurveRemainder::Complete => draw_sketch_curve(gizmos, basis, entity, color),
        BaseCurveRemainder::Lines(segments) => {
            for [start, end] in segments {
                gizmos.line(
                    sketch_world(basis, start.x, start.y, FINISHED_SKETCH_OFFSET),
                    sketch_world(basis, end.x, end.y, FINISHED_SKETCH_OFFSET),
                    color,
                );
            }
        }
        BaseCurveRemainder::Circular(ranges) => {
            let (center, radius) = match entity {
                EntityDto::Arc { center, radius, .. }
                | EntityDto::Circle { center, radius, .. } => (center, radius),
                _ => return,
            };
            for [start, end] in ranges {
                let sweep = end - start;
                let segments = ((sweep.abs() * 20.0).ceil() as usize).clamp(12, 128);
                draw_parametric_curve(gizmos, segments, color, |ratio| {
                    let angle = start + sweep * ratio;
                    sketch_world(
                        basis,
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                        FINISHED_SKETCH_OFFSET,
                    )
                });
            }
        }
    }
}

fn draw_sketch_curve<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    entity: &EntityDto,
    color: Color,
) {
    draw_sketch_curve_at_offset(gizmos, basis, entity, color, FINISHED_SKETCH_OFFSET);
}

fn draw_sketch_curve_at_offset<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    entity: &EntityDto,
    color: Color,
    offset: f32,
) {
    match entity {
        EntityDto::Point { .. } => {}
        EntityDto::Line { start, end, .. } => {
            gizmos.line(
                sketch_world(basis, start.x, start.y, offset),
                sketch_world(basis, end.x, end.y, offset),
                color,
            );
        }
        EntityDto::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            ..
        } => {
            let mut sweep = end_angle - start_angle;
            while sweep <= 0.0 {
                sweep += std::f64::consts::TAU;
            }
            let segments = ((sweep.abs() * 20.0).ceil() as usize).clamp(12, 128);
            draw_parametric_curve(gizmos, segments, color, |ratio| {
                let angle = start_angle + sweep * ratio;
                sketch_world(
                    basis,
                    center.x + radius * angle.cos(),
                    center.y + radius * angle.sin(),
                    offset,
                )
            });
        }
        EntityDto::Circle { center, radius, .. } => {
            draw_parametric_curve(gizmos, 72, color, |ratio| {
                let angle = std::f64::consts::TAU * ratio;
                sketch_world(
                    basis,
                    center.x + radius * angle.cos(),
                    center.y + radius * angle.sin(),
                    offset,
                )
            });
        }
        EntityDto::Spline { tessellation, .. } => {
            for pair in tessellation.windows(2) {
                gizmos.line(
                    sketch_world(basis, pair[0].x, pair[0].y, offset),
                    sketch_world(basis, pair[1].x, pair[1].y, offset),
                    color,
                );
            }
        }
    }
}

fn draw_sketch_entity_grips<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    entity: &EntityDto,
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    point_radius_px: f32,
    color: Color,
    hollow: bool,
) {
    for position in sketch_grip_positions(entity) {
        draw_sketch_grip(
            gizmos,
            basis,
            position,
            camera,
            viewport,
            point_radius_px,
            color,
            hollow,
        );
    }
}

fn sketch_grip_positions(entity: &EntityDto) -> &[SketchVec2] {
    match entity {
        EntityDto::Point { position, .. } => std::slice::from_ref(position),
        EntityDto::Arc { center, .. } | EntityDto::Circle { center, .. } => {
            std::slice::from_ref(center)
        }
        EntityDto::Spline { points, .. } => points,
        EntityDto::Line { .. } => &[],
    }
}

fn referenced_sketch_point<'a>(
    entity: &'a EntityDto,
    reference: &SketchPointRefDto,
) -> Option<&'a SketchVec2> {
    match (entity, &reference.point) {
        (EntityDto::Point { position, .. }, SketchPointKindDto::Point) => Some(position),
        (EntityDto::Line { start, .. }, SketchPointKindDto::Start) => Some(start),
        (EntityDto::Line { end, .. }, SketchPointKindDto::End) => Some(end),
        (EntityDto::Arc { center, .. }, SketchPointKindDto::Center)
        | (EntityDto::Circle { center, .. }, SketchPointKindDto::Center) => Some(center),
        (EntityDto::Spline { points, .. }, SketchPointKindDto::FitPoint { index }) => {
            points.get(*index as usize)
        }
        _ => None,
    }
}

fn draw_sketch_grip<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    position: &SketchVec2,
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    point_radius_px: f32,
    color: Color,
    hollow: bool,
) {
    let point = sketch_world(basis, position.x, position.y, 0.05);
    let radius = screen_space_disc_radius(camera, viewport, point, point_radius_px);
    let (right, up) = camera_facing_axes(camera);
    if hollow {
        let segments = 24;
        for index in 0..segments {
            let angle = index as f32 / segments as f32 * std::f32::consts::TAU;
            let next_angle = (index + 1) as f32 / segments as f32 * std::f32::consts::TAU;
            let start = point + right * (angle.cos() * radius) + up * (angle.sin() * radius);
            let end =
                point + right * (next_angle.cos() * radius) + up * (next_angle.sin() * radius);
            gizmos.line(start, end, color);
        }
    } else {
        draw_filled_disc(gizmos, point, right, up, radius, color, 4);
    }
}

/// Gizmos do not expose a filled world-space disc primitive. A handful of
/// parallel chords gives sketch points a true round-dot silhouette while
/// keeping their physical diameter tied to line weight on Retina and
/// standard-density displays. `half_steps` controls chord density: small
/// sketch grips stay cheap at 4, while large CAM pick markers pass one row
/// per logical pixel so the fill reads solid instead of striped.
fn draw_filled_disc<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    center: Vec3,
    u_axis: Vec3,
    v_axis: Vec3,
    radius: f32,
    color: Color,
    half_steps: i32,
) {
    let u = u_axis.normalize_or_zero();
    let v = v_axis.normalize_or_zero();
    if u == Vec3::ZERO || v == Vec3::ZERO {
        return;
    }
    for step in -half_steps..=half_steps {
        let ratio = step as f32 / half_steps as f32;
        let along_v = ratio * radius;
        let half_chord = (radius * radius - along_v * along_v).max(0.0).sqrt();
        let row_center = center + v * along_v;
        gizmos.line(
            row_center - u * half_chord,
            row_center + u * half_chord,
            color,
        );
    }
}

fn camera_facing_axes(camera: ViewportCamera) -> (Vec3, Vec3) {
    let camera_position = Vec3::from_array(camera.position);
    let forward = (Vec3::from_array(camera.target) - camera_position).normalize_or_zero();
    let up_hint = Vec3::from_array(camera.up).normalize_or_zero();
    let mut right = forward.cross(up_hint).normalize_or_zero();
    if right.length_squared() < 1.0e-8 {
        right = Vec3::X;
    }
    let mut up = right.cross(forward).normalize_or_zero();
    if up.length_squared() < 1.0e-8 {
        up = Vec3::Y;
    }
    (right, up)
}

fn screen_space_disc_radius(
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    center: Vec3,
    radius_px: f32,
) -> f32 {
    let world_per_pixel = world_per_pixel_at(camera, viewport, center);
    world_per_pixel * radius_px.max(1.0)
}

fn draw_parametric_curve(
    gizmos: &mut Gizmos<impl GizmoConfigGroup>,
    segments: usize,
    color: Color,
    point: impl Fn(f64) -> Vec3,
) {
    let mut previous = point(0.0);
    for index in 1..=segments {
        let next = point(index as f64 / segments as f64);
        gizmos.line(previous, next, color);
        previous = next;
    }
}

fn sketch_world(basis: &PlaneBasis, x: f64, y: f64, offset: f32) -> Vec3 {
    Vec3::new(
        (basis.origin[0] + basis.u[0] * x + basis.v[0] * y) as f32,
        (basis.origin[1] + basis.u[1] * x + basis.v[1] * y) as f32,
        (basis.origin[2] + basis.u[2] * x + basis.v[2] * y) as f32,
    ) + basis_vector(basis.normal) * offset
}

fn draw_profile_segments<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    segments: &[[Point2Dto; 2]],
    color: Color,
) {
    for [start, end] in segments {
        gizmos.line(
            sketch_world(basis, start.x, start.y, PROFILE_PICK_OFFSET),
            sketch_world(basis, end.x, end.y, PROFILE_PICK_OFFSET),
            color,
        );
    }
}

fn basis_vector(vector: [f64; 3]) -> Vec3 {
    Vec3::new(vector[0] as f32, vector[1] as f32, vector[2] as f32)
}

fn rgb(value: [f32; 3]) -> Color {
    Color::srgb(value[0], value[1], value[2])
}

fn rgba(value: [f32; 3], alpha: f32) -> Color {
    Color::srgba(value[0], value[1], value[2], alpha)
}

fn push_render_command(
    pending: &Arc<Mutex<PendingRenderCommands>>,
    command: RenderCommand,
) -> bool {
    let Ok(mut pending) = pending.lock() else {
        return false;
    };
    match command {
        RenderCommand::Resize {
            logical_width,
            logical_height,
            scale_factor,
            palette,
            hud,
        } => pending.resize = Some((logical_width, logical_height, scale_factor, palette, hud)),
        RenderCommand::Model(model) => pending.model = Some(model),
        RenderCommand::RebindModelSession { from, to } => {
            if !pending
                .rebind_model_sessions
                .iter()
                .any(|(existing_from, existing_to)| existing_from == &from && existing_to == &to)
            {
                pending.rebind_model_sessions.push((from, to));
            }
        }
        RenderCommand::DropModelSession(session_id) => {
            if !pending.drop_model_sessions.contains(&session_id) {
                pending.drop_model_sessions.push(session_id);
            }
        }
        RenderCommand::Camera(camera) => pending.camera = Some(camera),
        RenderCommand::Preview(preview) => pending.preview = Some(preview),
        RenderCommand::Presentation(presentation) => {
            pending.presentation = Some(presentation);
        }
    }
    if pending.scheduled {
        false
    } else {
        pending.scheduled = true;
        true
    }
}

/// Drains coalesced mutations on the platform UI thread. The Bevy App is kept
/// behind a process-lifetime pointer that is never dereferenced elsewhere.
fn drain_render_commands(
    runtime_pointer: &Arc<AtomicUsize>,
    pending: &Arc<Mutex<PendingRenderCommands>>,
    metrics: &Arc<Mutex<MetricsState>>,
) {
    #[cfg(target_os = "macos")]
    debug_assert!(MainThreadMarker::new().is_some());
    let pointer = runtime_pointer.load(Ordering::Acquire);
    if pointer == 0 {
        if let Ok(mut pending) = pending.lock() {
            pending.scheduled = false;
        }
        return;
    }
    let runtime = unsafe { &mut *(pointer as *mut MainThreadRenderRuntime) };

    loop {
        let commands = {
            let Ok(mut pending) = pending.lock() else {
                return;
            };
            if pending.resize.is_none()
                && pending.model.is_none()
                && pending.rebind_model_sessions.is_empty()
                && pending.drop_model_sessions.is_empty()
                && pending.camera.is_none()
                && pending.preview.is_none()
                && pending.presentation.is_none()
            {
                pending.scheduled = false;
                return;
            }
            (
                pending.resize.take(),
                pending.model.take(),
                std::mem::take(&mut pending.rebind_model_sessions),
                std::mem::take(&mut pending.drop_model_sessions),
                pending.camera.take(),
                pending.preview.take(),
                pending.presentation.take(),
            )
        };
        let resize_requested = commands.0.is_some();
        let requires_pipeline_settle = resize_requested
            || commands.1.is_some()
            || !commands.2.is_empty()
            || !commands.3.is_empty()
            || commands.5.is_some();

        if let Ok(mut current) = metrics.lock() {
            current.wakeups += 1;
        }
        let mut dirty = false;
        if let Some((logical_width, logical_height, scale_factor, palette, hud)) = commands.0 {
            apply_render_command(
                RenderCommand::Resize {
                    logical_width,
                    logical_height,
                    scale_factor,
                    palette,
                    hud,
                },
                runtime,
                metrics,
                &mut dirty,
            );
        }
        if let Some(model) = commands.1 {
            apply_render_command(RenderCommand::Model(model), runtime, metrics, &mut dirty);
        }
        for (from, to) in commands.2 {
            apply_render_command(
                RenderCommand::RebindModelSession { from, to },
                runtime,
                metrics,
                &mut dirty,
            );
        }
        for session_id in commands.3 {
            apply_render_command(
                RenderCommand::DropModelSession(session_id),
                runtime,
                metrics,
                &mut dirty,
            );
        }
        if let Some(camera) = commands.4 {
            apply_render_command(RenderCommand::Camera(camera), runtime, metrics, &mut dirty);
        }
        if let Some(preview) = commands.5 {
            apply_render_command(
                RenderCommand::Preview(preview),
                runtime,
                metrics,
                &mut dirty,
            );
        }
        if let Some(presentation) = commands.6 {
            apply_render_command(
                RenderCommand::Presentation(presentation),
                runtime,
                metrics,
                &mut dirty,
            );
        }
        if dirty {
            // Structural changes get a second update to settle Bevy's
            // extracted render world. Camera and presentation-only
            // changes use one frame on every platform; rendering them twice
            // adds pointer latency without creating any new GPU pipelines.
            // A Linux Vulkan surface can reject the first frames after the
            // embedded X11 drawable changes extent. Drain that short-lived
            // stale-surface window here instead of running a continuous idle
            // renderer. Normal interaction remains one frame per bridge wake.
            #[cfg(target_os = "linux")]
            let frame_count = if resize_requested {
                1
            } else if requires_pipeline_settle {
                2
            } else {
                1
            };
            #[cfg(not(target_os = "linux"))]
            let frame_count = if requires_pipeline_settle { 2 } else { 1 };
            render_frames(&mut runtime.app, frame_count, metrics);
            maybe_write_ready_probe(metrics);
        }
    }
}

fn apply_render_command(
    command: RenderCommand,
    runtime: &mut MainThreadRenderRuntime,
    metrics: &Arc<Mutex<MetricsState>>,
    dirty: &mut bool,
) {
    match command {
        RenderCommand::Resize {
            logical_width,
            logical_height,
            scale_factor,
            palette,
            hud,
        } => {
            let physical_width = (logical_width * scale_factor).round().max(1.0) as u32;
            let physical_height = (logical_height * scale_factor).round().max(1.0) as u32;
            let logical_size_changed = (runtime.logical_size.0 - logical_width as f32).abs() > 0.01
                || (runtime.logical_size.1 - logical_height as f32).abs() > 0.01;
            let scale_factor_changed = (runtime.scale_factor - scale_factor as f32).abs() > 0.001;
            let size_changed = logical_size_changed || scale_factor_changed;
            #[cfg(target_os = "linux")]
            if std::env::var_os("NBCAD_VIEWPORT_PROBE_FILE").is_some() {
                eprintln!(
                    "native Bevy resize command: {:.0}x{:.0} logical, changed={size_changed}, background={:?}, native_chrome={}",
                    logical_width, logical_height, palette.background, hud.render_native_chrome
                );
            }
            if size_changed {
                resize_embedded_window(
                    runtime.app.world_mut(),
                    logical_width as f32,
                    logical_height as f32,
                    scale_factor as f32,
                    scale_factor_changed,
                );
            }
            {
                let mut viewport = runtime
                    .app
                    .world_mut()
                    .resource_mut::<ViewportSizeResource>();
                viewport.logical_width = logical_width as f32;
                viewport.logical_height = logical_height as f32;
            }
            {
                let mut gizmo_config = runtime.app.world_mut().resource_mut::<GizmoConfigStore>();
                configure_viewport_line_widths(
                    &mut gizmo_config,
                    logical_width as f32,
                    logical_height as f32,
                    scale_factor as f32,
                );
            }
            let palette_changed = runtime.app.world().resource::<PaletteResource>().0 != palette;
            if palette_changed {
                *runtime.app.world_mut().resource_mut::<ClearColor>() =
                    ClearColor(rgb(palette.background));
                runtime.app.world_mut().resource_mut::<PaletteResource>().0 = palette;
                let mut model = runtime.app.world_mut().resource_mut::<ModelResource>();
                model.revision = model.revision.wrapping_add(1);
            }
            let hud_changed = runtime.app.world().resource::<HudResource>().hud != hud;
            if hud_changed || palette_changed {
                let mut resource = runtime.app.world_mut().resource_mut::<HudResource>();
                resource.hud = hud;
                resource.revision = resource.revision.wrapping_add(1);
            }
            runtime.logical_size = (logical_width as f32, logical_height as f32);
            runtime.scale_factor = scale_factor as f32;
            let mut first_layout = false;
            if let Ok(mut current) = metrics.lock() {
                first_layout = current.physical_width == 0;
                current.logical_width = logical_width;
                current.logical_height = logical_height;
                current.scale_factor = scale_factor;
                current.physical_width = physical_width;
                current.physical_height = physical_height;
            }
            if first_layout {
                eprintln!(
                    "native Bevy viewport ready: {:.0}x{:.0} logical, {}x{} physical, {:.2}x scale",
                    logical_width, logical_height, physical_width, physical_height, scale_factor
                );
            }
            // A full-screen transition can reparent/recreate the native layer
            // without changing its final dimensions. Every accepted layout is
            // therefore also an explicit redraw request.
            *dirty = true;
        }
        RenderCommand::Model(mut next) => {
            next.session_id = canonical_model_session(&runtime.session_aliases, &next.session_id);
            runtime.model = next;
            let mut resource = runtime.app.world_mut().resource_mut::<ModelResource>();
            resource.session_id = runtime.model.session_id.clone();
            resource.geometry_revision = runtime.model.geometry_revision;
            resource.scene = runtime.model.scene.clone();
            resource.active_sketch = runtime.model.active_sketch.clone();
            resource.finished_sketches = runtime.model.finished_sketches.clone();
            resource.datum_planes = runtime.model.datum_planes.clone();
            resource.profile_catalog = runtime.model.profile_catalog.clone();
            resource.body_appearances = runtime.model.body_appearances.clone();
            resource.body_poses = runtime.model.body_poses.clone();
            if !same_instance_layout(
                &resource.instance_body_poses,
                &runtime.model.instance_body_poses,
            ) {
                resource.instance_revision = resource.instance_revision.wrapping_add(1);
            }
            resource.instance_body_poses = runtime.model.instance_body_poses.clone();
            resource.revision = resource.revision.wrapping_add(1);
            if let Ok(mut current) = metrics.lock() {
                current.body_count = runtime.model.scene.bodies.len();
                current.triangle_count = runtime
                    .model
                    .scene
                    .bodies
                    .iter()
                    .map(|body| body.mesh.indices.len() / 3)
                    .sum();
            }
            *dirty = true;
        }
        RenderCommand::RebindModelSession { from, to } => {
            if from != to {
                let to = canonical_model_session(&runtime.session_aliases, &to);
                for alias in runtime.session_aliases.values_mut() {
                    if *alias == from {
                        *alias = to.clone();
                    }
                }
                runtime.session_aliases.insert(from.clone(), to.clone());
                if runtime.model.session_id == from {
                    runtime.model.session_id = to.clone();
                }
                if rebind_cached_model_session(runtime.app.world_mut(), &from, &to) {
                    *dirty = true;
                }
            }
        }
        RenderCommand::DropModelSession(session_id) => {
            drop_cached_model_session(runtime.app.world_mut(), &session_id);
            *dirty = true;
        }
        RenderCommand::Camera(next) => {
            runtime.camera = next;
            let mut resource = runtime.app.world_mut().resource_mut::<CameraResource>();
            resource.camera = next;
            resource.revision = resource.revision.wrapping_add(1);
            *dirty = true;
        }
        RenderCommand::Preview(next) => {
            let mut resource = runtime.app.world_mut().resource_mut::<PreviewResource>();
            if preview_mesh_content_changed(&resource.value, &next) {
                resource.mesh_revision = resource.mesh_revision.wrapping_add(1);
            }
            resource.value = next;
            resource.revision = resource.revision.wrapping_add(1);
            *dirty = true;
        }
        RenderCommand::Presentation(next) => {
            if runtime.model.body_poses != next.body_poses
                || runtime.model.instance_body_poses != next.instance_body_poses
            {
                let layout_changed = !same_instance_layout(
                    &runtime.model.instance_body_poses,
                    &next.instance_body_poses,
                );
                runtime.model.body_poses = next.body_poses.clone();
                runtime.model.instance_body_poses = next.instance_body_poses.clone();
                let mut model = runtime.app.world_mut().resource_mut::<ModelResource>();
                model.body_poses = next.body_poses.clone();
                model.instance_body_poses = next.instance_body_poses.clone();
                if layout_changed {
                    model.instance_revision = model.instance_revision.wrapping_add(1);
                }
                model.revision = model.revision.wrapping_add(1);
            }
            let mut resource = runtime
                .app
                .world_mut()
                .resource_mut::<PresentationResource>();
            if resource.0 != next {
                resource.0 = next;
                *dirty = true;
            }
        }
    }
}

fn preview_mesh_content_changed(current: &ViewportPreview, next: &ViewportPreview) -> bool {
    current.triangles != next.triangles || current.arrows != next.arrows
}

fn canonical_model_session(aliases: &HashMap<String, String>, session_id: &str) -> String {
    let mut current = session_id;
    for _ in 0..aliases.len() {
        let Some(next) = aliases.get(current) else {
            break;
        };
        if next == current {
            break;
        }
        current = next;
    }
    current.to_string()
}

/// Transfer already-uploaded GPU geometry to the permanent project-tab id.
/// This preserves the recovered solid and avoids retessellating it during the
/// startup handoff from the reserved bootstrap engine session.
fn rebind_cached_model_session(world: &mut World, from: &str, to: &str) -> bool {
    if from == to {
        return false;
    }

    let mut changed = false;
    {
        let mut query = world.query::<&mut NativeModelGeometry>();
        for mut geometry in query.iter_mut(world) {
            if geometry.session_id == from {
                geometry.session_id = to.to_string();
                changed = true;
            }
        }
    }
    {
        let mut model = world.resource_mut::<ModelResource>();
        if model.session_id == from {
            model.session_id = to.to_string();
            changed = true;
        }
    }
    {
        let mut cache = world.resource_mut::<ModelGeometryCache>();
        if let Some(revision) = cache.0.remove(from) {
            cache.0.insert(to.to_string(), revision);
            changed = true;
        }
    }
    changed
}

fn drop_cached_model_session(world: &mut World, session_id: &str) {
    let entities = {
        let mut query = world.query::<(Entity, &NativeModelGeometry)>();
        query
            .iter(world)
            .filter_map(|(entity, geometry)| (geometry.session_id == session_id).then_some(entity))
            .collect::<Vec<_>>()
    };
    for entity in entities {
        world.despawn(entity);
    }
    world
        .resource_mut::<ModelGeometryCache>()
        .0
        .remove(session_id);
}

fn resize_embedded_window(
    world: &mut World,
    logical_width: f32,
    logical_height: f32,
    scale_factor: f32,
    scale_factor_changed: bool,
) -> bool {
    let physical_width = (logical_width * scale_factor).round().max(1.0) as u32;
    let physical_height = (logical_height * scale_factor).round().max(1.0) as u32;
    let window_entity = {
        let mut query = world.query_filtered::<(Entity, &mut Window), With<PrimaryWindow>>();
        let Ok((window_entity, mut window)) = query.single_mut(world) else {
            return false;
        };
        window
            .resolution
            .set_scale_factor_override(Some(scale_factor));
        window
            .resolution
            .set_physical_resolution(physical_width, physical_height);
        window_entity
    };

    // This embedded renderer does not run bevy_winit, so no OS adapter exists
    // to translate host resize notifications into Bevy messages. camera_system
    // relies on these messages to recompute PerspectiveProjection::aspect_ratio;
    // without one, the swapchain stretches the old projection until camera
    // motion happens to mark Projection as changed.
    world.write_message(WindowResized {
        window: window_entity,
        width: logical_width,
        height: logical_height,
    });
    if scale_factor_changed {
        world.write_message(WindowScaleFactorChanged {
            window: window_entity,
            scale_factor: scale_factor as f64,
        });
    }
    true
}

fn render_frames(app: &mut bevy::app::App, count: usize, metrics: &Arc<Mutex<MetricsState>>) {
    for _ in 0..count {
        let started = Instant::now();
        app.update();
        #[cfg(target_os = "linux")]
        if std::env::var_os("NBCAD_VIEWPORT_PROBE_FILE").is_some() {
            if let Some(render_app) = app.get_sub_app(bevy::render::RenderApp) {
                let world = render_app.world();
                let windows = world.get_resource::<bevy::render::view::window::ExtractedWindows>();
                let cameras = world.get_resource::<bevy::render::camera::SortedCameras>();
                if let Some(windows) = windows {
                    for window in windows.windows.values() {
                        eprintln!(
                            "native Bevy extracted window: {}x{}, format={:?}, texture={}, initial_present={}",
                            window.physical_width,
                            window.physical_height,
                            window.swap_chain_texture_format,
                            window.swap_chain_texture.is_some(),
                            window.needs_initial_present,
                        );
                    }
                }
                eprintln!(
                    "native Bevy render world: windows={}, cameras={}",
                    windows.map_or(0, |windows| windows.windows.len()),
                    cameras.map_or(0, |cameras| cameras.0.len()),
                );
            }
        }
        let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
        if let Ok(mut current) = metrics.lock() {
            current.rendered_frames += 1;
            current.total_frame_ms += elapsed_ms;
        }
    }
}

fn pick_occt_scene(
    scene: &SolidSceneDto,
    camera: ViewportCamera,
    viewport: (f32, f32),
    x: f32,
    y: f32,
    hidden_body_ids: &[u64],
    body_poses: &[BodyPoseDto],
    instance_body_poses: &[InstanceBodyPoseDto],
    purpose: NativePickPurpose,
) -> Option<NativePick> {
    if viewport.0 <= 1.0 || viewport.1 <= 1.0 {
        return None;
    }
    let origin = Vec3::from_array(camera.position);
    let forward = (Vec3::from_array(camera.target) - origin).normalize_or_zero();
    let up_hint = Vec3::from_array(camera.up).normalize_or_zero();
    let right = forward.cross(up_hint).normalize_or_zero();
    let up = right.cross(forward).normalize_or_zero();
    if forward == Vec3::ZERO || right == Vec3::ZERO || up == Vec3::ZERO {
        return None;
    }

    let ndc_x = x / viewport.0 * 2.0 - 1.0;
    let ndc_y = 1.0 - y / viewport.1 * 2.0;
    let tangent = (camera.vertical_fov_degrees.to_radians() * 0.5).tan();
    let aspect = viewport.0 / viewport.1;
    let direction = (forward + right * ndc_x * tangent * aspect + up * ndc_y * tangent).normalize();
    let world_per_pixel_factor = 2.0 * tangent / viewport.1;

    let mut best: Option<NativePick> = None;
    for body in &scene.bodies {
        if hidden_body_ids.contains(&body.id.0) {
            continue;
        }
        let instances = if instance_body_poses.is_empty() {
            vec![(None, body_pose_transform(body_poses, body.id.0))]
        } else {
            instance_body_poses
                .iter()
                .filter(|instance| instance.body_id == body.id && instance.visible)
                .map(|instance| {
                    (
                        Some(instance.occurrence_id.0),
                        instance_body_pose_transform(
                            instance_body_poses,
                            body_poses,
                            body.id.0,
                            Some(instance.occurrence_id.0),
                        ),
                    )
                })
                .collect::<Vec<_>>()
        };
        for (occurrence_id, transform) in instances {
            pick_body(
                body,
                occurrence_id,
                transform,
                origin,
                direction,
                world_per_pixel_factor,
                &mut best,
                purpose,
            );
        }
    }
    best
}

fn pick_body(
    body: &BodyDto,
    occurrence_id: Option<u64>,
    transform: Transform,
    origin: Vec3,
    direction: Vec3,
    world_per_pixel_factor: f32,
    best: &mut Option<NativePick>,
    purpose: NativePickPurpose,
) {
    let inverse_rotation = transform.rotation.inverse();
    for face in &body.faces {
        let start = face.first_index as usize;
        let end = start
            .saturating_add(face.index_count as usize)
            .min(body.mesh.indices.len());
        for triangle in body.mesh.indices[start..end].chunks_exact(3) {
            let Some(a) =
                mesh_position(body, triangle[0]).map(|point| transform.transform_point(point))
            else {
                continue;
            };
            let Some(b) =
                mesh_position(body, triangle[1]).map(|point| transform.transform_point(point))
            else {
                continue;
            };
            let Some(c) =
                mesh_position(body, triangle[2]).map(|point| transform.transform_point(point))
            else {
                continue;
            };
            let Some(distance) = ray_triangle(origin, direction, a, b, c) else {
                continue;
            };
            let world_point = origin + direction * distance;
            let local_point = inverse_rotation * (world_point - transform.translation);
            let connector = connector_for_face(face, local_point);
            if !pick_should_replace(
                best.as_ref(),
                distance,
                connector.as_ref().map(|value| value.kind),
            ) {
                continue;
            }
            *best = Some(NativePick {
                body_id: body.id.0,
                occurrence_id,
                face_id: face.id.0,
                edge_id: None,
                point: world_point.to_array(),
                distance,
                connector_kind: connector.as_ref().map(|value| value.kind.to_string()),
                connector_origin: connector.as_ref().map(|value| value.origin.to_array()),
                connector_primary_axis: connector
                    .as_ref()
                    .map(|value| value.primary_axis.to_array()),
                connector_secondary_axis: connector
                    .as_ref()
                    .map(|value| value.secondary_axis.to_array()),
                connector_radius: connector.as_ref().and_then(|value| value.radius),
            });
        }

        // These full disks are joint-placement aids, not trimmed B-rep
        // faces. In ordinary selection they can fill a real hole or extend
        // beyond a partial revolution and falsely turn background into a hit.
        if purpose != NativePickPurpose::JointConnector {
            continue;
        }
        let Some(cylinder) = face.cylinder else {
            continue;
        };
        let axis = Vec3::new(
            cylinder.axis.x as f32,
            cylinder.axis.y as f32,
            cylinder.axis.z as f32,
        )
        .normalize_or_zero();
        if axis == Vec3::ZERO || !cylinder.radius.is_finite() || cylinder.radius <= 0.0 {
            continue;
        }
        let axis_origin = Vec3::new(
            cylinder.origin.x as f32,
            cylinder.origin.y as f32,
            cylinder.origin.z as f32,
        );
        let mut min_axial = f32::INFINITY;
        let mut max_axial = f32::NEG_INFINITY;
        for index in &body.mesh.indices[start..end] {
            let Some(point) = mesh_position(body, *index) else {
                continue;
            };
            let axial = (point - axis_origin).dot(axis);
            min_axial = min_axial.min(axial);
            max_axial = max_axial.max(axial);
        }
        if !min_axial.is_finite() || !max_axial.is_finite() || max_axial - min_axial <= 1.0e-5 {
            continue;
        }
        let reference = Vec3::new(
            cylinder.reference.x as f32,
            cylinder.reference.y as f32,
            cylinder.reference.z as f32,
        );
        for (axial, sign) in [(min_axial, -1.0_f32), (max_axial, 1.0_f32)] {
            let local_center = axis_origin + axis * axial;
            let local_primary = axis * sign;
            let world_center = transform.transform_point(local_center);
            let world_normal = (transform.rotation * local_primary).normalize_or_zero();
            let Some(distance) = ray_plane_disk(
                origin,
                direction,
                world_center,
                world_normal,
                cylinder.radius as f32,
            ) else {
                continue;
            };
            if !pick_should_replace(best.as_ref(), distance, Some("virtual_circular_face")) {
                continue;
            }
            let secondary = orthogonal_reference(axis, reference);
            *best = Some(NativePick {
                body_id: body.id.0,
                occurrence_id,
                face_id: face.id.0,
                edge_id: None,
                point: (origin + direction * distance).to_array(),
                distance,
                connector_kind: Some("virtual_circular_face".to_string()),
                connector_origin: Some(local_center.to_array()),
                connector_primary_axis: Some(local_primary.to_array()),
                connector_secondary_axis: Some(secondary.to_array()),
                connector_radius: Some(cylinder.radius as f32),
            });
        }
    }

    // Ordinary edge selection has its own screen-space picker. Analytic
    // connector rings must not masquerade as face hits with face_id = 0.
    if purpose != NativePickPurpose::JointConnector {
        return;
    }
    for edge in &body.edges {
        let Some(circle) = edge.circle.filter(|circle| circle.closed) else {
            continue;
        };
        if !circle.radius.is_finite() || circle.radius <= 0.0 {
            continue;
        }
        let local_center = Vec3::new(
            circle.center.x as f32,
            circle.center.y as f32,
            circle.center.z as f32,
        );
        let local_normal = Vec3::new(
            circle.normal.x as f32,
            circle.normal.y as f32,
            circle.normal.z as f32,
        )
        .normalize_or_zero();
        let local_reference = Vec3::new(
            circle.reference.x as f32,
            circle.reference.y as f32,
            circle.reference.z as f32,
        );
        if local_normal == Vec3::ZERO {
            continue;
        }
        let world_center = transform.transform_point(local_center);
        let world_normal = (transform.rotation * local_normal).normalize_or_zero();
        let Some(distance) = ray_plane_ring(
            origin,
            direction,
            world_center,
            world_normal,
            circle.radius as f32,
            world_per_pixel_factor,
        ) else {
            continue;
        };
        if !pick_should_replace(best.as_ref(), distance, Some("circular_edge")) {
            continue;
        }
        *best = Some(NativePick {
            body_id: body.id.0,
            occurrence_id,
            face_id: 0,
            edge_id: Some(edge.id.0),
            point: (origin + direction * distance).to_array(),
            distance,
            connector_kind: Some("circular_edge".to_string()),
            connector_origin: Some(local_center.to_array()),
            connector_primary_axis: Some(local_normal.to_array()),
            connector_secondary_axis: Some(
                orthogonal_reference(local_normal, local_reference).to_array(),
            ),
            connector_radius: Some(circle.radius as f32),
        });
    }
}

struct PickConnectorFrame {
    kind: &'static str,
    origin: Vec3,
    primary_axis: Vec3,
    secondary_axis: Vec3,
    radius: Option<f32>,
}

fn connector_for_face(
    face: &nbcad_solid::FaceDto,
    local_point: Vec3,
) -> Option<PickConnectorFrame> {
    if let Some(plane) = face.plane {
        let origin = face.signature.map_or_else(
            || {
                Vec3::new(
                    plane.origin[0] as f32,
                    plane.origin[1] as f32,
                    plane.origin[2] as f32,
                )
            },
            |signature| {
                Vec3::new(
                    signature.centroid.x as f32,
                    signature.centroid.y as f32,
                    signature.centroid.z as f32,
                )
            },
        );
        return Some(PickConnectorFrame {
            kind: "planar_face",
            origin,
            primary_axis: Vec3::new(
                plane.normal[0] as f32,
                plane.normal[1] as f32,
                plane.normal[2] as f32,
            )
            .normalize_or_zero(),
            secondary_axis: Vec3::new(plane.u[0] as f32, plane.u[1] as f32, plane.u[2] as f32)
                .normalize_or_zero(),
            radius: None,
        });
    }
    let cylinder = face.cylinder?;
    let axis_origin = Vec3::new(
        cylinder.origin.x as f32,
        cylinder.origin.y as f32,
        cylinder.origin.z as f32,
    );
    let axis = Vec3::new(
        cylinder.axis.x as f32,
        cylinder.axis.y as f32,
        cylinder.axis.z as f32,
    )
    .normalize_or_zero();
    if axis == Vec3::ZERO {
        return None;
    }
    let origin = axis_origin + axis * (local_point - axis_origin).dot(axis);
    let reference = Vec3::new(
        cylinder.reference.x as f32,
        cylinder.reference.y as f32,
        cylinder.reference.z as f32,
    );
    let radial = local_point - origin;
    Some(PickConnectorFrame {
        kind: "cylindrical_face",
        origin,
        primary_axis: axis,
        secondary_axis: if radial.length_squared() > 1.0e-10 {
            radial.normalize()
        } else {
            orthogonal_reference(axis, reference)
        },
        radius: Some(cylinder.radius as f32),
    })
}

fn orthogonal_reference(axis: Vec3, candidate: Vec3) -> Vec3 {
    let projected = candidate - axis * candidate.dot(axis);
    if projected.length_squared() > 1.0e-10 {
        projected.normalize()
    } else {
        let fallback = if axis.cross(Vec3::X).length_squared() > 1.0e-10 {
            Vec3::X
        } else {
            Vec3::Y
        };
        (fallback - axis * fallback.dot(axis)).normalize_or_zero()
    }
}

fn pick_should_replace(
    current: Option<&NativePick>,
    distance: f32,
    candidate_kind: Option<&str>,
) -> bool {
    let Some(current) = current else {
        return true;
    };
    const TIE_EPSILON: f32 = 1.0e-4;
    if distance < current.distance - TIE_EPSILON {
        return true;
    }
    if (distance - current.distance).abs() > TIE_EPSILON {
        return false;
    }
    let priority = |kind: Option<&str>| match kind {
        Some("circular_edge") => 2,
        Some("virtual_circular_face") => 0,
        _ => 1,
    };
    priority(candidate_kind) > priority(current.connector_kind.as_deref())
}

fn ray_plane_disk(
    origin: Vec3,
    direction: Vec3,
    center: Vec3,
    normal: Vec3,
    radius: f32,
) -> Option<f32> {
    let denominator = direction.dot(normal);
    if denominator.abs() <= 1.0e-7 {
        return None;
    }
    let distance = (center - origin).dot(normal) / denominator;
    if distance <= 1.0e-5 {
        return None;
    }
    let point = origin + direction * distance;
    ((point - center).length_squared() <= (radius * 1.025).powi(2)).then_some(distance)
}

fn ray_plane_ring(
    origin: Vec3,
    direction: Vec3,
    center: Vec3,
    normal: Vec3,
    radius: f32,
    world_per_pixel_factor: f32,
) -> Option<f32> {
    let denominator = direction.dot(normal);
    if denominator.abs() <= 1.0e-7 {
        return None;
    }
    let distance = (center - origin).dot(normal) / denominator;
    if distance <= 1.0e-5 {
        return None;
    }
    let point = origin + direction * distance;
    let radial_distance = (point - center).length();
    let tolerance = (distance * world_per_pixel_factor * 6.0)
        .max(radius * 0.01)
        .max(0.025);
    ((radial_distance - radius).abs() <= tolerance).then_some(distance)
}

fn mesh_position(body: &BodyDto, index: u32) -> Option<Vec3> {
    let offset = index as usize * 3;
    let coordinates = body.mesh.positions.get(offset..offset + 3)?;
    Some(Vec3::new(coordinates[0], coordinates[1], coordinates[2]))
}

fn ray_triangle(origin: Vec3, direction: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let edge_1 = b - a;
    let edge_2 = c - a;
    let p = direction.cross(edge_2);
    let determinant = edge_1.dot(p);
    if determinant.abs() < 1.0e-7 {
        return None;
    }
    let inverse = 1.0 / determinant;
    let t = origin - a;
    let u = t.dot(p) * inverse;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = t.cross(edge_1);
    let v = direction.dot(q) * inverse;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = edge_2.dot(q) * inverse;
    (distance > 0.0).then_some(distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_native_edge_eligibility_distinguishes_straight_from_bent_edges() {
        let edge = |points: &[[f64; 3]]| nbcad_solid::EdgeDto {
            id: nbcad_core::EdgeId(1),
            key: "edge".to_string(),
            points: points
                .iter()
                .map(|point| nbcad_solid::Point3Dto {
                    x: point[0],
                    y: point[1],
                    z: point[2],
                })
                .collect(),
            circle: None,
            refinable: true,
        };
        assert!(edge_is_straight(&edge(&[
            [0.0, 0.0, 0.0],
            [5.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
        ])));
        assert!(!edge_is_straight(&edge(&[
            [0.0, 0.0, 0.0],
            [5.0, 1.0, 0.0],
            [10.0, 0.0, 0.0],
        ])));
    }

    fn assert_engine_ok(response: String) -> serde_json::Value {
        let envelope: serde_json::Value =
            serde_json::from_str(&response).expect("engine response should be JSON");
        assert_eq!(envelope["ok"], true, "engine error: {envelope}");
        envelope["value"].clone()
    }

    #[test]
    fn cad_studio_lights_are_camera_relative_and_bilaterally_balanced() {
        let camera = ViewportCamera::default();
        let target = Vec3::from_array(camera.target);
        let view = (Vec3::from_array(camera.position) - target).normalize();
        let up = Vec3::from_array(camera.up).normalize();
        let right = view.cross(up).normalize();
        let (key, fill) = camera_relative_light_transforms(camera);
        let key_direction = (key.translation - target).normalize();
        let fill_direction = (fill.translation - target).normalize();

        assert!((key_direction.dot(view) - fill_direction.dot(view)).abs() < 1.0e-6);
        assert!((key_direction.dot(right) + fill_direction.dot(right)).abs() < 1.0e-6);

        let rotated = ViewportCamera {
            position: [-170.0, -170.0, 130.0],
            ..camera
        };
        let (rotated_key, rotated_fill) = camera_relative_light_transforms(rotated);
        assert_ne!(key.translation, rotated_key.translation);
        assert_ne!(fill.translation, rotated_fill.translation);
    }

    #[test]
    fn camera_transform_stays_finite_at_up_axis_alignment() {
        for position in [[0.0, 0.0, -25.0], [0.0, 0.0, 25.0]] {
            let transform = camera_transform(ViewportCamera {
                position,
                target: [0.0, 0.0, 0.0],
                up: [0.0, 0.0, 1.0],
                ..ViewportCamera::default()
            });
            assert!(
                transform
                    .to_matrix()
                    .to_cols_array()
                    .iter()
                    .all(|value| value.is_finite()),
                "camera transform must remain finite at position {position:?}"
            );
        }
    }

    #[test]
    fn native_model_carries_closed_profiles_from_an_internal_midplane_sketch() {
        let state = crate::state::AppState::new();
        assert_engine_ok(
            state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#),
        );
        assert_engine_ok(state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":-20.0,"y":-20.0},
                "p2":{"x":20.0,"y":20.0},
                "ctrl_held":false
            }"#,
        ));
        assert_engine_ok(state.engine_call("end_sketch", ""));
        assert_engine_ok(state.solid_extrude(
            r#"{
                "sketch_name":"Sketch1",
                "profile_indices":[0],
                "operation":"new_body",
                "extent":{"type":"distance","distance":20.0},
                "taper_angle_deg":0.0,
                "flip":false,
                "target_body_ids":[]
            }"#,
        ));
        assert_engine_ok(state.engine_call(
            "datum_plane_create",
            r#"{
                "source":{
                    "type":"offset",
                    "reference":{"type":"origin_plane","plane":"xy"},
                    "distance":20.0
                }
            }"#,
        ));
        assert_engine_ok(state.engine_call(
            "datum_plane_create",
            r#"{
                "source":{
                    "type":"midplane",
                    "first":{"type":"origin_plane","plane":"xy"},
                    "second":{"type":"datum_plane","datum_id":1}
                }
            }"#,
        ));
        assert_engine_ok(
            state.engine_call("begin_sketch", r#"{"type":"datum_plane","datum_id":2}"#),
        );
        assert_engine_ok(state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":-5.0,"y":-5.0},
                "p2":{"x":5.0,"y":5.0},
                "ctrl_held":false
            }"#,
        ));
        assert_engine_ok(state.engine_call("end_sketch", ""));

        let (_, _, scene, _, _, datum_planes, profile_catalog, _, _, _) = state.viewport_snapshot();
        assert_eq!(scene.bodies.len(), 1);
        assert_eq!(datum_planes.len(), 2);
        assert!((datum_planes[1].basis.origin[2] - 10.0).abs() < 1.0e-9);
        let profile = profile_catalog
            .iter()
            .find(|entry| entry.sketch_name == "Sketch2")
            .expect("the internal midplane sketch must reach the native model");
        assert_eq!(profile.profiles.len(), 1);
        assert_eq!(profile.profiles[0].nesting_depth, 0);
        assert_eq!(profile.profiles[0].points.len(), 4);
        assert!((profile.basis.origin[2] - 10.0).abs() < 1.0e-9);
    }

    #[test]
    fn visible_sketch_points_use_the_persistent_always_on_top_layer() {
        let point: EntityDto = serde_json::from_str(
            r#"{
                "kind": "point",
                "id": 7,
                "position": { "x": 0.0, "y": 0.0 },
                "fully_defined": false
            }"#,
        )
        .expect("standalone sketch point should deserialize");

        assert_eq!(sketch_grip_positions(&point), &[SketchVec2::ZERO]);
        assert!((-1.0..=1.0).contains(&SKETCH_DEPTH_BIAS));
        assert!(SKETCH_POINT_OUTLINE_DEPTH_BIAS > SKETCH_DEPTH_BIAS);
        assert!(SKETCH_LINE_WIDTH < HIGHLIGHT_LINE_WIDTH);
        assert!(SKETCH_POINT_OUTLINE_WIDTH > SKETCH_LINE_WIDTH);
        assert!(SKETCH_POINT_OUTLINE_WIDTH <= 2.0);
        assert!(SKETCH_POINT_OUTLINE_RADIUS_PX > SKETCH_POINT_RADIUS_PX);
        assert!(SKETCH_POINT_OUTLINE_RADIUS_PX < 3.5);
        assert!(HIGHLIGHT_LINE_WIDTH <= 2.0);
    }

    #[test]
    fn interaction_line_weight_tracks_logical_viewport_and_backing_density() {
        assert_eq!(DIRECT_PICK_FEEDBACK_OFFSET, PROFILE_PICK_OFFSET);
        assert_eq!(PICK_FEEDBACK_LINE_WIDTH, 1.0);
        assert_eq!(DIRECT_PICK_FEEDBACK_LINE_WIDTH, 0.75);
        assert_eq!(PROFILE_BORDER_LINE_WIDTH, DIRECT_PICK_FEEDBACK_LINE_WIDTH);
        assert!(PROFILE_BORDER_LINE_WIDTH < SKETCH_LINE_WIDTH);
        assert!(DIRECT_PICK_FEEDBACK_LINE_WIDTH < PICK_FEEDBACK_LINE_WIDTH);
        assert!(PICK_FEEDBACK_LINE_WIDTH < SKETCH_LINE_WIDTH);
        assert!(HIGHLIGHT_LINE_WIDTH > PICK_FEEDBACK_LINE_WIDTH);
        assert_eq!(
            PICK_FEEDBACK_HALO_LINE_WIDTH,
            PICK_FEEDBACK_LINE_WIDTH * 2.0
        );
        for bias in [
            SKETCH_DEPTH_BIAS,
            SKETCH_POINT_OUTLINE_DEPTH_BIAS,
            PICK_FEEDBACK_HALO_DEPTH_BIAS,
            PICK_FEEDBACK_DEPTH_BIAS,
            DIRECT_PICK_FEEDBACK_DEPTH_BIAS,
        ] {
            assert!(
                (-1.0..=1.0).contains(&bias),
                "gizmo depth bias {bias} must stay inside Bevy's documented range",
            );
        }
        assert!(SKETCH_DEPTH_BIAS > PICK_FEEDBACK_HALO_DEPTH_BIAS);
        assert!(PICK_FEEDBACK_HALO_DEPTH_BIAS > PICK_FEEDBACK_DEPTH_BIAS);
        assert!(PICK_FEEDBACK_DEPTH_BIAS > DIRECT_PICK_FEEDBACK_DEPTH_BIAS);
        assert!((viewport_line_logical_scale(960.0, 720.0) - 1.0).abs() < 1.0e-6);
        assert!(viewport_line_logical_scale(1600.0, 1000.0) > 1.0);
        assert_eq!(
            viewport_line_logical_scale(10_000.0, 10_000.0),
            VIEWPORT_LINE_SCALE_MAX
        );
        assert_eq!(
            viewport_line_logical_scale(320.0, 240.0),
            VIEWPORT_LINE_SCALE_MIN
        );
        let logical = viewport_line_raster_scale(1600.0, 1000.0, 1.0);
        let retina = viewport_line_raster_scale(1600.0, 1000.0, 2.0);
        assert!((retina - logical * 2.0).abs() < 1.0e-6);
    }

    #[test]
    fn sketch_point_radius_stays_constant_in_screen_space_at_different_depths() {
        let camera = ViewportCamera {
            position: [0.0, 0.0, 10.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            vertical_fov_degrees: 45.0,
        };
        let viewport = ViewportSizeResource {
            logical_width: 1_200.0,
            logical_height: 800.0,
        };
        let near = Vec3::ZERO;
        let far = Vec3::new(0.0, 0.0, -90.0);

        let near_radius = screen_space_disc_radius(camera, viewport, near, SKETCH_POINT_RADIUS_PX);
        let far_radius = screen_space_disc_radius(camera, viewport, far, SKETCH_POINT_RADIUS_PX);
        let near_world_per_pixel = world_per_pixel_at(camera, viewport, near);
        let far_world_per_pixel = world_per_pixel_at(camera, viewport, far);

        assert!((near_radius / near_world_per_pixel - SKETCH_POINT_RADIUS_PX).abs() < 1.0e-5);
        assert!((far_radius / far_world_per_pixel - SKETCH_POINT_RADIUS_PX).abs() < 1.0e-5);
        assert!(far_radius > near_radius);
    }

    #[test]
    fn sketch_point_billboard_axes_are_orthonormal() {
        let (right, up) = camera_facing_axes(ViewportCamera::default());

        assert!((right.length() - 1.0).abs() < 1.0e-5);
        assert!((up.length() - 1.0).abs() < 1.0e-5);
        assert!(right.dot(up).abs() < 1.0e-5);
    }

    #[test]
    fn native_preview_preserves_endpoint_snap_semantics() {
        let preview: ViewportPreview = serde_json::from_str(
            r#"{
                "lines": [{
                    "color": [0.2, 0.7, 1.0, 0.68],
                    "width": 1.15,
                    "pattern": "dotted",
                    "segments": [0.0, 0.0, 0.0, 10.0, 0.0, 0.0]
                }],
                "points": [],
                "triangles": [{
                    "color": [1.0, 0.4, 0.2, 0.25],
                    "positions": [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 5.0, 0.0],
                    "xray": true
                }],
                "arrows": [{
                    "start": [0.0, 0.0, 0.0],
                    "end": [0.0, 0.0, 10.0],
                    "color": [0.2, 0.7, 1.0, 1.0],
                    "width": 2.0,
                    "xray": true
                }],
                "annotations": [],
                "marker": {
                    "position": [12.0, -4.0, 0.18],
                    "kind": "point"
                }
            }"#,
        )
        .expect("semantic snap marker should deserialize across the Tauri boundary");

        let marker = preview.marker.expect("endpoint marker should be retained");
        assert_eq!(marker.kind, ViewportSnapKind::Point);
        assert_eq!(marker.position, [12.0, -4.0, 0.18]);
        assert_eq!(preview.lines[0].pattern, ViewportLinePattern::Dotted);
        assert_eq!(preview.lines[0].segments.len(), 6);
        assert!(preview.triangles[0].xray);
        assert_eq!(preview.triangles[0].positions.len(), 9);
        assert_eq!(preview.arrows[0].end, [0.0, 0.0, 10.0]);
        assert_eq!(preview.arrows[0].width, 2.0);
        assert!(HIGHLIGHT_LINE_WIDTH <= 2.0);
        assert!(SNAP_MARKER_HALF_SIZE_PX >= 5.0);
    }

    #[test]
    fn camera_frequency_annotation_updates_do_not_rebuild_preview_meshes() {
        let base: ViewportPreview = serde_json::from_str(
            r#"{
                "lines": [],
                "points": [],
                "triangles": [{
                    "color": [0.2, 0.7, 1.0, 0.25],
                    "positions": [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0],
                    "xray": true
                }],
                "arrows": [{
                    "start": [0.0, 0.0, 0.0],
                    "end": [0.0, 0.0, 10.0],
                    "color": [0.2, 0.7, 1.0, 1.0],
                    "width": 2.0,
                    "xray": true
                }],
                "annotations": [{
                    "screen": [100.0, 100.0],
                    "color": [1.0, 1.0, 1.0, 1.0],
                    "text": "10 mm",
                    "kind": "dimension"
                }],
                "marker": null
            }"#,
        )
        .expect("preview should deserialize");
        let mut moved_annotation = base.clone();
        moved_annotation.annotations[0].screen = [420.0, 240.0];
        assert!(
            !preview_mesh_content_changed(&base, &moved_annotation),
            "camera projection updates must keep retained GPU meshes"
        );

        let mut edited_tool = base.clone();
        edited_tool.arrows[0].end[2] = 25.0;
        assert!(preview_mesh_content_changed(&base, &edited_tool));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn dom_rect_mapping_accounts_for_window_title_bar_safe_area() {
        let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1_440.0, 900.0));
        let content = NSRect::new(NSPoint::new(0.0, 32.0), NSSize::new(1_440.0, 868.0));
        let viewport = ViewportRect {
            x: 240.0,
            y: 120.0,
            width: 1_200.0,
            height: 700.0,
            corner_radius: 0.0,
        };

        let mapped = dom_rect_to_content_rect(bounds, content, true, viewport);
        assert_eq!(mapped.origin.x, 240.0);
        assert_eq!(mapped.origin.y, 152.0);
        assert_eq!(mapped.size.height, 700.0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn fullscreen_dom_rect_mapping_uses_the_full_webview_bounds() {
        let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1_440.0, 900.0));
        let viewport = ViewportRect {
            x: 240.0,
            y: 120.0,
            width: 1_200.0,
            height: 728.0,
            corner_radius: 0.0,
        };

        let mapped = dom_rect_to_content_rect(bounds, bounds, true, viewport);
        assert_eq!(mapped.origin.y, 120.0);
        assert_eq!(mapped.size.height, 728.0);
    }

    #[test]
    fn rectangle_intersection_clips_overlay_to_viewport() {
        let viewport = ViewportRect {
            x: 100.0,
            y: 50.0,
            width: 300.0,
            height: 200.0,
            corner_radius: 0.0,
        };
        let overlay = ViewportRect {
            x: 80.0,
            y: 40.0,
            width: 80.0,
            height: 50.0,
            corner_radius: 12.0,
        };
        assert_eq!(
            intersect_rect(overlay, viewport)
                .expect("rectangles overlap")
                .width,
            60.0
        );
        assert_eq!(
            intersect_rect(viewport, viewport)
                .expect("identical rectangles overlap")
                .corner_radius,
            0.0
        );
        assert_eq!(
            intersect_rect(
                ViewportRect {
                    x: 120.0,
                    y: 70.0,
                    width: 80.0,
                    height: 60.0,
                    corner_radius: 14.0,
                },
                viewport,
            )
            .expect("rounded overlay is inside viewport")
            .corner_radius,
            14.0
        );
        assert_eq!(
            intersect_rect(overlay, viewport)
                .expect("clipped overlay overlaps")
                .corner_radius,
            0.0
        );
    }

    #[test]
    fn reference_planes_scale_with_camera_depth() {
        let viewport = ViewportSizeResource {
            logical_width: 1_200.0,
            logical_height: 800.0,
        };
        let near = ViewportCamera {
            position: [0.0, 0.0, 100.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            vertical_fov_degrees: 45.0,
        };
        let far = ViewportCamera {
            position: [0.0, 0.0, 200.0],
            ..near
        };
        let near_half = reference_plane_half_size(near, viewport, Vec3::ZERO);
        let far_half = reference_plane_half_size(far, viewport, Vec3::ZERO);
        assert!(near_half > 0.0);
        assert!((far_half / near_half - 2.0).abs() < 1.0e-5);
    }

    #[test]
    fn embedded_resize_updates_pixels_and_notifies_bevy_camera_system() {
        let mut app = App::new();
        app.add_message::<WindowResized>()
            .add_message::<WindowScaleFactorChanged>();
        let window_entity = app
            .world_mut()
            .spawn((PrimaryWindow, Window::default()))
            .id();

        assert!(resize_embedded_window(
            app.world_mut(),
            800.0,
            600.0,
            2.0,
            true,
        ));

        let window = app
            .world()
            .get::<Window>(window_entity)
            .expect("primary window should remain available");
        assert_eq!(window.resolution.physical_width(), 1_600);
        assert_eq!(window.resolution.physical_height(), 1_200);

        let resized = app.world().resource::<Messages<WindowResized>>();
        assert_eq!(
            resized.iter_current_update_messages().next(),
            Some(&WindowResized {
                window: window_entity,
                width: 800.0,
                height: 600.0,
            })
        );
        let scale_changed = app.world().resource::<Messages<WindowScaleFactorChanged>>();
        assert_eq!(
            scale_changed.iter_current_update_messages().next(),
            Some(&WindowScaleFactorChanged {
                window: window_entity,
                scale_factor: 2.0,
            })
        );
    }

    #[test]
    fn ray_triangle_returns_forward_hit() {
        let distance = ray_triangle(
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::NEG_Z,
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        )
        .expect("ray should hit");
        assert!((distance - 5.0).abs() < 1.0e-5);
    }

    #[test]
    fn native_reference_plane_matches_the_react_pick_footprint() {
        let mesh = reference_plane_mesh(&origin_plane_bases()[0].1, REFERENCE_PLANE_HALF_SIZE);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|values| values.as_float3())
            .expect("reference plane should expose float3 positions");
        let min_x = positions
            .iter()
            .map(|position| position[0])
            .fold(f32::INFINITY, f32::min);
        let max_x = positions
            .iter()
            .map(|position| position[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = positions
            .iter()
            .map(|position| position[1])
            .fold(f32::INFINITY, f32::min);
        let max_y = positions
            .iter()
            .map(|position| position[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(max_x - min_x, 100.0);
        assert_eq!(max_y - min_y, 100.0);
    }

    #[test]
    fn highlighted_face_boundary_omits_shared_tessellation_diagonal() {
        // OCCT intentionally emits separate vertices for every triangle so
        // normals remain face-correct. The two copies of the diagonal still
        // represent the same geometric segment and must cancel each other.
        let positions = vec![
            0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0, // first triangle
            0.0, 0.0, 0.0, 10.0, 10.0, 0.0, 0.0, 10.0, 0.0, // second triangle
        ];
        let segments = triangle_boundary_segments(&positions, &[0, 1, 2, 3, 4, 5]);
        assert_eq!(segments.len(), 4, "only the quad perimeter should remain");
        let first = Vec3::ZERO;
        let opposite = Vec3::new(10.0, 10.0, 0.0);
        assert!(
            !segments.iter().any(|(start, end)| {
                (*start == first && *end == opposite) || (*start == opposite && *end == first)
            }),
            "the internal triangulation diagonal must not be rendered"
        );
    }

    #[test]
    fn native_highlight_stroke_respects_two_pixel_cap() {
        assert!(HIGHLIGHT_LINE_WIDTH <= 2.0);
    }

    #[test]
    fn native_picker_hits_an_actual_occt_extrusion_snapshot() {
        let state = crate::state::AppState::new();
        state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#);
        state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":-10.0,"y":-10.0},
                "p2":{"x":10.0,"y":10.0},
                "ctrl_held":false
            }"#,
        );
        state.engine_call("end_sketch", "");
        state.solid_extrude(
            r#"{
                "sketch_name":"Sketch1",
                "profile_indices":[0],
                "operation":"new_body",
                "extent":{"type":"distance","distance":10.0},
                "taper_angle_deg":0.0,
                "flip":false,
                "target_body_ids":[]
            }"#,
        );

        let (_, _, scene, _, _, _, _, _, _, _) = state.viewport_snapshot();
        assert_eq!(scene.bodies.len(), 1);
        assert_eq!(scene.bodies[0].mesh.indices.len(), 36);
        let hit = pick_occt_scene(
            &scene,
            ViewportCamera {
                position: [0.0, 0.0, 100.0],
                target: [0.0, 0.0, 0.0],
                up: [0.0, 1.0, 0.0],
                vertical_fov_degrees: 45.0,
            },
            (800.0, 600.0),
            400.0,
            300.0,
            &[],
            &[],
            &[],
            NativePickPurpose::Geometry,
        )
        .expect("center ray should hit the OCCT box");
        assert_eq!(hit.body_id, scene.bodies[0].id.0);
        assert!(hit.point[2] > 9.99);
        assert!(
            pick_occt_scene(
                &scene,
                ViewportCamera {
                    position: [0.0, 0.0, 100.0],
                    target: [0.0, 0.0, 0.0],
                    up: [0.0, 1.0, 0.0],
                    vertical_fov_degrees: 45.0,
                },
                (800.0, 600.0),
                400.0,
                300.0,
                &[scene.bodies[0].id.0],
                &[],
                &[],
                NativePickPurpose::Geometry,
            )
            .is_none(),
            "browser-hidden bodies must not remain pickable"
        );
        let translated = BodyPoseDto {
            body_id: scene.bodies[0].id,
            translation: [40.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
        };
        let moved_hit = pick_occt_scene(
            &scene,
            ViewportCamera {
                position: [40.0, 0.0, 100.0],
                target: [40.0, 0.0, 0.0],
                up: [0.0, 1.0, 0.0],
                vertical_fov_degrees: 45.0,
            },
            (800.0, 600.0),
            400.0,
            300.0,
            &[],
            &[translated],
            &[],
            NativePickPurpose::Geometry,
        )
        .expect("native picking must follow the solved body pose");
        assert!((moved_hit.point[0] - 40.0).abs() < 1.0e-4);
        let mesh = body_mesh(&scene.bodies[0])
            .expect("committed OCCT geometry should become one indexed Bevy mesh per body");
        assert_eq!(
            mesh.count_vertices(),
            scene.bodies[0].mesh.positions.len() / 3
        );
        assert_eq!(
            mesh.indices().expect("body mesh should stay indexed").len(),
            scene.bodies[0].mesh.indices.len(),
            "body batching must preserve the complete OCCT tessellation"
        );
        let overlay = face_mesh(&scene.bodies[0], &scene.bodies[0].faces[0])
            .expect("a hovered or selected face can still create a transient overlay");
        assert_eq!(
            overlay.count_vertices(),
            scene.bodies[0].faces[0].index_count as usize
        );

        let expected_face_count = scene.bodies[0].faces.len();
        let mut render_app = App::new();
        render_app
            .init_resource::<ModelResource>()
            .init_resource::<ModelGeometryCache>()
            .init_resource::<RenderedRevisions>()
            .init_resource::<PaletteResource>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, rebuild_occt_meshes);
        {
            let mut model = render_app.world_mut().resource_mut::<ModelResource>();
            model.session_id = "batched-body-test".to_string();
            model.geometry_revision = 1;
            model.scene = scene.clone();
            model.revision = 1;
        }
        render_app.update();
        let body_draws = {
            let world = render_app.world_mut();
            let mut query = world.query::<(&NativeCadBody, &Mesh3d)>();
            query.iter(world).count()
        };
        let face_metadata = {
            let world = render_app.world_mut();
            let mut query = world.query::<(&NativeCadFace, Option<&Mesh3d>)>();
            query
                .iter(world)
                .inspect(|(_, mesh)| assert!(mesh.is_none()))
                .count()
        };
        assert_eq!(
            body_draws, 1,
            "one solid body should use one committed draw mesh"
        );
        assert_eq!(face_metadata, expected_face_count);

        let started = Instant::now();
        for _ in 0..10_000 {
            std::hint::black_box(pick_occt_scene(
                &scene,
                ViewportCamera {
                    position: [0.0, 0.0, 100.0],
                    target: [0.0, 0.0, 0.0],
                    up: [0.0, 1.0, 0.0],
                    vertical_fov_degrees: 45.0,
                },
                (800.0, 600.0),
                400.0,
                300.0,
                &[],
                &[],
                &[],
                NativePickPurpose::Geometry,
            ));
        }
        let average_micros = started.elapsed().as_secs_f64() * 100.0;
        eprintln!("actual OCCT box pick average: {average_micros:.3} µs");
        assert!(
            average_micros < 5_000.0,
            "native picking exceeded the demo's 5 ms CPU budget"
        );
    }

    #[test]
    fn native_occurrences_share_meshes_and_pick_the_exact_instance() {
        let state = crate::state::AppState::new();
        state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#);
        state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":-10.0,"y":-10.0},
                "p2":{"x":10.0,"y":10.0},
                "ctrl_held":false
            }"#,
        );
        state.engine_call("end_sketch", "");
        state.solid_extrude(
            r#"{
                "sketch_name":"Sketch1",
                "profile_indices":[0],
                "operation":"new_body",
                "extent":{"type":"distance","distance":10.0},
                "taper_angle_deg":0.0,
                "flip":false,
                "target_body_ids":[]
            }"#,
        );
        let (_, _, scene, _, _, _, _, _, _, _) = state.viewport_snapshot();
        let body_id = scene.bodies[0].id;
        let instances = vec![
            InstanceBodyPoseDto {
                occurrence_id: nbcad_sketch::OccurrenceId(41),
                component_id: nbcad_sketch::ComponentId(7),
                body_id,
                translation: [-30.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                visible: true,
            },
            InstanceBodyPoseDto {
                occurrence_id: nbcad_sketch::OccurrenceId(42),
                component_id: nbcad_sketch::ComponentId(7),
                body_id,
                translation: [30.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                visible: true,
            },
        ];
        for (x, occurrence_id) in [(-30.0, 41), (30.0, 42)] {
            let hit = pick_occt_scene(
                &scene,
                ViewportCamera {
                    position: [x, 0.0, 100.0],
                    target: [x, 0.0, 0.0],
                    up: [0.0, 1.0, 0.0],
                    vertical_fov_degrees: 45.0,
                },
                (800.0, 600.0),
                400.0,
                300.0,
                &[],
                &[],
                &instances,
                NativePickPurpose::Geometry,
            )
            .expect("each visible occurrence should be independently pickable");
            assert_eq!(hit.body_id, body_id.0);
            assert_eq!(hit.occurrence_id, Some(occurrence_id));
        }

        let mut render_app = App::new();
        render_app
            .init_resource::<ModelResource>()
            .init_resource::<ModelGeometryCache>()
            .init_resource::<RenderedRevisions>()
            .init_resource::<PaletteResource>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, rebuild_occt_meshes);
        {
            let mut model = render_app.world_mut().resource_mut::<ModelResource>();
            model.session_id = "instance-mesh-test".to_string();
            model.geometry_revision = 1;
            model.instance_revision = 1;
            model.scene = scene;
            model.instance_body_poses = instances;
            model.revision = 1;
        }
        render_app.update();
        let mesh_ids = {
            let world = render_app.world_mut();
            let mut query = world.query::<(&NativeCadBody, &Mesh3d)>();
            query
                .iter(world)
                .map(|(body, mesh)| (body.occurrence_id, mesh.0.id()))
                .collect::<Vec<_>>()
        };
        assert_eq!(mesh_ids.len(), 2);
        assert_ne!(mesh_ids[0].0, mesh_ids[1].0);
        assert_eq!(
            mesh_ids[0].1, mesh_ids[1].1,
            "reusable occurrences must retain one shared GPU mesh asset",
        );
    }

    #[test]
    fn native_pick_purpose_defaults_to_geometry_and_requires_explicit_connector_opt_in() {
        assert_eq!(NativePickPurpose::default(), NativePickPurpose::Geometry);
        assert_eq!(
            serde_json::from_str::<NativePickPurpose>(r#""geometry""#).unwrap(),
            NativePickPurpose::Geometry,
        );
        assert_eq!(
            serde_json::from_str::<NativePickPurpose>(r#""jointConnector""#).unwrap(),
            NativePickPurpose::JointConnector,
        );
        assert!(serde_json::from_str::<NativePickPurpose>(r#""unknown""#).is_err());
    }

    #[test]
    fn native_geometry_picker_ignores_virtual_caps_on_an_actual_partial_revolve() {
        let state = crate::state::AppState::new();
        state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xz"}"#);
        state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":2.0,"y":0.0},
                "p2":{"x":5.0,"y":10.0},
                "ctrl_held":true
            }"#,
        );
        state.engine_call("end_sketch", "");
        let response = state.solid_revolve(
            r#"{
                "sketch_name":"Sketch1",
                "profile_indices":[0],
                "axis_origin":{"x":0.0,"y":0.0},
                "axis_direction":{"x":0.0,"y":1.0},
                "angle_deg":80.0,
                "flip":false,
                "operation":"new_body",
                "target_body_ids":[]
            }"#,
        );
        let (_, _, scene, _, _, _, _, _, _, _) = state.viewport_snapshot();
        assert_eq!(scene.bodies.len(), 1, "{response}");
        assert!(scene.errors.is_empty());
        let body = &scene.bodies[0];
        assert!(body.faces.iter().any(|face| face.cylinder.is_some()));

        // This ray lies in the missing sector, inside the old full-disk
        // connector proxy but outside every physical face of the 80° part.
        let empty_sector_camera = ViewportCamera {
            position: [-3.0, 0.0, 50.0],
            target: [-3.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            vertical_fov_degrees: 45.0,
        };
        let pick = |scene: &SolidSceneDto, camera, purpose| {
            pick_occt_scene(
                scene,
                camera,
                (800.0, 600.0),
                400.0,
                300.0,
                &[],
                &[],
                &[],
                purpose,
            )
        };
        assert!(
            pick(&scene, empty_sector_camera, NativePickPurpose::Geometry).is_none(),
            "empty space beside a partial revolve must not select its cylindrical wall",
        );
        let connector = pick(
            &scene,
            empty_sector_camera,
            NativePickPurpose::JointConnector,
        )
        .expect("explicit joint picking retains the virtual opening target");
        assert_eq!(
            connector.connector_kind.as_deref(),
            Some("virtual_circular_face")
        );

        let top = body
            .faces
            .iter()
            .find(|face| {
                face.plane
                    .as_ref()
                    .is_some_and(|plane| plane.normal[2].abs() > 0.99 && plane.origin[2] > 9.99)
            })
            .expect("partial revolve has a physical top cap");
        let triangle = &body.mesh.indices[top.first_index as usize..top.first_index as usize + 3];
        let center = triangle
            .iter()
            .map(|index| mesh_position(body, *index).unwrap())
            .sum::<Vec3>()
            / 3.0;
        let cap_camera = ViewportCamera {
            position: (center + Vec3::Z * 50.0).to_array(),
            target: center.to_array(),
            ..empty_sector_camera
        };
        for purpose in [
            NativePickPurpose::Geometry,
            NativePickPurpose::JointConnector,
        ] {
            let cap = pick(&scene, cap_camera, purpose).expect("physical cap remains selectable");
            assert_eq!(cap.face_id, top.id.0, "{purpose:?} at {center:?}: {cap:?}");
            assert_ne!(cap.connector_kind.as_deref(), Some("virtual_circular_face"));
            assert!((Vec3::from_array(cap.point) - center).length() < 1.0e-4);
        }

        // A real part behind the missing sector must win; merely discarding
        // a virtual hit after nearest-hit resolution would lose this target.
        state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#);
        state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":-4.0,"y":-1.0},
                "p2":{"x":-2.0,"y":1.0},
                "ctrl_held":true
            }"#,
        );
        state.engine_call("end_sketch", "");
        let response = state.solid_extrude(
            r#"{
                "sketch_name":"Sketch2", "profile_indices":[0],
                "operation":"new_body", "extent":{"type":"distance","distance":5.0},
                "taper_angle_deg":0.0, "flip":true, "target_body_ids":[]
            }"#,
        );
        let (_, _, scene, _, _, _, _, _, _, _) = state.viewport_snapshot();
        assert_eq!(scene.bodies.len(), 2, "{response}");
        let behind = pick(&scene, empty_sector_camera, NativePickPurpose::Geometry)
            .expect("real geometry behind a virtual disk remains selectable");
        assert_eq!(behind.body_id, scene.bodies[1].id.0);
        assert!(behind.point[2].abs() < 1.0e-4);
    }

    #[test]
    fn native_picker_exposes_a_virtual_circular_connector_at_a_cylinder_opening() {
        let body = nbcad_solid::BodyDto {
            topology_signature: String::new(),
            id: nbcad_core::BodyId(7),
            name: "Holed component".to_string(),
            feature_id: nbcad_core::FeatureId(3),
            mesh: nbcad_solid::MeshDto {
                positions: vec![5.0, 0.0, 0.0, 5.0, 0.0, 10.0, 0.0, 5.0, 0.0, 0.0, 5.0, 10.0],
                normals: vec![0.0; 12],
                indices: vec![0, 1, 2, 2, 1, 3],
            },
            faces: vec![nbcad_solid::FaceDto {
                id: nbcad_core::FaceId(70),
                key: "cylindrical-wall".to_string(),
                first_index: 0,
                index_count: 6,
                plane: None,
                signature: None,
                cylinder: Some(nbcad_solid::CylindricalSurfaceDto {
                    origin: nbcad_solid::Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    axis: nbcad_solid::Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    reference: nbcad_solid::Point3Dto {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    radius: 5.0,
                }),
            }],
            edges: vec![nbcad_solid::EdgeDto {
                id: nbcad_core::EdgeId(71),
                key: "outer-chamfer-rim".to_string(),
                points: vec![],
                circle: Some(nbcad_solid::CircularCurveDto {
                    center: nbcad_solid::Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 10.0,
                    },
                    normal: nbcad_solid::Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    reference: nbcad_solid::Point3Dto {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    radius: 8.0,
                    closed: true,
                }),
                refinable: true,
            }],
        };
        let scene = SolidSceneDto {
            bodies: vec![body],
            errors: vec![],
        };
        for x in [0.0, 8.0] {
            assert!(
                pick_occt_scene(
                    &scene,
                    ViewportCamera {
                        position: [x, 0.0, 50.0],
                        target: [x, 0.0, 0.0],
                        up: [0.0, 1.0, 0.0],
                        vertical_fov_degrees: 45.0,
                    },
                    (800.0, 600.0),
                    400.0,
                    300.0,
                    &[],
                    &[],
                    &[],
                    NativePickPurpose::Geometry,
                )
                .is_none(),
                "ordinary face picking must not hit virtual openings or connector rings at x={x}",
            );
        }
        let hit = pick_occt_scene(
            &scene,
            ViewportCamera {
                position: [0.0, 0.0, 50.0],
                target: [0.0, 0.0, 0.0],
                up: [0.0, 1.0, 0.0],
                vertical_fov_degrees: 45.0,
            },
            (800.0, 600.0),
            400.0,
            300.0,
            &[],
            &[],
            &[],
            NativePickPurpose::JointConnector,
        )
        .expect("the otherwise empty cylinder opening should be pickable");
        assert_eq!(hit.connector_kind.as_deref(), Some("virtual_circular_face"));
        assert_eq!(hit.connector_origin, Some([0.0, 0.0, 10.0]));
        assert_eq!(hit.connector_primary_axis, Some([0.0, 0.0, 1.0]));

        let rim_hit = pick_occt_scene(
            &scene,
            ViewportCamera {
                position: [8.0, 0.0, 50.0],
                target: [8.0, 0.0, 0.0],
                up: [0.0, 1.0, 0.0],
                vertical_fov_degrees: 45.0,
            },
            (800.0, 600.0),
            400.0,
            300.0,
            &[],
            &[],
            &[],
            NativePickPurpose::JointConnector,
        )
        .expect("the exact outer chamfer rim should be pickable");
        assert_eq!(rim_hit.connector_kind.as_deref(), Some("circular_edge"));
        assert_eq!(rim_hit.edge_id, Some(71));
        assert_eq!(rim_hit.connector_origin, Some([0.0, 0.0, 10.0]));
        assert_eq!(rim_hit.connector_primary_axis, Some([0.0, 0.0, 1.0]));
        assert_eq!(rim_hit.connector_radius, Some(8.0));

        let wall_hit = pick_occt_scene(
            &scene,
            ViewportCamera {
                position: [50.0, 0.0, 5.0],
                target: [0.0, 0.0, 5.0],
                up: [0.0, 0.0, 1.0],
                vertical_fov_degrees: 45.0,
            },
            (800.0, 600.0),
            400.0,
            300.0,
            &[],
            &[],
            &[],
            NativePickPurpose::Geometry,
        )
        .expect("the physical internal cylinder wall should remain pickable");
        assert_eq!(wall_hit.connector_kind.as_deref(), Some("cylindrical_face"));
        assert_eq!(wall_hit.connector_origin, Some([0.0, 0.0, 5.0]));
        assert_eq!(wall_hit.connector_primary_axis, Some([0.0, 0.0, 1.0]));
        assert_eq!(wall_hit.connector_secondary_axis, Some([1.0, 0.0, 0.0]));
    }

    #[test]
    fn native_body_material_uses_the_document_appearance() {
        let body_id = 73;
        let mut appearance = BodyAppearance::default_for(nbcad_core::BodyId(body_id));
        appearance.color = nbcad_core::Rgba8::opaque(12, 123, 240);
        let model = ModelResource {
            body_appearances: vec![appearance],
            ..default()
        };

        let color = body_appearance_color(&model, body_id, [0.1, 0.2, 0.3]).to_srgba();
        assert!((color.red - 12.0 / 255.0).abs() < 1.0e-6);
        assert!((color.green - 123.0 / 255.0).abs() < 1.0e-6);
        assert!((color.blue - 240.0 / 255.0).abs() < 1.0e-6);
    }

    #[test]
    fn recovery_session_rebind_preserves_solid_faces_and_absorbs_late_bootstrap_sync() {
        let state = crate::state::AppState::new();
        state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#);
        state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":-10.0,"y":-10.0},
                "p2":{"x":10.0,"y":10.0},
                "ctrl_held":false
            }"#,
        );
        state.engine_call("end_sketch", "");
        state.solid_extrude(
            r#"{
                "sketch_name":"Sketch1",
                "profile_indices":[0],
                "operation":"new_body",
                "extent":{"type":"distance","distance":10.0},
                "taper_angle_deg":0.0,
                "flip":false,
                "target_body_ids":[]
            }"#,
        );

        let (
            session_id,
            geometry_revision,
            scene,
            active_sketch,
            finished_sketches,
            datum_planes,
            profile_catalog,
            body_appearances,
            body_poses,
            instance_body_poses,
        ) = state.viewport_snapshot();
        assert_eq!(session_id, BOOTSTRAP_SESSION_ID);
        let face_count = scene
            .bodies
            .iter()
            .map(|body| body.faces.len())
            .sum::<usize>();
        assert!(
            face_count > 0,
            "the recovery fixture must contain solid faces"
        );

        let bootstrap_model = ViewportModel {
            session_id: session_id.clone(),
            geometry_revision,
            scene,
            active_sketch,
            finished_sketches,
            datum_planes,
            profile_catalog,
            body_appearances,
            body_poses,
            instance_body_poses,
        };
        let mut app = App::new();
        app.init_resource::<ModelResource>()
            .init_resource::<ModelGeometryCache>();
        {
            let mut model = app.world_mut().resource_mut::<ModelResource>();
            model.session_id = session_id.clone();
            model.geometry_revision = geometry_revision;
            model.scene = bootstrap_model.scene.clone();
            model.instance_body_poses = bootstrap_model.instance_body_poses.clone();
        }
        app.world_mut()
            .resource_mut::<ModelGeometryCache>()
            .0
            .insert(session_id.clone(), (geometry_revision, 0));
        for body in &bootstrap_model.scene.bodies {
            for face in &body.faces {
                app.world_mut().spawn((
                    NativeCadFace {
                        body_id: body.id.0,
                        occurrence_id: None,
                        face_id: face.id.0,
                        boundary: face_boundary_segments(body, face),
                    },
                    NativeModelGeometry {
                        session_id: session_id.clone(),
                        geometry_revision,
                        instance_revision: 0,
                    },
                    Visibility::Inherited,
                ));
            }
        }

        let mut runtime = MainThreadRenderRuntime {
            app,
            model: bootstrap_model.clone(),
            camera: ViewportCamera::default(),
            logical_size: (800.0, 600.0),
            scale_factor: 2.0,
            session_aliases: HashMap::new(),
        };
        let metrics = Arc::new(Mutex::new(MetricsState::default()));
        let mut dirty = false;
        apply_render_command(
            RenderCommand::RebindModelSession {
                from: session_id.clone(),
                to: "recovered-tab".to_string(),
            },
            &mut runtime,
            &metrics,
            &mut dirty,
        );

        assert!(dirty, "renaming resident GPU geometry must redraw once");
        assert_eq!(runtime.model.session_id, "recovered-tab");
        assert_eq!(
            runtime.app.world().resource::<ModelResource>().session_id,
            "recovered-tab"
        );
        let cache = &runtime.app.world().resource::<ModelGeometryCache>().0;
        assert_eq!(cache.get("recovered-tab"), Some(&(geometry_revision, 0)));
        assert!(!cache.contains_key(&session_id));
        let retained_faces = {
            let world = runtime.app.world_mut();
            let mut query = world.query::<(&NativeCadFace, &NativeModelGeometry)>();
            query
                .iter(world)
                .filter(|(_, geometry)| geometry.session_id == "recovered-tab")
                .count()
        };
        assert_eq!(retained_faces, face_count);

        // A native model snapshot can race with the bind command. Once the
        // bootstrap id has been rebound, a late snapshot must resolve to the
        // permanent tab instead of recreating the temporary session.
        dirty = false;
        apply_render_command(
            RenderCommand::Model(bootstrap_model),
            &mut runtime,
            &metrics,
            &mut dirty,
        );
        assert!(dirty);
        assert_eq!(runtime.model.session_id, "recovered-tab");
        assert_eq!(
            runtime.app.world().resource::<ModelResource>().session_id,
            "recovered-tab"
        );
        let faces_after_late_sync = {
            let world = runtime.app.world_mut();
            let mut query = world.query::<&NativeModelGeometry>();
            query
                .iter(world)
                .filter(|geometry| geometry.session_id == "recovered-tab")
                .count()
        };
        assert_eq!(faces_after_late_sync, face_count);
    }
}
