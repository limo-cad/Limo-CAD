//! The document browser retains the engine's hierarchy and stable object ids.
//! Presentation selection does not alter geometry; eye controls persist through
//! the existing project visibility command, including sketches and datums.
use super::*;
use crate::native_viewport::interface_shell::{
    compact_label,
    ribbon::{compact_glyph, Icon},
    InterfaceCaption,
};
use limo_cad_core::{BrowserNode, BrowserNodeKind as Kind, DocumentDto};
use limo_cad_interface::{ControlInput, KeyChord};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BrowserCommand {
    Select(u64),
    Expand(u64),
    Visibility(u64),
    Edit(u64),
}

#[derive(Resource, Default)]
struct Browser {
    owner: Option<DocumentContext>,
    revision: u64,
    document: Option<Arc<DocumentDto>>,
    collapsed: HashSet<u64>,
    selected: Option<u64>,
    active_sketch: Option<String>,
    widgets: HashMap<String, (Entity, BrowserCommand, Icon)>,
    labels: HashMap<String, Entity>,
    text_boxes: HashMap<String, Entity>,
}

fn find(nodes: &[BrowserNode], id: u64) -> Option<&BrowserNode> {
    nodes.iter().find_map(|node| {
        if node.id.0 == id {
            Some(node)
        } else {
            find(&node.children, id)
        }
    })
}
fn rows<'a>(
    nodes: &'a [BrowserNode],
    collapsed: &HashSet<u64>,
    depth: usize,
    out: &mut Vec<(usize, &'a BrowserNode)>,
) {
    for node in nodes {
        out.push((depth, node));
        if !collapsed.contains(&node.id.0) {
            rows(&node.children, collapsed, depth + 1, out);
        }
    }
}
fn label(node: &BrowserNode) -> &str {
    node.name.as_deref().unwrap_or(match node.kind {
        Kind::DocumentSettings => "Document Settings",
        Kind::NamedViews => "Named Views",
        Kind::NamedView => "Named View",
        Kind::Origin => "Origin",
        Kind::OriginPlaneXy => "XY",
        Kind::OriginPlaneXz => "XZ",
        Kind::OriginPlaneYz => "YZ",
        Kind::OriginCenterPoint => "Origin Point",
        Kind::BodiesFolder | Kind::Body => "Bodies",
        Kind::SketchesFolder | Kind::Sketch => "Sketches",
        Kind::ConstructionFolder => "Construction",
        Kind::ConstructionPlane => "Construction Plane",
    })
}
fn icon(kind: Kind) -> Icon {
    match kind {
        Kind::DocumentSettings => Icon::Settings,
        Kind::NamedViews | Kind::NamedView => Icon::Bookmark,
        Kind::Origin => Icon::Crosshair,
        Kind::OriginCenterPoint => Icon::CircleDot,
        Kind::OriginPlaneXy
        | Kind::OriginPlaneXz
        | Kind::OriginPlaneYz
        | Kind::ConstructionPlane => Icon::Square,
        Kind::BodiesFolder | Kind::Body => Icon::Box,
        Kind::SketchesFolder | Kind::Sketch => Icon::PenLine,
        Kind::ConstructionFolder => Icon::Layers,
    }
}
fn hidden(node: &BrowserNode, presentation: &native_viewport::ViewportPresentation) -> bool {
    match node.kind {
        Kind::Body => node
            .reference_id
            .is_some_and(|id| presentation.hidden_body_ids.contains(&id)),
        Kind::Sketch => node
            .name
            .as_ref()
            .is_some_and(|name| presentation.hidden_sketch_names.contains(name)),
        Kind::ConstructionPlane => node
            .reference_id
            .is_some_and(|id| presentation.hidden_datum_plane_ids.contains(&id)),
        _ => false,
    }
}
struct ActionNode {
    kind: Kind,
    name: Option<String>,
    reference_id: Option<u64>,
    has_children: bool,
}

fn action_node(engine: &AppState, id: u64) -> Result<ActionNode, String> {
    engine.with_document(|document| {
        find(document.browser(), id)
            .map(|node| ActionNode {
                kind: node.kind,
                name: node.name.clone(),
                reference_id: node.reference_id,
                has_children: !node.children.is_empty(),
            })
            .ok_or_else(|| "The browser node no longer exists".into())
    })
}

