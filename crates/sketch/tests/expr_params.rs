//! Expression engine + parameter table tests (D9): grammar, precedence,
//! functions, d-references, chained dependencies, cycles, unknown names,
//! renames, division by zero.

use limo_cad_sketch::{
    eval_expression, parse_expression, referenced_idents, ExprError, ParamKind, ParamTable,
};

fn eval_ok(expr: &str) -> f64 {
    eval_expression(expr, &mut |name| {
        Err(ExprError::UnknownParameter(name.to_string()))
    })
    .unwrap()
}

fn eval_err(expr: &str) -> ExprError {
    eval_expression(expr, &mut |name| {
        Err(ExprError::UnknownParameter(name.to_string()))
    })
    .unwrap_err()
}

const EPS: f64 = 1e-12;

#[test]
fn arithmetic_and_precedence() {
    assert_eq!(eval_ok("1+2*3"), 7.0);
    assert_eq!(eval_ok("(1+2)*3"), 9.0);
    assert_eq!(eval_ok("10/4"), 2.5);
    assert_eq!(eval_ok("2^3^2"), 512.0);
    assert_eq!(eval_ok("-2^2"), -4.0);
    assert_eq!(eval_ok("+45"), 45.0);
    assert_eq!(eval_ok("=+2^2"), 4.0);
    assert_eq!(eval_ok("2*-+3"), -6.0);
    assert_eq!(eval_ok("-(2+3)"), -5.0);
    assert_eq!(eval_ok("=50/2"), 25.0);
    assert_eq!(eval_ok("1 - -3"), 4.0);
    assert_eq!(eval_ok(".5 * 4"), 2.0);
}

#[test]
fn functions_use_degrees_for_trig() {
    assert!((eval_ok("sin(30)") - 0.5).abs() < EPS);
    assert!((eval_ok("cos(60)") - 0.5).abs() < EPS);
    assert!((eval_ok("tan(45)") - 1.0).abs() < 1e-9);
    assert_eq!(eval_ok("sqrt(144)"), 12.0);
    assert_eq!(eval_ok("abs(-7)"), 7.0);
    assert_eq!(eval_ok("min(3, 9)"), 3.0);
    assert_eq!(eval_ok("max(3, 9)"), 9.0);
    assert_eq!(eval_ok("floor(2.7)"), 2.0);
    assert_eq!(eval_ok("ceil(2.1)"), 3.0);
    assert_eq!(eval_ok("sqrt(3^2+4^2)"), 5.0);
}

#[test]
fn parse_errors_are_clear() {
    assert!(matches!(eval_err("1 +"), ExprError::UnexpectedToken(_)));
    assert!(matches!(eval_err("foo(2)"), ExprError::UnexpectedToken(_)));
    assert!(matches!(
        eval_err("sqrt(-1)"),
        ExprError::UnexpectedToken(_)
    ));
    assert!(matches!(eval_err("1 2"), ExprError::UnexpectedToken(_)));
    assert!(matches!(
        eval_err("sin(1,2)"),
        ExprError::UnexpectedToken(_)
    ));
    assert_eq!(eval_err("1/0"), ExprError::DivisionByZero);
    assert_eq!(
        eval_err("d99 + 1"),
        ExprError::UnknownParameter("d99".to_string())
    );
}

#[test]
fn referenced_idents_found() {
    let ast = parse_expression("=d1*2 + sqrt(d2) - d1").unwrap();
    assert_eq!(
        referenced_idents(&ast),
        vec!["d1".to_string(), "d2".to_string()]
    );
}

#[test]
fn auto_names_and_literals() {
    let mut t = ParamTable::new();
    let d1 = t.add(ParamKind::Length, None, 50.0).unwrap();
    let d2 = t.add(ParamKind::Length, None, 30.0).unwrap();
    assert_eq!(t.get(d1).unwrap().name, "d1");
    assert_eq!(t.get(d2).unwrap().name, "d2");
    assert_eq!(t.get(d1).unwrap().value, 50.0);
}

#[test]
fn formula_with_d_references_evaluates() {
    let mut t = ParamTable::new();
    t.add(ParamKind::Length, None, 50.0).unwrap();
    let d2 = t.add(ParamKind::Length, Some("=d1/2"), 0.0).unwrap();
    assert_eq!(t.get(d2).unwrap().value, 25.0);
    let d3 = t
        .add(ParamKind::Length, Some("=d2*2 + sqrt(d1)"), 0.0)
        .unwrap();
    assert!((t.get(d3).unwrap().value - (50.0 + 50f64.sqrt())).abs() < 1e-9);
}

