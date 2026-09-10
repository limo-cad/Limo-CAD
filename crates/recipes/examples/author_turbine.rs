//! Deterministic authoring of an ordinary native recipe. This does not run CAD.
//! `cargo run -p nbcad-recipes --example author_turbine` updates the reviewed JSONC.
use serde_json::{json, Value};
use std::f64::consts::PI;

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
}
impl Author {
    fn new() -> Self {
        Self {
            steps: vec![],
            parts: vec![],
            drawings: vec![],
            serial: 0,
        }
    }
    fn call(&mut self, id: &str, _group: &str, op: &str, args: Value) {
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
    // Consecutive dimensioned edges plus a dependent closing edge: no fixed polygon.
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
        if operation == "new_body" {
            self.steps.push(
                json!({"view":"current","fit":true,"target":"active_sketch","duration_ms":300}),
            );
        }
        let finish = self.uid("finish");
        self.call(&finish, "sketch/draw", "sketch_finish", json!({}));
        let id = self.uid("extrude");
        self.call(&id,"solid/build","solid_extrude",json!({"sketch_name":name,"profile_indices":[0],"operation":operation,"extent":{"type":"distance","distance":height},"taper_angle_deg":0,"flip":false,"target_body_ids":target.map(|n|vec![body_ref(n)]).unwrap_or_default()}));
        id
    }
    fn cylinder(
        &mut self,
        name: &str,
        center: [f64; 2],
        diameter: f64,
        z: f64,
        height: f64,
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
        min: [f64; 2],
        max: [f64; 2],
        z: f64,
        height: f64,
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
        y: f64,
        z: f64,
        diameter: f64,
        half_grip: f64,
        seat_diameter: f64,
    ) {
        self.cross_bore(name, target, y, z, diameter);
        // Counterbores provide real, flat head/nut seats on the round hub.
        for x in [-35., half_grip] {
            let n = self.uid("clamp_spotface");
            self.begin(&n, "yz", x);
            self.circle([y, z], seat_diameter);
            self.extrude(&n, 35. - half_grip, "cut", Some(target));
        }
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
    fn part_drawing(&mut self, name: &str, height: f64, diameters: &[f64], note: &str) {
        let title = self.parts.iter().find(|p| p["id"] == name).unwrap()["name"]
            .as_str()
            .unwrap()
            .to_string();
        let sheet = format!("{name}_sheet");
        self.call(&sheet,"drawing/sheet","drawing_create_sheet",json!({"name":title,"format":"a3","orientation":"landscape","title_block":{"title":title,"drawing_number":format!("TUR-{name}"),"revision":"A-candidate","material":"PETG unless purchased part","finish":"Deburr; inspect clamp and rotating clearances"},"tolerance_note":{"preset":"custom","custom":"Millimetres. Fits are design allowances pending coupon and specimen measurement. No qualified printer tolerance is claimed."}}));
        let sheet_id = format!("{name}_sheet_id");
        self.bind(
            &sheet_id,
            select(r(&sheet), "/sheets", json!({}), "last", "/id"),
        );
        let scale = match name {
            "stage" | "cap" | "shaft" => 0.65,
            "base" => 0.8,
            "pinion" => 4.,
            "rotor_gear" => 1.8,
            "motor_mount" => 2.4,
            "tower" => 2.,
            _ => 1.2,
        };
        let mut views = vec![];
        for (kind, direction, up, position) in [
            ("top", [0., 0., 1.], [0., 1., 0.], [110., 90.]),
            ("front", [0., -1., 0.], [0., 0., 1.], [290., 105.]),
        ] {
            let view = format!("{name}_{kind}_view");
            self.call(&view,"drawing/views","drawing_add_view",json!({"sheet_id":r(&sheet_id),"view":{"name":kind,"kind":kind,"direction":direction,"up":up,"position":position,"scale":scale,"body_ids":[body_ref(name)],"show_hidden_lines":true}}));
            let view_id = format!("{view}_id");
            self.bind(
                &view_id,
                select(
                    select(r(&view), "/sheets", json!({"/id":r(&sheet_id)}), "one", ""),
                    "/views",
                    json!({}),
                    "last",
                    "/id",
                ),
            );
            let projection = format!("{name}_{kind}_projection");
            // Match the native sheet export's paper-space curve tolerance so
            // the exact projection serves both association picks and output.
            self.call(&projection,"drawing/views","drawing_projection",json!({"body_ids":[body_ref(name)],"direction":direction,"up":up,"include_hidden":true,"deflection":(0.08_f64/scale).max(0.01)}));
            views.push((view_id, projection));
        }
        for (i, diameter) in diameters.iter().enumerate() {
            let circle = format!("{name}_dimension_circle_{i}");
            self.bind(
                &circle,
                select(
                    r(&views[0].1),
                    "/circles",
                    json!({"/radius":diameter/2.}),
                    "first",
                    "",
                ),
            );
            let id = self.uid("diameter");
            self.call(&id,"drawing/dimensions","drawing_add_radial_dimension",json!({"sheet_id":r(&sheet_id),"view_id":r(&views[0].0),"feature":{"body_id":body_ref(name),"edge_id":at(&circle,"/edge_id"),"edge_key":at(&circle,"/edge_key"),"fallback_center":at(&circle,"/center_model"),"fallback_normal":at(&circle,"/normal_model"),"fallback_radius":at(&circle,"/radius"),"closed":at(&circle,"/closed")},"mode":"diameter","leader_angle_deg":30.+i as f64*80.,"offset":12.+i as f64*3.,"precision":2}));
        }
        let make_anchor = |z: f64| {
            let source = r(&views[1].1);
            let pred = if z == 0. {
                json!({"/model_point/2":z,"/point/0":at(&views[1].1,"/bounds/2")})
            } else {
                json!({"/model_point/2":z})
            };
            json!({"body_id":body_ref(name),"edge_id":select(source.clone(),"/anchors",pred.clone(),"first","/edge_id"),"edge_key":select(source.clone(),"/anchors",pred.clone(),"first","/edge_key"),"endpoint":select(source.clone(),"/anchors",pred.clone(),"first","/endpoint"),"fallback_point":select(source,"/anchors",pred,"first","/model_point")})
        };
        let id = self.uid("height");
        self.call(&id,"drawing/dimensions","drawing_add_linear_dimension",json!({"sheet_id":r(&sheet_id),"view_id":r(&views[1].0),"first":make_anchor(0.),"second":make_anchor(height),"mode":"vertical","offset":14.,"precision":2}));
        if name == "base" {
            let corner = |x: f64, y: f64| {
                let pred = json!({"/model_point/0":x,"/model_point/1":y,"/model_point/2":0.});
                json!({"body_id":body_ref(name),"edge_id":select(r(&views[0].1),"/anchors",pred.clone(),"first","/edge_id"),"edge_key":select(r(&views[0].1),"/anchors",pred.clone(),"first","/edge_key"),"endpoint":select(r(&views[0].1),"/anchors",pred.clone(),"first","/endpoint"),"fallback_point":select(r(&views[0].1),"/anchors",pred,"first","/model_point")})
            };
            for (label, first, second, mode) in [
                ("width", corner(-70., -65.), corner(90., -65.), "horizontal"),
                ("depth", corner(90., -65.), corner(90., 65.), "vertical"),
            ] {
                self.call(&format!("base_{label}_dimension"),"drawing/dimensions","drawing_add_linear_dimension",json!({"sheet_id":r(&sheet_id),"view_id":r(&views[0].0),"first":first,"second":second,"mode":mode,"offset":14.,"precision":2}));
            }
            let centre = |x: f64, y: f64, radius: f64| {
                let pred = json!({"/center_model/0":x,"/center_model/1":y,"/radius":radius});
                json!({"body_id":body_ref(name),"edge_id":select(r(&views[0].1),"/circles",pred.clone(),"first","/edge_id"),"edge_key":select(r(&views[0].1),"/circles",pred.clone(),"first","/edge_key"),"endpoint":"start","circle_center":true,"fallback_point":select(r(&views[0].1),"/circles",pred,"first","/center_model")})
            };
            self.call("base_shaft_spacing","drawing/dimensions","drawing_add_linear_dimension",json!({"sheet_id":r(&sheet_id),"view_id":r(&views[0].0),"first":centre(0.,0.,9.),"second":centre(45.25,23.,1.7),"mode":"horizontal","offset":35.,"precision":2,"suffix":" shaft spacing"}));
        }
        let id = self.uid("drawing_note");
        self.call(
            &id,
            "drawing/annotate",
            "drawing_add_note",
            json!({"sheet_id":r(&sheet_id),"text":wrapped(note),"position":[22.,228.]}),
        );
        let mut export = json!({"part":name,"sheet_id":r(&sheet_id)});
        for format in ["svg", "dxf"] {
            let id = format!("{name}_{format}");
            self.call(
                &id,
                "drawing/output",
                "drawing_export",
                json!({"sheet_id":r(&sheet_id),"format":format}),
            );
            export[format] = at(&id, "/content");
        }
        self.drawings.push(export);
    }
    fn assembly_drawing(&mut self) {
        self.call("assembly_sheet","drawing/sheet","drawing_create_sheet",json!({"name":"Turbine assembly","format":"a3","orientation":"landscape","title_block":{"title":"Savonius experiment / assembly","drawing_number":"TUR-000","revision":"A-candidate"}}));
        self.bind(
            "assembly_sheet_id",
            select(r("assembly_sheet"), "/sheets", json!({}), "last", "/id"),
        );
        for (kind, direction, up, position, scale) in [
            ("front", [0., -1., 0.], [0., 0., 1.], [105., 132.], 0.75),
            ("top", [0., 0., 1.], [0., 1., 0.], [300., 108.], 0.65),
        ] {
            let id = self.uid("assembly_view");
            self.call(&id,"drawing/views","drawing_add_view",json!({"sheet_id":r("assembly_sheet_id"),"view":{"name":format!("Assembly {kind}"),"kind":kind,"scope":"assembly","direction":direction,"up":up,"position":position,"scale":scale,"show_hidden_lines":false}}));
            let view_id = format!("assembly_{kind}_view_id");
            self.bind(
                &view_id,
                select(
                    select(
                        r(&id),
                        "/sheets",
                        json!({"/id":r("assembly_sheet_id")}),
                        "one",
                        "",
                    ),
                    "/views",
                    json!({}),
                    "last",
                    "/id",
                ),
            );
            let projection = format!("assembly_{kind}_projection");
            self.call(
                &projection,
                "drawing/views",
                "drawing_projection",
                json!({"scope":"assembly","direction":direction,"up":up,"include_hidden":false,"deflection":(0.08_f64/scale).max(0.01)}),
            );
            if kind == "front" {
                let anchor = |part: &str, point: Value| {
                    let mut pred = point;
                    pred["/body_id"] = body_ref(part);
                    pred["/occurrence_id"] = occ_ref(part);
                    json!({"body_id":body_ref(part),"occurrence_id":occ_ref(part),"edge_id":select(r(&projection),"/anchors",pred.clone(),"first","/edge_id"),"edge_key":select(r(&projection),"/anchors",pred.clone(),"first","/edge_key"),"endpoint":select(r(&projection),"/anchors",pred.clone(),"first","/endpoint"),"fallback_point":select(r(&projection),"/anchors",pred,"first","/model_point")})
                };
                self.call("assembly_height","drawing/dimensions","drawing_add_linear_dimension",json!({"sheet_id":r("assembly_sheet_id"),"view_id":r(&view_id),"first":anchor("base",json!({"/model_point/0":90.,"/model_point/2":0.})),"second":anchor("shaft",json!({"/model_point/2":281.})),"mode":"vertical","offset":14.,"precision":2}));
            } else {
                let pred = json!({"/body_id":body_ref("cap"),"/occurrence_id":occ_ref("cap"),"/radius":99.});
                let field =
                    |path: &str| select(r(&projection), "/circles", pred.clone(), "first", path);
                self.call("assembly_diameter","drawing/dimensions","drawing_add_radial_dimension",json!({"sheet_id":r("assembly_sheet_id"),"view_id":r(&view_id),"feature":{"body_id":body_ref("cap"),"occurrence_id":occ_ref("cap"),"edge_id":field("/edge_id"),"edge_key":field("/edge_key"),"fallback_center":field("/center_model"),"fallback_normal":field("/normal_model"),"fallback_radius":field("/radius"),"closed":field("/closed")},"mode":"diameter","leader_angle_deg":35.,"offset":12.,"precision":2}));
            }
        }
        self.call("assembly_note","drawing/annotate","drawing_add_note",json!({"sheet_id":r("assembly_sheet_id"),"position":[20.,254.],"text":"Two identical stages staggered 90 degrees.\nRotor and generator shafts use separate supports.\nSee TUR-BOM and individual part sheets for assembly and fits."}));
        let mut export = json!({"part":"assembly","sheet_id":r("assembly_sheet_id")});
        for format in ["svg", "dxf"] {
            let id = format!("assembly_{format}");
            self.call(
                &id,
                "drawing/output",
                "drawing_export",
                json!({"sheet_id":r("assembly_sheet_id"),"format":format}),
            );
            export[format] = at(&id, "/content");
        }
        self.drawings.push(export);
        self.call("bom_sheet","drawing/sheet","drawing_create_sheet",json!({"name":"Turbine procurement and assembly","format":"a3","orientation":"landscape","title_block":{"title":"Turbine parts and hardware","drawing_number":"TUR-BOM","revision":"A-candidate"}}));
        self.bind(
            "bom_sheet_id",
            select(r("bom_sheet"), "/sheets", json!({}), "last", "/id"),
        );
        let mut items=self.parts.iter().enumerate().map(|(i,p)|json!({"item_number":(i+1).to_string(),"body_id":p["body_id"],"part_number":format!("TUR-{}",p["id"].as_str().unwrap()),"description":p["name"],"quantity":p.get("quantity").cloned().unwrap_or(json!(1)),"material":p["material"]})).collect::<Vec<_>>();
        for (number, description, quantity) in [
            (
                "M3-12",
                "M3 x12 socket screw /2stage,2cradle base,4guard base",
                8,
            ),
            (
                "M3-16",
                "M3 x16 socket screw /2carrier base,2carrier clamp,1rotor hub",
                5,
            ),
            ("M3-20", "M3 x20 socket screw /generator cradle clamp", 1),
            (
                "M3-8-low",
                "M3 x8 low profile screw /head height <=1.65 /guard lid",
                4,
            ),
            (
                "M3-nut",
                "M3 hex nut /5.5 AF x2.4 nominal; confirm purchased fastener",
                18,
            ),
            ("M2-8", "M2 x8 socket screw /pinion clamp", 1),
            (
                "M2-nut",
                "M2 hex nut /4 AF x1.6 nominal; confirm purchased fastener",
                1,
            ),
        ] {
            items.push(json!({"item_number":(items.len()+1).to_string(),"part_number":number,"description":description,"quantity":quantity,"material":"purchased steel"}));
        }
        self.call(
            "turbine_bom",
            "drawing/sheet",
            "drawing_set_bom",
            json!({"sheet_id":r("bom_sheet_id"),"position":[16.,22.],"items":items}),
        );
        self.call("procurement_note","drawing/annotate","drawing_add_note",json!({"sheet_id":r("bom_sheet_id"),"position":[22.,207.],"text":wrapped("Hardware is a procurement list, not hidden printable geometry. Verify actual head, nut and shaft dimensions before purchase/printing. PETG baseline. Fits, motor projection, hub slip, rotor startup, bearing clamp load and guard access require physical qualification. Ages8-12 with adult guidance; age5 only with closer hands-on adult guidance.")}));
        let mut export = json!({"part":"bom","sheet_id":r("bom_sheet_id")});
        for format in ["svg", "dxf"] {
            let id = format!("bom_{format}");
            self.call(
                &id,
                "drawing/output",
                "drawing_export",
                json!({"sheet_id":r("bom_sheet_id"),"format":format}),
            );
            export[format] = at(&id, "/content");
        }
        self.drawings.push(export);
    }
    fn motor_cradle(&mut self) {
        self.cylinder("motor_mount", [0., 0.], 37., 0., 32., "new_body", None);
        self.cylinder(
            "motor_mount_cavity",
            [0., 0.],
            32.6,
            14.,
            18.,
            "cut",
            Some("motor_mount"),
        );
        self.block(
            "motor_wire_slot",
            [-0.6, -20.],
            [0.6, -13.],
            14.,
            18.,
            "cut",
            Some("motor_mount"),
        );
        self.block(
            "motor_clamp_left",
            [-8., -23.],
            [-0.6, -16.5],
            23.,
            8.,
            "join",
            Some("motor_mount"),
        );
        self.block(
            "motor_clamp_right",
            [0.6, -23.],
            [8., -16.5],
            23.,
            8.,
            "join",
            Some("motor_mount"),
        );
        self.cross_bore("motor_cradle_clamp", "motor_mount", -19.75, 27., 3.2);
        for y in [-23., 23.] {
            let n = self.uid("cradle_ear");
            self.cylinder(&n, [0., y], 13., 0., 4., "join", Some("motor_mount"));
            let n = self.uid("cradle_mount");
            self.cylinder(&n, [0., y], 3.4, 0., 4., "cut", Some("motor_mount"));
        }
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
        self.steps.push(json!({"view":"isometric","fit":true,"component_id":at(&format!("{name}_component"),"/id"),"duration_ms":450}));
        self.parts.push(json!({"id":name,"name":title,"body_id":body_ref(name),"component_id":at(&format!("{name}_component"),"/id"),"occurrence_id":occ_ref(name),"printable":printable,"material":if printable{"PETG"}else{"purchased — drawing/specimen confirmation required"},"print_pose":{"translation":[0,0,0],"rotation":[0,0,0,1]}}));
    }
    fn face(&mut self, name: &str, z: f64, normal: f64) -> String {
        let face = self.uid("face");
        let body = select(
            r("joint_geometry"),
            "/bodies",
            json!({"/id":body_ref(name)}),
            "one",
            "",
        );
        self.bind(
            &face,
            select(
                body,
                "/faces",
                json!({"/plane/normal/2":normal,"/plane/origin/2":z}),
                "first",
                "",
            ),
        );
        face
    }
    fn joint(
        &mut self,
        id: &str,
        a: &str,
        b: &str,
        origin_a: [f64; 3],
        za: f64,
        zb: f64,
        angle: f64,
        kind: &str,
    ) {
        self.joint_offset(id, a, b, origin_a, za, zb, angle, kind, 0.);
    }
    fn joint_offset(
        &mut self,
        id: &str,
        a: &str,
        b: &str,
        origin_a: [f64; 3],
        za: f64,
        zb: f64,
        angle: f64,
        kind: &str,
        offset: f64,
    ) {
        let af = self.face(a, za, 1.);
        let bf = self.face(b, zb, -1.);
        // Both declared axes point up: the underside face remains the topology
        // anchor, while its custom connector frame preserves an upright part.
        let connector = |name: &str, face: &str, origin: [f64; 3]| json!({"body_id":body_ref(name),"face_id":at(face,"/id"),"face_key":at(face,"/key"),"kind":"planar_face","frame":{"origin":origin,"primary_axis":[0,0,1],"secondary_axis":[1,0,0]},"source_surface_frame":{"origin":at(face,"/plane/origin"),"primary_axis":at(face,"/plane/normal"),"secondary_axis":at(face,"/plane/u")}});
        // Fixed separation/stagger belong to the connector home transform;
        // rigid joints have no motion coordinates, and revolute angles do.
        let anchor_a = [origin_a[0], origin_a[1], origin_a[2] + offset];
        let rigid = kind == "rigid";
        self.call(id,"assembly/joints","assembly_create_joint",json!({"name":id.replace('_'," "),"kind":kind,"connector_a":connector(a,&af,anchor_a),"connector_b":connector(b,&bf,[0.,0.,zb]),"flipped":true,"angle_offset_deg":if rigid{0.}else{angle},"linear_offset_mm":0.,"advanced":{"connector_a_occurrence_id":occ_ref(a),"connector_b_occurrence_id":occ_ref(b),"connector_a_twist_deg":if rigid{angle}else{0.}}}));
    }
    fn repeat(&mut self, name: &str, source: &str, pose: [f64; 3]) {
        self.bind(&format!("{name}_body"), r(&format!("{source}_body")));
        self.call(&format!("{name}_occurrence"),"assembly/joints","assembly_create_occurrence",json!({"name":name.replace('_'," "),"component_id":at(&format!("{source}_component"),"/id"),"local_pose":{"translation":pose,"rotation":[0,0,0,1]}}));
        let part = self.parts.iter_mut().find(|p| p["id"] == source).unwrap();
        part["quantity"] = json!(2);
    }
    fn gear(&mut self, name: &str, teeth: usize, bore: f64, hub: f64) {
        let m = 1.;
        let pitch = m * teeth as f64 / 2.;
        let root = pitch - 1.25 * m;
        let base = pitch * (20_f64.to_radians()).cos();
        let tip = pitch + m;
        self.cylinder(name, [0., 0.], 2. * root, 0., 3., "new_body", None);
        let inv = |radius: f64| {
            let t = ((radius / base).powi(2) - 1.).max(0.).sqrt();
            t - t.atan()
        };
        let half = PI / (2. * teeth as f64) - 0.10 / (2. * pitch); // 0.10 mm tooth thinning per gear.
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
        // Check the written polyline against the continuous involute, not just
        // its sampled vertices. Distance to a set is 1-Lipschitz; half the
        // largest intervening arc length bounds what lies between test points.
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
        // Snapshot before the tooth: select only new pattern bodies by the generated names.
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
            0.,
            if teeth > 30 { 12. } else { 6. },
            "join",
            Some(name),
        );
        self.cylinder(
            &format!("{name}_bore"),
            [0., 0.],
            bore,
            0.,
            15.,
            "cut",
            Some(name),
        );
        self.block(
            &format!("{name}_clamp_split"),
            [-0.6, -hub],
            [0.6, 0.],
            3.,
            12.,
            "cut",
            Some(name),
        );
        self.clamp(
            &format!("{name}_clamp_bolt"),
            name,
            if teeth > 30 { -7.68 } else { -3.2 },
            if teeth > 30 { 8. } else { 4.5 },
            if teeth > 30 { 3.2 } else { 2.2 },
            if teeth > 30 { 4.8 } else { 2. },
            if teeth > 30 { 6.4 } else { 4.8 },
        );
    }
}

