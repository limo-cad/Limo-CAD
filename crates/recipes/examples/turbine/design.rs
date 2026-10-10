//! One dimensional contract for construction, assembly, coupons and disclosure.
use serde_json::{json, Value};

pub(super) const D: Design = Design {
    base_height: 12.,
    shaft_bottom: 2.,
    shaft_length: 300.,
    bearing_width: 7.,
    bearing_spacing: 35.,
    bearing_inner_shoulder: 12.15,
    bearing_outer_recess: 19.2,
    shim_width: 1.,
    shim_bore: 8.,
    shim_diameter: 12.,
    collar_width: 8.,
    collar_diameter: 16.,
    axial_endplay: 0.2,
    stage_height: 100.,
    cap_height: 3.,
    gear_height: 12.,
    gear_spacing: 45.25,
    guard_height: 57.,
    guard_diameter: 132.,
    guard_wall: 3.,
    guard_bolt_radius: 61.,

    motor_case_diameter: 32.,
    motor_case_length: 28.,
    motor_shaft_length: 6.,
    motor_face_gap: 0.6,
    motor_rear_clearance: 4.,
    motor_adjustment: 6.,
};

#[derive(Clone, Copy)]
pub(super) struct Design {
    pub base_height: f64,
    pub shaft_bottom: f64,
    pub shaft_length: f64,
    pub bearing_width: f64,
    pub bearing_spacing: f64,
    pub bearing_inner_shoulder: f64,
    pub bearing_outer_recess: f64,
    pub shim_width: f64,
    pub shim_bore: f64,
    pub shim_diameter: f64,
    pub collar_width: f64,
    pub collar_diameter: f64,
    pub axial_endplay: f64,
    pub stage_height: f64,
    pub cap_height: f64,
    pub gear_height: f64,
    pub gear_spacing: f64,
    pub guard_height: f64,
    pub guard_diameter: f64,
    pub guard_wall: f64,
    pub guard_bolt_radius: f64,
    pub motor_case_diameter: f64,
    pub motor_case_length: f64,
    pub motor_shaft_length: f64,
    pub motor_face_gap: f64,
    pub motor_rear_clearance: f64,
    pub motor_adjustment: f64,
}

impl Design {
    pub fn upper_bearing(self) -> f64 {
        self.base_height + self.bearing_spacing
    }
    pub fn upper_shim(self) -> f64 {
        self.upper_bearing() + self.bearing_width
    }
    pub fn gear(self) -> f64 {
        self.upper_shim() + self.shim_width
    }
    pub fn shaft_top(self) -> f64 {
        self.shaft_bottom + self.shaft_length
    }
    pub fn upper_collar(self) -> f64 {
        self.shaft_top() - self.collar_width
    }
    pub fn cap(self) -> f64 {
        self.upper_collar() - self.cap_height
    }
    pub fn upper_stage(self) -> f64 {
        self.cap() - self.stage_height
    }
    pub fn stage(self) -> f64 {
        self.upper_stage() - self.stage_height
    }
    pub fn support(self) -> f64 {
        self.gear() + self.gear_height
    }
    pub fn support_length(self) -> f64 {
        self.stage() - self.support()
    }
    pub fn lower_shim(self) -> f64 {
        self.base_height - self.axial_endplay - self.shim_width
    }
    pub fn lower_collar(self) -> f64 {
        self.lower_shim() - self.collar_width
    }
    pub fn motor_shaft(self) -> f64 {
        self.gear() - self.motor_face_gap
    }
    pub fn motor_case(self) -> f64 {
        self.motor_shaft() - self.motor_case_length
    }
    pub fn motor_cradle(self) -> f64 {
        self.motor_case() - self.motor_rear_clearance
    }
    pub fn metadata(self) -> Value {
        json!({
            "diameter_mm":180,"bucket_height_mm":200,"stage_height_mm":self.stage_height,
            "stage_angle_deg":90,"gear_module_mm":1,"rotor_teeth":72,"pinion_teeth":18,
            "pressure_angle_deg":20,"centre_distance_mm":self.gear_spacing,
            "material":"PETG","individual_print_envelope_mm":[235.5,256,256],
            "axial_stack":{"coordinate_frame":"assembly world Z, millimetres",
                "shaft":[self.shaft_bottom,self.shaft_top()],"shaft_diameter_mm":8,
                "lower_collar":[self.lower_collar(),self.lower_shim()],
                "lower_shim":[self.lower_shim(),self.base_height-self.axial_endplay],
                "lower_bearing":[self.base_height,self.base_height+self.bearing_width],
                "upper_bearing":[self.upper_bearing(),self.upper_shim()],
                "upper_shim":[self.upper_shim(),self.gear()],
                "rotor_gear":[self.gear(),self.support()],
                "rotor_support_sleeve":[self.support(),self.stage()],
                "stage":[self.stage(),self.upper_stage()],
                "stage_upper":[self.upper_stage(),self.cap()],
                "cap":[self.cap(),self.upper_collar()],
                "upper_collar":[self.upper_collar(),self.shaft_top()],
                "endplay_mm":self.axial_endplay,
                "endplay_definition":"Lower inner-race shim top to lower bearing inner-ring bottom, with rotor weight seated on the upper inner-ring shim. Set with the accessible lower collar; do not preload the bearing pair.",
                "shim_nominal_mm":{"bore":self.shim_bore,"outside":self.shim_diameter,"thickness":self.shim_width},
                "support_sleeve_mm":{"bore":8.3,"outside":18,"length":self.support_length()},
                "bearing":"608ZZ, nominal 8x22x7; SKF608-2Z shoulder/recess envelope",
                "collar":"8x16x8, DIN916 M4x4 set screw; RulandMSC-8-F dimensional envelope"},
            "motor_specimen_pending":true,
            "motor":{"nominal_case_diameter_mm":self.motor_case_diameter,
                "provisional_case_length_mm":self.motor_case_length,
                "provisional_projecting_shaft_mm":self.motor_shaft_length,
                "shaft_diameter_mm":2,"pinion_engagement_mm":self.motor_shaft_length-self.motor_face_gap,
                "rear_seat_mm":self.motor_rear_clearance,
                "wire_route":{"diameter_mm":3,"center_world_mm":[self.gear_spacing,0.,self.motor_cradle()+self.motor_rear_clearance/2.],"direction":[1,0,0],"floor_and_case_clearance_mm":self.motor_rear_clearance/2.-1.5,"qualification":"Nominal insulated-pair envelope; measure the delivered leads and terminals and preserve strain relief."},
                "axial_adjustment_mm":[-self.motor_adjustment,self.motor_adjustment],
                "qualification":"Measure delivered case, front/rear bearing bosses, shaft projection and reinforced wire/terminal envelope. Default case/projection values are not supplier dimensions."}
        })
    }
}
