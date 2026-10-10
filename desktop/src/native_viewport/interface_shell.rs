//! Retained native controls on the existing application's Bevy surface.
//!
//! This is a renderer and input adapter, not a second document controller.
//! Human input and MCP resolve the same generational control key. The native
//! application reducer consumes the resulting action with its document owner.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use bevy::{
    prelude::*,
    text::FontWeight,
    ui::{CalculatedClip, ComputedStackIndex, UiGlobalTransform, UiSystems},
};
use limo_cad_interface::{
    Canvas, Control, ControlInput, ControlKey, ControlRequest, DocumentContext, Field, KeyChord,
    KeyboardRoute, Rect as InterfaceRect, ResolvedControl, Surface, SurfaceFrame, SurfaceRegistry,
};

use super::ui::{ViewportUiAssets, ViewportUiTheme};

pub(crate) mod fields;
mod geometry;
mod ime_diagnostics;
pub(crate) mod ranges;
pub(crate) mod ribbon;
mod shortcut_diagnostics;
mod window_focus;

use geometry::HitArea;

type InterfaceCallback = Arc<dyn Fn(&mut World, &NativeInterfaceHandle) + Send + Sync>;

type StyledControlQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static InterfaceControl,
        &'static InterfaceLabel,
        &'static InterfaceButtonStyle,
        Option<&'static ribbon::RibbonButton>,
        Option<&'static InterfaceCaption>,
        Option<&'static InterfaceFlat>,
        Option<&'static InterfaceReference>,
        Option<&'static PrimaryButton>,
        Option<&'static DimensionInk>,
        &'static mut Node,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
        Option<&'static mut Outline>,
    ),
>;

type RenderedControlQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        Ref<'static, InterfaceControl>,
        Ref<'static, ComputedNode>,
        Ref<'static, UiGlobalTransform>,
        Ref<'static, ComputedStackIndex>,
        Option<Ref<'static, CalculatedClip>>,
        Option<Ref<'static, InheritedVisibility>>,
        Option<&'static bevy::text::EditableText>,
        Option<Ref<'static, InterfaceTextRevision>>,
        Option<Ref<'static, InterfacePointerPassthrough>>,
        Option<Ref<'static, InterfaceCanvasAnnotation>>,
    ),
>;

type OccluderQuery<'w, 's> = Query<
    'w,
    's,
    (
        Ref<'static, ComputedNode>,
        Ref<'static, UiGlobalTransform>,
        Ref<'static, ComputedStackIndex>,
        Option<Ref<'static, CalculatedClip>>,
        Ref<'static, InheritedVisibility>,
        Option<Ref<'static, InterfaceCanvasOccluder>>,
    ),
    With<InterfaceOccluder>,
>;

const MAX_PENDING_ACTIONS: usize = 64;

/// Bevy node dimensions use UI units; Window dimensions already account for
/// monitor DPI but still need the application's interface-size preference.
pub(crate) fn window_ui_size(world: &mut World) -> Option<Vec2> {
    let scale = world
        .get_resource::<bevy::ui::UiScale>()
        .map_or(1., |scale| scale.0);
    let size = world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .single(world)
        .ok()
        .map(|window| Vec2::new(window.width(), window.height()));
    if let Some(size) = size.filter(|size| size.x > 0. && size.y > 0.) {
        return Some(size / scale);
    }
    let handle = world.get_resource::<NativeInterfaceHandle>()?;
    let client = handle.frame()?.client;
    Some(Vec2::new(client.width as f32, client.height as f32) * handle.presented_ui_scale() / scale)
}

/// The 3D rectangle remains the frame's `viewport` canvas; it is not inferred
/// from the full client size or duplicated in another UI taxonomy.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceFrame {
    pub context: DocumentContext,
    pub client: InterfaceRect,
    pub surface: InterfaceRect,
    pub canvases: Vec<Canvas>,
    pub surfaces: Vec<Surface>,
    pub modal_stack: Vec<String>,
    pub document_visible: bool,
}

impl InterfaceFrame {
    fn validate(&self) -> Result<(), String> {
        if !valid_rect(self.client) || !valid_rect(self.surface) {
            return Err("Native interface needs finite, positive client and surface bounds".into());
        }
        if !contains_rect(self.client, self.surface) {
            return Err("Native interface surface must fit inside the client area".into());
        }
        if self
            .canvases
            .iter()
            .any(|canvas| !valid_rect(canvas.bounds) || !contains_rect(self.client, canvas.bounds))
        {
            return Err("Native interface canvases must fit inside the client area".into());
        }
        Ok(())
    }

    /// Map physical native-child input to the logical application-window
    /// coordinates used by inspection. A scale change never rescales model data.
    pub fn physical_to_window(&self, point: [f64; 2], physical_size: [f64; 2]) -> Option<[f64; 2]> {
        if point
            .iter()
            .chain(physical_size.iter())
            .any(|v| !v.is_finite())
            || physical_size.iter().any(|v| *v <= 0.0)
        {
            return None;
        }
        Some([
            self.surface.x + point[0] * self.surface.width / physical_size[0],
            self.surface.y + point[1] * self.surface.height / physical_size[1],
        ])
    }
}

/// Metadata on an actual retained Bevy widget. Identity and hit bounds are
/// obtained from its entity and computed layout, never supplied by a client.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct InterfaceControl {
    /// Advance when this retained widget is rebound to another native action
    /// or target, even when its visible label does not change.
    pub binding: u64,
    pub surface: String,
    pub label: String,
    pub role: String,
    pub visible: bool,
    pub disabled: bool,
    pub expanded: Option<bool>,
    pub selected: Option<bool>,
    pub field: Field,
    pub modal_scope: Option<String>,
    pub text_editing: bool,
    pub owned_keys: Vec<KeyChord>,
}

/// The text adapter changes this only for buffer/selection edits. Camera or
/// font-layout updates must not clone long editor values into every frame.
#[derive(Component, Default)]
pub(crate) struct InterfaceTextRevision(pub u64);

/// Keep an annotation painted while the active geometry tool owns its clicks.
/// Its retained control remains inspectable, with activation disabled.
#[derive(Component)]
pub(crate) struct InterfacePointerPassthrough;

/// Canvas labels and glyphs retain their clicks while wheel and pinch gestures
/// navigate the camera beneath them. Panels and text editors remain blockers.
#[derive(Component)]
pub(crate) struct InterfaceCanvasAnnotation;

/// A painted panel blocks model picking without inventing an actionable
/// control for its background. Its children retain normal control semantics.
#[derive(Component, Clone, Default)]
pub(crate) struct InterfaceOccluder;

