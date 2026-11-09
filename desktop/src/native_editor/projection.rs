//! Owner/revision-scoped selection of associative sketch references. Geometry
//! and history belong to the shared engine; this picker stores only choices.
use super::*;
use crate::native_viewport::localization;
use crate::session_bridge::native_interface::controller::chrome::{rect, Widgets};
use limo_cad_sketch::projection::SketchCurveRef;
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Command {
    Open,
    Close,
    Tab {
        generation: u64,
        existing: bool,
    },
    Page {
        generation: u64,
        start: usize,
    },
    Add {
        generation: u64,
        source: SketchCurveRef,
    },
    Remove {
        generation: u64,
        id: u32,
    },
}

#[derive(Clone, Deserialize)]
struct Source {
    source: SketchCurveRef,
    sketch: String,
    kind: String,
    entity: u64,
}

#[derive(Clone, Deserialize)]
struct Reference {
    id: u32,
    source: SketchCurveRef,
    error: Option<String>,
}

pub(super) struct Picker {
    generation: u64,
    sources: Vec<Source>,
    references: Vec<Reference>,
    existing: bool,
    start: usize,
}

fn query(engine: &AppState, method: &str) -> Result<Value, String> {
    crate::session_bridge::parse_engine_envelope(engine.engine_call(method, ""))
}

pub(super) fn execute(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
    editor: &mut Editor,
    command: Command,
    validate: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    let expected = editor
        .stamp
        .as_ref()
        .filter(|stamp| stamp.sketch.is_some())
        .ok_or("Start or edit a sketch before projecting curves")?
        .clone();
    bridge.with_native_document_receipt(engine, owner, |revision| {
        validate()?;
        if revision != expected.revision {
            return Err("The sketch changed; reopen Project".into());
        }
        Ok(())
    })?;
    if command == Command::Close {
        editor.projection = None;
        return Ok(json!({"closed":true}));
    }
    if command == Command::Open {
        let (sources, references) =
            bridge.with_native_document_receipt(engine, owner, |revision| {
                if revision != expected.revision {
                    return Err("The sketch changed; reopen Project".into());
                }
                let sources = query(engine, "sketch_projection_sources")?;
                let references = query(engine, "sketch_projections")?;
                Ok((
                    serde_json::from_value::<Vec<Source>>(sources["sources"].clone())
                        .map_err(|e| e.to_string())?,
                    serde_json::from_value::<Vec<Reference>>(references["projections"].clone())
                        .map_err(|e| e.to_string())?,
                ))
            })?;
        editor.form_serial = editor
            .form_serial
            .checked_add(1)
            .ok_or("Editor generation exhausted")?;
        editor.draft.select(None);
        editor.interaction = Default::default();
        editor.error.clear();
        editor.press = None;
        clear_preview(world, engine, bridge, owner)?;
        editor.projection = Some(Picker {
            generation: editor.form_serial,
            sources,
            references,
            existing: false,
            start: 0,
        });
        return Ok(json!({"projection_picker":true}));
    }
    let picker = editor
        .projection
        .as_mut()
        .ok_or("Reopen Project to select a source")?;
    let generation = match command {
        Command::Tab { generation, .. }
        | Command::Page { generation, .. }
        | Command::Add { generation, .. }
        | Command::Remove { generation, .. } => generation,
        _ => unreachable!(),
    };
    if picker.generation != generation {
        return Err("The projection picker changed; reopen it".into());
    }
    match command {
        Command::Tab { existing, .. } => {
            picker.existing = existing;
            picker.start = 0;
        }
        Command::Page { start, .. } => {
            let count = if picker.existing {
                picker.references.len()
            } else {
                picker.sources.len()
            };
            if start >= count && start != 0 {
                return Err("Projection page no longer exists".into());
            }
            picker.start = start;
        }
        Command::Add { source, .. } => {
            if !picker.sources.iter().any(|item| item.source == source) {
                return Err("Projection source is no longer available".into());
            }
            return queue_mutation(
                world,
                expected,
                "sketch_project",
                json!({"sources":[source]}),
                Completion::Projection,
            );
        }
        Command::Remove { id, .. } => {
            if !picker.references.iter().any(|item| item.id == id) {
                return Err("Projection no longer exists".into());
            }
            return queue_mutation(
                world,
                expected,
                "sketch_projection_remove",
                json!({"id":id}),
                Completion::Projection,
            );
        }
        _ => unreachable!(),
    }
    Ok(json!({"projection_picker":true}))
}

