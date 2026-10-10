//! Pure acceptance-reader regressions; these never start a CAD process.
use super::*;

const TETRAHEDRON: &str = r#"<object id="1"><mesh><vertices>
<vertex x="0" y="0" z="0"/><vertex x="3" y="0" z="0"/>
<vertex x="0" y="2" z="0"/><vertex x="0" y="0" z="5"/>
</vertices><triangles>
<triangle v1="0" v2="2" v3="1"/><triangle v1="0" v2="1" v3="3"/>
<triangle v1="0" v2="3" v3="2"/><triangle v1="1" v2="2" v3="3"/>
</triangles></mesh></object>"#;

fn export(resources: &str, build: &str) -> Value {
    let model = format!(
        r#"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources>{resources}</resources><build>{build}</build></model>"#
    );
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file("3D/3dmodel.model", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(model.as_bytes()).unwrap();
    json!({"bytes_base64":BASE64.encode(archive.finish().unwrap().into_inner())})
}

fn source() -> Value {
    json!({"mesh":{"positions":[0.,0.,0.,3.,0.,0.,0.,2.,0.,0.,0.,5.],
        "indices":[0,2,1,0,1,3,0,3,2,1,2,3]}})
}

#[test]
fn flat_build_preserves_oriented_world_surfaces() {
    let meshes = exported_meshes(&export(TETRAHEDRON, r#"<item objectid="1"/>"#));
    assert_eq!(meshes.len(), 1);
    assert!(same_placed_surface(
        &meshes[0],
        &source(),
        &json!({"translation":[0.,0.,0.],"rotation":[0.,0.,0.,1.]})
    ));
}

#[test]
fn nested_component_and_build_transforms_place_repeated_meshes() {
    let resources = format!(
        r#"{TETRAHEDRON}<object id="2"><components>
<component objectid="1" transform="0 1 0 -1 0 0 0 0 1 1 2 3"/>
<component objectid="1" transform="0 1 0 -1 0 0 0 0 1 11 2 3"/>
</components></object><object id="3"><components>
<component objectid="2" transform="1 0 0 0 0 1 0 -1 0 4 5 6"/>
</components></object>"#
    );
    let meshes = exported_meshes(&export(
        &resources,
        r#"<item objectid="3" transform="1 0 0 0 1 0 0 0 1 10 20 30"/>"#,
    ));
    assert_eq!(meshes.len(), 2);
    for (mesh, x) in meshes.iter().zip([15., 25.]) {
        assert!(same_placed_surface(
            mesh,
            &source(),
            &json!({"translation":[x,22.,38.],"rotation":[0.5,-0.5,0.5,0.5]})
        ));
        assert!((mesh_measurement(mesh).2 - 5.).abs() < 1e-10);
    }
}

#[test]
fn acceptance_reader_rejects_invalid_meshes_and_reflections() {
    let identity = r#"<item objectid="1"/>"#;
    let invalid = [
        (
            "out-of-range index",
            TETRAHEDRON.replace("v3=\"3\"", "v3=\"99\""),
            identity,
        ),
        (
            "non-finite vertex",
            TETRAHEDRON.replace("x=\"3\"", "x=\"NaN\""),
            identity,
        ),
        (
            "open shell",
            TETRAHEDRON.replace(r#"<triangle v1="0" v2="2" v3="1"/>"#, ""),
            identity,
        ),
        (
            "inconsistent winding",
            TETRAHEDRON.replace(r#"v1="0" v2="2" v3="1""#, r#"v1="0" v2="1" v3="2""#),
            identity,
        ),
        (
            "degenerate edge",
            TETRAHEDRON.replace(r#"v1="0" v2="2" v3="1""#, r#"v1="0" v2="0" v3="1""#),
            identity,
        ),
        (
            "reflected shell",
            TETRAHEDRON.into(),
            r#"<item objectid="1" transform="-1 0 0 0 1 0 0 0 1 0 0 0"/>"#,
        ),
        (
            "non-finite placement",
            TETRAHEDRON.into(),
            r#"<item objectid="1" transform="1 0 0 0 1 0 0 0 1 NaN 0 0"/>"#,
        ),
        ("missing build", TETRAHEDRON.into(), ""),
    ];
    for (name, resources, build) in invalid {
        let package = export(&resources, build);
        assert!(
            std::panic::catch_unwind(|| exported_meshes(&package)).is_err(),
            "acceptance reader admitted {name}"
        );
    }
}