/// Painted canvas content still blocks model picking, but accepts input owned
/// by that canvas. Controls and panels above it retain their normal ownership.
#[derive(Component, Clone, Copy)]
pub(crate) struct InterfaceCanvasOccluder(pub &'static str);

#[derive(Clone, Copy)]
enum HitTarget {
    Control(ControlKey),
    Annotation(ControlKey),
    Canvas(&'static str),
    Occluder,
}

impl InterfaceControl {
    pub fn button(surface: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            binding: 1,
            surface: surface.into(),
            label: label.into(),
            role: "button".into(),
            visible: true,
            disabled: false,
            expanded: None,
            selected: None,
            field: Field::None,
            modal_scope: None,
            text_editing: false,
            owned_keys: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeInterfaceAction {
    pub context: DocumentContext,
    pub control: ResolvedControl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeModalKey {
    pub context: DocumentContext,
    pub modal_scope: String,
    pub key: KeyChord,
    generation: u64,
    controls: Vec<(ControlKey, u64)>,
}

/// Layout is usable even while minimized. Submission only records completion
/// of a host render update; it does not certify GPU presentation or visibility.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderReceipt {
    pub laid_out_revision: u64,
    pub submitted_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerPhase {
    Move,
    Down,
    Up,
    DoubleClick,
    Leave,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Secondary,
}

#[derive(Debug, Clone)]
struct Capture {
    resolved: ResolvedControl,
    context: DocumentContext,
    button: PointerButton,
}

struct Shared {
    ime_diagnostics: Option<ime_diagnostics::Trace>,
    shortcut_diagnostics: Option<shortcut_diagnostics::Trace>,
    registry: SurfaceRegistry,
    desired_frame: Option<InterfaceFrame>,
    presented_frame: Option<InterfaceFrame>,
    presented_ui_scale: f32,
    hit_order: Vec<(HitTarget, HitArea)>,
    receipt: RenderReceipt,
    submission_waiter: Option<u64>,
    render_dirty: bool,
    modal_generation: u64,
    modal_focus: Vec<(String, Option<ControlKey>)>,
    hovered: Option<ControlKey>,
    focused: Option<ControlKey>,
    window_focus_return: Option<window_focus::ReturnTarget>,
    tab_excluded: std::collections::HashSet<ControlKey>,
    capture: Option<Capture>,
    actions: VecDeque<NativeInterfaceAction>,
    modal_keys: VecDeque<NativeModalKey>,
    revision: u64,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            ime_diagnostics: ime_diagnostics::Trace::opt_in(),
            shortcut_diagnostics: shortcut_diagnostics::Trace::opt_in(),
            registry: SurfaceRegistry::new(),
            desired_frame: None,
            presented_frame: None,
            presented_ui_scale: 1.,
            hit_order: Vec::new(),
            receipt: RenderReceipt::default(),
            submission_waiter: None,
            render_dirty: false,
            modal_generation: 0,
            modal_focus: Vec::new(),
            hovered: None,
            focused: None,
            window_focus_return: None,
            tab_excluded: Default::default(),
            capture: None,
            actions: VecDeque::new(),
            modal_keys: VecDeque::new(),
            revision: 0,
        }
    }
}

/// Shared transport/input endpoint. It never owns a second SketchManager and
/// never dispatches commands through DOM nodes. `wake` schedules the existing
/// event-driven renderer after presentation or input changes.
#[derive(Resource, Clone)]
pub struct NativeInterfaceHandle {
    shared: Arc<Mutex<Shared>>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl NativeInterfaceHandle {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared::default())),
            wake: Arc::new(wake),
        }
    }

    pub fn present(&self, frame: InterfaceFrame) -> Result<(), String> {
        frame.validate()?;
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        if shared.desired_frame.as_ref() == Some(&frame) {
            return Ok(());
        }
        let old_modals = shared
            .desired_frame
            .as_ref()
            .map(|old| old.modal_stack.clone())
            .unwrap_or_default();
        if old_modals != frame.modal_stack {
            shared.modal_generation = shared.modal_generation.saturating_add(1);
        }
        if shared.desired_frame.as_ref().map(|f| &f.context) != Some(&frame.context) {
            shared.capture = None;
            shared.focused = None;
            shared.window_focus_return = None;
            shared.hovered = None;
            shared.actions.clear();
            shared.modal_keys.clear();
            shared.modal_focus.clear();
        } else {
            let common = old_modals
                .iter()
                .zip(&frame.modal_stack)
                .take_while(|(old, next)| old == next)
                .count();
            while shared.modal_focus.len() > common {
                if let Some((_, previous)) = shared.modal_focus.pop() {
                    shared.focused = previous;
                }
            }
            for scope in frame.modal_stack.iter().skip(common) {
                let previous = shared.focused;
                shared.modal_focus.push((scope.clone(), previous));
                shared.focused = None;
            }
        }
        shared.desired_frame = Some(frame);
        shared.revision = shared.revision.wrapping_add(1);
        drop(shared);
        (self.wake)();
        Ok(())
    }

    pub(crate) fn record_file_shortcut(
        &self,
        event: &crate::native_viewport::winit_host::NativeHostInput,
        decision: &'static str,
    ) {
        if let Ok(mut shared) = self.shared.lock() {
            if let Some(trace) = &mut shared.shortcut_diagnostics {
                trace.record(event, decision);
            }
        }
    }

    pub fn inspect(&self) -> Result<serde_json::Value, String> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        let context = current_context(&shared)?;
        let snapshot = shared.registry.inspect().map_err(|e| e.to_string())?;
        let snapshot = {
            let mut snapshot = snapshot;
            // Inspection IDs expire on every inspect. This read-only identity
            // lets an input harness retain the intended widget across those
            // observations without turning its key into an activation target.
            snapshot["focused_binding"] = shared
                .registry
                .frame()
                .controls
                .iter()
                .find(|control| {
                    snapshot["focused_control"].is_string()
                        && Some(control.key) == shared.registry.frame().focused
                })
                .map(|control| {
                    serde_json::json!({"control_key":control.key.0,
                    "binding":control.binding,"context":{"window_id":context.window_id,
                        "document_id":context.document_id,"epoch":context.epoch}})
                })
                .unwrap_or(serde_json::Value::Null);
            if let Some(trace) = &shared.shortcut_diagnostics {
                snapshot["file_shortcut_diagnostics"] = trace.snapshot();
            }
            if let Some(trace) = &shared.ime_diagnostics {
                snapshot["ime_diagnostics"] = trace.snapshot();
                snapshot["ime_diagnostics"]["focus"] = shared.registry.frame().controls.iter()
                    .find(|control| Some(control.key) == shared.focused)
                    .map(|control| serde_json::json!({"control_key":control.key.0,
                        "binding":control.binding, "label":control.label,
                        "context":shared.presented_frame.as_ref().map(|frame| ime_diagnostics::owner_snapshot(&frame.context))}))
                    .unwrap_or(serde_json::Value::Null);
            }
            snapshot
        };
        Ok(snapshot)
    }

    /// Read native presentation metadata without copying editor buffers or
    /// consuming MCP inspection IDs. The callback must not re-enter the handle.
    pub(crate) fn read_surface<T>(
        &self,
        read: impl FnOnce(&DocumentContext, &SurfaceFrame) -> T,
    ) -> Result<T, String> {
        let shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        let context = current_context(&shared)?;
        Ok(read(&context, shared.registry.frame()))
    }

    /// Assistive input uses the same stamped control as pointer and MCP input.
    /// A queued accessibility request cannot follow a control rebind or tab.
    pub(crate) fn assistive_action(
        &self,
        action: &NativeInterfaceAction,
        activate: bool,
    ) -> Result<(), String> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        if current_context(&shared)? != action.context {
            return Err("Native interface document changed".into());
        }
        shared
            .registry
            .validate_resolved(&action.control, &action.context)
            .map_err(|error| error.to_string())?;
        if activate && shared.actions.len() >= MAX_PENDING_ACTIONS {
            return Err("Native interface is busy".into());
        }
        set_focus(&mut shared, Some(action.control.key))?;
        if activate {
            shared.actions.push_back(action.clone());
        }
        shared.revision = shared.revision.wrapping_add(1);
        drop(shared);
        (self.wake)();
        Ok(())
    }

    pub(crate) fn assistive_edit(
        &self,
        original: &NativeInterfaceAction,
        input: ControlInput,
    ) -> Result<(), String> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        if current_context(&shared)? != original.context {
            return Err("Native interface document changed".into());
        }
        shared
            .registry
            .validate_resolved(&original.control, &original.context)
            .map_err(|e| e.to_string())?;
        enqueue(&mut shared, original.control.key, input, &original.context)?;
        set_focus(&mut shared, Some(original.control.key))?;
        drop(shared);
        (self.wake)();
        Ok(())
    }

    pub(crate) fn resolve_retained(
        &self,
        key: ControlKey,
    ) -> Result<NativeInterfaceAction, String> {
        let shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        let context = current_context(&shared)?;
        let control = shared
            .registry
            .resolve_key(key, ControlInput::Click, &context)
            .map_err(|error| error.to_string())?;
        Ok(NativeInterfaceAction { context, control })
    }

    /// The MCP path returns the exact same owned action as human input. The
    /// caller applies it through the native reducer and returns its real result.
    pub fn resolve(
        &self,
        request: &ControlRequest,
        context: &DocumentContext,
    ) -> Result<NativeInterfaceAction, String> {
        let shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        if current_context(&shared)? != *context {
            return Err("Native interface document changed".into());
        }
        let control = shared
            .registry
            .resolve(request, context)
            .map_err(|e| e.to_string())?;
        Ok(NativeInterfaceAction {
            context: context.clone(),
            control,
        })
    }

    /// Revalidate queued human input immediately before applying it. Removed,
    /// disabled, covered or replaced controls are rejected by the same registry.
    pub fn validate_action(&self, action: &NativeInterfaceAction) -> Result<(), String> {
        let shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        if current_context(&shared)? != action.context {
            return Err("Native interface document changed".into());
        }
        shared
            .registry
            .validate_resolved(&action.control, &action.context)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// An already resolved worker action may race its own footer repaint.
    /// Only status text may be pending; owner, layout, every other surface,
    /// visibility, modal ownership and the original control stamp stay strict.
    pub(crate) fn validate_dispatch_action(
        &self,
        action: &NativeInterfaceAction,
    ) -> Result<(), String> {
        let shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        let desired = shared
            .desired_frame
            .as_ref()
            .ok_or("Native interface has no document")?;
        let presented = shared
            .presented_frame
            .as_ref()
            .ok_or("Native interface has not been laid out")?;
        if desired.context != action.context || presented.context != action.context {
            return Err("Native interface document changed".into());
        }
        if desired.client != presented.client
            || desired.surface != presented.surface
            || desired.canvases != presented.canvases
            || desired.modal_stack != presented.modal_stack
            || desired.document_visible != presented.document_visible
            || desired.surfaces.len() != presented.surfaces.len()
            || desired
                .surfaces
                .iter()
                .zip(&presented.surfaces)
                .any(|(new, old)| {
                    new.name != old.name || (new.name != "document/status" && new.text != old.text)
                })
        {
            return Err("Native interface transition is pending".into());
        }
        shared
            .registry
            .validate_resolved(&action.control, &action.context)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Give a resolved direct/API action the same focus transition as a native
    /// press. The caller still performs the one authoritative reduction; this
    /// method does not enqueue or run the command a second time.
    pub(crate) fn prepare_activation(&self, action: &NativeInterfaceAction) -> Result<(), String> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        if current_context(&shared)? != action.context {
            return Err("Native interface document changed".into());
        }
        shared
            .registry
            .validate_resolved(&action.control, &action.context)
            .map_err(|error| error.to_string())?;
        set_focus(&mut shared, Some(action.control.key))?;
        shared.revision = shared.revision.wrapping_add(1);
        drop(shared);
        (self.wake)();
        Ok(())
    }

    pub fn take_actions(&self) -> Result<Vec<NativeInterfaceAction>, String> {
        Ok(self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?
            .actions
            .drain(..)
            .collect())
    }

    pub(crate) fn take_next_action(&self) -> Result<Option<NativeInterfaceAction>, String> {
        Ok(self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?
            .actions
            .pop_front())
    }

    pub(crate) fn resolve_input(
        &self,
        key: ControlKey,
        input: ControlInput,
        context: &DocumentContext,
    ) -> Result<NativeInterfaceAction, String> {
        let shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        if current_context(&shared)? != *context {
            return Err("Native interface document changed".into());
        }
        let control = shared
            .registry
            .resolve_key(key, input, context)
            .map_err(|e| e.to_string())?;
        Ok(NativeInterfaceAction {
            context: context.clone(),
            control,
        })
    }

    pub(crate) fn enqueue_action(&self, action: NativeInterfaceAction) -> Result<(), String> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        if current_context(&shared)? != action.context {
            return Err("Native interface document changed".into());
        }
        shared
            .registry
            .validate_resolved(&action.control, &action.context)
            .map_err(|e| e.to_string())?;
        if shared.actions.len() >= MAX_PENDING_ACTIONS {
            return Err("Native interface is busy".into());
        }
        shared.actions.push_back(action);
        drop(shared);
        (self.wake)();
        Ok(())
    }

    /// UI scale belonging to the published layout while queued preferences
    /// wait for Bevy to lay out the next frame.
    pub(crate) fn presented_ui_scale(&self) -> f32 {
        self.shared
            .lock()
            .map_or(1., |shared| shared.presented_ui_scale)
    }

    pub(crate) fn focused_key(&self) -> Option<ControlKey> {
        self.shared.lock().ok()?.focused
    }

    pub(crate) fn hit_key(&self, point: [f64; 2]) -> Option<ControlKey> {
        let shared = self.shared.lock().ok()?;
        hit(&shared, point)
    }

    pub(crate) fn owns_pointer(&self, point: [f64; 2]) -> bool {
        self.shared.lock().is_ok_and(|shared| {
            shared
                .hit_order
                .iter()
                .any(|(_, area)| area.contains(point))
        })
    }

    /// Wheel and pinch may cross annotations, but never other rendered controls
    /// or painted panels. Click hit testing continues to include annotations.
    pub(crate) fn blocks_camera_gesture(&self, point: [f64; 2]) -> bool {
        self.shared.lock().is_ok_and(|shared| {
            shared.hit_order.iter().any(|(target, area)| {
                !matches!(target, HitTarget::Annotation(_)) && area.contains(point)
            })
        })
    }

    pub(crate) fn canvas_owns_pointer(&self, canvas: &str, point: [f64; 2]) -> bool {
        self.shared.lock().is_ok_and(|shared| {
            shared
                .hit_order
                .iter()
                .rev()
                .find(|(_, area)| area.contains(point))
                .is_some_and(
                    |(target, _)| matches!(target, HitTarget::Canvas(owner) if *owner == canvas),
                )
        })
    }

    pub fn take_modal_keys(&self) -> Result<Vec<NativeModalKey>, String> {
        Ok(self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?
            .modal_keys
            .drain(..)
            .collect())
    }

    pub fn validate_modal_key(&self, request: &NativeModalKey) -> Result<(), String> {
        let shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        if current_context(&shared)? != request.context
            || shared.modal_generation != request.generation
            || shared.registry.frame().modal_stack.last() != Some(&request.modal_scope)
            || modal_controls(&shared, &request.modal_scope) != request.controls
        {
            return Err("Native modal changed before keyboard input could run".into());
        }
        Ok(())
    }

    pub(crate) fn request_redraw(&self) {
        (self.wake)();
    }

    pub(crate) fn invalidate_presentation(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.render_dirty = true;
        }
        (self.wake)();
    }

    /// Called after the host submitted this Bevy update. This is a render
    /// submission receipt, not a claim that the GPU/display has presented it.
    #[cfg(test)]
    pub(crate) fn submitted(&self) -> Result<(), String> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        shared.receipt.submitted_revision = shared.receipt.laid_out_revision;
        Ok(())
    }

    pub(crate) fn submitted_revision(&self, revision: u64) -> Result<(), String> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        if revision > shared.receipt.laid_out_revision {
            return Err("Render submitted an unknown interface revision".into());
        }
        shared.receipt.submitted_revision = shared.receipt.submitted_revision.max(revision);
        let wake = shared
            .submission_waiter
            .is_some_and(|target| target <= revision);
        if wake {
            shared.submission_waiter = None;
        }
        drop(shared);
        if wake {
            (self.wake)();
        }
        Ok(())
    }

    pub(crate) fn wait_for_submission(&self, revision: u64) -> bool {
        let Ok(mut shared) = self.shared.lock() else {
            return false;
        };
        if shared.receipt.submitted_revision >= revision {
            return true;
        }
        shared.submission_waiter = Some(revision);
        false
    }

    pub(crate) fn cancel_submission_wait(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.submission_waiter = None;
        }
    }

    pub fn render_receipt(&self) -> Option<RenderReceipt> {
        Some(self.shared.lock().ok()?.receipt)
    }

    pub fn frame(&self) -> Option<InterfaceFrame> {
        self.shared.lock().ok()?.presented_frame.clone()
    }

    /// Stamp ordered OS input with exactly the same laid-out owner as `frame`,
    /// without copying canvas metadata or potentially large surface text.
    /// A pending transition must not stamp input with its desired owner.
    pub(crate) fn presented_context(&self) -> Option<DocumentContext> {
        self.shared
            .lock()
            .ok()?
            .presented_frame
            .as_ref()
            .map(|frame| frame.context.clone())
    }

    pub fn has_capture(&self) -> bool {
        self.shared
            .lock()
            .is_ok_and(|shared| shared.capture.is_some())
    }

    /// Cancellation never activates the captured control, even while a newer
    /// semantic frame is waiting for layout.
    pub(crate) fn cancel_pointer(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            let changed = shared.capture.take().is_some() || shared.hovered.is_some();
            shared.hovered = None;
            if changed {
                shared.revision = shared.revision.wrapping_add(1);
                drop(shared);
                (self.wake)();
            }
        }
    }

    /// Returns true only when the native interface owns this gesture. A press
    /// captures through release/cancel, even after leaving the original bounds.
    pub fn pointer(
        &self,
        phase: PointerPhase,
        position: [f64; 2],
        button: PointerButton,
    ) -> Result<bool, String> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        let context = match current_context(&shared) {
            Ok(context) => context,
            Err(_) => {
                return Ok(hit(&shared, position).is_some()
                    || shared.capture.is_some()
                    || has_modal(&shared));
            }
        };
        if position.iter().any(|v| !v.is_finite()) {
            return Err("Pointer coordinates must be finite".into());
        }
        let key = hit(&shared, position);
        let had_capture = shared.capture.is_some();
        let old_hovered = shared.hovered;
        let old_focused = shared.focused;
        let consumed = had_capture
            || shared
                .hit_order
                .iter()
                .any(|(_, area)| area.contains(position))
            || has_modal(&shared);
        match phase {
            PointerPhase::Move => {
                shared.hovered = key;
                if let Some(capture) = shared
                    .capture
                    .clone()
                    .filter(|c| c.button == PointerButton::Primary)
                {
                    enqueue_range(&mut shared, &capture, &context, position[0])?;
                }
            }
            PointerPhase::Leave => shared.hovered = None,
            PointerPhase::Cancel => {
                shared.hovered = None;
                shared.capture = None;
            }
            PointerPhase::Down => {
                shared.capture = None;
                if let Some(key) = key {
                    let resolved = shared
                        .registry
                        .resolve_key(key, activation(button), &context)
                        .map_err(|e| e.to_string())?;
                    set_focus(&mut shared, Some(key))?;
                    shared.capture = Some(Capture {
                        resolved,
                        context: context.clone(),
                        button,
                    });
                    if button == PointerButton::Primary {
                        let capture = shared.capture.clone().unwrap();
                        enqueue_range(&mut shared, &capture, &context, position[0])?;
                    }
                } else if button == PointerButton::Primary
                    && !consumed
                    && shared.focused.is_some()
                    && shared.presented_frame.as_ref().is_some_and(|frame| {
                        frame.context == context
                            && frame.canvases.iter().any(|canvas| {
                                let bounds = canvas.bounds;
                                canvas.name == "viewport"
                                    && position[0] >= bounds.x
                                    && position[0] < bounds.x + bounds.width
                                    && position[1] >= bounds.y
                                    && position[1] < bounds.y + bounds.height
                            })
                    })
                {
                    // The canvas owns subsequent keys after a real viewport
                    // press; retain button focus for keyboard-only navigation.
                    set_focus(&mut shared, None)?;
                }
            }
            PointerPhase::Up => {
                if let Some(capture) = shared.capture.take() {
                    let range = capture.button == PointerButton::Primary
                        && enqueue_range(&mut shared, &capture, &context, position[0])?;
                    if !range
                        && capture.context == context
                        && capture.button == button
                        && Some(capture.resolved.key) == key
                    {
                        shared
                            .registry
                            .validate_resolved(&capture.resolved, &context)
                            .map_err(|e| e.to_string())?;
                        if shared.actions.len() >= MAX_PENDING_ACTIONS {
                            return Err("Native interface is busy".into());
                        }
                        shared.actions.push_back(NativeInterfaceAction {
                            context,
                            control: capture.resolved,
                        });
                    }
                }
            }
            PointerPhase::DoubleClick => {
                shared.capture = None;
                if let Some(key) = key {
                    enqueue(&mut shared, key, ControlInput::DoubleClick, &context)?;
                }
            }
        }
        shared.revision = shared.revision.wrapping_add(1);
        let changed_hover = old_hovered != shared.hovered;
        let changed_focus = old_focused != shared.focused;
        drop(shared);
        if consumed || changed_hover || changed_focus {
            (self.wake)();
        }
        Ok(consumed)
    }

    pub fn key(&self, key: KeyChord) -> Result<bool, String> {
        if self.navigate_menu(&key)? {
            return Ok(true);
        }
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        let context = match current_context(&shared) {
            Ok(context) => context,
            Err(_) => return Ok(has_modal(&shared)),
        };
        if let Some(modal_scope) = shared
            .registry
            .frame()
            .modal_stack
            .last()
            .cloned()
            .filter(|_| key.key == "Escape" || shared.focused.is_none())
        {
            if shared.modal_keys.len() >= MAX_PENDING_ACTIONS {
                return Err("Native interface is busy".into());
            }
            let generation = shared.modal_generation;
            let controls = modal_controls(&shared, &modal_scope);
            shared.modal_keys.push_back(NativeModalKey {
                context,
                modal_scope,
                key,
                generation,
                controls,
            });
        } else if let Some(control) = shared.focused {
            if shared
                .registry
                .keyboard_route(&context, &key)
                .map_err(|e| e.to_string())?
                == KeyboardRoute::Model
            {
                return Ok(false);
            }
            enqueue(&mut shared, control, ControlInput::Key(key), &context)?;
        } else {
            return Ok(false);
        }
        drop(shared);
        (self.wake)();
        Ok(true)
    }

    pub(crate) fn exclude_from_tab(&self, key: ControlKey) -> Result<(), String> {
        self.shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?
            .tab_excluded
            .insert(key);
        Ok(())
    }

    pub fn focus_next(&self, backwards: bool) -> Result<bool, String> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        let context = match current_context(&shared) {
            Ok(context) => context,
            Err(_) => return Ok(has_modal(&shared)),
        };
        let keys: Vec<_> = shared
            .registry
            .frame()
            .controls
            .iter()
            .filter(|control| {
                !shared.tab_excluded.contains(&control.key)
                    && shared
                        .registry
                        .resolve_key(control.key, ControlInput::Click, &context)
                        .is_ok()
            })
            .map(|control| control.key)
            .collect();
        if keys.is_empty() {
            return Ok(has_modal(&shared));
        }
        let index = shared
            .focused
            .and_then(|key| keys.iter().position(|candidate| *candidate == key));
        let next = match (index, backwards) {
            (Some(index), false) => (index + 1) % keys.len(),
            (Some(index), true) => (index + keys.len() - 1) % keys.len(),
            (None, false) => 0,
            (None, true) => keys.len() - 1,
        };
        set_focus(&mut shared, Some(keys[next]))?;
        shared.revision = shared.revision.wrapping_add(1);
        drop(shared);
        (self.wake)();
        Ok(true)
    }

    /// Menus traverse their enabled entries, excluding the dismissal backdrop.
    /// Called for physical keys and resolved MCP keys after the same owner guard.
    pub(crate) fn navigate_menu(&self, key: &KeyChord) -> Result<bool, String> {
        if key.ctrl
            || key.meta
            || key.alt
            || key.shift
            || !matches!(key.key.as_str(), "ArrowUp" | "ArrowDown" | "Home" | "End")
        {
            return Ok(false);
        }
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?;
        let Ok(context) = current_context(&shared) else {
            return Ok(false);
        };
        let Some(scope) = shared.registry.frame().modal_stack.last() else {
            return Ok(false);
        };
        let keys: Vec<_> = shared
            .registry
            .frame()
            .controls
            .iter()
            .filter(|c| c.role == "menuitem" && c.modal_scope.as_ref() == Some(scope))
            .filter(|c| {
                shared
                    .registry
                    .resolve_key(c.key, ControlInput::Click, &context)
                    .is_ok()
            })
            .map(|c| c.key)
            .collect();
        if keys.is_empty() {
            return Ok(false);
        }
        let current = shared
            .focused
            .and_then(|key| keys.iter().position(|candidate| *candidate == key));
        let next = match (key.key.as_str(), current) {
            ("Home", _) | ("ArrowDown", None) => 0,
            ("End", _) | ("ArrowUp", None) => keys.len() - 1,
            ("ArrowDown", Some(index)) => (index + 1) % keys.len(),
            ("ArrowUp", Some(index)) => (index + keys.len() - 1) % keys.len(),
            _ => unreachable!(),
        };
        set_focus(&mut shared, Some(keys[next]))?;
        shared.revision = shared.revision.wrapping_add(1);
        drop(shared);
        (self.wake)();
        Ok(true)
    }

    pub fn blur(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.window_focus_return = None;
            shared.capture = None;
            let _ = set_focus(&mut shared, None);
            shared.hovered = None;
            shared.revision = shared.revision.wrapping_add(1);
        }
        (self.wake)();
    }
}

