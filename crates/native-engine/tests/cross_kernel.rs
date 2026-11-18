//! A project finished on one kernel, opened on the other.
//!
//! A fillet or chamfer keeps the edges it works on by key, and the two
//! kernels do not write keys alike: Open CASCADE counts the edges of a
//! body, the Limo kernel names an edge after the faces it lies between.
//! Beside every key the project also keeps a mark, a few points of the
//! edge, and a kernel that meets a key in the other's language finds the
//! edge by its mark and from then on calls it by a name of its own.
//!
//! The kernel is chosen per process, by an environment variable or, when
//! that names none, by the application itself, and the choice is read each
//! time a host is made. This file is one test so that nothing else in the
//! process is making hosts while it changes.
#![cfg(feature = "native-occt")]
use limo_cad_native_engine::NativeEngineHost;
use serde_json::{json, Value};

fn ok(raw: String) -> Value {
    let v: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    v["value"].clone()
}

fn model(h: &NativeEngineHost) -> Value {
    serde_json::from_str(&h.project_model_projection().unwrap()).unwrap()
}

fn on(kernel: &str) -> NativeEngineHost {
    match kernel {
        "limo" => std::env::set_var("LIMO_CAD_KERNEL", "limo"),
        _ => std::env::remove_var("LIMO_CAD_KERNEL"),
    }
    NativeEngineHost::new()
}

/// The rim of radius `r` at height `y`.
fn rim(h: &NativeEngineHost, r: f64, y: f64) -> limo_cad_core::EdgeId {
    h.solid_scene_snapshot().bodies[0]
        .edges
        .iter()
        .find(|e| {
            e.circle
                .is_some_and(|c| (c.radius - r).abs() < 1e-6 && (c.center.y - y).abs() < 1e-6)
        })
        .unwrap_or_else(|| panic!("no rim of radius {r} at {y}"))
        .id
}

/// What the drawn skin of the one body encloses.
fn weight(h: &NativeEngineHost) -> f64 {
    let scene = h.solid_scene_snapshot();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    enclosed(h)
}

