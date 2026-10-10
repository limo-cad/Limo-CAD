//! On-canvas drawing dimensions. Typed values use the existing constrained
//! engine operations; these fields hold only one unfinished gesture.
use super::*;
use crate::native_forms::{DimensionKind, MeasurementInput, ParameterValue};
use crate::native_viewport::interface_shell::fields;
use crate::session_bridge::native_interface::controller::chrome::{rect, Widgets};
use limo_cad_core::UnitSystem;
use limo_cad_interface::{Field, KeyChord};
use limo_cad_sketch::{
    LockedCircleRequest, LockedRectangleRequest, LockedSegmentRequest, SlotRequest,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SizeField {
    Length,
    Angle,
    Width,
    Height,
    Diameter,
}
impl SizeField {
    fn label(self) -> &'static str {
        match self {
            Self::Length => "Length",
            Self::Angle => "Angle",
            Self::Width => "Width",
            Self::Height => "Height",
            Self::Diameter => "Diameter",
        }
    }
    fn kind(self) -> DimensionKind {
        if self == Self::Angle {
            DimensionKind::Angle
        } else {
            DimensionKind::Length
        }
    }
}

#[derive(Clone, Debug)]
struct LockedValue {
    text: String,
    resolved: Result<(f64, String), String>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct Sizes {
    values: HashMap<SizeField, LockedValue>,
}

impl Sizes {
    pub(super) fn has_locks(&self) -> bool {
        !self.values.is_empty()
    }
    fn set(&mut self, field: SizeField, text: String, units: UnitSystem, sketch: &SketchDto) {
        if text.trim().is_empty() {
            self.values.remove(&field);
            return;
        }
        let parameters: Vec<_> = sketch
            .dimensions
            .iter()
            .filter_map(|d| {
                Some(ParameterValue {
                    name: d.param_name.clone()?,
                    value: d.value,
                    kind: if d.kind == "angle" {
                        DimensionKind::Angle
                    } else {
                        DimensionKind::Length
                    },
                })
            })
            .collect();
        let mut input = MeasurementInput::new(field.kind(), 0., units);
        input.set_text(text.clone());
        let resolved = input.evaluate_expression(units, &parameters).and_then(|v| {
            if field != SizeField::Angle && v.0 <= 0. {
                Err("Enter a positive size".into())
            } else {
                Ok(v)
            }
        });
        self.values.insert(field, LockedValue { text, resolved });
    }
    fn value(&self, field: SizeField) -> Result<Option<(f64, String)>, String> {
        self.values
            .get(&field)
            .map(|v| v.resolved.clone())
            .transpose()
            .map_err(|e| format!("{}: {e}", field.label()))
    }
    fn pair(&self, field: SizeField) -> Result<(Option<f64>, Option<String>), String> {
        Ok(self
            .value(field)?
            .map(|(v, t)| (Some(v), Some(t)))
            .unwrap_or_default())
    }
    pub(super) fn prepare(
        &self,
        tool: CreateTool,
        picks: &[SketchPoint],
        ctrl: bool,
    ) -> Result<Option<Prepared>, String> {
        if self.values.is_empty() {
            return Ok(None);
        }
        self.request(tool, picks, ctrl).map(Some)
    }
    fn request(
        &self,
        tool: CreateTool,
        picks: &[SketchPoint],
        ctrl: bool,
    ) -> Result<Prepared, String> {
        let anchor = *picks.first().ok_or("Pick the first point")?;
        let hint = *picks.get(1).ok_or("Pick the second point")?;
        let (operation, arguments) = match tool {
            CreateTool::Line => {
                let (length_mm, length_text) = self.pair(SizeField::Length)?;
                let (angle_deg, angle_text) = self.pair(SizeField::Angle)?;
                (
                    "sketch_add_line_locked",
                    serde_json::to_value(LockedSegmentRequest {
                        from: anchor,
                        to_hint: hint,
                        length_mm,
                        length_text,
                        angle_deg,
                        angle_text,
                        ctrl_held: ctrl,
                        from_crossing: None,
                        to_crossing: None,
                        tracking: None,
                        intersection: None,
                    }),
                )
            }
            CreateTool::Rectangle(mode) => {
                let (width_mm, width_text) = self.pair(SizeField::Width)?;
                let (height_mm, height_text) = self.pair(SizeField::Height)?;
                (
                    "sketch_add_rectangle_locked",
                    serde_json::to_value(LockedRectangleRequest {
                        mode,
                        anchor,
                        corner_hint: hint,
                        width_mm,
                        width_text,
                        height_mm,
                        height_text,
                        ctrl_held: ctrl,
                    }),
                )
            }
            CreateTool::Circle(mode) => {
                let (diameter_mm, diameter_text) = self.pair(SizeField::Diameter)?;
                (
                    "sketch_add_circle_locked",
                    serde_json::to_value(LockedCircleRequest {
                        mode,
                        anchor,
                        edge_hint: hint,
                        diameter_mm,
                        diameter_text,
                        ctrl_held: ctrl,
                    }),
                )
            }
            CreateTool::Slot(mode) => {
                let (width_mm, width_text) = self.pair(SizeField::Width)?;
                (
                    "sketch_add_slot",
                    serde_json::to_value(SlotRequest {
                        mode,
                        p1: anchor,
                        p2: hint,
                        cursor: *picks.get(2).ok_or("Pick the slot width")?,
                        width_mm,
                        width_text,
                        ctrl_held: ctrl,
                    }),
                )
            }
            _ => return Err("This tool has no typed size fields".into()),
        };
        Ok(Prepared {
            operation,
            arguments: arguments.map_err(|e| e.to_string())?,
        })
    }
}

fn fields_for(draft: &Draft) -> &'static [SizeField] {
    if draft.points.is_empty() {
        return &[];
    }
    match draft.tool {
        Some(CreateTool::Line) => &[SizeField::Length, SizeField::Angle],
        Some(CreateTool::Rectangle(_)) => &[SizeField::Width, SizeField::Height],
        Some(CreateTool::Circle(_)) => &[SizeField::Diameter],
        Some(CreateTool::Slot(_)) if draft.points.len() == 2 => &[SizeField::Width],
        _ => &[],
    }
}

