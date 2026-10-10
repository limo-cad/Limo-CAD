//! Deterministic authoring of an ordinary native recipe. This does not run CAD.
//! `cargo run -p limo-cad-recipes --example author_turbine` updates the reviewed JSONC.
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::f64::consts::PI;
#[path = "turbine/demo.rs"]
mod turbine_demo;
#[path = "turbine/design.rs"]
mod turbine_design;
#[path = "turbine/drawings.rs"]
mod turbine_drawings;
#[path = "turbine/hardware.rs"]
mod turbine_hardware;
#[path = "turbine/parts.rs"]
mod turbine_parts;
#[path = "turbine/printing.rs"]
mod turbine_printing;
use turbine_design::D;

fn p(x: f64, y: f64) -> Value {
    json!({"x":x,"y":y})
}
fn r(name: &str) -> Value {
    json!({"$ref":name})
}
fn at(name: &str, path: &str) -> Value {
    json!({"$ref":name,"pointer":path})
}
fn select(from: Value, path: &str, predicate: Value, take: &str, pointer: &str) -> Value {
    let mut v = json!({"from":from,"path":path,"where":predicate,"take":take});
    if !pointer.is_empty() {
        v["pointer"] = json!(pointer);
    }
    json!({"$select":v})
}
fn body_ref(name: &str) -> Value {
    at(&format!("{name}_body"), "/id")
}
fn occ_ref(name: &str) -> Value {
    at(&format!("{name}_occurrence"), "/id")
}
fn wrapped(text: &str) -> String {
    let mut out = String::new();
    let mut column = 0;
    for word in text.split_whitespace() {
        if column + word.len() + 1 > 92 {
            out.push('\n');
            column = 0;
        }
        if column > 0 {
            out.push(' ');
            column += 1;
        }
        out.push_str(word);
        column += word.len();
    }
    out
}
struct Author {
    steps: Vec<Value>,
    parts: Vec<Value>,
    drawings: Vec<Value>,
    serial: usize,
    occurrences: BTreeMap<String, Value>,
    poses: BTreeMap<String, ([f64; 3], [f64; 4])>,
    hardware: Vec<Value>,
    clamps: Vec<Clamp>,
    joint_names: Vec<String>,
    sketch_visibility: Option<String>,
    last_build_scene: Option<Value>,
    present_construction: bool,
    profile_view: &'static str,
}
#[derive(Clone)]
struct Clamp {
    name: String,
    target: String,
    y: f64,
    z: f64,
    diameter: f64,
    half_grip: f64,
}
impl Author {
    fn new() -> Self {
        Self {
            steps: vec![],
            parts: vec![],
            drawings: vec![],
            serial: 0,
            occurrences: BTreeMap::new(),
            poses: BTreeMap::new(),
            hardware: vec![],
            clamps: vec![],
            joint_names: vec![],
            sketch_visibility: None,
            last_build_scene: None,
            present_construction: false,
            profile_view: "current",
        }
    }
    fn call(&mut self, id: &str, _group: &str, op: &str, args: Value) {
        let name = limo_cad_recipes::authoring::feature_name_step(id, op, &args);
        let catalog: Value =
            serde_json::from_str(include_str!("../../../interface/catalog.json")).unwrap();
        let mut group = None;
        for w in catalog["workspaces"].as_array().unwrap() {
            for p in w["panels"].as_array().unwrap() {
                if p["operations"]
                    .as_array()
                    .is_some_and(|ops| ops.contains(&json!(op)))
                {
                    group = Some(format!(
                        "{}/{}",
                        w["id"].as_str().unwrap(),
                        p["id"].as_str().unwrap()
                    ));
                }
            }
        }
        for g in catalog["groups"].as_array().unwrap() {
            if g["operations"]
                .as_array()
                .is_some_and(|ops| ops.contains(&json!(op)))
            {
                group = Some(g["id"].as_str().unwrap().to_string());
            }
        }
        self.steps.push(json!({"id":id,"call":{"group":group.unwrap_or_else(||panic!("Unregistered operation {op}")),"operation":op,"arguments":args}}));
        self.steps.extend(name);
    }
    fn bind(&mut self, name: &str, value: Value) {
        self.steps.push(json!({"let":{name:value}}));
    }
    fn note(&mut self, chapter: &str, note: &str) {
        self.steps
            .push(json!({"chapter":chapter,"note":note,"duration_ms":3000}));
    }
    fn uid(&mut self, base: &str) -> String {
        self.serial += 1;
        format!("{base}_{}", self.serial)
    }
    fn begin(&mut self, name: &str, plane: &str, z: f64) {
        assert!(
            self.sketch_visibility.is_none(),
            "Previous sketch visibility was not restored"
        );

        let visibility = format!("{name}_before_sketch_visibility");
        self.call(
            &visibility,
            "document/appearance",
            "project_visibility",
            json!({}),
        );

        let scene = self
            .last_build_scene
            .clone()
            .unwrap_or_else(|| json!({"bodies":[]}));
        let isolate = format!("{name}_clear_sketch_view");
        self.call(
            &isolate,
            "document/appearance",
            "project_set_visibility",
            json!({
                "hidden_body_ids":select(scene,"/bodies",json!({}),"all","/id"),
                "hidden_sketch_names":at(&visibility,"/hidden_sketch_names"),
                "hidden_datum_plane_ids":at(&visibility,"/hidden_datum_plane_ids")
            }),
        );
        self.sketch_visibility = Some(visibility);
        self.profile_view = match plane {
            "xy" => "top",
            "xz" => "front",
            "yz" => "right",
            _ => panic!("Unknown sketch plane {plane}"),
        };
        let datum = self.uid("datum");
        self.call(&datum,"construct/planes","construction_plane_offset",json!({"name":format!("{name} / datum"),"reference":{"type":"origin_plane","plane":plane},"distance":z}));
        let id = self.uid("begin");
        self.call(&id,"sketch/draw","sketch_begin",json!({"name":name,"plane":{"type":"datum_plane","datum_id":select(r(&datum),"/planes",json!({}),"last","/datum_id")}}));
        let id = self.uid("grid");
        self.call(
            &id,
            "sketch/selection",
            "sketch_set_grid_snap",
            json!({"enabled":false}),
        );
    }
    fn circle(&mut self, center: [f64; 2], diameter: f64) {
        let point = self.uid("centre_point");
        self.call(
            &point,
            "sketch/draw",
            "sketch_add_point",
            json!({"position":p(center[0],center[1])}),
        );
        let fix = self.uid("locate_circle");
        self.call(&fix,"sketch/constrain","sketch_add_constraint",json!({"type":"fix","entity":select(at(&point,"/sketch"),"/entities",json!({"/kind":"point"}),"last","/id")}));
        let id = self.uid("circle");
        let snap = self.uid("acquire_center");
        self.call(
            &snap,
            "sketch/selection",
            "sketch_set_grid_snap",
            json!({"enabled":true}),
        );
        self.call(&id,"sketch/draw","sketch_add_circle_locked",json!({"mode":"center_diameter","anchor":p(center[0],center[1]),"edge_hint":p(center[0]+diameter/2.,center[1]),"diameter_text":diameter.to_string(),"ctrl_held":false}));
        let snap = self.uid("exact_coordinates");
        self.call(
            &snap,
            "sketch/selection",
            "sketch_set_grid_snap",
            json!({"enabled":false}),
        );
    }

