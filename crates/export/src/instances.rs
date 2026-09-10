//! Place retained body meshes in the solved assembly without retessellating them.
use crate::{
    mesh_weld::validate_mesh_buffers, weld_triangle_mesh, ExportError, TriangleMesh,
    DEFAULT_WELD_EPSILON,
};
use nbcad_core::BodyId;

pub struct MeshInstance {
    pub body_id: BodyId,
    pub occurrence_id: u64,
    pub translation: [f64; 3],
    /// Quaternion x/y/z/w.
    pub rotation: [f64; 4],
    pub visible: bool,
}

pub fn place_mesh_instances(
    meshes: &[TriangleMesh],
    instances: &[MeshInstance],
) -> Result<Vec<TriangleMesh>, ExportError> {
    // The solved occurrence list is authoritative, including an empty list.
    // Legacy body-only projects are promoted to root occurrences by the
    // assembly solver before reaching export. A missing placement here means
    // an unused definition, not a standalone part at its authoring origin.
    let mut output = Vec::new();
    for source in meshes {
        let placements: Vec<_> = instances
            .iter()
            .filter(|p| p.body_id == source.body_id)
            .collect();
        if placements.is_empty() {
            continue;
        }
        // Weld in part coordinates before f32 assembly placement can amplify seam rounding.
        let indexed = weld_triangle_mesh(source, DEFAULT_WELD_EPSILON)?;
        for p in placements.into_iter().filter(|p| p.visible) {
            let norm = p.rotation.iter().map(|x| x * x).sum::<f64>().sqrt();
            if !norm.is_finite() || norm < 1e-12 || p.translation.iter().any(|x| !x.is_finite()) {
                return Err(ExportError(format!(
                    "Invalid export pose for occurrence {}",
                    p.occurrence_id
                )));
            }
            let [x, y, z, w] = p.rotation.map(|x| x / norm);
            let mut mesh = indexed.clone();
            mesh.name = format!("{} (instance {})", source.name, p.occurrence_id);
            for v in mesh.positions.chunks_exact_mut(3) {
                let a = f64::from(v[0]);
                let b = f64::from(v[1]);
                let c = f64::from(v[2]);
                v[0] = ((1. - 2. * (y * y + z * z)) * a
                    + 2. * (x * y - z * w) * b
                    + 2. * (x * z + y * w) * c
                    + p.translation[0]) as f32;
                v[1] = (2. * (x * y + z * w) * a
                    + (1. - 2. * (x * x + z * z)) * b
                    + 2. * (y * z - x * w) * c
                    + p.translation[1]) as f32;
                v[2] = (2. * (x * z - y * w) * a
                    + 2. * (y * z + x * w) * b
                    + (1. - 2. * (x * x + y * y)) * c
                    + p.translation[2]) as f32;
            }
            validate_mesh_buffers(&mesh)?;
            output.push(mesh);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tetra() -> TriangleMesh {
        TriangleMesh {
            body_id: BodyId(1),
            name: "Part".into(),
            positions: vec![0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1.],
            indices: vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        }
    }
    fn instance(id: u64) -> MeshInstance {
        MeshInstance {
            body_id: BodyId(1),
            occurrence_id: id,
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
            visible: true,
        }
    }
    #[test]
    fn repeated_rotated_parts_export_in_place_without_changing_the_source() {
        let source = tetra();
        let mut second = instance(2);
        second.translation = [10., 20., 30.];
        let half = std::f64::consts::FRAC_1_SQRT_2;
        second.rotation = [0., 0., half, half];
        let out =
            place_mesh_instances(std::slice::from_ref(&source), &[instance(1), second]).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].positions, source.positions);
        assert_eq!(&out[1].positions[3..6], &[10., 21., 30.]);
        assert_eq!(&out[1].positions[6..9], &[9., 20., 30.]);
        for mesh in &out {
            crate::validate_3mf_model_mesh(mesh).unwrap();
        }
        assert_eq!(out[1].body_id, source.body_id); // Material lookup retains definition identity.
        assert_eq!(source, tetra());
    }
    #[test]
    fn hidden_instances_do_not_leak_and_invalid_poses_fail() {
        let mut hidden = instance(1);
        hidden.visible = false;
        assert!(place_mesh_instances(&[tetra()], &[hidden])
            .unwrap()
            .is_empty());
        let mut invalid = instance(1);
        invalid.rotation = [0.; 4];
        assert!(place_mesh_instances(&[tetra()], &[invalid]).is_err());
        assert!(place_mesh_instances(&[tetra()], &[]).unwrap().is_empty());
    }
    #[test]
    fn unused_definitions_are_not_exported_beside_visible_instances() {
        let placed = tetra();
        let mut unused = tetra();
        unused.body_id = BodyId(2);
        unused.name = "Reusable definition without an occurrence".into();
        let mut pose = instance(1);
        pose.translation = [50., 0., 0.];
        let output = place_mesh_instances(&[placed, unused], &[pose]).unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].body_id, BodyId(1));
        assert_eq!(&output[0].positions[0..3], &[50., 0., 0.]);
    }
}
