//! Persistent drawing commands for the vise recipe. Geometry is measured by
//! native projection; these helpers only select topology and arrange the paper.
use super::{reference, Author, D};
use serde_json::{json, Value};

struct View {
    id: Value,
    projection: String,
    projection_request: Value,
}

fn first(from: Value, path: &str, criteria: Value, pointer: &str) -> Value {
    json!({"$select":{"from":from,"path":path,"where":criteria,"take":"first","pointer":pointer}})
}

fn sheet(a: &mut Author, part: &str, title: &str, assembly: bool) -> Value {
    let name = format!("vise_{part}_sheet");
    a.call(&name, "drawing/sheet", "drawing_create_sheet", json!({
        "name":title,"format":if assembly {"a1"} else {"a3"},"orientation":"landscape","projection_method":"third_angle",
        "title_block":{"title":title,"drawing_number":format!("VISE-{part}"),
            "revision":"B-development","material":if assembly {"FDM + purchased M5/M6 hardware"} else {"PETG, provisional"},
            "finish":"Deburr fits; qualify printed coupon before loading"},
        "tolerance_note":{"preset":"custom","custom":"Millimetres. Dimensions are model nominal values. Process clearances are design allowances, not a qualified printer tolerance or load rating."}
    }));
    let id = format!("{name}_id");
    a.bind(
        &id,
        first(
            reference(&name, ""),
            "/sheets",
            json!({"/name":title}),
            "/id",
        ),
    );
    reference(&id, "")
}

#[allow(clippy::too_many_arguments)]
fn view(
    a: &mut Author,
    sheet: &Value,
    key: &str,
    title: &str,
    body_ids: Value,
    direction: [f64; 3],
    up: [f64; 3],
    position: [f64; 2],
    scale: f64,
    assembly: bool,
    derivation: Option<Value>,
) -> View {
    let show_hidden = !assembly && !key.ends_with("_iso") && key != "nut_top" && key != "nut_front";
    let mut definition = json!({"name":title,"kind":"custom","direction":direction,
        "up":up,"position":position,"scale":scale,"body_ids":body_ids,
        "scope":if assembly {"assembly"} else {"definition"},
        "show_hidden_lines":show_hidden,"show_tangent_edges":false});
    if let Some(derivation) = derivation {
        definition["kind"] = json!("section");
        definition["derivation"] = derivation;
    }
    let call = format!("vise_{key}_view");
    a.call(
        &call,
        "drawing/views",
        "drawing_add_view",
        json!({"sheet_id":sheet,"view":definition}),
    );
    let id = format!("{call}_id");
    let target = first(reference(&call, ""), "/sheets", json!({"/id":sheet}), "");
    a.bind(&id, first(target, "/views", json!({"/name":title}), "/id"));
    View {
        id: reference(&id, ""),
        projection: format!("vise_{key}_projection"),
        projection_request: json!({
            "body_ids":body_ids,"scope":if assembly {"assembly"} else {"definition"},
            "direction":direction,"up":up,"include_hidden":show_hidden,


            "deflection":(0.08 / scale).max(0.01)
        }),
    }
}

fn project_for_dimensions(a: &mut Author, view: &View) {
    a.call(
        &view.projection,
        "drawing/views",
        "drawing_projection",
        view.projection_request.clone(),
    );
}

