use super::*;

/// Installation checks deliberately release the kinematic mates. A solved
/// joint cannot establish that a real part can enter its installed position.
struct Fixture {
    client: Client,
    scene: limo_cad_solid::SolidSceneDto,
    original: Vec<limo_cad_sketch::InstanceBodyPoseDto>,
    aliases: std::collections::BTreeMap<String, u64>,
}

impl Fixture {
    fn new(exports: &Value) -> Self {
        assert!(
            exports["final_assembly"]["component_structure"]["occurrences"]
                .as_array()
                .unwrap()
                .iter()
                .all(|occurrence| occurrence["parent_occurrence_id"].is_null()),
            "this installation fixture uses root occurrence world poses as local poses"
        );
        let mut client = Client::restore(&exports["final_model"]);
        let original = solved(&mut client);
        assert_eq!(
            original["instance_body_poses"],
            exports["final_solution"]["instance_body_poses"]
        );
        no_overlap(&client.call(
            "assembly_interference_check",
            json!({"clearance_threshold_mm":0.}),
        ));
        for joint in exports["final_assembly"]["joints"].as_array().unwrap() {
            client.call(
                "assembly_set_joint_enabled",
                json!({"joint_id":joint["id"],"enabled":false}),
            );
        }
        let aliases = exports["occurrences"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(alias, id)| (alias.clone(), id.as_u64().unwrap()))
            .collect();
        Self {
            client,
            scene: serde_json::from_value(exports["final_scene"].clone()).unwrap(),
            original: serde_json::from_value(
                exports["final_solution"]["instance_body_poses"].clone(),
            )
            .unwrap(),
            aliases,
        }
    }

    fn id(&self, alias: &str) -> u64 {
        *self
            .aliases
            .get(alias)
            .unwrap_or_else(|| panic!("missing turbine occurrence {alias}"))
    }

    fn offset(&mut self, alias: &str, delta: [f64; 3]) {
        let id = self.id(alias);
        let home = self
            .original
            .iter()
            .find(|pose| pose.occurrence_id.0 == id)
            .unwrap();
        let translation: [f64; 3] = std::array::from_fn(|i| home.translation[i] + delta[i]);
        let rotation = home.rotation;
        self.place(alias, translation, rotation);
    }

    fn place(&mut self, alias: &str, translation: [f64; 3], rotation: [f64; 4]) {
        let id = self.id(alias);
        self.client.call(
            "assembly_set_occurrence_pose",
            json!({
                "occurrence_id":id,"local_pose":{"translation":translation,"rotation":rotation}
            }),
        );
        let solution = self.client.call("assembly_solution", json!({}));
        assert_eq!(solution["solved"], true);
        let actual = solution["instance_body_poses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|pose| pose["occurrence_id"] == id)
            .unwrap();
        assert!(
            (0..3).all(
                |i| (actual["translation"][i].as_f64().unwrap() - translation[i]).abs() < 1e-8
            ),
            "installation probe must actually move {alias}"
        );
        let actual_rotation = vector::<4>(&actual["rotation"]);
        for basis in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
            let expected = rotate(rotation, basis);
            let actual = rotate(actual_rotation, basis);
            assert!(
                (0..3).all(|axis| (expected[axis] - actual[axis]).abs() < 1e-8),
                "installation probe must actually orient {alias}"
            );
        }
    }

    fn driver(&mut self, diameter: f64, length: f64) -> String {
        let alias = format!("driver_{diameter}_{length}");
        if self.aliases.contains_key(&alias) {
            return alias;
        }
        self.client.call(
            "sketch_begin",
            json!({"name":alias,"plane":{"type":"origin_plane","plane":"xy"}}),
        );
        self.client
            .call("sketch_set_grid_snap", json!({"enabled":false}));
        self.client.call("sketch_add_circle_locked", json!({"mode":"center_diameter","anchor":{"x":0.,"y":0.},"edge_hint":{"x":diameter/2.,"y":0.},"diameter_mm":diameter,"ctrl_held":true}));
        self.client.call("sketch_finish", json!({}));
        let result = self.client.call("solid_extrude", json!({"sketch_name":alias,"profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":length},"taper_angle_deg":0.,"flip":false,"target_body_ids":[]}));
        assert_eq!(result["scene"]["errors"], json!([]));
        let scene: limo_cad_solid::SolidSceneDto =
            serde_json::from_value(result["scene"].clone()).unwrap();
        let body = scene
            .bodies
            .iter()
            .find(|candidate| !self.scene.bodies.iter().any(|old| old.id == candidate.id))
            .unwrap()
            .id;
        self.scene = scene;
        let component = self.client.call(
            "assembly_create_component",
            json!({"name":alias,"body_ids":[body],"absorb_promoted_bodies":true}),
        );
        let assembly = self.client.call("assembly_document", json!({}));
        let occurrence = assembly["component_structure"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .find(|occurrence| occurrence["component_id"] == component["id"])
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        self.aliases.insert(alias.clone(), occurrence);
        alias
    }

