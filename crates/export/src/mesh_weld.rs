//! Export-time vertex welding and topology checks for indexed triangle meshes.
//!
//! OCCT tessellation emits three fresh positions per triangle with no shared
//! vertex indices. A 3MF `type="model"` mesh requires each edge to be shared,
//! by vertex index, with exactly one oppositely oriented neighboring triangle.

use std::collections::HashMap;

use crate::{ExportError, TriangleMesh};

type Cell = (i64, i64, i64);

/// Default position tolerance for welding tessellated millimetre geometry.
pub const DEFAULT_WELD_EPSILON: f32 = 1e-5;

/// Merge duplicate vertices without masking malformed source buffers.
///
/// Already-valid indexed meshes are preserved exactly. Triangle-soup meshes
/// first merge only identical coordinates, preserving thin native geometry.
/// If that does not form a valid solid, spatial welding uses neighboring hash
/// cells, so points within `epsilon` match across cell boundaries.
pub fn weld_triangle_mesh(mesh: &TriangleMesh, epsilon: f32) -> Result<TriangleMesh, ExportError> {
    validate_mesh_buffers(mesh)?;
    if !epsilon.is_finite() || epsilon <= 0.0 {
        return Err(ExportError(
            "mesh weld epsilon must be finite and greater than zero".into(),
        ));
    }
    if validate_3mf_model_mesh(mesh).is_ok() {
        return Ok(mesh.clone());
    }

    let exact = weld_exact_coordinates(mesh);
    if validate_3mf_model_mesh(&exact).is_ok() {
        return Ok(exact);
    }

    let vertex_count = mesh.positions.len() / 3;
    let epsilon64 = f64::from(epsilon);
    let epsilon_squared = epsilon64 * epsilon64;
    let mut cells: HashMap<Cell, Vec<u32>> = HashMap::new();
    let mut welded_positions = Vec::with_capacity(mesh.positions.len());
    let mut index_remap = Vec::with_capacity(vertex_count);

    for old_index in 0..vertex_count {
        let base = old_index * 3;
        let point = [
            mesh.positions[base],
            mesh.positions[base + 1],
            mesh.positions[base + 2],
        ];
        let cell = point_cell(point, epsilon64)?;
        let mut matched_index: Option<u32> = None;

        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let neighbor = (
                        cell.0.saturating_add(dx),
                        cell.1.saturating_add(dy),
                        cell.2.saturating_add(dz),
                    );
                    let Some(candidates) = cells.get(&neighbor) else {
                        continue;
                    };
                    for &candidate in candidates {
                        let candidate_base = candidate as usize * 3;
                        let squared_distance = (0..3)
                            .map(|axis| {
                                let delta = point[axis] - welded_positions[candidate_base + axis];
                                delta * delta
                            })
                            .sum::<f64>();
                        if squared_distance <= epsilon_squared
                            && matched_index.is_none_or(|current| candidate < current)
                        {
                            matched_index = Some(candidate);
                        }
                    }
                }
            }
        }

        let new_index = matched_index.unwrap_or_else(|| {
            let index = (welded_positions.len() / 3) as u32;
            welded_positions.extend_from_slice(&point);
            cells.entry(cell).or_default().push(index);
            index
        });
        index_remap.push(new_index);
    }

    let welded_indices = mesh
        .indices
        .iter()
        .map(|&index| index_remap[index as usize])
        .collect();

    Ok(TriangleMesh {
        body_id: mesh.body_id,
        name: mesh.name.clone(),
        positions: welded_positions,
        indices: welded_indices,
    })
}

