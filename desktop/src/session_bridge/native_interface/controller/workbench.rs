//! Workbench chrome over the shared command catalog. Retained controls keep
//! the document and binding guards.
use super::*;
use chrome::{rect, Widgets};
use interface_shell::ribbon::{self, Icon};

#[derive(Resource, Default)]
pub(crate) struct NavigationRectangle(pub Option<InterfaceRect>);

pub(crate) mod cam;
pub(crate) mod cam_export;
pub(crate) mod cam_view;
mod drawing_authoring;
pub(crate) mod drawing_editor;
mod drawing_navigation;
mod drawing_navigation_input;
mod drawing_paper;
mod ribbon_menu;
#[cfg(test)]
mod tests;
mod viewport;

pub(super) fn capture_paper_diagnostics(world: &World) -> Option<Value> {
    let state = world.get_resource::<Workbench>()?;
    drawing_paper::diagnostics(world, state)
}

pub(super) fn inspect_paper_navigation(world: &World) -> Option<Value> {
    drawing_paper::navigation_snapshot(world, world.get_resource::<Workbench>()?)
}

/// Called inside the publisher fence after an exact SelectSheet completion.
pub(crate) fn advance_sheet_selection(
    world: &mut World,
    owner: &DocumentContext,
    from: u64,
    to: u64,
) {
    drawing_paper::advance_sheet_selection(world, owner, from, to);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum NavigationTool {
    #[default]
    Select,
    Orbit,
    Pan,
    Zoom,
    ZoomWindow,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Workspace {
    #[default]
    Solid,
    Drawing,
    Cam,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Command {
    Menu(String),
    Dismiss,
    Navigation(NavigationTool),
    Workspace(Workspace),
    DrawingFit,
    DrawingZoom(i8),
    CamView(cam_view::Command),
    CamExport(cam_export::Command),
}
#[derive(Resource, Default)]
struct Workbench {
    owner: Option<DocumentContext>,
    menu: Option<String>,
    menu_x: f32,
    navigation: NavigationTool,
    workspace: Workspace,
    workspaces: HashMap<(String, String), Workspace>,
    sketch: bool,
    dial: Option<InterfaceRect>,
    widgets: Widgets,
    axes: Option<Entity>,
    paper_document: Option<(
        workspace::DocumentReceipt,
        Arc<limo_cad_sketch::DrawingDocumentDto>,
    )>,
    paper_key: Option<(
        u64,
        limo_cad_sketch::DrawingSheetDto,
        limo_cad_core::UnitSystem,
    )>,
    paper: Vec<drawing_paper::Segment>,
    paper_labels: Vec<drawing_paper::Label>,
    paper_fills: Vec<drawing_paper::Fill>,
    paper_view: Option<drawing_paper::PaperView>,
}

fn same_document(previous: Option<&DocumentContext>, current: &DocumentContext) -> bool {
    previous.is_some_and(|previous| {
        previous.window_id == current.window_id && previous.document_id == current.document_id
    })
}

impl Workbench {
    fn refresh_owner(&mut self, owner: &DocumentContext) {
        if self.owner.as_ref() == Some(owner) {
            return;
        }
        if !same_document(self.owner.as_ref(), owner) {
            if let Some(previous) = &self.owner {
                self.workspaces.insert(
                    (previous.window_id.clone(), previous.document_id.clone()),
                    self.workspace,
                );
            }
            self.workspace = self
                .workspaces
                .get(&(owner.window_id.clone(), owner.document_id.clone()))
                .copied()
                .unwrap_or_default();
        }
        self.menu = None;
        self.navigation = NavigationTool::Select;
        self.owner = Some(owner.clone());
        self.paper_document = None;
        self.paper_key = None;
        self.paper.clear();
        self.paper_labels.clear();
        self.paper_fills.clear();
        self.paper_view = None;
    }
}

pub(super) fn observe_document(world: &mut World, owner: &DocumentContext) {
    world.init_resource::<Workbench>();
    world.resource_mut::<Workbench>().refresh_owner(owner);
}

pub(super) fn cancel_navigation(world: &mut World) {
    if let Some(mut state) = world.get_resource_mut::<Workbench>() {
        if let Some(view) = &mut state.paper_view {
            view.navigation.cancel();
        }
    }
}

pub(super) fn evict_document_geometry(world: &mut World, owner: &DocumentContext) {
    cam::retire_document(world, owner, false);
    drawing_editor::retire_document(world, owner, false);
    drawing_paper::evict_document_geometry(world, owner);
}

pub(super) fn retire_document(world: &mut World, owner: &DocumentContext) {
    cam::retire_document(world, owner, true);
    drawing_editor::retire_document(world, owner, true);
    let Some(mut state) = world.get_resource_mut::<Workbench>() else {
        return;
    };
    state
        .workspaces
        .remove(&(owner.window_id.clone(), owner.document_id.clone()));
    if same_document(state.owner.as_ref(), owner) {
        state.owner = None;
        state.workspace = Workspace::Solid;
    }
}

pub(crate) fn modal(world: &World) -> Option<&'static str> {
    cam::modal(world)
        .or_else(|| cam_export::modal(world))
        .or_else(|| cam_view::modal(world))
        .or_else(|| section_review::modal(world))
        .or_else(|| {
            world
                .get_resource::<Workbench>()
                .and_then(|s| s.menu.as_ref())
                .map(|_| "workbench-menu")
        })
}
pub(crate) fn escape(world: &mut World) {
    // Nonmodal 3D inspection keeps ordinary camera gestures available.
    if section_review::in_3d(world) {
        section_review::escape(world);
        return;
    }
    if cam::modal(world).is_some() {
        cam::escape(world);
        return;
    }
    if cam_view::modal(world).is_some() {
        cam_view::escape(world);
        return;
    }
    cam_export::escape(world);
    if let Some(mut state) = world.get_resource_mut::<Workbench>() {
        state.menu = None;
    }
}
pub(crate) fn workspace(world: &World) -> Workspace {
    world
        .get_resource::<Workbench>()
        .map_or(Workspace::Solid, |state| state.workspace)
}
pub(crate) fn drawing_navigate(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    input: &crate::native_viewport::winit_host::NativeHostInput,
) -> Result<bool, String> {
    drawing_navigation_input::navigate(world, handle, input)
}
pub(crate) fn drawing_canvas(world: &World) -> Option<Canvas> {
    world
        .get_resource::<Workbench>()
        .and_then(drawing_paper::canvas)
}
pub(crate) fn drawing_author_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    input: &crate::native_viewport::winit_host::NativeHostInput,
) -> Result<bool, String> {
    drawing_authoring::process(world, handle, services, input)
}

/// Geometry and annotation handles belong to the paper canvas; form controls do not.
pub(crate) fn drawing_canvas_control(
    world: &World,
    handle: &NativeInterfaceHandle,
    cursor: [f64; 2],
) -> bool {
    handle.hit_key(cursor).is_some_and(|key| {
        matches!(
            world
                .get::<NativeCommandBinding>(Entity::from_bits(key.0))
                .map(|binding| &binding.command),
            Some(NativeCommand::Drawing(drawing_editor::Command::Annotation(
                _,
                drawing_authoring::Command::Select(_)
                    | drawing_authoring::Command::Anchor(_)
                    | drawing_authoring::Command::Circle(_)
                    | drawing_authoring::Command::Line(_)
                    | drawing_authoring::Command::Center(_)
                    | drawing_authoring::Command::CenterGrip(_, _)
                    | drawing_authoring::Command::CloudEdge(_, _)
                    | drawing_authoring::Command::Chamfer(_)
            )))
        )
    })
}
pub(crate) fn cancel_drawing_author_input(world: &mut World) {
    drawing_authoring::cancel_input(world);
}
pub(crate) fn drawing_author_pointer_active(world: &World) -> bool {
    drawing_authoring::pointer_active(world)
}
pub(crate) fn navigation(world: &World) -> NavigationTool {
    world
        .get_resource::<Workbench>()
        .map_or(NavigationTool::Select, |s| s.navigation)
}
pub(crate) fn dial(world: &World) -> Option<InterfaceRect> {
    world.get_resource::<Workbench>().and_then(|s| s.dial)
}
pub(crate) fn dial_key(world: &World) -> Option<limo_cad_interface::ControlKey> {
    world
        .get_resource::<Workbench>()
        .and_then(|s| s.axes)
        .map(|e| limo_cad_interface::ControlKey(e.to_bits()))
}
pub(crate) fn execute(world: &mut World, command: &Command) -> Result<Value, String> {
    if matches!(command, Command::DrawingFit | Command::DrawingZoom(_)) {
        return drawing_navigation_input::execute(world, command);
    }
    if let Command::CamView(command) = command {
        return cam_view::execute(world, command);
    }
    world.init_resource::<Workbench>();
    let mut state = world.resource_mut::<Workbench>();
    match command {
        Command::Menu(menu) => {
            state.menu = (state.menu.as_ref() != Some(menu)).then(|| menu.clone())
        }
        Command::Dismiss => state.menu = None,
        Command::Navigation(tool) => {
            state.navigation = if state.navigation == *tool {
                NavigationTool::Select
            } else {
                *tool
            };
            state.menu = None;
        }
        Command::Workspace(workspace) => {
            state.workspace = *workspace;
            state.menu = None;
        }
        Command::CamView(_) => unreachable!(),
        Command::DrawingFit | Command::DrawingZoom(_) => unreachable!(),
        Command::CamExport(_) => return Err("Post controls require their document receipt".into()),
    }
    Ok(json!({"handled":true}))
}

pub(super) fn tool_node() -> Node {
    ribbon::node(0., 0., 48.)
}

/// Status captions reserve space above this retained navigation row.
pub(super) fn navigation_top(height: f32) -> f32 {
    height - 94.
}

fn centered_button(
    (widgets, world, camera): (&mut Widgets, &mut World, Entity),
    (key, label, caption): (&str, &str, &str),
    command: NativeCommand,
    mut bounds: Node,
    selected: Option<bool>,
    disabled: bool,
    z: i32,
) -> Result<Entity, String> {
    let mut control = InterfaceControl::button("document/session", label);
    control.selected = selected;
    control.disabled = disabled;
    bounds.justify_content = JustifyContent::Center;
    let entity = widgets.button(
        world,
        camera,
        key,
        control,
        Some(caption),
        command,
        bounds,
        None,
        z,
    )?;
    if world.get::<ribbon::RibbonButton>(entity).is_none() {
        interface_shell::center_caption(world, entity);
        interface_shell::caption_size(world, entity, 10.);
    }
    Ok(entity)
}

pub(super) fn card(
    (widgets, world, camera): (&mut Widgets, &mut World, Entity),
    key: &str,
    mut bounds: Node,
    fill: Color,
    radius: f32,
    z: i32,
) {
    let theme = crate::native_viewport::ui::theme(world);
    bounds.border = UiRect::all(px(1.));
    bounds.border_radius = BorderRadius::all(px(radius));
    widgets.panel(world, camera, key, bounds, fill, z);
    if let Some(entity) = widgets.entity(key) {
        world
            .entity_mut(entity)
            .insert(BorderColor::all(theme.edge));
    }
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    controls: &HashMap<String, Entity>,
    (width, height, side): (f32, f32, f32),
    sketch: bool,
    owner: &DocumentContext,
    services: &NativeServices,
) -> Result<(), String> {
    let mut state = world.remove_resource::<Workbench>().unwrap_or_default();
    let result = (|| {
        state.refresh_owner(owner);
        if sketch != state.sketch {
            state.navigation = NavigationTool::Select;
            state.sketch = sketch;
        }
        if sketch
            && state
                .menu
                .as_deref()
                .is_some_and(|menu| menu != "workspace")
        {
            state.menu = None;
        }
        if files::modal(world).is_some() || history::modal(world).is_some() {
            state.menu = None;
        }
        state.widgets.begin();
        ribbon_menu::synchronize(world, camera, controls, width, sketch, services, &mut state)?;
        if state.workspace == Workspace::Drawing && !sketch {
            state.dial = None;
            if let Some(entity) = state.axes.take() {
                world.despawn(entity);
            }
            for entity in controls.values() {
                if matches!(
                    world
                        .get::<NativeCommandBinding>(*entity)
                        .map(|binding| &binding.command),
                    Some(
                        NativeCommand::Orient(_)
                            | NativeCommand::Fit
                            | NativeCommand::ClearSelection
                    )
                ) {
                    world.get_mut::<InterfaceControl>(*entity).unwrap().visible = false;
                }
            }
            drawing_paper::paint(
                world,
                camera,
                services,
                &mut state,
                (width, height, side),
                controls,
            )?;
        } else {
            state.paper_key = None;
            state.paper.clear();
            state.paper_labels.clear();
            state.paper_fills.clear();
            state.paper_view = None;
            viewport::synchronize(world, camera, controls, width, height, side, &mut state)?;
        }
        cam::synchronize(
            world,
            camera,
            services,
            owner,
            height,
            side,
            state.workspace == Workspace::Cam && !sketch,
        )?;
        drawing_authoring::synchronize(
            world,
            camera,
            services,
            owner,
            (height, side),
            state.workspace == Workspace::Drawing && !sketch,
            &state,
        )?;
        drawing_editor::synchronize(
            world,
            camera,
            services,
            owner,
            (height, side),
            state.workspace == Workspace::Drawing && !sketch,
            &state,
        )?;
        cam::synchronize_library(
            world,
            camera,
            services,
            owner,
            width,
            height,
            state.workspace == Workspace::Cam && !sketch,
        )?;
        let cam_visible =
            state.workspace == Workspace::Cam && !sketch && feature::panel(world).is_none();
        cam_view::synchronize(
            world,
            camera,
            services,
            owner,
            (width, height, side),
            cam_visible,
        )?;
        cam_export::synchronize(
            world,
            camera,
            services,
            owner,
            (width, height, side),
            cam_visible,
        )?;
        state.widgets.finish(world);
        Ok(())
    })();
    world.insert_resource(state);
    result
}