    fn contact(&mut self, a: &str, b: &str) -> Value {
        let result = self.client.call(
            "assembly_interference_check",
            json!({
                "occurrence_ids":[self.id(a),self.id(b)],"clearance_threshold_mm":1000.
            }),
        );
        assert_eq!(result["exact"], true);
        assert_eq!(
            result["pairs"].as_array().unwrap().len(),
            1,
            "contact witnesses use two distinct retained native solids"
        );
        result["pairs"][0].clone()
    }

    fn clear(&mut self, moving: &str, installed: &[&str]) {
        assert!(!installed.contains(&moving));
        let solution = self.client.call("assembly_solution", json!({}));
        let poses: Vec<limo_cad_sketch::InstanceBodyPoseDto> =
            serde_json::from_value(solution["instance_body_poses"].clone()).unwrap();
        let id = self.id(moving);
        let installed: std::collections::BTreeSet<_> =
            installed.iter().map(|part| self.id(part)).collect();
        let request = limo_cad_sketch::InterferenceCheckRequestDto {
            occurrence_ids: installed
                .iter()
                .copied()
                .chain([id])
                .map(limo_cad_sketch::OccurrenceId)
                .collect(),
            clearance_threshold_mm: 0.,
        };
        for (a, b) in
            limo_cad_sketch::broad_phase_interference_pairs(&self.scene, &poses, &request).unwrap()
        {
            let a = poses[a].occurrence_id.0;
            let b = poses[b].occurrence_id.0;
            if a != id && b != id {
                continue;
            }
            no_overlap(&self.client.call(
                "assembly_interference_check",
                json!({
                    "occurrence_ids":[a,b],"clearance_threshold_mm":0.
                }),
            ));
        }
    }

    fn path(&mut self, moving: &str, installed: &[&str], direction: [f64; 3], distances: &[f64]) {
        assert!(distances.last().is_some_and(|distance| *distance == 0.));
        for distance in distances {
            self.offset(moving, direction.map(|v| v * distance));
            self.clear(moving, installed);
        }
    }

    fn group_path(
        &mut self,
        moving: &[&str],
        installed: &[&str],
        direction: [f64; 3],
        distances: &[f64],
    ) {
        assert!(distances.last().is_some_and(|distance| *distance == 0.));
        for distance in distances {
            for part in moving {
                self.offset(part, direction.map(|v| v * distance));
            }
            for part in moving {
                self.clear(part, installed);
            }
        }
    }
}

fn assert_touch(pair: &Value, description: &str) {
    assert_eq!(pair["interfering"], false, "{description}: {pair}");
    assert!(
        pair["overlap_volume_mm3"].as_f64().unwrap() < 1e-6,
        "{description}: {pair}"
    );
    assert!(
        pair["minimum_clearance_mm"].as_f64().unwrap() < 1e-6,
        "{description} must seat on actual native faces: {pair}"
    );
}

fn assert_gap(pair: &Value, minimum: f64, description: &str) {
    assert_eq!(pair["interfering"], false, "{description}: {pair}");
    assert!(
        pair["overlap_volume_mm3"].as_f64().unwrap() < 1e-6,
        "{description}: {pair}"
    );
    assert!(
        pair["minimum_clearance_mm"].as_f64().unwrap() >= minimum,
        "{description}: {pair}"
    );
}

fn assert_penetration(pair: &Value, description: &str) {
    assert_eq!(
        pair["interfering"], true,
        "negative control must witness {description}: {pair}"
    );
    assert!(
        pair["overlap_volume_mm3"].as_f64().unwrap() > 1e-4,
        "{description}: {pair}"
    );
}