fn modal_controls(shared: &Shared, scope: &str) -> Vec<(ControlKey, u64)> {
    let mut controls: Vec<_> = shared
        .registry
        .frame()
        .controls
        .iter()
        .filter(|control| control.modal_scope.as_deref() == Some(scope))
        .map(|control| (control.key, control.binding))
        .collect();
    controls.sort_by_key(|(key, _)| key.0);
    controls
}

fn set_focus(shared: &mut Shared, focused: Option<ControlKey>) -> Result<(), String> {
    let context = shared
        .registry
        .context()
        .cloned()
        .ok_or("Native interface has no document")?;
    let mut frame = shared.registry.frame().clone();
    frame.focused = focused;
    shared
        .registry
        .replace(context, frame)
        .map_err(|e| e.to_string())?;
    shared.focused = focused;
    Ok(())
}

fn current_context(shared: &Shared) -> Result<DocumentContext, String> {
    let desired = shared
        .desired_frame
        .as_ref()
        .ok_or("Native interface has no document")?;
    let presented = shared
        .presented_frame
        .as_ref()
        .ok_or("Native interface has not been laid out")?;
    if desired != presented {
        return Err("Native interface transition is pending".into());
    }
    Ok(presented.context.clone())
}

fn has_modal(shared: &Shared) -> bool {
    shared
        .desired_frame
        .as_ref()
        .is_some_and(|frame| !frame.modal_stack.is_empty())
        || !shared.registry.frame().modal_stack.is_empty()
}

