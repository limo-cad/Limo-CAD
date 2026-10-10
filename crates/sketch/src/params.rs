//! Sketch parameter table: named, auto-assigned parameters (d1, d2, …)
//! backing every driving dimension. Expressions reference other parameters
//! by name; evaluation is memoized in
//! topological order with clear cycle errors.

use serde::{Deserialize, Serialize};

use crate::expr::{self, Ast, ExprError};

/// Stable internal id of a parameter (survives renames).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ParamId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    Length,
    Angle,
}

/// One named parameter. `expression: None` = literal value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    pub id: ParamId,
    pub name: String,
    pub kind: ParamKind,
    /// Formula text (without the leading `=`); `None` = literal `value`.
    pub expression: Option<String>,
    /// Current evaluated value (mm / deg), refreshed by `reevaluate`.
    pub value: f64,
}

/// The per-sketch parameter table (owned by `Sketch`, undo-snapshotted).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParamTable {
    params: Vec<Parameter>,
    next_id: u64,
    next_name: u32,
}

impl ParamTable {
    pub fn new() -> Self {
        Self {
            params: Vec::new(),
            next_id: 0,
            next_name: 0,
        }
    }

    pub fn all(&self) -> &[Parameter] {
        &self.params
    }

    pub fn get(&self, id: ParamId) -> Option<&Parameter> {
        self.params.iter().find(|p| p.id == id)
    }

    /// Validate the stable identifiers, names, and expression graph before
    /// accepting a parameter table from a project archive.
    pub(crate) fn validate(&self) -> Result<(), String> {
        use std::collections::HashSet;

        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for parameter in &self.params {
            if parameter.id.0 == 0 || !ids.insert(parameter.id) {
                return Err(format!("duplicate or zero parameter id {}", parameter.id.0));
            }
            if parameter.name.trim().is_empty() || !names.insert(parameter.name.as_str()) {
                return Err(format!(
                    "duplicate or empty parameter name '{}'",
                    parameter.name
                ));
            }
            if !parameter.value.is_finite() {
                return Err(format!(
                    "parameter '{}' has a non-finite value",
                    parameter.name
                ));
            }
        }
        if ids.iter().map(|id| id.0).max().unwrap_or(0) > self.next_id {
            return Err("next parameter id is behind the saved parameter table".to_string());
        }

        let mut evaluated = self.clone();
        evaluated
            .reevaluate()
            .map_err(|error| format!("invalid parameter expression: {error}"))?;
        if evaluated
            .params
            .iter()
            .any(|parameter| !parameter.value.is_finite())
        {
            return Err("parameter expressions produce a non-finite value".to_string());
        }
        Ok(())
    }

    pub fn by_name(&self, name: &str) -> Option<&Parameter> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Allocate the next auto name (d1, d2, … in creation order).
    fn alloc_name(&mut self) -> String {
        self.next_name += 1;
        format!("d{}", self.next_name)
    }

    /// Add a parameter with an auto-assigned name. `text: None` = literal
    /// `literal`; `Some` = formula (evaluated immediately; the add rolls
    /// back cleanly on unknown references/cycles).
    pub fn add(
        &mut self,
        kind: ParamKind,
        text: Option<&str>,
        literal: f64,
    ) -> Result<ParamId, ExprError> {
        self.next_id += 1;
        let id = ParamId(self.next_id);
        let name = self.alloc_name();
        self.params.push(Parameter {
            id,
            name,
            kind,
            expression: text.map(|t| t.trim().trim_start_matches('=').trim().to_string()),
            value: literal,
        });
        if let Err(e) = self.reevaluate() {
            self.params.retain(|p| p.id != id);
            let _ = self.reevaluate();
            return Err(e);
        }
        Ok(id)
    }

    /// Remove a parameter (dimension delete orphan cleanup).
    pub fn remove(&mut self, id: ParamId) {
        self.params.retain(|p| p.id != id);
    }