fn main() {
    let mut a = Author::new();
    let g = 54.5 * std::f64::consts::FRAC_1_SQRT_2;
    a.note("The experiment","Build a two-stage vertical-axis Savonius turbine. PETG baseline; 180 mm bucket diameter and 200 mm combined bucket height. Generator and hardware are native representative parts pending specimen fit and physical testing.");
    a.note("Repeated rotor stage","Concentric driving diameters define a bottom disc, shaft hub and two semicircular bucket walls. The clamp hub ends at 18 mm so only the 8 mm shaft divides the overlap above it. One stage definition appears twice, staggered by 90 degrees. Print each stage upright and the final cap separately.");
    let stage_plate = a.cylinder("stage", [0., 0.], 198., 0., 3., "new_body", None);
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
    a.cylinder("stage_hub", [0., 0.], 24., 0., 18., "join", Some("stage"));
    a.cylinder("bucket", [40.5, 0.], 99., 0., 100., "new_body", None);
    a.cylinder(
        "bucket_inner",
        [40.5, 0.],
        95.,
        0.,
        100.,
        "cut",
        Some("bucket"),
    );
    a.block(
        "bucket_half",
        [-15., -55.],
        [95., 0.],
        0.,
        100.,
        "cut",
        Some("bucket"),
    );
    a.call("bucket_pair","solid/pattern","solid_circular_pattern",json!({"body_ids":[body_ref("bucket")],"axis_origin":{"x":0,"y":0,"z":0},"axis_direction":{"x":0,"y":0,"z":1},"count":2,"total_angle_deg":360}));
    a.call("stage_join_buckets","solid/combine","solid_combine",json!({"target_body_id":body_ref("stage"),"tool_body_ids":select(r("bucket_pair"),"/scene/bodies",json!({"$or":[{"/id":body_ref("bucket")},{"/feature_id":select(r("bucket_pair"),"/document/features",json!({}),"last","/id")}]}),"all","/id"),"operation":"join","keep_tools":false}));
    a.cylinder(
        "stage_shaft_fit",
        [0., 0.],
        8.3,
        0.,
        100.,
        "cut",
        Some("stage"),
    );
    a.block(
        "stage_clamp_split",
        [-0.6, -14.],
        [0.6, 0.],
        0.,
        18.,
        "cut",
        Some("stage"),
    );
    a.clamp("stage_clamp_bolt", "stage", -8., 10., 3.2, 4., 6.4);
    a.component("stage", "Savonius stage / print twice", true, [0., 0., 73.]);
    a.cylinder("cap", [0., 0.], 198., 0., 3., "new_body", None);
    a.cylinder("cap_bore", [0., 0.], 8.4, 0., 3., "cut", Some("cap"));
    a.component("cap", "Rotor top endplate", true, [0., 0., 273.]);
    a.note("Separate rotor bearings","Two spaced 608 bearing seats support the 8 mm shaft. The generator carries no rotor weight. Named clearances are provisional diametral allowances; print the coupons before the full assembly.");
    a.block("base", [-70., -65.], [90., 65.], 0., 8., "new_body", None);
    a.cylinder(
        "base_shaft_clearance",
        [0., 0.],
        18.,
        0.,
        8.,
        "cut",
        Some("base"),
    );
    for (x, y) in [
        (-22., 0.),
        (22., 0.),
        (45.25, -23.),
        (45.25, 23.),
        (10. + g, g),
        (10. - g, g),
        (10. + g, -g),
        (10. - g, -g),
    ] {
        let n = a.uid("base_fastener");
        a.cylinder(&n, [x, y], 3.4, 0., 8., "cut", Some("base"));
        let n = a.uid("base_head_recess");
        a.cylinder(&n, [x, y], 6.4, 0., 3.2, "cut", Some("base"));
    }
    a.component(
        "base",
        "Base / bearing and generator datum",
        true,
        [0., 0., 0.],
    );
    a.cylinder("tower", [0., 0.], 52., 0., 6., "new_body", None);
    a.cylinder(
        "tower_column",
        [0., 0.],
        36.,
        0.,
        42.,
        "join",
        Some("tower"),
    );
    a.cylinder(
        "tower_relief",
        [0., 0.],
        17.8,
        0.,
        42.,
        "cut",
        Some("tower"),
    );
    a.cylinder(
        "tower_lower_seat",
        [0., 0.],
        22.3,
        0.,
        7.,
        "cut",
        Some("tower"),
    );
    a.cylinder(
        "tower_upper_seat",
        [0., 0.],
        22.3,
        35.,
        7.,
        "cut",
        Some("tower"),
    );
    a.block(
        "tower_split",
        [-0.6, -27.],
        [0.6, 0.],
        0.,
        42.,
        "cut",
        Some("tower"),
    );
    a.clamp("tower_clamp_lower", "tower", -14., 12., 3.2, 5., 6.4);
    a.clamp("tower_clamp_upper", "tower", -14., 32., 3.2, 5., 6.4);
    for x in [-22., 22.] {
        let n = a.uid("tower_mount");
        a.cylinder(&n, [x, 0.], 3.4, 0., 6., "cut", Some("tower"));
    }
    a.component(
        "tower",
        "Split bearing carrier / 608 seats",
        true,
        [0., 0., 8.],
    );
    a.cylinder("bearing", [0., 0.], 22., 0., 7., "new_body", None);
    a.cylinder("bearing_bore", [0., 0.], 8., 0., 7., "cut", Some("bearing"));
    a.component(
        "bearing",
        "608 bearing / purchased envelope",
        false,
        [0., 0., 8.],
    );
    a.cylinder("shaft", [0., 0.], 8., 0., 278., "new_body", None);
    a.component(
        "shaft",
        "8 mm steel shaft / cut to length",
        false,
        [0., 0., 3.],
    );
    a.cylinder("spacer", [0., 0.], 13., 0., 28., "new_body", None);
    a.cylinder("spacer_bore", [0., 0.], 8.2, 0., 28., "cut", Some("spacer"));
    a.component("spacer", "Inner race spacer", false, [0., 0., 15.]);
    a.cylinder("washer", [0., 0.], 13., 0., 0.6, "new_body", None);
    a.cylinder("washer_bore", [0., 0.], 8.2, 0., 0.6, "cut", Some("washer"));
    a.component(
        "washer",
        "Upper inner-race thrust washer",
        false,
        [0., 0., 50.],
    );
    a.cylinder("collar", [0., 0.], 16., 0., 5., "new_body", None);
    a.cylinder("collar_bore", [0., 0.], 8., 0., 5., "cut", Some("collar"));
    a.component(
        "collar",
        "Lower shaft collar / purchased",
        false,
        [0., 0., 3.],
    );
    a.note("Generator cartridge","The KW-GEN3 case is nominally 32 mm diameter. Its 28 mm case and 6 mm projecting round shaft are provisional, derived from the approximately 34 mm total envelope; measure the specimen before printing the cartridge and pinion.");
    a.motor_cradle();
    a.component(
        "motor_mount",
        "Removable KW-GEN3 cradle / specimen fit pending",
        true,
        [45.25, 0., 8.],
    );
    a.cylinder("motor", [0., 0.], 32., 0., 28., "new_body", None);
    a.component(
        "motor",
        "KW-GEN3 stator case / purchased envelope",
        false,
        [45.25, 0., 22.],
    );
    a.cylinder("motor_shaft", [0., 0.], 2., 0., 6., "new_body", None);
    a.component(
        "motor_shaft",
        "KW-GEN3 rotating shaft / provisional 6 mm projection",
        false,
        [45.25, 0., 50.],
    );
    a.note("Guard the transmission","A separately printed guard and lid cover the gear mesh. The rotor endplate overlaps the central opening. This is a supervised low-energy science model, not a child-safety-qualified product.");
    a.cylinder("guard", [0., 0.], 120., 0., 57., "new_body", None);
    a.cylinder(
        "guard_inside",
        [0., 0.],
        114.,
        0.,
        57.,
        "cut",
        Some("guard"),
    );
    for (x, y) in [(g, g), (-g, g), (g, -g), (-g, -g)] {
        let n = a.uid("guard_boss");
        a.cylinder(&n, [x, y], 8., 0., 57., "join", Some("guard"));
        let n = a.uid("guard_hole");
        a.cylinder(&n, [x, y], 3.4, 0., 57., "cut", Some("guard"));
        for z in [0., 54.] {
            let n = a.uid("guard_captive_nut");
            a.nut_pocket(&n, "guard", [x, y], z);
        }
    }
    a.component(
        "guard",
        "Transmission guard / four screw bosses",
        true,
        [10., 0., 8.],
    );
    a.cylinder("guard_lid", [0., 0.], 120., 0., 3., "new_body", None);
    a.cylinder(
        "guard_lid_axis",
        [-10., 0.],
        30.,
        0.,
        3.,
        "cut",
        Some("guard_lid"),
    );
    for (x, y) in [(g, g), (-g, g), (g, -g), (-g, -g)] {
        let n = a.uid("lid_hole");
        a.cylinder(&n, [x, y], 3.4, 0., 3., "cut", Some("guard_lid"));
    }
    a.component(
        "guard_lid",
        "Removable transmission lid",
        true,
        [10., 0., 65.],
    );
    a.note("A real 4:1 spur pair","The tooth flanks sample the involute of the 20-degree base circle with bounded chord error. A native circular pattern repeats one driving tooth and fuses it to the root disc. Module 1, 72/18 teeth, 0.10 mm tooth thinning each and 45.25 mm centre distance are explicit design inputs.");
    a.gear("rotor_gear", 72, 8.3, 24.);
    a.component(
        "rotor_gear",
        "72 tooth rotor gear / M3 split hub",
        true,
        [0., 0., 50.6],
    );
    a.gear("pinion", 18, 2.2, 12.);
    a.component(
        "pinion",
        "18 tooth generator pinion / M2 split hub",
        true,
        [45.25, 0., 50.6],
    );
    a.note("Native assembly relationships","Ground the base, place the carrier and generator cartridge, then constrain the rotor and generator with revolute joints. The second stage is a repeated occurrence of the original definition.");
    a.call("joint_geometry", "solid/check", "solid_scene", json!({}));
    a.call(
        "ground_base",
        "assembly/joints",
        "assembly_set_occurrence_grounded",
        json!({"occurrence_id":occ_ref("base"),"grounded":true}),
    );
    a.joint(
        "carrier_to_base",
        "base",
        "tower",
        [0., 0., 8.],
        8.,
        0.,
        0.,
        "rigid",
    );
    a.joint(
        "cradle_to_base",
        "base",
        "motor_mount",
        [45.25, 0., 8.],
        8.,
        0.,
        0.,
        "rigid",
    );
    a.joint(
        "guard_to_base",
        "base",
        "guard",
        [10., 0., 8.],
        8.,
        0.,
        0.,
        "rigid",
    );
    a.joint(
        "lid_to_guard",
        "guard",
        "guard_lid",
        [0., 0., 57.],
        57.,
        0.,
        0.,
        "rigid",
    );
    a.joint(
        "motor_to_cradle",
        "motor_mount",
        "motor",
        [0., 0., 14.],
        14.,
        0.,
        0.,
        "rigid",
    );
    a.joint(
        "generator_rotation",
        "motor",
        "motor_shaft",
        [0., 0., 28.],
        28.,
        0.,
        10.,
        "revolute",
    );
    a.joint_offset(
        "pinion_to_shaft",
        "motor_shaft",
        "pinion",
        [0., 0., 6.],
        6.,
        0.,
        0.,
        "rigid",
        -5.4,
    );
    a.joint_offset(
        "rotor_rotation",
        "tower",
        "rotor_gear",
        [0., 0., 42.],
        42.,
        0.,
        0.,
        "revolute",
        0.6,
    );
    a.joint_offset(
        "shaft_to_rotor_gear",
        "rotor_gear",
        "shaft",
        [0., 0., 12.],
        12.,
        0.,
        0.,
        "rigid",
        -59.6,
    );
    a.joint_offset(
        "lower_stage_to_shaft",
        "shaft",
        "stage",
        [0., 0., 278.],
        278.,
        0.,
        0.,
        "rigid",
        -208.,
    );
    a.repeat("stage_upper", "stage", [0., 0., 173.]);
    a.joint(
        "staggered_second_stage",
        "stage",
        "stage_upper",
        [0., 0., 100.],
        100.,
        0.,
        90.,
        "rigid",
    );
    a.joint(
        "rotor_cap",
        "stage_upper",
        "cap",
        [0., 0., 100.],
        100.,
        0.,
        0.,
        "rigid",
    );
    a.joint(
        "lower_bearing_seat",
        "base",
        "bearing",
        [0., 0., 8.],
        8.,
        0.,
        0.,
        "rigid",
    );
    a.repeat("bearing_upper", "bearing", [0., 0., 43.]);
    a.joint(
        "upper_bearing_seat",
        "tower",
        "bearing_upper",
        [0., 0., 35.],
        35.,
        0.,
        0.,
        "rigid",
    );
    a.joint_offset(
        "inner_race_spacer",
        "shaft",
        "spacer",
        [0., 0., 278.],
        278.,
        0.,
        0.,
        "rigid",
        -266.,
    );
    a.joint_offset(
        "thrust_washer",
        "shaft",
        "washer",
        [0., 0., 278.],
        278.,
        0.,
        0.,
        "rigid",
        -231.,
    );
    a.joint_offset(
        "shaft_retention",
        "shaft",
        "collar",
        [0., 0., 278.],
        278.,
        0.,
        0.,
        "rigid",
        -278.,
    );
    a.repeat("collar_upper", "collar", [0., 0., 276.]);
    a.joint_offset(
        "top_cap_retention",
        "shaft",
        "collar_upper",
        [0., 0., 278.],
        278.,
        0.,
        0.,
        "rigid",
        -5.,
    );
    a.call("gear_coupling","assembly/joints","assembly_create_gear_relation",json!({"name":"Printed 72:18 spur pair","joint_a":at("rotor_rotation","/id"),"joint_b":at("generator_rotation","/id"),"teeth_a":72,"teeth_b":18,"reverse":true,"phase_deg":10}));
    a.note("Read the manufacturing intent","Each native part carries its own editable drawing with actual projected edges, diameter and height dimensions. Fits are provisional. Ages 8–12 with adult guidance; age 5 requires closer hands-on adult guidance. Keep fingers away from the rotor and use only supervised low-energy airflow.");
    for (name,height,diameters,note) in [
        ("stage",100.,vec![198.,8.3],"PRINT 2 / PETG / flat disc on bed. Bucket sweep 180 mm; walls 2 mm; overlap 18 mm. The 18 mm hub leaves airflow around the 8 mm shaft above. M3 clamp center at 10 mm; hub and walls join the 3 mm disc. Print the fit coupon first. Stagger the second stage 90 degrees."),
        ("cap",3.,vec![198.,8.4],"PRINT 1 / flat on bed. Retain between the upper stage and purchased upper 8 mm shaft collar; no adhesive."),
        ("base",8.,vec![18.,3.4],"PRINT 1 / bottom on bed. 160 x 130 mm. M3 clearance 3.4 mm; underside head recesses 6.4 x 3.2 mm. Carrier centers +/-22; cradle (45.25, +/-23); guard bolt circle 109, center (10,0), holes staggered 45 degrees. Deburr recesses; keep heads below the base."),
        ("tower",42.,vec![52.,22.3,17.8],"PRINT 1 / flange down. Two 608 seats, 22.3 x 7 mm, at heights 0 and 35 mm; relief 17.8 mm between. Insert the lower bearing from below before fastening the base. Two M3 transverse split clamps. Clamp lightly; verify bearing rotation after fastening."),
        ("motor_mount",32.,vec![37.,32.6],"PRINT 1 / flange down. Cavity begins 14 mm above the base. Nominal 32 mm motor: measure the specimen, including shaft projection, before printing. Two M3 flange bolts and one transverse clamp. Route wires through the split before closing the guard."),
        ("guard",57.,vec![120.,114.,3.4],"PRINT 1 / upright. Four M3 clearance bores and eight hex nut traps: 5.8 mm across flats, 3 mm deep, open at bottom/top. Capture nuts before mounting. Bottom M3 x 12 socket screws; lid M3 x 8 low-profile heads no higher than 1.65 mm above the lid."),
        ("guard_lid",3.,vec![120.,30.,3.4],"PRINT 1 / flat. Axis opening offset (-10,0). Four M3 x 8 low-profile screws. Rotor disc is 5 mm above the lid, leaving 3.35 mm above heads no higher than 1.65 mm. Check that the rotor clears every screw before motion."),
        ("rotor_gear",12.,vec![8.3,24.],"PRINT 1 / teeth flat. Module 1; 72 teeth; 20 degree pressure angle; pitch diameter 72 mm; face 3 mm. Tooth thinning 0.10 mm. Split hub with M3 screw/nut; center distance 45.25 mm. Do not glue to the shaft."),
        ("pinion",6.,vec![2.2,12.],"PRINT 1 / teeth flat. Module 1; 18 teeth; 20 degree pressure angle; pitch diameter 18 mm; nominal face 3 mm. Tooth thinning 0.10 mm. M2 split clamp at 4.5 mm. Round 2 mm motor shaft: verify specimen projection and fit before printing."),
        ("shaft",278.,vec![8.],"PURCHASE / 8 mm straight steel shaft cut 278 mm long; deburr both ends. Separate 608 bearings carry rotor loads. This is a representative purchased envelope; straightness and surface finish must suit the actual bearings."),
    ] { a.part_drawing(name,height,&diameters,note); }
    a.assembly_drawing();
    a.steps
        .push(json!({"view":"isometric","fit":true,"duration_ms":600}));
    // Final assembly offsets and exact native connectors are validated by the MCP regression.
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
    let source = json!({"$schema":"./nbcad-script.schema.json","version":1,"name":"Vertical-axis turbine / two-stage Savonius","starting_state":"empty","steps":a.steps,"checks":checks,"exports":{"final_scene":r("final_scene"),"final_sketches":r("final_sketches"),"final_model":r("final_model"),"final_solution":r("final_solution"),"final_assembly":r("final_assembly"),"parts":a.parts,"drawings":a.drawings,"stage_plate_feature":r("stage_plate_feature"),"rotor_joint_id":at("rotor_rotation","/id"),"generator_joint_id":at("generator_rotation","/id"),"design":{"diameter_mm":180,"bucket_height_mm":200,"stage_height_mm":100,"stage_angle_deg":90,"gear_module_mm":1,"rotor_teeth":72,"pinion_teeth":18,"pressure_angle_deg":20,"centre_distance_mm":45.25,"material":"PETG","individual_print_envelope_mm":[200,200,200],"motor_specimen_pending":true}}});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/scripts/vertical-axis-turbine.nbcad.jsonc");
    std::fs::write(path,format!("// Generated by the Rust author_turbine example; replay uses the one native interpreter.\n// Millimetres/degrees. Native editable construction; physical qualification remains pending.\n{}\n",serde_json::to_string_pretty(&source).unwrap())).unwrap();
    author_fit_coupons();
}