#[test]
fn editing_a_parameter_reevaluates_dependents() {
    let mut t = ParamTable::new();
    let d1 = t.add(ParamKind::Length, None, 50.0).unwrap();
    let d2 = t.add(ParamKind::Length, Some("=d1/2"), 0.0).unwrap();
    t.set_expression(d1, "60").unwrap();
    assert_eq!(t.get(d1).unwrap().value, 60.0);
    assert_eq!(t.get(d2).unwrap().value, 30.0);

    t.set_expression(d2, "=d1*2").unwrap();
    assert_eq!(t.get(d2).unwrap().value, 120.0);
}

#[test]
fn cycles_are_detected_with_a_clear_path() {
    let mut t = ParamTable::new();
    let d1 = t.add(ParamKind::Length, None, 50.0).unwrap();
    let d2 = t.add(ParamKind::Length, Some("=d1"), 0.0).unwrap();
    t.set_expression(d1, "=d2").unwrap_err();
    let err = t.set_expression(d1, "=d2").unwrap_err();
    match err {
        ExprError::CircularReference(cycle) => {
            let text = cycle.join(" → ");
            assert!(text.contains("d1") && text.contains("d2"), "{text}");
        }
        other => panic!("expected CircularReference, got {other:?}"),
    }

    assert_eq!(t.get(d1).unwrap().value, 50.0);
    assert!(t.get(d1).unwrap().expression.is_none());

    t.set_expression(d2, "50").unwrap();
    t.set_expression(d1, "=d2").unwrap();
    let err = t.set_expression(d2, "=d1 + 1").unwrap_err();
    assert!(matches!(err, ExprError::CircularReference(_)));
    assert_eq!(err.to_string(), "circular reference: d1 → d2 → d1");
}

#[test]
fn unknown_parameter_in_expression_is_rejected_and_rolled_back() {
    let mut t = ParamTable::new();
    let d1 = t.add(ParamKind::Length, None, 50.0).unwrap();
    let err = t.set_expression(d1, "=d7*2").unwrap_err();
    assert_eq!(err, ExprError::UnknownParameter("d7".to_string()));
    assert!(t.get(d1).unwrap().expression.is_none());
}

#[test]
fn rename_rewrites_references_and_rejects_duplicates() {
    let mut t = ParamTable::new();
    let d1 = t.add(ParamKind::Length, None, 50.0).unwrap();
    let d2 = t.add(ParamKind::Length, Some("=d1*2"), 0.0).unwrap();
    t.rename(d1, "width").unwrap();
    assert_eq!(t.get(d1).unwrap().name, "width");
    let expr = t.get(d2).unwrap().expression.clone().unwrap();
    assert!(expr.contains("width") && !expr.contains("d1"), "{expr}");
    assert_eq!(t.get(d2).unwrap().value, 100.0);

    assert!(t.rename(d2, "width").is_err());

    t.set_expression(d2, "=width/2").unwrap();
    assert_eq!(t.get(d2).unwrap().value, 25.0);
}

#[test]
fn failed_literal_edit_preserves_formula_and_all_dependents() {
    let mut table = ParamTable::new();
    let width = table.add(ParamKind::Length, Some("2 + 3"), 0.0).unwrap();
    table.add(ParamKind::Length, Some("10 / d1"), 0.0).unwrap();
    let before = serde_json::to_value(&table).unwrap();

    assert_eq!(
        table.set_expression(width, "0"),
        Err(ExprError::DivisionByZero)
    );
    assert_eq!(serde_json::to_value(&table).unwrap(), before);
    table.reevaluate().unwrap();
    assert_eq!(table.get(width).unwrap().value, 5.0);
}

#[test]
fn rejected_add_does_not_consume_parameter_identity_or_auto_name() {
    let mut table = ParamTable::new();
    let before = serde_json::to_value(&table).unwrap();
    assert!(table
        .add(ParamKind::Length, Some("unknown + 1"), 0.0)
        .is_err());
    assert_eq!(serde_json::to_value(&table).unwrap(), before);
    let id = table.add(ParamKind::Length, None, 10.0).unwrap();
    assert_eq!(id.0, 1);
    assert_eq!(table.get(id).unwrap().name, "d1");
}

