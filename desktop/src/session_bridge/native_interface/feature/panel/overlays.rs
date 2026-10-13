use super::super::{FeaturePanel, SolidField as F, SolidFieldView, SolidFormKind as K};
use super::*;

fn copy_hint(panel: &FeaturePanel) -> &'static str {
    if panel.fields.iter().any(|row| {
        row.field == F::MoveObjectType
            && matches!(&row.value, Field::Choice{value,..} if value=="component")
    }) {
        "Creates a linked occurrence of the same component definition."
    } else {
        "Creates independent body geometry with its own stable body identity."
    }
}
pub(super) fn copy_height(panel: &FeaturePanel, inner: f32) -> f32 {
    44. + selection::text_height(copy_hint(panel), inner - 39.) + 12.
}

#[allow(clippy::too_many_arguments)]
pub(super) fn copy_card(
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
    let height = copy_height(panel, inner) - 12.;
    let key = "copy-card";
    labels.insert(key.into());
    let frame = *state.decorations.entry(key.into()).or_insert_with(|| {
        let entity = world.spawn((UiTargetCamera(camera), ZIndex(41))).id();
        world.entity_mut(body).add_child(entity);
        entity
    });
    let mut bounds = node(0., *y - state.scroll, inner, height);
    bounds.border = UiRect::all(px(1.));
    bounds.border_radius = BorderRadius::all(px(4.));
    world.entity_mut(frame).insert((
        bounds,
        BackgroundColor(theme.header),
        BorderColor::all(theme.edge),
    ));
    let mut control = InterfaceControl::button(panel.kind.group(), &row.label);
    control.field = row.value.clone();
    control.role = "checkbox".into();
    control.disabled = !row.enabled;
    widget(
        world,
        state,
        controls,
        "Copy",
        frame,
        camera,
        control,
        node(8., 4., inner - 16., 28.),
        FeatureCommand::Control {
            form_id: panel.form_id,
            action: FeatureControl::Field(F::Copy),
        },
        theme,
        assets,
    )?;
    let component = panel.fields.iter().any(|row| {
        row.field == F::MoveObjectType
            && matches!(&row.value, Field::Choice{value,..} if value=="component")
    });
    let hint = crate::native_viewport::localization::translate(
        world,
        if component {
            "bodyFeature.createCopyComponentHint"
        } else {
            "bodyFeature.createCopyBodyHint"
        },
    );
    label(
        world,
        state,
        labels,
        "copy-card-hint",
        frame,
        camera,
        hint,
        node(31., 34., inner - 39., height - 38.),
        theme,
        assets,
        false,
    );
    world.entity_mut(state.labels["copy-card-hint"]).insert((
        theme.text(assets, 10., FontWeight::NORMAL),
        bevy::text::LineHeight::Px(16.),
    ));
    *y += height + 12.;
    Ok(())
}

fn prompt(world: &World, panel: &FeaturePanel) -> Option<String> {
    Some(match panel.pick_target? {
        F::Source if panel.kind == K::Extrude => {
            "Select closed profiles or a planar model face for Extrude".into()
        }
        F::Source if panel.kind == K::Loft => "Select closed sketch profiles in loft order".into(),
        F::Source => format!("Select closed sketch profiles for {}", panel.kind.label()),
        F::Edges => format!("Select model edges for {}", panel.kind.label()),
        F::Faces => "Select faces to remove for Shell".into(),
        F::Cylinder => "Select an exterior cylindrical face for External Thread".into(),
        F::HoleSupport => "Select a planar face for Hole".into(),
        F::HolePositions => "Pick the selected free position on its support face, or toggle a sketch point reference".into(),
        F::FirstPlane => "Select a planar face or reference plane".into(),
        F::SecondPlane => "Select another parallel face or reference plane".into(),
        F::AxisLine => "Select a straight sketch line for the Revolve axis".into(),
        F::AxisEdge | F::DirectionEdge | F::SecondDirectionEdge => {
            "Select a straight model edge".into()
        }
        F::Path | F::Guide => "Select finished sketch curves in the viewport".into(),
        F::Bodies if panel.kind == K::SplitBody => "Select the body to split".into(),
        F::Bodies => "Select bodies or a component occurrence in the viewport".into(),
        F::TargetBody if panel.kind == K::Combine => {
            crate::native_viewport::localization::translate(world, "bodyFeature.clickTargetBody").into()
        }
        F::ToolBodies if panel.kind == K::Combine => {
            crate::native_viewport::localization::translate(world, "bodyFeature.clickToolBodies").into()
        }
        F::Targets | F::TargetBody | F::ToolBodies => "Select target bodies in the viewport".into(),
        F::StopFace => "Select a planar stop face in the viewport".into(),
        field if field.is_move_point() => "Select a sketch point, body vertex or surface".into(),
        _ => return None,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn selection_prompt(
    world: &mut World,
    state: &mut PanelWidgets,
    labels: &mut HashSet<String>,
    root: Entity,
    camera: Entity,
    panel: &FeaturePanel,
    area: Area,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) {
    let Some(prompt) = prompt(world, panel) else {
        return;
    };
    let Some(canvas) = world
        .get_resource::<NativeInterfaceHandle>()
        .and_then(|h| h.frame())
        .and_then(|frame| {
            frame
                .canvases
                .iter()
                .find(|c| c.name == "viewport")
                .map(|c| c.bounds)
        })
    else {
        return;
    };
    let space = (area.x - canvas.x) as f32;
    if space < 180. {
        return;
    }
    let width = (prompt.chars().count() as f32 * 6. + 24.).min(space - 24.);
    let height = selection::text_height(&prompt, width - 24.) + 12.;
    let key = "viewport-selection-prompt";
    labels.insert(key.into());
    let frame = *state.decorations.entry(key.into()).or_insert_with(|| {
        let entity = world.spawn((UiTargetCamera(camera), ZIndex(39))).id();
        world.entity_mut(root).add_child(entity);
        entity
    });
    let mut bounds = node(
        (canvas.x - area.x) as f32 + (space - width) * 0.5,
        (canvas.y - area.y) as f32 + 12.,
        width,
        height,
    );
    bounds.border = UiRect::all(px(1.));
    bounds.border_radius = BorderRadius::all(px(4.));
    world.entity_mut(frame).insert((
        bounds,
        BackgroundColor(theme.header.with_alpha(0.9)),
        BorderColor::all(theme.edge),
    ));
    label(
        world,
        state,
        labels,
        "viewport-selection-prompt-text",
        frame,
        camera,
        &prompt,
        node(12., 6., width - 24., height - 12.),
        theme,
        assets,
        false,
    );
    world
        .entity_mut(state.labels["viewport-selection-prompt-text"])
        .insert((
            TextColor(theme.ink),
            theme.text(assets, 12., FontWeight::NORMAL),
        ));
}
