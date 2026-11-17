use limo_cad_kernel::make::{self, Region, Seg};
use limo_cad_kernel::math::*;
use limo_cad_kernel::props::planar_props;
use limo_cad_kernel::tess::{tessellate, TessOptions};

fn fine() -> TessOptions {
    TessOptions {
        linear: 0.0005,
        angular: 0.02,
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Seg> {
    let p = [v3(x0, y0, 0.0), v3(x1, y0, 0.0), v3(x1, y1, 0.0), v3(x0, y1, 0.0)];
    (0..4)
        .map(|i| Seg::Line {
            a: p[i],
            b: p[(i + 1) % 4],
        })
        .collect()
}

#[test]
fn block_is_exact() {
    let body = make::block(v3(0.0, 0.0, 0.0), v3(60.0, 30.0, 12.0)).unwrap();
    assert_eq!((body.face_count(), body.edge_count(), body.verts.len()), (6, 12, 8));
    let mesh = tessellate(&body, &TessOptions::default());
    assert!((mesh.volume() - 60.0 * 30.0 * 12.0).abs() < 1e-9);
    assert!((mesh.area() - 2.0 * (60.0 * 30.0 + 60.0 * 12.0 + 30.0 * 12.0)).abs() < 1e-9);
    assert_eq!(mesh.triangle_count(), 12);
}

#[test]
fn cylinder_converges_to_analytic_volume() {
    let body = make::cylinder(v3(1.0, 2.0, 3.0), v3(0.0, 0.0, 1.0), 5.0, 20.0).unwrap();
    // One seamless side face and two caps.
    assert_eq!((body.face_count(), body.edge_count()), (3, 2));
    let mesh = tessellate(&body, &fine());
    let exact = PI * 25.0 * 20.0;
    assert!((mesh.volume() - exact).abs() / exact < 1e-4, "{} vs {exact}", mesh.volume());
    let coarse = tessellate(&body, &TessOptions::default());
    assert!((coarse.volume() - exact).abs() / exact < 0.03);
}

#[test]
fn plate_with_holes_needs_no_booleans() {
    let hole = |x: f64, y: f64, r: f64| {
        vec![Seg::Circle {
            c: v3(x, y, 0.0),
            p: v3(x + r, y, 0.0),
            n: V3::Z,
        }]
    };
    let region = Region {
        loops: vec![rect(0.0, 0.0, 80.0, 50.0), hole(20.0, 25.0, 6.0), hole(60.0, 25.0, 4.0)],
    };
    let body = make::extrude(&region, V3::Z, 0.0, 8.0).unwrap();
    assert_eq!(body.face_count(), 4 + 2 + 2);
    let exact = (80.0 * 50.0 - PI * 36.0 - PI * 16.0) * 8.0;
    let mesh = tessellate(&body, &fine());
    assert!((mesh.volume() - exact).abs() / exact < 1e-4, "{} vs {exact}", mesh.volume());
    // The top face reports exact area and centroid from boundary integrals.
    let top = (0..body.face_count() as u32)
        .find(|&f| body.f_surface(f).is_plane() && body.f_normal(f, V3::ZERO).z > 0.5)
        .unwrap();
    let props = planar_props(&body, top).unwrap();
    assert!((props.area - (80.0 * 50.0 - PI * 52.0)).abs() < 1e-9, "{}", props.area);
    assert_eq!((props.loops, props.edges), (3, 6));
    assert!((props.perimeter - (260.0 + TAU * 10.0)).abs() < 1e-9);
}

#[test]
fn reversed_and_downward_profiles_are_normalised() {
    // Clockwise outer loop, extruded downwards.
    let mut cw = rect(0.0, 0.0, 10.0, 10.0);
    cw.reverse();
    for s in &mut cw {
        if let Seg::Line { a, b } = s {
            std::mem::swap(a, b);
        }
    }
    let body = make::extrude(&Region { loops: vec![cw] }, V3::Z, 0.0, -5.0).unwrap();
    let mesh = tessellate(&body, &TessOptions::default());
    assert!((mesh.volume() - 500.0).abs() < 1e-9, "{}", mesh.volume());
}

#[test]
fn slot_profile_with_arcs() {
    // Stadium: two lines and two semicircular arcs of radius 5.
    let r = 5.0;
    let region = Region {
        loops: vec![vec![
            Seg::Line {
                a: v3(0.0, -r, 0.0),
                b: v3(20.0, -r, 0.0),
            },
            Seg::Arc {
                a: v3(20.0, -r, 0.0),
                m: v3(20.0 + r, 0.0, 0.0),
                b: v3(20.0, r, 0.0),
            },
            Seg::Line {
                a: v3(20.0, r, 0.0),
                b: v3(0.0, r, 0.0),
            },
            Seg::Arc {
                a: v3(0.0, r, 0.0),
                m: v3(-r, 0.0, 0.0),
                b: v3(0.0, -r, 0.0),
            },
        ]],
    };
    let body = make::extrude(&region, V3::Z, 2.0, 12.0).unwrap();
    assert_eq!(body.face_count(), 6);
    let exact = (20.0 * 10.0 + PI * 25.0) * 10.0;
    let mesh = tessellate(&body, &fine());
    assert!((mesh.volume() - exact).abs() / exact < 1e-4, "{} vs {exact}", mesh.volume());
    let bb = body.bbox();
    assert!((bb.lo.z - 2.0).abs() < 1e-12 && (bb.hi.z - 12.0).abs() < 1e-12);
}

#[test]
fn mirror_and_rotate_keep_a_valid_solid() {
    let body = make::block(v3(1.0, 2.0, 3.0), v3(4.0, 6.0, 8.0)).unwrap();
    let mirrored = body.transformed(&Xform::mirror(V3::ZERO, V3::X));
    mirrored.check().unwrap();
    let rotated = body.transformed(&Xform::rotation(v3(1.0, 1.0, 1.0), v3(1.0, 2.0, 3.0), 0.7));
    rotated.check().unwrap();
    for b in [&mirrored, &rotated] {
        let v = tessellate(b, &TessOptions::default()).volume();
        assert!((v - 60.0).abs() < 1e-9, "{v}");
    }
    assert!(mirrored.bbox().hi.x < 0.0);
}
