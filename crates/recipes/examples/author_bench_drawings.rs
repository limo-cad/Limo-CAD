//! Add deterministic drawings to the authored bench without rebuilding its geometry.
//! Run `cargo run -p limo-cad-recipes --example author_bench_drawings` after editing its manufacturing inputs.
use serde_json::{json, Value};
use std::{collections::BTreeMap, fmt::Write, fs};

const CHAPTER: &str = "Drawing package";

fn reference(name: &str) -> Value {
    json!({"$ref":name})
}
fn at(name: &str, pointer: &str) -> Value {
    json!({"$ref":name,"pointer":pointer})
}
fn select(from: Value, path: &str, predicate: Value, pointer: &str) -> Value {
    json!({"$select":{"from":from,"path":path,"where":predicate,"take":"first","pointer":pointer}})
}
fn axis(index: usize) -> [f64; 3] {
    let mut result = [0.; 3];
    result[index] = 1.;
    result
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn wrapped(text: &str, width: usize) -> String {
    let mut output = String::new();
    for line in text.lines() {
        let mut length = 0;
        for word in line.split_whitespace() {
            if length != 0 {
                if length + word.chars().count() + 1 > width {
                    output.push('\n');
                    length = 0;
                } else {
                    output.push(' ');
                    length += 1;
                }
            }
            output.push_str(word);
            length += word.chars().count();
        }
        output.push('\n');
    }
    output.trim_end().into()
}

struct Author {
    groups: BTreeMap<String, String>,
    steps: Vec<Value>,
    drawings: Vec<Value>,
}
impl Author {
    fn new() -> Self {
        let catalog: Value =
            serde_json::from_str(include_str!("../../../interface/catalog.json")).unwrap();
        let mut groups = BTreeMap::new();
        for workspace in catalog["workspaces"].as_array().unwrap() {
            for panel in workspace["panels"].as_array().unwrap() {
                if let Some(operations) = panel["operations"].as_array() {
                    for operation in operations {
                        groups.insert(
                            operation.as_str().unwrap().into(),
                            format!(
                                "{}/{}",
                                workspace["id"].as_str().unwrap(),
                                panel["id"].as_str().unwrap()
                            ),
                        );
                    }
                }
            }
        }
        for group in catalog["groups"].as_array().unwrap() {
            for operation in group["operations"].as_array().unwrap() {
                groups.insert(
                    operation.as_str().unwrap().into(),
                    group["id"].as_str().unwrap().into(),
                );
            }
        }
        Self {
            groups,
            steps: vec![
                json!({"chapter":CHAPTER,"note":"Review candidate: current part views, datum-based machining coordinates, assembly and cut list. Physical timber and load qualification remain required."}),
            ],
            drawings: Vec::new(),
        }
    }
    fn call(&mut self, id: &str, operation: &str, arguments: Value) {
        self.steps.push(json!({"id":id,"call":{"group":self.groups[operation],"operation":operation,"arguments":arguments}}));
    }
    fn bind(&mut self, id: &str, value: Value) {
        self.steps.push(json!({"let":{id:value}}));
    }
    fn sheet(&mut self, id: &str, title: &str, number: &str, format: &str) -> String {
        self.call(id, "drawing_create_sheet", json!({"name":title,"format":format,"orientation":"landscape","title_block":{"title":title,"drawing_number":number,"revision":"A-candidate","material":"Timber selected and conditioned for outdoor use","finish":"Ease exposed edges; exterior finish after dry assembly"},"tolerance_note":{"preset":"custom","custom":"mm. Finished size +/-0.5; location +/-0.5. Review moisture movement and grain before construction."}}));
        let bound = format!("{id}_id");
        self.bind(
            &bound,
            select(reference(id), "/sheets", json!({"/name":title}), "/id"),
        );
        bound
    }
    fn view(&mut self, id: &str, sheet: &str, view: Value) -> String {
        let name = view["name"].clone();
        self.call(
            id,
            "drawing_add_view",
            json!({"sheet_id":reference(sheet),"view":view}),
        );
        let bound = format!("{id}_id");
        let sheet = select(
            reference(id),
            "/sheets",
            json!({"/id":reference(sheet)}),
            "",
        );
        self.bind(
            &bound,
            select(sheet, "/views", json!({"/name":name}), "/id"),
        );
        bound
    }
    fn export(&mut self, id: &str, sheet: &str) {
        let mut record = json!({"part":id,"sheet_id":reference(sheet)});
        for format in ["svg", "dxf"] {
            let bound = format!("bench_drawing_{id}_{format}");
            self.call(
                &bound,
                "drawing_export",
                json!({"sheet_id":reference(sheet),"format":format}),
            );
            record[format] = at(&bound, "/content");
        }
        self.drawings.push(record);
    }
    fn dimension(
        &mut self,
        id: &str,
        sheet: &str,
        view: &str,
        projection: &str,
        coordinate: usize,
        mode: &str,
    ) {
        let mut anchors = Vec::with_capacity(2);
        for maximum in [false, true] {
            let predicate = json!({format!("/point/{coordinate}"):at(projection, &format!("/bounds/{}", coordinate + if maximum {2} else {0}))});
            let bound = format!("{id}_{}", if maximum { "last" } else { "first" });
            self.bind(
                &bound,
                select(reference(projection), "/anchors", predicate, ""),
            );
            let field = |pointer| at(&bound, pointer);
            anchors.push(json!({"body_id":field("/body_id"),"edge_id":field("/edge_id"),"edge_key":field("/edge_key"),"endpoint":field("/endpoint"),"fallback_point":field("/model_point")}));
        }
        self.call(id, "drawing_add_linear_dimension", json!({"sheet_id":reference(sheet),"view_id":reference(view),"first":anchors[0],"second":anchors[1],"mode":mode,"offset":if mode == "horizontal" {-10.} else {10.},"precision":1}));
    }
    fn part(&mut self, part: &Value, bores: &[Value], source_steps: &[Value], index: usize) {
        let body = part["body_id"]["$ref"].as_str().unwrap();
        let name = body.strip_suffix("_body").unwrap();
        let stock: Vec<f64> = part["stock_mm"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let grain = part["datums"]["grain_axis"].as_u64().unwrap() as usize;
        let mut other: Vec<usize> = (0..3).filter(|axis| *axis != grain).collect();
        other.sort_by(|a, b| stock[*b].total_cmp(&stock[*a]));
        let scale = (230. / stock[grain]).min(65. / stock[other[0]]).min(4.);
        let sheet = self.sheet(
            &format!("bench_drawing_{name}_sheet"),
            part["name"].as_str().unwrap(),
            &format!("BEN-{index:02}"),
            "a3",
        );
        for (view_index, vertical) in other.iter().enumerate() {
            let direction = cross(axis(grain), axis(*vertical));
            let view_name = format!("bench_drawing_{name}_view_{view_index}");
            let label = format!(
                "{} / {}",
                ["X", "Y", "Z"][grain],
                ["X", "Y", "Z"][*vertical]
            );
            let view = self.view(&view_name, &sheet, json!({"name":label,"kind":"custom","scope":"definition","body_ids":[reference(body)],"direction":direction,"up":axis(*vertical),"position":[145.,if view_index==0 {65.} else {132.}],"scale":scale,"show_hidden_lines":true}));
            let projection = format!("bench_drawing_{name}_projection_{view_index}");
            self.call(&projection, "drawing_projection", json!({"body_ids":[reference(body)],"direction":direction,"up":axis(*vertical),"include_hidden":true}));
            if view_index == 0 {
                self.dimension(
                    &format!("bench_drawing_{name}_length"),
                    &sheet,
                    &view,
                    &projection,
                    0,
                    "horizontal",
                );
            }
            self.dimension(
                &format!("bench_drawing_{name}_size_{view_index}"),
                &sheet,
                &view,
                &projection,
                1,
                "vertical",
            );
        }
        let iso = format!("bench_drawing_{name}_iso");
        self.view(&iso, &sheet, json!({"name":"Isometric","kind":"isometric","scope":"definition","body_ids":[reference(body)],"direction":[1.,-1.,1.],"up":[0.,0.,1.],"position":[335.,75.],"scale":scale*0.35}));
        let coordinate = |datum: &str| {
            let d = &part["datums"][datum];
            format!(
                "{datum}: {}={} mm",
                ["X", "Y", "Z"][d["normal_axis"].as_u64().unwrap() as usize],
                d["coordinate_mm"]
            )
        };
        let text = format!("Quantity {}. Finished stock X/Y/Z: {} / {} / {} mm.\nGrain along {}.\n{}; {}; {}.\nCoordinates refer to definition-local datums. Inspect mirrored parts separately.", part["quantity"],stock[0],stock[1],stock[2],["X","Y","Z"][grain],coordinate("A"),coordinate("B"),coordinate("C"));
        self.call(
            &format!("bench_drawing_{name}_datums"),
            "drawing_add_note",
            json!({"sheet_id":reference(&sheet),"position":[285.,150.],"text":wrapped(&text,35)}),
        );
        let mut machining = String::from("MACHINING COORDINATES / X,Y,Z mm\n");
        for bore in bores.iter().filter(|b| b["body_id"] == part["body_id"]) {
            writeln!(
                machining,
                "D{} / depth {} along {}:",
                bore["diameter"],
                bore["depth"],
                ["X", "Y", "Z"][bore["axis"].as_u64().unwrap() as usize]
            )
            .unwrap();
            for point in bore["points"].as_array().unwrap() {
                writeln!(machining, "  {}, {}, {}", point[0], point[1], point[2]).unwrap();
            }
        }
        if machining.lines().count() == 1 {
            machining.push_str("No modeled bores.\n");
        }
        for step in source_steps.iter().filter(|step| {
            step["id"]
                .as_str()
                .is_some_and(|id| id.starts_with(&format!("{name}_")))
        }) {
            let args = &step["call"]["arguments"];
            match step["call"]["operation"].as_str() {
                Some("sketch_add_rectangle_locked") if args["anchor"] != json!({"x":0,"y":0}) => {
                    let x = args["anchor"]["x"].as_f64().unwrap().clamp(0., stock[0]);
                    let y = args["anchor"]["y"].as_f64().unwrap().clamp(0., stock[1]);
                    let end_x = args["corner_hint"]["x"]
                        .as_f64()
                        .unwrap()
                        .clamp(0., stock[0]);
                    let end_y = args["corner_hint"]["y"]
                        .as_f64()
                        .unwrap()
                        .clamp(0., stock[1]);
                    writeln!(
                        machining,
                        "Through notch: X {x}..{end_x}, Y {y}..{end_y}. Square internal corners."
                    )
                    .unwrap();
                }
                Some("sketch_add_slot") => {
                    writeln!(
                        machining,
                        "Slot centers X/Y/Z: {} to {}. Cut through thickness.",
                        args["p1"]["$project"]["point"], args["p2"]["$project"]["point"]
                    )
                    .unwrap();
                    let width = source_steps
                        .iter()
                        .find(|step| step["id"] == "picket_refine_width")
                        .unwrap()["call"]["arguments"]["text"]
                        .as_str()
                        .unwrap();
                    writeln!(
                        machining,
                        "Finished slot width {width}; semicircular ends R(width/2)."
                    )
                    .unwrap();
                }
                Some("solid_fillet") => {
                    writeln!(
                        machining,
                        "Modeled edge rounding R{}; see the exact views for selected edges.",
                        args["radius"]
                    )
                    .unwrap();
                }
                _ => {}
            }
        }
        machining.push_str("Check dry fit; maintain specified clearance. Do not infer dimensions by scaling this sheet.");
        self.call(&format!("bench_drawing_{name}_machining"), "drawing_add_note", json!({"sheet_id":reference(&sheet),"position":[20.,180.],"text":wrapped(&machining,68)}));
        self.export(name, &sheet);
    }
    fn assembly(&mut self, parts: &[Value]) {
        let sheet = self.sheet(
            "bench_drawing_assembly_sheet",
            "Bench assembly",
            "BEN-000",
            "a2",
        );
        for (name, direction, up, position, scale) in [
            ("front", [0., -1., 0.], [0., 0., 1.], [170., 120.], 0.22),
            ("side", [1., 0., 0.], [0., 0., 1.], [440., 120.], 0.22),
            ("top", [0., 0., 1.], [0., 1., 0.], [170., 305.], 0.22),
        ] {
            let id = format!("bench_drawing_assembly_{name}");
            self.view(&id, &sheet, json!({"name":name,"kind":if name=="side" {"right"} else {name},"scope":"assembly","direction":direction,"up":up,"position":position,"scale":scale}));
        }
        self.call("bench_drawing_assembly_note", "drawing_add_note", json!({"sheet_id":reference(&sheet),"position":[360.,250.],"text":"Dry assemble the connected frame before fixing slats.\nSeat post-notch side clearance: 2 mm nominal,\n1 mm minimum after machining tolerance.\nArm/rear-post clearance: 3 mm nominal,\n1.5 mm minimum. Account for timber moisture.\nFastener locations and embedment are retained\nin verification_inputs; confirm selected hardware.\nStructural load rating requires physical qualification."}));
        self.export("assembly", &sheet);
        let sheet = self.sheet(
            "bench_drawing_cut_list_sheet",
            "Bench cut list",
            "BEN-BOM",
            "a2",
        );
        let items: Vec<Value> = parts.iter().enumerate().map(|(i,p)| json!({"item_number":(i+1).to_string(),"body_id":p["body_id"],"part_number":format!("BEN-{:02}",i+1),"description":format!("{} ({} x {} x {} mm)",p["name"].as_str().unwrap(),p["stock_mm"][0],p["stock_mm"][1],p["stock_mm"][2]),"quantity":p["quantity"],"material":"Conditioned outdoor timber","finish":"Exterior finish"})).collect();
        self.call(
            "bench_drawing_cut_list",
            "drawing_set_bom",
            json!({"sheet_id":reference(&sheet),"position":[16.,25.],"items":items}),
        );
        self.call("bench_drawing_cut_list_note", "drawing_add_note", json!({"sheet_id":reference(&sheet),"position":[20.,250.],"text":"Finished sizes; allow machining and saw kerf in rough stock.\nUse part sheets for mirrored handed parts and definition-local machining coordinates.\nChoose timber, exterior finish and fasteners for the installation.\nReview moisture movement, fastener purchase and physical load qualification before release."}));
        self.export("cut_list", &sheet);
        self.call("bench_drawing_document", "drawing_document", json!({}));
        self.steps.push(json!({"assert":{"$count":at("bench_drawing_document","/sheets")},"equals":parts.len()+2}));
        self.call(
            "bench_drawing_select_assembly",
            "drawing_select_sheet",
            json!({"sheet_id":reference("bench_drawing_assembly_sheet_id")}),
        );
    }
}

fn main() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/scripts/garden-bench.limo.jsonc"
    );
    let source = fs::read_to_string(path).unwrap().replace("\r\n", "\n");
    let recipe: Value = serde_json::from_str(&source).unwrap();
    let inputs = &recipe["exports"]["verification_inputs"];
    let parts = inputs["parts"].as_array().unwrap();
    let mut author = Author::new();
    for (index, part) in parts.iter().enumerate() {
        author.part(
            part,
            inputs["bores"].as_array().unwrap(),
            recipe["steps"].as_array().unwrap(),
            index + 1,
        );
    }
    author.assembly(parts);
    let checks = source.find("  ],\n  \"checks\":").unwrap();
    let prefix_end = source[..checks]
        .find(&format!("\"chapter\": \"{CHAPTER}\""))
        .map(|marker| source[..marker].rfind("    {").unwrap().saturating_sub(2))
        .unwrap_or(checks);
    let mut output = source[..prefix_end]
        .trim_start()
        .trim_end_matches(',')
        .trim_end()
        .to_string();
    for step in &author.steps {
        output.push_str(",\n");
        output.push_str(
            &serde_json::to_string_pretty(step)
                .unwrap()
                .lines()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    let exports_start = source.find("  \"exports\": {").unwrap();
    output.push('\n');
    output.push_str(&source[checks..exports_start]);
    let mut exports = recipe["exports"].clone();
    exports["drawings"] = json!(author.drawings);
    exports["drawing_document"] = reference("bench_drawing_document");
    output.push_str("  \"exports\": ");
    let serialized = serde_json::to_string_pretty(&exports).unwrap();
    output.push_str(
        &serialized
            .lines()
            .enumerate()
            .map(|(i, line)| {
                if i == 0 {
                    line.into()
                } else {
                    format!("  {line}")
                }
            })
            .collect::<Vec<String>>()
            .join("\n"),
    );
    output.push_str("\n}\n");
    limo_cad_script::Script::parse(&output).unwrap();
    fs::write(path, output).unwrap();
}
