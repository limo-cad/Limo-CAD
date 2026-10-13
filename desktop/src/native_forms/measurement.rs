use limo_cad_core::UnitSystem;
use limo_cad_sketch::{eval_expression, ExprError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DimensionKind {
    Length,
    Angle,
    Unitless,
}

impl DimensionKind {
    fn label(self) -> &'static str {
        match self {
            Self::Length => "length",
            Self::Angle => "angle",
            Self::Unitless => "unitless value",
        }
    }
}

/// Value in the engine's canonical millimeters/degrees, from the currently
/// owned sketch parameter snapshot. No parameter lookup crosses documents.
#[derive(Clone, Debug)]
pub(crate) struct ParameterValue {
    pub name: String,
    pub kind: DimensionKind,
    pub value: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct MeasurementInput {
    kind: DimensionKind,
    text: String,
}

fn length_scale(units: UnitSystem) -> f64 {
    match units {
        UnitSystem::Mm => 1.,
        UnitSystem::Cm => 10.,
        UnitSystem::In => 25.4,
    }
}

impl MeasurementInput {
    pub(crate) fn new(kind: DimensionKind, canonical: f64, units: UnitSystem) -> Self {
        let scale = if kind == DimensionKind::Length {
            length_scale(units)
        } else {
            1.
        };
        Self {
            kind,
            text: (canonical / scale).to_string(),
        }
    }

    /// Compact read-only length labels do not replace the editor's exact measurement text.
    pub(crate) fn display_length(canonical: f64, units: UnitSystem) -> String {
        let value = canonical / length_scale(units);
        let value = if value.abs() < 0.0005 { 0. } else { value };
        limo_cad_occt::drawing_presentation::text::trim(value, 3)
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn set_text(&mut self, text: String) {
        self.text = text;
    }

    /// Existing Rust arithmetic/parameter grammar, optionally followed by ONE
    /// result-unit suffix (mm, cm, in, deg, rad). Mixed per-operand units are
    /// not invented here. Expressions evaluate once into the existing numeric
    /// feature DTO; persistent feature-expression storage is not implied.
    pub(crate) fn evaluate(
        &self,
        units: UnitSystem,
        parameters: &[ParameterValue],
    ) -> Result<f64, String> {
        self.resolve(units, parameters).map(|(value, _, _)| value)
    }

    /// Sketch dimensions store expressions, not just their current value. Keep
    /// dependencies while translating a display-unit formula to engine units.
    pub(crate) fn evaluate_expression(
        &self,
        units: UnitSystem,
        parameters: &[ParameterValue],
    ) -> Result<(f64, String), String> {
        let (value, expression, scale) = self.resolve(units, parameters)?;
        if scale == 1. {
            return Ok((value, expression.to_owned()));
        }
        let ast = limo_cad_sketch::parse_expression(expression).map_err(|e| e.to_string())?;
        let names = limo_cad_sketch::referenced_idents(&ast);
        let mut converted = String::new();
        let mut chars = expression.trim().trim_start_matches('=').chars().peekable();
        while let Some(c) = chars.next() {
            if c.is_ascii_alphabetic() || c == '_' {
                let mut name = String::from(c);
                while chars
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    name.push(chars.next().unwrap());
                }
                let is_call = chars.clone().find(|c| !c.is_whitespace()) == Some('(');
                if names.contains(&name) && !is_call {
                    converted.push_str(&format!("({name}/{scale})"));
                } else {
                    converted.push_str(&name);
                }
            } else {
                converted.push(c);
            }
        }
        Ok((value, format!("({converted})*{scale}")))
    }

