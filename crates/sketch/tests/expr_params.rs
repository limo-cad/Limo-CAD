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
