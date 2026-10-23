//! Contextual hints and Help use the shared operation lessons.
use super::super::super::super::chrome::{self, rect};
use super::*;
use crate::native_editor::{
    CreateTool, EditorCommand as SketchCommand, FormKind, InteractionCommand, ModifyTool, Relation,
};
use crate::session_bridge::native_interface::feature::{FeatureCommand, SolidFormKind};
use std::sync::OnceLock;

const HOVER_DELAY: Duration = Duration::from_millis(700);
fn entries(id: &lesson::Id) -> &[lesson::Id] {
    match id {
        lesson::Id::Extrude | lesson::Id::Cut => &[lesson::Id::Extrude, lesson::Id::Cut],
        _ => std::slice::from_ref(id),
    }
}

// One typed mapping for every native solid/datum creation command. Exhaustive
// over SolidFormKind so a new native form cannot silently open another guide.
macro_rules! solid_guides {
    ($($kind:ident => $guide:ident),+ $(,)?) => {
        fn solid_lesson(kind: SolidFormKind) -> lesson::Id {
            match kind { $(SolidFormKind::$kind => lesson::Id::$guide,)+ }
        }
        #[cfg(test)]
        pub(super) fn solid_kind(id: lesson::Id) -> Option<SolidFormKind> {
            match id {
                $(lesson::Id::$guide => Some(SolidFormKind::$kind),)+
                lesson::Id::Cut => Some(SolidFormKind::Extrude),
                _ => None,
            }
        }
    };
}
solid_guides! {
    Extrude => Extrude,
    Revolve => Revolve,
    Sweep => Sweep,
    Loft => Loft,
    Rib => Rib,
    Fillet => Fillet,
    Chamfer => Chamfer,
    Shell => Shell,
    ExternalThread => ExternalThread,
    Hole => Hole,
    MoveCopy => MoveCopy,
    Combine => Combine,
    OffsetPlane => OffsetPlane,
    Midplane => Midplane,
    AnglePlane => PlaneAtAngle,
    Mirror => Mirror,
    SplitBody => SplitBody,
    RectangularPattern => RectangularPattern,
    CircularPattern => CircularPattern,
}

// Match the actual native editor command types. Menu headings, feature edits,
// pending tools and form-control actions never inherit a generic guide.
fn sketch_lesson(command: &SketchCommand) -> Option<lesson::Id> {
    Some(match command {
        SketchCommand::Support(crate::native_editor::support::Command::Start) => lesson::Id::Sketch,
        SketchCommand::Tool(tool) => match tool {
            CreateTool::Line => lesson::Id::SketchLine,
            CreateTool::MidpointLine => lesson::Id::SketchMidpointLine,
            CreateTool::Point => lesson::Id::SketchPoint,
            CreateTool::Rectangle(_) => lesson::Id::SketchRectangle,
            CreateTool::Circle(_) => lesson::Id::SketchCircle,
            CreateTool::Arc3Point | CreateTool::ArcCenter => lesson::Id::SketchArc,
            CreateTool::Spline => lesson::Id::SketchSpline,
            CreateTool::Slot(_) => lesson::Id::SketchSlot,
        },
        SketchCommand::Interaction(command) => match command {
            InteractionCommand::Form(kind) => match kind {
                FormKind::Fillet => lesson::Id::SketchFillet,
                FormKind::Chamfer => lesson::Id::SketchChamfer,
                FormKind::Offset => lesson::Id::SketchOffset,
                FormKind::MoveCopy => lesson::Id::SketchMoveCopy,
                FormKind::Scale => lesson::Id::SketchScale,
                FormKind::Mirror => lesson::Id::SketchMirror,
                FormKind::RectangularPattern => lesson::Id::SketchRectangularPattern,
                FormKind::CircularPattern => lesson::Id::SketchCircularPattern,
                FormKind::Polygon => lesson::Id::SketchPolygon,
            },
            InteractionCommand::Modify(tool) => match tool {
                ModifyTool::Trim => lesson::Id::SketchTrim,
                ModifyTool::Extend => lesson::Id::SketchExtend,
                ModifyTool::Break => lesson::Id::SketchBreak,
            },
            InteractionCommand::Relation(relation) => match relation {
                Relation::Align => lesson::Id::SketchHorizontalVertical,
                Relation::Coincident => lesson::Id::SketchCoincident,
                Relation::Tangent => lesson::Id::SketchTangent,
                Relation::Equal => lesson::Id::SketchEqual,
                Relation::Parallel => lesson::Id::SketchParallel,
                Relation::Perpendicular => lesson::Id::SketchPerpendicular,
                Relation::Fix => lesson::Id::SketchFixUnfix,
                Relation::Midpoint => lesson::Id::SketchMidpoint,
                Relation::Concentric => lesson::Id::SketchConcentric,
                Relation::Collinear => lesson::Id::SketchCollinear,
                Relation::Symmetry => lesson::Id::SketchSymmetry,
            },
            InteractionCommand::Dimension => lesson::Id::SketchDimension,
            InteractionCommand::Select => lesson::Id::SelectionSketch,
            _ => return None,
        },
        _ => return None,
    })
}

fn construction_reference_command(operation: &str, arguments: &serde_json::Value) -> bool {
    // The actual ribbon emits project_set_visibility with a complete snapshot.
    // Body-only Browser mutations must not acquire this construction guide.
    operation == "project_set_visibility"
        && arguments
            .get("hidden_sketch_names")
            .is_some_and(serde_json::Value::is_array)
        && arguments
            .get("hidden_datum_plane_ids")
            .is_some_and(serde_json::Value::is_array)
}

