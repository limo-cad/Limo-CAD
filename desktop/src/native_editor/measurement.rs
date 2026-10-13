//! Read-only selection measurements from the retained topology and mesh.
//! Parent body IDs identify ownership; they do not turn an edge into a body selection.
use super::*;
use crate::native_viewport::{ViewportHudRow, ViewportHudSelection, ViewportPresentation};
use bevy::math::DVec3;
use limo_cad_solid::{BodyDto, EdgeDto, FaceDto, Point3Dto, SolidSceneDto};

#[derive(Clone, PartialEq)]
struct SelectionKey {
    owner: String,
    revision: u64,
    visible: bool,
    occurrence: Option<u64>,
    bodies: Vec<u64>,
    faces: Vec<u64>,
    edges: Vec<u64>,
}
#[derive(Resource, Default)]
struct Readout {
    key: Option<SelectionKey>,
    value: Option<ViewportHudSelection>,
}

pub(super) fn synchronize(world: &mut World, visible: bool) {
    let (owner, _, view, _) = native_viewport::interface_view(world);
    let key = SelectionKey {
        owner: owner.into(),
        revision: native_viewport::interface_model_revision(world),
        visible,
        occurrence: view.selected_occurrence_id,
        bodies: view.selected_body_ids.clone(),
        faces: view.selected_face_ids.clone(),
        edges: view.selected_edge_ids.clone(),
    };
    if world
        .get_resource::<Readout>()
        .is_some_and(|cached| cached.key.as_ref() == Some(&key))
    {
        return;
    }
    let value = visible
        .then(|| measure(native_viewport::interface_geometry(world).scene, view))
        .flatten();
    native_viewport::apply_interface_selection_readout(world, value.clone());
    world.insert_resource(Readout {
        key: Some(key),
        value,
    });
}