#[test]
fn invalid_parameter_names_cannot_change_saved_references() {
    let mut table = ParamTable::new();
    let width = table.add(ParamKind::Length, None, 10.0).unwrap();
    table.add(ParamKind::Length, Some("d1 * 2"), 0.0).unwrap();
    let before = serde_json::to_value(&table).unwrap();
    for name in ["width gap", "width-gap", "width+gap", "width()", "width.1"] {
        assert!(table.rename(width, name).is_err(), "accepted {name}");
        assert_eq!(
            serde_json::to_value(&table).unwrap(),
            before,
            "changed {name}"
        );
    }
    table.rename(width, "width_1").unwrap();
    let saved = serde_json::to_string(&table).unwrap();
    let mut reopened: ParamTable = serde_json::from_str(&saved).unwrap();
    reopened.set_expression(width, "15").unwrap();
    assert_eq!(reopened.by_name("d2").unwrap().value, 30.0);
}

#[test]
fn non_finite_values_and_overflow_cannot_enter_parameter_tables() {
    let mut table = ParamTable::new();
    let width = table.add(ParamKind::Length, None, 10.0).unwrap();
    let before = serde_json::to_value(&table).unwrap();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(table.add(ParamKind::Length, None, value).is_err());
        assert_eq!(serde_json::to_value(&table).unwrap(), before);
    }
    for text in ["NaN", "inf", "-inf", "1e999", "10^999", "(-1)^0.5"] {
        assert!(
            table.set_expression(width, text).is_err(),
            "accepted {text}"
        );
        assert_eq!(
            serde_json::to_value(&table).unwrap(),
            before,
            "changed {text}"
        );
    }
}

#[test]
fn expression_evaluation_rejects_non_finite_intermediates_and_resolvers() {
    for text in ["10^999", "(-1)^0.5", "min(10^999, 1)", "0 * (10^999)"] {
        assert!(
            eval_expression(text, &mut |_| Ok(0.0)).is_err(),
            "accepted {text}"
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(eval_expression("d1", &mut |_| Ok(value)).is_err());
    }
}

#[test]
fn auto_names_skip_names_claimed_by_renamed_parameters() {
    let mut table = ParamTable::new();
    let first = table.add(ParamKind::Length, None, 5.0).unwrap();
    table.rename(first, "d2").unwrap();
    let second = table.add(ParamKind::Length, Some("d2 * 2"), 0.0).unwrap();
    assert_eq!(table.get(second).unwrap().name, "d3");
    assert_eq!(table.get(second).unwrap().value, 10.0);
    table.set_expression(first, "7").unwrap();
    assert_eq!(table.get(second).unwrap().value, 14.0);
}

#[test]
fn exhausted_saved_counters_reject_add_without_wrapping_or_changing_state() {
    for counters in [
        serde_json::json!({"next_id":u64::MAX,"next_name":0,"params":[]}),
        serde_json::json!({"next_id":0,"next_name":u32::MAX,"params":[]}),
    ] {
        let mut table: ParamTable = serde_json::from_value(counters.clone()).unwrap();
        assert!(table.add(ParamKind::Length, None, 1.0).is_err());
        assert_eq!(serde_json::to_value(&table).unwrap(), counters);
    }
}

#[test]
fn failed_formula_edit_rolls_back_when_a_later_dependent_becomes_invalid() {
    let mut table = ParamTable::new();
    let width = table.add(ParamKind::Length, None, 5.0).unwrap();
    table.add(ParamKind::Length, Some("d1 + 2"), 0.0).unwrap();
    table.add(ParamKind::Length, Some("sqrt(d1)"), 0.0).unwrap();
    let before = serde_json::to_value(&table).unwrap();
    assert!(table.set_expression(width, "2 - 10").is_err());
    assert_eq!(serde_json::to_value(&table).unwrap(), before);
}

#[test]
fn renaming_parameter_preserves_negative_power_formula_and_dependents() {
    let mut table = ParamTable::new();
    let width = table
        .add(ParamKind::Length, Some("10 + (-2)^2"), 0.)
        .unwrap();
    let dependent = table.add(ParamKind::Length, Some("d1 * 2"), 0.).unwrap();
    assert_eq!(table.get(width).unwrap().value, 14.);
    assert_eq!(table.get(dependent).unwrap().value, 28.);

    table.rename(width, "width").unwrap();
    table.reevaluate().unwrap();
    assert_eq!(table.by_name("width").unwrap().value, 14.);
    assert_eq!(table.get(dependent).unwrap().value, 28.);

    let saved = serde_json::to_string(&table).unwrap();
    let mut reopened: ParamTable = serde_json::from_str(&saved).unwrap();
    reopened.reevaluate().unwrap();
    assert_eq!(reopened.by_name("width").unwrap().value, 14.);
    assert_eq!(reopened.get(dependent).unwrap().value, 28.);
}