fn visibility_arguments(engine: &AppState, node: &ActionNode) -> Result<Value, String> {
    let mut value =
        crate::session_bridge::parse_engine_envelope(engine.engine_call("project_visibility", ""))?;
    let (key, target) = match node.kind {
        Kind::Body => (
            "hidden_body_ids",
            json!(node.reference_id.ok_or("Missing body id")?),
        ),
        Kind::Sketch => (
            "hidden_sketch_names",
            json!(node.name.as_ref().ok_or("Missing sketch name")?),
        ),
        Kind::ConstructionPlane => (
            "hidden_datum_plane_ids",
            json!(node.reference_id.ok_or("Missing plane id")?),
        ),
        _ => return Err("This browser node has no visibility control".into()),
    };
    let entries = value[key]
        .as_array_mut()
        .ok_or("Invalid project visibility")?;
    if entries.contains(&target) {
        entries.retain(|entry| entry != &target);
    } else {
        entries.push(target);
    }
    Ok(value)
}

pub(crate) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    command: &BrowserCommand,
) -> Result<Value, String> {
    bridge
        .with_native_document_owner(engine, &action.context, || handle.validate_action(action))?;
    let id = match *command {
        BrowserCommand::Select(id)
        | BrowserCommand::Expand(id)
        | BrowserCommand::Visibility(id)
        | BrowserCommand::Edit(id) => id,
    };
    let node = action_node(engine, id)?;
    world.init_resource::<Browser>();
    let input = &action.control.input;
    let context_gesture = matches!(input, ControlInput::ContextMenu)
        || matches!(input, ControlInput::Key(key) if key.key == "ContextMenu" || (key.key == "F10" && key.shift));
    let body_context =
        matches!(command, BrowserCommand::Select(_)) && node.kind == Kind::Body && context_gesture;
    if matches!(command, BrowserCommand::Select(_)) && node.kind == Kind::Sketch && context_gesture
    {
        let feature_id = engine.with_document(|document| {
            document
                .features()
                .features
                .iter()
                .find(|feature| {
                    feature.kind == limo_cad_core::FeatureKind::Sketch
                        && node.name.as_deref() == Some(feature.name.as_str())
                })
                .map(|feature| feature.id.0)
                .ok_or("The sketch history feature no longer exists")
        })?;
        world.resource_mut::<Browser>().selected = Some(id);
        return history::reduce(
            world,
            handle,
            engine,
            bridge,
            action,
            &history::HistoryCommand::Select(feature_id),
        );
    }
    if let ControlInput::Key(chord) = input {
        if !chord.ctrl
            && !chord.meta
            && !chord.alt
            && !chord.shift
            && matches!(chord.key.as_str(), "ArrowLeft" | "ArrowRight")
        {
            if node.has_children {
                let mut state = world.resource_mut::<Browser>();
                if chord.key == "ArrowLeft" {
                    state.collapsed.insert(id);
                } else {
                    state.collapsed.remove(&id);
                }
            }
            return Ok(json!({"node_id":id}));
        }
    }
    if !super::super::is_activation(input) && !body_context {
        return Err("Unsupported browser input".into());
    }
    if matches!(command, BrowserCommand::Edit(_))
        || (matches!(command, BrowserCommand::Select(_))
            && matches!(input, ControlInput::DoubleClick)
            && node.kind == Kind::Sketch)
    {
        return crate::native_editor::execute(
            world,
            engine,
            bridge,
            &action.context,
            crate::native_editor::EditorCommand::Edit(node.name.ok_or("Missing sketch name")?),
            || handle.validate_action(action),
        );
    }
    match command {
        BrowserCommand::Select(_) => {
            if matches!(node.kind, Kind::NamedViews | Kind::NamedView) {
                return named_views::open(
                    world,
                    engine,
                    &action.context,
                    (node.kind == Kind::NamedView)
                        .then_some(node.name.as_deref())
                        .flatten(),
                );
            }
            if let Some(panel) = feature::panel(world).filter(|p| {
                matches!(
                    p.pick_target,
                    Some(feature::SolidField::FirstPlane | feature::SolidField::SecondPlane)
                )
            }) {
                use limo_cad_core::{FaceId, PlaneRef};
                let plane = match node.kind {
                    Kind::OriginPlaneXy => Some(PlaneRef::ORIGIN_PLANES[0]),
                    Kind::OriginPlaneXz => Some(PlaneRef::ORIGIN_PLANES[1]),
                    Kind::OriginPlaneYz => Some(PlaneRef::ORIGIN_PLANES[2]),
                    Kind::ConstructionPlane => node.reference_id.map(|id| PlaneRef::DatumPlane {
                        datum_id: FaceId(id),
                    }),
                    _ => None,
                };
                if let Some(plane) = plane {
                    return feature::accept_pick(
                        engine,
                        bridge,
                        world,
                        &action.context,
                        panel.form_id,
                        feature::FeaturePick::Plane(plane),
                        || handle.validate_action(action),
                    );
                }
            }
            if crate::native_editor::support::picking(world) {
                use limo_cad_core::{FaceId, PlaneRef};
                let plane = match node.kind {
                    Kind::OriginPlaneXy => Some(PlaneRef::ORIGIN_PLANES[0]),
                    Kind::OriginPlaneXz => Some(PlaneRef::ORIGIN_PLANES[1]),
                    Kind::OriginPlaneYz => Some(PlaneRef::ORIGIN_PLANES[2]),
                    Kind::ConstructionPlane => node.reference_id.map(|id| PlaneRef::DatumPlane {
                        datum_id: FaceId(id),
                    }),
                    _ => None,
                };
                if let Some(plane) = plane {
                    return crate::native_editor::execute(
                        world,
                        engine,
                        bridge,
                        &action.context,
                        crate::native_editor::EditorCommand::Begin(plane),
                        || handle.validate_action(action),
                    );
                }
            }
            world.resource_mut::<Browser>().selected = Some(id);
            let command = if node.kind == Kind::Body {
                NativeCommand::SelectBody {
                    body_id: node.reference_id.ok_or("Missing body id")?,
                    occurrence_id: None,
                }
            } else {
                NativeCommand::ClearSelection
            };
            let mut result =
                bridge.with_native_document_receipt(engine, &action.context, |revision| {
                    view::apply(engine, world, &action.context, revision, command)
                })?;
            if body_context {
                result["status_message"] = json!("Use Design History for feature actions.");
            }
            Ok(result)
        }
        BrowserCommand::Expand(_) => {
            let mut state = world.resource_mut::<Browser>();
            if !state.collapsed.remove(&id) {
                state.collapsed.insert(id);
            }
            Ok(json!({"node_id":id,"expanded":!state.collapsed.contains(&id)}))
        }
        BrowserCommand::Visibility(_) => {
            let receipt = bridge.native_document_receipt(engine, &action.context)?;
            let draft = feature::profile_feature_visibility_draft(world, &receipt);
            let draft_receipt = receipt.clone();
            worker::enqueue_transaction(
                world,
                "project_set_visibility".into(),
                move |services, guard| {
                    services.bridge.apply_native_mutation_guarded(
                        &services.engine,
                        &receipt.owner,
                        Some(receipt.revision),
                        "project_set_visibility",
                        || {
                            let node = action_node(&services.engine, id)?;
                            visibility_arguments(&services.engine, &node)
                        },
                        || guard.validate(),
                    )
                },
                move |world, services, result| {
                    let result = result?;
                    let revision = result.engine_revision;
                    let value = finish_mutation(
                        &services.engine,
                        &services.bridge,
                        world,
                        "project_set_visibility",
                        result,
                    );
                    if value["render_error"].is_null() && value["publication_error"].is_null() {
                        if let Some(form_id) = draft {
                            feature::advance_profile_feature_visibility(
                                &services.engine,
                                &services.bridge,
                                world,
                                &draft_receipt,
                                form_id,
                                revision,
                            )?;
                        }
                    }
                    Ok(value)
                },
            )
        }
        BrowserCommand::Edit(_) => unreachable!(),
    }
}