fn command_lesson(command: &NativeCommand) -> Option<lesson::Id> {
    match command {
        NativeCommand::Sketch(command) => sketch_lesson(command),
        NativeCommand::Workbench(super::super::super::super::workbench::Command::Navigation(
            super::super::super::super::workbench::NavigationTool::Select,
        )) => Some(lesson::Id::SelectionSolid),
        NativeCommand::Measure(_, super::super::super::super::measure::Command::Open) => {
            Some(lesson::Id::Measure)
        }
        NativeCommand::SectionReview(
            _,
            super::super::super::super::section_review::Command::Open,
        ) => Some(lesson::Id::SectionAnalysis),
        NativeCommand::Mutation {
            operation,
            arguments,
        } if construction_reference_command(operation, arguments) => {
            Some(lesson::Id::ConstructionVisibility)
        }
        NativeCommand::Feature(FeatureCommand::Open {
            kind,
            feature_id: None,
        }) => Some(solid_lesson(*kind)),
        _ => None,
    }
}

// Evidence/tests must bind a real entry command for each guide. This helper is
// absent from the production hover path and never executes the command.
#[cfg(test)]
pub(super) fn command_for_guide(id: lesson::Id) -> Option<NativeCommand> {
    match id {
        lesson::Id::SelectionSolid => {
            return Some(NativeCommand::Workbench(
                super::super::super::super::workbench::Command::Navigation(
                    super::super::super::super::workbench::NavigationTool::Select,
                ),
            ))
        }
        lesson::Id::Measure => {
            return Some(NativeCommand::Measure(
                0,
                super::super::super::super::measure::Command::Open,
            ))
        }
        lesson::Id::SectionAnalysis => {
            return Some(NativeCommand::SectionReview(
                0,
                super::super::super::super::section_review::Command::Open,
            ))
        }
        lesson::Id::ConstructionVisibility => {
            return Some(NativeCommand::Mutation {
                operation: "project_set_visibility".into(),
                arguments: json!({"hidden_body_ids":[],"hidden_sketch_names":[],"hidden_datum_plane_ids":[]}),
            })
        }
        _ => {}
    }
    if let Some(kind) = solid_kind(id) {
        return Some(NativeCommand::Feature(FeatureCommand::Open {
            kind,
            feature_id: None,
        }));
    }
    Some(NativeCommand::Sketch(match id {
        lesson::Id::Sketch => SketchCommand::Support(crate::native_editor::support::Command::Start),
        lesson::Id::SketchLine => SketchCommand::Tool(CreateTool::Line),
        lesson::Id::SketchMidpointLine => SketchCommand::Tool(CreateTool::MidpointLine),
        lesson::Id::SketchPoint => SketchCommand::Tool(CreateTool::Point),
        lesson::Id::SketchRectangle => SketchCommand::Tool(CreateTool::Rectangle(
            limo_cad_sketch::RectangleMode::TwoPoint,
        )),
        lesson::Id::SketchCircle => SketchCommand::Tool(CreateTool::Circle(
            limo_cad_sketch::CircleMode::CenterDiameter,
        )),
        lesson::Id::SketchArc => SketchCommand::Tool(CreateTool::Arc3Point),
        lesson::Id::SketchSpline => SketchCommand::Tool(CreateTool::Spline),
        lesson::Id::SketchSlot => {
            SketchCommand::Tool(CreateTool::Slot(limo_cad_sketch::SlotMode::CenterToCenter))
        }
        lesson::Id::SketchFillet => {
            SketchCommand::Interaction(InteractionCommand::Form(FormKind::Fillet))
        }
        lesson::Id::SketchChamfer => {
            SketchCommand::Interaction(InteractionCommand::Form(FormKind::Chamfer))
        }
        lesson::Id::SketchOffset => {
            SketchCommand::Interaction(InteractionCommand::Form(FormKind::Offset))
        }
        lesson::Id::SketchMoveCopy => {
            SketchCommand::Interaction(InteractionCommand::Form(FormKind::MoveCopy))
        }
        lesson::Id::SketchScale => {
            SketchCommand::Interaction(InteractionCommand::Form(FormKind::Scale))
        }
        lesson::Id::SketchMirror => {
            SketchCommand::Interaction(InteractionCommand::Form(FormKind::Mirror))
        }
        lesson::Id::SketchRectangularPattern => {
            SketchCommand::Interaction(InteractionCommand::Form(FormKind::RectangularPattern))
        }
        lesson::Id::SketchCircularPattern => {
            SketchCommand::Interaction(InteractionCommand::Form(FormKind::CircularPattern))
        }
        lesson::Id::SketchPolygon => {
            SketchCommand::Interaction(InteractionCommand::Form(FormKind::Polygon))
        }
        lesson::Id::SketchTrim => {
            SketchCommand::Interaction(InteractionCommand::Modify(ModifyTool::Trim))
        }
        lesson::Id::SketchExtend => {
            SketchCommand::Interaction(InteractionCommand::Modify(ModifyTool::Extend))
        }
        lesson::Id::SketchBreak => {
            SketchCommand::Interaction(InteractionCommand::Modify(ModifyTool::Break))
        }
        lesson::Id::SketchHorizontalVertical => {
            SketchCommand::Interaction(InteractionCommand::Relation(Relation::Align))
        }
        lesson::Id::SketchCoincident => {
            SketchCommand::Interaction(InteractionCommand::Relation(Relation::Coincident))
        }
        lesson::Id::SketchTangent => {
            SketchCommand::Interaction(InteractionCommand::Relation(Relation::Tangent))
        }
        lesson::Id::SketchEqual => {
            SketchCommand::Interaction(InteractionCommand::Relation(Relation::Equal))
        }
        lesson::Id::SketchParallel => {
            SketchCommand::Interaction(InteractionCommand::Relation(Relation::Parallel))
        }
        lesson::Id::SketchPerpendicular => {
            SketchCommand::Interaction(InteractionCommand::Relation(Relation::Perpendicular))
        }
        lesson::Id::SketchFixUnfix => {
            SketchCommand::Interaction(InteractionCommand::Relation(Relation::Fix))
        }
        lesson::Id::SketchMidpoint => {
            SketchCommand::Interaction(InteractionCommand::Relation(Relation::Midpoint))
        }
        lesson::Id::SketchConcentric => {
            SketchCommand::Interaction(InteractionCommand::Relation(Relation::Concentric))
        }
        lesson::Id::SketchCollinear => {
            SketchCommand::Interaction(InteractionCommand::Relation(Relation::Collinear))
        }
        lesson::Id::SketchSymmetry => {
            SketchCommand::Interaction(InteractionCommand::Relation(Relation::Symmetry))
        }
        lesson::Id::SketchDimension => SketchCommand::Interaction(InteractionCommand::Dimension),
        lesson::Id::SelectionSketch => SketchCommand::Interaction(InteractionCommand::Select),
        _ => return None,
    }))
}
#[derive(Resource, Default)]
struct Hint {
    anchor: Option<Entity>,
    open: bool,
    wake: Option<Wake>,
}
fn contains(point: Vec2, bounds: [f32; 4]) -> bool {
    point.x >= bounds[0]
        && point.y >= bounds[1]
        && point.x <= bounds[0] + bounds[2]
        && point.y <= bounds[1] + bounds[3]
}
fn keep_hint(point: Vec2, anchor: [f32; 4], card: [f32; 4], open: bool) -> bool {
    contains(point, anchor)
        || (open
            && (contains(point, card)
                || contains(
                    point,
                    [
                        anchor[0],
                        anchor[1] + anchor[3],
                        anchor[2],
                        (card[1] - anchor[1] - anchor[3]).max(0.),
                    ],
                )))
}
fn ready_hint(
    hint: &mut Hint,
    anchor: Entity,
    keyboard: bool,
    hovered: bool,
    now: Instant,
) -> bool {
    if hint.anchor != Some(anchor) {
        hint.anchor = Some(anchor);
        hint.open = false;
        hint.wake = None;
    }
    if keyboard {
        hint.open = true;
        hint.wake = None;
    } else if !hovered {
        hint.open = false;
        hint.wake = None;
    } else if hint.wake.as_ref().is_some_and(|wake| now >= wake.due) {
        hint.open = true;
    }
    hint.open
}