/// Preserve every triangle and coordinate while indexing exact coincidences.
/// Buffer finiteness and index validity have already been checked by the caller.
fn weld_exact_coordinates(mesh: &TriangleMesh) -> TriangleMesh {
    let mut coordinates: HashMap<[u64; 3], u32> = HashMap::new();
    let mut positions = Vec::with_capacity(mesh.positions.len());
    let mut remap = Vec::with_capacity(mesh.positions.len() / 3);
    for point in mesh.positions.as_chunks::<3>().0 {
        // Signed zero has one geometric position, although its IEEE bits differ.
        let key = point.map(|coordinate| {
            if coordinate == 0.0 {
                0
            } else {
                coordinate.to_bits()
            }
        });
        let index = *coordinates.entry(key).or_insert_with(|| {
            let index = (positions.len() / 3) as u32;
            positions.extend_from_slice(point);
            index
        });
        remap.push(index);
    }
    TriangleMesh {
        body_id: mesh.body_id,
        name: mesh.name.clone(),
        positions,
        indices: mesh
            .indices
            .iter()
            .map(|&index| remap[index as usize])
            .collect(),
    }
}

/// Reject buffers that cannot be represented safely in STL or 3MF.
pub fn validate_mesh_buffers(mesh: &TriangleMesh) -> Result<(), ExportError> {
    if !mesh.positions.len().is_multiple_of(3) {
        return Err(ExportError(format!(
            "body {} has a malformed position buffer",
            mesh.body_id.0
        )));
    }
    if !mesh.indices.len().is_multiple_of(3) {
        return Err(ExportError(format!(
            "body {} has a malformed index buffer",
            mesh.body_id.0
        )));
    }
    if mesh
        .positions
        .iter()
        .any(|coordinate| !coordinate.is_finite())
    {
        return Err(ExportError(format!(
            "body {} has a non-finite vertex coordinate",
            mesh.body_id.0
        )));
    }
    let vertex_count = mesh.positions.len() / 3;
    if mesh
        .indices
        .iter()
        .any(|&index| index as usize >= vertex_count)
    {
        return Err(ExportError(format!(
            "body {} has an out-of-range triangle index",
            mesh.body_id.0
        )));
    }
    Ok(())
}

/// Validate the edge-sharing and winding requirements for a 3MF model mesh.
pub fn validate_3mf_model_mesh(mesh: &TriangleMesh) -> Result<(), ExportError> {
    validate_mesh_buffers(mesh)?;
    if mesh.triangle_count() < 4 {
        return Err(ExportError(format!(
            "body {} has fewer than four triangles and cannot form a 3MF solid",
            mesh.body_id.0
        )));
    }
    if mesh.indices.as_chunks::<3>().0.iter().any(|triangle| {
        triangle[0] == triangle[1] || triangle[1] == triangle[2] || triangle[2] == triangle[0]
    }) {
        return Err(ExportError(format!(
            "body {} contains a degenerate triangle after welding",
            mesh.body_id.0
        )));
    }
    for (triangle_index, triangle) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
        let points = triangle.map(|index| {
            let base = index as usize * 3;
            [
                mesh.positions[base],
                mesh.positions[base + 1],
                mesh.positions[base + 2],
            ]
        });
        let u: [f64; 3] = std::array::from_fn(|axis| points[1][axis] - points[0][axis]);
        let v: [f64; 3] = std::array::from_fn(|axis| points[2][axis] - points[0][axis]);
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let area_squared = cross.iter().map(|value| value * value).sum::<f64>();
        if !area_squared.is_finite() || area_squared == 0.0 {
            return Err(ExportError(format!(
                "body {} triangle {triangle_index} has zero-area or non-finite geometry after welding",
                mesh.body_id.0
            )));
        }
    }
    let invalid_edges = invalid_model_edge_count(mesh);
    if invalid_edges != 0 {
        return Err(ExportError(format!(
            "body {} is not a closed, consistently oriented 2-manifold \
             after welding ({invalid_edges} invalid edges)",
            mesh.body_id.0
        )));
    }
    if signed_volume_six(mesh) <= 0.0 {
        return Err(ExportError(format!(
            "body {} does not have outward-facing triangles with positive volume",
            mesh.body_id.0
        )));
    }
    Ok(())
}

/// Count undirected edges referenced by exactly one triangle.
pub fn boundary_edge_count(mesh: &TriangleMesh) -> usize {
    edge_uses(mesh)
        .values()
        .filter(|edge| edge.count == 1)
        .count()
}

