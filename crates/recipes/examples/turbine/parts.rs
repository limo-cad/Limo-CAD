//! Editable supports, purchased bearing contact envelopes and motor cartridge.
use super::*;

impl Author {
    pub(super) fn supports(&mut self) {
        self.block(
            "base",
            ([-75., -70.], [95., 70.]),
            (0., D.base_height),
            "new_body",
            None,
        );
        self.cylinder(
            "base_shaft_clearance",
            [0., 0.],
            18.,
            (0., D.base_height),
            "cut",
            Some("base"),
        );

        self.begin("lower_collar_key_access", "xz", 0.);
        self.circle([0., D.lower_collar() + D.collar_width / 2.], 6.);
        self.extrude("lower_collar_key_access", 70., "cut", Some("base"));

        let key_center = D.lower_collar() + D.collar_width / 2.;
        let tangent = 3. * std::f64::consts::FRAC_1_SQRT_2;
        let ceiling = key_center + 3. * std::f64::consts::SQRT_2 - 0.5;
        self.begin("lower_collar_key_roof", "xz", 0.);
        self.polygon(&[
            [-tangent, key_center + tangent],
            [tangent, key_center + tangent],
            [0.5, ceiling],
            [-0.5, ceiling],
        ]);
        self.extrude("lower_collar_key_roof", 70., "cut", Some("base"));
        let g = D.guard_bolt_radius * std::f64::consts::FRAC_1_SQRT_2;
        for (x, y) in [
            (-22., 0.),
            (22., 0.),
            (D.gear_spacing - 10., 34.),
            (D.gear_spacing + 10., 34.),
            (10. + g, g),
            (10. - g, g),
            (10. + g, -g),
            (10. - g, -g),
        ] {
            let n = self.uid("base_fastener");
            self.cylinder(&n, [x, y], 3.4, (0., D.base_height), "cut", Some("base"));
            let n = self.uid("base_head_recess");
            self.cylinder(&n, [x, y], 6.4, (0., 3.2), "cut", Some("base"));
        }
        for x in [-63., 83.] {
            for y in [-58., 58.] {
                let n = self.uid("bench_anchor");
                self.cylinder(&n, [x, y], 5.5, (0., D.base_height), "cut", Some("base"));
            }
        }
        self.round_vertical_corners(
            "base",
            &[[-75., -70.], [95., -70.], [95., 70.], [-75., 70.]],
            4.,
        );
        self.component(
            "base",
            "Base / accessible collar / optional M5 bench anchors",
            true,
            [0., 0., 0.],
        );
        self.cylinder("tower", [0., 0.], 52., (0., 6.), "new_body", None);
        self.cylinder(
            "tower_column",
            [0., 0.],
            36.,
            (0., 42.),
            "join",
            Some("tower"),
        );
        self.cylinder(
            "tower_relief",
            [0., 0.],
            17.8,
            (0., 42.),
            "cut",
            Some("tower"),
        );
        self.cylinder(
            "tower_lower_seat",
            [0., 0.],
            22.3,
            (0., 7.),
            "cut",
            Some("tower"),
        );
        self.cylinder(
            "tower_upper_seat",
            [0., 0.],
            22.3,
            (35., 7.),
            "cut",
            Some("tower"),
        );
        self.block(
            "tower_split",
            ([-0.6, -27.], [0.6, 0.]),
            (0., 42.),
            "cut",
            Some("tower"),
        );
        self.clamp("tower_clamp_lower", "tower", (-14., 4.5), 3.2, 5., 6.4);
        self.clamp("tower_clamp_upper", "tower", (-14., 37.5), 3.2, 5., 6.4);
        for x in [-22., 22.] {
            let n = self.uid("tower_mount");
            self.cylinder(&n, [x, 0.], 3.4, (0., 6.), "cut", Some("tower"));
        }
        self.component(
            "tower",
            "Split 608ZZ bearing carrier / captive M3 nuts",
            true,
            [0., 0., D.base_height],
        );

        self.ring("bearing", 22., D.bearing_outer_recess, 7.);
        self.component(
            "bearing",
            "608ZZ outer race / purchased envelope",
            false,
            [0., 0., D.base_height],
        );
        self.repeat("bearing_upper", "bearing", [0., 0., D.upper_bearing()]);
        self.ring("bearing_inner", D.bearing_inner_shoulder, 8., 7.);
        self.component(
            "bearing_inner",
            "608ZZ inner race / shaft abutment",
            false,
            [0., 0., D.base_height],
        );
        self.repeat(
            "bearing_inner_upper",
            "bearing_inner",
            [0., 0., D.upper_bearing()],
        );
        self.ring("bearing_shield", 19., 12.35, 0.2);
        self.component(
            "bearing_shield",
            "608ZZ noncontact shield / recessed envelope",
            false,
            [0., 0., D.base_height + 0.2],
        );
        self.repeat(
            "bearing_shield_lower_top",
            "bearing_shield",
            [0., 0., D.base_height + 6.6],
        );
        self.repeat(
            "bearing_shield_upper",
            "bearing_shield",
            [0., 0., D.upper_bearing() + 0.2],
        );
        self.repeat(
            "bearing_shield_upper_top",
            "bearing_shield",
            [0., 0., D.upper_bearing() + 6.6],
        );
        self.cylinder(
            "shaft",
            [0., 0.],
            8.,
            (0., D.shaft_length),
            "new_body",
            None,
        );
        self.component(
            "shaft",
            "Purchased straight 8 x300 mm steel shaft",
            false,
            [0., 0., D.shaft_bottom],
        );
        self.ring("washer", D.shim_diameter, D.shim_bore, D.shim_width);
        self.component(
            "washer",
            "Purchased 8 x12 x1 mm inner-race shim",
            false,
            [0., 0., D.upper_shim()],
        );
        self.repeat("washer_lower", "washer", [0., 0., D.lower_shim()]);
        self.ring("collar", D.collar_diameter, 8., D.collar_width);
        self.begin("collar_set_screw_envelope", "xz", 3.8);
        self.circle([0., D.collar_width / 2.], 4.);
        self.extrude("collar_set_screw_envelope", 4.2, "cut", Some("collar"));
        self.component(
            "collar",
            "Purchased 8 x16 x8 shaft collar / M4x4 set screw",
            false,
            [0., 0., D.lower_collar()],
        );
        self.repeat("collar_upper", "collar", [0., 0., D.upper_collar()]);
        self.ring("rotor_support_sleeve", 18., 8.3, D.support_length());
        self.round_rim("rotor_support_sleeve", 9., D.support_length(), 0.5);
        self.component(
            "rotor_support_sleeve",
            "Positive rotor support sleeve / print upright",
            true,
            [0., 0., D.support()],
        );
    }