fn active_scope(world: &World) -> Option<&'static str> {
    active_scope_with_menu(world, false)
}

fn active_scope_with_menu(world: &World, detached_menu: bool) -> Option<&'static str> {
    scope::owner(
        super::super::super::modal(world),
        workbench::modal(world),
        detached_menu,
        super::super::super::super::history::modal(world),
        crate::native_editor::panel::modal(world),
        crate::native_editor::support::modal(world),
    )
}

pub(super) fn dismiss_hint_menu(world: &mut World) -> Option<&'static str> {
    let scope = active_scope(world);
    let dismissed = match scope {
        Some("sketch-menu") => {
            crate::native_editor::panel::escape(world);
            scope
        }
        Some("workbench-menu") => {
            workbench::dismiss_menu(world);
            scope
        }
        _ => None,
    };
    if let Some(mut hint) = world.get_resource_mut::<Hint>() {
        *hint = Hint::default();
    }
    dismissed
}

#[cfg(test)]
pub(crate) fn paint_hint(
    world: &mut World,
    camera: Entity,
    _extrude: Option<Entity>,
    widgets: &mut chrome::Widgets,
    width: f32,
) -> Result<(), String> {
    paint_scoped_hint(world, camera, widgets, width, false)
}

/// Workbench owns its detached state while painting. Carry just that transient
/// menu scope, and still require the matching presented owner and anchor scope.
pub(crate) fn paint_workbench_hint(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    width: f32,
    menu_open: bool,
) -> Result<(), String> {
    paint_scoped_hint(world, camera, widgets, width, menu_open)
}