    fn polygon(&mut self, points: &[[f64; 2]]) {
        let mut edges = vec![];
        let mut last = String::new();
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            let id = self.uid("edge");
            if i + 1 < points.len() {
                self.call(&id,"sketch/draw","sketch_add_line_locked",json!({"from":p(a[0],a[1]),"to_hint":p(b[0],b[1]),"length_text":((b[0]-a[0]).hypot(b[1]-a[1])).to_string(),"angle_text":((b[1]-a[1]).atan2(b[0]-a[0])*180./PI).to_string(),"ctrl_held":true}));
            } else {
                self.call(
                    &id,
                    "sketch/draw",
                    "sketch_add_line",
                    json!({"from":p(a[0],a[1]),"to_raw":p(b[0],b[1]),"ctrl_held":true}),
                );
            }
            let edge = self.uid("edge_ref");
            self.bind(
                &edge,
                select(
                    at(&id, "/sketch"),
                    "/entities",
                    json!({"/kind":"line"}),
                    "last",
                    "",
                ),
            );
            edges.push(edge);
            last = id;
        }
        let mut constraints = vec![json!({"type":"fix","entity":at(&edges[0],"/start_id")})];
        for i in 0..edges.len() {
            constraints.push(json!({"type":"coincident","a":at(&edges[i],"/end_id"),"b":at(&edges[(i+1)%edges.len()],"/start_id")}));
        }
        let id = self.uid("close_profile");
        self.call(
            &id,
            "sketch/constrain",
            "sketch_add_constraints",
            json!({"constraints":constraints}),
        );
        let _ = last;
    }
    fn extrude(
        &mut self,
        name: &str,
        height: f64,
        operation: &str,
        target: Option<&str>,
    ) -> String {
        let view = if self.present_construction {
            self.profile_view
        } else {
            "current"
        };
        self.steps
            .push(json!({"view":view,"fit":true,"target":"active_sketch","duration_ms":300}));
        let finish = self.uid("finish");
        self.call(&finish, "sketch/draw", "sketch_finish", json!({}));
        let id = self.uid("extrude");
        self.call(&id,"solid/build","solid_extrude",json!({"sketch_name":name,"profile_indices":[0],"operation":operation,"extent":{"type":"distance","distance":height},"taper_angle_deg":0,"flip":false,"target_body_ids":target.map(|n|vec![body_ref(n)]).unwrap_or_default()}));
        self.last_build_scene = Some(at(&id, "/scene"));
        let saved_visibility = self
            .sketch_visibility
            .take()
            .expect("Sketch visibility snapshot");
        let restore = format!("{name}_restore_model_view");
        self.call(
            &restore,
            "document/appearance",
            "project_set_visibility",
            r(&saved_visibility),
        );
        let visibility = self.uid("completed_feature_references");
        self.call(
            &visibility,
            "solid/reference",
            "construction_set_visibility",
            json!({"visible":false}),
        );
        if self.present_construction {
            let body = target
                .map(body_ref)
                .unwrap_or_else(|| select(r(&id), "/scene/bodies", json!({}), "last", "/id"));
            turbine_demo::construction(self, name, body);
        }
        id
    }
    fn cylinder(
        &mut self,
        name: &str,
        center: [f64; 2],
        diameter: f64,
        (z, height): (f64, f64),
        op: &str,
        target: Option<&str>,
    ) -> String {
        self.begin(name, "xy", z);
        self.circle(center, diameter);
        let id = self.extrude(name, height, op, target);
        if op == "new_body" {
            self.bind(
                &format!("{name}_body"),
                select(r(&id), "/scene/bodies", json!({}), "last", ""),
            );
        }
        id
    }
    fn block(
        &mut self,
        name: &str,
        (min, max): ([f64; 2], [f64; 2]),
        (z, height): (f64, f64),
        op: &str,
        target: Option<&str>,
    ) -> String {
        self.begin(name, "xy", z);
        self.polygon(&[
            [min[0], min[1]],
            [max[0], min[1]],
            [max[0], max[1]],
            [min[0], max[1]],
        ]);
        let id = self.extrude(name, height, op, target);
        if op == "new_body" {
            self.bind(
                &format!("{name}_body"),
                select(r(&id), "/scene/bodies", json!({}), "last", ""),
            );
        }
        id
    }
    fn cross_bore(&mut self, name: &str, target: &str, y: f64, z: f64, diameter: f64) {
        self.begin(name, "yz", -35.);
        self.circle([y, z], diameter);
        self.extrude(name, 70., "cut", Some(target));
    }
    fn clamp(
        &mut self,
        name: &str,
        target: &str,
        (y, z): (f64, f64),
        diameter: f64,
        half_grip: f64,
        seat_diameter: f64,
    ) {
        self.cross_bore(name, target, y, z, diameter);

        let n = self.uid("clamp_head_seat");
        self.begin(&n, "yz", half_grip);
        self.circle([y, z], seat_diameter);
        self.extrude(&n, 35. - half_grip, "cut", Some(target));
        let n = self.uid("clamp_hex_capture");
        self.begin(&n, "yz", -35.);
        self.hex_profile([y, z], if diameter > 3. { 5.8 } else { 4.3 }, 30.);
        self.extrude(&n, 35. - half_grip, "cut", Some(target));
        self.clamps.push(Clamp {
            name: name.into(),
            target: target.into(),
            y,
            z,
            diameter,
            half_grip,
        });
    }
    fn nut_pocket(&mut self, name: &str, target: &str, center: [f64; 2], z: f64) {
        let radius = 5.8 / (2. * (PI / 6.).cos());
        let points = (0..6)
            .map(|i| {
                let angle = i as f64 * PI / 3.;
                [
                    center[0] + radius * angle.cos(),
                    center[1] + radius * angle.sin(),
                ]
            })
            .collect::<Vec<_>>();
        self.begin(name, "xy", z);
        self.polygon(&points);
        self.extrude(name, 3., "cut", Some(target));
    }
    fn motor_cradle(&mut self) {
        self.cylinder("motor_mount", [0., 0.], 37., (0., 24.), "new_body", None);
        self.cylinder(
            "motor_mount_cavity",
            [0., 0.],
            32.6,
            (D.motor_rear_clearance, 24. - D.motor_rear_clearance),
            "cut",
            Some("motor_mount"),
        );
        self.block(
            "motor_clamp_split",
            ([-0.6, -20.], [0.6, -13.]),
            (D.motor_rear_clearance, 24. - D.motor_rear_clearance),
            "cut",
            Some("motor_mount"),
        );
        self.block(
            "motor_clamp_left",
            ([-11., -23.], [-0.6, -16.5]),
            (0., 20.),
            "join",
            Some("motor_mount"),
        );
        self.block(
            "motor_clamp_right",
            ([0.6, -23.], [8., -16.5]),
            (0., 20.),
            "join",
            Some("motor_mount"),
        );
        self.block(
            "motor_adjustment_backtab",
            ([-13.5, 16.], [13.5, 23.]),
            (0., 20.),
            "join",
            Some("motor_mount"),
        );
        self.cylinder(
            "motor_rear_terminal_opening",
            [0., 0.],
            28.,
            (0., D.motor_rear_clearance),
            "cut",
            Some("motor_mount"),
        );
        self.block(
            "motor_independent_wire_channel",
            ([12., -4.], [22., 4.]),
            (0., D.motor_rear_clearance),
            "cut",
            Some("motor_mount"),
        );
        self.clamp(
            "motor_cradle_clamp",
            "motor_mount",
            (-19.75, 16.),
            3.2,
            8.,
            6.4,
        );
        for x in [-10., 10.] {
            let n = self.uid("cradle_adjuster_bore");
            self.begin(&n, "xz", -23.);
            self.circle([x, 12.], 3.4);
            self.extrude(&n, 7., "cut", Some("motor_mount"));
            let n = self.uid("cradle_adjuster_hex");
            self.begin(&n, "xz", -20.);
            self.hex_profile([x, 12.], 5.8, 30.);

            self.extrude(&n, 10., "cut", Some("motor_mount"));
        }
        self.round_rim("motor_mount", 18.5, 24., 0.5);
    }
    fn component(&mut self, name: &str, title: &str, printable: bool, pose: [f64; 3]) {
        let (color, color_name) = match name {
            "stage" | "cap" => ([220, 142, 50], "Warm orange rotor"),
            "rotor_gear" | "pinion" => ([232, 195, 94], "Golden drive"),
            "guard" | "guard_lid" => ([46, 78, 98], "Blue guard"),
            "motor" => ([112, 126, 139], "Generator case"),
            _ if printable => ([80, 105, 106], "Teal support"),
            _ => ([169, 180, 185], "Hardware envelope"),
        };
        self.call(&format!("{name}_appearance"),"document/appearance","set_body_appearance",json!({"body_id":body_ref(name),"color":{"r":color[0],"g":color[1],"b":color[2],"a":255},"material_name":if printable{"Generic PETG / qualification pending"}else{"Purchased hardware / representative envelope"},"filament_type":if printable{"PETG"}else{""},"brand":"Generic","color_name":color_name}));
        self.call(
            &format!("{name}_component"),
            "assembly/joints",
            "assembly_create_component",
            json!({"name":title,"body_ids":[body_ref(name)],"absorb_promoted_bodies":true}),
        );
        let snap = self.uid("assembly");
        self.call(&snap, "assembly/joints", "assembly_document", json!({}));
        self.bind(
            &format!("{name}_occurrence"),
            select(
                r(&snap),
                "/component_structure/occurrences",
                json!({"/component_id":at(&format!("{name}_component"),"/id")}),
                "one",
                "",
            ),
        );
        self.call(&format!("{name}_placement"),"assembly/joints","assembly_set_occurrence_pose",json!({"occurrence_id":occ_ref(name),"local_pose":{"translation":pose,"rotation":[0,0,0,1]}}));
        self.poses.insert(name.into(), (pose, [0., 0., 0., 1.]));
        self.occurrences.insert(name.into(), occ_ref(name));
        if self.present_construction && printable {
            turbine_demo::show_body(
                self,
                &format!("completed_{name}"),
                body_ref(name),
                &format!(
                    "{title}. Inspect the completed editable part before continuing the assembly."
                ),
            );
        } else {
            self.steps.push(json!({"view":"isometric","fit":true,"component_id":at(&format!("{name}_component"),"/id"),"duration_ms":450}));
        }
        self.parts.push(json!({"id":name,"name":title,"body_id":body_ref(name),"component_id":at(&format!("{name}_component"),"/id"),"occurrence_id":occ_ref(name),"printable":printable,"material":if printable{"PETG"}else{"purchased — drawing/specimen confirmation required"},"print_pose":{"translation":[0,0,0],"rotation":[0,0,0,1]}}));
    }
    fn repeat(&mut self, name: &str, source: &str, pose: [f64; 3]) {
        self.bind(&format!("{name}_body"), r(&format!("{source}_body")));
        self.call(&format!("{name}_occurrence"),"assembly/joints","assembly_create_occurrence",json!({"name":name.replace('_'," "),"component_id":at(&format!("{source}_component"),"/id"),"local_pose":{"translation":pose,"rotation":[0,0,0,1]}}));
        let part = self.parts.iter_mut().find(|p| p["id"] == source).unwrap();
        part["quantity"] = json!(part.get("quantity").and_then(Value::as_u64).unwrap_or(1) + 1);
        self.poses.insert(name.into(), (pose, [0., 0., 0., 1.]));
        self.occurrences.insert(name.into(), occ_ref(name));
    }
    fn gear(&mut self, name: &str, teeth: usize, bore: f64, hub: f64) {
        let m = 1.;
        let pitch = m * teeth as f64 / 2.;
        let root = pitch - 1.25 * m;
        let base = pitch * (20_f64.to_radians()).cos();
        let tip = pitch + m;
        self.cylinder(name, [0., 0.], 2. * root, (0., 3.), "new_body", None);
        let inv = |radius: f64| {
            let t = ((radius / base).powi(2) - 1.).max(0.).sqrt();
            t - t.atan()
        };
        let half = PI / (2. * teeth as f64) - 0.10 / (2. * pitch);
        let angle = |rad: f64| half + inv(pitch) - inv(rad.max(base));
        let polar = |rad: f64, ang: f64| [rad * ang.cos(), rad * ang.sin()];
        let mut points = vec![polar(root - 0.15, -angle(root))];
        let start = root.max(base);
        let subdivisions = 6;
        for i in 0..=subdivisions {
            let rad = start + (tip - start) * i as f64 / subdivisions as f64;
            points.push(polar(rad, -angle(rad)));
        }
        for i in 1..=5 {
            points.push(polar(tip, -angle(tip) + 2. * angle(tip) * i as f64 / 5.));
        }
        for i in (0..subdivisions).rev() {
            let rad = start + (tip - start) * i as f64 / subdivisions as f64;
            points.push(polar(rad, angle(rad)));
        }
        points.push(polar(root - 0.15, angle(root)));

        let distance = |point: [f64; 2], a: [f64; 2], b: [f64; 2]| {
            let d = [b[0] - a[0], b[1] - a[1]];
            let t = (((point[0] - a[0]) * d[0] + (point[1] - a[1]) * d[1])
                / (d[0] * d[0] + d[1] * d[1]))
                .clamp(0., 1.);
            (point[0] - a[0] - t * d[0]).hypot(point[1] - a[1] - t * d[1])
        };
        let count = 2000;
        let mut max_error: f64 = 0.;
        for i in 0..=count {
            let radius = start + (tip - start) * i as f64 / count as f64;
            let exact = polar(radius, -angle(radius));
            max_error = max_error.max(
                points
                    .windows(2)
                    .map(|edge| distance(exact, edge[0], edge[1]))
                    .fold(f64::INFINITY, f64::min),
            );
        }
        let last = tip - (tip - start) / count as f64;
        let continuous_bound = max_error + (tip * tip - last * last) / (4. * base);
        assert!(
            continuous_bound < 0.01,
            "{teeth}-tooth involute exceeds0.01 mm chord budget: {continuous_bound}"
        );
        eprintln!("{teeth}-tooth continuous involute chord bound: {continuous_bound:.6} mm");
        let tooth = format!("{name}_tooth");
        self.begin(&tooth, "xy", 0.);
        self.polygon(&points);
        let built = self.extrude(&tooth, 3., "new_body", None);
        self.bind(
            &format!("{tooth}_body"),
            select(r(&built), "/scene/bodies", json!({}), "last", ""),
        );
        let patterned = format!("{name}_pattern");
        self.call(&patterned,"solid/pattern","solid_circular_pattern",json!({"body_ids":[body_ref(&tooth)],"axis_origin":{"x":0,"y":0,"z":0},"axis_direction":{"x":0,"y":0,"z":1},"count":teeth,"total_angle_deg":360}));

        let tools = select(
            r(&patterned),
            "/scene/bodies",
            json!({"$or":[{"/id":body_ref(&tooth)},{"/feature_id":select(r(&patterned),"/document/features",json!({}),"last","/id")}]}),
            "all",
            "/id",
        );
        self.call(&format!("{name}_fuse"),"solid/combine","solid_combine",json!({"target_body_id":body_ref(name),"tool_body_ids":tools,"operation":"join","keep_tools":false}));
        self.cylinder(
            &format!("{name}_hub"),
            [0., 0.],
            hub,
            (0., if teeth > 30 { 12. } else { 6. }),
            "join",
            Some(name),
        );
        self.cylinder(
            &format!("{name}_bore"),
            [0., 0.],
            bore,
            (0., 15.),
            "cut",
            Some(name),
        );
        self.block(
            &format!("{name}_clamp_split"),
            ([-0.6, -hub], [0.6, 0.]),
            (3., 12.),
            "cut",
            Some(name),
        );
        self.clamp(
            &format!("{name}_clamp_bolt"),
            name,
            (
                if teeth > 30 { -7.68 } else { -3.2 },
                if teeth > 30 { 8. } else { 4.5 },
            ),
            if teeth > 30 { 3.2 } else { 2.2 },
            if teeth > 30 { 4.8 } else { 2. },
            if teeth > 30 { 6.4 } else { 4.8 },
        );
    }
}