fn activation(button: PointerButton) -> ControlInput {
    match button {
        PointerButton::Primary => ControlInput::Click,
        PointerButton::Secondary => ControlInput::ContextMenu,
    }
}

fn enqueue(
    shared: &mut Shared,
    key: ControlKey,
    input: ControlInput,
    context: &DocumentContext,
) -> Result<(), String> {
    if shared.actions.len() >= MAX_PENDING_ACTIONS {
        return Err("Native interface is busy; wait for the pending command".into());
    }
    let control = shared
        .registry
        .resolve_key(key, input, context)
        .map_err(|e| e.to_string())?;
    shared.actions.push_back(NativeInterfaceAction {
        context: context.clone(),
        control,
    });
    Ok(())
}

/// Pointer drags retain their original binding/owner. Coalesce only adjacent
/// values for that same slider, preserving intervening commands and avoiding a
/// backlog of obsolete preview poses while the render/kernel worker is busy.
fn enqueue_range(
    shared: &mut Shared,
    capture: &Capture,
    context: &DocumentContext,
    x: f64,
) -> Result<bool, String> {
    let Some(control) = shared
        .registry
        .frame()
        .controls
        .iter()
        .find(|c| c.key == capture.resolved.key)
    else {
        return Ok(false);
    };
    let Some(value) = ranges::pointer_value(control, x) else {
        return Ok(false);
    };
    if capture.context != *context {
        return Err("Slider belongs to an earlier document".into());
    }
    let validate = shared
        .registry
        .validate_resolved(&capture.resolved, context);
    if let Err(error) = validate {
        if error != limo_cad_interface::ControlError::Disabled {
            return Err(error.to_string());
        }
    }
    if let Some(last) = shared.actions.back().filter(|a| {
        a.context == *context
            && a.control.key == capture.resolved.key
            && a.control.binding() == capture.resolved.binding()
            && matches!(a.control.input, ControlInput::SetValue(_))
    }) {
        if let Err(error) = shared.registry.validate_resolved(&last.control, context) {
            if error != limo_cad_interface::ControlError::Disabled {
                return Err(error.to_string());
            }
        }
        shared.actions.pop_back();
    }
    if shared.actions.len() >= MAX_PENDING_ACTIONS {
        return Err("Native interface is busy; wait for the pending command".into());
    }
    let mut resolved = capture.resolved.clone();
    resolved.input = ControlInput::SetValue(value.to_string());
    shared.actions.push_back(NativeInterfaceAction {
        context: context.clone(),
        control: resolved,
    });
    Ok(true)
}

