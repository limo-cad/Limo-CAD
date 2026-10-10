//! Opt-in owned-window capture evidence. No model/kernel access or UI changes.
use super::*;
use bevy::camera::visibility::RenderLayers;
use bevy::ui::{CalculatedClip, ComputedStackIndex, ComputedUiTargetCamera, UiGlobalTransform};

fn component<T: Component + std::fmt::Debug>(world: &World, entity: Entity) -> Value {
    world
        .get::<T>(entity)
        .map(|v| json!(format!("{v:?}")))
        .unwrap_or(Value::Null)
}
fn node(world: &World, entity: Entity) -> Value {
    let corners = world
        .get::<ComputedNode>(entity)
        .zip(world.get::<UiGlobalTransform>(entity))
        .map(|(node, transform)| {
            let r = node.border_box();
            [
                r.min,
                Vec2::new(r.max.x, r.min.y),
                r.max,
                Vec2::new(r.min.x, r.max.y),
            ]
            .map(|point| transform.transform_point2(point).to_array())
        });
    json!({
        "entity":format!("{entity:?}"),"node":component::<Node>(world,entity),
        "computed":component::<ComputedNode>(world,entity),"physical_corners":corners,
        "computed_size":world.get::<ComputedNode>(entity).map(|n|n.size().to_array()),
        "transform":component::<UiGlobalTransform>(world,entity),
        "clip":component::<CalculatedClip>(world,entity),
        "visibility":component::<Visibility>(world,entity),
        "inherited_visibility":component::<InheritedVisibility>(world,entity),
        "view_visibility":component::<ViewVisibility>(world,entity),
        "z_index":component::<ZIndex>(world,entity),
        "stack_index":world.get::<ComputedStackIndex>(entity).map(|index| index.0),
        "camera":component::<UiTargetCamera>(world,entity),
        "computed_camera":component::<ComputedUiTargetCamera>(world,entity),
        "render_layers":component::<RenderLayers>(world,entity),
        "background":component::<BackgroundColor>(world,entity),
        "image":component::<ImageNode>(world,entity),
        "parent":world.get::<ChildOf>(entity).map(|p|format!("{:?}",p.parent()))
    })
}
pub(in super::super::super) fn snapshot(world: &World, state: &Workbench) -> Option<Value> {
    let view = state.paper_view.as_ref()?;
    let transform = view.navigation.transform();
    let mut nodes = Vec::new();
    for key in [
        "drawing-backdrop",
        "drawing-content-clip",
        "drawing-paper",
        "drawing-projected-edges",
        "drawing-render-error",
    ] {
        let Some(entity) = state.widgets.entity(key) else {
            nodes.push(json!({"key":key,"missing":true}));
            continue;
        };
        let mut row = node(world, entity);
        row["key"] = json!(key);
        let mut parents = Vec::new();
        let mut current = entity;
        for _ in 0..8 {
            let Some(parent) = world.get::<ChildOf>(current).map(ChildOf::parent) else {
                break;
            };
            parents.push(node(world, parent));
            current = parent;
        }
        row["parents"] = json!(parents);
        nodes.push(row);
    }
    Some(json!({
        "owner":state.owner.as_ref().map(|owner| json!({"window_id":owner.window_id,"document_id":owner.document_id,"epoch":owner.epoch})),"paper_ready":state.paper_key.is_some(),
        "annotation_segments":state.paper.len(),"annotation_labels":state.paper_labels.len(),
        "client_size":[view.width,view.height],"fitted":view.navigation.fitted,
        "paper_origin":transform.origin,"paper_scale":transform.scale,"sheet_mm":transform.sheet_mm,
        "sample_logical":transform.to_screen([3.,3.]),
        "sample_intent":"3 mm inside fitted sheet top-left, before its 5 mm frame",
        "nodes":nodes,
        "render_camera":component::<Camera>(world,view.camera),
        "render_camera_layers":component::<RenderLayers>(world,view.camera),
        "art_error":view.art_failure.as_ref().map(|failure| &failure.error)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_zero_size_hidden_and_fully_clipped_nodes_are_retained_in_evidence() {
        let mut world = World::new();
        let entity = world
            .spawn((
                Node::default(),
                Visibility::Hidden,
                CalculatedClip::FullyClipped,
                ComputedStackIndex(37),
            ))
            .id();
        let captured = node(&world, entity);
        assert_eq!(captured["visibility"], "Hidden");
        assert_eq!(captured["clip"], "FullyClipped");
        assert_eq!(captured["computed_size"], json!([0., 0.]));
        assert_eq!(captured["stack_index"], 37);
        assert!(captured["physical_corners"].is_array());
    }
}
