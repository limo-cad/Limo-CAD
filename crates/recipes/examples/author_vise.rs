//! Author the readable native command source; this does not create geometry.
//! Run from the repository root: cargo run -p limo-cad-recipes --example author_vise
//! Geometry is produced only when the ordinary Rust MCP interpreter replays it.
#![recursion_limit = "256"]
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
mod vise_demo;
mod vise_drawings;

const D: Design = Design {
    deck: 14.,
    frame_length: 230.,
    frame_width: 160.,
    jaw_width: 100.,
    carriage_width: 110.,
    jaw_top: 80.,
    opening: 90.,
    fixed_face: 205.,
    carriage_length: 72.,
    jaw_web: 38.,
    axis: 50.,
    thread_diameter: 24.,
    lead: 4.,
    thread_depth: 2.,
    thread_corner: 0.3,
    radial_relief: 0.25,
    axial_relief: 0.2,
    flat_below_axis: 7.,
    guide_center: 34.,
    guide_base: 12.,
    guide_head: 28.,
    guide_height: 8.,
    guide_clearance: 0.4,
    bridge_start: 10.,
    bridge_end: 38.,
    bridge_key_depth: 6.,
    bridge_foot_inner: 50.,
    bridge_foot_outer: 78.,
    bridge_bolt_y: 70.,
    fit_clearance: 0.4,
    root_radius: 8.,
    keeper_seat_gap: 0.4,
    keeper_ear_inner: 22.,
    keeper_pin_extension: 1.,
    design_force: 300.,
};

#[derive(Clone, Copy)]
struct Design {
    deck: f64,
    frame_length: f64,
    frame_width: f64,
    jaw_width: f64,
    carriage_width: f64,
    jaw_top: f64,
    opening: f64,
    fixed_face: f64,
    carriage_length: f64,
    jaw_web: f64,
    axis: f64,
    thread_diameter: f64,
    lead: f64,
    thread_depth: f64,
    thread_corner: f64,
    radial_relief: f64,
    axial_relief: f64,
    flat_below_axis: f64,
    guide_center: f64,
    guide_base: f64,
    guide_head: f64,
    guide_height: f64,
    guide_clearance: f64,
    bridge_start: f64,
    bridge_end: f64,
    bridge_key_depth: f64,
    bridge_foot_inner: f64,
    bridge_foot_outer: f64,
    bridge_bolt_y: f64,
    fit_clearance: f64,
    root_radius: f64,
    keeper_seat_gap: f64,
    keeper_ear_inner: f64,
    keeper_pin_extension: f64,
    design_force: f64,
}
impl Design {
    fn home(self) -> f64 {
        self.fixed_face - self.opening
    }
    fn rear(self) -> f64 {
        self.home() - self.carriage_length
    }
    fn wall(self) -> f64 {
        self.home() - self.jaw_web
    }
    fn flat(self) -> f64 {
        self.axis - self.flat_below_axis
    }
    fn stub_start(self) -> f64 {
        self.home() - 36.
    }
    fn stub_end(self) -> f64 {
        self.home() - 18.
    }
    fn head_front(self) -> f64 {
        self.home() - 10.
    }
    fn keeper_back(self) -> f64 {
        self.home() - 32.2
    }
    fn keeper_front(self) -> f64 {
        self.home() - 20.2
    }
    fn pin_x(self) -> f64 {
        (self.keeper_back() + self.keeper_front()) / 2.
    }
    fn jaw_keeper_seat_x(self, abs_y: f64) -> f64 {
        self.keeper_back() - (30. + self.fit_clearance - abs_y)
    }
    fn keeper_ear_back(self, abs_y: f64) -> f64 {
        self.jaw_keeper_seat_x(abs_y) + self.keeper_seat_gap
    }
    fn bridge_top(self) -> f64 {
        self.deck + 25.
    }
    fn nut_support_top(self) -> f64 {
        self.axis - 8. / 3_f64.sqrt() - 0.1
    }
}

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
    sketch_visibility: Option<String>,
    last_build_scene: Option<Value>,
    present_construction: bool,
    profile_view: &'static str,
}
impl Author {
    fn call(&mut self, id: &str, group: &str, operation: &str, arguments: Value) {
        let name = limo_cad_recipes::authoring::feature_name_step(id, operation, &arguments);
        self.steps.push(
            json!({"id":id,"call":{"group":group,"operation":operation,"arguments":arguments}}),
        );
        self.steps.extend(name);
    }
    fn bind(&mut self, name: &str, value: Value) {
        self.steps.push(json!({"let":{name:value}}));
    }
    fn note(&mut self, chapter: &str, note: &str) {
        self.steps
            .push(json!({"chapter":chapter,"note":note,"duration_ms":1800}));
    }
    fn plane(&mut self, axis: &str, distance: f64) -> Value {
        let key = format!("{axis}_{distance}");
        if let Some(plane) = self.planes.get(&key) {
            return plane.clone();
        }
        if distance == 0. {
            return json!({"type":"origin_plane","plane":axis});
        }
        let id = format!("datum_{}", self.planes.len());
        self.call(
            &id,
            "solid/reference",
            "construction_plane_offset",
            json!({
                "name":format!("{axis} datum at {distance} mm"),
                "reference":{"type":"origin_plane","plane":axis},"distance":distance
            }),
        );
        let plane = json!({"type":"datum_plane","datum_id":select(reference(&id,""),"/planes",json!({"/name":format!("{axis} datum at {distance} mm")}),"/datum_id")});
        self.bind(&format!("{id}_plane"), plane);
        let result = reference(&format!("{id}_plane"), "");
        self.planes.insert(key, result.clone());
        result
    }
    fn begin(&mut self, id: &str, axis: &str, distance: f64) {
        assert!(
            self.sketch_visibility.is_none(),
            "Previous sketch visibility was not restored"
        );
        let visibility = format!("{id}_before_sketch_visibility");
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
        self.call(&format!("{id}_clear_sketch_view"), "document/appearance", "project_set_visibility", json!({
            "hidden_body_ids":{"$select":{"from":scene,"path":"/bodies","take":"all","pointer":"/id"}},
            "hidden_sketch_names":reference(&visibility,"/hidden_sketch_names"),
            "hidden_datum_plane_ids":reference(&visibility,"/hidden_datum_plane_ids")
        }));
        self.sketch_visibility = Some(visibility);
        self.profile_view = match axis {
            "xy" => "top",
            "xz" => "front",
            "yz" => "right",
            _ => panic!("Unknown sketch plane {axis}"),
        };
        let plane = self.plane(axis, distance);
        self.call(
            &format!("{id}_begin"),
            "sketch/draw",
            "sketch_begin",
            json!({"name":id,"plane":plane}),
        );
        self.call(
            &format!("{id}_snap"),
            "sketch/selection",
            "sketch_set_grid_snap",
            json!({"enabled":false}),
        );
        self.sketches += 1;
    }
    fn rectangle(&mut self, id: &str, low: [f64; 2], high: [f64; 2]) {
        self.call(
            &format!("{id}_profile"),
            "sketch/draw",
            "sketch_add_rectangle_locked",
            json!({
                "mode":"two_point","anchor":{"x":low[0],"y":low[1]},
                "corner_hint":{"x":high[0],"y":high[1]},"width_mm":high[0]-low[0],
                "height_mm":high[1]-low[1],"ctrl_held":true
            }),
        );
        let point = select(
            reference(&format!("{id}_profile"), "/sketch"),
            "/entities",
            json!({"/kind":"point","/position/x":low[0],"/position/y":low[1]}),
            "/id",
        );
        self.call(
            &format!("{id}_locate"),
            "sketch/constrain",
            "sketch_add_constraint",
            json!({"type":"fix","entity":point}),
        );
    }
    /// A locked circle centered on a fixed point.
    ///
    /// The point is created first and the circle is placed exactly on it, so the
    /// circle already owns that point as its center: a center pick that lands on
    /// an existing point reuses it and binds it. The recipe therefore does not
    /// add `center_coincident` itself — that step now comes back as a duplicate.
    fn circle(&mut self, id: &str, center: [f64; 2], diameter: f64) {
        self.call(
            &format!("{id}_point"),
            "sketch/draw",
            "sketch_add_point",
            json!({"position":{"x":center[0],"y":center[1]},"ctrl_held":true}),
        );
        self.call(
            &format!("{id}_point_fix"),
            "sketch/constrain",
            "sketch_add_constraint",
            json!({"type":"fix","entity":reference(&format!("{id}_point"),"/entities/0")}),
        );
        self.call(&format!("{id}_profile"),"sketch/draw","sketch_add_circle_locked",json!({
            "mode":"center_diameter","anchor":{"x":center[0],"y":center[1]},
            "edge_hint":{"x":center[0]+diameter/2.,"y":center[1]},"diameter_mm":diameter,"ctrl_held":true
        }));
    }
    fn extrude(&mut self, id: &str, distance: f64, operation: &str, part: &str) {
        let view = if self.present_construction {
            self.profile_view
        } else {
            "current"
        };
        let duration = if self.present_construction { 400 } else { 180 };
        self.steps
            .push(json!({"view":view,"fit":true,"target":"active_sketch","duration_ms":duration}));
        self.call(
            &format!("{id}_finish"),
            "sketch/draw",
            "sketch_finish",
            json!({}),
        );
        let targets = if operation == "new_body" {
            json!([])
        } else {
            json!([self.body_id(part)])
        };
        self.call(&format!("{id}_build"),"solid/build","solid_extrude",json!({
            "sketch_name":id,"profile_indices":[0],"operation":operation,"extent":{"type":"distance","distance":distance.abs()},
            "taper_angle_deg":0,"flip":distance<0.,"target_body_ids":targets
        }));
        self.last_build_scene = Some(reference(&format!("{id}_build"), "/scene"));
        let visibility = self
            .sketch_visibility
            .take()
            .expect("Sketch visibility snapshot");
        self.call(
            &format!("{id}_restore_model_view"),
            "document/appearance",
            "project_set_visibility",
            reference(&visibility, ""),
        );
        if operation == "new_body" {
            let feature = json!({"$select":{"from":reference(&format!("{id}_build"),""),"path":"/document/features","take":"last","pointer":"/id"}});
            self.bind(&format!("{part}_feature"), feature);
            self.bind(
                &format!("{part}_body_id"),
                select(
                    reference(&format!("{id}_build"), ""),
                    "/scene/bodies",
                    json!({"/feature_id":reference(&format!("{part}_feature"),"")}),
                    "/id",
                ),
            );
        }
        self.bodies.insert(
            part.into(),
            select(
                reference(&format!("{id}_build"), ""),
                "/scene/bodies",
                json!({"/id":self.body_id(part)}),
                "",
            ),
        );

        self.call(
            &format!("{id}_hide_references"),
            "solid/reference",
            "construction_set_visibility",
            json!({"visible":false}),
        );
        if self.present_construction {
            vise_demo::construction(self, id, part);
        }
    }
    fn body_id(&self, part: &str) -> Value {
        reference(&format!("{part}_body_id"), "")
    }
    fn box_shape(&mut self, id: &str, low: [f64; 3], high: [f64; 3], operation: &str, part: &str) {
        self.begin(id, "xy", low[2]);
        self.rectangle(id, [low[0], low[1]], [high[0], high[1]]);
        self.extrude(id, high[2] - low[2], operation, part);
    }
    fn cylinder_x(
        &mut self,
        id: &str,
        start: f64,
        end: f64,
        radius: f64,
        operation: &str,
        part: &str,
    ) {
        self.begin(id, "yz", start);
        self.circle(id, [0., D.axis], 2. * radius);
        self.extrude(id, end - start, operation, part);
    }
    fn cylinder_z(
        &mut self,
        id: &str,
        (start, end): (f64, f64),
        center: [f64; 2],
        radius: f64,
        operation: &str,
        part: &str,
    ) {
        self.begin(id, "xy", start);
        self.circle(id, center, 2. * radius);
        self.extrude(id, end - start, operation, part);
    }