pub(super) fn caption(world: &World) -> Option<String> {
    let value = world.get_resource::<Readout>()?.value.as_ref()?;
    Some(format!(
        "{} · {}\n{}",
        value.title,
        value.subject,
        value
            .rows
            .iter()
            .map(|row| format!("{}: {}", row.label, row.value))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}
fn number(value: f64) -> String {
    let value = if value.abs() < 0.0005 { 0. } else { value };
    format!("{value:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .into()
}
fn row(label: &str, value: f64, unit: &str, approximate: bool) -> ViewportHudRow {
    ViewportHudRow {
        label: label.into(),
        value: format!(
            "{}{} {unit}",
            if approximate { "≈ " } else { "" },
            number(value)
        ),
    }
}
fn point(p: &Point3Dto) -> DVec3 {
    DVec3::new(p.x, p.y, p.z)
}
fn length(edge: &EdgeDto) -> f64 {
    edge.circle.filter(|c| c.closed).map_or_else(
        || {
            edge.points
                .windows(2)
                .map(|p| point(&p[0]).distance(point(&p[1])))
                .sum()
        },
        |c| std::f64::consts::TAU * c.radius,
    )
}
fn vertex(body: &BodyDto, index: u32) -> Option<DVec3> {
    let i = (index as usize).checked_mul(3)?;
    let p = body.mesh.positions.get(i..i.checked_add(3)?)?;
    Some(DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64))
}
fn triangles(body: &BodyDto, first: usize, count: usize) -> impl Iterator<Item = [DVec3; 3]> + '_ {
    body.mesh
        .indices
        .get(first..first.saturating_add(count))
        .unwrap_or(&[])
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|t| {
            Some([
                vertex(body, t[0])?,
                vertex(body, t[1])?,
                vertex(body, t[2])?,
            ])
        })
}
fn area(body: &BodyDto, face: &FaceDto) -> f64 {
    triangles(body, face.first_index as usize, face.index_count as usize)
        .map(|[a, b, c]| (b - a).cross(c - a).length() * 0.5)
        .sum()
}
fn perimeter(body: &BodyDto, face: &FaceDto) -> f64 {
    body.edges
        .iter()
        .filter(|edge| face.edge_keys.contains(&edge.key))
        .map(length)
        .sum()
}
fn measure(scene: &SolidSceneDto, view: &ViewportPresentation) -> Option<ViewportHudSelection> {
    let edges: Vec<_> = scene
        .bodies
        .iter()
        .flat_map(|b| b.edges.iter())
        .filter(|e| view.selected_edge_ids.contains(&e.id.0))
        .collect();
    let faces: Vec<_> = scene
        .bodies
        .iter()
        .flat_map(|b| b.faces.iter().map(move |f| (b, f)))
        .filter(|(_, f)| view.selected_face_ids.contains(&f.id.0))
        .collect();
    let bodies: Vec<_> = if edges.is_empty() && faces.is_empty() {
        scene
            .bodies
            .iter()
            .filter(|b| view.selected_body_ids.contains(&b.id.0))
            .collect()
    } else {
        vec![]
    };
    if edges.is_empty() && faces.is_empty() && bodies.is_empty() {
        return None;
    }
    let mut rows = Vec::new();
    let approximate_edges = edges
        .iter()
        .any(|e| e.points.len() > 2 && !e.circle.is_some_and(|c| c.closed));
    let mut approximate = false;
    let subject = if !edges.is_empty() && !faces.is_empty() {
        format!("{} objects", edges.len() + faces.len())
    } else if edges.len() == 1 {
        "Edge".into()
    } else if !edges.is_empty() {
        format!("{} edges", edges.len())
    } else if faces.len() == 1 {
        "Face".into()
    } else if !faces.is_empty() {
        format!("{} faces", faces.len())
    } else if bodies.len() == 1 {
        bodies[0].name.clone()
    } else {
        format!("{} bodies", bodies.len())
    };
    if !edges.is_empty() {
        rows.push(row(
            if edges.len() == 1 {
                "Length"
            } else {
                "Total length"
            },
            edges.iter().map(|e| length(e)).sum(),
            "mm",
            approximate_edges,
        ));
        approximate |= approximate_edges;
        if edges.len() == 1 {
            if let Some(circle) = edges[0].circle {
                rows.push(row("Radius", circle.radius, "mm", false));
            }
        } else if edges.len() == 2 && edges.iter().all(|e| e.points.len() == 2) {
            let a = point(&edges[0].points[1]) - point(&edges[0].points[0]);
            let b = point(&edges[1].points[1]) - point(&edges[1].points[0]);
            if let Some((a, b)) = a.try_normalize().zip(b.try_normalize()) {
                rows.push(row(
                    "Angle",
                    a.dot(b).abs().clamp(0., 1.).acos().to_degrees(),
                    "°",
                    false,
                ));
            }
        }
    }
    if !faces.is_empty() {
        rows.push(row(
            if faces.len() == 1 {
                "Area"
            } else {
                "Total area"
            },
            faces.iter().map(|(b, f)| area(b, f)).sum(),
            "mm²",
            true,
        ));
        rows.push(row(
            if faces.len() == 1 {
                "Perimeter"
            } else {
                "Total perimeter"
            },
            faces.iter().map(|(b, f)| perimeter(b, f)).sum(),
            "mm",
            true,
        ));
        if faces.len() == 1 {
            if let Some(cylinder) = faces[0].1.cylinder {
                rows.push(row("Radius", cylinder.radius, "mm", false));
            }
        }
        approximate = true;
    }
    if !bodies.is_empty() {
        if bodies.len() == 1 {
            let mut low = DVec3::splat(f64::INFINITY);
            let mut high = DVec3::splat(f64::NEG_INFINITY);
            for p in bodies[0].mesh.positions.as_chunks::<3>().0.iter() {
                let p = DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
                low = low.min(p);
                high = high.max(p);
            }
            if low.is_finite() && high.is_finite() {
                rows.push(ViewportHudRow {
                    label: "Size".into(),
                    value: format!("≈ {} mm", (high - low).to_array().map(number).join(" × ")),
                });
            }
        }
        let area: f64 = bodies
            .iter()
            .flat_map(|b| triangles(b, 0, b.mesh.indices.len()))
            .map(|[a, b, c]| (b - a).cross(c - a).length() * 0.5)
            .sum();
        let volume: f64 = bodies
            .iter()
            .map(|b| {
                triangles(b, 0, b.mesh.indices.len())
                    .map(|[a, b, c]| a.dot(b.cross(c)) / 6.)
                    .sum::<f64>()
                    .abs()
            })
            .sum();
        rows.push(row(
            if bodies.len() == 1 {
                "Surface area"
            } else {
                "Total surface area"
            },
            area,
            "mm²",
            true,
        ));
        rows.push(row(
            if bodies.len() == 1 {
                "Volume"
            } else {
                "Total volume"
            },
            volume,
            "mm³",
            true,
        ));
        approximate = true;
    }
    rows.retain(|r| !r.value.contains("NaN") && !r.value.contains("inf"));
    Some(ViewportHudSelection {
        title: "SELECTION".into(),
        subject,
        rows,
        footer: approximate.then(|| "≈ Measured from the display mesh".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_edge_reports_its_length_without_measuring_its_parent_body() {
        let scene: SolidSceneDto = serde_json::from_value(json!({"bodies":[{"id":1,"feature_id":1,"name":"Box","mesh":{"positions":[],"normals":[],"indices":[]},"faces":[],"edges":[{"id":2,"key":"side","points":[{"x":0,"y":0,"z":0},{"x":5,"y":0,"z":0}]}]}],"errors":[]})).unwrap();
        let view = ViewportPresentation {
            selected_body_ids: vec![1],
            selected_edge_ids: vec![2],
            ..Default::default()
        };
        let measured = measure(&scene, &view).unwrap();
        assert_eq!(measured.subject, "Edge");
        assert_eq!(
            measured.rows,
            vec![ViewportHudRow {
                label: "Length".into(),
                value: "5 mm".into()
            }]
        );
        assert!(measured.footer.is_none());
        assert!(measure(&scene, &ViewportPresentation::default()).is_none());
    }
}
