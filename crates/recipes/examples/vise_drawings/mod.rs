//! Persistent drawing commands for the vise recipe. Geometry is measured by
//! native projection; these helpers only select topology and arrange the paper.
use super::{reference, Author};
use serde_json::{json, Value};

struct View {
    id: Value,
    projection: String,
}

fn first(from: Value, path: &str, criteria: Value, pointer: &str) -> Value {
    json!({"$select":{"from":from,"path":path,"where":criteria,"take":"first","pointer":pointer}})
}

fn sheet(a: &mut Author, part: &str, title: &str, assembly: bool) -> Value {
    let name = format!("vise_{part}_sheet");
    a.call(&name, "drawing/sheet", "drawing_create_sheet", json!({
        "name":title,"format":if assembly {"a2"} else {"a3"},"orientation":"landscape","projection_method":"third_angle",
        "title_block":{"title":title,"drawing_number":format!("VISE-{part}"),
            "revision":"A-candidate","material":if assembly {"PETG + purchased M3 hardware"} else {"PETG, provisional"},
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
    let show_hidden = !assembly && key != "nut_top" && key != "nut_front";
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
    let projection = format!("vise_{key}_projection");
    a.call(
        &projection,
        "drawing/views",
        "drawing_projection",
        json!({
            "body_ids":body_ids,"scope":if assembly {"assembly"} else {"definition"},
            "direction":direction,"up":up,"include_hidden":show_hidden
        }),
    );
    View {
        id: reference(&id, ""),
        projection,
    }
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
    radius: f64,
    angle: f64,
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

fn cross_hole_location(a: &mut Author, sheet: &Value, view: &View, part: &str) {
    let circle = first(
        reference(&view.projection, ""),
        "/circles",
        json!({"/body_id":a.body_id(part),"/radius":1.7}),
        "",
    );
    let field = |pointer| first(json!([circle.clone()]), "", json!({}), pointer);
    let center = json!({"body_id":a.body_id(part),"edge_id":field("/edge_id"),
        "edge_key":field("/edge_key"),"endpoint":"start","circle_center":true,
        "fallback_point":field("/center_model")});
    let (side, bottom, horizontal_offset, vertical_offset) = if part == "frame" {
        (-35., 0., -64., 12.)
    } else {
        (-18., 10., -84., -24.)
    };
    let datum = anchor(
        view,
        a.body_id(part),
        json!({"/model_point/1":side,"/model_point/2":bottom}),
        None,
    );
    for (mode, offset) in [
        ("horizontal", horizontal_offset),
        ("vertical", vertical_offset),
    ] {
        a.call(
            &format!("vise_{part}_cross_hole_{mode}_dimension"),
            "drawing/dimensions",
            "drawing_add_linear_dimension",
            json!({"sheet_id":sheet,"view_id":view.id,"first":datum,"second":center,
                "mode":mode,"offset":offset,"precision":2}),
        );
    }
}

// Circle centres provide a real axis for the longitudinal cut without inventing
// an unassociated point. The persistent reference continues to follow the edge.
fn axis_anchor(a: &Author, part: &str, x: f64, assembly: bool) -> Value {
    let edge = first(
        first(
            reference("vise_drawing_scene", ""),
            "/bodies",
            json!({"/id":a.body_id(part)}),
            "",
        ),
        "/edges",
        json!({"/circle/center/x":x,"/circle/center/y":0.,"/circle/center/z":28.}),
        "",
    );
    let field = |pointer| first(json!([edge.clone()]), "", json!({}), pointer);
    let mut result = json!({"body_id":a.body_id(part),"edge_id":field("/id"),"edge_key":field("/key"),
        "endpoint":"start","circle_center":true,
        "fallback_point":[field("/circle/center/x"),field("/circle/center/y"),field("/circle/center/z")]});
    if assembly {
        result["occurrence_id"] = reference(&format!("{part}_occurrence"), "");
    }
    result
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

pub(super) fn add(a: &mut Author) -> Vec<String> {
    let mut exports = Vec::new();
    a.note("Drawings: fits, load path and assembly", "Create persistent assembly and part sheets from native topology. Dimensions stay associated with the model; sections reveal the nut, rotating thrust chamber and keeper. Export both SVG and DXF for review.");
    a.call(
        "vise_drawing_scene",
        "solid/check",
        "solid_scene",
        json!({}),
    );
    let assembly = sheet(a, "assembly", "D screw vise / assembly and service", true);
    let top = view(
        a,
        &assembly,
        "assembly_top",
        "Top / assembled home",
        json!([]),
        [0., 0., 1.],
        [0., 1., 0.],
        [165., 80.],
        1.1,
        true,
        None,
    );
    let section = json!({"type":"section","parent_view_id":top.id,
        "first":axis_anchor(a,"frame",8.,true),"second":axis_anchor(a,"frame",32.,true),
        "label":"A-A","hatch_angle_deg":45.,"hatch_spacing_mm":2.});
    view(
        a,
        &assembly,
        "assembly_section",
        "A-A / screw axis, y = 0",
        json!([]),
        [0., -1., 0.],
        [0., 0., 1.],
        [165., 192.],
        1.1,
        true,
        Some(section),
    );
    view(
        a,
        &assembly,
        "assembly_iso",
        "Assembly / nine components",
        json!([]),
        [1., -1., 1.],
        [0., 0., 1.],
        [455., 95.],
        0.8,
        true,
        None,
    );
    let body = |part| a.body_id(part);
    let first = anchor(
        &top,
        body("jaw"),
        json!({"/model_point/0":82.,"/model_point/1":-30.}),
        Some(reference("jaw_occurrence", "")),
    );
    let second = anchor(
        &top,
        body("frame"),
        json!({"/model_point/0":130.,"/model_point/1":-30.}),
        Some(reference("frame_occurrence", "")),
    );
    a.call(
        "vise_opening_dimension",
        "drawing/dimensions",
        "drawing_add_linear_dimension",
        json!({
            "sheet_id":assembly,"view_id":top.id,"first":first,"second":second,
            "mode":"horizontal","offset":20.,"precision":2,"suffix":" at home"
        }),
    );
    let items = [
        ("frame","Frame + fixed jaw","PETG","Base down"),
        ("jaw","Guided jaw","PETG","Broad end down"),
        ("nut","20.5 x 2.5 wear nut","PETG","Thread axis vertical"),
        ("screw","D screw + paddle","PETG","Through-axis flat down"),
        ("keeper","Retained U keeper","PETG","Broad end down"),
        ("retainer_screw","M3 x 40 socket screw","Purchased","Verify supplier drawing"),
        ("retainer_nut","M3 hex nut","Purchased","Verify supplier drawing"),
        ("cartridge_screw","M3 x 25 cartridge screw","Purchased","Verify supplier drawing"),
        ("cartridge_nut","M3 cartridge hex nut","Purchased","Verify supplier drawing"),
    ].into_iter().enumerate().map(|(i,(part,description,material,finish))| json!({
        "item_number":(i+1).to_string(),"body_id":a.body_id(part),"part_number":format!("VISE-{part}"),
        "description":description,"quantity":1.,"material":material,"finish":finish
    })).collect::<Vec<_>>();
    a.call(
        "vise_assembly_bom",
        "drawing/sheet",
        "drawing_set_bom",
        json!({"sheet_id":assembly,"position":[315.,240.],"items":items}),
    );
    notes(a,&assembly,"assembly",290.,&[
        "ASSEMBLE 1: Print and test the paired interrupted-thread coupon. Deburr rails, thread starts and the keeper slot.",
        "2: Insert the M3 nuts into the jaw and housing pockets. Lower the wear cartridge; fit its M3 x 25 cross-bolt before turning in the D screw.",
        "3: Slide the jaw onto the rails and over the screw head. Lower the keeper over the neck; fit its M3 x 40 screw from above.",
        "4: Check both retainers and the full travel by hand before loading. Keep hands away from the jaw pinch region; use adult guidance.",
        "LOAD PATH: Closing thrust passes from screw head into the jaw shoulder, then through the workpiece to the fixed jaw and frame.",
        "SERVICE: Unload. Remove the top keeper screw, lift keeper, slide jaw off and unscrew the drive. Remove the cartridge cross-bolt before lifting the nut.",
        "The D thread has asymmetric contact. CAD motion and interference checks do not establish creep, wear or a clamping-force rating.",
        "A-A passes through the screw axis. Purchased hardware is shown as clearance envelopes; threads and sockets are not manufactured here.",
    ]);
    export(a, &assembly, "assembly", &mut exports);

    for (part, title, low, high, scale) in [
        (
            "frame",
            "Frame / nut housing and guide datums",
            [0., -35., 0.],
            [150., 35., 52.],
            1.,
        ),
        (
            "jaw",
            "Moving jaw / thrust chamber and keeper seat",
            [60., -30., 8.],
            [82., 30., 52.],
            1.6,
        ),
        (
            "nut",
            "Replaceable custom FDM thread cartridge",
            [12., -18., 10.],
            [28., 18., 46.],
            2.,
        ),
        (
            "screw",
            "One-piece D screw / handle, neck and thrust head",
            [-74., -20., 28.],
            [76., 20., 42.],
            1.,
        ),
        (
            "keeper",
            "Keeper / downward throat and positive retention",
            [66.6, -20., 13.6],
            [71.4, 20., 52.],
            2.,
        ),
    ] {
        let sheet = sheet(a, part, title, false);
        let bodies = json!([a.body_id(part)]);
        let top = view(
            a,
            &sheet,
            &format!("{part}_top"),
            "Top",
            bodies.clone(),
            [0., 0., 1.],
            [0., 1., 0.],
            [120., 65.],
            scale,
            false,
            None,
        );
        let front = view(
            a,
            &sheet,
            &format!("{part}_front"),
            "Front",
            bodies.clone(),
            [0., -1., 0.],
            [0., 0., 1.],
            [120., 157.],
            scale,
            false,
            None,
        );
        let end = view(
            a,
            &sheet,
            &format!("{part}_end"),
            if part == "frame" {
                "Rear / screw-entry axis"
            } else {
                "End / axis view"
            },
            bodies.clone(),
            [
                if part == "frame" || part == "jaw" {
                    -1.
                } else {
                    1.
                },
                0.,
                0.,
            ],
            [0., 0., 1.],
            [310., 95.],
            scale,
            false,
            None,
        );
        // Offsets start at the selected topology endpoint, not the view center.
        // These nominal paper placements were checked against native SVGs and
        // put overall dimension lines 12 mm outside each nominal view boundary.
        let (length_offset, width_offset, height_offset) = match part {
            "frame" => (12., 12., -23.),
            "jaw" => (43.36, 12., -12.),
            "nut" => (35.4226, 84., -84.),
            "screw" => (12., 12.03835, 0.),
            "keeper" => (12., 12., -39.2),
            _ => unreachable!(),
        };
        linear(
            a,
            &sheet,
            &front,
            &format!("{part}_length"),
            part,
            0,
            low[0],
            high[0],
            "horizontal",
            length_offset,
        );
        linear(
            a,
            &sheet,
            &end,
            &format!("{part}_width"),
            part,
            1,
            low[1],
            high[1],
            "horizontal",
            width_offset,
        );
        if part != "screw" {
            linear(
                a,
                &sheet,
                &end,
                &format!("{part}_height"),
                part,
                2,
                low[2],
                high[2],
                "vertical",
                height_offset,
            );
        }
        match part {
            "frame" => {
                cross_hole_location(a, &sheet, &end, part);
                radial(
                    a,
                    &sheet,
                    &end,
                    "frame_cartridge_cross_hole",
                    part,
                    1.7,
                    145.,
                    "diameter",
                );
                radial(
                    a,
                    &sheet,
                    &end,
                    "housing_envelope",
                    part,
                    10.6,
                    45.,
                    "diameter",
                );
                linear(
                    a,
                    &sheet,
                    &top,
                    "cartridge_pocket_length",
                    part,
                    0,
                    11.6,
                    28.4,
                    "horizontal",
                    -17.,
                );
                notes(a,&sheet,part,215.,&[
                    "PRINT: Base down. The housing clearance has a 45-degree self-supporting roof. Deburr mounting slots.",
                    "FIT: Cartridge pocket has 0.4 mm nominal clearance per side. Rail clearance is set in the jaw.",
                    "The broad base carries closing load between the nut housing and fixed jaw. Secure it through the slots.",
                    "Section A-A on the assembly sheet shows the screw envelope, cartridge shoulders and axial load path.",
                    "Cartridge retention: load the M3 hex nut into the rear housing pocket; install the M3 x 25 bolt from the opposite face.",
                ]);
            }
            "jaw" => {
                radial(
                    a,
                    &sheet,
                    &end,
                    "thrust_chamber",
                    part,
                    14.4,
                    40.,
                    "diameter",
                );
                linear(
                    a,
                    &sheet,
                    &front,
                    "keeper_slot_width",
                    part,
                    0,
                    66.2,
                    71.8,
                    "horizontal",
                    -20.,
                );
                linear(
                    a,
                    &sheet,
                    &end,
                    "jaw_guide_width",
                    part,
                    1,
                    -23.4,
                    -18.6,
                    "horizontal",
                    -25.,
                );
                notes(a,&sheet,part,215.,&[
                    "PRINT: Broad end down as shown in the print layout. Keep rails and the thrust shoulder free of support scars.",
                    "FIT: 0.4 mm radial clearance around the 28 mm head. Keeper has 0.4 mm nominal axial clearance per side.",
                    "Load M3 nut from below before installing jaw. Its hex pocket resists rotation; the base prevents escape.",
                    "The frame top supports the jaw bottom; rail sides retain 0.4 mm clearance. The M3 screw retains the keeper against lift.",
                ]);
            }
            "nut" => {
                cross_hole_location(a, &sheet, &end, part);
                radial(
                    a,
                    &sheet,
                    &end,
                    "nut_cartridge_cross_hole",
                    part,
                    1.7,
                    35.,
                    "diameter",
                );
                let section = json!({"type":"section","parent_view_id":top.id,
                    "first":axis_anchor(a,part,12.,false),"second":axis_anchor(a,part,28.,false),
                    "label":"B-B","hatch_angle_deg":45.,"hatch_spacing_mm":1.5});
                view(
                    a,
                    &sheet,
                    "nut_section",
                    "B-B / thread and engagement",
                    bodies,
                    [0., -1., 0.],
                    [0., 0., 1.],
                    [310., 185.],
                    2.,
                    false,
                    Some(section),
                );
                notes(a,&sheet,part,225.,&[
                    "THREAD: Custom nominal 20.5 x 2.5, right hand, ISO 60-degree form; not a standard M20 6H fit.",
                    "0.25 mm nominal radial process relief relative to the M20 screw. Print axis vertical and qualify the coupon.",
                    "16 mm engagement. Housing prevents rotation; the M3 cross-bolt prevents lift. Unload and remove the bolt before replacing the nut.",
                ]);
            }
            "screw" => {
                radial(a, &sheet, &end, "retention_neck", part, 6., 35., "diameter");
                radial(a, &sheet, &end, "thrust_head", part, 14., 145., "radius");
                notes(a,&sheet,part,215.,&[
                    "THREAD: M20 x 2.5 right hand, modeled ISO form, interrupted by the through-axis D flat. Lead is 2.5 mm per turn.",
                    "PRINT: Full length on its flat; inspect the thin thread crests. Keep the neck and thrust face smooth.",
                    "The rear extension keeps the rotating paddle outside the frame through the allowed 48 mm travel.",
                    "The semicircular head and neck rotate in full circular clearances. Qualify fit, friction and wear before loading.",
                ]);
            }
            "keeper" => {
                radial(
                    a,
                    &sheet,
                    &end,
                    "neck_clearance",
                    part,
                    6.4,
                    40.,
                    "diameter",
                );
                linear(
                    a,
                    &sheet,
                    &end,
                    "keeper_throat",
                    part,
                    1,
                    -6.4,
                    6.4,
                    "horizontal",
                    -22.,
                );
                notes(a,&sheet,part,215.,&[
                    "PRINT: Broad end down. The throat opens DOWNWARD in the assembly so the keeper drops over the neck.",
                    "FIT: 0.4 mm radial neck clearance; 0.4 mm nominal axial slot clearance per side. Feet seat on the jaw floor.",
                    "Positive retention uses a purchased M3 x 40 socket screw and trapped M3 nut; do not rely on friction alone.",
                    "Unload the vise before removing the retainer. The keeper carries opening force, not the main closing thrust.",
                ]);
            }
            _ => unreachable!(),
        }
        export(a, &sheet, part, &mut exports);
    }
    exports
}