fn check_bearing_stack(fixture: &mut Fixture, exports: &Value) {
    assert_touch(
        &fixture.contact("washer", "bearing_inner_upper"),
        "upper shim on the upper inner ring",
    );
    for stationary in [
        "bearing_upper",
        "bearing_shield_upper",
        "bearing_shield_upper_top",
    ] {
        assert_gap(
            &fixture.contact("washer", stationary),
            0.05,
            "upper inner-ring shim must clear the stationary bearing envelope",
        );
    }
    for stationary in ["bearing", "bearing_shield", "bearing_shield_lower_top"] {
        assert_gap(
            &fixture.contact("washer_lower", stationary),
            0.05,
            "lower shim must clear the stationary bearing envelope",
        );
        assert_gap(
            &fixture.contact("collar", stationary),
            0.5,
            "the wide lower collar must not rub the bearing shield or outer ring",
        );
    }
    let endplay = exports["design"]["axial_stack"]["endplay_mm"]
        .as_f64()
        .unwrap();
    assert!(
        (0.1..=0.3).contains(&endplay),
        "the test expects a deliberately small, nonzero endplay"
    );
    let lower_gap = fixture.contact("washer_lower", "bearing_inner");
    assert_gap(&lower_gap, endplay - 1e-6, "unloaded lower shim");
    assert!((lower_gap["minimum_clearance_mm"].as_f64().unwrap() - endplay).abs() < 1e-6);

    let groups = MotionChecks::new(
        &exports["final_scene"],
        &exports["final_assembly"],
        &exports["final_solution"],
    );
    let rotor_group = groups.rigid_groups[&fixture.id("rotor_gear")];

    let mut moving_ids = std::collections::BTreeSet::new();
    let moving: Vec<String> = fixture
        .aliases
        .iter()
        .filter(|(name, id)| {
            groups.rigid_groups[*id] == rotor_group && !name.starts_with("bearing")
        })
        .filter(|(_, id)| moving_ids.insert(**id))
        .map(|(name, _)| name.clone())
        .collect();
    assert!(moving.iter().any(|name| name == "washer_lower"));
    assert!(moving.iter().any(|name| name == "stage_upper"));
    let installed: Vec<String> = fixture
        .aliases
        .keys()
        .filter(|name| !moving_ids.contains(&fixture.id(name)))
        .cloned()
        .collect();
    let installed: Vec<&str> = installed.iter().map(String::as_str).collect();
    for name in &moving {
        fixture.offset(name, [0., 0., endplay]);
    }
    for name in &moving {
        fixture.clear(name, &installed);
    }
    assert_touch(
        &fixture.contact("washer_lower", "bearing_inner"),
        "lower shim seats at the upper end of the shaft's axial float",
    );
    assert_gap(
        &fixture.contact("washer", "bearing_inner_upper"),
        endplay - 1e-6,
        "the upper shim releases instead of preloading both bearings",
    );

    fixture.offset("washer_lower", [0., 0., endplay + 0.2]);
    assert_penetration(
        &fixture.contact("washer_lower", "bearing_inner"),
        "overtravel into the lower inner ring",
    );
    for name in &moving {
        fixture.offset(name, [0.; 3]);
    }
}