/// The same, whether or not every feature of the project could be built.
fn enclosed(h: &NativeEngineHost) -> f64 {
    let scene = h.solid_scene_snapshot();
    assert_eq!(scene.bodies.len(), 1);
    let body = &scene.bodies[0];
    let at = |i: u32| {
        let k = 3 * i as usize;
        [
            f64::from(body.mesh.positions[k]),
            f64::from(body.mesh.positions[k + 1]),
            f64::from(body.mesh.positions[k + 2]),
        ]
    };
    body.mesh
        .indices
        .chunks(3)
        .map(|t| {
            let (a, b, c) = (at(t[0]), at(t[1]), at(t[2]));
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum::<f64>()
        .abs()
}

/// A stepped ring turned from an L, with the inner rim of its top rounded
/// and the outer rim of its flange cut: one fillet and one chamfer, each
/// on an edge that goes right round.
fn turned(h: &NativeEngineHost) {
    ok(h.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
    ok(h.engine_call("set_grid_snap", r#"{"enabled":false}"#));
    let pts = [
        (10., 0.),
        (10., 30.),
        (15., 30.),
        (15., 3.),
        (22., 3.),
        (22., 0.),
    ];
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        ok(h.engine_call(
            "add_line",
            &json!({"from":{"x":a.0,"y":a.1},"to_raw":{"x":b.0,"y":b.1},"ctrl_held":true})
                .to_string(),
        ));
    }
    ok(h.engine_call("end_sketch", ""));
    ok(h.solid_revolve(&json!({"sketch_name":"Sketch1","profile_indices":[0],"axis_origin":{"x":0.,"y":0.},"axis_direction":{"x":0.,"y":1.},"angle_deg":360.,"flip":false,"operation":"new_body","target_body_ids":[]}).to_string()));
    let inner = rim(h, 10., 30.);
    ok(h.solid_fillet(
        &json!({"body_id":1,"edge_ids":[inner],"radius":1.,"tangent_chain":false}).to_string(),
    ));
    let outer = rim(h, 22., 3.);
    ok(h.solid_chamfer(&json!({"body_id":1,"edge_ids":[outer],"distance":0.5,"angle_deg":null,"tangent_chain":false}).to_string()));
}

/// A block 40 by 25 by 10 with a smaller block standing 20 high in the
/// middle of it, joined on: two features, and a body whose faces and edges
/// the two kernels do not list in the same order.
fn stepped(h: &NativeEngineHost) {
    for (name, pts, height, join) in [
        (
            "Sketch1",
            [(0., 0.), (40., 0.), (40., 25.), (0., 25.)],
            10.,
            false,
        ),
        (
            "Sketch2",
            [(10., 8.), (30., 8.), (30., 18.), (10., 18.)],
            20.,
            true,
        ),
    ] {
        ok(h.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
        ok(h.engine_call("set_grid_snap", r#"{"enabled":false}"#));
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            ok(h.engine_call(
                "add_line",
                &json!({"from":{"x":a.0,"y":a.1},"to_raw":{"x":b.0,"y":b.1},"ctrl_held":true})
                    .to_string(),
            ));
        }
        ok(h.engine_call("end_sketch", ""));
        ok(h.solid_extrude(&json!({"sketch_name":name,"profile_indices":[0],"extent":{"type":"distance","distance":height},"taper_angle_deg":0.,"flip":false,"operation":if join {"join"} else {"new_body"},"target_body_ids":if join {vec![1]} else {vec![]}}).to_string()));
    }
}

/// The upright edge of the stepped block at the corner `(x, y)`.
fn upright(h: &NativeEngineHost, x: f64, y: f64) -> limo_cad_core::EdgeId {
    h.solid_scene_snapshot().bodies[0]
        .edges
        .iter()
        .find(|e| {
            e.points.len() == 2
                && e.points
                    .iter()
                    .all(|p| (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9)
        })
        .unwrap_or_else(|| panic!("no upright edge at {x}, {y}"))
        .id
}

/// The face the stepped block stands on.
fn underside(h: &NativeEngineHost) -> limo_cad_core::FaceId {
    h.solid_scene_snapshot().bodies[0]
        .faces
        .iter()
        .find(|f| {
            f.plane
                .as_ref()
                .is_some_and(|p| p.normal[2] < -1.0 + 1e-9 && p.origin[2].abs() < 1e-9)
        })
        .expect("the face underneath")
        .id
}

/// Take every edge mark out of a project model, which leaves it as a
/// release from before there were marks saved it. Says how many it took.
fn forget_marks(model: &mut Value) -> usize {
    match model {
        Value::Object(fields) => {
            usize::from(fields.remove("edge_marks").is_some())
                + fields.values_mut().map(forget_marks).sum::<usize>()
        }
        Value::Array(items) => items.iter_mut().map(forget_marks).sum(),
        _ => 0,
    }
}

/// The keys of the one fillet and the one chamfer, and how many marks each
/// carries.
fn keys(h: &NativeEngineHost) -> (Vec<String>, usize) {
    let model = model(h);
    let mut keys = Vec::new();
    let mut marks = 0;
    for kind in ["fillets", "chamfers"] {
        let finish = &model[kind][0];
        for key in finish["edge_keys"].as_array().unwrap() {
            keys.push(key.as_str().unwrap().to_string());
        }
        marks += finish["edge_marks"].as_array().map_or(0, Vec::len);
    }
    (keys, marks)
}

fn counted(key: &str) -> bool {
    key.strip_prefix("edge:")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

#[test]
fn a_project_finished_on_one_kernel_opens_on_the_other() {
    for (first, second) in [("occt", "limo"), ("limo", "occt")] {
        let native = |kernel: &str, key: &str| counted(key) == (kernel == "occt");
        // Finished on the first kernel: keys in its language, and a mark
        // beside every one of them.
        let made = on(first);
        turned(&made);
        let whole = weight(&made);
        let (made_keys, made_marks) = keys(&made);
        assert!(made_keys.iter().all(|k| native(first, k)), "{made_keys:?}");
        assert_eq!(made_marks, made_keys.len(), "a mark for every key");
        let saved = made.project_snapshot().unwrap();

        // Opened on the second: the same solid, found by the marks, and
        // from here on kept under that kernel's own keys.
        let opened = on(second);
        ok(opened.project_load_snapshot(&saved));
        let again = weight(&opened);
        assert!(
            (again - whole).abs() < 2.0e-3 * whole,
            "{first} made {whole}, {second} makes {again}"
        );
        let (opened_keys, opened_marks) = keys(&opened);
        assert!(
            opened_keys.iter().all(|k| native(second, k)),
            "{opened_keys:?}"
        );
        assert_eq!(opened_marks, opened_keys.len());

        // It is an ordinary project there: the round can be made larger,
        // and the chamfer after it still finds its edge.
        let fillet = model(&opened)["fillets"][0].clone();
        ok(opened.solid_edit_fillet(
            &json!({"feature_id":fillet["feature_id"],"fillet":{
                "body_id":fillet["body_id"],"edge_ids":fillet["edge_ids"],
                "radius":2.,"tangent_chain":false}})
            .to_string(),
        ));
        let larger = weight(&opened);
        assert!(larger < again - 1.0, "{again} then {larger}");

        // And back again on the first, larger round and all.
        let back = on(first);
        ok(back.project_load_snapshot(&opened.project_snapshot().unwrap()));
        let returned = weight(&back);
        assert!(
            (returned - larger).abs() < 2.0e-3 * larger,
            "{second} made {larger}, {first} makes {returned}"
        );
        assert!(keys(&back).0.iter().all(|k| native(first, k)));
    }

    // A project older than marks. Every project a release before this one
    // saved has its fillets by Open CASCADE's count alone, and the edge at
    // the same count in the Limo kernel's list is as a rule another edge:
    // on this body, a longer one. Opened on the Limo kernel, Open CASCADE
    // reads its own count once and says where the edge is, and the round
    // goes where it was put.
    let made = on("occt");
    stepped(&made);
    let corner = upright(&made, 40., 25.);
    ok(made.solid_fillet(
        &json!({"body_id":1,"edge_ids":[corner],"radius":2.,"tangent_chain":false}).to_string(),
    ));
    let whole = weight(&made);
    let exported = ok(made.engine_call("project_export_model", ""));
    let mut old: Value = serde_json::from_str(exported.as_str().unwrap()).unwrap();
    assert_eq!(forget_marks(&mut old), 1);
    let opened = on("limo");
    ok(opened.project_load(&serde_json::to_string(&old.to_string()).unwrap()));
    let again = weight(&opened);
    assert!(
        (again - whole).abs() < 1.0e-4 * whole,
        "made as {whole}, opened as {again}"
    );
    // From there on it is in the Limo kernel's own keys, marked.
    let fillet = model(&opened)["fillets"][0].clone();
    let key = fillet["edge_keys"][0].as_str().unwrap();
    assert!(!counted(key), "{key}");
    assert_eq!(fillet["edge_marks"].as_array().map_or(0, Vec::len), 1);

    // What does not cross yet: a face. A shell keeps the faces it opens by
    // key alone, with no mark, and here the face at the same count is again
    // another face. Neither kernel guesses: the shell is refused with a
    // word on what to do, and the body is left solid.
    for (first, second) in [("occt", "limo"), ("limo", "occt")] {
        let made = on(first);
        stepped(&made);
        let solid = weight(&made);
        let open = underside(&made);
        ok(made.solid_body_feature(
            &json!({"type":"shell","request":{"body_id":1,"face_ids":[open],"thickness":2.,"inward":true}})
                .to_string(),
        ));
        assert!(weight(&made) < 0.6 * solid, "{first} did not hollow it");
        let saved = made.project_snapshot().unwrap();
        let opened = on(second);
        ok(opened.project_load_snapshot(&saved));
        let errors = opened.solid_scene_snapshot().errors.clone();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].message.contains("reselect the face"),
            "{second} says {:?}",
            errors[0].message
        );
        let left = enclosed(&opened);
        assert!(
            (left - solid).abs() < 1.0e-6 * solid,
            "{second} left {left} of {solid}"
        );
    }

    // Who chooses. With no kernel named in the environment the choice is
    // the application's own, which is what its Settings save for the next
    // start, and that is Open CASCADE until it says otherwise.
    std::env::remove_var("LIMO_CAD_KERNEL");
    assert_eq!(limo_cad_occt::kernel_named_in_environment(), None);
    assert!(!limo_cad_occt::limo_kernel_selected());
    limo_cad_occt::choose_limo_kernel(true);
    let chosen = NativeEngineHost::new();
    turned(&chosen);
    assert!(keys(&chosen).0.iter().all(|k| !counted(k)));

    // The environment still decides when it names a kernel, either one.
    std::env::set_var("LIMO_CAD_KERNEL", "occt");
    assert_eq!(limo_cad_occt::kernel_named_in_environment(), Some(false));
    let named = NativeEngineHost::new();
    turned(&named);
    assert!(keys(&named).0.iter().all(|k| counted(k)));
    limo_cad_occt::choose_limo_kernel(false);
    std::env::set_var("LIMO_CAD_KERNEL", "LIMO");
    assert_eq!(limo_cad_occt::kernel_named_in_environment(), Some(true));
    assert!(limo_cad_occt::limo_kernel_selected());

    // Set but empty, it names none.
    std::env::set_var("LIMO_CAD_KERNEL", "");
    assert_eq!(limo_cad_occt::kernel_named_in_environment(), None);
    assert!(!limo_cad_occt::limo_kernel_selected());
    std::env::remove_var("LIMO_CAD_KERNEL");
}
