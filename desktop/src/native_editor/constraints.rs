//! Selection rules for the existing constraint engine. These are interaction
//! requirements, not a second solver; the engine still rejects conflicts.
use super::sketch::Prepared;
use limo_cad_sketch::{Constraint, ConstraintBatchRequest, EntityDto, EntityId, SketchDto};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Relation {
    Align,
    Coincident,
    Tangent,
    Equal,
    Parallel,
    Perpendicular,
    Fix,
    Midpoint,
    Concentric,
    Collinear,
    Symmetry,
}
impl Relation {
    pub const ALL: [Self; 11] = [
        Self::Coincident,
        Self::Align,
        Self::Tangent,
        Self::Parallel,
        Self::Perpendicular,
        Self::Equal,
        Self::Fix,
        Self::Midpoint,
        Self::Concentric,
        Self::Collinear,
        Self::Symmetry,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Align => "Horizontal/Vertical",
            Self::Coincident => "Coincident",
            Self::Tangent => "Tangent",
            Self::Equal => "Equal",
            Self::Parallel => "Parallel",
            Self::Perpendicular => "Perpendicular",
            Self::Fix => "Fix/Unfix",
            Self::Midpoint => "Midpoint",
            Self::Concentric => "Concentric",
            Self::Collinear => "Collinear",
            Self::Symmetry => "Symmetry",
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Self::Align => "hv",
            Self::Fix => "fix",
            Self::Midpoint => "midpointC",
            Self::Coincident => "coincident",
            Self::Tangent => "tangent",
            Self::Equal => "equal",
            Self::Parallel => "parallel",
            Self::Perpendicular => "perpendicular",
            Self::Concentric => "concentric",
            Self::Collinear => "collinear",
            Self::Symmetry => "symmetry",
        }
    }
    pub fn needed(self) -> usize {
        match self {
            Self::Fix | Self::Align => 1,
            Self::Symmetry => 3,
            _ => 2,
        }
    }
    pub fn prepare(self, sketch: &SketchDto, ids: &[EntityId]) -> Result<Option<Prepared>, String> {
        let entities: Vec<_> = ids
            .iter()
            .map(|id| {
                sketch
                    .entities
                    .iter()
                    .find(|e| e.id() == *id)
                    .ok_or("The selected geometry no longer exists")
            })
            .collect::<Result<_, _>>()?;
        if ids.iter().enumerate().any(|(i, id)| ids[..i].contains(id)) {
            return Err("Select distinct entities".into());
        }
        if ids.len() < self.needed()
            || self == Self::Align
                && ids.len() == 1
                && matches!(entities[0], EntityDto::Point { .. })
        {
            return Ok(None);
        }
        let line = |e: &&EntityDto| {
            matches!(
                e,
                EntityDto::Line {
                    consumed: false,
                    ..
                }
            )
        };
        let point = |e: &&EntityDto| matches!(e, EntityDto::Point { .. });
        let curve = |e: &&EntityDto| matches!(e, EntityDto::Arc { .. } | EntityDto::Circle { .. });
        let invalid = || {
            format!(
                "{}: select {}",
                self.label(),
                match self {
                    Self::Align => "lines or two points",
                    Self::Coincident => "a point and a point/curve, or two circular curves",
                    Self::Tangent => "a line and a circular curve, or two circular curves",
                    Self::Equal => "two lines or two circular curves",
                    Self::Parallel | Self::Perpendicular | Self::Collinear => "two lines",
                    Self::Fix => "one or more entities",
                    Self::Midpoint => "one point and one line",
                    Self::Concentric => "two circular curves",
                    Self::Symmetry => "two points and an axis line, or three lines (axis last)",
                }
            )
        };
        if self == Self::Fix {
            return Ok(Some(Prepared {
                operation: "sketch_toggle_fix",
                arguments: serde_json::json!({"entity_ids":ids}),
            }));
        }
        let mut constraints = vec![];
        if self == Self::Align {
            if entities.iter().all(line) {
                for entity in entities {
                    if let EntityDto::Line { id, start, end, .. } = entity {
                        constraints.push(if (end.x - start.x).abs() >= (end.y - start.y).abs() {
                            Constraint::Horizontal { entity: *id }
                        } else {
                            Constraint::Vertical { entity: *id }
                        });
                    }
                }
            } else if entities.len() == 2 && entities.iter().all(point) {
                let (EntityDto::Point { position: a, .. }, EntityDto::Point { position: b, .. }) =
                    (entities[0], entities[1])
                else {
                    unreachable!()
                };
                constraints.push(if (b.x - a.x).abs() >= (b.y - a.y).abs() {
                    Constraint::HorizontalPoints {
                        a: ids[0],
                        b: ids[1],
                    }
                } else {
                    Constraint::VerticalPoints {
                        a: ids[0],
                        b: ids[1],
                    }
                });
            } else {
                return Err(invalid());
            }
        } else if self == Self::Symmetry {
            if ids.len() != 3 {
                return Err(invalid());
            }
            let lines: Vec<_> = entities
                .iter()
                .filter(|e| line(e))
                .map(|e| e.id())
                .collect();
            let points: Vec<_> = entities
                .iter()
                .filter(|e| point(e))
                .map(|e| e.id())
                .collect();
            let (a, b, axis) = if lines.len() == 3 {
                (ids[0], ids[1], ids[2])
            } else if points.len() == 2 && lines.len() == 1 {
                (points[0], points[1], lines[0])
            } else {
                return Err(invalid());
            };
            constraints.push(Constraint::Symmetry { a, b, axis });
        } else {
            if ids.len() != 2 {
                return Err(invalid());
            }
            let [a, b] = [ids[0], ids[1]];
            let both_lines = entities.iter().all(line);
            let both_curves = entities.iter().all(curve);
            let value = match self {
                Self::Coincident
                    if both_curves
                        || entities.iter().any(point)
                            && entities.iter().all(|e| point(e) || line(e) || curve(e)) =>
                {
                    Constraint::Coincident { a, b }
                }
                Self::Tangent
                    if both_curves || entities.iter().any(line) && entities.iter().any(curve) =>
                {
                    Constraint::Tangent { a, b }
                }
                Self::Equal if both_lines || both_curves => Constraint::Equal { a, b },
                Self::Parallel if both_lines => Constraint::Parallel { a, b },
                Self::Perpendicular if both_lines => Constraint::Perpendicular { a, b },
                Self::Collinear if both_lines => Constraint::Collinear { a, b },
                Self::Concentric if both_curves => Constraint::Concentric { a, b },
                Self::Midpoint if entities.iter().any(point) && entities.iter().any(line) => {
                    let p = entities.iter().find(|e| point(e)).unwrap().id();
                    let l = entities.iter().find(|e| line(e)).unwrap().id();
                    Constraint::Midpoint { a: p, b: l }
                }
                _ => return Err(invalid()),
            };
            constraints.push(value);
        }
        Ok(Some(Prepared {
            operation: "sketch_add_constraints",
            arguments: serde_json::to_value(ConstraintBatchRequest { constraints })
                .map_err(|e| e.to_string())?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use serde_json::{json, Value};
    fn parse(raw: String) -> Result<Value, String> {
        let envelope: Value = serde_json::from_str(&raw).unwrap();
        if envelope["ok"] == true {
            Ok(envelope["value"].clone())
        } else {
            Err(envelope["error"].to_string())
        }
    }
    fn call(engine: &AppState, method: &str, args: Value) -> Value {
        parse(engine.engine_call(method, &args.to_string())).unwrap()
    }
    fn drawing(engine: &AppState) -> SketchDto {
        serde_json::from_value(call(engine, "active_sketch", Value::Null)).unwrap()
    }
    fn apply(engine: &AppState, command: Prepared) -> Result<Value, String> {
        let spec = limo_cad_mcp_mutate::lookup_mutate(command.operation).unwrap();
        parse(engine.engine_call(
            spec.engine_method,
            &limo_cad_mcp_mutate::encode_payload(spec.payload, &command.arguments).unwrap(),
        ))
    }
    fn lines() -> AppState {
        let engine = AppState::new();
        call(
            &engine,
            "begin_sketch",
            json!({"type":"origin_plane","plane":"xy"}),
        );
        for (a, b) in [([10., 10.], [40., 11.]), ([10., 30.], [40., 31.])] {
            call(
                &engine,
                "add_line",
                json!({"from":{"x":a[0],"y":a[1]},"to_raw":{"x":b[0],"y":b[1]},"ctrl_held":true}),
            );
        }
        engine
    }
    #[test]
    fn aligned_selection_is_one_engine_transaction_and_undo_restores_geometry() {
        let engine = lines();
        let before = drawing(&engine);
        let ids = before
            .entities
            .iter()
            .filter(|e| matches!(e, EntityDto::Line { .. }))
            .map(EntityDto::id)
            .collect::<Vec<_>>();
        apply(
            &engine,
            Relation::Align.prepare(&before, &ids).unwrap().unwrap(),
        )
        .unwrap();
        let after = drawing(&engine);
        assert_eq!(after.constraints.len(), before.constraints.len() + 2);
        for entity in after.entities {
            if let EntityDto::Line { start, end, .. } = entity {
                assert!((start.y - end.y).abs() < 1e-5);
            }
        }
        call(&engine, "undo", Value::Null);
        assert_eq!(drawing(&engine).entities, before.entities);
    }
    #[test]
    fn invalid_or_stale_selection_cannot_produce_a_constraint_command() {
        let engine = lines();
        let sketch = drawing(&engine);
        let point = sketch
            .entities
            .iter()
            .find(|e| matches!(e, EntityDto::Point { .. }))
            .unwrap()
            .id();
        let line = sketch
            .entities
            .iter()
            .find(|e| matches!(e, EntityDto::Line { .. }))
            .unwrap()
            .id();
        assert!(Relation::Parallel.prepare(&sketch, &[point, line]).is_err());
        assert!(Relation::Equal.prepare(&sketch, &[line, line]).is_err());
        assert!(Relation::Align
            .prepare(&sketch, &[EntityId(u64::MAX)])
            .is_err());
        assert!(Relation::Align
            .prepare(&sketch, &[point])
            .unwrap()
            .is_none());
        let command = Relation::Midpoint
            .prepare(&sketch, &[line, point])
            .unwrap()
            .unwrap();
        assert_eq!(command.arguments["constraints"][0]["a"], json!(point));
        assert_eq!(command.arguments["constraints"][0]["b"], json!(line));
    }
}