pub(super) fn set(
    world: &mut World,
    (engine, bridge, owner): (&AppState, &SessionBridgeState, &DocumentContext),
    editor: &mut Editor,
    generation: u64,
    field: SizeField,
    text: String,
    validate: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    bridge.with_native_document_owner(engine, owner, validate)?;
    if editor.draft.generation != generation || !fields_for(&editor.draft).contains(&field) {
        return Err("This drawing gesture changed".into());
    }
    let sketch = active(engine)?.ok_or("There is no active sketch")?;
    editor
        .draft
        .sizes
        .set(field, text, engine.document_units(), &sketch);
    editor.draft.resolved_preview = None;
    editor.error = fields_for(&editor.draft)
        .iter()
        .find_map(|field| editor.draft.sizes.value(*field).err())
        .unwrap_or_default();
    clear_preview(world, engine, bridge, owner)?;
    if editor.error.is_empty() {
        if let Some(raw) = editor.draft.cursor {
            preview(world, engine, bridge, owner, editor, raw, false)?;
        }
    }
    Ok(
        json!({"edited":true,"valid":editor.error.is_empty(),"locked":editor.draft.sizes.values.contains_key(&field)}),
    )
}

/// Query the engine's exact locked construction without changing history.
/// Return resolved points for the existing lightweight outline renderer.
pub(super) fn preview_points(
    engine: &AppState,
    draft: &Draft,
    raw: SketchPoint,
    ctrl: bool,
) -> Result<Option<[SketchPoint; 2]>, String> {
    let Some(tool) = draft.tool else {
        return Ok(None);
    };
    if draft.points.is_empty()
        || draft.sizes.values.is_empty()
        || !matches!(tool, CreateTool::Rectangle(_) | CreateTool::Circle(_))
    {
        return Ok(None);
    }
    let mut request = draft.sizes.request(tool, &[draft.points[0], raw], ctrl)?;
    snapping::attach(&mut request.arguments, draft.snap_context);
    let method = if matches!(tool, CreateTool::Rectangle(_)) {
        "preview_rectangle_locked"
    } else {
        "preview_circle_locked"
    };
    let value = crate::session_bridge::parse_engine_envelope(
        engine.engine_call(method, &request.arguments.to_string()),
    )?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}
