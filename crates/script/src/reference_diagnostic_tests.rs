//! Parser error hints stay bounded without restricting valid binding names.
use super::*;

#[test]
fn unknown_unicode_reference_keeps_typo_help_without_slicing_a_codepoint() {
    let name = "abc🧰";
    let source = json!({"version":1,"name":"Unicode binding","steps":[
        {"let":{(name):0}},
        {"assert":{"$ref":format!("{name}d")},"equals":0}
    ]});
    let error = Script::parse(&source.to_string()).unwrap_err();
    assert!(error.contains("before it is defined"), "{error}");
    assert!(
        error.contains(&format!("similar earlier results: {name}")),
        "{error}"
    );
}

#[test]
fn valid_long_bindings_work_but_unknown_long_names_skip_fuzzy_diagnostics() {
    let name = "a".repeat(129);
    let mut source = json!({"version":1,"name":"Long binding","steps":[
        {"let":{(name.clone()):0}},
        {"assert":{"$ref":name},"equals":0}
    ]});
    let script = Script::parse(&source.to_string()).unwrap();
    let result = run(
        &script,
        |_, _| Err("No host call was expected".into()),
        RunOptions::default(),
    )
    .unwrap();
    assert_eq!(result["steps_completed"], 2);
    source["steps"][1]["assert"]["$ref"] = json!(format!("{name}x"));
    let error = Script::parse(&source.to_string()).unwrap_err();
    assert!(error.contains("before it is defined"), "{error}");
    assert!(!error.contains("similar earlier results"), "{error}");
}

#[test]
fn short_reference_diagnostics_never_copy_an_oversized_candidate() {
    let available = BTreeSet::from(["edge_1".to_owned(), format!("edge{}", "x".repeat(129))]);
    assert_eq!(similar_names("edge", &available), ["edge_1"]);
}
