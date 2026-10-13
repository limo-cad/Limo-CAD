use super::*;

fn rotated_offset(exports: &Value, id: &str, offset: [f64; 3]) -> [f64; 3] {
    let pose = exports["final_solution"]["instance_body_poses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pose| pose["body_id"] == part(exports, id)["body_id"])
        .unwrap();
    let q: Vec<_> = pose["rotation"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let u = [q[0], q[1], q[2]];
    let uv = cross(u, offset);
    let uuv = cross(u, uv);
    std::array::from_fn(|i| offset[i] + 2. * (q[3] * uv[i] + uuv[i]))
}

fn clear_path(
    client: &mut Client,
    exports: &Value,
    moving: &str,
    installed: &[&str],
    direction: [f64; 3],
    distances: &[f64],
) {
    assert!(!installed.contains(&moving));
    check_stationary_parts(client, exports, installed);
    for distance in distances {
        let offset = rotated_offset(exports, moving, direction.map(|v| v * distance));
        displaced(client, exports, moving, offset);
        check_moving_part(
            client,
            exports,
            moving,
            installed,
            &format!("installation {distance} mm"),
        );
    }
    displaced(client, exports, moving, [0.; 3]);
}

/// A temporary native cylinder represents the straight shank of a hex driver.
/// It is test-only stock, built through ordinary MCP tools in the restored copy.
fn driver(client: &mut Client, exports: &Value) -> Value {
    client.call("sketch_begin", json!({"name":"Vise assembly / driver clearance witness", "plane":{"type":"origin_plane","plane":"yz"}}));
    client.call("sketch_set_grid_snap", json!({"enabled":false}));
    client.call("sketch_add_circle_locked", json!({"mode":"center_diameter","anchor":{"x":0.,"y":0.},"edge_hint":{"x":3.,"y":0.},"diameter_mm":6.,"ctrl_held":true}));
    client.call("sketch_finish", json!({}));
    let made = client.call("solid_extrude", json!({"sketch_name":"Vise assembly / driver clearance witness","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":50.},"taper_angle_deg":0.,"flip":false,"target_body_ids":[]}));
    assert_eq!(made["scene"]["errors"], json!([]));
    let body = &made["scene"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| {
            !exports["final_scene"]["bodies"]
                .as_array()
                .unwrap()
                .iter()
                .any(|original| original["id"] == candidate["id"])
        })
        .unwrap()["id"];
    let component = client.call("assembly_create_component", json!({"name":"Temporary driver clearance witness","body_ids":[body],"absorb_promoted_bodies":true}));
    let assembly = client.call("assembly_document", json!({}));
    assembly["component_structure"]["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|occurrence| occurrence["component_id"] == component["id"])
        .unwrap()["id"]
        .clone()
}

fn driver_clearance(
    client: &mut Client,
    exports: &Value,
    witness: &Value,
    position: [f64; 3],
    rotation: [f64; 4],
    installed: &[&str],
) {
    client.call(
        "assembly_set_occurrence_pose",
        json!({"occurrence_id":witness,"local_pose":{"translation":position,"rotation":rotation}}),
    );
    let mut occurrences: Vec<_> = installed
        .iter()
        .map(|id| part(exports, id)["occurrence_id"].clone())
        .collect();
    occurrences.push(witness.clone());
    no_overlap(&client.call(
        "assembly_interference_check",
        json!({"occurrence_ids":occurrences,"clearance_threshold_mm":0.}),
    ));
}

pub(super) fn check_hardware_paths(exports: &Value) {
    let mut client = Client::restore(&exports["final_model"]);
    release_joints(&mut client, exports);

    for side in ["left", "right"] {
        let nut = format!("bridge_nut_{side}");
        let bolt = format!("bridge_screw_{side}");
        clear_path(
            &mut client,
            exports,
            &nut,
            &["frame"],
            [0., 0., -1.],
            &[20., 10., 5., 2., 0.],
        );
        clear_path(
            &mut client,
            exports,
            &bolt,
            &["frame", "nut", &nut],
            [0., 0., 1.],
            &[45., 30., 15., 5., 0.],
        );
    }

    displaced(&mut client, exports, "jaw", [85., 0., 0.]);
    clear_path(
        &mut client,
        exports,
        "thrust_nut",
        &["screw"],
        [0., 0., -1.],
        &[16., 10., 6., 3., 0.],
    );
    clear_path(
        &mut client,
        exports,
        "thrust",
        &["screw", "thrust_nut", "frame", "nut", "jaw"],
        [1., 0., 0.],
        &[35., 25., 15., 5., 0.],
    );

    displaced(
        &mut client,
        exports,
        "thrust_nut",
        rotated_offset(exports, "thrust_nut", [0.8, 0., 0.]),
    );
    assert!(
        has_overlap(&interference(
            &mut client,
            exports,
            &["screw", "thrust_nut"]
        )),
        "shaft nut must retain a front axial shoulder"
    );
    displaced(
        &mut client,
        exports,
        "thrust_nut",
        rotated_offset(exports, "thrust_nut", [0., 0., -0.5]),
    );
    assert!(
        has_overlap(&interference(
            &mut client,
            exports,
            &["thrust", "thrust_nut"]
        )),
        "fitting must support the nut before it sags off the bolt axis"
    );
    displaced(&mut client, exports, "thrust_nut", [0.; 3]);
    clear_path(
        &mut client,
        exports,
        "thrust_screw",
        &["screw", "thrust", "thrust_nut", "jaw"],
        [1., 0., 0.],
        &[35., 25., 15., 5., 0.],
    );

    let witness = driver(&mut client, exports);

    driver_clearance(
        &mut client,
        exports,
        &witness,
        [105.01, 0., 50.],
        [0., 0., 0., 1.],
        &["frame", "nut", "jaw", "screw", "thrust"],
    );
    displaced(&mut client, exports, "jaw", [0.; 3]);
    assert!(has_overlap(&client.call("assembly_interference_check",json!({"occurrence_ids":[witness,part(exports,"jaw")["occurrence_id"]],"clearance_threshold_mm":0.}))), "driver witness must detect the inaccessible assembled jaw position");

    clear_path(
        &mut client,
        exports,
        "retainer_nut",
        &["jaw", "keeper"],
        [0., 1., 0.],
        &[20., 10., 5., 2., 0.],
    );
    clear_path(
        &mut client,
        exports,
        "retainer_screw",
        &["jaw", "keeper", "retainer_nut"],
        [0., -1., 0.],
        &[100., 70., 40., 15., 0.],
    );
    let h = std::f64::consts::FRAC_1_SQRT_2;
    driver_clearance(
        &mut client,
        exports,
        &witness,
        [88.8, -50.01, 70.],
        [0., 0., -h, h],
        &["frame", "jaw", "keeper", "nut", "screw", "thrust"],
    );
    for y in [-70., 70.] {
        driver_clearance(
            &mut client,
            exports,
            &witness,
            [24., y, 45.01],
            [0., -h, 0., h],
            &["frame", "nut", "jaw"],
        );
    }

    for i in 0..2 {
        for j in 0..2 {
            let stem = format!("mount_{i}_{j}");
            let top = format!("{stem}_top_washer");
            let bottom = format!("{stem}_bottom_washer");
            let bolt = format!("{stem}_bolt");
            let nut = format!("{stem}_nut");
            clear_path(
                &mut client,
                exports,
                &top,
                &["frame", "jaw", "nut"],
                [0., 0., 1.],
                &[40., 20., 5., 0.],
            );
            clear_path(
                &mut client,
                exports,
                &bolt,
                &["frame", "jaw", &top],
                [0., 0., 1.],
                &[55., 35., 15., 0.],
            );
            clear_path(
                &mut client,
                exports,
                &bottom,
                &["frame", &bolt],
                [0., 0., -1.],
                &[25., 10., 3., 0.],
            );
            clear_path(
                &mut client,
                exports,
                &nut,
                &["frame", &bolt, &bottom],
                [0., 0., -1.],
                &[25., 10., 3., 0.],
            );
            driver_clearance(
                &mut client,
                exports,
                &witness,
                [[70., 155.][i], [-68., 68.][j], 21.61],
                [0., -h, 0., h],
                &["frame", "jaw", "nut", &top],
            );
        }
    }
}