    fn equal_edge_profile(&mut self, id: &str, vertices: &[[f64; 2]], side_length: f64) {
        let count = vertices.len();
        for i in 0..count {
            let from = vertices[i];
            let to = vertices[(i + 1) % count];
            let mut arguments = json!({"from":{"x":from[0],"y":from[1]},"to_hint":{"x":to[0],"y":to[1]},"ctrl_held":true});
            if i > 0 {
                arguments["from"] = json!({"$select":{"from":reference(&format!("{id}_edge_{}",i-1),"/sketch"),"path":"/entities","where":{"/kind":"line"},"take":"last","pointer":"/end"}});
            }
            if i < count - 1 {
                arguments["angle_text"] = json!(format!(
                    "{:.15}",
                    (to[1] - from[1]).atan2(to[0] - from[0]).to_degrees()
                ));
                if i == 0 {
                    arguments["length_text"] = json!(format!("{side_length:.15}"));
                } else {
                    arguments["length_text"] = json!("d1");
                }
            }
            self.call(
                &format!("{id}_edge_{i}"),
                "sketch/draw",
                "sketch_add_line_locked",
                arguments,
            );
            if i == 0 {
                self.call(&format!("{id}_locate"),"sketch/constrain","sketch_add_constraint",json!({"type":"fix","entity":json!({"$select":{"from":reference(&format!("{id}_edge_0"),"/sketch"),"path":"/entities","where":{"/kind":"line"},"take":"last","pointer":"/start_id"}})}));
            }
        }
        let point_on_edge = |edge: usize, endpoint: &str| json!({"$select":{"from":reference(&format!("{id}_edge_{edge}"),"/sketch"),"path":"/entities","where":{"/kind":"line"},"take":"last","pointer":endpoint}});
        self.call(&format!("{id}_close"),"sketch/constrain","sketch_add_constraint",json!({"type":"coincident","a":point_on_edge(count-1,"/end_id"),"b":point_on_edge(0,"/start_id")}));
    }
    fn show(&mut self, part: &str) {
        if self.present_construction {
            vise_demo::show_part(
                self,
                &format!("completed_{part}"),
                part,
                "Inspect the completed part before it joins the assembly.",
            );
        } else {
            self.steps.push(json!({"view":"isometric","fit":true,"body_id":self.body_id(part),"duration_ms":400}));
        }
    }
    fn component(&mut self, part: &str, title: &str) {
        self.bind(&format!("{part}_body"), self.bodies[part].clone());
        self.call(
            &format!("{part}_component"),
            "assembly/joints",
            "assembly_create_component",
            json!({"name":title,"body_ids":[self.body_id(part)],"absorb_promoted_bodies":true}),
        );
        self.call(
            &format!("{part}_assembly"),
            "assembly/joints",
            "assembly_document",
            json!({}),
        );
        self.bind(
            &format!("{part}_occurrence"),
            select(
                reference(&format!("{part}_assembly"), ""),
                "/component_structure/occurrences",
                json!({"/component_id":reference(&format!("{part}_component"),"/id")}),
                "/id",
            ),
        );
    }
    fn connector(&self, part: &str, origin: [f64; 3]) -> Value {
        json!({"body_id":self.body_id(part),"face_id":select(reference(&format!("{part}_body"),""),"/faces",json!({"/key":reference(&format!("{part}_anchor_face"),"/key")}),"/id"),
            "face_key":reference(&format!("{part}_anchor_face"),"/key"),"kind":"planar_face",
            "frame":{"origin":origin,"primary_axis":[1,0,0],"secondary_axis":[0,1,0]},
            "source_surface_frame":{"origin":reference(&format!("{part}_anchor_face"),"/plane/origin"),"primary_axis":reference(&format!("{part}_anchor_face"),"/plane/normal"),"secondary_axis":reference(&format!("{part}_anchor_face"),"/plane/u")}})
    }
    fn joint(&mut self, id: &str, kind: &str, a: &str, b: &str, origin: [f64; 3], limits: Value) {
        let ca = self.connector(a, origin);
        let cb = self.connector(b, origin);

        let phase =
            (270. + (D.stub_start() - (D.bridge_start - 0.0001)) / D.lead * 360.).rem_euclid(360.);
        let home_twist = match id {
            "screw_drive" => phase,
            "thrust_retention" => -phase,
            _ => 0.,
        };
        self.call(id,"assembly/joints","assembly_create_joint",json!({
            "name":id,"kind":kind,"connector_a":ca,"connector_b":cb,"flipped":true,
            "angle_offset_deg":0,"linear_offset_mm":0,"linear_limits":limits,
            "advanced":{"screw_pitch_mm_per_revolution":D.lead,
                "connector_a_twist_deg":home_twist,
                "connector_a_occurrence_id":reference(&format!("{a}_occurrence"),""),"connector_b_occurrence_id":reference(&format!("{b}_occurrence"),"")}
        }));
    }
}

