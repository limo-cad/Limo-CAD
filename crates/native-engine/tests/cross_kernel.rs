//! A project finished on one kernel, opened on the other.
//!
//! A fillet or chamfer keeps the edges it works on by key, and the two
//! kernels do not write keys alike: Open CASCADE counts the edges of a
//! body, the Limo kernel names an edge after the faces it lies between.
//! Beside every key the project also keeps a mark, a few points of the
//! edge, and a kernel that meets a key in the other's language finds the
//! edge by its mark and from then on calls it by a name of its own. A
//! shell and an external thread keep faces the same way.
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

/// Take every mark out of a project model, which leaves it as a release
/// from before there were marks saved it. Says how many features it took
/// them from.
fn forget_marks(model: &mut Value) -> usize {
    match model {
        Value::Object(fields) => {
            let marked = ["edge_marks", "face_marks", "face_mark"]
                .into_iter()
                .filter(|kind| fields.remove(*kind).is_some())
                .count();
            marked + fields.values_mut().map(forget_marks).sum::<usize>()
        }
        Value::Array(items) => items.iter_mut().map(forget_marks).sum(),
        _ => 0,
    }
}

/// The one shell or thread of a project, as the project model has it.
fn body_feature(h: &NativeEngineHost) -> Value {
    let features = model(h)["body_features"].clone();
    assert_eq!(features.as_array().map_or(0, Vec::len), 1, "{features}");
    features[0].clone()
}