fn hit(shared: &Shared, point: [f64; 2]) -> Option<ControlKey> {
    shared
        .hit_order
        .iter()
        .rev()
        .find(|(_, area)| area.contains(point))
        .and_then(|(target, _)| match target {
            HitTarget::Control(key) | HitTarget::Annotation(key) => Some(*key),
            HitTarget::Canvas(_) | HitTarget::Occluder => None,
        })
}

fn valid_rect(rect: InterfaceRect) -> bool {
    [
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        rect.x + rect.width,
        rect.y + rect.height,
    ]
    .iter()
    .all(|v| v.is_finite())
        && rect.width > 0.0
        && rect.height > 0.0
}

fn contains_point(rect: InterfaceRect, point: [f64; 2]) -> bool {
    point[0] >= rect.x
        && point[0] < rect.x + rect.width
        && point[1] >= rect.y
        && point[1] < rect.y + rect.height
}

fn contains_rect(outer: InterfaceRect, inner: InterfaceRect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

#[derive(Component)]
struct InterfaceLabel(Entity);

#[derive(Component)]
struct InterfaceButtonStyle(ViewportUiTheme);

/// Theme changes repaint retained controls and their native editors. Semantic
/// keys, focus and unfinished text survive the change.
pub(crate) fn refresh_theme(world: &mut World, theme: ViewportUiTheme) {
    let mut controls = world.query::<(
        &mut InterfaceButtonStyle,
        Option<&PrimaryButton>,
        Option<&DestructiveButton>,
        Option<&InterfaceReference>,
        Option<&DimensionInk>,
    )>();
    for (mut style, primary, destructive, reference, dimension) in controls.iter_mut(world) {
        if destructive.is_some() {
            continue;
        }
        style.0 = theme;
        if primary.is_some() {
            style.0.panel = theme.accent;
            style.0.hover = ribbon::css_mix(Color::WHITE, theme.accent, 0.12);
            style.0.accent_soft = theme.accent;
            style.0.ink = Color::WHITE;
            style.0.accent = Color::WHITE;
            style.0.edge = theme.accent;
        } else if reference.is_some() {
            style.0.panel = theme.header;
            style.0.accent_soft = ribbon::css_mix(theme.accent, theme.panel, 0.15);
        }
        if let Some(dimension) = dimension {
            style.0.ink = dimension.0;
            style.0.accent = dimension.0;
        }
    }
    fields::refresh_theme(world, theme);
    ranges::refresh_theme(world, theme);
}

#[derive(Component)]
struct DimensionInk(Color);

/// Create a genuine retained button; root attaches its typed native command to
/// the returned entity. Updating `InterfaceControl` changes this same widget.
#[derive(Component, PartialEq)]
pub(crate) struct InterfaceCaption(pub String);

/// Compact rows and menu items share focus/selection behavior with buttons,
/// while leaving the enclosing panel visible behind their idle state.
#[derive(Component)]
pub(crate) struct InterfaceFlat;

#[derive(Component)]
struct InterfaceReference;

pub(crate) fn compact_label(world: &mut World, entity: Entity, inset: f32) {
    let label = world.get::<InterfaceLabel>(entity).unwrap().0;
    let assets = world.resource::<ViewportUiAssets>().clone();
    let theme = world.get::<InterfaceButtonStyle>(entity).unwrap().0;
    world.entity_mut(label).insert((
        theme.text(&assets, 12., FontWeight::NORMAL),
        TextLayout::no_wrap(),
        Node {
            margin: UiRect::left(px(inset)),
            ..default()
        },
    ));
    world.entity_mut(entity).insert(InterfaceFlat);
}

#[derive(Component)]
struct CaptionClip(Entity);

/// Clip a single-line caption while retaining the control's full accessible name
/// and hit area. Bevy clips descendants, so the text needs its own container.
pub(crate) fn clip_caption(world: &mut World, entity: Entity, inset: f32, trailing: f32) {
    let label = world.get::<InterfaceLabel>(entity).unwrap().0;
    let clip = if let Some(clip) = world.get::<CaptionClip>(entity) {
        clip.0
    } else {
        let clip = world.spawn_empty().id();
        world
            .entity_mut(entity)
            .add_child(clip)
            .insert(CaptionClip(clip));
        world.entity_mut(clip).add_child(label);
        clip
    };
    caption_node(
        world,
        entity,
        Node {
            min_width: px(0.),
            flex_shrink: 0.,
            ..default()
        },
    );
    let layout = TextLayout::no_wrap();
    if world.get::<TextLayout>(label).is_none_or(|current| {
        current.justify != layout.justify || current.linebreak != layout.linebreak
    }) {
        world.entity_mut(label).insert(layout);
    }
    let bounds = Node {
        position_type: PositionType::Absolute,
        left: px(inset),
        right: px(trailing),
        top: px(0.),
        bottom: px(0.),
        min_width: px(0.),
        align_items: AlignItems::Center,
        overflow: Overflow::clip(),
        ..default()
    };
    if world.get::<Node>(clip) != Some(&bounds) {
        world.entity_mut(clip).insert(bounds);
    }
}

pub(crate) fn caption_node(world: &mut World, entity: Entity, node: Node) {
    if let Some(label) = world.get::<InterfaceLabel>(entity).map(|label| label.0) {
        if world.get::<Node>(label) != Some(&node) {
            world.entity_mut(label).insert(node);
        }
    }
}

pub(crate) fn caption_size(world: &mut World, entity: Entity, size: f32) {
    let size = bevy::text::FontSize::Px(size);
    let Some(label) = world.get::<InterfaceLabel>(entity).map(|label| label.0) else {
        return;
    };
    if let Some(mut font) = world.get_mut::<TextFont>(label) {
        if font.font_size != size {
            font.font_size = size;
        }
    }
}

pub(crate) fn caption_tracking(world: &mut World, entity: Entity, spacing: f32) {
    if let Some(label) = world.get::<InterfaceLabel>(entity).map(|label| label.0) {
        world
            .entity_mut(label)
            .insert(bevy::text::LetterSpacing::Px(spacing));
    }
}

pub(crate) fn caption_weight(world: &mut World, entity: Entity, weight: FontWeight) {
    if let Some(label) = world.get::<InterfaceLabel>(entity).map(|label| label.0) {
        if let Some(mut font) = world.get_mut::<TextFont>(label) {
            font.weight = weight;
        }
    }
}

/// Center the caption in the control's real bounds. A left inset on a flex
/// child shifts its visual center even when the parent is center-aligned.
pub(crate) fn center_caption(world: &mut World, entity: Entity) {
    let Some(label) = world.get::<InterfaceLabel>(entity).map(|label| label.0) else {
        return;
    };
    let bounds = Node {
        max_width: percent(100.),
        margin: UiRect::ZERO,
        ..default()
    };
    if world.get::<Node>(label) != Some(&bounds) {
        world
            .entity_mut(label)
            .insert((bounds, TextLayout::justify(Justify::Center)));
    }
}

pub(crate) fn tab_style(world: &mut World, entity: Entity) {
    if let Some(mut style) = world.get_mut::<InterfaceButtonStyle>(entity) {
        style.0.accent_soft = style.0.panel;
    }
    world.entity_mut(entity).remove::<InterfaceFlat>();
}

/// Shared state styling for tabs and history chips without altering their
/// semantic names, actions or input ownership.
pub(crate) fn control_colors(world: &mut World, entity: Entity, ink: Color, fill: Color) {
    if let Some(mut style) = world.get_mut::<InterfaceButtonStyle>(entity) {
        style.0.ink = ink;
        style.0.panel = fill;
        style.0.accent_soft = ribbon::css_mix(style.0.accent, fill, 0.15);
    }
}

/// Reference cards reserve room for the separate clear control and explanatory
/// line. Keep the actual accessible name intact for keyboard/MCP selection.
pub(crate) fn reference_button(world: &mut World, entity: Entity) {
    if world.get::<InterfaceReference>(entity).is_none() {
        let mut style = world.get::<InterfaceButtonStyle>(entity).unwrap().0;
        style.accent_soft = ribbon::css_mix(style.accent, style.panel, 0.15);
        style.panel = style.header;
        world
            .entity_mut(entity)
            .insert((InterfaceReference, InterfaceButtonStyle(style)));
    }
}
pub(crate) fn reference_caption(world: &mut World, entity: Entity, selecting: bool) {
    reference_button(world, entity);
    let label = world.get::<InterfaceLabel>(entity).unwrap().0;
    let assets = world.resource::<ViewportUiAssets>().clone();
    let theme = world.get::<InterfaceButtonStyle>(entity).unwrap().0;
    let bounds = Node {
        position_type: PositionType::Absolute,
        left: px(31.),
        right: px(if selecting { 90. } else { 8. }),
        top: px(7.),
        height: px(20.),
        overflow: Overflow::clip(),
        ..default()
    };
    if world.get::<Node>(label) != Some(&bounds) {
        world
            .entity_mut(label)
            .insert((bounds, theme.text(&assets, 12., FontWeight::NORMAL)));
    }
    let layout = TextLayout::new(Justify::Left, bevy::text::LineBreak::NoWrap);
    if world.get::<TextLayout>(label).is_none_or(|current| {
        current.justify != layout.justify || current.linebreak != layout.linebreak
    }) {
        world.entity_mut(label).insert(layout);
    }
}

/// A dimension label remains a real inspectable button, painted in the same
/// color as its extension lines. Field/dialog styles are unaffected.
pub(crate) fn dimension_label(world: &mut World, entity: Entity, color: Color) {
    world.entity_mut(entity).insert(DimensionInk(color));
    let mut style = world.get_mut::<InterfaceButtonStyle>(entity).unwrap();
    style.0.ink = color;
    style.0.accent = color;

    caption_size(world, entity, 12.);
    let label = world.get::<InterfaceLabel>(entity).unwrap().0;
    world.entity_mut(label).insert(Node::default());
}

#[derive(Component)]
struct DestructiveButton;

#[derive(Component)]
struct PrimaryButton;

#[derive(Component)]
struct CheckboxDecoration {
    square: Entity,
    check: Entity,
}

#[derive(Component)]
struct RadioDecoration {
    circle: Entity,
    dot: Entity,
}

/// One accessible radio control with a retained, font-independent indicator.
pub(crate) fn radio_card(world: &mut World, entity: Entity, camera: Entity, checked: bool) {
    let theme = world.get::<InterfaceButtonStyle>(entity).unwrap().0;
    let (circle, dot) = if let Some(parts) = world.get::<RadioDecoration>(entity) {
        (parts.circle, parts.dot)
    } else {
        compact_label(world, entity, 26.);
        let dot = world
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(3.),
                    top: px(3.),
                    width: px(6.),
                    height: px(6.),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                UiTargetCamera(camera),
                BackgroundColor(theme.accent),
            ))
            .id();
        let circle = world
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(8.),
                    top: px(10.),
                    width: px(14.),
                    height: px(14.),
                    border: UiRect::all(px(1.)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                UiTargetCamera(camera),
            ))
            .add_child(dot)
            .id();
        world
            .entity_mut(entity)
            .add_child(circle)
            .insert(RadioDecoration { circle, dot });
        (circle, dot)
    };
    let border = BorderColor::all(if checked { theme.accent } else { theme.mute });
    if world.get::<BackgroundColor>(dot) != Some(&BackgroundColor(theme.accent)) {
        world.entity_mut(dot).insert(BackgroundColor(theme.accent));
    }
    if world.get::<BorderColor>(circle) != Some(&border) {
        world.entity_mut(circle).insert(border);
    }
    let visibility = if checked {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if world.get::<Visibility>(dot) != Some(&visibility) {
        world.entity_mut(dot).insert(visibility);
    }
    world.entity_mut(entity).remove::<InterfaceFlat>();
    let label = world.get::<InterfaceLabel>(entity).unwrap().0;
    let bounds = Node {
        position_type: PositionType::Absolute,
        left: px(30.),
        right: px(8.),
        top: px(6.),
        height: px(22.),
        ..default()
    };
    if world.get::<Node>(label) != Some(&bounds) {
        world.entity_mut(label).insert(bounds);
    }
}

