//! Associative native part sheets, assembly views and the actual procurement BOM.
use super::*;

impl Author {
    pub(super) fn part_drawing(&mut self, name: &str, height: f64, diameters: &[f64], note: &str) {
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
            (
                "top",
                [0., 0., 1.],
                [0., 1., 0.],
                [
                    110.,
                    if matches!(name, "guard" | "guard_lid") {
                        110.
                    } else {
                        90.
                    },
                ],
            ),
            (
                "front",
                [0., -1., 0.],
                [0., 0., 1.],
                [290., if name == "shaft" { 120. } else { 105. }],
            ),
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

            let (leader_angle, offset) = match (name, i) {
                ("stage", 1) => (180., 22.),
                ("tower", 1) => (110., 22.),
                ("pinion", 1) => (110., 26.),
                _ => (30. + i as f64 * 80., 12. + i as f64 * 3.),
            };
            self.call(&id,"drawing/dimensions","drawing_add_radial_dimension",json!({"sheet_id":r(&sheet_id),"view_id":r(&views[0].0),"feature":{"body_id":body_ref(name),"edge_id":at(&circle,"/edge_id"),"edge_key":at(&circle,"/edge_key"),"fallback_center":at(&circle,"/center_model"),"fallback_normal":at(&circle,"/normal_model"),"fallback_radius":at(&circle,"/radius"),"closed":at(&circle,"/closed")},"mode":"diameter","leader_angle_deg":leader_angle,"offset":offset,"precision":2}));
        }
        let make_anchor = |z: f64| {
            let source = r(&views[1].1);
            let pred = if z == 0. {
                let x = match name {
                    "tower" => json!(23.7),
                    "bearing_coupon" => json!(11.15),
                    "motor_mount" => json!(13.5),
                    "motor_bracket" => json!(13.5),
                    _ => at(&views[1].1, "/bounds/2"),
                };
                json!({"/model_point/2":z,"/point/0":x})
            } else {
                json!({"/model_point/2":z})
            };
            json!({"body_id":body_ref(name),"edge_id":select(source.clone(),"/anchors",pred.clone(),"first","/edge_id"),"edge_key":select(source.clone(),"/anchors",pred.clone(),"first","/edge_key"),"endpoint":select(source.clone(),"/anchors",pred.clone(),"first","/endpoint"),"fallback_point":select(source,"/anchors",pred,"first","/model_point")})
        };
        let id = self.uid("height");
        self.call(&id,"drawing/dimensions","drawing_add_linear_dimension",json!({"sheet_id":r(&sheet_id),"view_id":r(&views[1].0),"first":make_anchor(0.),"second":make_anchor(height),"mode":"vertical","offset":match name {"bearing_coupon"=>24.,"motor_mount"=>26.,_=>14.},"precision":2}));
        if name == "base" {
            let corner = |x: f64, y: f64| {
                let pred = json!({"/model_point/0":x,"/model_point/1":y,"/model_point/2":0.});
                json!({"body_id":body_ref(name),"edge_id":select(r(&views[0].1),"/anchors",pred.clone(),"first","/edge_id"),"edge_key":select(r(&views[0].1),"/anchors",pred.clone(),"first","/edge_key"),"endpoint":select(r(&views[0].1),"/anchors",pred.clone(),"first","/endpoint"),"fallback_point":select(r(&views[0].1),"/anchors",pred,"first","/model_point")})
            };
            for (label, first, second, mode) in [
                ("width", corner(-75., -66.), corner(95., -66.), "horizontal"),
                ("depth", corner(91., -70.), corner(91., 70.), "vertical"),
            ] {
                self.call(&format!("base_{label}_dimension"),"drawing/dimensions","drawing_add_linear_dimension",json!({"sheet_id":r(&sheet_id),"view_id":r(&views[0].0),"first":first,"second":second,"mode":mode,"offset":14.,"precision":2}));
            }
            let centre = |x: f64, y: f64, radius: f64| {
                let pred = json!({"/center_model/0":x,"/center_model/1":y,"/radius":radius});
                json!({"body_id":body_ref(name),"edge_id":select(r(&views[0].1),"/circles",pred.clone(),"first","/edge_id"),"edge_key":select(r(&views[0].1),"/circles",pred.clone(),"first","/edge_key"),"endpoint":"start","circle_center":true,"fallback_point":select(r(&views[0].1),"/circles",pred,"first","/center_model")})
            };
            self.call("base_bracket_mount_offset","drawing/dimensions","drawing_add_linear_dimension",json!({"sheet_id":r(&sheet_id),"view_id":r(&views[0].0),"first":centre(0.,0.,9.),"second":centre(D.gear_spacing+10.,34.,1.7),"mode":"horizontal","offset":35.,"precision":2}));
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
    pub(super) fn assembly_drawing(&mut self) {
        self.call("assembly_sheet","drawing/sheet","drawing_create_sheet",json!({"name":"Turbine assembly","format":"a3","orientation":"landscape","title_block":{"title":"Savonius experiment / assembly","drawing_number":"TUR-000","revision":"A-candidate"}}));
        self.bind(
            "assembly_sheet_id",
            select(r("assembly_sheet"), "/sheets", json!({}), "last", "/id"),
        );
        for (kind, direction, up, position, scale) in [
            ("front", [0., -1., 0.], [0., 0., 1.], [105., 125.], 0.7),
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
                self.call("assembly_height","drawing/dimensions","drawing_add_linear_dimension",json!({"sheet_id":r("assembly_sheet_id"),"view_id":r(&view_id),"first":anchor("base",json!({"/model_point/0":95.,"/model_point/2":0.})),"second":anchor("shaft",json!({"/model_point/2":D.shaft_top()})),"mode":"vertical","offset":14.,"precision":2}));
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
        let items=self.parts.iter().filter(|p| !matches!(p["id"].as_str(),Some("bearing_inner"|"bearing_shield"|"motor_shaft"))).enumerate().map(|(i,p)|json!({"item_number":(i+1).to_string(),"body_id":p["body_id"],"part_number":format!("TUR-{}",p["id"].as_str().unwrap()),"description":if p["id"]=="bearing" {json!("Complete 608ZZ bearing, 8 x22 x7; separate race and shield envelopes shown")} else {p["name"].clone()},"quantity":p.get("quantity").cloned().unwrap_or(json!(1)),"material":p["material"]})).collect::<Vec<_>>();
        self.call(
            "turbine_bom",
            "drawing/sheet",
            "drawing_set_bom",
            json!({"sheet_id":r("bom_sheet_id"),"position":[16.,22.],"items":items}),
        );
        self.call("procurement_note","drawing/annotate","drawing_add_note",json!({"sheet_id":r("bom_sheet_id"),"position":[22.,207.],"text":wrapped("Purchased fasteners have native installed envelopes. The two complete 608ZZ bearings include the separately modeled inner rings and shields; the generator includes its shaft. Verify actual head, nut and shaft dimensions before purchase/printing. PETG baseline. Fits, motor projection, hub slip, rotor startup, bearing clamp load and guard access require physical qualification. Ages8-12 with adult guidance; age5 only with closer hands-on adult guidance.")}));
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
}
