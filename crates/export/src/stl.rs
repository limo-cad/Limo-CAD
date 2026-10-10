//! STL writer: binary for exact f32 geometry, otherwise lossless f64 ASCII.

use std::fmt::Write;

use crate::{mesh_weld::validate_mesh_buffers, ExportError, TriangleMesh};

pub fn write_stl(meshes: &[TriangleMesh]) -> Result<Vec<u8>, ExportError> {
    if meshes.is_empty() {
        return Err(ExportError("There are no active bodies to export.".into()));
    }
    for mesh in meshes {
        validate_mesh_buffers(mesh)?;
    }
    // Binary STL stores f32 vertices. Use one ASCII artifact when any native
    // coordinate would change, rather than quantizing narrow source features.
    let binary = meshes.iter().all(|mesh| {
        mesh.positions.iter().all(|&value| {
            let represented = value as f32;
            represented.is_finite() && f64::from(represented) == value
        })
    });
    if !binary {
        return write_ascii_stl(meshes);
    }

    let triangle_count = meshes.iter().try_fold(0_u32, |count, mesh| {
        let additional = u32::try_from(mesh.triangle_count()).map_err(|_| {
            ExportError("STL triangle count exceeds the binary format limit".into())
        })?;
        count
            .checked_add(additional)
            .ok_or_else(|| ExportError("STL triangle count exceeds the binary format limit".into()))
    })?;
    let mut out = Vec::with_capacity(84 + triangle_count as usize * 50);
    let mut header = [0u8; 80];
    let label = b"Limo CAD binary STL (millimetres)";
    header[..label.len()].copy_from_slice(label);
    out.extend_from_slice(&header);
    out.extend_from_slice(&triangle_count.to_le_bytes());

    for mesh in meshes {
        for (triangle_index, tri) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
            let points = triangle_points(mesh, tri);
            let normal = checked_normal(mesh, triangle_index, points)?;
            for value in normal {
                out.extend_from_slice(&(value as f32).to_le_bytes());
            }
            for point in points {
                for value in point {
                    out.extend_from_slice(&(value as f32).to_le_bytes());
                }
            }
            out.extend_from_slice(&0u16.to_le_bytes());
        }
    }
    Ok(out)
}

fn write_ascii_stl(meshes: &[TriangleMesh]) -> Result<Vec<u8>, ExportError> {
    let mut out = String::from("solid LimoCAD\n");
    for mesh in meshes {
        for (triangle_index, tri) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
            let points = triangle_points(mesh, tri);
            let normal = checked_normal(mesh, triangle_index, points)?;
            // Display formatting emits shortest-roundtrip f64 decimals.
            writeln!(
                out,
                "  facet normal {} {} {}",
                normal[0], normal[1], normal[2]
            )
            .unwrap();
            out.push_str("    outer loop\n");
            for point in points {
                writeln!(out, "      vertex {} {} {}", point[0], point[1], point[2]).unwrap();
            }
            out.push_str("    endloop\n  endfacet\n");
        }
    }
    out.push_str("endsolid LimoCAD\n");
    Ok(out.into_bytes())
}

fn triangle_points(mesh: &TriangleMesh, indices: &[u32; 3]) -> [[f64; 3]; 3] {
    indices.map(|index| {
        let begin = index as usize * 3;
        [
            mesh.positions[begin],
            mesh.positions[begin + 1],
            mesh.positions[begin + 2],
        ]
    })
}

fn checked_normal(
    mesh: &TriangleMesh,
    triangle_index: usize,
    [a, b, c]: [[f64; 3]; 3],
) -> Result<[f64; 3], ExportError> {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let normal = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = normal[0].hypot(normal[1]).hypot(normal[2]);
    if !length.is_finite() || length == 0.0 {
        return Err(ExportError(format!(
            "body {} triangle {}: STL vertices form a zero-area or non-finite triangle",
            mesh.body_id.0, triangle_index
        )));
    }
    Ok(normal.map(|value| value / length))
}
