//! Author the readable native command source; this does not create geometry.
//! Run from the repository root: cargo run -p nbcad-recipes --example author_vise
//! Geometry is produced only when the ordinary Rust MCP interpreter replays it.
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

fn reference(name: &str, pointer: &str) -> Value {
    json!({"$ref":name,"pointer":pointer})
}
fn select(from: Value, path: &str, criteria: Value, pointer: &str) -> Value {
    json!({"$select":{"from":from,"path":path,"where":criteria,"take":"one","pointer":pointer}})
}
struct Author {
    steps: Vec<Value>,
    planes: BTreeMap<String, Value>,
    bodies: BTreeMap<String, Value>,
    sketches: usize,
}
impl Author {
    fn call(&mut self, id: &str, group: &str, operation: &str, arguments: Value) {
        self.steps.push(json!({"id":id,"call":{"group":group,"operation":operation,"arguments":arguments}}));
    }
    fn bind(&mut self, name: &str, value: Value) {
        self.steps.push(json!({"let":{name:value}}));
    }
    fn note(&mut self, chapter: &str, note: &str) {
        self.steps.push(json!({"chapter":chapter,"note":note,"duration_ms":1800}));
    }
    fn plane(&mut self, axis: &str, distance: f64) -> Value {
        let key = format!("{axis}_{distance}");
        if let Some(plane) = self.planes.get(&key) { return plane.clone(); }
        if distance == 0. { return json!({"type":"origin_plane","plane":axis}); }
        let id = format!("datum_{}", self.planes.len());
        self.call(&id, "solid/reference", "construction_plane_offset", json!({
            "name":format!("{axis} datum at {distance} mm"),
            "reference":{"type":"origin_plane","plane":axis},"distance":distance
        }));
        let plane = json!({"type":"datum_plane","datum_id":select(reference(&id,""),"/planes",json!({"/name":format!("{axis} datum at {distance} mm")}),"/datum_id")});
        self.bind(&format!("{id}_plane"), plane);
        let result = reference(&format!("{id}_plane"), "");
        self.planes.insert(key, result.clone());
        result
    }
    fn begin(&mut self, id: &str, axis: &str, distance: f64) {
        let plane = self.plane(axis, distance);
        self.call(&format!("{id}_begin"),"sketch/draw","sketch_begin",json!({"name":id,"plane":plane}));
        self.call(&format!("{id}_snap"),"sketch/selection","sketch_set_grid_snap",json!({"enabled":false}));
        self.sketches += 1;
    }
    fn rectangle(&mut self, id: &str, low: [f64;2], high: [f64;2]) {
        self.call(&format!("{id}_profile"),"sketch/draw","sketch_add_rectangle_locked",json!({
            "mode":"two_point","anchor":{"x":low[0],"y":low[1]},
            "corner_hint":{"x":high[0],"y":high[1]},"width_mm":high[0]-low[0],
            "height_mm":high[1]-low[1],"ctrl_held":true
        }));
        let point = select(reference(&format!("{id}_profile"),"/sketch"),"/entities",json!({"/kind":"point","/position/x":low[0],"/position/y":low[1]}),"/id");
        self.call(&format!("{id}_locate"),"sketch/constrain","sketch_add_constraint",json!({"type":"fix","entity":point}));
    }
    fn circle(&mut self, id: &str, center: [f64;2], diameter: f64) {
        self.call(&format!("{id}_point"),"sketch/draw","sketch_add_point",json!({"position":{"x":center[0],"y":center[1]},"ctrl_held":true}));
        self.call(&format!("{id}_point_fix"),"sketch/constrain","sketch_add_constraint",json!({"type":"fix","entity":reference(&format!("{id}_point"),"/entities/0")}));
        self.call(&format!("{id}_profile"),"sketch/draw","sketch_add_circle_locked",json!({
            "mode":"center_diameter","anchor":{"x":center[0],"y":center[1]},
            "edge_hint":{"x":center[0]+diameter/2.,"y":center[1]},"diameter_mm":diameter,"ctrl_held":true
        }));
        self.call(&format!("{id}_locate"),"sketch/constrain","sketch_add_constraint",json!({
            "type":"center_coincident","point":reference(&format!("{id}_point"),"/entities/0"),"curve":reference(&format!("{id}_profile"),"/entities/0")
        }));
    }
    fn extrude(&mut self, id: &str, distance: f64, operation: &str, part: &str) {
        self.steps.push(json!({"view":"current","fit":true,"target":"active_sketch","duration_ms":180}));
        self.call(&format!("{id}_finish"),"sketch/draw","sketch_finish",json!({}));
        let targets = if operation == "new_body" { json!([]) } else { json!([self.body_id(part)]) };
        self.call(&format!("{id}_build"),"solid/build","solid_extrude",json!({
            "sketch_name":id,"profile_indices":[0],"operation":operation,"extent":{"type":"distance","distance":distance.abs()},
            "taper_angle_deg":0,"flip":distance<0.,"target_body_ids":targets
        }));
        if operation == "new_body" {
            let feature = json!({"$select":{"from":reference(&format!("{id}_build"),""),"path":"/document/features","take":"last","pointer":"/id"}});
            self.bind(&format!("{part}_feature"),feature);
            self.bind(&format!("{part}_body_id"),select(reference(&format!("{id}_build"),""),"/scene/bodies",json!({"/feature_id":reference(&format!("{part}_feature"),"")}),"/id"));
        }
        self.bodies.insert(part.into(), select(reference(&format!("{id}_build"),""),"/scene/bodies",json!({"/id":self.body_id(part)}),""));
    }
    fn body_id(&self, part: &str) -> Value { reference(&format!("{part}_body_id"),"") }
    fn box_shape(&mut self, id: &str, low: [f64;3], high: [f64;3], operation: &str, part: &str) {
        self.begin(id,"xy",low[2]); self.rectangle(id,[low[0],low[1]],[high[0],high[1]]);
        self.extrude(id,high[2]-low[2],operation,part);
    }
    fn cylinder_x(&mut self, id: &str, start: f64, end: f64, radius: f64, operation: &str, part: &str) {
        self.begin(id,"yz",start); self.circle(id,[0.,28.],2.*radius); self.extrude(id,end-start,operation,part);
    }
    fn cylinder_z(&mut self,id:&str,start:f64,end:f64,center:[f64;2],radius:f64,operation:&str,part:&str) {
        self.begin(id,"xy",start); self.circle(id,center,2.*radius); self.extrude(id,end-start,operation,part);
    }
    fn hexagon_z(&mut self,id:&str,start:f64,end:f64,across_flats:f64,operation:&str,part:&str) {
        self.begin(id,"xy",start);
        let radius=across_flats/3_f64.sqrt();
        let vertices:Vec<_>=(0..6).map(|i| {let angle=(i as f64*60.).to_radians(); [69.+radius*angle.cos(),12.+radius*angle.sin()]}).collect();
        for i in 0..6 {
            let from=vertices[i]; let to=vertices[(i+1)%6];
            let mut arguments=json!({"from":{"x":from[0],"y":from[1]},"to_hint":{"x":to[0],"y":to[1]},"ctrl_held":false});
            if i<5 {
                arguments["angle_deg"]=json!((to[1]-from[1]).atan2(to[0]-from[0]).to_degrees());
                if i==0 { arguments["length_mm"]=json!(radius); } else { arguments["length_text"]=json!("d1"); }
            }
            self.call(&format!("{id}_edge_{i}"),"sketch/draw","sketch_add_line_locked",arguments);
        }
        self.call(&format!("{id}_snapshot"),"sketch/draw","sketch_active",json!({}));
        self.call(&format!("{id}_locate"),"sketch/constrain","sketch_add_constraint",json!({"type":"fix","entity":json!({"$select":{"from":reference(&format!("{id}_snapshot"),""),"path":"/entities","where":{"/kind":"point"},"take":"first","pointer":"/id"}})}));
        self.extrude(id,end-start,operation,part);
    }
    fn show(&mut self, part: &str) {
        self.steps.push(json!({"view":"isometric","fit":true,"body_id":self.body_id(part),"duration_ms":400}));
    }
    fn component(&mut self, part: &str, title: &str) {
        self.bind(&format!("{part}_body"),self.bodies[part].clone());
        self.call(&format!("{part}_component"),"assembly/joints","assembly_create_component",json!({"name":title,"body_ids":[self.body_id(part)],"absorb_promoted_bodies":true}));
        self.call(&format!("{part}_assembly"),"assembly/joints","assembly_document",json!({}));
        self.bind(&format!("{part}_occurrence"),select(reference(&format!("{part}_assembly"),""),"/component_structure/occurrences",json!({"/component_id":reference(&format!("{part}_component"),"/id")}),"/id"));
    }
    fn connector(&self, part: &str, origin: [f64;3]) -> Value {
        json!({"body_id":self.body_id(part),"face_id":select(reference(&format!("{part}_body"),""),"/faces",json!({"/key":reference(&format!("{part}_anchor_face"),"/key")}),"/id"),
            "face_key":reference(&format!("{part}_anchor_face"),"/key"),"kind":"planar_face",
            "frame":{"origin":origin,"primary_axis":[1,0,0],"secondary_axis":[0,1,0]},
            "source_surface_frame":{"origin":reference(&format!("{part}_anchor_face"),"/plane/origin"),"primary_axis":reference(&format!("{part}_anchor_face"),"/plane/normal"),"secondary_axis":reference(&format!("{part}_anchor_face"),"/plane/u")}})
    }
    fn joint(&mut self, id: &str, kind: &str, a: &str, b: &str, origin: [f64;3], limits: Value) {
        let ca=self.connector(a,origin);
        let cb=self.connector(b,origin);
        // Male cylinder: start x=61.8, axis -X, radial basis +Y. Female:
        // start x=12-0.0001, axis +X, radial basis +Z. Register a male crest
        // with a female groove, including the cutter's axial start allowance.
        let phase = (90. + 180. + (61.8_f64 - 11.9999) / 2.5 * 360.).rem_euclid(360.);
        let home_twist = match id { "screw_drive" => phase, "thrust_retention" => -phase, _ => 0. };
        self.call(id,"assembly/joints","assembly_create_joint",json!({
            "name":id,"kind":kind,"connector_a":ca,"connector_b":cb,"flipped":true,
            "angle_offset_deg":0,"linear_offset_mm":0,"linear_limits":limits,
            "advanced":{"screw_pitch_mm_per_revolution":2.5,
                "connector_a_twist_deg":home_twist,
                "connector_a_occurrence_id":reference(&format!("{a}_occurrence"),""),"connector_b_occurrence_id":reference(&format!("{b}_occurrence"),"")}
        }));
    }
}
fn main() {
    let mut a=Author{steps:Vec::new(),planes:BTreeMap::new(),bodies:BTreeMap::new(),sketches:0};
    a.note("A small functional vise", "Five printable parts: frame, guided jaw, round threaded wear nut, one-piece D screw and sliding keeper. PETG is the provisional indoor material. Geometry checks do not establish a physical clamping-force rating.");
    a.call("name","document/files","cad_set_document_name",json!({"name":"D-screw vise / PETG design candidate"}));
    a.note("Frame: base and load path", "The 150 × 70 × 8 base connects a broad fixed jaw to a captive nut housing. Four rectangular slots allow fixture screws to be positioned without making the example dependent on a particular bench.");
    a.box_shape("Frame base / 150 by 70",[0.,-35.,0.],[150.,35.,8.],"new_body","frame");
    for (i,x) in [42.,104.].into_iter().enumerate() { for (j,y) in [-30.,24.].into_iter().enumerate() {
        a.box_shape(&format!("Mount slot {i}-{j} / 18 by 6"),[x,y,0.],[x+18.,y+6.,8.],"cut","frame");
    }}
    a.box_shape("Fixed jaw / broad 60 mm face",[130.,-30.,8.],[150.,30.,52.],"join","frame");
    a.box_shape("Nut housing / axial shoulders",[8.,-26.,8.],[32.,26.,52.],"join","frame");
    a.box_shape("Nut cartridge pocket / 0.4 side clearance",[11.6,-18.4,9.6],[28.4,18.4,53.],"cut","frame");
    a.cylinder_x("Housing / circular screw envelope",7.,33.,10.6,"cut","frame");
    // Low rails locate both edges of the moving jaw. They do not intersect mounting slots.
    for (id,y) in [("left",-23.),("right",19.)] {
        a.box_shape(&format!("Guide rail {id} / 4 high"),[34.,y,8.],[130.,y+4.,12.],"join","frame");
    }
    a.show("frame");

    a.note("Jaw: support and retention", "The jaw rides on the flat base between two rails. A round chamber accepts the rotating screw head. A separate U keeper drops into a transverse slot behind the head; the main forward thrust is carried by the broad front shoulder.");
    a.box_shape("Moving jaw / 60 mm gripping face",[60.,-30.,8.4],[82.,30.,52.],"new_body","jaw");
    for (id,y) in [("left",-23.4),("right",18.6)] {
        a.box_shape(&format!("Jaw guide {id} / running clearance"),[59.,y,8.3],[83.,y+4.8,12.4],"cut","jaw");
    }
    a.cylinder_x("Jaw / circular thrust chamber",59.,76.4,14.4,"cut","jaw");
    a.box_shape("Jaw / transverse keeper slot",[66.2,-20.4,13.6],[71.8,20.4,53.],"cut","jaw");
    a.hexagon_z("Jaw / bottom-loaded M3 nut pocket",8.4,11.8,5.9,"cut","jaw");
    a.cylinder_z("Jaw / retainer screw clearance",9.,53.,[69.,12.],1.7,"cut","jaw");
    a.cylinder_z("Jaw / retainer head access",49.,53.,[69.,12.],3.1,"cut","jaw");
    a.show("jaw");

    a.note("Replaceable nut cartridge", "The housing prevents nut rotation. The circular female thread uses the ISO 60-degree profile with a special 20.5 mm nominal diameter and 2.5 mm pitch: 0.25 mm radial process relief relative to M20. This custom FDM nut is not an M20 6H standard fit. Qualify its fit with the paired coupon.");
    a.box_shape("Nut cartridge / removable wear part",[12.,-18.,10.],[28.,18.,46.],"new_body","nut");
    let nut_face=select(a.bodies["nut"].clone(),"/faces",json!({"/plane/normal/0":-1}),"");
    a.bind("nut_start_face",nut_face);
    a.call("nut_thread","solid/refine","solid_hole",json!({
        "body_id":a.body_id("nut"),"face_id":reference("nut_start_face","/id"),
        "position":{"$project":{"point":[12,0,28],"basis":reference("nut_start_face","/plane")}},
        "diameter":17.5,"extent":{"type":"through_all"},"style":"simple","flip":false,
        "thread":{"standard":"iso_metric","series":"metric_fine","designation":"CUSTOM FDM 20.5 x 2.5 / ISO 60-degree form / 6H envelope; not standard M20","class":"6H","nominal_diameter":20.5,"pitch":2.5,"threads_per_inch":null,"hand":"right","depth":null,"representation":"modeled"}
    }));
    let nut_thread_step = a.steps.pop().unwrap();
    a.bodies.insert("nut".into(),select(reference("nut_thread",""),"/scene/bodies",json!({"/id":a.body_id("nut")}),""));
    a.show("nut");

    a.note("Screw: integral handle and thrust features", "Join the hand paddle, rear shoulder, retention neck and front thrust head to the exact cylinder. The expensive helix is a final refinement, so unrelated stock operations do not repeatedly rebuild it.");
    a.cylinder_x("Screw / nominal 20 mm cylinder",-50.,62.,10.,"new_body","screw");
    a.box_shape("Screw handle / integral flat paddle",[-74.,-20.,28.],[-49.,20.,36.],"join","screw");
    a.cylinder_x("Screw / rear thrust collar",61.8,66.,14.,"join","screw");
    a.cylinder_x("Screw / retention neck",65.8,72.2,6.,"join","screw");
    a.cylinder_x("Screw / captured thrust head",72.,76.,14.,"join","screw");
    a.show("screw");

    a.note("Slide-in keeper with positive retention", "The inverted U throat opens downward so the keeper drops over the neck from above. An accessible M3 x 40 socket screw secures it to a bottom-loaded trapped M3 nut in the jaw. The small fastener prevents lifting during handling; main forward thrust bypasses the keeper.");
    a.box_shape("Keeper / transverse plate",[66.6,-20.,14.],[71.4,20.,52.],"new_body","keeper");
    a.cylinder_x("Keeper / circular neck clearance",66.,72.,6.4,"cut","keeper");
    a.box_shape("Keeper / downward installation throat",[66.,-6.4,13.],[72.,6.4,28.],"cut","keeper");
    a.cylinder_z("Keeper / M3 retainer clearance",13.,53.,[69.,12.],1.7,"cut","keeper");
    a.cylinder_z("Keeper / recessed socket-head seat",49.,53.,[69.,12.],3.1,"cut","keeper");
    a.show("keeper");

    a.note("Purchased retention hardware", "M3 x 40 socket screw: 3 mm shaft, 5.5 by 3 mm head. M3 nut: 5.5 mm across flats and 2.4 mm thick. These are simplified clearance envelopes, not printed substitutes or strength-rated fastener threads. Confirm the supplier's drawing. Load the nut before sliding the jaw onto the base, then insert the keeper and fit the screw from above.");
    a.cylinder_z("Retainer screw / purchased M3 x 40 envelope",9.,49.,[69.,12.],1.5,"new_body","retainer_screw");
    a.cylinder_z("Retainer screw / socket head envelope",49.,52.,[69.,12.],2.75,"join","retainer_screw");
    a.hexagon_z("Retainer nut / purchased M3 envelope",9.4,11.8,5.5,"new_body","retainer_nut");
    a.cylinder_z("Retainer nut / simplified thread envelope",9.3,11.9,[69.,12.],1.5,"cut","retainer_nut");

    a.note("Real helical engagement and the printable D flat", "Thread the round nut and the finished cylindrical screw blank, then cut the screw flat through its axis. The remaining half thread retains a true 2.5 mm lead. Its rotating envelope is circular; the asymmetric contact needs physical fit and wear qualification.");
    a.steps.push(nut_thread_step);
    a.bind("screw_thread_face",select(a.bodies["screw"].clone(),"/faces",json!({"/cylinder/radius":10}),""));
    a.bind("male_thread_request",json!({
        "body_id":a.body_id("screw"),"face_id":reference("screw_thread_face","/id"),"flip":false,
        "thread":{"standard":"iso_metric","series":"metric_coarse","designation":"M20 x 2.5 - 6g","class":"6g","nominal_diameter":20,"pitch":2.5,"threads_per_inch":null,"hand":"right","depth":104,"representation":"modeled"}
    }));
    a.call("male_thread","solid/refine","solid_external_thread",reference("male_thread_request",""));
    a.bind("male_thread_feature_id",json!({"$select":{"from":reference("male_thread",""),"path":"/document/features","take":"last","pointer":"/id"}}));
    a.bodies.insert("screw".into(),select(reference("male_thread",""),"/scene/bodies",json!({"/id":a.body_id("screw")}),""));
    a.box_shape("Screw / D flat through axis",[-75.,-21.,7.],[77.,21.,28.],"cut","screw");
    a.show("screw");

    a.note("Assembly: one controlled motion", "Ground the frame, locate the nut and keeper, and close the screw–revolute–slider loop. The lead is 2.5 mm per turn. Command the screw angle; the jaw position and retention rotation must be solved, not independently animated.");
    for (part,title) in [("frame","Frame / fixed jaw"),("jaw","Guided moving jaw"),("nut","Custom 20.5 x 2.5 FDM wear nut"),("screw","D screw and integral paddle"),("keeper","Removable jaw keeper"),("retainer_screw","Purchased M3 x 40 socket screw"),("retainer_nut","Purchased M3 hex nut")] {
        a.component(part,title);
        if !part.starts_with("retainer_") { a.call(&format!("{part}_material"),"document/appearance","set_body_appearance",json!({"body_id":a.body_id(part),"preset_id":if part=="frame" || part=="jaw" {"bambu.petg.hf.black"} else {"bambu.petg.hf.white"}})); }
        a.bind(&format!("{part}_anchor_face"),json!({"$select":{"from":reference(&format!("{part}_body"),""),"path":"/faces","where":{"/plane/normal/2":1},"take":"first"}}));
    }
    a.call("ground_frame","assembly/joints","assembly_set_occurrence_grounded",json!({"occurrence_id":reference("frame_occurrence",""),"grounded":true}));
    a.joint("nut_in_housing","rigid","frame","nut",[20.,0.,28.],Value::Null);
    a.joint("keeper_in_jaw","rigid","jaw","keeper",[69.,0.,28.],Value::Null);
    a.joint("screw_drive","screw","frame","screw",[20.,0.,28.],json!({"min":0,"max":48}));
    a.joint("thrust_retention","revolute","screw","jaw",[69.,0.,28.],Value::Null);
    a.joint("jaw_guide","slider","frame","jaw",[69.,0.,28.],json!({"min":0,"max":48}));
    a.joint("retainer_screw_in_jaw","rigid","jaw","retainer_screw",[69.,12.,49.],Value::Null);
    a.joint("retainer_nut_in_jaw","rigid","jaw","retainer_nut",[69.,12.,10.6],Value::Null);
    a.call("home_drive","assembly/joints","assembly_set_joint_motion",json!({"joint_id":reference("screw_drive","/id"),"angle_offset_deg":0,"linear_offset_mm":0}));
    a.steps.push(json!({"view":"isometric","fit":true,"duration_ms":650}));

    a.note("Print layout and assembly order", "The screw prints on its through-axis flat. Stand the nut on its end, and put the jaw and keeper on their broad end faces. Five separated parts fit within 204 by 166 mm, including the conservative 235.5 by 256 mm dual-tool bed. Insert the nut, turn in the screw, slide on the jaw and install the keeper. Keep supports off for the first fit experiment; inspect the short housing bore bridges before loading.");
    for joint in ["nut_in_housing","keeper_in_jaw","screw_drive","thrust_retention","jaw_guide"] {
        a.call(&format!("print_disable_{joint}"),"assembly/joints","assembly_set_joint_enabled",json!({"joint_id":reference(joint,"/id"),"enabled":false}));
    }
    let quarter_y = json!([0.,std::f64::consts::FRAC_1_SQRT_2,0.,std::f64::consts::FRAC_1_SQRT_2]);
    for (part,translation,rotation) in [
        ("frame",json!([0,35,0]),json!([0,0,0,1])),
        ("screw",json!([74,105,-28]),json!([0,0,0,1])),
        ("jaw",json!([151.6,30,82]),quarter_y.clone()),
        ("nut",json!([150,148,28]),quarter_y.clone()),
        ("keeper",json!([146,95,71.4]),quarter_y),
    ] {
        a.call(&format!("print_pose_{part}"),"assembly/joints","assembly_set_occurrence_pose",json!({"occurrence_id":reference(&format!("{part}_occurrence"),""),"local_pose":{"translation":translation,"rotation":rotation}}));
    }
    a.steps.push(json!({"view":"isometric","fit":true,"duration_ms":650}));
    a.call("print_model","document/files","cad_project_model",json!({}));
    a.call("print_solution","assembly/joints","assembly_solution",json!({}));
    a.call("print_3mf","document/export","solid_export_3mf",json!({"slicer_target":"standard","body_ids":[a.body_id("frame"),a.body_id("jaw"),a.body_id("nut"),a.body_id("screw"),a.body_id("keeper")]}));
    for part in ["frame","screw","jaw","nut","keeper"] {
        a.call(&format!("restore_pose_{part}"),"assembly/joints","assembly_set_occurrence_pose",json!({"occurrence_id":reference(&format!("{part}_occurrence"),""),"local_pose":{"translation":[0,0,0],"rotation":[0,0,0,1]}}));
    }
    for joint in ["nut_in_housing","keeper_in_jaw","screw_drive","thrust_retention","jaw_guide"] {
        a.call(&format!("restore_enable_{joint}"),"assembly/joints","assembly_set_joint_enabled",json!({"joint_id":reference(joint,"/id"),"enabled":true}));
    }
    a.call("restore_home_drive","assembly/joints","assembly_set_joint_motion",json!({"joint_id":reference("screw_drive","/id"),"angle_offset_deg":0,"linear_offset_mm":0}));
    a.steps.push(json!({"view":"isometric","fit":true,"duration_ms":650}));

    let mut checks=Vec::new();
    for (id,group,operation) in [("final_scene","solid/check","solid_scene"),("final_sketches","sketch/draw","sketch_finished"),("final_assembly","assembly/joints","assembly_document"),("final_solution","assembly/joints","assembly_solution"),("final_model","document/files","cad_project_model")] {
        checks.push(json!({"id":id,"call":{"group":group,"operation":operation,"arguments":{}}}));
    }
    checks.push(json!({"assert":reference("final_scene","/errors"),"equals":[]}));
    checks.push(json!({"assert":{"$count":reference("final_scene","/bodies")},"equals":7}));
    checks.push(json!({"assert":{"$count":reference("final_sketches","")},"equals":a.sketches}));
    for index in 0..a.sketches { checks.push(json!({"assert":reference("final_sketches",&format!("/{index}/dof/value")),"equals":0})); }
    checks.push(json!({"assert":reference("final_solution","/solved"),"equals":true}));
    checks.push(json!({"assert":reference("final_solution","/diagnostics"),"equals":[]}));
    let mut exports=Map::new();
    for id in ["final_scene","final_sketches","final_assembly","final_solution","final_model"] { exports.insert(id.into(),reference(id,"")); }
    for part in ["frame","jaw","nut","screw","keeper","retainer_screw","retainer_nut"] {
        exports.insert(format!("{part}_body_id"),a.body_id(part));
        exports.insert(format!("{part}_occurrence_id"),reference(&format!("{part}_occurrence"),""));
    }
    exports.insert("screw_joint_id".into(),reference("screw_drive","/id"));
    exports.insert("jaw_joint_id".into(),reference("jaw_guide","/id"));
    exports.insert("retention_joint_id".into(),reference("thrust_retention","/id"));
    for id in ["print_model","print_solution","print_3mf","male_thread_request","male_thread_feature_id"] { exports.insert(id.into(),reference(id,"")); }
    exports.insert("design_inputs".into(),json!({"jaw_width_mm":60,"initial_opening_mm":48,"allowed_travel_mm":48,"lead_mm":2.5,"nominal_thread_mm":20,"nut_special_nominal_mm":20.5,"radial_process_relief_mm":0.25,"flat_axis_z_mm":28,"thread_engagement_mm":16,"provisional_material":"Bambu PETG HF","input_torque_Nm":0.25,"assumed_overall_efficiency":0.2,"contact_patch_mm2":600,"physical_load_rating":null}));
    let document=json!({"$schema":"./nbcad-script.schema.json","version":1,"name":"D-shaped printed screw vise","starting_state":"empty","steps":a.steps,"checks":checks,"exports":exports});
    let text=format!("// Functional FDM design candidate; dimensions in millimetres.\n// Native sketches, features and joints only. No imported mesh or captured entity IDs.\n// Authored with crates/recipes/examples/author_vise.rs; replay with cargo xtask run-script --recipe d-screw-vise.\n{}\n",serde_json::to_string_pretty(&document).unwrap());
    nbcad_script::Script::parse(&text).expect("authored source must pass preflight");
    std::fs::write("examples/scripts/d-screw-vise.nbcad.jsonc",text).unwrap();
    author_fit_coupon();
}