/// Use a drawn checkbox rather than relying on font-specific checkbox glyphs.
/// The enclosing control remains the single focus and activation target.
pub(crate) fn checkbox_button(world: &mut World, entity: Entity, camera: Entity, checked: bool) {
    let theme = world.get::<InterfaceButtonStyle>(entity).unwrap().0;
    let (square, check) = if let Some(parts) = world.get::<CheckboxDecoration>(entity) {
        (parts.square, parts.check)
    } else {
        compact_label(world, entity, 24.);
        let square = world
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(4.),
                    top: px(8.),
                    width: px(14.),
                    height: px(14.),
                    border: UiRect::all(px(1.)),
                    border_radius: BorderRadius::all(px(2.)),
                    ..default()
                },
                UiTargetCamera(camera),
            ))
            .id();
        let check = ribbon::decoration(world, camera, ribbon::Icon::Finish, Color::WHITE);
        world.entity_mut(check).insert(Node {
            position_type: PositionType::Absolute,
            left: px(1.),
            top: px(1.),
            width: px(10.),
            height: px(10.),
            ..default()
        });
        world.entity_mut(square).add_child(check);
        world
            .entity_mut(entity)
            .add_child(square)
            .insert(CheckboxDecoration { square, check });
        (square, check)
    };
    let background = BackgroundColor(if checked { theme.accent } else { Color::NONE });
    let border = BorderColor::all(if checked { theme.accent } else { theme.mute });
    if world.get::<BackgroundColor>(square) != Some(&background) {
        world.entity_mut(square).insert(background);
    }
    if world.get::<BorderColor>(square) != Some(&border) {
        world.entity_mut(square).insert(border);
    }
    let visibility = if checked {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if world.get::<Visibility>(check) != Some(&visibility) {
        world.entity_mut(check).insert(visibility);
    }
}