impl Author {
    fn fresh() -> Self {
        Self {
            steps: Vec::new(),
            planes: BTreeMap::new(),
            bodies: BTreeMap::new(),
            sketches: 0,
            sketch_visibility: None,
            last_build_scene: None,
            present_construction: false,
            profile_view: "current",
        }
    }
    fn refresh_body(&mut self, id: &str, part: &str) {
        self.bodies.insert(
            part.into(),
            select(
                reference(id, ""),
                "/scene/bodies",
                json!({"/id":self.body_id(part)}),
                "",
            ),
        );
    }
    fn polygon(&mut self, id: &str, vertices: &[[f64; 2]]) {
        for i in 0..vertices.len() {
            let from = vertices[i];
            let to = vertices[(i + 1) % vertices.len()];
            let mut args = json!({"from":{"x":from[0],"y":from[1]},"to_hint":{"x":to[0],"y":to[1]},"ctrl_held":true});
            if i > 0 {
                args["from"] = json!({"$select":{"from":reference(&format!("{id}_edge_{}",i-1),"/sketch"),"path":"/entities","where":{"/kind":"line"},"take":"last","pointer":"/end"}});
            }
            if i + 1 < vertices.len() {
                args["angle_text"] = json!(format!(
                    "{:.15}",
                    (to[1] - from[1]).atan2(to[0] - from[0]).to_degrees()
                ));
                args["length_text"] =
                    json!(format!("{:.15}", (to[0] - from[0]).hypot(to[1] - from[1])));
            }
            self.call(
                &format!("{id}_edge_{i}"),
                "sketch/draw",
                "sketch_add_line_locked",
                args,
            );
            if i == 0 {
                self.call(&format!("{id}_locate"),"sketch/constrain","sketch_add_constraint",json!({"type":"fix","entity":json!({"$select":{"from":reference(&format!("{id}_edge_0"),"/sketch"),"path":"/entities","where":{"/kind":"line"},"take":"last","pointer":"/start_id"}})}));
            }
        }
        let endpoint = |edge: usize, p: &str| json!({"$select":{"from":reference(&format!("{id}_edge_{edge}"),"/sketch"),"path":"/entities","where":{"/kind":"line"},"take":"last","pointer":p}});
        self.call(&format!("{id}_close"),"sketch/constrain","sketch_add_constraint",json!({"type":"coincident","a":endpoint(vertices.len()-1,"/end_id"),"b":endpoint(0,"/start_id")}));
    }
    fn prism_x(
        &mut self,
        id: &str,
        start: f64,
        end: f64,
        vertices: &[[f64; 2]],
        operation: &str,
        part: &str,
    ) {
        self.begin(id, "yz", start);
        self.polygon(id, vertices);
        self.extrude(id, end - start, operation, part);
    }
    fn prism_y(
        &mut self,
        id: &str,
        start: f64,
        end: f64,
        vertices: &[[f64; 2]],
        operation: &str,
        part: &str,
    ) {
        self.begin(id, "xz", -start);
        self.polygon(id, vertices);
        self.extrude(id, start - end, operation, part);
    }
    fn prism_z(
        &mut self,
        id: &str,
        start: f64,
        end: f64,
        vertices: &[[f64; 2]],
        operation: &str,
        part: &str,
    ) {
        self.begin(id, "xy", start);
        self.polygon(id, vertices);
        self.extrude(id, end - start, operation, part);
    }
    fn bore_x(
        &mut self,
        id: &str,
        (start, end): (f64, f64),
        center: [f64; 2],
        radius: f64,
        operation: &str,
        part: &str,
    ) {
        self.begin(id, "yz", start);
        self.circle(id, center, 2. * radius);
        self.extrude(id, end - start, operation, part);
    }
    fn bore_y(
        &mut self,
        id: &str,
        (start, end): (f64, f64),
        center: [f64; 2],
        radius: f64,
        operation: &str,
        part: &str,
    ) {
        self.begin(id, "xz", -start);
        self.circle(id, center, 2. * radius);
        self.extrude(id, start - end, operation, part);
    }
    fn hex(
        &mut self,
        id: &str,
        axis: &str,
        (start, end): (f64, f64),
        center: [f64; 2],
        af: f64,
        (operation, part): (&str, &str),
    ) {
        let r = af / 3_f64.sqrt();
        let points: Vec<_> = (0..6)
            .map(|i| {
                let angle = ((if axis == "xz" { 0. } else { 90. }) + i as f64 * 60.).to_radians();
                [center[0] + r * angle.cos(), center[1] + r * angle.sin()]
            })
            .collect();
        self.begin(id, axis, if axis == "xz" { -start } else { start });
        self.equal_edge_profile(id, &points, r);
        self.extrude(
            id,
            if axis == "xz" {
                start - end
            } else {
                end - start
            },
            operation,
            part,
        );
    }

    fn teardrop_z(&mut self, id: &str, start: f64, end: f64, c: [f64; 2], r: f64, part: &str) {
        self.cylinder_z(id, (start, end), c, r, "cut", part);
        let t = r * std::f64::consts::FRAC_1_SQRT_2;
        let roof = format!("{id} / print roof");
        self.begin(&roof, "xy", start);
        self.polygon(
            &roof,
            &[
                [c[0] - t, c[1] - t],
                [c[0] - r * 2_f64.sqrt(), c[1]],
                [c[0] - t, c[1] + t],
            ],
        );
        self.extrude(&roof, end - start, "cut", part);
    }
    fn teardrop_y(&mut self, id: &str, start: f64, end: f64, c: [f64; 2], r: f64, part: &str) {
        self.bore_y(id, (start, end), c, r, "cut", part);
        let t = r * std::f64::consts::FRAC_1_SQRT_2;
        self.prism_y(
            &format!("{id} / print roof"),
            start,
            end,
            &[
                [c[0] - t, c[1] - t],
                [c[0] - r * 2_f64.sqrt(), c[1]],
                [c[0] - t, c[1] + t],
            ],
            "cut",
            part,
        );
    }
    fn fillet(&mut self, id: &str, part: &str, criteria: Value, radius: f64) {
        self.bind(&format!("{id}_edges"),json!({"$select":{"from":self.bodies[part],"path":"/edges","where":criteria,"take":"all","pointer":"/id"}}));
        self.bind(
            &format!("{id}_first_edge"),
            json!({"$select":{"from":reference(&format!("{id}_edges"),""),"take":"first"}}),
        );
        self.call(id,"solid/refine","solid_fillet",json!({"body_id":self.body_id(part),"edge_ids":reference(&format!("{id}_edges"),""),"radius":radius,"tangent_chain":false}));
        self.refresh_body(id, part);
    }
    fn chamfer(&mut self, id: &str, part: &str, criteria: Value, distance: f64) {
        self.bind(&format!("{id}_edges"),json!({"$select":{"from":self.bodies[part],"path":"/edges","where":criteria,"take":"all","pointer":"/id"}}));
        self.bind(
            &format!("{id}_first_edge"),
            json!({"$select":{"from":reference(&format!("{id}_edges"),""),"take":"first"}}),
        );
        self.call(id,"solid/refine","solid_chamfer",json!({"body_id":self.body_id(part),"edge_ids":reference(&format!("{id}_edges"),""),"distance":distance,"tangent_chain":false}));
        self.refresh_body(id, part);
    }
    fn vertical_corners(&mut self, id: &str, part: &str, corners: &[[f64; 2]], radius: f64) {
        let edges:Vec<_>=corners.iter().map(|c|select(self.bodies[part].clone(),"/edges",json!({"/refinable":true,"$every":{"path":"/points","where":{"/x":c[0],"/y":c[1]}}}),"/id")).collect();
        self.call(id,"solid/refine","solid_fillet",json!({"body_id":self.body_id(part),"edge_ids":edges,"radius":radius,"tangent_chain":false}));
        self.refresh_body(id, part);
    }
}

fn thread(depth: Option<f64>) -> Value {
    json!({"standard":"custom_trapezoidal","series":"rounded","designation":"CUSTOM rounded 30-degree 24 x 4 FDM lead screw; not ISO Tr","class":"custom","nominal_diameter":D.thread_diameter,"pitch":D.lead,"threads_per_inch":null,"hand":"right","depth":depth,"representation":"modeled","rounded_profile":{"radial_depth":D.thread_depth,"corner_radius":D.thread_corner,"radial_clearance":D.radial_relief,"axial_clearance":D.axial_relief}})
}
fn guide_profile(center: f64, clearance: f64) -> Vec<[f64; 2]> {
    vec![
        [center - D.guide_base / 2. - clearance, D.deck],
        [center + D.guide_base / 2. + clearance, D.deck],
        [
            center + D.guide_head / 2. + 2. * clearance,
            D.deck + D.guide_height + clearance,
        ],
        [
            center - D.guide_head / 2. - 2. * clearance,
            D.deck + D.guide_height + clearance,
        ],
    ]
}
fn guide_rail_profile(center: f64) -> Vec<[f64; 2]> {
    vec![
        [center - D.guide_base / 2., D.deck],
        [center + D.guide_base / 2., D.deck],
        [center + D.guide_head / 2., D.deck + D.guide_height],
        [center - D.guide_head / 2., D.deck + D.guide_height],
    ]
}
fn set_pose(a: &mut Author, id: &str, part: &str, translation: Value, rotation: Value) {
    a.call(id,"assembly/joints","assembly_set_occurrence_pose",json!({"occurrence_id":reference(&format!("{part}_occurrence"),""),"local_pose":{"translation":translation,"rotation":rotation}}));
}
fn final_checks(a: &Author) -> Vec<Value> {
    let mut checks = Vec::new();
    for (id, group, operation) in [
        ("final_scene", "solid/check", "solid_scene"),
        ("final_sketches", "sketch/draw", "sketch_finished"),
        ("final_assembly", "assembly/joints", "assembly_document"),
        ("final_solution", "assembly/joints", "assembly_solution"),
        ("final_model", "document/files", "cad_project_model"),
    ] {
        checks.push(json!({"id":id,"call":{"group":group,"operation":operation,"arguments":{}}}));
    }
    checks.push(json!({"assert":reference("final_scene","/errors"),"equals":[]}));
    checks.push(
        json!({"assert":{"$count":reference("final_scene","/bodies")},"equals":a.bodies.len()}),
    );
    for i in 0..a.sketches {
        checks.push(
            json!({"assert":reference("final_sketches",&format!("/{i}/dof/value")),"equals":0}),
        );
    }
    checks.push(json!({"assert":reference("final_solution","/solved"),"equals":true}));
    checks.push(json!({"assert":reference("final_solution","/diagnostics"),"equals":[]}));
    checks.push(json!({"id":"final_interference","call":{"group":"assembly/inspect","operation":"assembly_interference_check","arguments":{"clearance_threshold_mm":0}}}));
    checks.push(json!({"assert":{"$count":{"$select":{"from":reference("final_interference",""),"path":"/pairs","where":{"/interfering":true},"take":"all"}}},"equals":0}));
    checks
}
fn write_script(
    path: &str,
    name: &str,
    a: Author,
    exports: Map<String, Value>,
    checks: Vec<Value>,
) {
    let doc = json!({"$schema":"./limo-cad-script.schema.json","version":1,"name":name,"starting_state":"empty","steps":a.steps,"checks":checks,"exports":exports});
    let text=format!("// Native editable manufacturing candidate. Millimetres.\n// Authored by crates/recipes/examples/author_vise.rs; geometry is built only by native MCP replay.\n// Print poses and clearances are design intent; physical fit/load/creep and slicer qualification remain required.\n{}\n",serde_json::to_string_pretty(&doc).unwrap());
    limo_cad_script::Script::parse(&text).expect("authored script preflight");
    std::fs::write(path, text).unwrap();
}