fn main() {
    let mut a = Author::new();
    a.present_construction = true;
    a.note("The experiment","Two 198 mm rotor discs, 200 mm combined bucket height, an 8 x 300 mm shaft and a 4:1 generator drive. The printed design and representative hardware still need physical fit and output testing.");
    a.note("Repeated rotor stage","Concentric driving diameters define a bottom disc, shaft hub and two semicircular bucket walls. The clamp hub ends at 18 mm so only the 8 mm shaft divides the overlap above it. One stage definition appears twice, staggered by 90 degrees. Print each stage upright and the final cap separately.");
    let stage_plate = a.cylinder("stage", [0., 0.], 198., (0., 3.), "new_body", None);
    a.bind(
        "stage_plate_feature",
        select(
            r(&stage_plate),
            "/document/features",
            json!({}),
            "last",
            "/id",
        ),
    );
    a.cylinder("stage_hub", [0., 0.], 24., (0., 18.), "join", Some("stage"));
    a.cylinder("bucket", [40.5, 0.], 99., (0., 100.), "new_body", None);
    a.cylinder(
        "bucket_inner",
        [40.5, 0.],
        95.,
        (0., 100.),
        "cut",
        Some("bucket"),
    );
    a.block(
        "bucket_half",
        ([-15., -55.], [95., 0.]),
        (0., 100.),
        "cut",
        Some("bucket"),
    );
    a.call("bucket_pair","solid/pattern","solid_circular_pattern",json!({"body_ids":[body_ref("bucket")],"axis_origin":{"x":0,"y":0,"z":0},"axis_direction":{"x":0,"y":0,"z":1},"count":2,"total_angle_deg":360}));
    a.call("stage_join_buckets","solid/combine","solid_combine",json!({"target_body_id":body_ref("stage"),"tool_body_ids":select(r("bucket_pair"),"/scene/bodies",json!({"$or":[{"/id":body_ref("bucket")},{"/feature_id":select(r("bucket_pair"),"/document/features",json!({}),"last","/id")}]}),"all","/id"),"operation":"join","keep_tools":false}));
    a.cylinder(
        "stage_shaft_fit",
        [0., 0.],
        8.3,
        (0., 100.),
        "cut",
        Some("stage"),
    );
    a.block(
        "stage_clamp_split",
        ([-0.6, -14.], [0.6, 0.]),
        (0., 18.),
        "cut",
        Some("stage"),
    );
    a.clamp("stage_clamp_bolt", "stage", (-8., 10.), 3.2, 4., 6.4);

    a.round_vertical_corners(
        "stage",
        &[
            [-90., 0.],
            [-88., 0.],
            [-9., 0.],
            [-7., 0.],
            [7., 0.],
            [9., 0.],
            [88., 0.],
            [90., 0.],
        ],
        0.4,
    );
    a.round_rim("stage", 99., 3., 0.6);
    a.component(
        "stage",
        "Savonius stage / print twice",
        true,
        [0., 0., D.stage()],
    );
    a.repeat("stage_upper", "stage", [0., 0., D.upper_stage()]);
    a.set_pose(
        "stage_upper",
        [0., 0., D.upper_stage()],
        turbine_hardware::rz(90.),
    );
    a.cylinder("cap", [0., 0.], 198., (0., 3.), "new_body", None);
    a.cylinder("cap_bore", [0., 0.], 8.4, (0., 3.), "cut", Some("cap"));
    a.round_rim("cap", 99., 3., 0.6);
    a.round_rim("cap", 4.2, 3., 0.3);
    a.component("cap", "Rounded rotor top endplate", true, [0., 0., D.cap()]);
    a.note("A supported axial stack", "Two 608ZZ bearings support a common uncut 300 mm shaft. Narrow purchased shims contact the inner rings only. An accessible lower collar sets 0.2 mm endplay without compressing a between-bearing spacer. A printed sleeve carries the rotor onto the gear hub and upper inner-ring shim.");
    a.supports();
    a.note("A measurable generator cartridge", "The replacement cradle provides 12 mm of actual axial adjustment, an open terminal recess, and an independent wire channel. Measure the delivered motor: the default 28 mm case and 6 mm projecting shaft are explicitly provisional, not dimensions inferred from a supplier envelope.");
    a.adjustable_generator();
    a.transmission_guard();
    a.note("A real 4:1 spur pair", "A native patterned involute gives 72 and 18 teeth at module 1, 20 degrees, with 0.10 mm tooth thinning each and 45.25 mm shaft spacing. The generator pinion has a real captive M2 nut, and all screw envelopes participate in the assembly checks.");
    a.gear("rotor_gear", 72, 8.3, 24.);
    a.component(
        "rotor_gear",
        "72 tooth rotor gear / captive M3 split hub",
        true,
        [0., 0., D.gear()],
    );
    a.gear("pinion", 18, 2.2, 12.);
    a.component(
        "pinion",
        "18 tooth generator pinion / captive M2 split hub",
        true,
        [D.gear_spacing, 0., D.gear()],
    );
    a.set_pose(
        "pinion",
        [D.gear_spacing, 0., D.gear()],
        turbine_hardware::rz(10.),
    );
    a.hardware_definitions();

    a.steps.push(json!({"id":"assembly_overview_fit",
        "view":"isometric","fit":true,"duration_ms":650}));
    a.steps.push(json!({"id":"assembly_introduction","chapter":"Assemble the supported drive",
        "note":"Ground the base, constrain the bearing-supported rotor and generator, then install the screws and captive nuts. The two shaft joints are linked by the 72:18 gear relation.",
        "duration_ms":6000}));
    a.call("joint_geometry", "solid/check", "solid_scene", json!({}));
    a.call(
        "ground_base",
        "assembly/joints",
        "assembly_set_occurrence_grounded",
        json!({"occurrence_id":occ_ref("base"),"grounded":true}),
    );
    for (id, parent, child, kind, angle) in [
        ("carrier_to_base", "base", "tower", "rigid", 0.),
        (
            "cradle_bracket_to_base",
            "base",
            "motor_bracket",
            "rigid",
            0.,
        ),
        (
            "adjustable_cartridge_home",
            "motor_bracket",
            "motor_mount",
            "rigid",
            0.,
        ),
        ("motor_to_cradle", "motor_mount", "motor", "rigid", 0.),
        (
            "generator_rotation",
            "motor",
            "motor_shaft",
            "revolute",
            10.,
        ),
        ("pinion_to_shaft", "motor_shaft", "pinion", "rigid", 0.),
        ("rotor_rotation", "tower", "rotor_gear", "revolute", 0.),
        ("shaft_to_rotor_gear", "rotor_gear", "shaft", "rigid", 0.),
        (
            "positive_rotor_support",
            "rotor_gear",
            "rotor_support_sleeve",
            "rigid",
            0.,
        ),
        ("lower_stage_to_shaft", "shaft", "stage", "rigid", 0.),
        (
            "staggered_second_stage",
            "stage",
            "stage_upper",
            "rigid",
            0.,
        ),
        ("rotor_cap", "stage_upper", "cap", "rigid", 0.),
        ("lower_bearing_seat", "base", "bearing", "rigid", 0.),
        ("upper_bearing_seat", "tower", "bearing_upper", "rigid", 0.),
        ("lower_inner_ring", "shaft", "bearing_inner", "rigid", 0.),
        (
            "upper_inner_ring",
            "shaft",
            "bearing_inner_upper",
            "rigid",
            0.,
        ),
        ("lower_shield", "bearing", "bearing_shield", "rigid", 0.),
        (
            "lower_top_shield",
            "bearing",
            "bearing_shield_lower_top",
            "rigid",
            0.,
        ),
        (
            "upper_shield",
            "bearing_upper",
            "bearing_shield_upper",
            "rigid",
            0.,
        ),
        (
            "upper_top_shield",
            "bearing_upper",
            "bearing_shield_upper_top",
            "rigid",
            0.,
        ),
        ("thrust_washer", "shaft", "washer", "rigid", 0.),
        ("lower_race_shim", "shaft", "washer_lower", "rigid", 0.),
        ("shaft_retention", "shaft", "collar", "rigid", 0.),
        ("top_cap_retention", "shaft", "collar_upper", "rigid", 0.),
        ("guard_to_base", "base", "guard", "rigid", 0.),
        ("lid_to_guard", "guard", "guard_lid", "rigid", 0.),
    ] {
        a.mount_placed(id, parent, child, kind, angle);
    }
    a.call("gear_coupling","assembly/joints","assembly_create_gear_relation",json!({"name":"Printed 72:18 spur pair","joint_a":at("rotor_rotation","/id"),"joint_b":at("generator_rotation","/id"),"teeth_a":72,"teeth_b":18,"reverse":true,"phase_deg":10}));
    a.install_clamps();
    a.install_remaining_hardware();
    turbine_demo::run(&mut a);
    let print_plates = a.print_plates();
    a.note("Read the manufacturing intent","Each native part carries its own editable drawing with actual projected edges, diameter and height dimensions. Fits are provisional. Ages 8–12 with adult guidance; age 5 requires closer hands-on adult guidance. Anchor the base before any fan or wind test. Keep fingers away from the rotor and use only supervised low-energy airflow.");
    for (name,height,diameters,note) in [
        ("stage",100.,vec![198.,8.3],"PRINT 2 / disc down. 180 mm bucket sweep, 2 mm walls, 18 mm overlap. Rounded touch rim and R0.4 vertical bucket lips. Actual captive M3 nut and accessible head. The printed sleeve supports the lower stage; set the repeated stage 90 degrees and inspect the assembly before operation."),
        ("cap",3.,vec![198.,8.4],"PRINT 1 / flat. Rounded top rim. Retain above the repeated stage with the purchased 8 x16 x8 upper shaft collar; no adhesive. This is not a rated child-safe rotor."),
        ("base",D.base_height,vec![18.,3.4],"PRINT 1 / bottom down. 170 x140 x12 mm. M5 bench-anchor clearance holes. Front 6 mm tool corridor reaches the lower collar after assembly. Use narrow bearing shims, not generic M8 washers; set 0.2 mm axial float."),
        ("tower",42.,vec![52.,22.3,17.8],"PRINT 1 / flange down. 608ZZ seats 22.3 x7 mm at each end, 35 mm apart. Captive M3 nuts. Install bearings from opposite ends. Fit carrier mounting nuts before the rotor gear; set bearing clamps before the generator and check free rotation. Coupon and slicer qualification required."),
        ("rotor_support_sleeve",D.support_length(),vec![18.,8.3],"PRINT 1 / upright. Positive axial support between gear hub and lower stage. This carries rotor weight without relying on the stage clamp's axial grip. Inspect end-face seating and creep; no load rating is claimed."),
        ("motor_bracket",44.,vec![3.4],"PRINT 1 / feet down. Two vertical slots provide +/-6 mm axial cartridge travel. Two M3 base fasteners and two M3x12 cartridge adjusters. Set engagement with the measured motor, then tighten both adjusters."),
        ("motor_mount",24.,vec![37.,32.6,28.],"PRINT 1 / lower rim down. Supported clamp ears, preloaded captive nuts, 4 mm rear-terminal seat and independent wire channel. Provisional 32 mm case; measure rear boss, leads, case length and shaft projection. Default 5.4 mm pinion engagement is not a supplier guarantee."),
        ("guard",D.guard_height,vec![D.guard_diameter,D.guard_diameter-6.,3.4],"PRINT 1 / upright. Four captive nut columns. Separate pointed wire exit with tie holes for strain relief. Route leads before closing the lid and keep them below the gear plane. Gearbox access requires removal; the guard is not a safety certification."),
        ("guard_lid",3.,vec![D.guard_diameter,30.,3.4],"PRINT 1 / flat. Rounded touch rim. Four M3x8 low-profile screws, maximum head height 1.65 mm. Install lid before the rotor stages; remove stages for straight top-driver access. Check runout and screw-head clearance before motion."),
        ("rotor_gear",12.,vec![8.3,24.],"PRINT 1 / teeth down. Module1 /72 teeth /20 degree pressure angle /3 mm face /0.10 mm thinning. Captive M3 nut. Narrow shim bears only on the upper inner ring. Native geometry and coupons do not qualify strength or wear."),
        ("pinion",6.,vec![2.2,12.],"PRINT 1 / teeth down. Module1 /18 teeth /20 degree pressure angle /3 mm face. Preload captive M2 nut before meshing; nominal 2 mm motor shaft. Check measured shaft engagement, clamp slip and backlash before applying electrical load."),
        ("shaft",D.shaft_length,vec![8.],"PURCHASE / straight 8 x300 mm shaft, uncut. Deburr without shortening the bearing seats. Purchased 8 x16 x8 collars use M4x4 set screws. Verify shaft and bearing fits and inspect the exposed top before use."),
    ] {
        if name == "shaft" {
            a.steps.push(json!({
                "id":"shaft_drawing_introduction","chapter":"Specify the purchased shaft",
                "note":"The straight 8 x 300 mm shaft is purchased stock. This sheet records its diameter, length and collar requirements; check bearing fits against the delivered parts.",
                "duration_ms":6000
            }));
        }
        a.part_drawing(name,height,&diameters,note);
        if name == "pinion" {
            a.steps.push(json!({
                "chapter":"Read the generator pinion",
                "note":"Two projected views locate the 2.2 mm shaft bore and 12 mm hub. Read the tooth dimensions and captive M2 nut notes; the printed fit still needs measurement.",
                "duration_ms":4500
            }));
        }
    }
    a.steps.push(json!({
        "id":"assembly_drawing_introduction","chapter":"Read the assembly and parts list",
        "note":"The assembly views locate the rotor and generator. The following bill of materials lists printed parts and purchased hardware, with procurement and fit notes.",
        "duration_ms":6000
    }));
    a.assembly_drawing();
    a.steps
        .push(json!({"view":"isometric","fit":true,"duration_ms":600}));

    a.call(
        "assembly_final",
        "assembly/joints",
        "assembly_document",
        json!({}),
    );
    let mut checks = vec![];
    for (id, group, operation) in [
        ("final_scene", "solid/check", "solid_scene"),
        ("final_sketches", "sketch/draw", "sketch_finished"),
        ("final_model", "document/files", "cad_project_model"),
        ("final_solution", "assembly/joints", "assembly_solution"),
        ("final_assembly", "assembly/joints", "assembly_document"),
        (
            "final_interference",
            "assembly/inspect",
            "assembly_interference_check",
        ),
    ] {
        checks.push(json!({"id":id,"call":{"group":group,"operation":operation,"arguments":{}}}));
    }
    checks.push(json!({"assert":at("final_scene","/errors"),"equals":[]}));
    checks.push(json!({"assert":at("final_solution","/solved"),"equals":true}));
    checks.push(json!({"assert":at("final_solution","/diagnostics"),"equals":[]}));
    checks.push(json!({"assert":{"$count":select(r("final_sketches"),"",json!({"/dof/value":0}),"all","")},"equals":{"$count":r("final_sketches")}}));
    checks.push(json!({"assert":select(r("final_interference"),"/pairs",json!({"/interfering":true}),"all",""),"equals":[]}));
    let source = json!({"$schema":"./limo-cad-script.schema.json","version":1,"name":"Vertical-axis turbine / two-stage Savonius","starting_state":"empty","steps":a.steps,"checks":checks,"exports":{"final_scene":r("final_scene"),"final_sketches":r("final_sketches"),"final_model":r("final_model"),"final_solution":r("final_solution"),"final_assembly":r("final_assembly"),"parts":a.parts,"drawings":a.drawings,"print_plates":print_plates,"stage_plate_feature":r("stage_plate_feature"),"rotor_joint_id":at("rotor_rotation","/id"),"generator_joint_id":at("generator_rotation","/id"),"design":D.metadata(),"hardware":a.hardware,"occurrences":a.occurrences}});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/scripts/vertical-axis-turbine.limo.jsonc");
    std::fs::write(path,format!("// Generated by the Rust author_turbine example; replay uses the one native interpreter.\n// Millimetres/degrees. Native editable construction; physical qualification remains pending.\n{}\n",serde_json::to_string_pretty(&source).unwrap())).unwrap();
    author_fit_coupons();
}