fn author_fit_coupon() {
    let mut a=Author{steps:Vec::new(),planes:BTreeMap::new(),bodies:BTreeMap::new(),sketches:0};
    a.note("Qualify the actual interrupted thread", "Print the male coupon on its D flat and the female coupon with its thread axis vertical. The female is a custom 20.5 × 2.5 ISO-derived profile, not a standard M20 6H fit. Record material, layer height, extrusion width, fit, required turning torque and wear before committing to a full screw.");
    a.box_shape("Coupon nut / 36 by 36 by 10",[45.,-18.,0.],[81.,18.,10.],"new_body","nut");
    a.bind("coupon_nut_face",select(a.bodies["nut"].clone(),"/faces",json!({"/plane/normal/2":1}),""));
    a.cylinder_x("Coupon screw / ten complete turns",0.,25.,10.,"new_body","screw");
    a.bind("coupon_screw_face",select(a.bodies["screw"].clone(),"/faces",json!({"/cylinder/radius":10}),""));
    a.call("coupon_female_thread","solid/refine","solid_hole",json!({
        "body_id":a.body_id("nut"),"face_id":reference("coupon_nut_face","/id"),
        "position":{"$project":{"point":[63,0,10],"basis":reference("coupon_nut_face","/plane")}},
        "diameter":17.5,"extent":{"type":"through_all"},"style":"simple","flip":false,
        "thread":{"standard":"iso_metric","series":"metric_fine","designation":"CUSTOM FDM 20.5 x 2.5 / ISO 60-degree form / 6H envelope; not standard M20","class":"6H","nominal_diameter":20.5,"pitch":2.5,"threads_per_inch":null,"hand":"right","depth":null,"representation":"modeled"}
    }));
    a.call("coupon_male_thread","solid/refine","solid_external_thread",json!({
        "body_id":a.body_id("screw"),"face_id":reference("coupon_screw_face","/id"),"flip":false,
        "thread":{"standard":"iso_metric","series":"metric_coarse","designation":"M20 x 2.5 - 6g","class":"6g","nominal_diameter":20,"pitch":2.5,"threads_per_inch":null,"hand":"right","depth":null,"representation":"modeled"}
    }));
    a.box_shape("Coupon screw / through-axis D flat",[-1.,-11.,17.],[26.,11.,28.],"cut","screw");
    a.call("coupon_screw_on_bed","solid/body","solid_move_copy",json!({
        "body_ids":[a.body_id("screw")],"translation":{"x":0,"y":0,"z":-28},
        "rotation":[0,0,0,1],"pivot":{"x":0,"y":0,"z":0},"copy":false
    }));
    a.call("coupon_positive_bed_coordinates","solid/body","solid_move_copy",json!({
        "body_ids":[a.body_id("screw"),a.body_id("nut")],"translation":{"x":0,"y":18,"z":0},
        "rotation":[0,0,0,1],"pivot":{"x":0,"y":0,"z":0},"copy":false
    }));
    for part in ["screw","nut"] {
        a.call(&format!("{part}_material"),"document/appearance","set_body_appearance",json!({"body_id":a.body_id(part),"preset_id":"bambu.petg.hf.white"}));
    }
    a.steps.push(json!({"view":"isometric","fit":true,"duration_ms":500}));
    let mut checks=vec![
        json!({"id":"final_scene","call":{"group":"solid/check","operation":"solid_scene","arguments":{}},"expect":{"/errors":[]}}),
        json!({"id":"final_model","call":{"group":"document/files","operation":"cad_project_model","arguments":{}}}),
        json!({"id":"final_sketches","call":{"group":"sketch/draw","operation":"sketch_finished","arguments":{}}}),
        json!({"assert":{"$count":reference("final_scene","/bodies")},"equals":2})
    ];
    for index in 0..a.sketches { checks.push(json!({"assert":reference("final_sketches",&format!("/{index}/dof/value")),"equals":0})); }
    let document=json!({"$schema":"./nbcad-script.schema.json","version":1,"name":"D-screw vise / paired print-fit coupons","starting_state":"empty","steps":a.steps,"checks":checks,"exports":{
        "final_model":reference("final_model",""),"final_scene":reference("final_scene",""),"final_sketches":reference("final_sketches",""),
        "screw_body_id":a.body_id("screw"),"nut_body_id":a.body_id("nut"),"coupon_length_mm":25,"nominal_male_mm":20,"nominal_female_mm":20.5,"pitch_mm":2.5
    }});
    let text=format!("// Paired physical fit coupon; print the supplied orientations without supports first.\n// It is an experiment, not proof that a chosen printer/profile will achieve the intended clearance.\n{}\n",serde_json::to_string_pretty(&document).unwrap());
    nbcad_script::Script::parse(&text).unwrap();
    std::fs::write("examples/scripts/d-screw-vise-fit.nbcad.jsonc",text).unwrap();
}