fn check_entry_paths(fixture: &mut Fixture) {
    let guard_names: Vec<String> = fixture
        .aliases
        .keys()
        .filter(|name| name.starts_with("guard"))
        .cloned()
        .collect();
    let lower = [
        "bearing",
        "bearing_inner",
        "bearing_shield",
        "bearing_shield_lower_top",
    ];
    let upper = [
        "bearing_upper",
        "bearing_inner_upper",
        "bearing_shield_upper",
        "bearing_shield_upper_top",
    ];
    fixture.group_path(&lower, &["tower"], [0., 0., -1.], &[35., 20., 10., 3., 0.]);
    fixture.offset("bearing", [0., 0., 20.]);
    assert_penetration(
        &fixture.contact("bearing", "tower"),
        "lower bearing inserted through the carrier relief",
    );
    fixture.offset("bearing", [0.; 3]);
    let mut installed = vec!["tower"];
    installed.extend(lower);
    fixture.group_path(&upper, &installed, [0., 0., 1.], &[35., 20., 10., 3., 0.]);
    installed.extend(upper);
    let mut carrier = installed.clone();
    carrier.extend([
        "carrier_base_nut_0",
        "carrier_base_nut_1",
        "tower_tower_clamp_lower_screw",
        "tower_tower_clamp_lower_nut",
        "tower_tower_clamp_upper_screw",
        "tower_tower_clamp_upper_nut",
    ]);
    fixture.group_path(&carrier, &["base"], [0., 0., 1.], &[50., 25., 10., 3., 0.]);
    installed.push("base");
    fixture.path(
        "shaft",
        &installed,
        [0., 0., 1.],
        &[310., 280., 245., 100., 50., 15., 1., 0.],
    );
    installed.push("shaft");
    fixture.path(
        "washer_lower",
        &installed,
        [0., 0., -1.],
        &[30., 15., 5., 1., 0.],
    );
    installed.push("washer_lower");
    fixture.path("collar", &installed, [0., 0., -1.], &[30., 15., 5., 1., 0.]);
    installed.push("collar");
    fixture.path(
        "washer",
        &installed,
        [0., 0., 1.],
        &[250., 100., 30., 10., 1., 0.],
    );
    installed.push("washer");
    fixture.path(
        "rotor_gear",
        &installed,
        [0., 0., 1.],
        &[250., 100., 30., 10., 1., 0.],
    );
    installed.push("rotor_gear");
    fixture.path(
        "rotor_support_sleeve",
        &installed,
        [0., 0., 1.],
        &[250., 100., 30., 10., 1., 0.],
    );
    installed.push("rotor_support_sleeve");
    installed.extend(guard_names.iter().map(String::as_str));
    fixture.group_path(
        &[
            "stage",
            "stage_stage_clamp_bolt_screw",
            "stage_stage_clamp_bolt_nut",
        ],
        &installed,
        [0., 0., 1.],
        &[250., 150., 100., 30., 5., 0.],
    );
    installed.push("stage");
    fixture.group_path(
        &[
            "stage_upper",
            "stage_upper_stage_clamp_bolt_screw",
            "stage_upper_stage_clamp_bolt_nut",
        ],
        &installed,
        [0., 0., 1.],
        &[150., 100., 30., 5., 0.],
    );
    installed.push("stage_upper");
    fixture.path("cap", &installed, [0., 0., 1.], &[30., 10., 1., 0.]);
    installed.push("cap");
    fixture.path(
        "collar_upper",
        &installed,
        [0., 0., 1.],
        &[30., 10., 1., 0.],
    );
    fixture.offset("pinion", [-1., 0., 0.]);
    assert_penetration(
        &fixture.contact("pinion", "rotor_gear"),
        "gear pair forced one millimetre too close",
    );
    fixture.offset("pinion", [0.; 3]);
}

fn outward_rotation(axis: [f64; 3]) -> [f64; 4] {
    assert!((axis.iter().map(|v| v * v).sum::<f64>() - 1.).abs() < 1e-10);
    if axis[2] < -1. + 1e-10 {
        return [1., 0., 0., 0.];
    }
    let q = [-axis[1], axis[0], 0., 1. + axis[2]];
    let norm = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    q.map(|v| v / norm)
}

