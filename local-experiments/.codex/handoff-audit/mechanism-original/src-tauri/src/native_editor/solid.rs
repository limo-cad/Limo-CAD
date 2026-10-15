//! Ordinary solid selection uses the retained renderer's camera and topology.
//! Feature pickers keep priority; no model is cloned or rebuilt for hovering.
use super::*;
use crate::native_viewport::{NativePick, NativePickPurpose, ViewportPresentation};
use crate::session_bridge::native_interface::{clear_selection, feature};

fn pick(
    world: &World,
    owner: &DocumentContext,
    point: [f32; 2],
) -> Result<Option<NativePick>, String> {
    let edge =
        native_viewport::interface_pick(world, &owner.document_id, point, NativePickPurpose::Edge)?;
    if edge.is_some() {
        return Ok(edge);
    }
    native_viewport::interface_pick(
        world,
        &owner.document_id,
        point,
        NativePickPurpose::Geometry,
    )
}
pub(super) fn allowed(world: &World) -> bool {
    feature::panel(world).is_none() && !crate::session_bridge::native_interface::controller::assembly::joint::active(world)
        && native_viewport::interface_geometry(world)
            .active_sketch
            .is_none()
}
pub(super) fn hover(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    point: Option<[f32; 2]>,
) -> Result<(), String> {
    if !allowed(world) {
        return Ok(());
    }
    services
        .bridge
        .with_native_document_owner(&services.engine, owner, || {
            let hit = point.map(|p| pick(world, owner, p)).transpose()?.flatten();
            let (_, _, mut view, _) = native_viewport::interface_view_snapshot(world);
            view.hovered_body_id = hit.as_ref().map(|h| h.body_id);
            view.hovered_occurrence_id = hit.as_ref().and_then(|h| h.occurrence_id);
            view.hovered_face_id = hit
                .as_ref()
                .filter(|h| h.edge_id.is_none())
                .map(|h| h.face_id);
            view.hovered_edge_id = hit.and_then(|h| h.edge_id);
            native_viewport::apply_interface_view(world, &owner.document_id, None, Some(view))
        })
}
fn toggle(ids: &mut Vec<u64>, id: u64, additive: bool) {
    if additive && ids.contains(&id) {
        ids.retain(|v| *v != id);
    } else if !ids.contains(&id) {
        ids.push(id);
    }
}
fn selection(
    view: &mut ViewportPresentation,
    hit: Option<&NativePick>,
    additive: bool,
    scene: &nbcad_solid::SolidSceneDto,
) {
    // Selection IDs are source-local. Additive selection cannot alias the
    // same face ID across two occurrences; starting in another instance resets it.
    let replace = !additive || hit.is_some_and(|h| view.selected_occurrence_id != h.occurrence_id);
    if replace {
        clear_selection(view);
    }
    let Some(hit) = hit else {
        return;
    };
    view.selected_occurrence_id = hit.occurrence_id;
    if let Some(edge) = hit.edge_id {
        toggle(&mut view.selected_edge_ids, edge, additive);
        view.selected_surface_point = None;
    } else {
        toggle(&mut view.selected_face_ids, hit.face_id, additive);
        view.selected_surface_point =
            view.selected_face_ids
                .contains(&hit.face_id)
                .then_some(nbcad_solid::Point3Dto {
                    x: hit.point[0] as f64,
                    y: hit.point[1] as f64,
                    z: hit.point[2] as f64,
                });
    }
    view.selected_body_ids = scene
        .bodies
        .iter()
        .filter(|b| {
            b.faces
                .iter()
                .any(|f| view.selected_face_ids.contains(&f.id.0))
                || b.edges
                    .iter()
                    .any(|e| view.selected_edge_ids.contains(&e.id.0))
        })
        .map(|b| b.id.0)
        .collect();
    if view.selected_body_ids.is_empty() {
        view.selected_occurrence_id = None;
    }
}
pub(super) fn select(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    point: [f32; 2],
    additive: bool,
) -> Result<Value, String> {
    if !allowed(world) {
        return Ok(json!({"handled":true}));
    }
    services.bridge.with_native_document_owner(&services.engine, owner, || {
        let hit = pick(world, owner, point)?;
        let (_, _, mut view, _) = native_viewport::interface_view_snapshot(world);
        selection(&mut view, hit.as_ref(), additive, native_viewport::interface_geometry(world).scene);
        let result = json!({"handled":true,"selection":{"bodies":view.selected_body_ids,"faces":view.selected_face_ids,"edges":view.selected_edge_ids,"occurrence":view.selected_occurrence_id}});
        native_viewport::apply_interface_view(world, &owner.document_id, None, Some(view))?;
        Ok(result)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn face_and_edge_selection_toggle_clear_and_do_not_alias_occurrences() {
        let scene: nbcad_solid::SolidSceneDto = serde_json::from_value(json!({"bodies":[{"id":1,"feature_id":1,"name":"Plate","mesh":{"positions":[],"normals":[],"indices":[]},"faces":[{"id":10,"key":"front","first_index":0,"index_count":0}],"edges":[{"id":2,"key":"edge","points":[]}]}],"errors":[]})).unwrap();
        let mut hit = NativePick {
            body_id: 1,
            occurrence_id: Some(42),
            face_id: 10,
            edge_id: None,
            point: [1., 2., 3.],
            distance: 1.,
            connector_kind: None,
            connector_origin: None,
            connector_primary_axis: None,
            connector_secondary_axis: None,
            connector_radius: None,
        };
        let mut view = ViewportPresentation::default();
        selection(&mut view, Some(&hit), false, &scene);
        assert_eq!(view.selected_face_ids, vec![10]);
        assert_eq!(view.selected_body_ids, vec![1]);
        assert_eq!(view.selected_surface_point.unwrap().z, 3.);
        selection(&mut view, None, true, &scene);
        assert_eq!(view.selected_face_ids, vec![10]);
        hit.edge_id = Some(2);
        selection(&mut view, Some(&hit), true, &scene);
        assert_eq!(view.selected_face_ids, vec![10]);
        assert_eq!(view.selected_edge_ids, vec![2]);
        selection(&mut view, Some(&hit), true, &scene);
        assert!(view.selected_edge_ids.is_empty());
        assert_eq!(view.selected_body_ids, vec![1]);
        hit.edge_id = None;
        hit.occurrence_id = Some(43);
        selection(&mut view, Some(&hit), true, &scene);
        assert_eq!(view.selected_occurrence_id, Some(43));
        assert_eq!(view.selected_face_ids, vec![10]);
        selection(&mut view, Some(&hit), true, &scene);
        assert!(view.selected_body_ids.is_empty());
        assert_eq!(view.selected_occurrence_id, None);
        selection(&mut view, Some(&hit), false, &scene);
        selection(&mut view, None, false, &scene);
        assert!(view.selected_body_ids.is_empty());
        assert!(view.selected_face_ids.is_empty());
        assert!(view.selected_surface_point.is_none());
    }
}