    /// Set a parameter's expression (or make it literal when `text` parses
    /// as a plain number). Re-evaluates dependents.
    pub fn set_expression(&mut self, id: ParamId, text: &str) -> Result<(), ExprError> {
        let trimmed = text.trim().trim_start_matches('=').trim();
        if trimmed.is_empty() {
            return Err(ExprError::UnexpectedToken("empty expression".to_string()));
        }
        let param = self
            .params
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| ExprError::UnknownParameter(format!("#{id:?}")))?;
        if let Ok(v) = trimmed.parse::<f64>() {
            param.expression = None;
            param.value = v;
        } else {
            let old = param.expression.clone();
            param.expression = Some(trimmed.to_string());
            if let Err(e) = self.reevaluate() {
                if let Some(param) = self.params.iter_mut().find(|p| p.id == id) {
                    param.expression = old;
                }
                let _ = self.reevaluate();
                return Err(e);
            }
            return Ok(());
        }
        self.reevaluate()
    }

    /// Rename a parameter (stable internal id). References in other
    /// parameters' expressions are rewritten to the new name. Rejected
    /// when the new name is already in use (documented choice).
    pub fn rename(&mut self, id: ParamId, new_name: &str) -> Result<(), ExprError> {
        let new_name = new_name.trim();
        if new_name.is_empty() || !new_name.chars().next().unwrap().is_ascii_alphabetic() {
            return Err(ExprError::UnexpectedToken(format!(
                "invalid parameter name '{new_name}'"
            )));
        }
        if self.params.iter().any(|p| p.id != id && p.name == new_name) {
            return Err(ExprError::UnexpectedToken(format!(
                "parameter '{new_name}' already exists"
            )));
        }
        let Some(old_name) = self
            .params
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
        else {
            return Err(ExprError::UnknownParameter(format!("#{id:?}")));
        };
        if old_name == new_name {
            return Ok(());
        }
        for p in self.params.iter_mut() {
            if p.id == id {
                p.name = new_name.to_string();
            } else if let Some(text) = &p.expression {
                if let Ok(mut ast) = expr::parse(text) {
                    expr::rename_ident(&mut ast, &old_name, new_name);
                    p.expression = Some(expr::to_string(&ast));
                }
            }
        }
        self.reevaluate()
    }

    /// Recompute every parameter's value in dependency order.
    pub fn reevaluate(&mut self) -> Result<(), ExprError> {
        let mut asts: Vec<Option<Ast>> = Vec::with_capacity(self.params.len());
        for p in &self.params {
            asts.push(match &p.expression {
                Some(text) => Some(expr::parse(text)?),
                None => None,
            });
        }

        let n = self.params.len();
        let mut values = vec![0.0; n];
        let mut state = vec![0u8; n];

        fn visit(
            i: usize,
            params: &[Parameter],
            asts: &[Option<Ast>],
            values: &mut [f64],
            state: &mut [u8],
            stack: &mut Vec<String>,
        ) -> Result<(), ExprError> {
            match state[i] {
                2 => return Ok(()),
                1 => {
                    let start = stack.iter().position(|n| n == &params[i].name).unwrap_or(0);
                    let mut cycle = stack[start..].to_vec();
                    cycle.push(params[i].name.clone());
                    return Err(ExprError::CircularReference(cycle));
                }
                _ => {}
            }
            state[i] = 1;
            stack.push(params[i].name.clone());
            let value = match &asts[i] {
                None => params[i].value,
                Some(ast) => {
                    let mut resolver = |name: &str| -> Result<f64, ExprError> {
                        let Some(j) = params.iter().position(|p| p.name == name) else {
                            return Err(ExprError::UnknownParameter(name.to_string()));
                        };
                        visit(j, params, asts, values, state, stack)?;
                        Ok(values[j])
                    };
                    expr::eval(ast, &mut resolver)?
                }
            };
            stack.pop();
            state[i] = 2;
            values[i] = value;
            Ok(())
        }

        for i in 0..n {
            let mut stack = Vec::new();
            visit(i, &self.params, &asts, &mut values, &mut state, &mut stack)?;
        }
        for (p, v) in self.params.iter_mut().zip(values) {
            p.value = v;
        }
        Ok(())
    }
}