fn main() {
    let mut a = Author::fresh();
    a.present_construction = true;
    a.note("A larger workholding vise", "100 mm jaws, 90 mm opening and a 230 x 160 mm frame. Six printed parts use a 24 x 4 mm screw and purchased hardware. Physical fit and load qualification remain pending.");
    a.call(
        "name",
        "document/files",
        "cad_set_document_name",
        json!({"name":"100 mm captured-slide vise / workholding development"}),
    );
    a.note("Frame: stiff deck and useful mounting lands", "A 14 mm deck and 25 mm fixed jaw carry the closing load. The large root blend has a matching clearance in the moving carriage. Outboard mounting lands accept bolts or edge clamps without entering the jaw sweep.");
    a.box_shape(
        "Frame deck / 230 by 160 by 14",
        [0., -D.frame_width / 2., 0.],
        [D.frame_length, D.frame_width / 2., D.deck],
        "new_body",
        "frame",
    );
    a.vertical_corners(
        "Frame / rounded deck corners",
        "frame",
        &[[0., -80.], [0., 80.], [230., -80.], [230., 80.]],
        6.,
    );
    a.box_shape(
        "Fixed jaw / 100 mm face and 25 mm stock",
        [D.fixed_face, -D.jaw_width / 2., D.deck],
        [D.frame_length, D.jaw_width / 2., D.jaw_top],
        "join",
        "frame",
    );
    a.fillet("Fixed jaw / reinforced 8 mm root","frame",json!({"/refinable":true,"$every":{"path":"/points","where":{"/x":D.fixed_face,"/z":D.deck}}}),D.root_radius);
    a.fillet(
        "Fixed jaw / rounded touched rim",
        "frame",
        json!({"/refinable":true,"$every":{"path":"/points","where":{"/z":D.jaw_top}}}),
        2.,
    );
    for (side, y) in [("left", -D.guide_center), ("right", D.guide_center)] {
        a.prism_x(
            &format!("Captured rail {side} / 45 degree flanks"),
            0.,
            D.fixed_face - D.root_radius,
            &guide_rail_profile(y),
            "join",
            "frame",
        );
    }
    for (side, sign) in [("left", -1.), ("right", 1.)] {
        let (low, high) = if sign < 0. {
            (-62.4, -49.6)
        } else {
            (49.6, 62.4)
        };
        a.box_shape(
            &format!("Bridge key socket {side} / axial load shoulder"),
            [
                D.bridge_start - D.fit_clearance,
                low,
                D.deck - D.bridge_key_depth,
            ],
            [D.bridge_end + D.fit_clearance, high, D.deck + 0.1],
            "cut",
            "frame",
        );
        a.cylinder_z(
            &format!("Bridge bolt {side} / frame clearance"),
            (-0.1, D.deck + 0.1),
            [(D.bridge_start + D.bridge_end) / 2., sign * D.bridge_bolt_y],
            3.3,
            "cut",
            "frame",
        );
        a.hex(
            &format!("Bridge nut {side} / underside captive pocket"),
            "xy",
            (0., 10.),
            [(D.bridge_start + D.bridge_end) / 2., sign * D.bridge_bolt_y],
            10.6,
            ("cut", "frame"),
        );
    }
    for (i, x) in [70., 155.].into_iter().enumerate() {
        for (j, y) in [-68., 68.].into_iter().enumerate() {
            a.box_shape(
                &format!("Mount slot {i}-{j} / outboard 20 by 7"),
                [x - 10., y - 3.5, -0.1],
                [x + 10., y + 3.5, D.deck + 0.1],
                "cut",
                "frame",
            );
        }
    }
    a.show("frame");

    a.note("Captured carriage", "The 72 mm carriage has two wide dovetail channels, a 100 mm gripping face and a broad support datum. It enters from the open rear before the bridge is installed. The front underside clears the fixed-jaw root at full closure. Print gripping face down, with channels parallel to the print direction.");
    a.box_shape(
        "Carriage / 72 mm captured bearing length",
        [D.rear(), -D.carriage_width / 2., D.deck],
        [D.home(), D.carriage_width / 2., D.deck + 20.],
        "new_body",
        "jaw",
    );
    a.box_shape(
        "Moving jaw / 100 mm gripping face",
        [D.wall(), -D.jaw_width / 2., D.deck],
        [D.home(), D.jaw_width / 2., D.jaw_top],
        "join",
        "jaw",
    );

    let gusset_outer = D.jaw_width / 2. - 2.;
    for (side, lo, hi) in [
        ("left", -gusset_outer, -gusset_outer + 15.),
        ("right", gusset_outer - 15., gusset_outer),
    ] {
        a.prism_y(
            &format!("Jaw gusset {side} / load into long carriage"),
            lo,
            hi,
            &[
                [D.rear() + 12., D.deck + 20.],
                [D.wall(), D.deck + 20.],
                [D.wall(), D.jaw_top - 10.],
            ],
            "join",
            "jaw",
        );
    }
    a.fillet(
        "Moving jaw / rounded touched rim",
        "jaw",
        json!({"/refinable":true,"$every":{"path":"/points","where":{"/z":D.jaw_top}}}),
        2.,
    );
    for (side, y) in [("left", -D.guide_center), ("right", D.guide_center)] {
        a.prism_x(
            &format!("Jaw dovetail {side} / profile clearance"),
            D.rear() - 1.,
            D.home() + 1.,
            &guide_profile(y, D.guide_clearance),
            "cut",
            "jaw",
        );
    }
    a.prism_y(
        "Carriage / fixed-root clearance",
        -56.,
        56.,
        &[
            [D.home() - D.root_radius - D.fit_clearance, D.deck - 0.1],
            [D.home() + 0.1, D.deck - 0.1],
            [D.home() + 0.1, D.deck + D.root_radius + D.fit_clearance],
        ],
        "cut",
        "jaw",
    );
    a.cylinder_x(
        "Jaw / full round thrust chamber",
        D.wall() - 1.,
        D.head_front() + D.fit_clearance,
        16. + D.fit_clearance,
        "cut",
        "jaw",
    );
    a.box_shape(
        "Jaw / removable keeper slot",
        [D.keeper_back(), -30.4, D.axis - 16.4],
        [D.keeper_front(), 30.4, D.jaw_top + 1.],
        "cut",
        "jaw",
    );

    a.box_shape(
        "Jaw / open keeper rear relief",
        [D.rear() - 1., -D.keeper_ear_inner, D.axis - 16.4],
        [D.keeper_back() + 0.1, D.keeper_ear_inner, D.jaw_top + 1.],
        "cut",
        "jaw",
    );
    for (side, sign) in [("left", -1.), ("right", 1.)] {
        a.prism_z(
            &format!("Jaw / 45 degree keeper shoulder {side}"),
            D.axis - 16.4,
            D.jaw_top + 1.,
            &[
                [D.keeper_back(), sign * D.keeper_ear_inner],
                [D.keeper_back(), sign * (30. + D.fit_clearance)],
                [
                    D.jaw_keeper_seat_x(D.keeper_ear_inner),
                    sign * D.keeper_ear_inner,
                ],
            ],
            "cut",
            "jaw",
        );
    }

    let jaw_access_outer = D.carriage_width / 2. + 1.;
    a.teardrop_y(
        "Jaw / keeper pin clearance",
        -jaw_access_outer,
        jaw_access_outer,
        [D.pin_x(), 70.],
        2.75,
        "jaw",
    );
    a.teardrop_y(
        "Jaw / recessed keeper pin head",
        -jaw_access_outer,
        -45.,
        [D.pin_x(), 70.],
        4.6,
        "jaw",
    );
    a.hex(
        "Jaw / keeper nut pocket",
        "xz",
        (40.3, jaw_access_outer),
        [D.pin_x(), 70.],
        8.5,
        ("cut", "jaw"),
    );
    a.show("jaw");

    a.note("A replaceable bridge with positive keys", "The bridge itself is the wear nut. Its two feet enter recessed deck sockets; those shoulders carry axial load, while accessible M6 bolts retain the feet. Removing the bridge exposes the carriage entry. Print on the X end so the full 28 mm female thread is vertical; sideways bolt bores have 45 degree print roofs.");
    a.box_shape(
        "Threaded bridge / central housing",
        [D.bridge_start, -20., D.deck + 9.],
        [D.bridge_end, 20., D.jaw_top],
        "new_body",
        "nut",
    );
    a.box_shape(
        "Threaded bridge / broad load crossbeam",
        [D.bridge_start, -D.bridge_foot_outer, D.deck + 9.],
        [D.bridge_end, D.bridge_foot_outer, D.bridge_top()],
        "join",
        "nut",
    );
    for (side, sign) in [("left", -1.), ("right", 1.)] {
        a.prism_x(
            &format!("Bridge gusset {side} / thread reaction into keyed feet"),
            D.bridge_start,
            D.bridge_end,
            &[
                [sign * 20., D.bridge_top()],
                [sign * 62., D.bridge_top()],
                [sign * 20., D.jaw_top - 10.],
            ],
            "join",
            "nut",
        );
    }
    for (side, sign) in [("left", -1.), ("right", 1.)] {
        let (low, high) = if sign < 0. {
            (-D.bridge_foot_outer, -D.bridge_foot_inner)
        } else {
            (D.bridge_foot_inner, D.bridge_foot_outer)
        };
        a.box_shape(
            &format!("Bridge foot {side} / deck seat"),
            [D.bridge_start, low, D.deck],
            [D.bridge_end, high, D.deck + 11.],
            "join",
            "nut",
        );
        let (kl, kh) = if sign < 0. { (-62., -50.) } else { (50., 62.) };
        a.box_shape(
            &format!("Bridge key {side} / replaceable shear shoulder"),
            [
                D.bridge_start,
                kl,
                D.deck - D.bridge_key_depth + D.fit_clearance,
            ],
            [D.bridge_end, kh, D.deck + 0.2],
            "join",
            "nut",
        );
        a.teardrop_z(
            &format!("Bridge bolt {side} / self-supporting bore"),
            D.deck - 1.,
            D.bridge_top() + 1.,
            [(D.bridge_start + D.bridge_end) / 2., sign * D.bridge_bolt_y],
            3.3,
            "nut",
        );
    }
    a.fillet(
        "Bridge / soft top edges",
        "nut",
        json!({"/refinable":true,"$every":{"path":"/points","where":{"/z":D.jaw_top}}}),
        3.,
    );

    a.bind(
        "nut_start_face",
        select(
            a.bodies["nut"].clone(),
            "/faces",
            json!({"/plane/normal/0":-1}),
            "",
        ),
    );
    let female = json!({"body_id":a.body_id("nut"),"face_id":reference("nut_start_face","/id"),"position":{"$project":{"point":[D.bridge_start,0.,D.axis],"basis":reference("nut_start_face","/plane")}},"diameter":D.thread_diameter-2.*D.thread_depth,"extent":{"type":"through_all"},"style":"simple","flip":false,"thread":thread(None)});
    a.show("nut");

    a.note("A compact rounded grip and a shallower flat", "The 24 mm shaft has a 4 mm lead and a flat 7 mm below its axis. With the 20 mm root cylinder this starts near a 45 degree underside. A thick rounded T-grip shares that bed plane and clears a tabletop by more than 25 mm throughout rotation. Fit and turning effort still need a real print coupon.");
    a.cylinder_x(
        "Screw / 24 mm rounded-thread blank",
        -D.opening - 6.,
        D.stub_start(),
        D.thread_diameter / 2.,
        "new_body",
        "screw",
    );
    a.cylinder_x(
        "Screw / 18 mm removable-fitting stub",
        D.stub_start() - 0.2,
        D.stub_end(),
        9.,
        "join",
        "screw",
    );
    let grip_start = -D.opening - 25.;
    let grip_end = -D.opening - 5.;
    a.prism_x(
        "Screw grip / thick comfortable T profile",
        grip_start,
        grip_end,
        &[
            [-17., D.flat()],
            [17., D.flat()],
            [22., D.flat() + 5.],
            [22., D.flat() + 16.],
            [14., D.flat() + 24.],
            [-14., D.flat() + 24.],
            [-22., D.flat() + 16.],
            [-22., D.flat() + 5.],
        ],
        "join",
        "screw",
    );
    for (i, (y, z)) in [
        (-22., D.flat() + 16.),
        (-14., D.flat() + 24.),
        (14., D.flat() + 24.),
        (22., D.flat() + 16.),
    ]
    .into_iter()
    .enumerate()
    {
        a.fillet(
            &format!("Grip / rounded upper transition {i}"),
            "screw",
            json!({"/refinable":true,"$every":{"path":"/points","where":{"/y":y,"/z":z}}}),
            4.,
        );
    }

    for (i, y) in [-22., 22.].into_iter().enumerate() {
        a.fillet(
            &format!("Grip / rounded lower wing {i}"),
            "screw",
            json!({"/refinable":true,"$every":{"path":"/points","where":{"/y":y,"/z":D.flat()+5.}}}),
            2.5,
        );
    }

    for (end, x) in [("rear", grip_start), ("front", grip_end)] {
        a.chamfer(
            &format!("Grip / {end} touch rim"),
            "screw",
            json!({"/refinable":true,"$every":{"path":"/points","where":{"/x":x}},"$or":[{"/circle":null},{"/circle/radius":2.5},{"/circle/radius":4.}]}),
            0.6,
        );
    }
    a.note("Rounded contact surfaces, deliberate running fits", "The grip has R4 upper blends, R2.5 lower-wing blends and 0.6 mm end-rim chamfers. Its central print flat stays intact. Frame, jaw and bridge top rims are softened, while the dovetail running faces and assembly datums retain their designed dimensions. Inspect the toolpaths and printed touch edges before use.");
    a.bore_x(
        "Screw / axial M5 clearance",
        (D.stub_start() - 7., D.stub_end() + 1.),
        [0., D.axis],
        2.75,
        "cut",
        "screw",
    );
    a.hex(
        "Screw / captive fitting nut",
        "yz",
        (D.stub_start() + 1., D.stub_start() + 6.3),
        [0., D.axis],
        8.5,
        ("cut", "screw"),
    );
    a.box_shape(
        "Screw / nut loading throat from print flat",
        [D.stub_start() + 1., -4.25, D.flat() - 1.],
        [D.stub_start() + 6.3, 4.25, D.axis],
        "cut",
        "screw",
    );

    a.box_shape(
        "Screw / sliding nut-support keyway",
        [D.stub_start() + 0.8, -4.25, D.flat() - 1.],
        [D.stub_end() + 1., 4.25, D.nut_support_top() + 0.2],
        "cut",
        "screw",
    );
    a.bind(
        "screw_thread_face",
        select(
            a.bodies["screw"].clone(),
            "/faces",
            json!({"/cylinder/radius":D.thread_diameter/2.}),
            "",
        ),
    );
    a.bind("male_thread_request",json!({"body_id":a.body_id("screw"),"face_id":reference("screw_thread_face","/id"),"flip":false,"thread":thread(Some(D.stub_start()+D.opening-4.))}));

    a.note("A complete removable thrust fitting", "The full round head and sleeve come off together. First turn the bare 18 mm keyed stub through the bridge, then load its M5 nut with the jaw parked forward. The fitting slides over the stub and its internal ledge holds the nut aligned before the axial bolt enters. Closing thrust uses the stub end and blind fitting floor; the bolt and captive nut retain the fitting for opening. No large integral collar blocks installation.");
    let sleeve_start = D.stub_start() + D.fit_clearance;
    let head_back = D.head_front() - 10.;
    a.cylinder_x(
        "Thrust fitting / full round neck",
        sleeve_start,
        head_back + 0.2,
        11.,
        "new_body",
        "thrust",
    );
    a.cylinder_x(
        "Thrust fitting / 32 mm load head",
        head_back,
        D.head_front(),
        16.,
        "join",
        "thrust",
    );

    let r = 9. + D.radial_relief;
    let chord = (r * r - (D.flat_below_axis + D.radial_relief).powi(2)).sqrt();
    a.begin("Thrust fitting / D socket", "yz", sleeve_start - 0.1);

    a.circle("Thrust fitting / D socket", [0., D.axis], 2. * r);
    a.extrude(
        "Thrust fitting / D socket",
        D.stub_end() + D.fit_clearance - (sleeve_start - 0.1),
        "cut",
        "thrust",
    );

    a.prism_x(
        "Thrust fitting / D key bearing",
        sleeve_start,
        D.stub_end() + D.fit_clearance,
        &[
            [-chord, D.flat() - D.radial_relief],
            [chord, D.flat() - D.radial_relief],
            [chord, D.axis - r],
            [-chord, D.axis - r],
        ],
        "join",
        "thrust",
    );
    a.box_shape(
        "Thrust fitting / captive nut support ledge",
        [D.stub_start() + 1., -4., D.axis - r],
        [D.stub_end() + D.fit_clearance, 4., D.nut_support_top()],
        "join",
        "thrust",
    );
    a.bore_x(
        "Thrust fitting / axial retaining screw",
        (D.stub_end(), D.head_front() + 1.),
        [0., D.axis],
        2.75,
        "cut",
        "thrust",
    );
    a.bore_x(
        "Thrust fitting / recessed M5 socket head",
        (D.head_front() - 5., D.head_front() + 1.),
        [0., D.axis],
        4.6,
        "cut",
        "thrust",
    );
    a.show("thrust");

    a.note("Accessible keeper with printed bearing shoulders", "A thick U keeper drops over the full round sleeve. Its diagonal rear ears bear on matching jaw shoulders after 0.4 mm axial seating. A recessed M5 x 90 cross-pin keeps it from lifting; its 1 mm elongated keeper bore lets the bearing faces seat first. The open rear relief removes the former broad print bridge.");
    a.box_shape(
        "Keeper / 11.2 mm cross plate",
        [D.keeper_back() + D.fit_clearance, -30., D.axis - 16.4],
        [D.keeper_front() - D.fit_clearance, 30., D.jaw_top],
        "new_body",
        "keeper",
    );
    for (side, sign) in [("left", -1.), ("right", 1.)] {
        a.prism_z(
            &format!("Keeper / diagonal opening-load ear {side}"),
            D.axis - 16.4,
            D.jaw_top,
            &[
                [
                    D.keeper_ear_back(D.keeper_ear_inner),
                    sign * D.keeper_ear_inner,
                ],
                [D.keeper_ear_back(30.), sign * 30.],
                [D.keeper_back() + 2. * D.fit_clearance, sign * 30.],
                [
                    D.keeper_back() + 2. * D.fit_clearance,
                    sign * D.keeper_ear_inner,
                ],
            ],
            "join",
            "keeper",
        );
    }
    a.cylinder_x(
        "Keeper / sleeve running clearance",
        D.keeper_back() - 1.,
        D.keeper_front() + 1.,
        11. + D.fit_clearance,
        "cut",
        "keeper",
    );
    a.box_shape(
        "Keeper / downward installation throat",
        [D.keeper_back() - 1., -11.4, D.axis - 17.],
        [D.keeper_front() + 1., 11.4, D.axis],
        "cut",
        "keeper",
    );
    for (end, offset) in [("rear", -0.5), ("front", 0.5)] {
        a.teardrop_y(
            if end == "rear" {
                "Keeper / cross-pin clearance"
            } else {
                "Keeper / cross-pin axial slot front"
            },
            -31.,
            31.,
            [D.pin_x() + offset * D.keeper_pin_extension, 70.],
            2.75,
            "keeper",
        );
    }
    a.box_shape(
        "Keeper / cross-pin slot connecting web",
        [D.pin_x() - D.keeper_pin_extension / 2., -31., 70. - 2.75],
        [D.pin_x() + D.keeper_pin_extension / 2., 31., 70. + 2.75],
        "cut",
        "keeper",
    );
    a.fillet(
        "Keeper / rounded top rim",
        "keeper",
        json!({"/refinable":true,"$every":{"path":"/points","where":{"/z":D.jaw_top}}}),
        2.,
    );
    a.show("keeper");

    a.note("Real installation hardware envelopes", "Purchased M6 bridge screws, M5 keeper and thrust screws, trapped nuts, and optional M6 tabletop bolts and washers are modeled in their installed positions. Their simplified threads reserve physical envelopes; verify supplier dimensions. Only printed parts enter the print plates.");
    let mut hardware: Vec<(String, String, String)> = Vec::new();
    for (side, sign) in [("left", -1.), ("right", 1.)] {
        let center = [(D.bridge_start + D.bridge_end) / 2., sign * D.bridge_bolt_y];
        let bolt = format!("bridge_screw_{side}");
        a.cylinder_z(
            &format!("{bolt} / M6 x 35 shaft"),
            (D.bridge_top() - 35., D.bridge_top()),
            center,
            3.,
            "new_body",
            &bolt,
        );
        a.cylinder_z(
            &format!("{bolt} / socket head"),
            (D.bridge_top(), D.bridge_top() + 6.),
            center,
            5.,
            "join",
            &bolt,
        );
        hardware.push((
            bolt,
            format!("M6 x 35 socket screw / rear bridge {side}"),
            "frame".into(),
        ));
        let nut = format!("bridge_nut_{side}");
        a.hex(
            &format!("{nut} / M6 envelope"),
            "xy",
            (5., 10.),
            center,
            10.,
            ("new_body", &nut),
        );
        a.cylinder_z(
            &format!("{nut} / thread envelope"),
            (4.9, 10.1),
            center,
            3.,
            "cut",
            &nut,
        );
        hardware.push((nut, format!("M6 nut / rear bridge {side}"), "frame".into()));
    }
    a.bore_y(
        "Keeper pin / M5 x 90 shaft",
        (-45., 45.),
        [D.pin_x(), 70.],
        2.5,
        "new_body",
        "retainer_screw",
    );
    a.bore_y(
        "Keeper pin / recessed socket head",
        (-50., -45.),
        [D.pin_x(), 70.],
        4.25,
        "join",
        "retainer_screw",
    );
    hardware.push((
        "retainer_screw".into(),
        "M5 x 90 socket screw / keeper cross-pin".into(),
        "jaw".into(),
    ));
    a.hex(
        "Keeper pin / trapped M5 nut",
        "xz",
        (40.3, 45.),
        [D.pin_x(), 70.],
        8.,
        ("new_body", "retainer_nut"),
    );
    a.bore_y(
        "Keeper pin nut / thread envelope",
        (40.2, 45.1),
        [D.pin_x(), 70.],
        2.5,
        "cut",
        "retainer_nut",
    );
    hardware.push((
        "retainer_nut".into(),
        "M5 nut / keeper cross-pin".into(),
        "jaw".into(),
    ));
    a.bore_x(
        "Thrust screw / M5 x 25 shaft",
        (D.head_front() - 30., D.head_front() - 5.),
        [0., D.axis],
        2.5,
        "new_body",
        "thrust_screw",
    );
    a.bore_x(
        "Thrust screw / recessed socket head",
        (D.head_front() - 5., D.head_front()),
        [0., D.axis],
        4.25,
        "join",
        "thrust_screw",
    );
    hardware.push((
        "thrust_screw".into(),
        "M5 x 25 socket screw / thrust fitting".into(),
        "screw".into(),
    ));
    a.hex(
        "Thrust screw / captive M5 nut",
        "yz",
        (D.stub_start() + 1.3, D.stub_start() + 6.),
        [0., D.axis],
        8.,
        ("new_body", "thrust_nut"),
    );
    a.bore_x(
        "Thrust nut / thread envelope",
        (D.stub_start() + 1.2, D.stub_start() + 6.1),
        [0., D.axis],
        2.5,
        "cut",
        "thrust_nut",
    );
    hardware.push((
        "thrust_nut".into(),
        "M5 captive nut / thrust fitting".into(),
        "screw".into(),
    ));
    for (i, x) in [70., 155.].into_iter().enumerate() {
        for (j, y) in [-68., 68.].into_iter().enumerate() {
            let stem = format!("mount_{i}_{j}");
            let location = format!(
                "{} {}",
                if i == 0 { "rear" } else { "front" },
                if j == 0 { "left" } else { "right" }
            );
            let c = [x, y];
            let bolt = format!("{stem}_bolt");
            a.cylinder_z(
                &format!("{bolt} / M6 x 45 shaft"),
                (D.deck + 1.6 - 45., D.deck + 1.6),
                c,
                3.,
                "new_body",
                &bolt,
            );
            a.cylinder_z(
                &format!("{bolt} / socket head"),
                (D.deck + 1.6, D.deck + 7.6),
                c,
                5.,
                "join",
                &bolt,
            );
            hardware.push((
                bolt,
                format!("M6 x 45 mounting bolt / {location}"),
                "frame".into(),
            ));
            for (label, z) in [("top_washer", D.deck), ("bottom_washer", -19.6)] {
                let part = format!("{stem}_{label}");
                a.cylinder_z(
                    &format!("{part} / broad washer"),
                    (z, z + 1.6),
                    c,
                    9.,
                    "new_body",
                    &part,
                );
                a.cylinder_z(
                    &format!("{part} / clearance"),
                    (z - 0.1, z + 1.7),
                    c,
                    3.3,
                    "cut",
                    &part,
                );
                hardware.push((
                    part,
                    format!(
                        "M6 broad {} washer / {location}",
                        if label == "top_washer" {
                            "top"
                        } else {
                            "bottom"
                        }
                    ),
                    "frame".into(),
                ));
            }
            let nut = format!("{stem}_nut");
            a.hex(
                &format!("{nut} / M6 envelope"),
                "xy",
                (-24.6, -19.6),
                c,
                10.,
                ("new_body", &nut),
            );
            a.cylinder_z(
                &format!("{nut} / thread envelope"),
                (-24.7, -19.5),
                c,
                3.,
                "cut",
                &nut,
            );
            hardware.push((nut, format!("M6 mounting nut / {location}"), "frame".into()));
        }
    }

    a.call("nut_thread", "solid/refine", "solid_hole", female);
    a.refresh_body("nut_thread", "nut");
    a.call(
        "male_thread",
        "solid/refine",
        "solid_external_thread",
        reference("male_thread_request", ""),
    );
    a.refresh_body("male_thread", "screw");
    a.bind("male_thread_feature_id",json!({"$select":{"from":reference("male_thread",""),"path":"/document/features","take":"last","pointer":"/id"}}));
    a.box_shape(
        "Screw / shallow continuous print flat",
        [grip_start - 1., -25., D.axis - 20.],
        [D.stub_end() + 1., 25., D.flat()],
        "cut",
        "screw",
    );
    a.show("screw");

    a.present_construction = false;

    let printed = [
        ("frame", "Reinforced frame and fixed jaw"),
        ("jaw", "100 mm moving jaw with captured slide"),
        ("nut", "24 x 4 threaded bridge with keyed feet"),
        ("screw", "24 x 4 lead-screw with integral rounded grip"),
        ("thrust", "Removable complete thrust fitting"),
        ("keeper", "Removable captured-jaw keeper"),
    ];
    let mut parts = Vec::new();
    for (part, name) in printed
        .iter()
        .map(|(p, n)| (p.to_string(), n.to_string()))
        .chain(hardware.iter().map(|(p, n, _)| (p.clone(), n.clone())))
    {
        a.component(&part, &name);
        let printable = printed.iter().any(|(p, _)| *p == part);
        if printable {
            a.call(&format!("{part}_material"),"document/appearance","set_body_appearance",json!({"body_id":a.body_id(&part),"preset_id":if part=="frame"||part=="jaw" {"bambu.petg.hf.black"}else{"bambu.petg.hf.white"}}));
        }
        a.bind(&format!("{part}_anchor_face"),json!({"$select":{"from":reference(&format!("{part}_body"),""),"path":"/faces","where":{"$or":[{"/plane/normal/0":1},{"/plane/normal/0":-1},{"/plane/normal/1":1},{"/plane/normal/1":-1},{"/plane/normal/2":1},{"/plane/normal/2":-1}]},"take":"first"}}));
        parts.push(json!({"id":part,"name":name,"body_id":a.body_id(&part),"occurrence_id":reference(&format!("{part}_occurrence"),""),"printable":printable,"print_plate":if printable{Some(part.clone())}else{None}}));
    }
    a.call(
        "ground_frame",
        "assembly/joints",
        "assembly_set_occurrence_grounded",
        json!({"occurrence_id":reference("frame_occurrence",""),"grounded":true}),
    );
    a.joint(
        "nut_in_housing",
        "rigid",
        "frame",
        "nut",
        [24., 0., D.axis],
        Value::Null,
    );
    a.joint(
        "keeper_in_jaw",
        "rigid",
        "jaw",
        "keeper",
        [D.pin_x(), 0., D.axis],
        Value::Null,
    );
    a.joint(
        "screw_drive",
        "screw",
        "frame",
        "screw",
        [24., 0., D.axis],
        json!({"min":0,"max":D.opening}),
    );
    a.joint(
        "thrust_on_screw",
        "rigid",
        "screw",
        "thrust",
        [D.stub_end(), 0., D.axis],
        Value::Null,
    );
    a.joint(
        "thrust_retention",
        "revolute",
        "thrust",
        "jaw",
        [D.pin_x(), 0., D.axis],
        Value::Null,
    );
    a.joint(
        "jaw_guide",
        "slider",
        "frame",
        "jaw",
        [D.pin_x(), 0., D.axis],
        json!({"min":0,"max":D.opening}),
    );
    for (part, _, parent) in &hardware {
        a.joint(
            &format!("{part}_installed"),
            "rigid",
            parent,
            part,
            [D.pin_x(), 0., D.axis],
            Value::Null,
        );
    }
    a.call("home_drive","assembly/joints","assembly_set_joint_motion",json!({"joint_id":reference("screw_drive","/id"),"angle_offset_deg":0,"linear_offset_mm":0}));
    a.call(
        "hide_finished_construction",
        "solid/reference",
        "construction_set_visibility",
        json!({"visible":false}),
    );
    a.note("Assembly is a physical sequence", "Load the bridge nuts from below. With the rear bridge removed, feed the captured jaw onto the open rail ends and park it 85 mm forward. Seat and bolt the keyed bridge. Screw the bare shaft through the bridge first, then load its M5 nut into the exposed stub. Slide on the complete thrust fitting from the front so its ledge supports the nut, then secure the axial M5. Slide the jaw rearward over the head, drop in the keeper and fit the transverse M5. The optional tabletop bolts and washers remain outside every moving part; clamp lands offer an alternate mounting route.");
    a.steps
        .push(json!({"view":"isometric","fit":true,"duration_ms":650}));

    vise_demo::run(&mut a);

    a.call(
        "assembled_visibility",
        "document/appearance",
        "project_visibility",
        json!({}),
    );
    let joint_names: Vec<_> = vec![
        "nut_in_housing".to_string(),
        "keeper_in_jaw".to_string(),
        "screw_drive".to_string(),
        "thrust_on_screw".to_string(),
        "thrust_retention".to_string(),
        "jaw_guide".to_string(),
    ]
    .into_iter()
    .chain(hardware.iter().map(|(p, _, _)| format!("{p}_installed")))
    .collect();
    for name in &joint_names {
        a.call(
            &format!("print_disable_{name}"),
            "assembly/joints",
            "assembly_set_joint_enabled",
            json!({"joint_id":reference(name,"/id"),"enabled":false}),
        );
    }
    let q = json!([
        0.,
        std::f64::consts::FRAC_1_SQRT_2,
        0.,
        std::f64::consts::FRAC_1_SQRT_2
    ]);
    let poses = [
        ("frame", json!([0., 80., 0.]), json!([0, 0, 0, 1])),
        ("jaw", json!([0., 55., D.home()]), q.clone()),
        ("nut", json!([0., 78., D.bridge_end]), q.clone()),
        (
            "screw",
            json!([D.opening + 25., 25., -D.flat()]),
            json!([0, 0, 0, 1]),
        ),
        ("thrust", json!([0., 16., D.head_front()]), q.clone()),
        (
            "keeper",
            json!([0., 30., D.keeper_front() - D.fit_clearance]),
            q,
        ),
    ];
    let mut plates = Vec::new();
    for (part, translation, rotation) in poses {
        a.note(&format!("Print orientation: {part}"),"This native plate exports only the named printable part. Use the intended material/profile, inspect the toolpath, and qualify short bridges, flat contact and fitted surfaces before treating the no-support orientation as proven.");
        set_pose(
            &mut a,
            &format!("print_pose_{part}"),
            part,
            translation,
            rotation,
        );
        let hidden: Vec<_> = a
            .bodies
            .keys()
            .filter(|id| id.as_str() != part)
            .map(|id| a.body_id(id))
            .collect();
        a.call(&format!("print_{part}_visibility"), "document/appearance", "project_set_visibility", json!({"hidden_body_ids":hidden,"hidden_sketch_names":reference("assembled_visibility","/hidden_sketch_names"),"hidden_datum_plane_ids":reference("assembled_visibility","/hidden_datum_plane_ids")}));
        a.show(part);
        a.call(
            &format!("print_{part}_model"),
            "document/files",
            "cad_project_model",
            json!({}),
        );
        a.call(
            &format!("print_{part}_solution"),
            "assembly/joints",
            "assembly_solution",
            json!({}),
        );
        a.call(
            &format!("print_{part}_3mf"),
            "document/export",
            "solid_export_3mf",
            json!({"slicer_target":"standard","body_ids":[a.body_id(part)]}),
        );
        plates.push(json!({"name":part,"body_ids":[a.body_id(part)],"model":reference(&format!("print_{part}_model"),""),"solution":reference(&format!("print_{part}_solution"),""),"export":reference(&format!("print_{part}_3mf"),"")}));
        set_pose(
            &mut a,
            &format!("restore_pose_{part}"),
            part,
            json!([0, 0, 0]),
            json!([0, 0, 0, 1]),
        );
        a.call(
            &format!("restore_{part}_visibility"),
            "document/appearance",
            "project_set_visibility",
            reference("assembled_visibility", ""),
        );

        a.steps
            .push(json!({"id":format!("restore_{part}_assembly_fit"),
            "view":"isometric","fit":true,"duration_ms":650}));
    }
    for name in &joint_names {
        a.call(
            &format!("restore_enable_{name}"),
            "assembly/joints",
            "assembly_set_joint_enabled",
            json!({"joint_id":reference(name,"/id"),"enabled":true}),
        );
    }
    a.call("restore_home_drive","assembly/joints","assembly_set_joint_motion",json!({"joint_id":reference("screw_drive","/id"),"angle_offset_deg":0,"linear_offset_mm":0}));
    let drawing_exports = vise_drawings::add(&mut a, &parts);
    a.steps
        .push(json!({"view":"isometric","fit":true,"duration_ms":650}));
    let mut exports = Map::new();
    for id in [
        "final_model",
        "final_scene",
        "final_sketches",
        "final_assembly",
        "final_solution",
        "final_interference",
        "male_thread_request",
        "male_thread_feature_id",
    ] {
        exports.insert(id.into(), reference(id, ""));
    }
    for part in &parts {
        let id = part["id"].as_str().unwrap();
        exports.insert(format!("{id}_body_id"), a.body_id(id));
        exports.insert(
            format!("{id}_occurrence_id"),
            reference(&format!("{id}_occurrence"), ""),
        );
    }
    for (alias, id) in [
        ("screw_joint_id", "screw_drive"),
        ("jaw_joint_id", "jaw_guide"),
        ("retention_joint_id", "thrust_retention"),
    ] {
        exports.insert(alias.into(), reference(id, "/id"));
    }
    for id in drawing_exports {
        exports.insert(id.clone(), reference(&id, ""));
    }
    exports.insert("parts".into(), json!(parts));
    exports.insert("print_plates".into(), json!(plates));
    exports.insert("nut_support".into(), json!({"ledge_width_mm":8.,"channel_width_mm":8.5,"ledge_top_z_mm":D.nut_support_top(),"channel_roof_z_mm":D.nut_support_top()+0.2,"nominal_nut_sag_mm":0.1,"retains_axial_pocket_wall":true}));
    exports.insert("design_inputs".into(),json!({"jaw_width_mm":D.jaw_width,"carriage_width_mm":D.carriage_width,"initial_opening_mm":D.opening,"allowed_travel_mm":D.opening,"lead_mm":D.lead,"nominal_thread_mm":D.thread_diameter,"thread_radial_depth_mm":D.thread_depth,"thread_corner_radius_mm":D.thread_corner,"female_radial_relief_mm":D.radial_relief,"female_axial_relief_mm":D.axial_relief,"thread_engagement_mm":D.bridge_end-D.bridge_start,"flat_axis_z_mm":D.flat(),"flat_below_axis_mm":D.flat_below_axis,"screw_axis_z_mm":D.axis,"deck_mm":D.deck,"fixed_face_x_mm":D.fixed_face,"home_face_x_mm":D.home(),"carriage_rear_x_mm":D.rear(),"carriage_length_mm":D.carriage_length,"jaw_top_z_mm":D.jaw_top,"guide_centers_y_mm":[-D.guide_center,D.guide_center],"guide_base_width_mm":D.guide_base,"guide_head_width_mm":D.guide_head,"guide_height_mm":D.guide_height,"guide_clearance_mm":D.guide_clearance,"guide_top_z_mm":D.deck+D.guide_height,"guide_end_x_mm":D.fixed_face-D.root_radius,"bridge_start_x_mm":D.bridge_start,"bridge_end_x_mm":D.bridge_end,"bridge_socket_floor_z_mm":D.deck-D.bridge_key_depth,"bridge_key_bottom_z_mm":D.deck-D.bridge_key_depth+D.fit_clearance,"stub_radius_mm":9.,"stub_start_x_mm":D.stub_start(),"stub_end_x_mm":D.stub_end(),"head_front_x_mm":D.head_front(),"keeper_pin_x_mm":D.pin_x(),"keeper_pin_z_mm":70.,"keeper_back_x_mm":D.keeper_back(),"keeper_front_x_mm":D.keeper_front(),"keeper_axial_seating_mm":D.keeper_seat_gap,"keeper_ear_inner_y_mm":D.keeper_ear_inner,"keeper_pin_slot_extension_mm":D.keeper_pin_extension,"design_load_case_N":D.design_force,"input_torque_Nm":1.,"assumed_overall_efficiency":0.2,"illustrative_axial_force_N":2.*std::f64::consts::PI*0.2/(D.lead/1000.),"contact_patch_mm2":600.,"physical_load_rating":null,"mount_board_thickness_mm":18.,"mount_hardware_optional":true}));
    exports.insert("assembly_paths".into(),json!({"jaw_rear_entry_translation_x_mm":-D.home()-5.,"jaw_service_translation_x_mm":85.,"bridge_install_axis":[0,0,-1],"thrust_install_axis":[-1,0,0],"thrust_front_approach_mm":35.,"keeper_install_axis":[0,0,-1],"keeper_pin_install_axis":[0,1,0],"shaft_install_axis":[1,0,0],"shaft_install_requires_coupled_thread_rotation":true,"sleeve_requires_axial_bolt_before_jaw_returns":true,"order":["load bridge nuts from underneath","rear-feed jaw with bridge absent","park jaw at plus 85 mm","lower keyed bridge and install two M6 bolts","turn bare shaft through bridge, then load M5 nut into exposed stub","slide complete thrust fitting to trap and support nut, then install axial M5","slide jaw rearward over secured fitting","lower keeper and insert transverse M5","install outboard mounting bolts or use clamp lands"]}));
    let checks = final_checks(&a);
    write_script(
        "examples/scripts/d-screw-vise.limo.jsonc",
        "100 mm captured-slide printed vise",
        a,
        exports,
        checks,
    );
    author_fit_coupon();
}