#[derive(Resource, Default)]
struct Panel {
    widgets: Widgets,
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    editor: &Editor,
    canvas: InterfaceRect,
) -> Result<(), String> {
    let mut panel = world.remove_resource::<Panel>().unwrap_or_default();
    panel.widgets.begin();
    let result = (|| {
        let Some(picker) = editor.projection.as_ref() else {
            return Ok(());
        };
        let tr = |key| localization::translate(world, key).to_owned();
        let title = tr("projection.title");
        let instruction = tr("projection.instruction");
        let add = tr("projection.add");
        let references = tr("projection.references");
        let close = tr("projection.close");
        let previous = tr("projection.previous");
        let next = tr("projection.next");
        let empty = tr(if picker.existing {
            "projection.noReferences"
        } else {
            "projection.noSources"
        });
        let missing = tr("projection.missing");
        let remove = tr("projection.remove");
        let kinds = [
            ("line", tr("projection.line")),
            ("arc", tr("projection.arc")),
            ("circle", tr("projection.circle")),
            ("spline", tr("projection.spline")),
        ];
        let width = (canvas.width as f32 - 8.).clamp(220., 320.);
        let height = (canvas.height as f32 - 8.).clamp(188., 372.);
        let left = (canvas.x + canvas.width) as f32 - width - 4.;
        let top = canvas.y as f32 + 4.;
        let theme = crate::native_viewport::ui::theme(world);
        panel.widgets.panel(
            world,
            camera,
            "body",
            rect(left, top, width, height),
            theme.panel.with_alpha(1.),
            40,
        );
        panel.widgets.text(
            world,
            camera,
            "title",
            rect(left + 10., top + 6., width - 20., 24.),
            &title,
            13.,
            41,
        );
        panel.widgets.text(
            world,
            camera,
            "instruction",
            rect(left + 10., top + 32., width - 20., 48.),
            &instruction,
            11.,
            41,
        );
        let generation = picker.generation;
        for (key, label, existing, x) in [
            ("add", &add, false, left + 10.),
            ("references", &references, true, left + width / 2.),
        ] {
            let mut control = InterfaceControl::button("sketch/projection", label);
            control.selected = Some(picker.existing == existing);
            panel.widgets.button(
                world,
                camera,
                key,
                control,
                None,
                NativeCommand::Sketch(EditorCommand::Projection(Command::Tab {
                    generation,
                    existing,
                })),
                rect(x, top + 82., width / 2. - 12., 26.),
                None,
                41,
            )?;
        }
        let count = if picker.existing {
            picker.references.len()
        } else {
            picker.sources.len()
        };
        let per_page = ((height - 156.) / 28.).floor().max(1.) as usize;
        if count == 0 {
            panel.widgets.text(
                world,
                camera,
                "empty",
                rect(left + 10., top + 114., width - 20., height - 154.),
                &empty,
                11.,
                41,
            );
        }
        for (row, index) in (picker.start..count).take(per_page).enumerate() {
            let (caption, command, disabled) = if picker.existing {
                let item = &picker.references[index];
                let caption = if item.error.is_some() {
                    format!("{remove} {} · {missing}", item.id)
                } else {
                    format!(
                        "{remove} {} · {}:{}",
                        item.id, item.source.feature.0, item.source.entity.0
                    )
                };
                (
                    caption,
                    Command::Remove {
                        generation,
                        id: item.id,
                    },
                    false,
                )
            } else {
                let item = &picker.sources[index];
                let kind = kinds
                    .iter()
                    .find(|(kind, _)| *kind == item.kind)
                    .map_or(item.kind.as_str(), |(_, label)| label.as_str());
                (
                    format!(
                        "{}:{} · {kind} · {}",
                        item.source.feature.0, item.entity, item.sketch
                    ),
                    Command::Add {
                        generation,
                        source: item.source,
                    },
                    picker
                        .references
                        .iter()
                        .any(|reference| reference.source == item.source),
                )
            };
            let mut control = InterfaceControl::button("sketch/projection", &caption);
            control.disabled = disabled;
            panel.widgets.button(
                world,
                camera,
                &format!("row-{row}"),
                control,
                None,
                NativeCommand::Sketch(EditorCommand::Projection(command)),
                rect(left + 10., top + 112. + row as f32 * 28., width - 20., 26.),
                None,
                41,
            )?;
        }
        for (key, label, start, disabled, x) in [
            (
                "previous",
                previous,
                picker.start.saturating_sub(per_page),
                picker.start == 0,
                left + 10.,
            ),
            (
                "next",
                next,
                picker.start + per_page,
                picker.start + per_page >= count,
                left + width / 3.,
            ),
        ] {
            let mut control = InterfaceControl::button("sketch/projection", &label);
            control.disabled = disabled;
            panel.widgets.button(
                world,
                camera,
                key,
                control,
                None,
                NativeCommand::Sketch(EditorCommand::Projection(Command::Page {
                    generation,
                    start,
                })),
                rect(x, top + height - 34., width / 3. - 12., 26.),
                None,
                41,
            )?;
        }
        panel.widgets.button(
            world,
            camera,
            "close",
            InterfaceControl::button("sketch/projection", &close),
            None,
            NativeCommand::Sketch(EditorCommand::Projection(Command::Close)),
            rect(
                left + width * 2. / 3.,
                top + height - 34.,
                width / 3. - 10.,
                26.,
            ),
            None,
            41,
        )?;
        Ok(())
    })();
    panel.widgets.finish(world);
    world.insert_resource(panel);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_preferences::Locale;
    use limo_cad_core::FeatureId;
    use limo_cad_sketch::EntityId;

    fn editor() -> Editor {
        Editor {
            stamp: Some(Stamp {
                owner: DocumentContext {
                    window_id: "projection-test".into(),
                    document_id: "owned-scratch".into(),
                    epoch: 1,
                },
                revision: 7,
                sketch: Some("Sketch2".into()),
                basis: None,
            }),
            projection: Some(Picker {
                generation: 3,
                sources: (1..=30)
                    .map(|id| Source {
                        source: SketchCurveRef {
                            feature: FeatureId(1),
                            entity: EntityId(id),
                        },
                        sketch: "Sketch1".into(),
                        kind: "line".into(),
                        entity: id,
                    })
                    .collect(),
                references: vec![],
                existing: false,
                start: 0,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn projection_picker_retires_on_every_owner_revision_or_sketch_change() {
        for change in 0..4 {
            let mut editor = editor();
            let mut stamp = editor.stamp.clone().unwrap();
            match change {
                0 => stamp.owner.epoch += 1,
                1 => stamp.owner.document_id = "other".into(),
                2 => stamp.revision += 1,
                _ => stamp.sketch = Some("Sketch3".into()),
            }
            assert!(synchronize_stamp(&mut editor, stamp));
            assert!(editor.projection.is_none());
        }
        let mut editor = editor();
        let stamp = editor.stamp.clone().unwrap();
        assert!(!synchronize_stamp(&mut editor, stamp));
        assert!(editor.projection.is_some());
    }

    #[test]
    fn projection_picker_rebinds_paged_rows_and_retires_all_controls() {
        let mut world = World::new();
        world.init_resource::<ViewportUiAssets>();
        let camera = world.spawn_empty().id();
        let mut editor = editor();
        let canvas = InterfaceRect {
            x: 64.,
            y: 120.,
            width: 800.,
            height: 500.,
        };
        synchronize(&mut world, camera, &editor, canvas).unwrap();
        let row = world.resource::<Panel>().widgets.entity("row-0").unwrap();
        let first = world.get::<InterfaceControl>(row).unwrap().binding;
        editor.projection.as_mut().unwrap().start = 7;
        synchronize(&mut world, camera, &editor, canvas).unwrap();
        assert_eq!(world.resource::<Panel>().widgets.entity("row-0"), Some(row));
        let control = world.get::<InterfaceControl>(row).unwrap();
        assert_ne!(control.binding, first);
        assert!(control.label.starts_with("1:8 ·"));
        editor.projection = None;
        synchronize(&mut world, camera, &editor, canvas).unwrap();
        assert_eq!(world.query::<&InterfaceControl>().iter(&world).count(), 0);
    }

    #[test]
    fn projection_controls_fit_minimum_scaled_canvas_in_four_locales() {
        let pixels = |value| match value {
            Val::Px(value) => value,
            _ => panic!("Expected pixels"),
        };
        for locale in [Locale::En, Locale::Es, Locale::De, Locale::ZhCn] {
            for scale in [1., 1.75] {
                let mut world = World::new();
                world.init_resource::<ViewportUiAssets>();
                localization::set_locale(&mut world, locale);
                let camera = world.spawn_empty().id();
                let editor = editor();
                let canvas = InterfaceRect {
                    x: 64.,
                    y: 120.,
                    width: 1200. / scale - 76.,
                    height: 760. / scale - 168.,
                };
                synchronize(&mut world, camera, &editor, canvas).unwrap();
                for (control, node) in world.query::<(&InterfaceControl, &Node)>().iter(&world) {
                    assert!(
                        pixels(node.left) >= canvas.x as f32 && pixels(node.top) >= canvas.y as f32,
                        "{}",
                        control.label
                    );
                    assert!(
                        pixels(node.left) + pixels(node.width) <= (canvas.x + canvas.width) as f32,
                        "{}",
                        control.label
                    );
                    assert!(
                        pixels(node.top) + pixels(node.height) <= (canvas.y + canvas.height) as f32,
                        "{}",
                        control.label
                    );
                    assert!(!control.label.starts_with("projection."));
                }
            }
        }
    }
}