/// Count edges that violate 3MF's two-triangle/opposite-winding requirement.
pub fn invalid_model_edge_count(mesh: &TriangleMesh) -> usize {
    edge_uses(mesh)
        .values()
        .filter(|edge| edge.count != 2 || edge.orientation_balance != 0)
        .count()
}

#[derive(Default)]
struct EdgeUse {
    count: usize,
    orientation_balance: i32,
}

fn edge_uses(mesh: &TriangleMesh) -> HashMap<(u32, u32), EdgeUse> {
    let mut uses = HashMap::new();
    for triangle in mesh.indices.as_chunks::<3>().0 {
        for (from, to) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let key = if from <= to { (from, to) } else { (to, from) };
            let edge: &mut EdgeUse = uses.entry(key).or_default();
            edge.count += 1;
            edge.orientation_balance += if from < to {
                1
            } else if from > to {
                -1
            } else {
                2
            };
        }
    }
    uses
}

fn point_cell(point: [f64; 3], epsilon: f64) -> Result<Cell, ExportError> {
    let mut coordinates = [0_i64; 3];
    for axis in 0..3 {
        let scaled = (point[axis] / epsilon).floor();
        if scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
            return Err(ExportError(
                "mesh coordinate is too large for the weld tolerance".into(),
            ));
        }
        coordinates[axis] = scaled as i64;
    }
    Ok((coordinates[0], coordinates[1], coordinates[2]))
}

fn signed_volume_six(mesh: &TriangleMesh) -> f64 {
    let origin = [mesh.positions[0], mesh.positions[1], mesh.positions[2]];
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| {
            let point = |index: u32| {
                let base = index as usize * 3;
                [
                    mesh.positions[base] - origin[0],
                    mesh.positions[base + 1] - origin[1],
                    mesh.positions[base + 2] - origin[2],
                ]
            };
            let a = point(triangle[0]);
            let b = point(triangle[1]);
            let c = point(triangle[2]);
            let cross = [
                b[1] * c[2] - b[2] * c[1],
                b[2] * c[0] - b[0] * c[2],
                b[0] * c[1] - b[1] * c[0],
            ];
            a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use limo_cad_core::BodyId;

    #[test]
    fn weld_rejects_out_of_range_triangle_indices() {
        let mesh = TriangleMesh {
            body_id: BodyId(1),
            name: "bad".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            indices: vec![0, 1, 2, 0, 1, 99],
        };
        let error = weld_triangle_mesh(&mesh, DEFAULT_WELD_EPSILON).unwrap_err();
        assert!(error.0.contains("out-of-range"));
    }

    #[test]
    fn weld_checks_neighboring_hash_cells() {
        let mesh = TriangleMesh {
            body_id: BodyId(1),
            name: "cell-boundary".into(),
            positions: vec![
                0.049, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0, 0.051, 0.0, 0.0, 0.0, 10.0, 0.0,
                10.0, 0.0, 0.0,
            ],
            indices: vec![0, 1, 2, 3, 4, 5],
        };
        let welded = weld_triangle_mesh(&mesh, 0.1).unwrap();
        assert_eq!(welded.positions.len() / 3, 3);
        assert_eq!(welded.indices, vec![0, 1, 2, 0, 2, 1]);
    }

    #[test]
    fn topology_check_rejects_same_direction_shared_edge() {
        let mesh = TriangleMesh {
            body_id: BodyId(1),
            name: "bad-winding".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2, 0, 1, 3, 0, 2, 3, 1, 2, 3],
        };
        assert!(invalid_model_edge_count(&mesh) > 0);
        assert!(validate_3mf_model_mesh(&mesh).is_err());
    }

    #[test]
    fn topology_check_rejects_an_inverted_closed_mesh() {
        let mesh = TriangleMesh {
            body_id: BodyId(1),
            name: "inverted-tetrahedron".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2, 0, 3, 1, 0, 2, 3, 1, 3, 2],
        };
        assert_eq!(invalid_model_edge_count(&mesh), 0);
        assert!(validate_3mf_model_mesh(&mesh).is_err());
    }
}