fn paint_scoped_hint(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    width: f32,
    detached_menu: bool,
) -> Result<(), String> {
    world.init_resource::<Hint>();
    let scope = active_scope_with_menu(world, detached_menu);
    let allowed = !world.resource::<Files>().script.preview.open
        && scope::allows(scope, scope)
        && world
            .get_resource::<NativeInterfaceHandle>()
            .is_none_or(|handle| {
                handle.presented_context().map_or(scope.is_none(), |owner| {
                    handle.presented_focus_matches(&owner, scope)
                })
            })
        && crate::session_bridge::native_interface::feature::panel(world).is_none();
    if !allowed {
        *world.resource_mut::<Hint>() = Hint::default();
        return Ok(());
    }
    let scale = world
        .get_resource::<bevy::ui::UiScale>()
        .map_or(1., |s| s.0);
    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next();
    let point = window
        .filter(|w| w.focused)
        .and_then(Window::cursor_position)
        .map(|p| p / scale);
    let focused = world
        .get_resource::<NativeInterfaceHandle>()
        .and_then(NativeInterfaceHandle::focused_key)
        .filter(|_| window.is_none_or(|w| w.focused));
    let mut selected = None;
    // One query across visible commands, independent of the number of guides.
    // Typed Sketch variants and Solid forms share this scan and hover worker.
    let mut anchors = world.query::<(Entity, &NativeCommandBinding, &InterfaceControl, &Node)>();
    for (entity, binding, control, node) in anchors.iter(world) {
        if !control.visible
            || control.disabled
            || !scope::allows(scope, control.modal_scope.as_deref())
        {
            continue;
        }
        let Some(id) = command_lesson(&binding.command) else {
            continue;
        };
        let (Val::Px(x), Val::Px(y), Val::Px(w), Val::Px(h)) =
            (node.left, node.top, node.width, node.height)
        else {
            continue;
        };
        let anchor = [x, y, w, h];
        let cw = (width - 24.).clamp(260., 280.);
        let cx = x.min((width - cw - 8.).max(4.));
        let cy = y + h + 6.;
        let entries = entries(&id);
        let keyboard = focused.is_some_and(|key| {
            key.0 == entity.to_bits()
                || entries.iter().any(|id| {
                    widgets
                        .entity(&format!("{}-show-me", id.key()))
                        .is_some_and(|e| e.to_bits() == key.0)
                })
        });
        let hovered = point.is_some_and(|p| {
            keep_hint(
                p,
                anchor,
                [cx, cy, cw, 44. + entries.len() as f32 * 36.],
                world.resource::<Hint>().anchor == Some(entity) && world.resource::<Hint>().open,
            )
        });
        if keyboard
            || (hovered && (selected.is_none() || world.resource::<Hint>().anchor == Some(entity)))
        {
            selected = Some((entity, id, cx, cy, cw, keyboard, hovered));
            if keyboard {
                break;
            }
        }
    }
    let Some((anchor, id, x, y, w, keyboard, hovered)) = selected else {
        *world.resource_mut::<Hint>() = Hint::default();
        return Ok(());
    };
    let now = Instant::now();
    let open = ready_hint(
        &mut world.resource_mut::<Hint>(),
        anchor,
        keyboard,
        hovered,
        now,
    );
    if !open {
        if world.resource::<Hint>().wake.is_none() {
            if let Some(handle) = world.get_resource::<NativeInterfaceHandle>().cloned() {
                let cancelled = Arc::new(AtomicBool::new(false));
                hover::arm(world, handle, now + HOVER_DELAY, cancelled.clone())?;
                world.resource_mut::<Hint>().wake = Some(Wake {
                    due: now + HOVER_DELAY,
                    cancelled,
                });
            }
        }
        return Ok(());
    }
    let theme = crate::native_viewport::ui::theme(world);
    let language = lesson::language(world);
    let t = |key: &str| limo_cad_mcp::lessons::text(language, key);
    widgets.panel(
        world,
        camera,
        "extrude-help-card",
        Node {
            border: UiRect::all(px(1.)),
            border_radius: BorderRadius::all(px(6.)),
            ..rect(x, y, w, 44. + entries(&id).len() as f32 * 36.)
        },
        theme.panel.with_alpha(1.),
        65,
    );
    widgets.text(
        world,
        camera,
        "extrude-help-title",
        rect(x + 12., y + 8., w - 24., 20.),
        t("ui.guidedExamples"),
        12.,
        66,
    );
    let entries = entries(&id);
    widgets.panel(
        world,
        camera,
        "guided-examples-divider",
        rect(x + 12., y + 34., w - 24., 1.),
        theme.edge,
        66,
    );
    let mut previous = anchor;
    for (index, id) in entries.iter().enumerate() {
        let caption = t(&format!("{}.example", id.key()));
        let mut control = InterfaceControl::button("document/scripts", caption);
        control.disabled = available(world).is_err();
        // Book is a sibling of its command in this menu's modal/tab scope.
        // Background commands and real dialogs keep their normal isolation.
        if let Some(scope) = scope {
            control.modal_scope = Some(scope.into());
            control.role = "menuitem".into();
            control.owned_keys = ["ArrowUp", "ArrowDown", "Home", "End"]
                .map(limo_cad_interface::KeyChord::plain)
                .into();
        }
        let entry = widgets.button(
            world,
            camera,
            &format!("{}-show-me", id.key()),
            control,
            Some(caption),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Lesson(*id))),
            rect(x + 8., y + 40. + index as f32 * 36., w - 16., 32.),
            Some(interface_shell::ribbon::Icon::Book),
            66,
        )?;
        if let Some(handle) = world.get_resource::<NativeInterfaceHandle>() {
            handle.place_in_tab_order_after(
                limo_cad_interface::ControlKey(entry.to_bits()),
                limo_cad_interface::ControlKey(previous.to_bits()),
            )?;
        }
        previous = entry;
    }
    Ok(())
}

