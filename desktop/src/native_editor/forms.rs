//! Typed sketch modification drafts; request construction uses the existing
//! engine DTOs. Selection and numeric text remain editable until Apply.
use super::sketch::Prepared;
use crate::native_forms::{DimensionKind, MeasurementInput, ParameterValue};
use limo_cad_core::UnitSystem;
use limo_cad_sketch::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FormKind {
    Fillet,
    Chamfer,
    Offset,
    MoveCopy,
    Scale,
    Mirror,
    RectangularPattern,
    CircularPattern,
    Polygon,
}

impl FormKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fillet => "Fillet",
            Self::Chamfer => "Chamfer",
            Self::Offset => "Offset",
            Self::MoveCopy => "Move/Copy",
            Self::Scale => "Scale",
            Self::Mirror => "Mirror",
            Self::RectangularPattern => "Rectangular Pattern",
            Self::CircularPattern => "Circular Pattern",
            Self::Polygon => "Polygon",
        }
    }
    pub fn group(self) -> &'static str {
        match self {
            Self::Mirror | Self::RectangularPattern | Self::CircularPattern => "sketch/repeat",
            Self::Polygon => "sketch/draw",
            _ => "sketch/edit",
        }
    }
    pub fn fields(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Fillet => &[("Radius", "2")],
            Self::Chamfer => &[("Distance", "2")],
            Self::Offset => &[("Distance", "2")],
            Self::MoveCopy => &[("X distance", "0"), ("Y distance", "0")],
            Self::Scale => &[("Scale factor", "1"), ("Origin X", "0"), ("Origin Y", "0")],
            Self::Mirror => &[],
            Self::RectangularPattern => &[
                ("Direction angle", "0"),
                ("Spacing", "10"),
                ("Count", "3"),
                ("Second direction angle", "90"),
                ("Second spacing", "10"),
                ("Second count", "1"),
            ],
            Self::CircularPattern => &[
                ("Center X", "0"),
                ("Center Y", "0"),
                ("Count", "6"),
                ("Total angle", "360"),
            ],
            Self::Polygon => &[
                ("Center X", "0"),
                ("Center Y", "0"),
                ("Sides", "6"),
                ("Radius", "10"),
                ("Rotation angle", "0"),
            ],
        }
    }
    pub fn instruction(self) -> &'static str {
        match self {
            Self::Fillet | Self::Chamfer => "Select two lines at a corner",
            Self::Offset => "Select a curve, then click on its offset side",
            Self::Mirror => "Select geometry, then select the mirror axis last",
            Self::Polygon => "Click the center, or enter its coordinates",
            _ => "Select geometry (Shift-click removes a selection)",
        }
    }
}
pub(super) struct ModifyForm {
    pub id: u64,
    pub kind: FormKind,
    pub values: Vec<String>,
    pub option: bool,
    pub point: Option<Vec2>,
}
impl ModifyForm {
    pub fn new(id: u64, kind: FormKind) -> Self {
        Self {
            id,
            kind,
            values: kind.fields().iter().map(|(_, v)| v.to_string()).collect(),
            option: false,
            point: None,
        }
    }
    pub fn request(
        &self,
        sketch: &SketchDto,
        selection: &[EntityId],
        units: UnitSystem,
    ) -> Result<Prepared, String> {
        let entities = selection
            .iter()
            .map(|id| {
                sketch
                    .entities
                    .iter()
                    .find(|e| e.id() == *id)
                    .ok_or("Selected geometry changed")
            })
            .collect::<Result<Vec<_>, _>>()?;
        if entities.is_empty() && self.kind != FormKind::Polygon {
            return Err(self.kind.instruction().into());
        }
        let parameters: Vec<_> = sketch
            .dimensions
            .iter()
            .filter_map(|d| {
                Some(ParameterValue {
                    name: d.param_name.clone()?,
                    kind: if d.kind == "angle" {
                        DimensionKind::Angle
                    } else {
                        DimensionKind::Length
                    },
                    value: d.value,
                })
            })
            .collect();
        let number = |i: usize, kind| -> Result<f64, String> {
            let mut value = MeasurementInput::new(kind, 0., units);
            value.set_text(self.values[i].clone());
            value
                .evaluate(units, &parameters)
                .map_err(|e| format!("{}: {e}", self.kind.fields()[i].0))
        };
        let expression = |i: usize, kind| -> Result<String, String> {
            let mut value = MeasurementInput::new(kind, 0., units);
            value.set_text(self.values[i].clone());
            value
                .evaluate_expression(units, &parameters)
                .map(|(_, text)| text)
                .map_err(|e| format!("{}: {e}", self.kind.fields()[i].0))
        };
        let count = |i: usize, minimum: u32, maximum: u32| -> Result<u32, String> {
            let value = number(i, DimensionKind::Unitless)?;
            if !(f64::from(minimum)..=f64::from(maximum)).contains(&value) || value.fract() != 0. {
                return Err(format!(
                    "{} must be an integer from {minimum} to {maximum}",
                    self.kind.fields()[i].0
                ));
            }
            Ok(value as u32)
        };
        let pair = || -> Result<(EntityId, EntityId), String> {
            if entities.len() != 2
                || !entities.iter().all(|e| {
                    matches!(
                        e,
                        EntityDto::Line {
                            consumed: false,
                            ..
                        }
                    )
                })
            {
                return Err(self.kind.instruction().into());
            }
            Ok((selection[0], selection[1]))
        };
        fn encoded(
            operation: &'static str,
            value: impl serde::Serialize,
        ) -> Result<Prepared, String> {
            Ok(Prepared {
                operation,
                arguments: serde_json::to_value(value).map_err(|e| e.to_string())?,
            })
        }
        use DimensionKind::{Angle, Length};
        match self.kind {
            FormKind::Fillet => {
                let (l1, l2) = pair()?;
                encoded(
                    "sketch_fillet",
                    FilletRequest {
                        l1,
                        l2,
                        radius_text: expression(0, Length)?,
                    },
                )
            }
            FormKind::Chamfer => {
                let (l1, l2) = pair()?;
                encoded(
                    "sketch_chamfer",
                    ChamferRequest {
                        l1,
                        l2,
                        distance_text: expression(0, Length)?,
                    },
                )
            }
            FormKind::Offset => {
                if selection.len() != 1 || matches!(entities[0], EntityDto::Point { .. }) {
                    return Err(self.kind.instruction().into());
                }
                encoded(
                    "sketch_offset",
                    OffsetRequest {
                        entity: selection[0],
                        distance_text: expression(0, Length)?,
                        cursor: self.point.ok_or("Click on the desired offset side")?,
                    },
                )
            }
            FormKind::MoveCopy => encoded(
                "sketch_move_copy",
                MoveCopyRequest {
                    entity_ids: selection.to_vec(),
                    dx: number(0, Length)?,
                    dy: number(1, Length)?,
                    copy: self.option,
                },
            ),
            FormKind::Scale => encoded(
                "sketch_scale",
                ScaleRequest {
                    entity_ids: selection.to_vec(),
                    origin: Vec2::new(number(1, Length)?, number(2, Length)?),
                    factor_text: expression(0, DimensionKind::Unitless)?,
                },
            ),
            FormKind::Mirror => {
                let Some(axis) = entities.last().filter(|e| {
                    matches!(
                        e,
                        EntityDto::Line {
                            consumed: false,
                            ..
                        }
                    )
                }) else {
                    return Err(self.kind.instruction().into());
                };
                if entities.len() < 2 {
                    return Err(self.kind.instruction().into());
                }
                encoded(
                    "sketch_mirror",
                    MirrorRequest {
                        entity_ids: selection[..selection.len() - 1].to_vec(),
                        axis_line: axis.id(),
                    },
                )
            }
            FormKind::RectangularPattern => {
                let a = number(0, Angle)?.to_radians();
                let b = number(3, Angle)?.to_radians();
                let second = count(5, 1, 1000)?;
                encoded(
                    "sketch_rectangular_pattern",
                    RectangularPatternRequest {
                        entity_ids: selection.to_vec(),
                        direction: Vec2::new(a.cos(), a.sin()),
                        spacing: number(1, Length)?,
                        count: count(2, 2, 1000)?,
                        second_direction: (second > 1).then(|| Vec2::new(b.cos(), b.sin())),
                        second_spacing: number(4, Length)?,
                        second_count: second,
                    },
                )
            }
            FormKind::CircularPattern => encoded(
                "sketch_circular_pattern",
                CircularPatternRequest {
                    entity_ids: selection.to_vec(),
                    center: Vec2::new(number(0, Length)?, number(1, Length)?),
                    count: count(2, 2, 1000)?,
                    total_angle_deg: number(3, Angle)?,
                },
            ),
            FormKind::Polygon => {
                let sides = count(2, 3, 10000)?;
                encoded(
                    "sketch_polygon",
                    PolygonRequest {
                        center: Vec2::new(number(0, Length)?, number(1, Length)?),
                        edge_count: sides,
                        radius_text: expression(3, Length)?,
                        rotation_deg: number(4, Angle)?,
                        mode: if self.option {
                            "circumscribed"
                        } else {
                            "inscribed"
                        }
                        .into(),
                    },
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use serde_json::{json, Value};

    fn call(engine: &AppState, method: &str, args: Value) -> Value {
        let envelope: Value =
            serde_json::from_str(&engine.engine_call(method, &args.to_string())).unwrap();
        assert_eq!(envelope["ok"], true, "{method}: {envelope}");
        envelope["value"].clone()
    }
    fn drawing(engine: &AppState) -> SketchDto {
        serde_json::from_value(call(engine, "active_sketch", Value::Null)).unwrap()
    }
    fn rectangle() -> AppState {
        let engine = AppState::new();
        call(
            &engine,
            "begin_sketch",
            json!({"type":"origin_plane","plane":"xy"}),
        );
        call(
            &engine,
            "add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":30.,"y":20.},"ctrl_held":true}),
        );
        engine
    }
    fn apply(engine: &AppState, command: Prepared) -> Value {
        let spec = limo_cad_mcp_mutate::lookup_mutate(command.operation).unwrap();
        let args = limo_cad_mcp_mutate::encode_payload(spec.payload, &command.arguments).unwrap();
        call(
            engine,
            spec.engine_method,
            serde_json::from_str(&args).unwrap(),
        )
    }

    #[test]
    fn native_modify_forms_use_engine_transactions_and_undo() {
        for kind in [
            FormKind::Fillet,
            FormKind::Chamfer,
            FormKind::Offset,
            FormKind::MoveCopy,
            FormKind::Scale,
            FormKind::Mirror,
            FormKind::RectangularPattern,
            FormKind::CircularPattern,
            FormKind::Polygon,
        ] {
            let engine = rectangle();
            let before = drawing(&engine);
            let lines: Vec<_> = before
                .entities
                .iter()
                .filter(|e| matches!(e, EntityDto::Line { .. }))
                .map(EntityDto::id)
                .collect();
            let selection = match kind {
                FormKind::Fillet | FormKind::Chamfer | FormKind::Mirror => &lines[..2],
                FormKind::Polygon => &lines[..0],
                _ => &lines[..1],
            };
            let mut form = ModifyForm::new(1, kind);
            if kind == FormKind::Offset {
                form.point = Some(Vec2::new(15., -5.));
            }
            if kind == FormKind::Scale {
                form.values[0] = "2".into();
            }
            if kind == FormKind::MoveCopy {
                form.values[0] = "5".into();
                form.option = true;
            }
            let prepared = form.request(&before, selection, UnitSystem::Mm).unwrap();
            let preview =
                super::super::modify_preview::form(&engine, before.basis, &prepared).unwrap();
            if matches!(
                kind,
                FormKind::Fillet | FormKind::Chamfer | FormKind::Offset
            ) {
                assert!(preview
                    .as_ref()
                    .is_some_and(|p| p.lines.iter().any(|l| !l.segments.is_empty())));
                assert_eq!(
                    drawing(&engine),
                    before,
                    "{kind:?} preview mutated the sketch or history"
                );
            }
            apply(&engine, prepared);
            let after = drawing(&engine);
            if matches!(kind, FormKind::Chamfer | FormKind::Offset) {
                let segments = &preview.as_ref().unwrap().lines[0].segments;
                assert_eq!(segments.len(), 6);
                let a = Vec2::new(f64::from(segments[0]), f64::from(segments[1]));
                let b = Vec2::new(f64::from(segments[3]), f64::from(segments[4]));
                assert!(after.entities.iter().any(|e| matches!(e, EntityDto::Line { start, end, consumed: false, .. }
                    if (start.distance(a) < 1e-5 && end.distance(b) < 1e-5) || (start.distance(b) < 1e-5 && end.distance(a) < 1e-5))),
                    "{kind:?} preview differs from its committed construction");
            }
            assert_ne!(
                after.entities, before.entities,
                "{kind:?} did not modify geometry"
            );
            call(&engine, "undo", Value::Null);
            assert_eq!(
                drawing(&engine).entities,
                before.entities,
                "{kind:?} did not undo atomically"
            );
        }
    }

    #[test]
    fn measurements_and_invalid_pattern_counts_do_not_silently_change_meaning() {
        let engine = rectangle();
        let sketch = drawing(&engine);
        let ids: Vec<_> = sketch
            .entities
            .iter()
            .filter(|e| matches!(e, EntityDto::Line { .. }))
            .map(EntityDto::id)
            .collect();
        let mut form = ModifyForm::new(1, FormKind::Fillet);
        form.values[0] = "0.125".into();
        let command = form.request(&sketch, &ids[..2], UnitSystem::In).unwrap();
        assert_eq!(
            limo_cad_sketch::eval_expression(
                command.arguments["radius_text"].as_str().unwrap(),
                &mut |_| unreachable!()
            )
            .unwrap(),
            3.175
        );
        form.values[0] = "3 mm".into();
        assert_eq!(
            form.request(&sketch, &ids[..2], UnitSystem::In)
                .unwrap()
                .arguments["radius_text"],
            "3"
        );
        for kind in [FormKind::RectangularPattern, FormKind::CircularPattern] {
            let mut form = ModifyForm::new(1, kind);
            for text in ["0", "1", "1.5", "1001", "nan"] {
                form.values[2] = text.into();
                assert!(
                    form.request(&sketch, &ids[..1], UnitSystem::Mm).is_err(),
                    "{kind:?}: {text}"
                );
            }
        }
        assert!(form
            .request(&sketch, &[EntityId(u64::MAX)], UnitSystem::Mm)
            .is_err());
    }

    #[test]
    fn fillet_form_keeps_a_live_radius_expression() {
        let engine = rectangle();
        let lines: Vec<_> = drawing(&engine)
            .entities
            .iter()
            .filter(|e| matches!(e, EntityDto::Line { .. }))
            .map(EntityDto::id)
            .collect();
        call(
            &engine,
            "add_dimension",
            json!({"entities":[lines[0]],"text_pos":{"x":15.,"y":-5.},"value_text":"30"}),
        );
        let before = drawing(&engine);
        let source = &before.dimensions[0];
        let name = source.param_name.as_ref().unwrap();
        let mut form = ModifyForm::new(1, FormKind::Fillet);
        form.values[0] = format!("{name}/10");
        apply(
            &engine,
            form.request(&before, &lines[..2], UnitSystem::Mm).unwrap(),
        );
        let after = drawing(&engine);
        assert!(after
            .dimensions
            .iter()
            .any(|d| d.param_expression.as_deref() == Some(form.values[0].as_str())));
        call(
            &engine,
            "edit_dimension",
            json!({"constraint_id":source.constraint_id,"text":"40"}),
        );
        assert!(
            drawing(&engine)
                .entities
                .iter()
                .any(|e| matches!(e, EntityDto::Arc {radius,..} if (radius - 4.).abs() < 1e-6)),
            "Radius did not follow the source dimension"
        );
    }
}