fn author_fit_coupons() {
    let mut a = Author::new();
    a.note("Qualify the fits first", "Print these four separate PETG specimens in their native Z-up orientation. Use the actual measured shaft, bearing and generator. Record filament, printer profile, measured bore and clamp slip before building both rotor stages. These are fit specimens, not strength or safety certification.");
    a.cylinder("shaft_coupon", [0., 0.], 36., 0., 3., "new_body", None);
    a.cylinder(
        "shaft_coupon_hub",
        [0., 0.],
        24.,
        0.,
        18.,
        "join",
        Some("shaft_coupon"),
    );
    a.cylinder(
        "shaft_coupon_fit",
        [0., 0.],
        8.3,
        0.,
        18.,
        "cut",
        Some("shaft_coupon"),
    );
    a.block(
        "shaft_coupon_split",
        [-0.6, -14.],
        [0.6, 0.],
        0.,
        18.,
        "cut",
        Some("shaft_coupon"),
    );
    a.clamp("shaft_coupon_clamp", "shaft_coupon", -8., 10., 3.2, 4., 6.4);
    a.component(
        "shaft_coupon",
        "8 mm shaft / 8.3 bore / +0.3 diametral",
        true,
        [0., 0., 0.],
    );
    a.note("Bearing insertion and clamping", "The lower 608 seat opens at the bed side. Remove first-layer flare before measurement. Insert the bearing from below, then tighten the transverse M3 clamp lightly and check that the bearing still rotates freely.");
    a.cylinder("bearing_coupon", [0., 0.], 52., 0., 6., "new_body", None);
    a.cylinder(
        "bearing_coupon_column",
        [0., 0.],
        36.,
        0.,
        18.,
        "join",
        Some("bearing_coupon"),
    );
    a.cylinder(
        "bearing_coupon_relief",
        [0., 0.],
        17.8,
        0.,
        18.,
        "cut",
        Some("bearing_coupon"),
    );
    a.cylinder(
        "bearing_coupon_fit",
        [0., 0.],
        22.3,
        0.,
        7.,
        "cut",
        Some("bearing_coupon"),
    );
    a.block(
        "bearing_coupon_split",
        [-0.6, -27.],
        [0.6, 0.],
        0.,
        18.,
        "cut",
        Some("bearing_coupon"),
    );
    a.clamp(
        "bearing_coupon_clamp",
        "bearing_coupon",
        -14.,
        12.,
        3.2,
        5.,
        6.4,
    );
    a.component(
        "bearing_coupon",
        "608 bearing / 22.3 seat / +0.3 diametral",
        true,
        [60., 0., 0.],
    );
    a.note("Measure the generator specimen", "The case and projecting shaft need separate checks. The complete small cradle reuses the exact turbine construction, including its 32.6 mm cavity, flat M3 clamp ears and wire slot. Nominal case 32 mm and shaft 2 mm must be checked against the delivered motor.");
    a.motor_cradle();
    a.component(
        "motor_mount",
        "32 mm motor case / 32.6 cavity / +0.6 diametral",
        true,
        [0., 70., 0.],
    );
    // The actual small pinion is the coupon: its short shaft engagement and
    // reduced tooth face under the M2 seats must not be disguised by a tall ring.
    a.gear("pinion", 18, 2.2, 12.);
    a.component(
        "pinion",
        "2 mm motor shaft / 2.2 bore / +0.2 diametral",
        true,
        [55., 70., 0.],
    );
    for (name,height,diameters,note) in [
        ("shaft_coupon",18.,vec![8.3,24.],"FIT S / 8 mm shaft; bore 8.3 mm gives 0.3 mm diametral allowance. Actual 18 mm stage hub, M3 clamp at 10 mm, 8 mm grip and 6.4 mm flat seats. Reduced 36 mm disc saves filament; it does not reproduce full rotor stiffness. Print disc down."),
        ("bearing_coupon",18.,vec![22.3,17.8],"FIT B / 22 mm 608 bearing; seat 22.3 x 7 mm gives 0.3 mm diametral allowance. Actual lower carrier section, M3 clamp at 12 mm and 10 mm grip. Print flange down. Insert from underside; remove first-layer flare and check rotation after light clamping."),
        ("motor_mount",32.,vec![32.6,37.],"FIT M / nominal 32 mm KW-GEN3 case; cavity 32.6 mm gives 0.6 mm diametral allowance. Exact complete small cradle. Print flange down. Measure the actual case, terminal locations and shaft projection before tightening the M3 clamp."),
        ("pinion",6.,vec![2.2,12.],"FIT P / nominal 2 mm motor shaft; bore 2.2 mm gives 0.2 mm diametral allowance. Exact 18T pinion and M2 clamp, 4 mm grip. Print teeth down. Provisional 6 mm projection; seats leave 2.1 mm minimum tooth face locally. Check engagement, free rotation and slip under measured load."),
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
    checks.push(json!({"assert":{"$count":select(r("final_sketches"),"",json!({"/dof/value":0}),"all","")},"equals":{"$count":r("final_sketches")}}));
    let source = json!({"$schema":"./nbcad-script.schema.json","version":1,"name":"Turbine fit coupons / measure before printing the rotor","starting_state":"empty","steps":a.steps,"checks":checks,"exports":{"parts":a.parts,"drawings":a.drawings,"final_scene":r("final_scene"),"final_sketches":r("final_sketches"),"final_model":r("final_model"),"final_solution":r("final_solution")}});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/scripts/turbine-fit-coupons.nbcad.jsonc");
    std::fs::write(path,format!("// Generated by the same Rust author_turbine example; one native replay interpreter.\n// Actual fit specimens in their intended print orientations. All sizes are millimetres.\n{}\n",serde_json::to_string_pretty(&source).unwrap())).unwrap();
}