pub(super) fn help(id: lesson::Id, language: &'static str) -> &'static str {
    static PAGES: OnceLock<HashMap<(lesson::Id, &'static str), String>> = OnceLock::new();
    &PAGES.get_or_init(|| {
        lesson::ALL
            .into_iter()
            .flat_map(|id| {
                limo_cad_mcp::lessons::LOCALES
                    .into_iter()
                    .map(move |language| {
                        (
                            (id, language),
                            limo_cad_mcp::lessons::page(language, id).body,
                        )
                    })
            })
            .collect()
    })[&(id, language)]
}

/// Resolve only after layout publishes this control, against the original owner.
pub(super) fn focus_control(world: &mut World, entity: Entity) {
    let Some(owner) = world.resource::<Files>().script.preview.focus_owner.clone() else {
        return;
    };
    let Some(handle) = world.get_resource::<NativeInterfaceHandle>().cloned() else {
        return;
    };
    if !handle.presented_focus_matches(&owner, None) {
        let menu = world.resource::<Files>().script.preview.focus_from_menu;
        if menu.is_some_and(|menu| handle.presented_focus_matches(&owner, Some(menu))) {
            // The accepted Book command dismissed this menu; its presented
            // frame can lag the new card by one layout. Do not activate through it.
            handle.request_redraw();
            return;
        }
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.focus_owner = None;
        preview.focus_from_menu = None;
        return;
    }
    if let Ok(action) = handle.resolve_retained(limo_cad_interface::ControlKey(entity.to_bits())) {
        if action.context == owner && handle.prepare_activation(&action).is_ok() {
            let preview = &mut world.resource_mut::<Files>().script.preview;
            preview.focus_owner = None;
            preview.focus_from_menu = None;
            return;
        }
    }
    handle.request_redraw();
}

#[derive(Default)]
pub(super) struct HelpView {
    pub offset: f32,
    pub(super) return_focus: bool,
    pub(super) restore_offset: Option<f32>,
    max: f32,
    bounds: [f32; 4],
}

pub(super) fn scroll_help_key(
    world: &mut World,
    key: &limo_cad_interface::KeyChord,
) -> Result<(), String> {
    if key.ctrl || key.alt || key.meta || key.shift {
        return Err("Use unmodified navigation keys to scroll help".into());
    }
    let preview = &mut world.resource_mut::<Files>().script.preview;
    if !preview.open || !preview.help {
        return Err("Open help before scrolling".into());
    }
    let view = &mut preview.help_view;
    view.restore_offset = None;
    let page = view.bounds[3] * 0.8;
    view.offset = match key.key.as_str() {
        "ArrowUp" => view.offset - 28.,
        "ArrowDown" => view.offset + 28.,
        "PageUp" => view.offset - page,
        "PageDown" => view.offset + page,
        "Home" => 0.,
        "End" => view.max,
        _ => return Err("Use arrows, Page Up/Down, Home or End to scroll help".into()),
    }
    .clamp(0., view.max);
    Ok(())
}

pub(super) fn help_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    event: &NativeHostInput,
) -> Result<bool, String> {
    if !world.resource::<Files>().script.preview.help
        || !handle
            .presented_pointer_context()
            .is_some_and(|(owner, empty, _)| empty && event.context.as_ref() == Some(&owner))
    {
        return Ok(false);
    }
    let WindowEvent::MouseWheel(wheel) = &event.event else {
        return Ok(false);
    };
    let bounds = world.resource::<Files>().script.preview.help_view.bounds;
    if !event.cursor.is_some_and(|point| contains(point, bounds)) {
        return Ok(false);
    }
    let scale = world
        .get::<Window>(wheel.window)
        .map_or(1., |w| w.resolution.scale_factor());
    let amount = match wheel.unit {
        bevy::input::mouse::MouseScrollUnit::Line => wheel.y * 28.,
        bevy::input::mouse::MouseScrollUnit::Pixel => {
            wheel.y / (scale * handle.presented_ui_scale())
        }
    };
    if amount.is_finite() {
        let view = &mut world.resource_mut::<Files>().script.preview.help_view;
        view.restore_offset = None;
        view.offset = (view.offset - amount).clamp(0., view.max);
        handle.request_redraw();
    }
    Ok(true)
}

pub(super) fn paint_instructions(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    bounds: [f32; 4],
    theme: ViewportUiTheme,
) -> Result<(), String> {
    let id = lesson::current(world).unwrap_or(lesson::Id::Sketch);
    let language = lesson::language(world);
    let t = |key: &str| limo_cad_mcp::lessons::text(language, key);
    let body = help(id, language);
    let footer = if id.prerequisite().is_some() { 40. } else { 0. };
    let outer = bounds;
    let bounds = [
        bounds[0],
        bounds[1],
        bounds[2] - 8.,
        (bounds[3] - footer).max(32.),
    ];
    let matching_text = widgets
        .entity("extrude-help-text")
        .and_then(|entity| world.get::<Text>(entity))
        .is_some_and(|text| text.0 == body);
    let content_height = widgets
        .entity("extrude-help-text")
        .and_then(|entity| world.get::<ComputedNode>(entity))
        .map_or(0., |node| node.size.y * node.inverse_scale_factor);
    let view = &mut world.resource_mut::<Files>().script.preview.help_view;
    view.bounds = bounds;
    view.max = (content_height - bounds[3]).max(0.);
    if matching_text && content_height > 0. {
        if let Some(offset) = view.restore_offset.take() {
            view.offset = offset;
        }
    }
    view.offset = view.offset.clamp(0., view.max);
    let (offset, max) = (view.offset, view.max);
    let mut control = InterfaceControl::button(
        "document/scripts",
        format!("{}: {}", id.title(language), t("ui.instructions")),
    );
    control.role = "region".into();
    control.owned_keys = ["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End"]
        .map(limo_cad_interface::KeyChord::plain)
        .into();
    let clip = widgets.button(
        world,
        camera,
        "extrude-help-body",
        control,
        Some(""),
        NativeCommand::File(FileCommand::ScriptPreview(Action::HelpBody)),
        rect(bounds[0], bounds[1], bounds[2], bounds[3]),
        None,
        61,
    )?;
    widgets.text(
        world,
        camera,
        "extrude-help-text",
        Node {
            height: Val::Auto,
            align_items: AlignItems::Start,
            ..rect(0., -offset, bounds[2] - 4., 0.)
        },
        body,
        12.,
        1,
    );
    widgets.parent(world, "extrude-help-text", clip);
    if max > 0. {
        let thumb_height = (bounds[3] * bounds[3] / content_height).max(20.);
        widgets.panel(
            world,
            camera,
            "extrude-help-scrollbar",
            rect(
                bounds[0] + bounds[2] + 4.,
                bounds[1] + (bounds[3] - thumb_height) * offset / max,
                2.,
                thumb_height,
            ),
            theme.mute.with_alpha(0.6),
            62,
        );
    }
    if let Some(previous) = id.prerequisite() {
        let label = format!("{} {}", t("ui.prerequisite"), previous.title(language));
        widgets.button(
            world,
            camera,
            "lesson-prerequisite",
            InterfaceControl::button("document/scripts", &label),
            Some(&label),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Read(previous))),
            rect(outer[0], outer[1] + outer[3] - 32., outer[2], 32.),
            Some(interface_shell::ribbon::Icon::Book),
            61,
        )?;
    }
    if !world.resource::<Files>().script.preview.steps_open {
        focus_control(world, clip);
    }
    Ok(())
}

/// Keep the catalog bounded by the actual logical window height, including at
/// large UI scales. Only the visible page creates controls or shapes text.
pub(super) fn index_range(
    page: usize,
    count: usize,
    height: f32,
) -> (usize, std::ops::Range<usize>) {
    let capacity = ((height - 124.).max(62.) / 62.).floor() as usize;
    let page = page.min(count.saturating_sub(1) / capacity);
    let start = page * capacity;
    (page, start..(start + capacity).min(count))
}

