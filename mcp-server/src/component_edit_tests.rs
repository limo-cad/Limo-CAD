use super::*;

#[test]
fn shared_occurrence_dimension_edits_preserve_joints_through_the_product_interface() {
    let mut server = CadServer::new().unwrap();
    server
        .call_tool(
            "cad_interface",
            json!({"action":"script","recipe":"repeated-bracket-assembly","mode":"fast"}),
        )
        .unwrap();
    let assembly = server.call_tool("assembly_document", json!({})).unwrap();
    let occurrence = assembly["component_structure"]["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|occurrence| occurrence["name"] == "angle-bracket-2")
        .unwrap()["id"]
        .clone();
    let edited = server
        .call_tool(
            "sketch_edit",
            json!({"name":"Bracket / dimensioned L section","occurrence_id":occurrence}),
        )
        .unwrap();
    assert_eq!(edited["edit_occurrence_id"], occurrence);
    let dimension = edited["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|dimension| {
            dimension["mode"] == "driving" && dimension["value"].as_f64().unwrap() > 10.
        })
        .unwrap();
    let constraint = dimension["constraint_id"].clone();
    let original = dimension["value"].as_f64().unwrap();
    let changed = server
        .call_tool(
            "sketch_edit_dimension",
            json!({"constraint_id":constraint,"text":format!("{original} + 2")}),
        )
        .unwrap();
    assert_eq!(changed["sketch"]["edit_occurrence_id"], occurrence);
    server.call_tool("sketch_finish", json!({})).unwrap();
    server.call_tool("solid_recompute", json!({})).unwrap();
    let solution = server.call_tool("assembly_solution", json!({})).unwrap();
    assert_eq!(solution["solved"], true, "{solution}");
    assert_eq!(solution["diagnostics"], json!([]), "{solution}");
    let final_assembly = server.call_tool("assembly_document", json!({})).unwrap();
    assert_eq!(final_assembly["joints"].as_array().unwrap().len(), 3);
    assert_eq!(
        final_assembly["component_structure"]["occurrences"],
        assembly["component_structure"]["occurrences"]
    );
    let saved = server.call_tool("cad_project_model", json!({})).unwrap();
    let mut reopened = CadServer::new().unwrap();
    reopened
        .call_tool("cad_load_project_model", json!({"model_json":saved}))
        .unwrap();
    assert_eq!(
        reopened.call_tool("assembly_solution", json!({})).unwrap(),
        solution
    );
    let finished = reopened.call_tool("sketch_finished", json!({})).unwrap();
    let local = finished
        .as_array()
        .unwrap()
        .iter()
        .find(|sketch| sketch["name"] == "Bracket / dimensioned L section")
        .unwrap();
    let local_basis: limo_cad_core::PlaneBasis =
        serde_json::from_value(local["basis"].clone()).unwrap();
    let pose = solution["instance_body_poses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pose| pose["occurrence_id"] == occurrence)
        .unwrap();
    let placement: limo_cad_sketch::AssemblyTransformDto = serde_json::from_value(
        json!({"translation":pose["translation"],"rotation":pose["rotation"]}),
    )
    .unwrap();
    let reentered = reopened
        .call_tool(
            "sketch_edit",
            json!({"name":"Bracket / dimensioned L section","occurrence_id":occurrence}),
        )
        .unwrap();
    let displayed_basis: limo_cad_core::PlaneBasis =
        serde_json::from_value(reentered["basis"].clone()).unwrap();
    for (actual, expected) in displayed_basis
        .to_3d([7., 4.])
        .into_iter()
        .zip(placement.transform_point(local_basis.to_3d([7., 4.])))
    {
        assert!((actual - expected).abs() < 1e-9);
    }
    let dimension = reentered["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|dimension| dimension["constraint_id"] == constraint)
        .unwrap();
    assert!((dimension["value"].as_f64().unwrap() - (original + 2.)).abs() < 1e-6);
}