fn node(x: f32, y: f32, w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(x),
        top: px(y),
        width: px(w),
        height: px(h),
        align_items: AlignItems::Center,
        overflow: Overflow::clip(),
        ..default()
    }
}

#[allow(clippy::too_many_arguments)]
fn button(
    world: &mut World,
    state: &mut Browser,
    live: &mut HashSet<String>,
    camera: Entity,
    assets: &ViewportUiAssets,
    theme: ViewportUiTheme,
    key: String,
    name: String,
    caption: &str,
    command: BrowserCommand,
    glyph: Icon,
    bounds: Node,
    inset: f32,
    selected: Option<bool>,
    expanded: Option<bool>,
    disabled: bool,
) -> Result<(), String> {
    live.insert(key.clone());
    if state
        .widgets
        .get(&key)
        .is_some_and(|(_, _, prior)| *prior != glyph)
    {
        let (entity, _, prior) = state.widgets.get_mut(&key).unwrap();
        interface_shell::ribbon::replace_compact_glyph(world, *entity, glyph);
        *prior = glyph;
    }
    let entity = if let Some((entity, _, _)) = state.widgets.get(&key) {
        *entity
    } else {
        let entity = spawn_button(
            &mut world.commands(),
            camera,
            bounds.clone(),
            InterfaceControl::button("solid/selection", &name),
            theme,
            assets,
        );
        world.flush();
        bind_command(world, entity, NativeCommand::Browser(command))?;
        compact_label(world, entity, inset);
        compact_glyph(
            world,
            entity,
            glyph,
            if caption.is_empty() { 3. } else { inset - 17. },
            if caption.is_empty() { 12. } else { 13. },
        );
        world.entity_mut(entity).insert((
            InterfaceCaption(caption.into()),
            ZIndex(if caption.is_empty() { 32 } else { 31 }),
        ));
        state.widgets.insert(key.clone(), (entity, command, glyph));
        entity
    };
    if state.widgets[&key].1 != command {
        bind_command(world, entity, NativeCommand::Browser(command))?;
        state.widgets.get_mut(&key).unwrap().1 = command;
    }
    if world.get::<Node>(entity) != Some(&bounds) {
        world.entity_mut(entity).insert(bounds);
    }
    let mut control = world.get::<InterfaceControl>(entity).unwrap().clone();
    control.label = name;
    control.selected = selected;
    control.expanded = expanded;
    control.disabled = disabled;
    control.surface = match command {
        BrowserCommand::Visibility(_) => {
            limo_cad_interface::catalog::group_for("project_set_visibility").unwrap()
        }
        BrowserCommand::Edit(_) => limo_cad_interface::catalog::group_for("sketch_edit").unwrap(),
        _ => "solid/selection",
    }
    .into();
    if let BrowserCommand::Select(id) = command {
        control.role = "treeitem".into();
        control.owned_keys = ["ArrowLeft", "ArrowRight"]
            .map(|key| KeyChord {
                key: key.into(),
                ..default()
            })
            .to_vec();
        if state
            .document
            .as_ref()
            .and_then(|document| find(&document.browser, id))
            .is_some_and(|node| matches!(node.kind, Kind::Sketch | Kind::Body))
        {
            control.owned_keys.push(KeyChord::plain("ContextMenu"));
            control.owned_keys.push(KeyChord {
                key: "F10".into(),
                shift: true,
                ..default()
            });
        }
    }
    if world.get::<InterfaceControl>(entity) != Some(&control) {
        world.entity_mut(entity).insert(control);
    }
    let caption = InterfaceCaption(caption.into());
    if world.get::<InterfaceCaption>(entity) != Some(&caption) {
        world.entity_mut(entity).insert(caption);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn text(
    world: &mut World,
    state: &mut Browser,
    camera: Entity,
    assets: &ViewportUiAssets,
    theme: ViewportUiTheme,
    key: &str,
    value: String,
    bounds: Node,
    size: f32,
) {
    let entity = *state.labels.entry(key.into()).or_insert_with(|| {
        world
            .spawn((
                UiTargetCamera(camera),
                ZIndex(30),
                Text::default(),
                theme.text(assets, size, FontWeight::NORMAL),
                TextColor(theme.ink),
                TextLayout::no_wrap(),
            ))
            .id()
    });
    let container = *state.text_boxes.entry(key.into()).or_insert_with(|| {
        let container = world.spawn((UiTargetCamera(camera), ZIndex(30))).id();
        world.entity_mut(container).add_child(entity);
        world.entity_mut(entity).insert(Node {
            min_width: px(0.),
            flex_shrink: 0.,
            ..default()
        });
        container
    });
    if world.get::<Node>(container) != Some(&bounds) {
        world.entity_mut(container).insert(bounds);
    }
    if world.get::<Text>(entity).is_none_or(|text| text.0 != value) {
        world.entity_mut(entity).insert(Text::new(value));
    }
    if world.get::<TextColor>(entity) != Some(&TextColor(theme.ink)) {
        world.entity_mut(entity).insert(TextColor(theme.ink));
    }
}

pub(crate) fn synchronize(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    revision: u64,
    bounds: InterfaceRect,
    scroll: &mut f32,
) -> Result<(), String> {
    let mut state = world.remove_resource::<Browser>().unwrap_or_default();
    let result = (|| {
        if state.owner.as_ref() != Some(owner) {
            state.selected = None;
            state.collapsed.clear();
            *scroll = 0.;
            state.owner = Some(owner.clone());
            state.document = None;
            state.active_sketch = None;
        }
        if state.document.is_none() || state.revision != revision {
            let doc = services.engine.document_snapshot();
            if state.document.is_none() {
                for n in &doc.browser {
                    if n.kind != Kind::BodiesFolder {
                        state.collapsed.insert(n.id.0);
                    }
                }
            }
            let active = crate::native_editor::active(&services.engine)?.map(|s| s.name);
            if state.active_sketch != active {
                if active.is_some() {
                    for folder in &doc.browser {
                        if folder.kind == Kind::SketchesFolder {
                            state.collapsed.remove(&folder.id.0);
                        }
                    }
                }
                state.active_sketch = active;
            }
            if state
                .selected
                .is_some_and(|id| find(&doc.browser, id).is_none())
            {
                state.selected = None;
            }
            state.document = Some(Arc::new(doc));
            state.revision = revision;
        }
        let document = state.document.as_ref().unwrap().clone();
        let document_title =
            if document.name == "Untitled" && world.contains_resource::<files::Files>() {
                files::tabs(world, services, owner)
                    .ok()
                    .and_then(|tabs| {
                        tabs.into_iter()
                            .find(|tab| tab.active && tab.owner == *owner)
                            .map(|tab| tab.display_name().to_owned())
                    })
                    .unwrap_or_else(|| document.name.clone())
            } else {
                document.name.clone()
            };
        let mut visible = vec![];
        rows(&document.browser, &state.collapsed, 0, &mut visible);
        let mut cameras = world.query_filtered::<Entity, With<InterfaceCamera>>();
        let camera = cameras
            .single(world)
            .map_err(|_| "Missing interface camera")?;
        let assets = world.resource::<ViewportUiAssets>().clone();
        let theme = crate::native_viewport::ui::theme(world);
        let x = bounds.x as f32;
        let y = bounds.y as f32;
        let width = bounds.width as f32;
        let height = bounds.height as f32;
        text(
            world,
            &mut state,
            camera,
            &assets,
            theme,
            "heading",
            "BROWSER".into(),
            node(x + 8., y + 5., width - 16., 18.),
            10.,
        );
        let heading = state.labels["heading"];
        world.entity_mut(heading).insert((
            theme.text(&assets, 10., FontWeight::SEMIBOLD),
            bevy::text::LetterSpacing::Px(1.),
            TextColor(theme.mute),
        ));
        let divider = *state
            .labels
            .entry("heading-divider".into())
            .or_insert_with(|| world.spawn(UiTargetCamera(camera)).id());
        world.entity_mut(divider).insert((
            node(x, y + 27., width, 1.),
            BackgroundColor(theme.edge),
            ZIndex(24),
        ));
        text(
            world,
            &mut state,
            camera,
            &assets,
            theme,
            "document",
            document_title,
            node(x + 26., y + 33., width - 66., 24.),
            12.,
        );
        let globe = *state
            .labels
            .entry("document-icon".into())
            .or_insert_with(|| {
                interface_shell::ribbon::decoration(world, camera, Icon::Globe, theme.mute)
            });
        let globe_bounds = node(x + 8., y + 39., 13., 13.);
        if world.get::<Node>(globe) != Some(&globe_bounds) {
            world.entity_mut(globe).insert(globe_bounds);
        }
        text(
            world,
            &mut state,
            camera,
            &assets,
            theme,
            "units",
            format!("{:?}", document.settings.units).to_uppercase(),
            node(x + width - 34., y + 35., 28., 20.),
            10.,
        );
        let units = state.labels["units"];
        let mut badge = node(x + width - 34., y + 35., 26., 18.);
        badge.border = UiRect::all(px(1.));
        badge.border_radius = BorderRadius::all(px(3.));
        badge.justify_content = JustifyContent::Center;
        world
            .entity_mut(state.text_boxes["units"])
            .insert((badge, BorderColor::all(theme.edge)));
        world
            .entity_mut(units)
            .insert((TextColor(theme.mute), TextLayout::justify(Justify::Center)));
        *scroll = scroll
            .min((visible.len() as f32 * 24. - (height - 66.)).max(0.))
            .max(0.);
        let mut live = HashSet::new();
        for (index, (depth, n)) in visible.into_iter().enumerate() {
            let row_y = y + 62. + index as f32 * 24. - *scroll;
            if row_y < y + 62. || row_y + 24. > y + height {
                continue;
            }
            let id = n.id.0;
            let indent = x + 18. + depth as f32 * 14.;
            let name = label(n);
            let caption = match n.kind {
                Kind::OriginPlaneXy => "XY Plane",
                Kind::OriginPlaneXz => "XZ Plane",
                Kind::OriginPlaneYz => "YZ Plane",
                Kind::OriginCenterPoint => "Center Point",
                _ => name,
            };
            let selected = if n.kind == Kind::Body {
                n.reference_id.is_some_and(|id| {
                    native_viewport::interface_view(world)
                        .2
                        .selected_body_ids
                        .contains(&id)
                })
            } else {
                state.selected == Some(id)
            };
            let can_hide = matches!(n.kind, Kind::Body | Kind::Sketch | Kind::ConstructionPlane);
            let expanded = (!n.children.is_empty()).then_some(!state.collapsed.contains(&id));
            button(
                world,
                &mut state,
                &mut live,
                camera,
                &assets,
                theme,
                format!("row-{id}"),
                name.into(),
                caption,
                BrowserCommand::Select(id),
                icon(n.kind),
                node(x, row_y, width, 24.),
                indent - x + 37.,
                Some(selected),
                expanded,
                false,
            )?;
            let row = state.widgets[&format!("row-{id}")].0;
            interface_shell::clip_caption(
                world,
                row,
                indent - x + 37.,
                if n.kind == Kind::Sketch {
                    48.
                } else if can_hide {
                    26.
                } else {
                    6.
                },
            );
            if n.kind == Kind::Sketch {
                let entity = state.widgets[&format!("row-{id}")].0;
                let active = state.active_sketch.as_deref() == Some(name);
                interface_shell::control_colors(
                    world,
                    entity,
                    if active { theme.accent } else { theme.ink },
                    theme.panel,
                );
                interface_shell::caption_weight(
                    world,
                    entity,
                    if active {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    },
                );
            }
            if let Some(expanded) = expanded {
                button(
                    world,
                    &mut state,
                    &mut live,
                    camera,
                    &assets,
                    theme,
                    format!("expand-{id}"),
                    format!("{} {name}", if expanded { "Collapse" } else { "Expand" }),
                    "",
                    BrowserCommand::Expand(id),
                    if expanded {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    },
                    node(indent, row_y, 18., 24.),
                    0.,
                    None,
                    Some(expanded),
                    false,
                )?;
            }
            if can_hide {
                let hide = hidden(n, native_viewport::interface_view(world).2);
                button(
                    world,
                    &mut state,
                    &mut live,
                    camera,
                    &assets,
                    theme,
                    format!("eye-{id}"),
                    format!("{} {name}", if hide { "Show" } else { "Hide" }),
                    "",
                    BrowserCommand::Visibility(id),
                    if hide { Icon::EyeOff } else { Icon::Eye },
                    node(x + width - 22., row_y, 20., 24.),
                    0.,
                    None,
                    None,
                    false,
                )?;
            }
            if n.kind == Kind::Sketch {
                button(
                    world,
                    &mut state,
                    &mut live,
                    camera,
                    &assets,
                    theme,
                    format!("edit-{id}"),
                    format!("Edit {name}"),
                    "",
                    BrowserCommand::Edit(id),
                    Icon::Pencil,
                    node(x + width - 44., row_y, 20., 24.),
                    0.,
                    None,
                    None,
                    native_viewport::interface_view(world).2.mode
                        == native_viewport::ViewportMode::Sketch,
                )?;
            }
        }
        state.widgets.retain(|key, (entity, _, _)| {
            if live.contains(key) {
                true
            } else {
                world.despawn(*entity);
                false
            }
        });
        Ok(())
    })();
    world.insert_resource(state);
    result
}

#[cfg(test)]
mod tests;

pub(crate) fn hide(world: &mut World) {
    if let Some(mut state) = world.remove_resource::<Browser>() {
        for (_, (entity, _, _)) in state.widgets.drain() {
            world.despawn(entity);
        }
        for (_, entity) in state.labels.drain() {
            world.despawn(entity);
        }
        for (_, entity) in state.text_boxes.drain() {
            world.despawn(entity);
        }
        world.insert_resource(state);
    }
}