/// The keys of the faces the one shell opens, and how many marks it
/// carries for them.
fn opened_faces(h: &NativeEngineHost) -> (Vec<String>, usize) {
    let shell = body_feature(h);
    let keys = shell["face_keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| key.as_str().unwrap().to_string())
        .collect();
    (keys, shell["face_marks"].as_array().map_or(0, Vec::len))
}

fn counted_face(key: &str) -> bool {
    key.strip_prefix("face:")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// A pin 16 high on the origin, and how thick it is.
fn pin(h: &NativeEngineHost) -> (limo_cad_core::FaceId, f64) {
    ok(h.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
    ok(h.engine_call("set_grid_snap", r#"{"enabled":false}"#));
    ok(h.engine_call(
        "add_circle",
        r#"{"mode":"center_diameter","p1":{"x":0,"y":0},"p2":{"x":4,"y":0},"ctrl_held":true}"#,
    ));
    ok(h.engine_call("end_sketch", ""));
    ok(h.solid_extrude(&json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":16.},"taper_angle_deg":0.,"flip":false,"operation":"new_body","target_body_ids":[]}).to_string()));
    let scene = h.solid_scene_snapshot();
    let side = scene.bodies[0]
        .faces
        .iter()
        .find(|f| f.cylinder.is_some())
        .expect("the side of the pin");
    (side.id, 2.0 * side.cylinder.unwrap().radius)
}

/// A block 40 by 24 by 10 about the origin, and on its top face a sketch
/// with a round boss 6 high built from it.
fn bossed(h: &NativeEngineHost) {
    ok(h.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
    ok(h.engine_call("set_grid_snap", r#"{"enabled":false}"#));
    let pts = [(-20., -12.), (20., -12.), (20., 12.), (-20., 12.)];
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        ok(h.engine_call(
            "add_line",
            &json!({"from":{"x":a.0,"y":a.1},"to_raw":{"x":b.0,"y":b.1},"ctrl_held":true})
                .to_string(),
        ));
    }
    ok(h.engine_call("end_sketch", ""));
    ok(h.solid_extrude(&block_of(10.)["extrude"].to_string()));
    let top = h.solid_scene_snapshot().bodies[0]
        .faces
        .iter()
        .find(|f| {
            f.plane
                .is_some_and(|p| p.normal[2] > 0.9 && (p.origin[2] - 10.).abs() < 1e-6)
        })
        .expect("the top of the block")
        .id;
    ok(h.engine_call(
        "begin_sketch",
        &json!({"type":"planar_face","face_id":top}).to_string(),
    ));
    ok(h.engine_call(
        "add_circle_locked",
        &json!({"mode":"center_diameter","anchor":{"x":0,"y":0},"edge_hint":{"x":4,"y":0},
            "diameter_mm":8.,"ctrl_held":true})
        .to_string(),
    ));
    ok(h.engine_call("end_sketch", ""));
    ok(h.solid_extrude(&json!({"sketch_name":"Sketch2","profile_indices":[0],"operation":"join","extent":{"type":"distance","distance":6.},"taper_angle_deg":0.,"flip":false,"target_body_ids":[1]}).to_string()));
}

/// The block of [`bossed`] at another height, as an edit of its extrude.
fn block_of(height: f64) -> Value {
    json!({"extrude":{"sketch_name":"Sketch1","profile_indices":[0],
        "extent":{"type":"distance","distance":height},"taper_angle_deg":0.,"flip":false,
        "operation":"new_body","target_body_ids":[]}})
}

/// Make the block of [`bossed`] that high; the reply, whatever it is.
fn raise(h: &NativeEngineHost, height: f64) -> Value {
    let mut edit = block_of(height);
    edit["feature_id"] = model(h)["extrudes"][0]["feature_id"].clone();
    serde_json::from_str(&h.solid_edit_extrude(&edit.to_string())).unwrap()
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

    // Faces cross the same way. A shell keeps the faces it opens by key
    // and by mark, and on this body the face at the same count in the
    // other kernel's list is another face: before faces had marks, the
    // body was hollowed out through that one and nothing said so.
    for (first, second) in [("occt", "limo"), ("limo", "occt")] {
        let native = |kernel: &str, key: &str| counted_face(key) == (kernel == "occt");
        let made = on(first);
        stepped(&made);
        let solid = weight(&made);
        let open = underside(&made);
        ok(made.solid_body_feature(
            &json!({"type":"shell","request":{"body_id":1,"face_ids":[open],"thickness":2.,"inward":true}})
                .to_string(),
        ));
        let hollow = weight(&made);
        assert!(hollow < 0.6 * solid, "{first} did not hollow it");
        // The kernel that made it has said where the face is.
        let (made_keys, made_marks) = opened_faces(&made);
        assert!(made_keys.iter().all(|k| native(first, k)), "{made_keys:?}");
        assert_eq!((made_keys.len(), made_marks), (1, 1));
        let saved = made.project_snapshot().unwrap();

        // Opened on the other: the same face, and from here on under that
        // kernel's own key. The two kernels round the inside of the step
        // a little differently, so the shells agree only nearly.
        let opened = on(second);
        ok(opened.project_load_snapshot(&saved));
        let again = weight(&opened);
        assert!(
            (again - hollow).abs() < 2.0e-3 * hollow,
            "{first} made {hollow}, {second} makes {again}"
        );
        let (opened_keys, opened_marks) = opened_faces(&opened);
        assert!(
            opened_keys.iter().all(|k| native(second, k)),
            "{opened_keys:?}"
        );
        assert_eq!((opened_keys.len(), opened_marks), (1, 1));

        // It is an ordinary shell there: its wall can be made thicker.
        let shell = body_feature(&opened);
        ok(opened.solid_edit_body_feature(
            &json!({"feature_id":shell["feature_id"],"feature":{"type":"shell","request":{
                "body_id":1,"face_ids":shell["face_ids"],"thickness":3.,"inward":true}}})
            .to_string(),
        ));
        let thicker = weight(&opened);
        assert!(thicker > again + 100.0, "{again} then {thicker}");
        assert_eq!(opened_faces(&opened), (opened_keys, 1));

        // And back again on the first, thicker wall and all.
        let back = on(first);
        ok(back.project_load_snapshot(&opened.project_snapshot().unwrap()));
        let returned = weight(&back);
        assert!(
            (returned - thicker).abs() < 2.0e-3 * thicker,
            "{second} made {thicker}, {first} makes {returned}"
        );
        assert!(opened_faces(&back).0.iter().all(|k| native(first, k)));
    }

    // The cylinder an external thread is cut on, likewise.
    for (first, second) in [("occt", "limo"), ("limo", "occt")] {
        let native = |kernel: &str, key: &str| counted_face(key) == (kernel == "occt");
        let made = on(first);
        let (side, across) = pin(&made);
        let blank = weight(&made);
        let pitch = if across < 6.0 { 0.7 } else { 1.25 };
        let request = limo_cad_solid::BodyFeatureRequestDto::ExternalThread(
            limo_cad_solid::ExternalThreadRequest {
                body_id: limo_cad_core::BodyId(1),
                face_id: side,
                thread: limo_cad_solid::HoleThreadDto {
                    standard: limo_cad_solid::HoleThreadStandard::IsoMetric,
                    series: limo_cad_solid::HoleThreadSeries::MetricCoarse,
                    designation: format!("M{across} x {pitch} - 6g"),
                    class: "6g".to_string(),
                    nominal_diameter: across,
                    pitch,
                    threads_per_inch: None,
                    hand: limo_cad_solid::HoleThreadHand::Right,
                    depth: Some(6.0),
                    representation: limo_cad_solid::HoleThreadRepresentation::Modeled,
                    tap_drill_designation: None,
                    rounded_profile: None,
                },
                flip: false,
            },
        );
        ok(made.solid_body_feature(&serde_json::to_string(&request).unwrap()));
        let cut = weight(&made);
        assert!(
            cut < blank - 1.0e-2,
            "{first} cut no thread: {blank} then {cut}"
        );
        let thread = body_feature(&made);
        assert!(
            native(first, thread["face_key"].as_str().unwrap()),
            "{thread}"
        );
        assert_eq!(
            thread["face_mark"]["points"].as_array().map_or(0, Vec::len),
            5
        );

        let opened = on(second);
        ok(opened.project_load_snapshot(&made.project_snapshot().unwrap()));
        let again = weight(&opened);
        assert!(
            (again - cut).abs() < 2.0e-3 * cut,
            "{first} cut it to {cut}, {second} to {again}"
        );
        let thread = body_feature(&opened);
        assert!(
            native(second, thread["face_key"].as_str().unwrap()),
            "{thread}"
        );
        assert_eq!(
            thread["face_mark"]["points"].as_array().map_or(0, Vec::len),
            5
        );

        let back = on(first);
        ok(back.project_load_snapshot(&opened.project_snapshot().unwrap()));
        let returned = weight(&back);
        assert!(
            (returned - cut).abs() < 1.0e-6 * cut,
            "{cut} then {returned}"
        );
        assert!(native(
            first,
            body_feature(&back)["face_key"].as_str().unwrap()
        ));
    }

    // A shell older than marks: its face by Open CASCADE's count alone.
    // On the Limo kernel Open CASCADE reads the count, as for the round
    // above, and the shell opens where it was put.
    let made = on("occt");
    stepped(&made);
    let open = underside(&made);
    ok(made.solid_body_feature(
        &json!({"type":"shell","request":{"body_id":1,"face_ids":[open],"thickness":2.,"inward":true}})
            .to_string(),
    ));
    let hollow = weight(&made);
    let exported = ok(made.engine_call("project_export_model", ""));
    let mut old: Value = serde_json::from_str(exported.as_str().unwrap()).unwrap();
    assert_eq!(forget_marks(&mut old), 1);
    let opened = on("limo");
    ok(opened.project_load(&serde_json::to_string(&old.to_string()).unwrap()));
    let again = weight(&opened);
    assert!(
        (again - hollow).abs() < 2.0e-3 * hollow,
        "made as {hollow}, opened as {again}"
    );
    let (keys_there, marks_there) = opened_faces(&opened);
    assert!(
        keys_there.iter().all(|k| !counted_face(k)),
        "{keys_there:?}"
    );
    assert_eq!(marks_there, 1);

    // A sketch drawn on a face. It keeps its own plane, so what is built
    // from it is the same on either kernel. A project made on Open CASCADE
    // goes to the Limo kernel, is edited there below the sketch, and comes
    // back and is edited again: each kernel takes the edit, and makes of
    // it what the other does.
    let made = on("occt");
    bossed(&made);
    let first = weight(&made);
    let there = on("limo");
    ok(there.project_load_snapshot(&made.project_snapshot().unwrap()));
    assert!((weight(&there) - first).abs() < 2.0e-3 * first);
    assert_eq!(raise(&there, 14.)["ok"], true);
    let taller = weight(&there);
    assert!(taller > first + 3000.0, "{first} then {taller}");
    let back = on("occt");
    ok(back.project_load_snapshot(&there.project_snapshot().unwrap()));
    assert!((weight(&back) - taller).abs() < 2.0e-3 * taller);
    assert_eq!(raise(&made, 14.)["ok"], true);
    assert!((weight(&made) - weight(&back)).abs() < 1.0e-6 * taller);
    assert_eq!(raise(&back, 10.)["ok"], true);
    assert!((weight(&back) - first).abs() < 1.0e-6 * first);

    // The other way has a limit. The sketch is tied to its face by the
    // face's id, and Open CASCADE cannot follow an id the Limo kernel
    // wrote: a project made there opens the same, but a feature below the
    // sketch cannot be edited until the sketch is put on its face again.
    // It says so and changes nothing.
    let made = on("limo");
    bossed(&made);
    let first = weight(&made);
    let there = on("occt");
    ok(there.project_load_snapshot(&made.project_snapshot().unwrap()));
    assert!((weight(&there) - first).abs() < 2.0e-3 * first);
    let refused = raise(&there, 14.);
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .is_some_and(|error| error.contains("broken reference")),
        "{refused}"
    );
    assert!((weight(&there) - first).abs() < 2.0e-3 * first);

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