pub(crate) fn primary_button(world: &mut World, entity: Entity) {
    if world.get::<PrimaryButton>(entity).is_some() {
        return;
    }
    let mut theme = world.get::<InterfaceButtonStyle>(entity).unwrap().0;
    theme.panel = theme.accent;
    theme.hover = ribbon::css_mix(Color::WHITE, theme.accent, 0.12);
    theme.accent_soft = theme.panel;
    theme.ink = Color::WHITE;
    theme.accent = Color::WHITE;
    theme.edge = theme.panel;
    let label = world.get::<InterfaceLabel>(entity).unwrap().0;
    let assets = world.resource::<ViewportUiAssets>().clone();
    world
        .entity_mut(label)
        .insert(theme.text(&assets, 12., FontWeight::SEMIBOLD));
    world
        .entity_mut(entity)
        .remove::<InterfaceFlat>()
        .insert((InterfaceButtonStyle(theme), PrimaryButton));
}

pub(crate) fn destructive_button(world: &mut World, entity: Entity) {
    if world.get::<DestructiveButton>(entity).is_some() {
        return;
    }
    let mut theme = world.get::<InterfaceButtonStyle>(entity).unwrap().0;
    theme.panel = Color::srgb_u8(220, 38, 38);
    theme.hover = Color::srgb_u8(239, 68, 68);
    theme.accent_soft = theme.panel;
    theme.ink = Color::WHITE;
    theme.accent = Color::WHITE;
    theme.edge = theme.panel;
    let label = world.get::<InterfaceLabel>(entity).unwrap().0;
    let assets = world.resource::<ViewportUiAssets>().clone();
    world
        .entity_mut(label)
        .insert(theme.text(&assets, 12., FontWeight::SEMIBOLD));
    world
        .entity_mut(entity)
        .remove::<InterfaceFlat>()
        .insert((InterfaceButtonStyle(theme), DestructiveButton));
}

pub(crate) fn spawn_button(
    commands: &mut Commands,
    camera: Entity,
    node: Node,
    control: InterfaceControl,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) -> Entity {
    let label = commands
        .spawn((
            Text::new(control.label.clone()),
            theme.text(assets, 12.0, FontWeight::SEMIBOLD),
            TextColor(theme.ink),
        ))
        .id();
    commands
        .spawn((
            Name::new(format!("Native interface: {}", control.label)),
            control,
            node,
            UiTargetCamera(camera),
            InterfaceLabel(label),
            InterfaceButtonStyle(theme),
            BackgroundColor(theme.panel),
            BorderColor::all(theme.edge),
            Outline::new(px(2.), px(1.), Color::NONE),
            ZIndex(30),
        ))
        .add_child(label)
        .id()
}

#[derive(Resource, Clone)]
struct InterfaceReducer(InterfaceCallback);

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct InterfaceLayout;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct InterfaceReduction;

pub(crate) fn install(
    app: &mut App,
    handle: NativeInterfaceHandle,
    reducer: impl Fn(&mut World, &NativeInterfaceHandle) + Send + Sync + 'static,
) {
    app.insert_resource(handle)
        .insert_resource(InterfaceReducer(Arc::new(reducer)))
        .add_systems(Startup, setup_camera)
        .add_systems(
            Update,
            (
                drain_actions.in_set(InterfaceReduction),
                update_controls,
                ribbon::update_glyphs,
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            publish_layout
                .in_set(InterfaceLayout)
                .after(UiSystems::Stack)
                .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
}

fn drain_actions(world: &mut World) {
    let reducer = world.resource::<InterfaceReducer>().clone();
    let handle = world.resource::<NativeInterfaceHandle>().clone();
    (reducer.0)(world, &handle);
}

/// Full-surface interface camera, separate from the CAD camera's inner viewport.
#[derive(Component)]
pub struct InterfaceCamera;

fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Name::new("Native application interface camera"),
        InterfaceCamera,
        Camera2d,
        bevy::camera::visibility::RenderLayers::none(),
        Camera {
            order: 2,
            is_active: false,
            clear_color: bevy::camera::ClearColorConfig::None,
            ..default()
        },
    ));
}

#[cfg(feature = "dev-ui-lab")]
pub(super) fn install_visual_lab(app: &mut App) {
    app.insert_resource(NativeInterfaceHandle::new(|| {}))
        .add_systems(Update, (update_controls, ribbon::update_glyphs));
}