fn anchor(view: &View, body: Value, criteria: Value, occurrence: Option<Value>) -> Value {
    let mut criteria = criteria;
    criteria["/body_id"] = body.clone();
    if let Some(occurrence) = &occurrence {
        criteria["/occurrence_id"] = occurrence.clone();
    }
    let selected = first(reference(&view.projection, ""), "/anchors", criteria, "");
    let field = |pointer| first(json!([selected.clone()]), "", json!({}), pointer);
    let mut result = json!({"body_id":body,"edge_id":field("/edge_id"),"edge_key":field("/edge_key"),
        "endpoint":field("/endpoint"),"fallback_point":field("/model_point")});
    if let Some(occurrence) = occurrence {
        result["occurrence_id"] = occurrence;
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn linear(
    a: &mut Author,
    sheet: &Value,
    view: &View,
    key: &str,
    part: &str,
    axis: usize,
    low: f64,
    high: f64,
    mode: &str,
    offset: f64,
) {
    let point = |value| {
        let mut criteria = json!({});
        criteria[format!("/model_point/{axis}")] = json!(value);
        anchor(view, a.body_id(part), criteria, None)
    };
    a.call(
        &format!("vise_{key}_dimension"),
        "drawing/dimensions",
        "drawing_add_linear_dimension",
        json!({
            "sheet_id":sheet,"view_id":view.id,"first":point(low),"second":point(high),
            "mode":mode,"offset":offset,"precision":2
        }),
    );
}

fn radial(
    a: &mut Author,
    sheet: &Value,
    view: &View,
    key: &str,
    part: &str,
    (radius, angle): (f64, f64),
    mode: &str,
) {
    let offset = if key == "frame_cartridge_cross_hole" {
        22.
    } else {
        14.
    };
    let selected = first(
        reference(&view.projection, ""),
        "/circles",
        json!({"/body_id":a.body_id(part),"/radius":radius}),
        "",
    );
    let field = |pointer| first(json!([selected.clone()]), "", json!({}), pointer);
    a.call(
        &format!("vise_{key}_dimension"),
        "drawing/dimensions",
        "drawing_add_radial_dimension",
        json!({
            "sheet_id":sheet,"view_id":view.id,"feature":{"body_id":a.body_id(part),
                "edge_id":field("/edge_id"),"edge_key":field("/edge_key"),
                "fallback_center":field("/center_model"),"fallback_normal":field("/normal_model"),
                "fallback_radius":field("/radius"),"closed":field("/closed")},
            "mode":mode,"leader_angle_deg":angle,"offset":offset,"precision":2
        }),
    );
}

fn notes(a: &mut Author, sheet: &Value, key: &str, y: f64, lines: &[&str]) {
    for (i, text) in lines.iter().enumerate() {
        a.call(
            &format!("vise_{key}_note_{i}"),
            "drawing/annotate",
            "drawing_add_note",
            json!({"sheet_id":sheet,"text":text,"position":[20.,y+i as f64*6.]}),
        );
    }
}

fn export(a: &mut Author, sheet: &Value, key: &str, exports: &mut Vec<String>) {
    for format in ["svg", "dxf"] {
        let id = format!("vise_{key}_{format}");
        a.call(
            &id,
            "drawing/output",
            "drawing_export",
            json!({"sheet_id":sheet,"format":format}),
        );
        exports.push(id);
    }
}

pub fn add(a: &mut Author, parts: &[Value]) -> Vec<String> {
    a.note("Drawings that explain the revised mechanism", "The assembly and six printable-part sheets retain native projection references. Review the installed hardware, actual rear-entry assembly sequence, full-round detachable head and separate print orientations. Dimensions state nominal geometry, not a qualified tolerance or load rating.");
    let mut exports = Vec::new();
    let assembly = sheet(a, "assembly", "100 mm captured-slide vise / assembly", true);
    let all: Vec<_> = a.bodies.keys().map(|p| a.body_id(p)).collect();
    a.call(
        "vise_drawing_scene",
        "solid/check",
        "solid_scene",
        json!({}),
    );
    let top = view(
        a,
        &assembly,
        "assembly_top",
        "Top / mounting outside carriage",
        json!(all),
        [0., 0., 1.],
        [0., 1., 0.],
        [230., 135.],
        1.,
        true,
        None,
    );
    view(
        a,
        &assembly,
        "assembly_iso",
        "Assembly / screw, captured slide and bridge",
        json!(all),
        [1., -1., 1.],
        [0., 0., 1.],
        [515., 360.],
        0.7,
        true,
        None,
    );
    let axis_anchor = |x| {
        let body = first(
            reference("vise_drawing_scene", ""),
            "/bodies",
            json!({"/id":a.body_id("thrust")}),
            "",
        );
        let edge = first(
            body,
            "/edges",
            json!({"/circle/center/x":x,"/circle/center/y":0.,"/circle/center/z":D.axis}),
            "",
        );
        let field = |pointer| first(json!([edge.clone()]), "", json!({}), pointer);
        json!({"body_id":a.body_id("thrust"),"occurrence_id":reference("thrust_occurrence",""),"edge_id":field("/id"),"edge_key":field("/key"),"endpoint":"start","circle_center":true,"fallback_point":[field("/circle/center/x"),field("/circle/center/y"),field("/circle/center/z")]})
    };
    let section = json!({"type":"section","parent_view_id":top.id,"first":axis_anchor(D.head_front()-10.),"second":axis_anchor(D.head_front()),"label":"A-A","hatch_angle_deg":45.,"hatch_spacing_mm":2.});
    view(
        a,
        &assembly,
        "assembly_section",
        "A-A / actual axial load and retention path",
        json!(all),
        [0., -1., 0.],
        [0., 0., 1.],
        [230., 325.],
        1.,
        true,
        Some(section),
    );
    let mut items = Vec::new();
    for (i, part) in a.bodies.keys().enumerate() {
        let printed = ["frame", "jaw", "nut", "screw", "thrust", "keeper"].contains(&part.as_str());
        let description = &parts.iter().find(|entry| entry["id"] == *part).unwrap()["name"];
        items.push(json!({"item_number":(i+1).to_string(),"body_id":a.body_id(part),"part_number":part,"description":description,"quantity":1.,"material":if printed{"FDM / qualify print"}else{"Purchased"},"finish":if printed{"Individual print plate"}else if part.starts_with("mount_"){"Optional tabletop mount"}else{"Verify supplier envelope"}}));
    }
    a.call(
        "vise_assembly_bom",
        "drawing/sheet",
        "drawing_set_bom",
        json!({"sheet_id":assembly,"position":[600.,30.],"items":items}),
    );
    notes(a,&assembly,"assembly",480.,&[
        "100 mm gripping jaw; 90 mm opening. 300 N is a DESIGN LOAD CASE, not a rated working load.",
        "1: Load bridge nuts below the deck. With the bridge absent, slide the jaw onto the open rear rail ends.",
        "2: Park jaw +85 mm. Seat the keyed bridge; install its two M6 x 35 screws from above.",
        "3: Turn the bare shaft through the bridge, then load its M5 nut into the exposed stub.",
        "4: Slide the sleeve ledge under the shaft nut and fit the axial M5 x 25. Return the jaw over the head.",
        "5: Drop the keeper over the sleeve and fit its recessed M5 x 90 cross-pin and trapped nut.",
        "Mount: optional M6 bolts and broad washers at outboard lands; modeled for an 18 mm board. Edge clamps are an alternative.",
        "Qualification: toolpaths/supports, sliding effort, thread torque, load/deflection, creep and repeated-use wear remain physical work."
    ]);
    export(a, &assembly, "assembly", &mut exports);
    for (part, title) in [
        ("frame", "Frame / stiff deck, root blend and captured rails"),
        (
            "jaw",
            "Moving jaw / 100 mm grip and 72 mm captured carriage",
        ),
        ("nut", "Replaceable threaded bridge / keyed feet"),
        ("screw", "One-piece shallow-D shaft and rounded grip"),
        ("thrust", "Complete removable thrust and retention fitting"),
        (
            "keeper",
            "Captured-jaw keeper / accessible transverse fastener",
        ),
    ] {
        let sheet = sheet(a, part, title, false);
        let body = json!([a.body_id(part)]);

        let (top_position, top_scale, end_position, end_scale, iso_position, iso_scale) = match part
        {
            "frame" => ([103., 94.], 0.65, [292., 75.], 0.65, [290., 172.], 0.42),
            "jaw" => ([95., 100.], 1., [295., 75.], 1., [295., 176.], 0.55),
            "nut" => ([90., 110.], 1., [285., 82.], 1.1, [290., 175.], 0.65),
            "screw" => ([160., 72.], 1.1, [345., 75.], 1.6, [228., 167.], 0.7),
            _ => ([150., 110.], 1.4, [325., 105.], 1.4, [250., 180.], 0.7),
        };
        let top = view(
            a,
            &sheet,
            &format!("{part}_top"),
            "Top",
            body.clone(),
            [0., 0., 1.],
            [0., 1., 0.],
            top_position,
            top_scale,
            false,
            None,
        );
        let end = view(
            a,
            &sheet,
            &format!("{part}_end"),
            if part == "jaw" {
                "YZ"
            } else {
                "End / grip and bearing profiles"
            },
            body.clone(),
            [1., 0., 0.],
            [0., 0., 1.],
            end_position,
            end_scale,
            false,
            None,
        );
        view(
            a,
            &sheet,
            &format!("{part}_iso"),
            "Native isometric",
            body,
            [1., -1., 1.],
            [0., 0., 1.],
            iso_position,
            iso_scale,
            false,
            None,
        );

        if matches!(part, "frame" | "jaw" | "nut") {
            project_for_dimensions(a, &top);
        }
        if matches!(part, "jaw" | "thrust") {
            project_for_dimensions(a, &end);
        }
        match part {
            "frame" => {
                linear(
                    a,
                    &sheet,
                    &top,
                    "frame_length",
                    part,
                    0,
                    0.,
                    D.frame_length,
                    "horizontal",
                    34.,
                );
                linear(
                    a,
                    &sheet,
                    &top,
                    "frame_width",
                    part,
                    1,
                    -D.frame_width / 2.,
                    D.frame_width / 2.,
                    "vertical",
                    -16.,
                );
                notes(a,&sheet,part,222.,&["PRINT: deck down. Keep the support datum and captured flanks clean; qualify mounting-slot and nut-pocket short bridges.","14 mm deck; 25 mm fixed jaw and R8 root reinforcement. Jaw front relief clears the root at full closure.","Bridge keys carry axial load in recessed sockets. M6 screws retain the bridge; they are not the sole axial shear path.","Mounting hardware stays outboard of the 110 mm carriage. Keep clamp jaws within the clear side lands."]);
            }
            "jaw" => {
                let mouth = |y| {
                    anchor(
                        &end,
                        a.body_id(part),
                        json!({"/model_point/0":D.rear(),"/model_point/1":y,"/model_point/2":D.deck}),
                        None,
                    )
                };
                let first = mouth(-D.guide_center - D.guide_base / 2. - D.guide_clearance);
                let second = mouth(-D.guide_center + D.guide_base / 2. + D.guide_clearance);
                a.call("vise_jaw_guide_mouth_dimension", "drawing/dimensions", "drawing_add_linear_dimension", json!({"sheet_id":sheet,"view_id":end.id,"first":first,"second":second,"mode":"horizontal","offset":16.,"precision":2}));

                let grip_corner = |y| {
                    anchor(
                        &end,
                        a.body_id(part),
                        json!({"/model_point/0":D.home(),"/model_point/1":y,"/model_point/2":D.deck+20.}),
                        None,
                    )
                };
                a.call(
                    "vise_jaw_grip_width_dimension",
                    "drawing/dimensions",
                    "drawing_add_linear_dimension",
                    json!({
                        "sheet_id":sheet,"view_id":end.id,
                        "first":grip_corner(-D.jaw_width/2.),"second":grip_corner(D.jaw_width/2.),
                        "mode":"horizontal","offset":-60.,"precision":2
                    }),
                );
                linear(
                    a,
                    &sheet,
                    &top,
                    "jaw_carriage_length",
                    part,
                    0,
                    D.rear(),
                    D.home(),
                    "horizontal",
                    12.,
                );
                notes(a,&sheet,part,222.,&["PRINT: gripping face down; captured channels run vertically. The open rear keeper relief grows at 45 degrees.","Rear-entry captured carriage, 72 mm engagement. Nominal rail side and roof gaps are 0.4 mm.","Diagonal rear shoulders seat the keeper after 0.4 mm axial movement. The slotted cross-pin retains vertical position.","Measure sideways play, lift, binding and loaded face parallelism; ideal assembly joints do not establish these."]);
            }
            "nut" => {
                linear(
                    a,
                    &sheet,
                    &top,
                    "bridge_thread_engagement",
                    part,
                    0,
                    D.bridge_start,
                    D.bridge_end,
                    "horizontal",
                    80.,
                );
                notes(a,&sheet,part,222.,&["PRINT: X end down; 28 mm thread vertical. Sideways M6 bores have 45-degree roofs for this pose.","CUSTOM rounded 30-degree 24 x 4 thread: radial depth 2, corner R0.3, female radial relief 0.25, total axial relief 0.2 mm.","This is not an ISO Tr designation. Replace the whole keyed bridge when the female thread wears.","Lower keys into the deck sockets before fitting the two M6 x 35 screws. Verify the supplier's hardware envelope."]);
            }
            "screw" => {
                notes(a,&sheet,part,222.,&["PRINT: common shallow flat down. 24 mm shaft, 20 mm root, 4 mm lead; flat is 7 mm below the axis.","Rounded T grip and shaft are one piece. Every oversized thrust feature is part of the removable fitting.","Turn the bare 18 mm stub through the bridge first. Load its M5 nut, then slide the sleeve ledge underneath to hold alignment.","A 1 N.m input and assumed efficiency 0.2 imply 314 N at 4 mm lead; this is sensitivity, not capacity."]);
            }
            "thrust" => {
                radial(
                    a,
                    &sheet,
                    &end,
                    "thrust_head_diameter",
                    part,
                    (16., 120.),
                    "diameter",
                );
                notes(a,&sheet,part,222.,&["PRINT: front head face down; full round bearing and D-keyed blind socket are vertical.","Install AFTER the bare shaft passes the bridge. The neck and head detach as one complete fitting.","Closing thrust goes through the stub end into the blind floor and jaw shoulder; the M5 retains opening motion.","Fit the axial M5 while jaw is parked +85 mm, then return the jaw and fit the keeper. Inspect creep and wear."]);
            }
            "keeper" => {
                notes(a,&sheet,part,222.,&["PRINT: X end down. U throat opens downward in the assembled vise; the transverse bore has a print roof.","11.2 mm central web plus diagonal rear ears; 5.4 mm shoulder width per side bears on the jaw.","The M5 x 90 cross-pin has 1 mm axial float in the keeper so the 0.4 mm bearing gap can seat first.","The diagonal seats carry opening force. Main closing thrust bypasses the keeper through the full-round head and front jaw."]);
            }
            _ => unreachable!(),
        }
        export(a, &sheet, part, &mut exports);
        if part == "jaw" {
            a.call(
                "vise_demo_jaw_sheet",
                "drawing/sheet",
                "drawing_select_sheet",
                json!({"sheet_id":sheet}),
            );
            a.note("Read the carriage drawing", "The projected views locate the 100 mm jaw, 72 mm carriage and captured guide channels. Drawing notes distinguish nominal running gaps from fits that still need measurement.");
            a.steps.push(json!({"note":"Use the end view to inspect the guide profile and the top view for carriage length.","duration_ms":3500}));
        }
    }
    exports
}
