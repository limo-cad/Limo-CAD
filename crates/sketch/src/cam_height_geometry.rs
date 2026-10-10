//! Independent CAM height references. Invalid or removed geometry fails closed.
use crate::dto::{EntityDto, SketchDto};
use limo_cad_cam::{CamHeightGeometryDto, CamSetupDto};
use limo_cad_solid::SolidSceneDto;

pub fn resolve(
    reference: &CamHeightGeometryDto,
    setup: &CamSetupDto,
    scene: &SolidSceneDto,
    sketches: &[SketchDto],
) -> Result<f64, String> {
    let invalid = || {
        "Height reference is missing or no longer lies in one setup-Z plane. Reselect its geometry."
            .to_string()
    };
    let points: Vec<[f64; 3]> = match reference {
        CamHeightGeometryDto::Face { body_id, key }
        | CamHeightGeometryDto::Edge { body_id, key }
        | CamHeightGeometryDto::Vertex { body_id, key, .. } => {
            let body = scene
                .bodies
                .iter()
                .find(|b| b.id.0 == *body_id && setup.body_ids.contains(&b.id))
                .ok_or_else(invalid)?;
            if matches!(reference, CamHeightGeometryDto::Face { .. }) {
                let plane = body
                    .faces
                    .iter()
                    .find(|f| f.key == *key)
                    .and_then(|f| f.plane.as_ref())
                    .ok_or_else(invalid)?;
                let dot: f64 = plane
                    .normal
                    .iter()
                    .zip(setup.wcs.z_axis)
                    .map(|(a, b)| a * b)
                    .sum();
                if !dot.is_finite() || (dot.abs() - 1.0).abs() > 1e-6 {
                    return Err(invalid());
                }
                vec![plane.origin]
            } else {
                let edge = body
                    .edges
                    .iter()
                    .find(|e| e.key == *key)
                    .ok_or_else(invalid)?;
                if let CamHeightGeometryDto::Vertex { end, .. } = reference {
                    let p = if *end {
                        edge.points.last()
                    } else {
                        edge.points.first()
                    }
                    .ok_or_else(invalid)?;
                    vec![[p.x, p.y, p.z]]
                } else {
                    edge.points.iter().map(|p| [p.x, p.y, p.z]).collect()
                }
            }
        }
        CamHeightGeometryDto::SketchPoint { sketch, entity_id }
        | CamHeightGeometryDto::SketchLine { sketch, entity_id } => {
            let sketch = sketches
                .iter()
                .find(|s| s.name == *sketch)
                .ok_or_else(invalid)?;
            let entity = sketch
                .entities
                .iter()
                .find(|e| e.id().0 == *entity_id)
                .ok_or_else(invalid)?;
            let uv = match (reference, entity) {
                (CamHeightGeometryDto::SketchPoint { .. }, EntityDto::Point { position, .. }) => {
                    vec![*position]
                }
                (
                    CamHeightGeometryDto::SketchLine { .. },
                    EntityDto::Line {
                        start,
                        end,
                        consumed: false,
                        ..
                    },
                ) => vec![*start, *end],
                _ => return Err(invalid()),
            };
            uv.iter()
                .map(|p| {
                    std::array::from_fn(|i| {
                        sketch.basis.origin[i] + sketch.basis.u[i] * p.x + sketch.basis.v[i] * p.y
                    })
                })
                .collect()
        }
    };
    let origin = [setup.wcs.origin.x, setup.wcs.origin.y, setup.wcs.origin.z];
    let levels: Vec<f64> = points
        .iter()
        .map(|p| {
            (0..3)
                .map(|i| (p[i] - origin[i]) * setup.wcs.z_axis[i])
                .sum()
        })
        .collect();
    let z = *levels.first().ok_or_else(invalid)?;
    if levels
        .iter()
        .any(|v| !v.is_finite() || (v - z).abs() > 1e-4)
    {
        return Err(invalid());
    }
    Ok(z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (CamSetupDto, SolidSceneDto) {
        let setup = serde_json::from_value(json!({
            "id":1,"name":"Test","wcs":{"origin":{"x":0,"y":0,"z":2},"x_axis":[1,0,0],"y_axis":[0,1,0],"z_axis":[0,0,1]},
            "work_offset":"g54","work_offset_count":1,"wcs_origin":{"mode":"explicit"},
            "stock_spec":{"mode":"legacy_box"},"resolved_stock":{"shape":"box"},
            "stock":{"min":{"x":0,"y":0,"z":-2},"max":{"x":10,"y":10,"z":10}},"body_ids":[1],"operations":[]
        })).unwrap();
        let scene = serde_json::from_value(json!({"errors":[],"bodies":[{
            "id":1,"name":"Block","feature_id":1,"mesh":{"positions":[],"normals":[],"indices":[]},
            "faces":[{"id":1,"key":"floor","first_index":0,"index_count":0,"plane":{"origin":[0,0,5],"u":[1,0,0],"v":[0,1,0],"normal":[0,0,1]}}],
            "edges":[{"id":1,"key":"rim","points":[{"x":0,"y":0,"z":5},{"x":10,"y":0,"z":5}]},
                     {"id":2,"key":"upright","points":[{"x":0,"y":0,"z":0},{"x":0,"y":0,"z":8}]}]
        }]})).unwrap();
        (setup, scene)
    }
    #[test]
    fn face_edge_vertex_resolve_in_setup_frame_and_follow_current_geometry() {
        let (setup, mut scene) = fixture();
        let face = CamHeightGeometryDto::Face {
            body_id: 1,
            key: "floor".into(),
        };
        let edge = CamHeightGeometryDto::Edge {
            body_id: 1,
            key: "rim".into(),
        };
        let vertex = CamHeightGeometryDto::Vertex {
            body_id: 1,
            key: "upright".into(),
            end: true,
        };
        assert_eq!(resolve(&face, &setup, &scene, &[]).unwrap(), 3.0);
        assert_eq!(resolve(&edge, &setup, &scene, &[]).unwrap(), 3.0);
        assert_eq!(resolve(&vertex, &setup, &scene, &[]).unwrap(), 6.0);
        scene.bodies[0].faces[0].plane.as_mut().unwrap().origin[2] = 7.0;
        assert_eq!(resolve(&face, &setup, &scene, &[]).unwrap(), 5.0);
        let saved = serde_json::to_string(&face).unwrap();
        assert_eq!(
            serde_json::from_str::<CamHeightGeometryDto>(&saved).unwrap(),
            face
        );
    }
    #[test]
    fn invalid_or_removed_geometry_never_reuses_the_old_height() {
        let (mut setup, mut scene) = fixture();
        let face = CamHeightGeometryDto::Face {
            body_id: 1,
            key: "floor".into(),
        };
        let edge = CamHeightGeometryDto::Edge {
            body_id: 1,
            key: "upright".into(),
        };
        assert!(resolve(&edge, &setup, &scene, &[]).is_err());
        scene.bodies[0].faces[0].plane.as_mut().unwrap().normal = [1.0, 0.0, 0.0];
        assert!(resolve(&face, &setup, &scene, &[]).is_err());
        setup.wcs.z_axis = [1.0, 0.0, 0.0];
        assert_eq!(resolve(&face, &setup, &scene, &[]).unwrap(), 0.0);
        setup.body_ids.clear();
        assert!(resolve(&face, &setup, &scene, &[]).is_err());
        assert!(resolve(&face, &setup, &SolidSceneDto::default(), &[]).is_err());
    }
}
