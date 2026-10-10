//! Shared viewport selection cards, including the separate clear action.
use super::super::{FeaturePanel, SolidField as F, SolidFieldView, SolidFormKind as K};
use super::*;

pub(super) fn text_height(text: &str, width: f32) -> f32 {
    let columns = (width / 4.8).floor().max(12.) as usize;
    (text.chars().count().div_ceil(columns).max(1) as f32) * 16.
}

fn component_selection(panel: &FeaturePanel, field: F) -> bool {
    field == F::Bodies
        && panel.kind == K::MoveCopy
        && panel.fields.iter().any(|row| {
            row.field == F::MoveObjectType
                && matches!(&row.value, limo_cad_interface::Field::Choice { value, .. } if value == "component")
        })
}

fn hint(panel: &FeaturePanel, field: F) -> &'static str {
    match field {
        F::Source if panel.kind == K::Extrude => "Click a closed sketch region or an existing planar solid face. Picking one source type clears the other.",
        F::Source if panel.kind == K::Loft => "Click profiles in the order they should be connected. Selected sections are highlighted in the model.",
        F::Source => "Selected regions are highlighted in the model. Click this field to change them.",
        F::Edges => "Click edges on one body to add or remove them from the selection.",
        F::HoleSupport => "Select a planar face for the hole direction.",
        F::HolePositions => "Face picks update the selected free position. Sketch points add linked positions; pick one again to remove it.",
        F::Cylinder => "Choose an exterior cylinder; hole walls are rejected.",
        F::Faces => "Click faces on one body to add or remove openings.",
        F::TargetBody => "Click the body that will receive the result.",
        F::Bodies if panel.kind == K::SplitBody => "Click the body to divide at the reference plane.",
        F::Bodies if component_selection(panel, field) => "The clicked occurrence is selected by its visible geometry.",
        F::Bodies => "Selected bodies are highlighted in the model. Continue clicking to select more than one.",
        F::ToolBodies => "The target stays separate from the tool bodies.",
        F::FirstPlane | F::SecondPlane => "Choose in the browser or click a planar face.",
        F::AxisEdge if panel.kind.is_pattern() => "An edge supplies both origin and direction.",
        F::AxisEdge => "Choose a straight edge on the reference plane.",
        F::DirectionEdge | F::SecondDirectionEdge => "Choose a straight edge or enter XYZ below.",
        F::AxisLine => "Click a straight line on the profile plane.",
        F::Path if panel.kind == K::Rib => "Selected centerline curves are highlighted in the model.",
        F::Path | F::Guide => "Click connected curves to add or remove them from the selection.",
        F::Targets => "Continue clicking, or use Shift/Ctrl/Cmd, to select multiple target bodies.",
        field if field.is_move_point() => "Pick a sketch point, body vertex or surface.",
        _ => "The selected face is highlighted in the model.",
    }
}

fn legend(panel: &FeaturePanel, field: F) -> &'static str {
    match field {
        F::Source if panel.kind == K::Extrude => "PROFILES OR PLANAR FACE",
        F::Source if panel.kind == K::Loft => "SECTIONS",
        F::Source if panel.kind == K::Sweep => "PROFILE",
        F::Source => "PROFILES",
        F::Edges => "EDGES",
        F::Faces => "FACES TO REMOVE",
        F::Cylinder => "CYLINDRICAL SURFACE",
        F::FromPoint => "FROM POINT",
        F::ToPoint => "TO POINT",
        F::PivotPoint => "ROTATION PIVOT",
        F::HoleSupport => "SUPPORT FACE",
        F::HolePositions => "POSITIONS",
        F::TargetBody => "TARGET BODY",
        F::Bodies if panel.kind == K::SplitBody => "BODY TO SPLIT",
        F::Bodies if component_selection(panel, field) => "COMPONENT",
        F::Bodies => "BODIES",
        F::ToolBodies => "TOOL BODIES",
        F::FirstPlane if panel.kind == K::Midplane => "FIRST REFERENCE",
        F::FirstPlane => "REFERENCE PLANE",
        F::SecondPlane => "SECOND REFERENCE",
        F::AxisEdge => "AXIS REFERENCE",
        F::DirectionEdge => "FIRST DIRECTION REFERENCE",
        F::SecondDirectionEdge => "SECOND DIRECTION REFERENCE",
        F::AxisLine => "AXIS LINE",
        F::Targets => "TARGET BODIES",
        F::StopFace => "STOP FACE",
        F::Path if matches!(panel.kind, K::Loft | K::Rib) => "CENTERLINE",
        F::Path => "PATH",
        F::Guide => "GUIDE RAIL",
        _ => "REFERENCE",
    }
}

pub(super) fn height(panel: &FeaturePanel, row: &SolidFieldView, inner: f32) -> f32 {
    let selected = panel
        .presentation
        .references
        .get(&row.field)
        .is_some_and(|r| r.has_selection);
    20. + card_height(panel, row.field, inner) + if selected { 28. } else { 0. } + 12.
}