pub(super) fn line_preview(
    engine: &AppState,
    draft: &Draft,
    raw: SketchPoint,
    ctrl: bool,
) -> Result<limo_cad_sketch::PreviewDto, String> {
    let mut request = draft.sizes.request(
        CreateTool::Line,
        &[draft.points.last().copied().unwrap_or(raw), raw],
        ctrl,
    )?;
    snapping::attach(&mut request.arguments, draft.snap_context);
    let value = crate::session_bridge::parse_engine_envelope(
        engine.engine_call("preview_segment_locked", &request.arguments.to_string()),
    )?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

pub(super) fn slot_cursor(draft: &Draft, raw: SketchPoint) -> Result<SketchPoint, String> {
    if !matches!(draft.tool, Some(CreateTool::Slot(_))) || draft.points.len() != 2 {
        return Ok(raw);
    }
    let Some((width, _)) = draft.sizes.value(SizeField::Width)? else {
        return Ok(raw);
    };
    let axis = draft.points[1] - draft.points[0];
    let length = axis.length();
    if length < 1e-9 {
        return Ok(raw);
    }
    let delta = raw - draft.points[0];
    let sign = if axis.x * delta.y - axis.y * delta.x >= 0. {
        1.
    } else {
        -1.
    };
    Ok(draft.points[0]
        + axis * (delta.dot(axis) / (length * length))
        + SketchPoint::new(-axis.y, axis.x) * (sign * width / (2. * length)))
}

#[derive(Resource, Default)]
struct DynamicPanel {
    generation: Option<u64>,
    owner: Option<DocumentContext>,
    fields: HashMap<SizeField, Entity>,
    widgets: Widgets,
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    engine: &AppState,
    owner: &DocumentContext,
    editor: &Editor,
    canvas: InterfaceRect,
) -> Result<(), String> {
    let mut panel = world.remove_resource::<DynamicPanel>().unwrap_or_default();
    let result = (|| {
        let visible = fields_for(&editor.draft);
        if panel.generation != Some(editor.draft.generation) || panel.owner.as_ref() != Some(owner)
        {
            for (_, entity) in panel.fields.drain() {
                world.despawn(entity);
            }
            panel.generation = Some(editor.draft.generation);
            panel.owner = Some(owner.clone());
        }
        panel.fields.retain(|key, entity| {
            if visible.contains(key) {
                true
            } else {
                world.despawn(*entity);
                false
            }
        });
        panel.widgets.begin();
        if !visible.is_empty() {
            let cursor = editor.draft.cursor.unwrap_or(editor.draft.points[0]);
            let basis = editor
                .stamp
                .as_ref()
                .and_then(|s| s.basis)
                .ok_or("Sketch plane changed")?;
            let screen = native_viewport::interface_world_point(
                world,
                &owner.document_id,
                basis.to_3d([cursor.x, cursor.y]),
            )?
            .unwrap_or([0., 0.]);
            let width = visible.len() as f32 * 154.;
            let left = (canvas.x as f32 + screen[0] + 18.).clamp(
                canvas.x as f32 + 8.,
                (canvas.x as f32 + canvas.width as f32 - 240. - width - 8.)
                    .max(canvas.x as f32 + 8.),
            );
            let top = (canvas.y as f32 + screen[1] + 22.).clamp(
                canvas.y as f32 + 8.,
                (canvas.y as f32 + canvas.height as f32 - 68.).max(canvas.y as f32 + 8.),
            );
            let theme = crate::native_viewport::ui::theme(world);
            let assets = world.resource::<ViewportUiAssets>().clone();
            let units = engine.document_units();
            for (index, &field) in visible.iter().enumerate() {
                let x = left + index as f32 * 154.;
                let locked = editor.draft.sizes.values.get(&field);
                let value = locked.map(|v| v.text.clone()).unwrap_or_else(|| {
                    let [anchor, endpoint] = editor
                        .draft
                        .resolved_preview
                        .unwrap_or([editor.draft.points[0]; 2]);
                    let d = endpoint - anchor;
                    let full = if matches!(
                        editor.draft.tool,
                        Some(CreateTool::Rectangle(RectangleMode::Center))
                    ) {
                        2.
                    } else {
                        1.
                    };
                    let value = match field {
                        SizeField::Length => d.length(),
                        SizeField::Angle => d.y.atan2(d.x).to_degrees(),
                        SizeField::Width
                            if matches!(editor.draft.tool, Some(CreateTool::Slot(_))) =>
                        {
                            let axis = editor.draft.points[1] - editor.draft.points[0];
                            2. * (axis.x * d.y - axis.y * d.x).abs() / axis.length().max(1e-9)
                        }
                        SizeField::Width => d.x.abs() * full,
                        SizeField::Height => d.y.abs() * full,
                        SizeField::Diameter => {
                            d.length()
                                * if editor.draft.tool
                                    == Some(CreateTool::Circle(CircleMode::CenterDiameter))
                                {
                                    2.
                                } else {
                                    1.
                                }
                        }
                    };
                    let scale = if field == SizeField::Angle {
                        1.
                    } else {
                        match units {
                            UnitSystem::Mm => 1.,
                            UnitSystem::Cm => 10.,
                            UnitSystem::In => 25.4,
                        }
                    };
                    format!("{:.3}", value / scale)
                });
                panel.widgets.panel(
                    world,
                    camera,
                    &format!("size-bg-{index}"),
                    rect(x, top, 150., 54.),
                    theme.panel,
                    34,
                );
                panel.widgets.text(
                    world,
                    camera,
                    &format!("size-label-{index}"),
                    rect(x + 6., top + 2., 138., 18.),
                    &format!(
                        "{}{}",
                        field.label(),
                        if locked.is_some() { " · locked" } else { "" }
                    ),
                    10.,
                    35,
                );
                let mut c = InterfaceControl::button(
                    "sketch/draw",
                    format!("Drawing {}", field.label().to_lowercase()),
                );
                c.field = Field::Text {
                    value,
                    read_only: false,
                    selection: None,
                };
                c.owned_keys.push(KeyChord::plain("Enter"));
                let mut bounds = rect(x + 4., top + 21., 142., 28.);
                bounds.border = UiRect::all(px(1.));
                bounds.padding = UiRect::horizontal(px(5.));
                let entity = if let Some(entity) = panel.fields.get(&field) {
                    *entity
                } else {
                    let entity = fields::spawn_text_field(
                        &mut world.commands(),
                        camera,
                        bounds.clone(),
                        c.clone(),
                        theme,
                        &assets,
                    )?;
                    world.flush();
                    bind_command(
                        world,
                        entity,
                        NativeCommand::Sketch(EditorCommand::Size {
                            generation: editor.draft.generation,
                            field,
                            text: String::new(),
                        }),
                    )?;
                    panel.fields.insert(field, entity);
                    world.entity_mut(entity).insert(fields::DrawingDimension {
                        generation: editor.draft.generation,
                        index,
                    });
                    if index == 0 {
                        fields::request_focus(world, entity, owner);
                    }
                    entity
                };
                let mut old = world.get::<InterfaceControl>(entity).unwrap().clone();
                if locked.is_some() || !fields::has_uncommitted_edit(world, entity) {
                    old.field = c.field;
                }
                if world.get::<InterfaceControl>(entity) != Some(&old) {
                    world.entity_mut(entity).insert(old);
                }
                if world.get::<Node>(entity) != Some(&bounds) {
                    world.entity_mut(entity).insert(bounds);
                }
                let border = BorderColor::all(if locked.is_some_and(|v| v.resolved.is_err()) {
                    Color::srgb(0.95, 0.3, 0.3)
                } else if locked.is_some() {
                    theme.accent
                } else {
                    theme.edge
                });
                if world.get::<BorderColor>(entity) != Some(&border) {
                    world.entity_mut(entity).insert(border);
                }
                world.entity_mut(entity).insert(ZIndex(35));
            }
        }
        panel.widgets.finish(world);
        Ok(())
    })();
    world.insert_resource(panel);
    result
}

#[cfg(test)]
#[path = "dynamic_preview_tests.rs"]
mod preview_tests;

#[cfg(test)]
mod tests {
    use super::*;
    fn call(engine: &AppState, method: &str, args: Value) -> Value {
        crate::session_bridge::parse_engine_envelope(engine.engine_call(method, &args.to_string()))
            .unwrap()
    }
    fn blank() -> AppState {
        let engine = AppState::new();
        call(
            &engine,
            "begin_sketch",
            json!({"type":"origin_plane","plane":"xy"}),
        );
        engine
    }
    fn apply(engine: &AppState, command: Prepared) -> Value {
        let spec = limo_cad_mcp_mutate::lookup_mutate(command.operation).unwrap();
        let payload =
            limo_cad_mcp_mutate::encode_payload(spec.payload, &command.arguments).unwrap();
        crate::session_bridge::parse_engine_envelope(
            engine.engine_call(spec.engine_method, &payload),
        )
        .unwrap()
    }
    #[test]
    fn enter_finishes_typed_shapes_once_and_one_undo_removes_the_geometry() {
        for tool in [
            CreateTool::Line,
            CreateTool::Rectangle(RectangleMode::TwoPoint),
            CreateTool::Rectangle(RectangleMode::Center),
            CreateTool::Circle(CircleMode::CenterDiameter),
            CreateTool::Circle(CircleMode::TwoPoint),
            CreateTool::Slot(SlotMode::CenterToCenter),
            CreateTool::Slot(SlotMode::Overall),
            CreateTool::Slot(SlotMode::CenterPoint),
        ] {
            let engine = blank();
            let initial = active(&engine).unwrap().unwrap();
            let mut draft = Draft::default();
            draft.select(Some(tool));
            draft.prepare(SketchPoint::ZERO, false).unwrap();
            if matches!(tool, CreateTool::Slot(_)) {
                draft.prepare(SketchPoint::new(20., 0.), false).unwrap();
            }
            draft.cursor = Some(SketchPoint::new(12., 8.));
            let sizes: &[(SizeField, &str)] = match tool {
                CreateTool::Line => &[(SizeField::Length, "10"), (SizeField::Angle, "45")],
                CreateTool::Rectangle(_) => &[(SizeField::Width, "5"), (SizeField::Height, "9")],
                CreateTool::Circle(_) => &[(SizeField::Diameter, "6")],
                _ => &[(SizeField::Width, "4")],
            };
            for (field, text) in sizes {
                draft
                    .sizes
                    .set(*field, (*text).into(), UnitSystem::Mm, &initial);
            }
            let points = draft.points.clone();
            let generation = draft.generation;
            let command = draft.complete().unwrap().unwrap();
            assert_eq!(draft.points, points, "{tool:?}");
            assert_eq!(draft.generation, generation);
            assert_eq!(active(&engine).unwrap().unwrap(), initial);
            let result = apply(&engine, command);
            draft.accepted(&result).unwrap();
            let sketch = active(&engine).unwrap().unwrap();
            assert!(!sketch.entities.is_empty(), "{tool:?}");
            if tool == CreateTool::Line {
                let json = serde_json::to_value(&sketch).unwrap();
                let line = json["entities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|e| e["kind"] == "line")
                    .unwrap();
                assert!((line["end"]["x"].as_f64().unwrap() - 10. / 2_f64.sqrt()).abs() < 1e-7);
                assert!((line["end"]["y"].as_f64().unwrap() - 10. / 2_f64.sqrt()).abs() < 1e-7);
                assert_eq!(sketch.dimensions.len(), 2);
            }
            call(&engine, "undo", json!({}));
            let sketch = active(&engine).unwrap().unwrap();
            assert!(
                sketch.entities.is_empty()
                    && sketch.dimensions.is_empty()
                    && sketch.constraints.is_empty(),
                "{tool:?}"
            );
        }
    }

    #[test]
    fn enter_rejects_invalid_size_without_losing_the_picked_anchor() {
        let engine = blank();
        let initial = active(&engine).unwrap().unwrap();
        let mut draft = Draft::default();
        draft.select(Some(CreateTool::Line));
        draft.prepare(SketchPoint::ZERO, false).unwrap();
        draft.cursor = Some(SketchPoint::new(12., 8.));
        draft.sizes.set(
            SizeField::Length,
            "invalid".into(),
            UnitSystem::Mm,
            &initial,
        );
        assert!(draft.complete().is_err());
        assert_eq!(draft.points, vec![SketchPoint::ZERO]);
        assert_eq!(active(&engine).unwrap().unwrap(), initial);
    }
    #[test]
    fn partial_rectangle_size_at_its_anchor_waits_for_the_other_axis() {
        for mode in [RectangleMode::TwoPoint, RectangleMode::Center] {
            for first in [SizeField::Width, SizeField::Height] {
                let engine = blank();
                let initial = active(&engine).unwrap().unwrap();
                let mut draft = Draft::default();
                draft.select(Some(CreateTool::Rectangle(mode)));
                draft.prepare(SketchPoint::ZERO, true).unwrap();
                draft
                    .sizes
                    .set(first, "60".into(), UnitSystem::Mm, &initial);
                assert_eq!(
                    preview_points(&engine, &draft, SketchPoint::ZERO, true).unwrap(),
                    None
                );
                assert_eq!(active(&engine).unwrap().unwrap(), initial);
                let command = draft.prepare(SketchPoint::ZERO, true).unwrap().unwrap();
                let spec = limo_cad_mcp_mutate::lookup_mutate(command.operation).unwrap();
                let payload =
                    limo_cad_mcp_mutate::encode_payload(spec.payload, &command.arguments).unwrap();
                assert!(crate::session_bridge::parse_engine_envelope(
                    engine.engine_call(spec.engine_method, &payload)
                )
                .is_err());
                assert_eq!(active(&engine).unwrap().unwrap(), initial);
                let other = if first == SizeField::Width {
                    SizeField::Height
                } else {
                    SizeField::Width
                };
                draft
                    .sizes
                    .set(other, "30".into(), UnitSystem::Mm, &initial);
                let points = preview_points(&engine, &draft, SketchPoint::ZERO, true)
                    .unwrap()
                    .unwrap();
                assert_eq!(active(&engine).unwrap().unwrap(), initial);
                let result = apply(
                    &engine,
                    draft.prepare(SketchPoint::ZERO, true).unwrap().unwrap(),
                );
                draft.accepted(&result).unwrap();
                let after = active(&engine).unwrap().unwrap();
                assert_eq!(after.dimensions.len(), 2);
                assert!(after
                    .entities
                    .iter()
                    .filter_map(|entity| match entity {
                        limo_cad_sketch::EntityDto::Point { position, .. } => Some(position),
                        _ => None,
                    })
                    .any(|position| position.distance(points[1]) < 1e-6));
                call(&engine, "undo", json!({}));
                assert_eq!(active(&engine).unwrap().unwrap().entities, initial.entities);
            }
        }
    }
    #[test]
    fn snapped_cursor_keeps_typed_circle_preview_and_commit_on_the_same_direction() {
        let engine = blank();
        call(
            &engine,
            "add_point",
            json!({"position":{"x":40.,"y":20.},"ctrl_held":true}),
        );
        let initial = active(&engine).unwrap().unwrap();
        let mut draft = Draft::default();
        draft.select(Some(CreateTool::Circle(CircleMode::TwoPoint)));
        draft.prepare(SketchPoint::ZERO, false).unwrap();
        draft
            .sizes
            .set(SizeField::Diameter, "25.4".into(), UnitSystem::Mm, &initial);
        let context = limo_cad_sketch::ViewportSnapContext {
            grid_step_mm: 10.,
            point_tolerance_mm: 1.4,
            grid_capture_mm: 0.7,
        };
        draft.snap_context = Some(context);
        let raw = SketchPoint::new(39.4, 20.6);
        let acquired = snapping::acquire(&engine, &draft, raw, false, context).unwrap();
        assert_eq!(acquired.snapped_to, SketchPoint::new(40., 20.));
        let preview = preview_points(&engine, &draft, raw, false)
            .unwrap()
            .unwrap();
        let mut command = draft
            .prepare(snapping::pick(&draft, raw, &acquired), false)
            .unwrap()
            .unwrap();
        snapping::attach(&mut command.arguments, Some(context));
        apply(&engine, command);
        let after = active(&engine).unwrap().unwrap();
        let (center, radius) = after
            .entities
            .iter()
            .find_map(|entity| match entity {
                limo_cad_sketch::EntityDto::Circle { center, radius, .. } => {
                    Some((*center, *radius))
                }
                _ => None,
            })
            .unwrap();
        assert!(center.distance((preview[0] + preview[1]) * 0.5) < 1e-6);
        assert!((radius - 12.7).abs() < 1e-6);
        call(&engine, "undo", json!({}));
        assert_eq!(active(&engine).unwrap().unwrap().entities, initial.entities);
    }
    #[test]
    fn typed_creation_drives_geometry_preview_and_single_undo() {
        for (tool, fields) in [
            (
                CreateTool::Line,
                vec![(SizeField::Length, "25.4"), (SizeField::Angle, "30")],
            ),
            (
                CreateTool::Rectangle(RectangleMode::TwoPoint),
                vec![(SizeField::Width, "25.4"), (SizeField::Height, "12.7")],
            ),
            (
                CreateTool::Rectangle(RectangleMode::Center),
                vec![(SizeField::Width, "25.4"), (SizeField::Height, "12.7")],
            ),
            (
                CreateTool::Circle(CircleMode::CenterDiameter),
                vec![(SizeField::Diameter, "25.4")],
            ),
            (
                CreateTool::Circle(CircleMode::TwoPoint),
                vec![(SizeField::Diameter, "25.4")],
            ),
            (
                CreateTool::Slot(SlotMode::CenterToCenter),
                vec![(SizeField::Width, "12.7")],
            ),
            (
                CreateTool::Slot(SlotMode::Overall),
                vec![(SizeField::Width, "12.7")],
            ),
            (
                CreateTool::Slot(SlotMode::CenterPoint),
                vec![(SizeField::Width, "12.7")],
            ),
        ] {
            let engine = blank();
            let initial = active(&engine).unwrap().unwrap();
            let mut draft = Draft::default();
            draft.select(Some(tool));
            draft.prepare(SketchPoint::ZERO, true).unwrap();
            if matches!(tool, CreateTool::Slot(_)) {
                draft.prepare(SketchPoint::new(40., 0.), true).unwrap();
            }
            for (field, text) in &fields {
                draft
                    .sizes
                    .set(*field, (*text).into(), UnitSystem::Mm, &initial);
            }
            let raw = SketchPoint::new(40., 20.);
            let preview = preview_points(&engine, &draft, raw, true).unwrap();
            assert_eq!(
                active(&engine).unwrap().unwrap(),
                initial,
                "Preview mutated {tool:?}"
            );
            let result = apply(&engine, draft.prepare(raw, true).unwrap().unwrap());
            let after = active(&engine).unwrap().unwrap();
            assert_eq!(after.dimensions.len(), fields.len(), "{tool:?}");
            for (field, text) in &fields {
                let expected: f64 = text.parse().unwrap();
                assert!(
                    after
                        .dimensions
                        .iter()
                        .any(|d| (d.value - expected).abs() < 1e-6),
                    "{tool:?} {field:?}: {:?}",
                    after.dimensions
                );
            }
            if let Some(points) = preview {
                let mut resolved = draft.clone();
                resolved.points = vec![points[0]];
                let outline = resolved.outline(points[1]);
                let position_of = |id| {
                    after
                        .entities
                        .iter()
                        .find_map(|e| match e {
                            limo_cad_sketch::EntityDto::Point {
                                id: point,
                                position,
                                ..
                            } if *point == id => Some(*position),
                            _ => None,
                        })
                        .expect("A center relation must reference an existing point")
                };
                let mut center_count = 0;
                let mut line_count = 0;
                for e in &after.entities {
                    match e {
                        limo_cad_sketch::EntityDto::Point { id, position, .. } => {
                            let center = after.constraints.iter().find_map(|c| match c.constraint {
                                limo_cad_sketch::Constraint::SpanMidpoint { point, start, end } if point == *id => {
                                    assert_eq!(tool, CreateTool::Rectangle(RectangleMode::Center));
                                    let start = position_of(start);
                                    let end = position_of(end);
                                    assert!(outline.iter().flatten().any(|p| p.distance(start) < 1e-6));
                                    assert!(outline.iter().flatten().any(|p| p.distance(end) < 1e-6));
                                    let center = (start + end) * 0.5;
                                    assert!(center.distance(points[0]) < 1e-6, "Center rectangle lost its picked center");
                                    Some(center)
                                }
                                limo_cad_sketch::Constraint::CenterCoincident { point, curve } if point == *id => {
                                    let expected = match tool {
                                        CreateTool::Circle(CircleMode::CenterDiameter) => points[0],
                                        CreateTool::Circle(CircleMode::TwoPoint) => (points[0] + points[1]) * 0.5,
                                        _ => panic!("Unexpected curve center for {tool:?}"),
                                    };
                                    let center = after.entities.iter().find_map(|e| match e {
                                        limo_cad_sketch::EntityDto::Circle { id, center, .. } if *id == curve => Some(*center),
                                        _ => None,
                                    }).expect("The selectable circle center must reference its circle");
                                    assert!(center.distance(expected) < 1e-6, "Circle preview and committed center differ");
                                    Some(center)
                                }
                                _ => None,
                            });
                            if let Some(center) = center {
                                center_count += 1;
                                assert!(
                                    position.distance(center) < 1e-6,
                                    "{tool:?}: center handle is not at its constrained center"
                                );
                            } else {
                                assert!(
                                    outline
                                        .iter()
                                        .flatten()
                                        .any(|p| p.distance(*position) < 1e-6),
                                    "{tool:?}: perimeter point {position:?} does not match its preview"
                                );
                            }
                        }
                        limo_cad_sketch::EntityDto::Line { start, end, .. } => {
                            line_count += 1;
                            assert!(
                                outline.iter().any(|[a, b]| (a.distance(*start) < 1e-6
                                    && b.distance(*end) < 1e-6)
                                    || (a.distance(*end) < 1e-6 && b.distance(*start) < 1e-6)),
                                "{tool:?}: committed edge does not match a preview segment"
                            );
                        }
                        limo_cad_sketch::EntityDto::Circle { center, radius, .. } => {
                            assert!(outline
                                .iter()
                                .flatten()
                                .all(|p| (p.distance(*center) - radius).abs() < 1e-6))
                        }
                        _ => (),
                    }
                }
                assert_eq!(
                    center_count,
                    usize::from(matches!(
                        tool,
                        CreateTool::Rectangle(RectangleMode::Center) | CreateTool::Circle(_)
                    )),
                    "{tool:?}: every centered primitive must expose exactly one constrained center"
                );
                assert_eq!(
                    line_count,
                    if matches!(tool, CreateTool::Rectangle(_)) {
                        4
                    } else {
                        0
                    }
                );
            }
            draft.accepted(&result).unwrap();
            assert!(draft.sizes.values.is_empty());
            call(&engine, "undo", json!({}));
            assert_eq!(
                active(&engine).unwrap().unwrap().entities,
                initial.entities,
                "{tool:?}"
            );
        }
    }
    #[test]
    fn invalid_sizes_block_commit_and_clearing_restores_free_drawing() {
        let engine = blank();
        let sketch = active(&engine).unwrap().unwrap();
        let mut draft = Draft::default();
        draft.select(Some(CreateTool::Rectangle(RectangleMode::TwoPoint)));
        draft.prepare(SketchPoint::ZERO, true).unwrap();
        for text in ["0", "-3", "missing+1", "4/"] {
            draft
                .sizes
                .set(SizeField::Width, text.into(), UnitSystem::Mm, &sketch);
            assert!(draft.prepare(SketchPoint::new(30., 20.), true).is_err());
            assert_eq!(draft.points, vec![SketchPoint::ZERO]);
            assert_eq!(active(&engine).unwrap().unwrap(), sketch);
        }
        draft
            .sizes
            .set(SizeField::Width, "1 in".into(), UnitSystem::Mm, &sketch);
        let request = draft
            .prepare(SketchPoint::new(30., 20.), true)
            .unwrap()
            .unwrap();
        assert_eq!(request.arguments["width_mm"], 25.4);
        draft
            .sizes
            .set(SizeField::Width, "".into(), UnitSystem::Mm, &sketch);
        assert_eq!(
            draft
                .prepare(SketchPoint::new(30., 20.), true)
                .unwrap()
                .unwrap()
                .operation,
            "sketch_add_rectangle"
        );
        let old = draft.generation;
        draft.escape();
        assert!(draft.generation > old);
    }
}
