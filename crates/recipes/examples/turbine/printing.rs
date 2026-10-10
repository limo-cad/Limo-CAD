//! Native print handoffs retain editable history and restore the live assembly.
use super::*;

impl Author {
    pub(super) fn print_plates(&mut self) -> Vec<Value> {
        self.note("One editable print plate per part", "Each plate shows one actual occurrence in its bed pose and exports that occurrence as 3MF. Print the stage plate twice. Hardware and other components remain editable but hidden. The complete assembly is restored before drawings and final checks; qualify toolpaths and measured fits before use.");
        self.call(
            "plate_home_model",
            "document/files",
            "cad_project_model",
            json!({}),
        );
        self.call(
            "plate_home_visibility",
            "document/appearance",
            "project_visibility",
            json!({}),
        );
        self.call(
            "plate_home_assembly",
            "assembly/joints",
            "assembly_document",
            json!({}),
        );
        let joint_names = self.joint_names.clone();
        for name in &joint_names {
            self.call(
                &format!("plate_disable_{name}"),
                "assembly/joints",
                "assembly_set_joint_enabled",
                json!({"joint_id":at(name,"/id"),"enabled":false}),
            );
        }

        let occurrences = self.occurrences.clone();
        for (name, id) in &occurrences {
            self.call(
                &format!("plate_park_{name}"),
                "assembly/joints",
                "assembly_update_occurrence",
                json!({"occurrence":{"id":id,"visible":false}}),
            );
        }
        let printed = self
            .parts
            .iter()
            .filter(|part| part["printable"] == true)
            .map(|p| p["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let mut plates = vec![];
        for name in printed {
            let (translation, rotation) = self.poses[&name];
            let print_pose = json!({"translation":[110.,110.,0.],"rotation":[0.,0.,0.,1.]});
            let part = self
                .parts
                .iter_mut()
                .find(|part| part["id"] == name)
                .unwrap();
            part["print_pose"] = print_pose.clone();
            part["print_plate"] = json!(name);
            self.call(
                &format!("plate_ground_{name}"),
                "assembly/joints",
                "assembly_set_occurrence_grounded",
                json!({"occurrence_id":occ_ref(&name),"grounded":true}),
            );
            self.call(
                &format!("plate_place_{name}"),
                "assembly/joints",
                "assembly_update_occurrence",
                json!({"occurrence":{"id":occ_ref(&name),"visible":true,"local_pose":print_pose}}),
            );
            let hidden = self
                .parts
                .iter()
                .filter(|part| part["id"] != name)
                .map(|part| part["body_id"].clone())
                .collect::<Vec<_>>();
            self.call(&format!("plate_visibility_{name}"),"document/appearance","project_set_visibility",json!({"hidden_body_ids":hidden,"hidden_sketch_names":at("plate_home_visibility","/hidden_sketch_names"),"hidden_datum_plane_ids":at("plate_home_visibility","/hidden_datum_plane_ids")}));
            self.steps.push(json!({"view":"isometric","fit":true,"component_id":at(&format!("{name}_component"),"/id"),"duration_ms":450}));
            self.call(
                &format!("plate_model_{name}"),
                "document/files",
                "cad_project_model",
                json!({}),
            );
            self.call(
                &format!("plate_solution_{name}"),
                "assembly/joints",
                "assembly_solution",
                json!({}),
            );
            self.steps.push(
                json!({"assert":at(&format!("plate_solution_{name}"),"/solved"),"equals":true}),
            );
            self.steps.push(
                json!({"assert":{"$count":select(r(&format!("plate_solution_{name}")),"/instance_body_poses",json!({"/visible":true}),"all","")},"equals":1}),
            );
            self.call(
                &format!("plate_export_{name}"),
                "document/export",
                "solid_export_3mf",
                json!({"slicer_target":"standard","scope":"assembly","body_ids":[body_ref(&name)]}),
            );
            plates.push(json!({"name":name,"part_id":name,"body_id":body_ref(&name),"body_ids":[body_ref(&name)],"occurrence_id":occ_ref(&name),"model":r(&format!("plate_model_{name}")),"solution":r(&format!("plate_solution_{name}")),"export":r(&format!("plate_export_{name}"))}));
            self.call(&format!("plate_restore_pose_{name}"),"assembly/joints","assembly_update_occurrence",json!({"occurrence":{"id":occ_ref(&name),"visible":false,"local_pose":{"translation":translation,"rotation":rotation}}}));
        }
        self.call(
            "plate_restore_base_ground",
            "assembly/joints",
            "assembly_set_occurrence_grounded",
            json!({"occurrence_id":occ_ref("base"),"grounded":true}),
        );
        for (name, id) in &occurrences {
            let saved = select(
                r("plate_home_assembly"),
                "/component_structure/occurrences",
                json!({"/id":id}),
                "one",
                "",
            );
            self.call(
                &format!("plate_restore_occurrence_{name}"),
                "assembly/joints",
                "assembly_update_occurrence",
                json!({"occurrence":saved}),
            );
        }
        self.call(
            "plate_restore_visibility",
            "document/appearance",
            "project_set_visibility",
            r("plate_home_visibility"),
        );

        self.steps.push(json!({"id":"plate_restore_assembly_fit",
            "view":"isometric","fit":true,"duration_ms":650}));
        for name in &joint_names {
            self.call(
                &format!("plate_enable_{name}"),
                "assembly/joints",
                "assembly_set_joint_enabled",
                json!({"joint_id":at(name,"/id"),"enabled":true}),
            );
        }
        self.call(
            "plates_restored_model",
            "document/files",
            "cad_project_model",
            json!({}),
        );
        self.steps
            .push(json!({"assert":r("plates_restored_model"),"equals":r("plate_home_model")}));
        plates
    }
}