pub(super) fn paint_index(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    width: f32,
    height: f32,
    theme: ViewportUiTheme,
) -> Result<(), String> {
    let language = lesson::language(world);
    let t = |key: &str| limo_cad_mcp::lessons::text(language, key);
    let card = card::Card::new(width);
    let navigation_height = card::navigation_height(world);
    let (x, y, w) = (card.x, card.y + navigation_height, card.width);
    let available = (height - y - 12.).max(186.);
    let (page, range) = index_range(
        world.resource::<Files>().script.preview.index_page,
        lesson::ALL.len(),
        available,
    );
    world.resource_mut::<Files>().script.preview.index_page = page;
    let paged = range.start > 0 || range.end < lesson::ALL.len();
    let rows = range.len();
    let footer = y + 48. + rows as f32 * 62.;
    card.paint(
        world,
        camera,
        widgets,
        "index",
        88. + rows as f32 * 62. + if paged { 36. } else { 0. } + navigation_height,
        t("ui.guidedExamples"),
        theme,
    )?;
    let restored_focus = world.resource::<Files>().script.preview.index_focus;
    let focus_visible =
        restored_focus.is_some_and(|id| range.clone().any(|i| lesson::ALL[i] == id));
    for (row, index) in range.clone().enumerate() {
        let id = lesson::ALL[index];
        let top = y + 44. + row as f32 * 62.;
        let entry = widgets.button(
            world,
            camera,
            &format!("help-{}", id.key()),
            InterfaceControl::button("document/scripts", t(&format!("{}.example", id.key()))),
            Some(t(&format!("{}.example", id.key()))),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Lesson(id))),
            rect(x + 8., top, w - 16., 28.),
            Some(interface_shell::ribbon::Icon::Book),
            61,
        )?;
        widgets.colored_text(
            world,
            camera,
            &format!("help-summary-{}", id.key()),
            rect(x + 12., top + 28., w - 24., 28.),
            t(&format!("{}.outcome", id.key())),
            11.,
            61,
            theme.mute,
        );
        if restored_focus == Some(id) || (!focus_visible && row == 0) {
            focus_control(world, entry);
        }
    }
    if paged {
        let label = format!(
            "{}\u{2013}{} / {}",
            range.start + 1,
            range.end,
            lesson::ALL.len()
        );
        widgets.text(
            world,
            camera,
            "help-index-page",
            rect(x + 68., footer, w - 136., 28.),
            &label,
            12.,
            61,
        );
        for (next, key, caption, left, disabled) in [
            (
                false,
                "help-index-previous",
                "ui.previous",
                x + 8.,
                range.start == 0,
            ),
            (
                true,
                "help-index-next",
                "ui.next",
                x + w - 56.,
                range.end == lesson::ALL.len(),
            ),
        ] {
            let mut control = InterfaceControl::button("document/scripts", t(caption));
            control.disabled = disabled;
            widgets.button(
                world,
                camera,
                key,
                control,
                Some(if next { ">" } else { "<" }),
                NativeCommand::File(FileCommand::ScriptPreview(Action::IndexPage(next))),
                rect(left, footer, 48., 28.),
                None,
                61,
            )?;
        }
    }
    widgets.button(
        world,
        camera,
        "help-full-build",
        InterfaceControl::button("document/scripts", t("ui.fullBuild")),
        Some(t("ui.fullBuild")),
        NativeCommand::File(FileCommand::ScriptPreview(Action::FullBuild)),
        rect(x + 8., footer + if paged { 36. } else { 0. }, w - 16., 32.),
        None,
        61,
    )?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_script_preview_book_focus_waits_only_for_its_dismissed_menu() {
        for unrelated in [false, true] {
            let (mut app, handle, target, _) = interface_shell::tests::fixture();
            initialize(
                app.world_mut(),
                Arc::new(Mutex::new(DocumentWorkspace::default())),
            );
            let owner = handle.presented_context().unwrap();
            {
                let preview = &mut app.world_mut().resource_mut::<Files>().script.preview;
                preview.focus_owner = Some(owner.clone());
                preview.focus_from_menu = Some("sketch-menu");
            }
            let mut frame = handle.frame().unwrap();
            frame.surfaces.push(limo_cad_interface::Surface {
                name: "sketch-menu".into(),
                text: None,
            });
            frame.modal_stack = vec!["sketch-menu".into()];
            handle.present(frame.clone()).unwrap();
            app.update();
            focus_control(app.world_mut(), target);
            assert_eq!(
                app.world()
                    .resource::<Files>()
                    .script
                    .preview
                    .focus_owner
                    .as_ref(),
                Some(&owner)
            );
            assert!(
                handle.focused_key().is_none(),
                "Never focus through the closing menu"
            );
            frame.modal_stack.clear();
            if unrelated {
                frame.surfaces.push(limo_cad_interface::Surface {
                    name: "file-dialog".into(),
                    text: None,
                });
                frame.modal_stack.push("file-dialog".into());
            }
            handle.present(frame).unwrap();
            app.update();
            focus_control(app.world_mut(), target);
            let preview = &app.world().resource::<Files>().script.preview;
            assert!(preview.focus_owner.is_none() && preview.focus_from_menu.is_none());
            assert_eq!(
                handle.focused_key(),
                (!unrelated).then_some(limo_cad_interface::ControlKey(target.to_bits()))
            );
        }
    }

    #[test]
    fn native_script_preview_inspection_commands_keep_real_control_identity() {
        use super::super::super::super::super::{measure, section_review, workbench};
        assert_eq!(
            command_lesson(&NativeCommand::Workbench(workbench::Command::Navigation(
                workbench::NavigationTool::Select
            ))),
            Some(lesson::Id::SelectionSolid)
        );
        assert_eq!(
            command_lesson(&NativeCommand::Sketch(SketchCommand::Interaction(
                InteractionCommand::Select
            ))),
            Some(lesson::Id::SelectionSketch)
        );
        assert_eq!(
            command_lesson(&NativeCommand::Measure(31, measure::Command::Open)),
            Some(lesson::Id::Measure)
        );
        assert_eq!(
            command_lesson(&NativeCommand::SectionReview(
                42,
                section_review::Command::Open
            )),
            Some(lesson::Id::SectionAnalysis)
        );
        for command in [
            NativeCommand::Workbench(workbench::Command::Navigation(
                workbench::NavigationTool::Orbit,
            )),
            NativeCommand::Measure(31, measure::Command::Close),
            NativeCommand::SectionReview(42, section_review::Command::ToggleView),
            NativeCommand::Mutation {
                operation: "project_set_visibility".into(),
                arguments: json!({"hidden_body_ids":[1]}),
            },
            NativeCommand::Mutation {
                operation: "construction_set_visibility".into(),
                arguments: json!({"hidden_sketch_names":[],"hidden_datum_plane_ids":[]}),
            },
        ] {
            assert_eq!(command_lesson(&command), None);
        }
    }

    #[test]
    fn native_script_preview_sketch_variants_preserve_their_operation_identity() {
        use limo_cad_sketch::{CircleMode, RectangleMode, SlotMode};
        for (tool, id) in [
            (CreateTool::Line, lesson::Id::SketchLine),
            (CreateTool::MidpointLine, lesson::Id::SketchMidpointLine),
            (CreateTool::Point, lesson::Id::SketchPoint),
            (
                CreateTool::Rectangle(RectangleMode::TwoPoint),
                lesson::Id::SketchRectangle,
            ),
            (
                CreateTool::Rectangle(RectangleMode::Center),
                lesson::Id::SketchRectangle,
            ),
            (
                CreateTool::Circle(CircleMode::CenterDiameter),
                lesson::Id::SketchCircle,
            ),
            (
                CreateTool::Circle(CircleMode::TwoPoint),
                lesson::Id::SketchCircle,
            ),
            (CreateTool::Arc3Point, lesson::Id::SketchArc),
            (CreateTool::ArcCenter, lesson::Id::SketchArc),
            (CreateTool::Spline, lesson::Id::SketchSpline),
            (
                CreateTool::Slot(SlotMode::CenterToCenter),
                lesson::Id::SketchSlot,
            ),
            (CreateTool::Slot(SlotMode::Overall), lesson::Id::SketchSlot),
            (
                CreateTool::Slot(SlotMode::CenterPoint),
                lesson::Id::SketchSlot,
            ),
        ] {
            assert_eq!(
                command_lesson(&NativeCommand::Sketch(SketchCommand::Tool(tool))),
                Some(id)
            );
        }
        for command in [
            SketchCommand::Edit("existing sketch".into()),
            SketchCommand::Finish,
            SketchCommand::Cancel,
            SketchCommand::Complete,
            SketchCommand::Interaction(InteractionCommand::Menu(Some("draw"))),
            SketchCommand::Interaction(InteractionCommand::Menu(Some("edit"))),
            SketchCommand::Interaction(InteractionCommand::Menu(Some("constrain"))),
            SketchCommand::Interaction(InteractionCommand::DimensionActions),
            SketchCommand::Interaction(InteractionCommand::DimensionReference),
            SketchCommand::Interaction(InteractionCommand::ApplyForm { id: 1 }),
            SketchCommand::Interaction(InteractionCommand::CancelForm { id: 1 }),
        ] {
            assert!(command_lesson(&NativeCommand::Sketch(command)).is_none());
        }
    }
    #[test]
    fn native_script_preview_every_authored_command_routes_only_to_its_guides() {
        for id in lesson::ALL {
            let command =
                command_for_guide(id).expect("Every authored guide has a real entry command");
            let actual = command_lesson(&command).unwrap();
            assert!(entries(&actual).contains(&id), "{id:?}");
            if let Some(kind) = solid_kind(id) {
                assert!(
                    command_lesson(&NativeCommand::Feature(FeatureCommand::Open {
                        kind,
                        feature_id: Some(1),
                    }))
                    .is_none(),
                    "Feature edits cannot masquerade as creation entries"
                );
            }
            if !matches!(id, lesson::Id::Extrude | lesson::Id::Cut) {
                assert_eq!(entries(&id), &[id]);
            }
        }
    }

    #[test]
    fn native_script_preview_catalog_pages_every_guide_at_small_logical_heights() {
        for height in [186., 240., 330., 600.] {
            for count in [0, 1, 4, 17, lesson::ALL.len(), 200] {
                let mut visited = Vec::new();
                let mut page = 0;
                loop {
                    let (actual, range) = index_range(page, count, height);
                    assert_eq!(actual, page);
                    assert!(124. + range.len() as f32 * 62. <= height);
                    visited.extend(range.clone());
                    if range.end == count {
                        break;
                    }
                    page += 1;
                }
                assert_eq!(visited, (0..count).collect::<Vec<_>>());
                let (_, tail) = index_range(usize::MAX, count, height);
                assert_eq!(tail.end, count);
            }
        }
    }
    #[test]
    fn native_script_preview_long_hover_waits_and_cancels_without_repeated_wakes() {
        let now = Instant::now();
        let anchor = Entity::from_bits(100);
        let mut hint = Hint::default();
        assert!(!ready_hint(&mut hint, anchor, false, true, now));
        let token = Arc::new(AtomicBool::new(false));
        hint.wake = Some(Wake {
            due: now + HOVER_DELAY,
            cancelled: token.clone(),
        });
        assert!(!ready_hint(
            &mut hint,
            anchor,
            false,
            true,
            now + HOVER_DELAY - Duration::from_millis(1)
        ));
        assert!(ready_hint(
            &mut hint,
            anchor,
            false,
            true,
            now + HOVER_DELAY
        ));
        assert!(!ready_hint(
            &mut hint,
            anchor,
            false,
            false,
            now + HOVER_DELAY
        ));
        assert!(token.load(Ordering::Acquire));
        assert!(hint.wake.is_none());
        assert!(ready_hint(&mut hint, anchor, true, false, now));
        assert!(hint.wake.is_none(), "Keyboard entry has no hover timer");
    }
    #[test]
    fn native_script_preview_help_scroll_is_bounded_and_preserves_navigation() {
        let mut world = World::new();
        initialize(
            &mut world,
            Arc::new(Mutex::new(DocumentWorkspace::default())),
        );
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.open = true;
        preview.help = true;
        preview.help_view.bounds = [0., 0., 280., 120.];
        preview.help_view.max = 200.;
        let key = limo_cad_interface::KeyChord::plain;
        scroll_help_key(&mut world, &key("PageDown")).unwrap();
        assert_eq!(
            world.resource::<Files>().script.preview.help_view.offset,
            96.
        );
        scroll_help_key(&mut world, &key("End")).unwrap();
        scroll_help_key(&mut world, &key("ArrowDown")).unwrap();
        assert_eq!(
            world.resource::<Files>().script.preview.help_view.offset,
            200.
        );
        scroll_help_key(&mut world, &key("Home")).unwrap();
        scroll_help_key(&mut world, &key("ArrowUp")).unwrap();
        assert_eq!(
            world.resource::<Files>().script.preview.help_view.offset,
            0.
        );
        assert!(scroll_help_key(&mut world, &key("Enter")).is_err());
        assert!(world.resource::<Files>().script.preview.help);
        world.resource_mut::<Files>().script.preview.open = false;
        assert!(scroll_help_key(&mut world, &key("PageDown")).is_err());
    }
    #[test]
    fn native_script_preview_keyboard_can_tab_into_show_me_without_a_pointer() {
        use bevy::ui::{ComputedStackIndex, UiGlobalTransform};
        let (mut app, handle, extrude, _) = interface_shell::tests::fixture();
        initialize(
            app.world_mut(),
            Arc::new(Mutex::new(DocumentWorkspace::default())),
        );
        app.world_mut().init_resource::<ViewportUiAssets>();
        app.world_mut()
            .entity_mut(extrude)
            .insert(rect(110., 34., 48., 54.));
        let camera = app.world_mut().spawn_empty().id();
        let mut widgets = chrome::Widgets::default();
        let action = handle
            .resolve_retained(limo_cad_interface::ControlKey(extrude.to_bits()))
            .unwrap();
        handle.prepare_activation(&action).unwrap();
        widgets.begin();
        paint_hint(app.world_mut(), camera, Some(extrude), &mut widgets, 1000.).unwrap();
        let entry = widgets.entity("extrude-show-me").unwrap();
        app.world_mut().entity_mut(entry).insert((
            ComputedNode {
                size: Vec2::new(288., 26.),
                inverse_scale_factor: 1.,
                ..default()
            },
            UiGlobalTransform::from_translation(Vec2::new(266., 181.)),
            ComputedStackIndex(10),
            InheritedVisibility::VISIBLE,
        ));
        app.update();
        assert!(handle.focus_next(false).unwrap());
        assert_eq!(
            handle.focused_key(),
            Some(limo_cad_interface::ControlKey(entry.to_bits()))
        );
        widgets.begin();
        paint_hint(app.world_mut(), camera, Some(extrude), &mut widgets, 1000.).unwrap();
        widgets.finish(app.world_mut());
        assert!(app.world().get::<InterfaceControl>(entry).is_some());
        assert!(handle
            .resolve_retained(limo_cad_interface::ControlKey(entry.to_bits()))
            .is_ok());
    }

    #[test]
    fn native_script_preview_hint_can_be_entered_without_running_or_latching_over_other_tools() {
        let anchor = [110., 34., 48., 54.];
        let card = [110., 94., 312., 112.];
        assert!(keep_hint(Vec2::new(125., 50.), anchor, card, false));
        assert!(keep_hint(Vec2::new(125., 91.), anchor, card, true));
        assert!(keep_hint(Vec2::new(250., 180.), anchor, card, true));
        assert!(!keep_hint(Vec2::new(250., 50.), anchor, card, true));
        assert!(!keep_hint(Vec2::new(600., 180.), anchor, card, true));
        assert!(!keep_hint(Vec2::new(250., 180.), anchor, card, false));
    }

    #[test]
    fn native_script_preview_sketch_entries_support_retained_keyboard_focus() {
        for id in lesson::ALL
            .into_iter()
            .filter(|id| id.key().starts_with("sketch_"))
        {
            use bevy::ui::{ComputedStackIndex, UiGlobalTransform};
            let (mut app, handle, extrude, _) = interface_shell::tests::fixture();
            initialize(
                app.world_mut(),
                Arc::new(Mutex::new(DocumentWorkspace::default())),
            );
            app.world_mut().init_resource::<ViewportUiAssets>();
            app.world_mut()
                .entity_mut(extrude)
                .insert(rect(110., 34., 48., 54.));
            let camera = app.world_mut().spawn_empty().id();
            let mut widgets = chrome::Widgets::default();
            crate::session_bridge::native_interface::bind_command(
                app.world_mut(),
                extrude,
                command_for_guide(id).unwrap(),
            )
            .unwrap();
            let action = handle
                .resolve_retained(limo_cad_interface::ControlKey(extrude.to_bits()))
                .unwrap();
            handle.prepare_activation(&action).unwrap();
            widgets.begin();
            paint_hint(app.world_mut(), camera, Some(extrude), &mut widgets, 1000.).unwrap();
            let entry = widgets.entity(&format!("{}-show-me", id.key())).unwrap();
            app.world_mut().entity_mut(entry).insert((
                ComputedNode {
                    size: Vec2::new(288., 26.),
                    inverse_scale_factor: 1.,
                    ..default()
                },
                UiGlobalTransform::from_translation(Vec2::new(266., 181.)),
                ComputedStackIndex(10),
                InheritedVisibility::VISIBLE,
            ));
            app.update();
            assert!(handle.focus_next(false).unwrap());
            assert_eq!(
                handle.focused_key(),
                Some(limo_cad_interface::ControlKey(entry.to_bits()))
            );
            widgets.begin();
            paint_hint(app.world_mut(), camera, Some(extrude), &mut widgets, 1000.).unwrap();
            widgets.finish(app.world_mut());
            assert!(app.world().get::<InterfaceControl>(entry).is_some());
            assert!(handle
                .resolve_retained(limo_cad_interface::ControlKey(entry.to_bits()))
                .is_ok());
        }
    }
}