    fn resolve<'a>(
        &'a self,
        units: UnitSystem,
        parameters: &[ParameterValue],
    ) -> Result<(f64, &'a str, f64), String> {
        let text = self.text.trim();
        if text.is_empty() {
            return Err("Enter a measurement".into());
        }
        if text.len() > 1024 {
            return Err("A measurement expression must be at most 1024 bytes".into());
        }
        let mut nesting = 0usize;
        for character in text.chars() {
            if character == '(' {
                nesting += 1;
            }
            if nesting > 64 {
                return Err("The measurement expression is nested too deeply".into());
            }
            if character == ')' {
                nesting = nesting.saturating_sub(1);
            }
        }
        let mut expression = text;
        let mut scale = if self.kind == DimensionKind::Length {
            length_scale(units)
        } else {
            1.
        };
        let bare = text.strip_prefix('=').unwrap_or(text).trim();
        let single_identifier = bare
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
            && bare.chars().all(|c| c.is_alphanumeric() || c == '_');
        for (suffix, kind, factor) in [
            ("mm", DimensionKind::Length, 1.),
            ("cm", DimensionKind::Length, 10.),
            ("in", DimensionKind::Length, 25.4),
            ("deg", DimensionKind::Angle, 1.),
            ("rad", DimensionKind::Angle, 180. / std::f64::consts::PI),
        ] {
            if let Some(prefix) = text.strip_suffix(suffix).filter(|_| !single_identifier) {
                let separated = prefix
                    .chars()
                    .last()
                    .is_some_and(|c| c.is_whitespace() || c == ')');
                let numeric_tail: String = prefix
                    .chars()
                    .rev()
                    .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
                    .collect();
                let number = numeric_tail.chars().any(|c| c.is_ascii_digit())
                    && numeric_tail.chars().all(|c| c.is_ascii_digit() || c == '.');
                if separated || number {
                    if self.kind != kind {
                        return Err(format!(
                            "Expected {}, but {suffix} is a {} unit",
                            self.kind.label(),
                            kind.label()
                        ));
                    }
                    expression = prefix.trim_end();
                    scale = factor;
                    break;
                }
            }
        }
        let evaluated = eval_expression(expression, &mut |name| {
            let mut matches = parameters.iter().filter(|parameter| parameter.name == name);
            let parameter = matches
                .next()
                .ok_or_else(|| ExprError::UnknownParameter(name.into()))?;
            if matches.next().is_some() {
                return Err(ExprError::UnexpectedToken(format!(
                    "parameter '{name}' is ambiguous"
                )));
            }
            if parameter.kind != self.kind {
                return Err(ExprError::UnexpectedToken(format!(
                    "parameter '{name}' is {}, expected {}",
                    parameter.kind.label(),
                    self.kind.label()
                )));
            }
            if !parameter.value.is_finite() {
                return Err(ExprError::UnexpectedToken(format!(
                    "parameter '{name}' is not finite"
                )));
            }
            Ok(parameter.value / scale)
        })
        .map_err(|error| error.to_string())?;
        let canonical = evaluated * scale;
        if !canonical.is_finite() {
            return Err("The measurement result must be finite".into());
        }
        Ok((canonical, expression, scale))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(kind: DimensionKind, text: &str) -> MeasurementInput {
        let mut value = MeasurementInput::new(kind, 0., UnitSystem::Mm);
        value.set_text(text.into());
        value
    }

    #[test]
    fn stored_unit_expressions_keep_live_dependencies_and_function_names() {
        let mut params = vec![ParameterValue {
            name: "d1".into(),
            kind: DimensionKind::Length,
            value: 50.8,
        }];
        let (value, expression) = input(DimensionKind::Length, "d1 / 2 + 1")
            .evaluate_expression(UnitSystem::In, &params)
            .unwrap();
        assert!((value - 50.8).abs() < 1e-10);
        params[0].value = 76.2;
        let changed = eval_expression(&expression, &mut |_| Ok(params[0].value)).unwrap();
        assert!((changed - 63.5).abs() < 1e-10, "{expression}");
        params[0].name = "min".into();
        let (_, expression) = input(DimensionKind::Length, "min(min, 2)")
            .evaluate_expression(UnitSystem::In, &params)
            .unwrap();
        assert!(
            (eval_expression(&expression, &mut |_| Ok(76.2)).unwrap() - 50.8).abs() < 1e-10,
            "{expression}"
        );
    }

    #[test]
    fn expressions_reuse_engine_grammar_and_resolve_units_explicitly() {
        assert_eq!(
            input(DimensionKind::Length, "2")
                .evaluate(UnitSystem::Cm, &[])
                .unwrap(),
            20.
        );
        assert_eq!(
            input(DimensionKind::Length, "=1/4 in")
                .evaluate(UnitSystem::Mm, &[])
                .unwrap(),
            6.35
        );
        assert_eq!(
            input(DimensionKind::Length, "=(2 + 3)*4 mm")
                .evaluate(UnitSystem::In, &[])
                .unwrap(),
            20.
        );
        let radians = input(DimensionKind::Angle, "3.141592653589793 rad")
            .evaluate(UnitSystem::Mm, &[])
            .unwrap();
        assert!((radians - 180.).abs() < 1e-10);
        assert_eq!(
            input(DimensionKind::Unitless, "=2^3 + sin(30)*2")
                .evaluate(UnitSystem::In, &[])
                .unwrap(),
            9.
        );
        assert!(input(DimensionKind::Length, "2 mm + 1 in")
            .evaluate(UnitSystem::Mm, &[])
            .is_err());
    }

    #[test]
    fn dimension_kind_errors_and_actual_parser_failures_are_not_coerced_to_zero() {
        let parameters = [ParameterValue {
            name: "d1".into(),
            kind: DimensionKind::Length,
            value: 50.8,
        }];
        assert_eq!(
            input(DimensionKind::Length, "=d1/2 + 1")
                .evaluate(UnitSystem::In, &parameters)
                .unwrap(),
            50.8
        );
        assert!(input(DimensionKind::Angle, "d1")
            .evaluate(UnitSystem::Mm, &parameters)
            .unwrap_err()
            .contains("expected angle"));
        assert!(input(DimensionKind::Length, "3 deg")
            .evaluate(UnitSystem::Mm, &[])
            .is_err());
        assert!(input(DimensionKind::Unitless, "3 mm")
            .evaluate(UnitSystem::Mm, &[])
            .is_err());
        assert!(input(DimensionKind::Length, "1/0")
            .evaluate(UnitSystem::Mm, &[])
            .unwrap_err()
            .contains("division by zero"));
        assert!(input(DimensionKind::Length, "missing")
            .evaluate(UnitSystem::Mm, &[])
            .unwrap_err()
            .contains("unknown parameter"));
        assert!(input(DimensionKind::Length, "10^10000")
            .evaluate(UnitSystem::Mm, &[])
            .is_err());
        assert!(input(DimensionKind::Length, " ")
            .evaluate(UnitSystem::Mm, &[])
            .is_err());
        assert!(input(
            DimensionKind::Length,
            &format!("{}1{}", "(".repeat(100), ")".repeat(100))
        )
        .evaluate(UnitSystem::Mm, &[])
        .is_err());
    }

    #[test]
    fn result_unit_suffix_never_reinterprets_an_identifier_inside_an_expression() {
        let parameters = [ParameterValue {
            name: "part2in".into(),
            kind: DimensionKind::Length,
            value: 10.,
        }];
        assert_eq!(
            input(DimensionKind::Length, "=1 + part2in")
                .evaluate(UnitSystem::Mm, &parameters)
                .unwrap(),
            11.
        );
        assert!(
            (input(DimensionKind::Length, "=1 + 2in")
                .evaluate(UnitSystem::Mm, &[])
                .unwrap()
                - 76.2)
                .abs()
                < 1e-10
        );
        assert_eq!(
            input(DimensionKind::Length, "=(1 + part2in) mm")
                .evaluate(UnitSystem::In, &parameters)
                .unwrap(),
            11.
        );
    }
}
