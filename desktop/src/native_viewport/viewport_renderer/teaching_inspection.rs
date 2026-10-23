//! Presentation of real retained native results on the teaching render layer only.
use super::*;
use crate::native_viewport::script_preview::inspection::Inspection;

fn mesh(positions: &[f32], normals: &[f32], indices: Vec<u32>) -> Option<Mesh> {
    if indices.is_empty() {
        return None;
    }
    let mut out = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    out.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        positions.as_chunks::<3>().0.to_vec(),
    );
    out.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.as_chunks::<3>().0.to_vec());
    out.insert_indices(Indices::U32(indices));
    Some(out)
}

pub(super) fn meshes(
    body: &limo_cad_solid::BodyDto,
    inspection: Option<&Inspection>,
) -> Vec<(Mesh, bool)> {
    let Some(i) = inspection else {
        return body_mesh(body).into_iter().map(|m| (m, false)).collect();
    };
    if i.visibility
        .as_ref()
        .is_some_and(|v| v.hidden_body_ids.contains(&body.id.0))
    {
        return vec![];
    }
    if let Some(s) = i.section.as_ref().filter(|s| s.request.body_id == body.id) {
        // Use the returned capped OCCT half-solid verbatim; never clip or manufacture triangles.
        let Some(b) = &s.cutaway else { return vec![] };
        let axis = s.request.plane.axis();
        let mut plain = Vec::new();
        let mut caps = Vec::new();
        for f in &b.faces {
            let cap = f.plane.is_some_and(|p| {
                p.normal[axis].abs() > 0.999999
                    && (p.origin[axis] - s.request.offset_mm).abs() < 1e-6
            });
            let target = if cap { &mut caps } else { &mut plain };
            target.extend_from_slice(
                &b.indices[f.first_index as usize..(f.first_index + f.index_count) as usize],
            );
        }
        return [(plain, false), (caps, true)]
            .into_iter()
            .filter_map(|(idx, cap)| mesh(&b.positions, &b.normals, idx).map(|m| (m, cap)))
            .collect();
    }
    let Some(selected) = &i.selection else {
        return body_mesh(body).into_iter().map(|m| (m, false)).collect();
    };
    if selected.body_ids.contains(&body.id.0) {
        return body_mesh(body).into_iter().map(|m| (m, true)).collect();
    }
    let faces = body
        .faces
        .iter()
        .filter(|f| selected.face_ids.contains(&f.id.0))
        .collect::<Vec<_>>();
    if faces.is_empty() {
        return body_mesh(body).into_iter().map(|m| (m, false)).collect();
    }
    let mut plain = Vec::new();
    let mut accent = Vec::new();
    for (offset, triangle) in body.mesh.indices.chunks_exact(3).enumerate() {
        let chosen = faces.iter().any(|f| {
            offset * 3 >= f.first_index as usize
                && offset * 3 < (f.first_index + f.index_count) as usize
        });
        (if chosen { &mut accent } else { &mut plain }).extend_from_slice(triangle);
    }
    [(plain, false), (accent, true)]
        .into_iter()
        .filter_map(|(idx, chosen)| {
            mesh(&body.mesh.positions, &body.mesh.normals, idx).map(|m| (m, chosen))
        })
        .collect()
}

pub(super) fn edge_selected(
    i: Option<&Inspection>,
    body: &limo_cad_solid::BodyDto,
    edge: &limo_cad_solid::EdgeDto,
) -> bool {
    i.and_then(|i| i.selection.as_ref()).is_some_and(|s| {
        s.body_ids.contains(&body.id.0)
            || body
                .faces
                .iter()
                .any(|f| s.face_ids.contains(&f.id.0) && f.edge_keys.contains(&edge.key))
    })
}