fn check_hardware(fixture: &mut Fixture, exports: &Value, selected: Option<&[&str]>) {
    let physical: Vec<String> = exports["occurrences"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let rows = exports["hardware"].as_array().unwrap();
    assert!(!rows.is_empty());
    let mut checked = std::collections::BTreeSet::new();
    for item in rows {
        let id = item["id"].as_str().unwrap();
        if selected.is_some_and(|aliases| !aliases.contains(&id)) {
            continue;
        }
        let parent = item["parent_id"].as_str().unwrap();
        let nut = item["kind"] == "nut";
        let enclosure_hardware = id.starts_with("guard_") || parent.starts_with("guard");
        assert!(
            checked.insert(fixture.id(id)),
            "hardware must be checked once per actual occurrence"
        );
        assert_eq!(fixture.id(id), item["occurrence_id"].as_u64().unwrap());
        let axis = vector::<3>(&item["axis"]);
        let native_pose = fixture
            .original
            .iter()
            .find(|pose| pose.occurrence_id.0 == fixture.id(id))
            .unwrap();
        let actual_axis = rotate(native_pose.rotation, [0., 0., 1.]);
        assert!(
            (0..3).all(|i| (axis[i] - actual_axis[i]).abs() < 1e-10),
            "{id}: the access direction must follow the actual hardware axis"
        );
        if !nut {
            let end = vector::<3>(&item["head_end"]);
            let top = mesh_measurement(body(&exports["final_scene"], &item["body_id"])).1[2];
            assert!(
                (0..3).all(
                    |i| (end[i] - native_pose.translation[i] - actual_axis[i] * top).abs() < 1e-5
                ),
                "{id}: the driver must start at the actual outer head face"
            );
        }

        let installed: Vec<&str> = physical
            .iter()
            .map(String::as_str)
            .filter(|name| {
                if *name == id {
                    return false;
                }
                if nut && parent.starts_with("guard") {
                    return *name == "guard";
                }

                if nut && (parent == "motor_mount" || parent == "pinion") {
                    return *name == parent;
                }
                if id.starts_with("carrier_base_nut") || id.starts_with("motor_base_nut") {
                    return *name == parent || *name == "base";
                }
                if name.starts_with("guard") && !enclosure_hardware {
                    return false;
                }
                if enclosure_hardware
                    && (name.starts_with("stage")
                        || *name == "cap"
                        || name.starts_with("collar_upper"))
                {
                    return false;
                }
                if parent == "tower" && (name.starts_with("motor") || name.starts_with("pinion")) {
                    return false;
                }
                true
            })
            .collect();
        assert!(
            installed.contains(&parent),
            "actual owning part must participate in {id} access"
        );
        eprintln!("turbine installation: {id}");
        if id.starts_with("motor_adjuster_nut") {
            for lift in [35., 20., 10., 5., 0.] {
                fixture.offset(id, [0., -12., lift]);
                fixture.clear(id, &installed);
            }
            fixture.path(id, &installed, axis, &[12., 10., 8., 6., 4., 2., 1., 0.]);
        } else {
            fixture.path(id, &installed, axis, &[35., 20., 10., 5., 2., 1., 0.]);
        }
        if nut {
            continue;
        }
        let diameter = item["driver_diameter_mm"].as_f64().unwrap();
        let length = item["driver_length_mm"].as_f64().unwrap();
        assert!(
            diameter >= 2. && length >= 40.,
            "use a real straight driver envelope"
        );
        let driver = fixture.driver(diameter, length);
        let end = vector::<3>(&item["head_end"]);
        let rotation = outward_rotation(axis);

        let mut tool_installed = installed.clone();
        tool_installed.push(id);
        for distance in [20., 10., 2., 0.] {
            fixture.place(
                &driver,
                std::array::from_fn(|i| end[i] + axis[i] * distance),
                rotation,
            );
            fixture.clear(&driver, &tool_installed);
        }
        if id == "tower_tower_clamp_upper_screw" {
            assert_penetration(
                &fixture.contact(&driver, "motor"),
                "upper carrier driver obstructed when the generator is installed too early",
            );
        }
        if id == "guard_lid_screw_0" {
            assert_penetration(
                &fixture.contact(&driver, "stage"),
                "the lower rotor stage obstructs top lid driver access when fitted too early",
            );
        }
    }
    if let Some(aliases) = selected {
        assert_eq!(
            checked.len(),
            aliases.len(),
            "every explicitly selected hardware alias must exist"
        );
    }
}

fn check_generator_installation(fixture: &mut Fixture, exports: &Value) {
    fixture.group_path(
        &["motor_bracket", "motor_base_nut_0", "motor_base_nut_1"],
        &[
            "base",
            "tower",
            "bearing",
            "bearing_upper",
            "shaft",
            "rotor_gear",
        ],
        [0., 0., 1.],
        &[40., 20., 5., 0.],
    );
    let hardware = exports["hardware"].as_array().unwrap();
    let cradle_hardware: Vec<&str> = hardware
        .iter()
        .filter(|row| row["parent_id"] == "motor_mount")
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    let mut loaded_cradle = vec!["motor_mount"];
    loaded_cradle.extend(cradle_hardware.iter().copied());
    fixture.group_path(
        &["motor", "motor_shaft"],
        &loaded_cradle,
        [0., 0., 1.],
        &[50., 35., 20., 10., 3., 0.],
    );
    let mut cartridge = loaded_cradle;
    cartridge.extend(["motor", "motor_shaft"]);

    let installed = [
        "base",
        "tower",
        "bearing",
        "bearing_upper",
        "bearing_inner",
        "bearing_inner_upper",
        "bearing_shield",
        "bearing_shield_lower_top",
        "bearing_shield_upper",
        "bearing_shield_upper_top",
        "shaft",
        "washer",
        "washer_lower",
        "collar",
        "rotor_gear",
        "motor_bracket",
    ];
    fixture.group_path(
        &cartridge,
        &installed,
        [1., 0., 0.],
        &[80., 50., 30., 15., 5., 0.],
    );
    let mut pinion_stationary = installed.to_vec();
    pinion_stationary.extend(cartridge);
    let mut pinion = vec!["pinion"];
    pinion.extend(
        hardware
            .iter()
            .filter(|row| row["parent_id"] == "pinion")
            .map(|row| row["id"].as_str().unwrap()),
    );
    fixture.group_path(
        &pinion,
        &pinion_stationary,
        [0., 0., 1.],
        &[30., 20., 10., 3., 1., 0.],
    );
}

fn check_motor_adjustment(fixture: &mut Fixture, exports: &Value) {
    let mut moving = vec!["motor_mount"];
    moving.extend(
        exports["hardware"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| {
                row["parent_id"] == "motor_mount"
                    || row["id"]
                        .as_str()
                        .unwrap()
                        .starts_with("motor_adjuster_screw")
            })
            .map(|row| row["id"].as_str().unwrap()),
    );
    let travel = vector::<2>(&exports["design"]["motor"]["axial_adjustment_mm"]);
    assert!(travel[0] <= -5. && travel[1] >= 5.);
    for distance in [travel[0], travel[0] / 2., 0., travel[1] / 2., travel[1], 0.] {
        for part in &moving {
            fixture.offset(part, [0., 0., distance]);
        }
        for part in &moving {
            fixture.clear(part, &["motor_bracket", "base"]);
        }
    }
    fixture.offset("motor_adjuster_screw_0", [0., 0., travel[1] + 0.5]);
    assert_penetration(
        &fixture.contact("motor_adjuster_screw_0", "motor_bracket"),
        "an adjustment screw driven beyond the actual closed slot end",
    );
    fixture.offset("motor_adjuster_screw_0", [0.; 3]);
}

fn check_wire_route(fixture: &mut Fixture, exports: &Value) {
    let wire = fixture.driver(3., 50.);
    let installed: Vec<&str> = exports["occurrences"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let rotation = outward_rotation([1., 0., 0.]);
    fixture.place(&wire, [45.25, 0., 24.4], rotation);
    fixture.clear(&wire, &installed);
    assert_gap(
        &fixture.contact(&wire, "motor"),
        0.49,
        "wire below the actual rear case face",
    );
    assert_gap(
        &fixture.contact(&wire, "motor_mount"),
        0.49,
        "wire through the separate cradle channel",
    );
    assert_gap(
        &fixture.contact(&wire, "guard"),
        1.,
        "wire through the pointed guard outlet",
    );
    fixture.place(&wire, [45.25, 0., 32.4], rotation);
    assert_penetration(
        &fixture.contact(&wire, "guard"),
        "a wire routed through the closed wall above its outlet",
    );
}

fn check_guard_installation(fixture: &mut Fixture, exports: &Value) {
    let names: Vec<&str> = exports["occurrences"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let guard: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| name.starts_with("guard") && !name.starts_with("guard_base_screw"))
        .collect();
    let installed: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| {
            !name.starts_with("guard")
                && !name.starts_with("stage")
                && *name != "cap"
                && !name.starts_with("collar_upper")
        })
        .collect();
    fixture.group_path(
        &guard,
        &installed,
        [0., 0., 1.],
        &[100., 60., 30., 15., 5., 1., 0.],
    );
}

pub(super) fn check(exports: &Value) {
    let mut fixture = Fixture::new(exports);
    check_bearing_stack(&mut fixture, exports);
    check_entry_paths(&mut fixture);
    check_hardware(&mut fixture, exports, None);
    check_motor_adjustment(&mut fixture, exports);
    check_generator_installation(&mut fixture, exports);
    check_guard_installation(&mut fixture, exports);
    check_wire_route(&mut fixture, exports);
}

pub(super) fn check_adjuster_access(exports: &Value) {
    let mut fixture = Fixture::new(exports);
    check_hardware(
        &mut fixture,
        exports,
        Some(&["motor_adjuster_nut_0", "motor_adjuster_nut_1"]),
    );
}