fn card_height(panel: &FeaturePanel, field: F, inner: f32) -> f32 {
    24. + text_height(
        hint(panel, field),
        inner
            - if panel.pick_target == Some(field) {
                118.
            } else {
                39.
            },
    ) + 8.
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render(
    world: &mut World,
    state: &mut PanelWidgets,
    controls: &mut HashSet<String>,
    labels: &mut HashSet<String>,
    body: Entity,
    camera: Entity,
    panel: &FeaturePanel,
    row: &SolidFieldView,
    inner: f32,
    y: &mut f32,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) -> Result<(), String> {
    let key = format!("{:?}", row.field);
    let selected = panel.presentation.references.get(&row.field);
    let active = panel.pick_target == Some(row.field);
    let component = component_selection(panel, row.field);
    let heading = if component {
        crate::native_viewport::localization::translate(world, "bodyFeature.component")
    } else {
        legend(panel, row.field)
    };
    label(
        world,
        state,
        labels,
        &format!("{key}-label"),
        body,
        camera,
        heading,
        node(0., *y - state.scroll, inner, 18.),
        theme,
        assets,
        false,
    );
    *y += 20.;
    let card_height = card_height(panel, row.field, inner);
    let mut control = InterfaceControl::button(panel.kind.group(), &row.label);
    control.disabled = !row.enabled;
    control.selected = Some(active);
    widget(
        world,
        state,
        controls,
        &key,
        body,
        camera,
        control,
        node(0., *y - state.scroll, inner, card_height),
        FeatureCommand::Control {
            form_id: panel.form_id,
            action: FeatureControl::Pick(row.field),
        },
        theme,
        assets,
    )?;
    let entity = state.controls[&key].0;
    world
        .entity_mut(entity)
        .insert(interface_shell::InterfaceCaption(
            selected.map_or_else(|| row.label.clone(), |r| r.caption.clone()),
        ));
    interface_shell::reference_caption(world, entity, active);
    let icon_key = format!("{key}-pointer");
    labels.insert(icon_key.clone());
    let icon = *state.decorations.entry(icon_key).or_insert_with(|| {
        let e = ribbon::decoration(world, camera, Icon::Select, theme.accent);
        world.entity_mut(entity).add_child(e);
        e
    });
    ribbon::refresh_decoration(
        world,
        icon,
        Icon::Select,
        if active || selected.is_some_and(|r| r.has_selection) {
            theme.accent
        } else {
            theme.mute
        },
    );
    world
        .entity_mut(icon)
        .insert(node(8., (card_height - 14.) * 0.5, 14., 14.));
    let guidance = if component {
        crate::native_viewport::localization::translate(world, "bodyFeature.componentHint")
    } else {
        hint(panel, row.field)
    };
    label(
        world,
        state,
        labels,
        &format!("{key}-hint"),
        body,
        camera,
        guidance,
        node(31., *y + 24. - state.scroll, inner - 39., card_height - 30.),
        theme,
        assets,
        false,
    );
    world
        .entity_mut(state.labels[&format!("{key}-hint")])
        .insert((
            theme.text(assets, 10., FontWeight::NORMAL),
            bevy::text::LineHeight::Px(16.),
        ));
    if active {
        let badge = format!("{key}-selecting");
        label(
            world,
            state,
            labels,
            &badge,
            body,
            camera,
            "SELECTING",
            node(
                inner - 79.,
                *y + (card_height - 18.) * 0.5 - state.scroll,
                71.,
                18.,
            ),
            theme,
            assets,
            false,
        );
        world.entity_mut(state.labels[&badge]).insert((
            TextColor(theme.accent),
            BackgroundColor(ribbon::css_mix(theme.accent, theme.panel, 0.20)),
        ));
        world.entity_mut(state.labels[&badge]).insert((
            theme.text(assets, 9., FontWeight::SEMIBOLD),
            TextLayout::justify(Justify::Center),
        ));
        // Leave the badge's column clear on every line of the helper text.
        if let Some(mut bounds) = world.get_mut::<Node>(state.labels[&format!("{key}-hint")]) {
            bounds.width = px(inner - 118.);
        }
    }
    *y += card_height;
    if selected.is_some_and(|r| r.has_selection) {
        let mut clear = InterfaceControl::button(panel.kind.group(), clear_label(row.field));
        clear.disabled = !row.enabled;
        widget(
            world,
            state,
            controls,
            &format!("{key}-clear"),
            body,
            camera,
            clear,
            node(0., *y + 4. - state.scroll, 58., 24.),
            FeatureCommand::Control {
                form_id: panel.form_id,
                action: FeatureControl::Clear(row.field),
            },
            theme,
            assets,
        )?;
        let clear_entity = state.controls[&format!("{key}-clear")].0;
        let icon_key = format!("{key}-clear-icon");
        labels.insert(icon_key.clone());
        let icon = *state.decorations.entry(icon_key).or_insert_with(|| {
            let e = ribbon::decoration(world, camera, Icon::Cancel, theme.mute);
            world.entity_mut(clear_entity).add_child(e);
            e
        });
        ribbon::refresh_decoration(world, icon, Icon::Cancel, theme.mute);
        world.entity_mut(icon).insert(node(8., 6., 11., 11.));
        interface_shell::compact_label(world, clear_entity, 23.);
        world
            .entity_mut(clear_entity)
            .remove::<interface_shell::InterfaceFlat>();
        interface_shell::caption_size(world, clear_entity, 10.);
        *y += 28.;
    }
    *y += 12.;
    if let Some(error) = visible_error(panel, row) {
        label(
            world,
            state,
            labels,
            &format!("{key}-error"),
            body,
            camera,
            error,
            node(0., *y - state.scroll, inner, 38.),
            theme,
            assets,
            false,
        );
        *y += 42.;
    }
    Ok(())
}

fn clear_label(field: F) -> &'static str {
    match field {
        F::FromPoint => "Clear from point",
        F::ToPoint => "Clear to point",
        F::PivotPoint => "Clear rotation pivot",
        F::HoleSupport => "Clear support face",
        F::HolePositions => "Clear hole positions",
        F::Source => "Clear source profiles",
        F::AxisLine => "Clear axis line",
        F::Targets => "Clear target bodies",
        F::StopFace => "Clear stop face",
        F::Path => "Clear path curves",
        F::Guide => "Clear guide curves",
        _ => "Clear reference",
    }
}
