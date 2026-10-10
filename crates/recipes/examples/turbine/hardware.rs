//! Ordinary editable purchased-part envelopes and topology-backed placement.
use super::*;

pub(super) fn rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let t = [
        2. * (q[1] * v[2] - q[2] * v[1]),
        2. * (q[2] * v[0] - q[0] * v[2]),
        2. * (q[0] * v[1] - q[1] * v[0]),
    ];
    [
        v[0] + q[3] * t[0] + q[1] * t[2] - q[2] * t[1],
        v[1] + q[3] * t[1] + q[2] * t[0] - q[0] * t[2],
        v[2] + q[3] * t[2] + q[0] * t[1] - q[1] * t[0],
    ]
}
pub(super) fn product(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
pub(super) fn rz(degrees: f64) -> [f64; 4] {
    let a = degrees.to_radians() / 2.;
    [0., 0., a.sin(), a.cos()]
}

impl Author {
    pub(super) fn hex_profile(&mut self, centre: [f64; 2], across_flats: f64, phase: f64) {
        let radius = across_flats / 3_f64.sqrt();
        let points = (0..6)
            .map(|i| {
                let t = (phase + i as f64 * 60.).to_radians();
                [centre[0] + radius * t.cos(), centre[1] + radius * t.sin()]
            })
            .collect::<Vec<_>>();
        self.polygon(&points);
    }

    pub(super) fn hardware_definitions(&mut self) {
        self.note("Hardware that can actually be assembled","Native purchased-part envelopes include screw heads, captive nuts, collar set screws and bearing race faces. Threads are represented by their nominal installed envelope; measured purchased hardware and the fit coupons still govern physical use.");
        for (id, length, diameter, head, head_height, key) in [
            ("m3x12", 12., 3., 5.5, 3., 2.5),
            ("m3x16", 16., 3., 5.5, 3., 2.5),
            ("m3x20", 20., 3., 5.5, 3., 2.5),
            ("m3x8low", 8., 3., 5.5, 1.65, 2.),
            ("m2x8", 8., 2., 3.8, 2., 1.5),
        ] {
            self.cylinder(id, [0., 0.], diameter, (-length, length), "new_body", None);
            self.cylinder(
                &format!("{id}_head"),
                [0., 0.],
                head,
                (0., head_height),
                "join",
                Some(id),
            );
            let pocket = format!("{id}_hex_key");
            self.begin(&pocket, "xy", head_height * 0.45);
            self.hex_profile([0., 0.], key, 0.);
            self.extrude(&pocket, head_height, "cut", Some(id));
            self.component(
                id,
                &format!("Purchased M{diameter} x{length} socket screw"),
                false,
                [0., 0., 0.],
            );
        }
        for (id, af, bore, height) in [("m3_nut", 5.5, 3., 2.4), ("m2_nut", 4., 2., 1.6)] {
            self.begin(id, "xy", 0.);
            self.hex_profile([0., 0.], af, 0.);
            let built = self.extrude(id, height, "new_body", None);
            self.bind(
                &format!("{id}_body"),
                select(r(&built), "/scene/bodies", json!({}), "last", ""),
            );
            self.cylinder(
                &format!("{id}_thread_envelope"),
                [0., 0.],
                bore,
                (0., height),
                "cut",
                Some(id),
            );
            self.component(
                id,
                &format!("Purchased M{bore} hex nut / AF{af}"),
                false,
                [0., 0., 0.],
            );
        }
        self.cylinder("m4x4_set", [0., 0.], 4., (-4., 4.), "new_body", None);
        self.begin("collar_set_socket", "xy", -1.5);
        self.hex_profile([0., 0.], 2., 0.);
        self.extrude("collar_set_socket", 1.5, "cut", Some("m4x4_set"));
        self.component(
            "m4x4_set",
            "Purchased DIN916 M4 x4 collar set screw",
            false,
            [0., 0., 0.],
        );

        for part in &mut self.parts {
            if part["id"].as_str().is_some_and(|s| {
                matches!(
                    s,
                    "m3x12"
                        | "m3x16"
                        | "m3x20"
                        | "m3x8low"
                        | "m2x8"
                        | "m3_nut"
                        | "m2_nut"
                        | "m4x4_set"
                )
            }) {
                part["quantity"] = json!(0);
            }
        }
    }

    pub(super) fn installed_hardware(
        &mut self,
        alias: &str,
        definition: &str,
        parent: &str,
        (local, orientation): ([f64; 3], [f64; 4]),
        kind: &str,
        (driver, length): (f64, f64),
    ) {
        let (p, q) = self.poses[parent];
        let offset = rotate(q, local);
        let translation = [p[0] + offset[0], p[1] + offset[1], p[2] + offset[2]];
        let rotation = product(q, orientation);
        let count = self
            .parts
            .iter()
            .find(|part| part["id"] == definition)
            .unwrap()["quantity"]
            .as_u64()
            .unwrap();
        if count == 0 {
            self.bind(&format!("{alias}_body"), r(&format!("{definition}_body")));
            self.bind(
                &format!("{alias}_occurrence"),
                r(&format!("{definition}_occurrence")),
            );
            self.occurrences.insert(alias.into(), occ_ref(alias));
            self.occurrences.remove(definition);
            self.parts
                .iter_mut()
                .find(|part| part["id"] == definition)
                .unwrap()["quantity"] = json!(1);
        } else {
            self.repeat(alias, definition, translation);
        }
        self.set_pose(alias, translation, rotation);
        self.mount_placed(&format!("{alias}_installed"), parent, alias, "rigid", 0.);
        let axis = rotate(rotation, [0., 0., 1.]);
        let head_height = match definition {
            "m3x12" | "m3x16" | "m3x20" => 3.,
            "m3x8low" => 1.65,
            "m2x8" => 2.,
            _ => 0.,
        };
        let end = [
            translation[0] + axis[0] * head_height,
            translation[1] + axis[1] * head_height,
            translation[2] + axis[2] * head_height,
        ];
        let (access_stage, removed) = if alias.starts_with("guard_lid_screw") {
            ("Close the guard before installing either rotor stage; remove the stages for a straight top driver during service.", vec!["stage", "stage_upper", "cap", "collar_upper"])
        } else if kind == "nut" && matches!(parent, "motor_mount" | "pinion" | "rotor_gear") {
            ("Preload the captive nut in the separate printed part before installing the motor module or meshing the gears.", vec!["tower", "motor", "guard", "guard_lid"])
        } else if parent == "guard" && kind == "nut" {
            (
                "Preload the captive nuts while the guard is separate from the turbine.",
                vec![],
            )
        } else if alias.starts_with("motor_base_nut") {
            ("Fit the bracket mounting nuts on the base before the cartridge and its adjustment screws.", vec!["motor_mount", "motor", "motor_shaft", "pinion", "guard", "guard_lid"])
        } else if alias.starts_with("carrier_base_nut") {
            ("Fit the carrier mounting nuts on the base before installing the rotor gear and upper shaft stack.", vec!["rotor_gear", "rotor_support_sleeve", "stage", "stage_upper", "cap", "collar_upper", "guard", "guard_lid"])
        } else if parent == "tower" && alias.contains("clamp") {
            (
                "Set the bearing clamps before fitting the generator module and guard.",
                vec![
                    "motor_bracket",
                    "motor_mount",
                    "motor",
                    "motor_shaft",
                    "pinion",
                    "guard",
                    "guard_lid",
                ],
            )
        } else if parent == "base" {
            (
                "Use the underside screw access before securing the base to the bench.",
                vec![],
            )
        } else if matches!(
            parent,
            "motor_mount" | "motor_bracket" | "rotor_gear" | "pinion"
        ) {
            (
                "Adjust the drive with the guard and lid removed, before fitting the rotor stages.",
                vec![
                    "guard",
                    "guard_lid",
                    "stage",
                    "stage_upper",
                    "cap",
                    "collar_upper",
                ],
            )
        } else {
            (
                "Install with the named parent; check the complete native hardware approach path.",
                vec![],
            )
        };
        self.hardware.push(json!({"id":alias,"definition_id":definition,"kind":kind,"parent_id":parent,
            "body_id":body_ref(alias),"occurrence_id":occ_ref(alias),"parent_occurrence_id":occ_ref(parent),
            "coordinate_frame":"assembly world at home; axis points out toward the tool",
            "axis":axis,"installed_origin":translation,"head_end":end,
            "driver_diameter_mm":driver,"driver_length_mm":length,
            "installed_after":[parent],"access_stage":access_stage,"access_requires_removed":removed,
            "guard_must_be_removed":removed.contains(&"guard")}));
    }

    pub(super) fn install_clamps(&mut self) {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        for c in self.clamps.clone() {
            let screw = if c.diameter < 3. {
                "m2x8"
            } else if c.target == "stage" {
                "m3x12"
            } else if c.target == "motor_mount" {
                "m3x20"
            } else {
                "m3x16"
            };
            let nut = if c.diameter < 3. { "m2_nut" } else { "m3_nut" };
            let mut parents = vec![c.target.clone()];
            if c.target == "stage" {
                parents.push("stage_upper".into());
            }
            for parent in parents {
                let prefix = format!("{}_{}", parent, c.name);
                self.installed_hardware(
                    &format!("{prefix}_screw"),
                    screw,
                    &parent,
                    ([c.half_grip, c.y, c.z], [0., s, 0., s]),
                    "bolt",
                    (if c.diameter < 3. { 2.2 } else { 3.2 }, 45.),
                );
                self.installed_hardware(
                    &format!("{prefix}_nut"),
                    nut,
                    &parent,
                    ([-c.half_grip, c.y, c.z], [0., -s, 0., s]),
                    "nut",
                    (0., 0.),
                );
            }
        }
    }

    pub(super) fn install_remaining_hardware(&mut self) {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        for parent in ["collar", "collar_upper"] {
            self.installed_hardware(
                &format!("{parent}_set_screw"),
                "m4x4_set",
                parent,
                ([0., -8., 4.], [s, 0., 0., s]),
                "bolt",
                (2.5, 90.),
            );
        }
        for (i, x) in [-22., 22.].into_iter().enumerate() {
            self.installed_hardware(
                &format!("carrier_base_screw_{i}"),
                "m3x20",
                "base",
                ([x, 0., 3.2], [1., 0., 0., 0.]),
                "bolt",
                (3.2, 45.),
            );
            self.installed_hardware(
                &format!("carrier_base_nut_{i}"),
                "m3_nut",
                "tower",
                ([x, 0., 6.], [0., 0., 0., 1.]),
                "nut",
                (0., 0.),
            );
        }
        let mid = D.motor_cradle() - D.base_height + 12.;
        for (i, x) in [-10., 10.].into_iter().enumerate() {
            self.installed_hardware(
                &format!("motor_base_screw_{i}"),
                "m3x16",
                "base",
                ([D.gear_spacing + x, 34., 3.2], [1., 0., 0., 0.]),
                "bolt",
                (3.2, 45.),
            );
            self.installed_hardware(
                &format!("motor_base_nut_{i}"),
                "m3_nut",
                "motor_bracket",
                ([x, 34., 4.], [0., 0., 0., 1.]),
                "nut",
                (0., 0.),
            );
            self.installed_hardware(
                &format!("motor_adjuster_screw_{i}"),
                "m3x12",
                "motor_bracket",
                ([x, 29., mid], [-s, 0., 0., s]),
                "bolt",
                (3.2, 45.),
            );
            self.installed_hardware(
                &format!("motor_adjuster_nut_{i}"),
                "m3_nut",
                "motor_mount",
                ([x, 20., 12.], product([s, 0., 0., s], rz(30.))),
                "nut",
                (0., 0.),
            );
        }
        let g = D.guard_bolt_radius * std::f64::consts::FRAC_1_SQRT_2;
        for (i, (x, y)) in [(g, g), (-g, g), (g, -g), (-g, -g)].into_iter().enumerate() {
            self.installed_hardware(
                &format!("guard_base_screw_{i}"),
                "m3x16",
                "base",
                ([10. + x, y, 3.2], [1., 0., 0., 0.]),
                "bolt",
                (3.2, 45.),
            );
            self.installed_hardware(
                &format!("guard_base_nut_{i}"),
                "m3_nut",
                "guard",
                ([x, y, 3.], [1., 0., 0., 0.]),
                "nut",
                (0., 0.),
            );
            self.installed_hardware(
                &format!("guard_lid_screw_{i}"),
                "m3x8low",
                "guard_lid",
                ([x, y, 3.], [0., 0., 0., 1.]),
                "bolt",
                (2.5, 45.),
            );
            self.installed_hardware(
                &format!("guard_lid_nut_{i}"),
                "m3_nut",
                "guard",
                ([x, y, D.guard_height - 3.], [0., 0., 0., 1.]),
                "nut",
                (0., 0.),
            );
        }
    }
    pub(super) fn set_pose(&mut self, name: &str, translation: [f64; 3], rotation: [f64; 4]) {
        let id = self.uid(&format!("{name}_pose"));
        self.call(&id,"assembly/joints","assembly_set_occurrence_pose",json!({"occurrence_id":occ_ref(name),"local_pose":{"translation":translation,"rotation":rotation}}));
        self.poses.insert(name.into(), (translation, rotation));
    }

    /// Both native connector frames describe the same physical point and axes.
    /// Coordinates are transformed through the actual parent occurrence, so a
    /// repeated 90-degree stage never inherits a world offset as a local one.
    pub(super) fn mount_placed(
        &mut self,
        id: &str,
        parent: &str,
        child: &str,
        kind: &str,
        angle: f64,
    ) {
        let (pa, qa) = self.poses[parent];
        let (pb, qb) = self.poses[child];
        let inverse = [-qa[0], -qa[1], -qa[2], qa[3]];
        let origin = rotate(inverse, [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]]);
        let home = product(qb, rz(-angle));
        let relative = product(inverse, home);
        let frame = |name: &str, point: [f64; 3], primary: [f64; 3], secondary: [f64; 3]| {
            let body = select(
                r("joint_geometry"),
                "/bodies",
                json!({"/id":body_ref(name)}),
                "one",
                "",
            );

            let stage = matches!(name, "stage" | "stage_upper");
            let anchor = if stage {
                json!({"/plane/normal/2":-1.,"/plane/origin/2":0.})
            } else {
                json!({"/plane/normal/2":1.})
            };
            let field = |pointer: &str| {
                select(
                    body.clone(),
                    "/faces",
                    anchor.clone(),
                    if stage { "one" } else { "first" },
                    pointer,
                )
            };
            json!({"body_id":body_ref(name),"face_id":field("/id"),"face_key":field("/key"),"kind":"planar_face",
                "frame":{"origin":point,"primary_axis":primary,"secondary_axis":secondary},
                "source_surface_frame":{"origin":field("/plane/origin"),"primary_axis":field("/plane/normal"),"secondary_axis":field("/plane/u")}})
        };
        self.call(id,"assembly/joints","assembly_create_joint",json!({"name":id.replace('_'," "),"kind":kind,
            "connector_a":frame(parent,origin,rotate(relative,[0.,0.,1.]),rotate(relative,[1.,0.,0.])),
            "connector_b":frame(child,[0.,0.,0.],[0.,0.,1.],[1.,0.,0.]),
            "flipped":true,"angle_offset_deg":angle,"linear_offset_mm":0.,
            "advanced":{"connector_a_occurrence_id":occ_ref(parent),"connector_b_occurrence_id":occ_ref(child)}}));
        self.joint_names.push(id.to_string());
    }

    pub(super) fn ring(&mut self, name: &str, outside: f64, bore: f64, height: f64) {
        self.cylinder(name, [0., 0.], outside, (0., height), "new_body", None);
        self.cylinder(
            &format!("{name}_bore"),
            [0., 0.],
            bore,
            (0., height),
            "cut",
            Some(name),
        );
    }

    pub(super) fn round_vertical_corners(&mut self, name: &str, corners: &[[f64; 2]], blend: f64) {
        let id = self.uid(&format!("{name}_touch_corners"));
        self.call(
            &format!("{id}_scene"),
            "solid/check",
            "solid_scene",
            json!({}),
        );
        let body = select(
            r(&format!("{id}_scene")),
            "/bodies",
            json!({"/id":body_ref(name)}),
            "one",
            "",
        );
        let edges = corners
            .iter()
            .map(|c| {
                select(
                    body.clone(),
                    "/edges",
                    json!({"$every":{"path":"/points","where":{"/x":c[0],"/y":c[1]}}}),
                    "one",
                    "/id",
                )
            })
            .collect::<Vec<_>>();
        self.call(
            &id,
            "solid/refine",
            "solid_fillet",
            json!({"body_id":body_ref(name),"edge_ids":edges,"radius":blend,"tangent_chain":false}),
        );
    }

    pub(super) fn round_rim(&mut self, name: &str, radius: f64, z: f64, blend: f64) {
        let id = self.uid(&format!("{name}_touch_rim"));
        self.call(
            &format!("{id}_scene"),
            "solid/check",
            "solid_scene",
            json!({}),
        );
        let body = select(
            r(&format!("{id}_scene")),
            "/bodies",
            json!({"/id":body_ref(name)}),
            "one",
            "",
        );
        let edges = select(
            body,
            "/edges",
            json!({"/circle/radius":radius,"/circle/center/z":z}),
            "all",
            "/id",
        );
        self.bind(&format!("{id}_edges"), edges);

        self.bind(
            &format!("{id}_edge"),
            json!({"$select":{"from":r(&format!("{id}_edges")),"take":"first"}}),
        );
        self.call(&id,"solid/refine","solid_fillet",json!({"body_id":body_ref(name),"edge_ids":r(&format!("{id}_edges")),"radius":blend,"tangent_chain":false}));
    }
}