fn author_fit_coupon() {
    let mut a = Author::fresh();
    a.note("Qualify both printed interfaces", "Four specimens retain the exact 24 x 4 rounded thread, full 28 mm bridge engagement and actual captured-dovetail profile. Print in the supplied orientations. Record sliding force, turning torque, play, material/profile and wear; the designed no-support poses still require toolpath and physical checks.");
    a.cylinder_x(
        "Thread coupon / 40 mm male",
        0.,
        40.,
        D.thread_diameter / 2.,
        "new_body",
        "screw",
    );
    a.bind(
        "coupon_male_face",
        select(
            a.bodies["screw"].clone(),
            "/faces",
            json!({"/cylinder/radius":D.thread_diameter/2.}),
            "",
        ),
    );
    a.box_shape(
        "Thread coupon / full bridge engagement",
        [60., -20., 0.],
        [100., 20., D.bridge_end - D.bridge_start],
        "new_body",
        "nut",
    );
    a.bind(
        "coupon_female_face",
        select(
            a.bodies["nut"].clone(),
            "/faces",
            json!({"/plane/normal/2":1}),
            "",
        ),
    );
    a.box_shape(
        "Guide coupon / supported rail base",
        [0., -20., D.deck - 4.],
        [40., 20., D.deck],
        "new_body",
        "guide_male",
    );
    a.prism_x(
        "Guide coupon / production dovetail",
        0.,
        40.,
        &guide_rail_profile(0.),
        "join",
        "guide_male",
    );
    a.box_shape(
        "Guide coupon / production socket stock",
        [60., -22., D.deck],
        [100., 22., D.deck + 20.],
        "new_body",
        "guide_female",
    );
    a.prism_x(
        "Guide coupon / production running clearance",
        59.,
        101.,
        &guide_profile(0., D.guide_clearance),
        "cut",
        "guide_female",
    );
    a.call("coupon_female_thread","solid/refine","solid_hole",json!({"body_id":a.body_id("nut"),"face_id":reference("coupon_female_face","/id"),"position":{"$project":{"point":[80.,0.,D.bridge_end-D.bridge_start],"basis":reference("coupon_female_face","/plane")}},"diameter":D.thread_diameter-2.*D.thread_depth,"extent":{"type":"through_all"},"style":"simple","flip":false,"thread":thread(None)}));
    a.refresh_body("coupon_female_thread", "nut");
    a.call("coupon_male_thread","solid/refine","solid_external_thread",json!({"body_id":a.body_id("screw"),"face_id":reference("coupon_male_face","/id"),"flip":false,"thread":thread(None)}));
    a.refresh_body("coupon_male_thread", "screw");
    a.box_shape(
        "Thread coupon / production shallow flat",
        [-1., -13., D.axis - 13.],
        [41., 13., D.flat()],
        "cut",
        "screw",
    );
    let mut parts = Vec::new();
    for (part, name) in [
        ("screw", "Production shallow-D thread coupon"),
        ("nut", "Full-engagement female coupon"),
        ("guide_male", "Production captured rail coupon"),
        ("guide_female", "Production captured socket coupon"),
    ] {
        a.component(part, name);
        a.bind(&format!("{part}_anchor_face"),json!({"$select":{"from":reference(&format!("{part}_body"),""),"path":"/faces","where":{"$or":[{"/plane/normal/0":1},{"/plane/normal/0":-1},{"/plane/normal/1":1},{"/plane/normal/1":-1},{"/plane/normal/2":1},{"/plane/normal/2":-1}]},"take":"first"}}));
        parts.push(json!({"id":part,"name":name,"body_id":a.body_id(part),"occurrence_id":reference(&format!("{part}_occurrence"),""),"printable":true,"print_plate":"fit"}));
    }
    set_pose(
        &mut a,
        "coupon_screw_print",
        "screw",
        json!([0., 25., -D.flat()]),
        json!([0, 0, 0, 1]),
    );
    set_pose(
        &mut a,
        "coupon_nut_print",
        "nut",
        json!([0., 30., 0.]),
        json!([0, 0, 0, 1]),
    );
    set_pose(
        &mut a,
        "coupon_rail_print",
        "guide_male",
        json!([0., 75., 4. - D.deck]),
        json!([0, 0, 0, 1]),
    );
    set_pose(
        &mut a,
        "coupon_socket_print",
        "guide_female",
        json!([120., 140., 100.]),
        json!([
            0.,
            std::f64::consts::FRAC_1_SQRT_2,
            0.,
            std::f64::consts::FRAC_1_SQRT_2
        ]),
    );
    a.call(
        "ground_coupon_plate",
        "assembly/joints",
        "assembly_set_occurrence_grounded",
        json!({"occurrence_id":reference("screw_occurrence",""),"grounded":true}),
    );

    for (part, origin, primary) in [
        ("nut", [0., -5., -D.flat()], [1., 0., 0.]),
        (
            "guide_male",
            [0., -50., D.deck - 4. - D.flat()],
            [1., 0., 0.],
        ),
        (
            "guide_female",
            [100. + D.flat(), -115., -120.],
            [0., 0., 1.],
        ),
    ] {
        let mut connector = a.connector(part, origin);
        connector["frame"]["primary_axis"] = json!(primary);
        a.call(&format!("coupon_{part}_fixed"), "assembly/joints", "assembly_create_joint", json!({
            "name":format!("Print layout / {part}"),"kind":"rigid","flipped":true,
            "connector_a":a.connector("screw", [0.,0.,0.]),"connector_b":connector,
            "angle_offset_deg":0,"linear_offset_mm":0,
            "advanced":{"connector_a_occurrence_id":reference("screw_occurrence",""),"connector_b_occurrence_id":reference(&format!("{part}_occurrence"),"")}
        }));
    }
    a.call("print_3mf","document/export","solid_export_3mf",json!({"slicer_target":"standard","body_ids":[a.body_id("screw"),a.body_id("nut"),a.body_id("guide_male"),a.body_id("guide_female")]}));
    a.call(
        "hide_coupon_construction",
        "solid/reference",
        "construction_set_visibility",
        json!({"visible":false}),
    );
    a.steps
        .push(json!({"view":"isometric","fit":true,"duration_ms":650}));
    let mut exports = Map::new();
    for id in [
        "final_model",
        "final_scene",
        "final_sketches",
        "final_assembly",
        "final_solution",
        "final_interference",
        "print_3mf",
    ] {
        exports.insert(id.into(), reference(id, ""));
    }
    for p in ["screw", "nut", "guide_male", "guide_female"] {
        exports.insert(format!("{p}_body_id"), a.body_id(p));
        exports.insert(
            format!("{p}_occurrence_id"),
            reference(&format!("{p}_occurrence"), ""),
        );
    }
    exports.insert("parts".into(), json!(parts));
    exports.insert("coupon_length_mm".into(), json!(40.));
    exports.insert(
        "female_engagement_mm".into(),
        json!(D.bridge_end - D.bridge_start),
    );
    exports.insert("nominal_male_mm".into(), json!(D.thread_diameter));
    exports.insert(
        "nominal_female_mm".into(),
        json!(D.thread_diameter + 2. * D.radial_relief),
    );
    exports.insert("pitch_mm".into(), json!(D.lead));
    exports.insert(
        "rounded_profile".into(),
        thread(None)["rounded_profile"].clone(),
    );
    exports.insert("guide_profile".into(),json!({"base_width_mm":D.guide_base,"head_width_mm":D.guide_head,"height_mm":D.guide_height,"clearance_mm":D.guide_clearance,"engagement_mm":40.}));
    let checks = final_checks(&a);
    write_script(
        "examples/scripts/d-screw-vise-fit.limo.jsonc",
        "100 mm vise / thread and captured-slide fit coupons",
        a,
        exports,
        checks,
    );
}