    pub(super) fn adjustable_generator(&mut self) {
        self.block(
            "motor_bracket",
            ([-10., 18.], [10., 34.]),
            (0., 4.),
            "new_body",
            None,
        );
        for x in [-10., 10.] {
            let n = self.uid("motor_bracket_foot");
            self.cylinder(&n, [x, 34.], 10., (0., 4.), "join", Some("motor_bracket"));
            let n = self.uid("motor_bracket_bolt");
            self.cylinder(&n, [x, 34.], 3.4, (0., 4.), "cut", Some("motor_bracket"));
        }
        self.block(
            "motor_bracket_upright",
            ([-13.5, 23.], [13.5, 29.]),
            (0., 44.),
            "join",
            Some("motor_bracket"),
        );
        let mid = D.motor_cradle() - D.base_height + 12.;
        for x in [-10., 10.] {
            for z in [mid - D.motor_adjustment, mid + D.motor_adjustment] {
                let n = self.uid("motor_adjust_slot_end");
                self.begin(&n, "xz", -29.);
                self.circle([x, z], 3.4);
                self.extrude(&n, 6., "cut", Some("motor_bracket"));
            }
            let n = self.uid("motor_adjust_slot_web");
            self.begin(&n, "xz", -29.);
            self.polygon(&[
                [x - 1.7, mid - D.motor_adjustment],
                [x + 1.7, mid - D.motor_adjustment],
                [x + 1.7, mid + D.motor_adjustment],
                [x - 1.7, mid + D.motor_adjustment],
            ]);
            self.extrude(&n, 6., "cut", Some("motor_bracket"));
        }
        self.round_vertical_corners(
            "motor_bracket",
            &[[-13.5, 23.], [13.5, 23.], [13.5, 29.], [-13.5, 29.]],
            1.,
        );
        self.component(
            "motor_bracket",
            "Fixed generator bracket / 12 mm axial slot travel",
            true,
            [D.gear_spacing, 0., D.base_height],
        );
        self.motor_cradle();
        self.component(
            "motor_mount",
            "Replaceable generator cartridge / terminal relief",
            true,
            [D.gear_spacing, 0., D.motor_cradle()],
        );
        self.cylinder(
            "motor",
            [0., 0.],
            D.motor_case_diameter,
            (0., D.motor_case_length),
            "new_body",
            None,
        );
        self.component(
            "motor",
            "KW-GEN3 provisional case / measure delivered specimen",
            false,
            [D.gear_spacing, 0., D.motor_case()],
        );
        self.cylinder(
            "motor_shaft",
            [0., 0.],
            2.,
            (0., D.motor_shaft_length),
            "new_body",
            None,
        );
        self.component(
            "motor_shaft",
            "KW-GEN3 provisional projecting shaft",
            false,
            [D.gear_spacing, 0., D.motor_shaft()],
        );
        self.set_pose(
            "motor_shaft",
            [D.gear_spacing, 0., D.motor_shaft()],
            turbine_hardware::rz(10.),
        );
    }