fn author_fit_coupons() {
    let mut a = Author::new();
    a.note("Qualify the fits first", "Print these four separate PETG specimens in their native Z-up orientation. Use the actual measured shaft, bearing and generator. Record filament, printer profile, measured bore and clamp slip before building both rotor stages. These are fit specimens, not strength or safety certification.");
    a.cylinder("shaft_coupon", [0., 0.], 36., (0., 3.), "new_body", None);
    a.cylinder(
        "shaft_coupon_hub",
        [0., 0.],
        24.,
        (0., 18.),
        "join",
        Some("shaft_coupon"),
    );
    a.cylinder(
        "shaft_coupon_fit",
        [0., 0.],
        8.3,
        (0., 18.),
        "cut",
        Some("shaft_coupon"),
    );
    a.block(
        "shaft_coupon_split",
        ([-0.6, -14.], [0.6, 0.]),
        (0., 18.),
        "cut",
        Some("shaft_coupon"),
    );
    a.clamp(
        "shaft_coupon_clamp",
        "shaft_coupon",
        (-8., 10.),
        3.2,
        4.,
        6.4,
    );
    a.component(
        "shaft_coupon",
        "8 mm shaft / 8.3 bore / +0.3 diametral",
        true,
        [78., 70., 0.],
    );
    a.note("Bearing insertion and clamping", "The lower 608 seat opens at the bed side. Remove first-layer flare before measurement. Insert the bearing from below, then tighten the transverse M3 clamp lightly and check that the bearing still rotates freely.");
    a.cylinder("bearing_coupon", [0., 0.], 52., (0., 6.), "new_body", None);
    a.cylinder(
        "bearing_coupon_column",
        [0., 0.],
        36.,
        (0., 18.),
        "join",
        Some("bearing_coupon"),
    );
    a.cylinder(
        "bearing_coupon_relief",
        [0., 0.],
        17.8,
        (0., 18.),
        "cut",
        Some("bearing_coupon"),
    );
    a.cylinder(
        "bearing_coupon_fit",
        [0., 0.],
        22.3,
        (0., 7.),
        "cut",
        Some("bearing_coupon"),
    );
    a.block(
        "bearing_coupon_split",
        ([-0.6, -27.], [0.6, 0.]),
        (0., 18.),
        "cut",
        Some("bearing_coupon"),
    );
    a.clamp(
        "bearing_coupon_clamp",
        "bearing_coupon",
        (-14., 4.5),
        3.2,
        5.,
        6.4,
    );
    a.component(
        "bearing_coupon",
        "608 bearing / 22.3 seat / +0.3 diametral",
        true,
        [138., 70., 0.],
    );
    a.note("Measure the generator specimen", "The case and projecting shaft need separate checks. The complete small cradle reuses the exact turbine construction, including its 32.6 mm cavity, supported M3 clamp ears, hex nut captures and independent wire channel. Nominal case 32 mm and shaft 2 mm must be checked against the delivered motor.");
    a.motor_cradle();
    a.component(
        "motor_mount",
        "32 mm motor case / 32.6 cavity / +0.6 diametral",
        true,
        [78., 140., 0.],
    );

    a.gear("pinion", 18, 2.2, 12.);
    a.component(
        "pinion",
        "2 mm motor shaft / 2.2 bore / +0.2 diametral",
        true,
        [138., 140., 0.],
    );
    a.call("joint_geometry", "solid/check", "solid_scene", json!({}));
    a.call(
        "ground_coupon_plate",
        "assembly/joints",
        "assembly_set_occurrence_grounded",
        json!({"occurrence_id":occ_ref("shaft_coupon"),"grounded":true}),
    );
    for part in ["bearing_coupon", "motor_mount", "pinion"] {
        a.mount_placed(
            &format!("coupon_{part}_print_pose"),
            "shaft_coupon",
            part,
            "rigid",
            0.,
        );
    }
    for (name,height,diameters,note) in [
        ("shaft_coupon",18.,vec![8.3,24.],"FIT S / 8 mm shaft; bore 8.3 mm gives 0.3 mm diametral allowance. Actual 18 mm stage hub, M3 clamp at 10 mm, 8 mm grip, 6.4 mm head recess and 5.8 mm AF captive nut. Reduced 36 mm disc saves filament; it does not reproduce full rotor stiffness. Print disc down."),
        ("bearing_coupon",18.,vec![22.3,17.8],"FIT B / 22 mm 608 bearing; seat 22.3 x 7 mm gives 0.3 mm diametral allowance. Actual lower carrier section, M3 clamp at 4.5 mm and 10 mm grip. Print flange down. Insert from underside; remove first-layer flare and check rotation after light clamping."),
        ("motor_mount",24.,vec![32.6,37.],"FIT M / nominal 32 mm KW-GEN3 case; cavity 32.6 mm gives 0.6 mm diametral allowance. Exact open-terminal cartridge with supported ears and captive nuts. Print lower rim down. Measure the actual case, terminal locations and shaft projection before tightening the M3 clamp."),
        ("pinion",6.,vec![2.2,12.],"FIT P / nominal 2 mm motor shaft; bore 2.2 mm gives 0.2 mm diametral allowance. Exact 18T pinion and M2 clamp, 4 mm grip. Print teeth down. Provisional 6 mm projection; captive nut pocket leaves about 2.0 mm minimum tooth face locally. Check engagement, free rotation and slip under measured load."),
    ] { a.part_drawing(name,height,&diameters,note); }
    a.steps
        .push(json!({"view":"isometric","fit":true,"duration_ms":600}));
    let mut checks = vec![];
    for (id, group, operation) in [
        ("final_scene", "solid/check", "solid_scene"),
        ("final_sketches", "sketch/draw", "sketch_finished"),
        ("final_model", "document/files", "cad_project_model"),
        ("final_solution", "assembly/joints", "assembly_solution"),
    ] {
        checks.push(json!({"id":id,"call":{"group":group,"operation":operation,"arguments":{}}}));
    }
    checks.push(json!({"assert":at("final_scene","/errors"),"equals":[]}));
    checks.push(json!({"assert":at("final_solution","/solved"),"equals":true}));
    checks.push(json!({"assert":at("final_solution","/diagnostics"),"equals":[]}));
    checks.push(json!({"assert":{"$count":select(r("final_sketches"),"",json!({"/dof/value":0}),"all","")},"equals":{"$count":r("final_sketches")}}));
    let source = json!({"$schema":"./limo-cad-script.schema.json","version":1,"name":"Turbine fit coupons / measure before printing the rotor","starting_state":"empty","steps":a.steps,"checks":checks,"exports":{"parts":a.parts,"drawings":a.drawings,"final_scene":r("final_scene"),"final_sketches":r("final_sketches"),"final_model":r("final_model"),"final_solution":r("final_solution"),"occurrences":a.occurrences}});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/scripts/turbine-fit-coupons.limo.jsonc");
    std::fs::write(path,format!("// Generated by the same Rust author_turbine example; one native replay interpreter.\n// Actual fit specimens in their intended print orientations. All sizes are millimetres.\n{}\n",serde_json::to_string_pretty(&source).unwrap())).unwrap();
}