fn update_controls(
    handle: Res<NativeInterfaceHandle>,
    native_locale: Option<Res<crate::native_viewport::localization::NativeLocale>>,
    mut controls: StyledControlQuery,
    mut labels: Query<(&mut Text, &mut TextColor)>,
    mut cameras: Query<&mut Camera, With<InterfaceCamera>>,
) {
    let locale = crate::native_viewport::localization::locale_of(native_locale.as_deref());
    let Ok(shared) = handle.shared.lock() else {
        return;
    };
    let active =
        shared.desired_frame.is_some() && controls.iter().any(|(_, control, ..)| control.visible);
    for mut camera in &mut cameras {
        if camera.is_active != active {
            camera.is_active = active;
        }
    }
    for (
        entity,
        control,
        label,
        style,
        ribbon,
        caption,
        flat,
        reference,
        primary,
        dimension,
        mut node,
        mut background,
        mut border,
        outline,
    ) in &mut controls
    {
        let key = ControlKey(entity.to_bits());
        let theme = style.0;
        let hovered = !control.disabled && shared.hovered == Some(key);
        let active = control.selected == Some(true)
            || shared
                .capture
                .as_ref()
                .is_some_and(|capture| capture.resolved.key == key);
        let display = if control.visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let fill = if let Some(ribbon) = ribbon {
            ribbon.fill(theme, active, hovered, control.disabled)
        } else if flat.is_some() && control.role == "checkbox" {
            if hovered {
                ribbon::css_mix(theme.edge, theme.panel, 0.2)
            } else {
                Color::NONE
            }
        } else if flat.is_some() && active {
            ribbon::css_mix(theme.accent, theme.panel, 0.20)
        } else if active {
            theme.accent_soft
        } else if hovered {
            theme.hover
        } else if flat.is_some() {
            Color::NONE
        } else {
            theme.panel
        };
        let fill = if primary.is_some() && control.disabled {
            fill.with_alpha(0.4)
        } else {
            fill
        };
        if background.0 != fill {
            background.0 = fill;
        }
        let mut edge = BorderColor::all(
            if shared.focused == Some(key) || (reference.is_some() && active) {
                theme.accent
            } else if ribbon.is_some() || flat.is_some() {
                Color::NONE
            } else {
                theme.edge
            },
        );
        if primary.is_some() && control.disabled {
            edge = BorderColor::all(theme.edge.with_alpha(0.4));
        }
        if *border != edge {
            *border = edge;
        }
        let focus_ring = if !control.disabled && shared.focused == Some(key) {
            theme.accent
        } else {
            Color::NONE
        };
        if let Some(mut outline) = outline {
            if outline.color != focus_ring {
                outline.color = focus_ring;
            }
        }
        if let Ok((mut text, mut color)) = labels.get_mut(label.0) {
            let localized = ribbon.and_then(|button| {
                button
                    .message_key()
                    .map(|_| button.localized_label(locale).to_owned())
            });
            let shown = localized.as_deref().unwrap_or_else(|| {
                ribbon.map_or_else(
                    || caption.map_or(control.label.as_str(), |item| item.0.as_str()),
                    |button| button.label(),
                )
            });
            if text.0 != shown {
                text.0 = shown.to_owned();
            }
            let ink = if let Some(ribbon) = ribbon {
                ribbon.ink(theme, control.disabled)
            } else if control.role == "heading" {
                theme.mute
            } else if primary.is_some() && control.disabled {
                // Match the reference's opacity on the whole submit button:
                // blend its white caption over the dialog footer, too.
                ribbon::css_mix(theme.ink, theme.header, 0.4)
            } else if let Some(dimension) = dimension.filter(|_| control.disabled) {
                // Construction disables dimension editing, not measurement
                // readability. Retain the ink used by its extension lines.
                dimension.0
            } else if control.disabled {
                ribbon::css_mix(theme.mute, theme.panel, 0.4)
            } else {
                theme.ink
            };
            if color.0 != ink {
                color.0 = ink;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn publish_layout(
    handle: Res<NativeInterfaceHandle>,
    scale: Option<Res<bevy::ui::UiScale>>,
    controls: RenderedControlQuery,
    occluders: OccluderQuery,
    mut removed_occluders: RemovedComponents<InterfaceOccluder>,
    mut removed_canvas_owners: RemovedComponents<InterfaceCanvasOccluder>,
    mut removed_passthrough: RemovedComponents<InterfacePointerPassthrough>,
    mut removed_annotations: RemovedComponents<InterfaceCanvasAnnotation>,
    mut removed: RemovedComponents<InterfaceControl>,
    mut removed_clips: RemovedComponents<CalculatedClip>,
    mut last_revision: Local<Option<u64>>,
) {
    let Ok(mut shared) = handle.shared.lock() else {
        return;
    };
    let removed = removed.read().count() > 0
        || removed_clips.read().count() > 0
        || removed_occluders.read().count() > 0
        || removed_canvas_owners.read().count() > 0
        || removed_passthrough.read().count() > 0
        || removed_annotations.read().count() > 0;
    if *last_revision == Some(shared.revision)
        && !removed
        && scale
            .as_ref()
            .is_none_or(|scale| scale.0 == shared.presented_ui_scale)
        && !occluders
            .iter()
            .any(|(node, transform, stack, clip, visibility, canvas)| {
                node.is_changed()
                    || transform.is_changed()
                    || stack.is_changed()
                    || clip.as_ref().is_some_and(|clip| clip.is_changed())
                    || visibility.is_changed()
                    || canvas.as_ref().is_some_and(|canvas| canvas.is_changed())
            })
        && !controls.iter().any(
            |(
                _,
                control,
                node,
                transform,
                stack,
                clip,
                visibility,
                _,
                text,
                passthrough,
                annotation,
            )| {
                control.is_changed()
                    || node.is_changed()
                    || transform.is_changed()
                    || stack.is_changed()
                    || clip.as_ref().is_some_and(|clip| clip.is_changed())
                    || visibility
                        .as_ref()
                        .is_some_and(|visibility| visibility.is_changed())
                    || text.as_ref().is_some_and(|revision| revision.is_changed())
                    || passthrough
                        .as_ref()
                        .is_some_and(|marker| marker.is_changed())
                    || annotation
                        .as_ref()
                        .is_some_and(|marker| marker.is_changed())
            },
        )
    {
        if shared.render_dirty {
            shared.receipt.laid_out_revision = shared.receipt.laid_out_revision.saturating_add(1);
            shared.render_dirty = false;
        }
        return;
    }
    let Some(frame) = shared.desired_frame.clone() else {
        return;
    };
    let mut stacked: Vec<_> = controls
        .iter()
        .map(
            |(
                entity,
                control,
                computed,
                transform,
                stack,
                clip,
                visibility,
                editor,
                _,
                passthrough,
                annotation,
            )| {
                let area = HitArea::new(&computed, &transform, clip.as_deref(), frame.surface);
                let bounds = area.bounds;
                (
                    stack.0,
                    Control {
                        key: ControlKey(entity.to_bits()),
                        binding: control.binding,
                        surface: control.surface.clone(),
                        label: control.label.clone(),
                        role: control.role.clone(),
                        bounds,
                        visible: control.visible
                            && bounds.width > 0.0
                            && bounds.height > 0.0
                            && visibility
                                .as_ref()
                                .is_some_and(|visibility| visibility.get()),
                        disabled: control.disabled || passthrough.is_some(),
                        expanded: control.expanded,
                        selected: control.selected,
                        field: match (&control.field, editor) {
                            (Field::Text { read_only, .. }, Some(editor)) => {
                                let value = editor.value().to_string();
                                let range = editor.editor.raw_selection().text_range();
                                let selection = if editor.is_composing() {
                                    None
                                } else {
                                    value.get(..range.start).zip(value.get(..range.end)).map(
                                        |(start, end)| limo_cad_interface::TextSelection {
                                            start: start.encode_utf16().count(),
                                            end: end.encode_utf16().count(),
                                        },
                                    )
                                };
                                Field::Text {
                                    value,
                                    read_only: *read_only,
                                    selection,
                                }
                            }
                            _ => control.field.clone(),
                        },
                        modal_scope: control.modal_scope.clone(),
                        text_editing: control.text_editing,
                        owned_keys: control.owned_keys.clone(),
                    },
                    area,
                    passthrough.is_some(),
                    annotation.is_some(),
                )
            },
        )
        .collect();
    stacked.sort_by_key(|(stack, _, _, _, _)| *stack);
    let mut hits: Vec<_> = stacked
        .iter()
        .filter(|(_, control, _, passthrough, _)| control.visible && !*passthrough)
        .map(|(stack, control, area, _, annotation)| {
            let target = if *annotation {
                HitTarget::Annotation(control.key)
            } else {
                HitTarget::Control(control.key)
            };
            (*stack, target, area.clone())
        })
        .collect();
    for (node, transform, stack, clip, visibility, canvas) in &occluders {
        if !visibility.get() {
            continue;
        }
        let area = HitArea::new(&node, &transform, clip.as_deref(), frame.surface);
        if area.bounds.width > 0. && area.bounds.height > 0. {
            let target = canvas
                .as_ref()
                .map_or(HitTarget::Occluder, |canvas| HitTarget::Canvas(canvas.0));
            hits.push((stack.0, target, area));
        }
    }
    hits.sort_by_key(|(stack, _, _)| *stack);
    let hits = hits
        .into_iter()
        .map(|(_, key, bounds)| (key, bounds))
        .collect();
    let mut published: Vec<_> = stacked
        .into_iter()
        .map(|(_, control, _, _, _)| control)
        .collect();
    published.sort_by(|a, b| {
        a.bounds
            .y
            .total_cmp(&b.bounds.y)
            .then(a.bounds.x.total_cmp(&b.bounds.x))
            .then(a.key.0.cmp(&b.key.0))
    });
    let eligible = |key| {
        published.iter().any(|control| {
            control.key == key
                && control.visible
                && !control.disabled
                && frame
                    .modal_stack
                    .last()
                    .is_none_or(|modal| control.modal_scope.as_ref() == Some(modal))
        })
    };
    shared
        .tab_excluded
        .retain(|key| published.iter().any(|control| control.key == *key));
    shared.focused = shared.focused.filter(|key| eligible(*key));
    shared.hovered = shared.hovered.filter(|key| eligible(*key));
    if shared
        .capture
        .as_ref()
        .is_some_and(|capture| !eligible(capture.resolved.key))
    {
        shared.capture = None;
    }
    let next = SurfaceFrame {
        client: frame.client,
        controls: published,
        surfaces: frame.surfaces.clone(),
        canvases: frame.canvases.clone(),
        focused: shared.focused,
        modal_stack: frame.modal_stack.clone(),
        document_visible: frame.document_visible,
    };
    let changed =
        shared.registry.context() != Some(&frame.context) || shared.registry.frame() != &next;
    if changed {
        if let Err(error) = shared.registry.replace(frame.context.clone(), next) {
            eprintln!("Native interface frame rejected: {error}");
            return;
        }
    }
    if changed || shared.render_dirty {
        shared.receipt.laid_out_revision = shared.receipt.laid_out_revision.saturating_add(1);
        shared.render_dirty = false;
    }
    shared.hit_order = hits;
    shared.presented_frame = Some(frame);
    shared.presented_ui_scale = scale.map_or(1., |scale| scale.0);
    *last_revision = Some(shared.revision);
}

fn intersection(a: InterfaceRect, b: InterfaceRect) -> InterfaceRect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    InterfaceRect {
        x,
        y,
        width: (a.x + a.width).min(b.x + b.width).max(x) - x,
        height: (a.y + a.height).min(b.y + b.height).max(y) - y,
    }
}

#[cfg(test)]
pub(crate) mod tests;