    pub(super) fn transmission_guard(&mut self) {
        let g = D.guard_bolt_radius * std::f64::consts::FRAC_1_SQRT_2;
        self.ring(
            "guard",
            D.guard_diameter,
            D.guard_diameter - 2. * D.guard_wall,
            D.guard_height,
        );
        for (x, y) in [(g, g), (-g, g), (g, -g), (-g, -g)] {
            let n = self.uid("guard_boss");
            self.cylinder(&n, [x, y], 8., (0., D.guard_height), "join", Some("guard"));
            let n = self.uid("guard_bore");
            self.cylinder(&n, [x, y], 3.4, (0., D.guard_height), "cut", Some("guard"));
            for z in [0., D.guard_height - 3.] {
                let n = self.uid("guard_captive_nut");
                self.nut_pocket(&n, "guard", [x, y], z);
            }
        }

        self.begin("guard_wire_exit", "yz", 57.);
        self.polygon(&[[-4., 8.], [4., 8.], [4., 12.], [0., 16.], [-4., 12.]]);
        self.extrude("guard_wire_exit", 12., "cut", Some("guard"));
        for y in [-7., 7.] {
            let n = self.uid("wire_strain_relief_tie");
            self.begin(&n, "yz", 57.);
            self.circle([y, 10.], 3.);
            self.extrude(&n, 12., "cut", Some("guard"));
        }
        self.round_rim("guard", D.guard_diameter / 2., D.guard_height, 0.6);
        self.component(
            "guard",
            "Guard / captive nuts / separate wire exit and tie holes",
            true,
            [10., 0., D.base_height],
        );
        self.cylinder(
            "guard_lid",
            [0., 0.],
            D.guard_diameter,
            (0., 3.),
            "new_body",
            None,
        );
        self.cylinder(
            "guard_lid_axis",
            [-10., 0.],
            30.,
            (0., 3.),
            "cut",
            Some("guard_lid"),
        );
        for (x, y) in [(g, g), (-g, g), (g, -g), (-g, -g)] {
            let n = self.uid("lid_hole");
            self.cylinder(&n, [x, y], 3.4, (0., 3.), "cut", Some("guard_lid"));
        }
        self.round_rim("guard_lid", D.guard_diameter / 2., 3., 0.6);
        self.round_rim("guard_lid", 15., 3., 0.4);
        self.component(
            "guard_lid",
            "Removable rounded transmission lid",
            true,
            [10., 0., D.base_height + D.guard_height],
        );
    }
}
