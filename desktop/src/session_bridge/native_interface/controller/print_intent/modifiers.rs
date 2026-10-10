//! Local manufacturing zones share the Print Settings draft and document fence.
use super::*;
use crate::native_forms::{DimensionKind, MeasurementInput};
use crate::native_viewport::{ViewportLineLayer, ViewportPreview, ViewportTriangleLayer};
#[cfg(test)]
mod tests;
use limo_cad_core::{BodyId, PrintModifierDto, PrintModifierPrimitiveDto, UnitSystem};
use limo_cad_sketch::AssemblyTransformDto;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Saved,
    Name,
    Enabled,
    Shape,
    Size(usize),
    Radius,
    Height,
    Translation(usize),
    Rotation(usize),
    CopyTarget,
    Overlays,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Create,
    Delete,
    Copy,
}
pub(super) struct Draft {
    modifier: PrintModifierDto,
    placement: assembly::TransformDraft,
    dimensions: [MeasurementInput; 5],
    changed: bool,
}
impl Draft {
    fn new(modifier: PrintModifierDto, units: UnitSystem) -> Self {
        let values = match modifier.primitive {
            PrintModifierPrimitiveDto::Box { size_mm: [x, y, z] } => [x, y, z, x / 2., z],
            PrintModifierPrimitiveDto::Cylinder {
                radius_mm: r,
                height_mm: h,
            } => [2. * r, 2. * r, h, r, h],
        };
        Self {
            placement: assembly::TransformDraft::new(
                AssemblyTransformDto {
                    translation: modifier.local_pose.translation_mm,
                    rotation: modifier.local_pose.rotation,
                },
                units,
            ),
            modifier,
            dimensions: values.map(|v| MeasurementInput::new(DimensionKind::Length, v, units)),
            changed: false,
        }
    }
    fn value(
        &self,
        settings: &PrintSettingsDto,
        units: UnitSystem,
    ) -> Result<PrintModifierDto, String> {
        let mut value = self.modifier.clone();
        let pose = self.placement.value(units)?;
        value.local_pose.translation_mm = pose.translation;
        value.local_pose.rotation = pose.rotation;
        value.primitive = match value.primitive {
            PrintModifierPrimitiveDto::Box { .. } => PrintModifierPrimitiveDto::Box {
                size_mm: [
                    self.dimensions[0].evaluate(units, &[])?,
                    self.dimensions[1].evaluate(units, &[])?,
                    self.dimensions[2].evaluate(units, &[])?,
                ],
            },
            PrintModifierPrimitiveDto::Cylinder { .. } => PrintModifierPrimitiveDto::Cylinder {
                radius_mm: self.dimensions[3].evaluate(units, &[])?,
                height_mm: self.dimensions[4].evaluate(units, &[])?,
            },
        };
        value.settings = settings.clone();
        value.validate()?;
        Ok(value)
    }
}
#[derive(Default)]
pub(super) struct Overlays {
    original: Option<Arc<ViewportPreview>>,
    revision: Option<u64>,
    key: String,
    hidden: bool,
    labels: Vec<([f32; 2], String)>,
}
pub(super) fn dirty(state: &State) -> bool {
    state.modifier_draft.as_ref().is_some_and(|d| d.changed)
}
pub(super) fn canonical(
    state: &mut State,
    document: &PrintIntentDocumentDto,
) -> Option<PrintModifierDto> {
    if !state.modifier_scope {
        return None;
    }
    if state.modifier_selection.is_empty() {
        if let Some(zone) = document
            .modifiers
            .iter()
            .find(|m| m.body_id.0 == state.body)
        {
            state.modifier_selection = zone.id.clone();
        }
    }
    document
        .modifiers
        .iter()
        .find(|m| m.id == state.modifier_selection && m.body_id.0 == state.body)
        .cloned()
}
pub(super) fn accept(state: &mut State, value: Option<PrintModifierDto>) {
    state.modifier_original = value.clone();
    state.modifier_draft = value.map(|v| Draft::new(v, state.units));
}
pub(super) fn field_key(field: Field) -> &'static str {
    match field {
        Field::Saved => "modifier",
        Field::Name => "modifier_name",
        Field::Enabled => "modifier_enabled",
        Field::Shape => "modifier_shape",
        Field::Size(0) => "modifier_size_x",
        Field::Size(1) => "modifier_size_y",
        Field::Size(_) => "modifier_size_z",
        Field::Radius => "modifier_radius",
        Field::Height => "modifier_height",
        Field::Translation(0) => "modifier_x",
        Field::Translation(1) => "modifier_y",
        Field::Translation(_) => "modifier_z",
        Field::Rotation(0) => "modifier_rx",
        Field::Rotation(1) => "modifier_ry",
        Field::Rotation(_) => "modifier_rz",
        Field::CopyTarget => "modifier_copy_target",
        Field::Overlays => "modifier_overlays",
    }
}
pub(super) fn choices(world: &World, state: &State, field: Field) -> Option<Vec<ChoiceOption>> {
    Some(match field {
        Field::Saved => std::iter::once(option("", "No saved local modifier"))
            .chain(
                state
                    .document
                    .as_ref()?
                    .modifiers
                    .iter()
                    .filter(|m| m.body_id.0 == state.body)
                    .map(|m| {
                        option(
                            &m.id,
                            format!(
                                "{} · {}",
                                m.name,
                                if m.enabled { "enabled" } else { "disabled" }
                            ),
                        )
                    }),
            )
            .collect(),
        Field::Enabled | Field::Overlays => {
            vec![option("true", "Enabled"), option("false", "Disabled")]
        }
        Field::Shape => vec![option("box", "Box"), option("cylinder", "Cylinder")],
        Field::CopyTarget => body_choices(world, state),
        _ => return None,
    })
}
pub(super) fn text(state: &State, field: Field) -> String {
    if let Some((text, _)) = state.errors.get(field_key(field)) {
        return text.clone();
    }
    match field {
        Field::Saved => return state.modifier_selection.clone(),
        Field::CopyTarget => return state.modifier_copy_target.clone(),
        Field::Overlays => return (!state.modifier_overlays.hidden).to_string(),
        _ => {}
    }
    let Some(draft) = &state.modifier_draft else {
        return String::new();
    };
    match field {
        Field::Name => draft.modifier.name.clone(),
        Field::Enabled => draft.modifier.enabled.to_string(),
        Field::Shape => match draft.modifier.primitive {
            PrintModifierPrimitiveDto::Box { .. } => "box",
            PrintModifierPrimitiveDto::Cylinder { .. } => "cylinder",
        }
        .into(),
        Field::Size(i) => draft.dimensions[i].text().into(),
        Field::Radius => draft.dimensions[3].text().into(),
        Field::Height => draft.dimensions[4].text().into(),
        Field::Translation(i) => draft.placement.translation[i].text().into(),
        Field::Rotation(i) => draft.placement.rotation[i].text().into(),
        _ => String::new(),
    }
}
pub(super) fn edit(
    state: &mut State,
    field: Field,
    value: &str,
    units: UnitSystem,
) -> Result<Value, String> {
    match field {
        Field::Saved => {
            if state.dirty() {
                return Err(
                    "Apply or discard the local modifier draft before selecting another zone"
                        .into(),
                );
            }
            let document = state.document.clone().ok_or("Wait for print settings")?;
            if !value.is_empty()
                && !document
                    .modifiers
                    .iter()
                    .any(|m| m.id == value && m.body_id.0 == state.body)
            {
                return Err("Select a saved local modifier belonging to this part".into());
            }
            state.modifier_selection = value.into();
            let modifier = canonical(state, &document);
            state.draft = modifier
                .as_ref()
                .map(|m| m.settings.clone())
                .unwrap_or_default();
            state.original = state.draft.clone();
            accept(state, modifier);
            state.errors.clear();
            state.error = None;
            return Ok(json!({"selected":value}));
        }
        Field::CopyTarget => {
            state.modifier_copy_target = value.into();
            return Ok(json!({"handled":true}));
        }
        Field::Overlays => {
            state.modifier_overlays.hidden = value == "false";
            return Ok(json!({"handled":true}));
        }
        _ => {}
    }
    let draft = state
        .modifier_draft
        .as_mut()
        .ok_or("Create or select a local modifier")?;
    match field {
        Field::Name => draft.modifier.name = value.into(),
        Field::Enabled => draft.modifier.enabled = value == "true",
        Field::Shape => {
            draft.modifier.primitive = if value == "box" {
                PrintModifierPrimitiveDto::Box { size_mm: [10.; 3] }
            } else {
                PrintModifierPrimitiveDto::Cylinder {
                    radius_mm: 5.,
                    height_mm: 10.,
                }
            }
        }
        Field::Size(i) => draft.dimensions[i].set_text(value.into()),
        Field::Radius => draft.dimensions[3].set_text(value.into()),
        Field::Height => draft.dimensions[4].set_text(value.into()),
        Field::Translation(i) => draft.placement.translation[i].set_text(value.into()),
        Field::Rotation(i) => {
            draft.placement.rotation[i].set_text(value.into());
            draft.placement.exact_rotation = None
        }
        _ => unreachable!(),
    }
    draft.changed = true;
    state.errors.retain(|key, _| !key.starts_with("modifier_"));
    if let Err(error) = draft.value(&state.draft, units) {
        state
            .errors
            .insert(field_key(field), (value.into(), error.clone()));
        return Ok(json!({"handled":true,"valid":false,"error":error}));
    }
    state.errors.remove(field_key(field));
    state.error = None;
    Ok(json!({"handled":true,"valid":true}))
}
pub(super) fn create(world: &mut World, engine: &AppState) -> Result<Value, String> {
    let state = world.resource::<State>();
    if !state.modifier_scope {
        return Err("Choose Local modifiers scope first".into());
    }
    let body = state.body;
    let mesh = native_viewport::interface_geometry(world)
        .scene
        .bodies
        .iter()
        .find(|b| b.id.0 == body)
        .ok_or("Choose a live source body")?;
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for p in mesh.mesh.positions.as_chunks::<3>().0 {
        for i in 0..3 {
            min[i] = min[i].min(p[i] as f64);
            max[i] = max[i].max(p[i] as f64)
        }
    }
    let translation_mm = std::array::from_fn(|i| (min[i] + max[i]) / 2.);
    let size_mm = std::array::from_fn(|i| ((max[i] - min[i]) * 0.35).max(0.1));
    let value = PrintModifierDto {
        id: uuid::Uuid::new_v4().to_string(),
        name: "Local print modifier".into(),
        body_id: BodyId(body),
        enabled: true,
        local_pose: limo_cad_core::PrintLocalPoseDto {
            translation_mm,
            rotation: [0., 0., 0., 1.],
        },
        primitive: PrintModifierPrimitiveDto::Box { size_mm },
        settings: Default::default(),
    };
    value.validate()?;
    let mut state = world.resource_mut::<State>();
    state.modifier_selection = value.id.clone();
    state.modifier_original = None;
    state.modifier_draft = Some(Draft::new(value, engine.document_units()));
    state.modifier_draft.as_mut().unwrap().changed = true;
    state.draft = Default::default();
    state.original = Default::default();
    state.errors.clear();
    state.error = None;
    Ok(json!({"draft":true,"modifier_id":state.modifier_selection}))
}
pub(super) fn write(
    state: &State,
    command: &super::Command,
    units: UnitSystem,
) -> Result<(&'static str, Value), String> {
    let draft = state
        .modifier_draft
        .as_ref()
        .ok_or("Create or select a local modifier")?;
    match command {
        super::Command::Apply => {
            let modifier = draft.value(&state.draft, units)?;
            Ok((
                if state.modifier_original.is_some() {
                    "print_modifier_update"
                } else {
                    "print_modifier_create"
                },
                json!({"modifier":modifier}),
            ))
        }
        super::Command::Inherit => {
            if state.modifier_original.is_none() {
                return Err("Save the new modifier before resetting its requests".into());
            }
            Ok(("print_modifier_reset", json!({"id":draft.modifier.id})))
        }
        super::Command::Modifier(Command::Delete) => {
            if state.dirty() {
                return Err(
                    "Apply or discard the modifier draft before deleting its saved zone".into(),
                );
            }
            Ok(("print_modifier_remove", json!({"id":draft.modifier.id})))
        }
        super::Command::Modifier(Command::Copy) => {
            if state.dirty() {
                return Err(
                    "Apply or discard the modifier draft before copying its saved zone".into(),
                );
            }
            let target = state
                .modifier_copy_target
                .parse::<u64>()
                .map_err(|_| "Choose the destination source body")?;
            Ok((
                "print_modifier_copy",
                json!({"source_id":draft.modifier.id,"target_body_id":target}),
            ))
        }
        _ => Err("Unknown local modifier command".into()),
    }
}
pub(super) fn clear_overlay(world: &mut World) -> Result<(), String> {
    let mut state = world.remove_resource::<State>().unwrap_or_default();
    let result = restore_overlay(world, &mut state);
    world.insert_resource(state);
    result
}
pub(super) fn restore_overlay(world: &mut World, state: &mut State) -> Result<(), String> {
    state.modifier_overlays.labels.clear();
    if let Some(original) = state.modifier_overlays.original.take() {
        if state.modifier_overlays.revision
            == Some(native_viewport::interface_preview_revision(world))
        {
            if let Some(owner) = &state.owner {
                if native_viewport::interface_camera_snapshot(world).0 == owner.document_id {
                    native_viewport::apply_interface_preview(world, &owner.document_id, original)?;
                }
            }
        }
    }
    state.modifier_overlays.revision = None;
    state.modifier_overlays.key.clear();
    Ok(())
}
pub(super) fn synchronize_overlay(
    world: &mut World,
    state: &mut State,
    owner: &DocumentContext,
) -> Result<(), String> {
    if state.modifier_overlays.hidden {
        return restore_overlay(world, state);
    }
    let mut zones: Vec<_> = state
        .document
        .as_ref()
        .map(|d| {
            d.modifiers
                .iter()
                .filter(|m| m.body_id.0 == state.body)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    if state.modifier_scope {
        if let Some(draft) = &state.modifier_draft {
            if let Ok(value) = draft.value(&state.draft, state.units) {
                zones.retain(|m| m.id != value.id);
                zones.push(value);
            }
        }
    }
    if zones.is_empty() {
        return restore_overlay(world, state);
    }
    let (_, camera) = native_viewport::interface_camera_snapshot(world);
    let key=serde_json::to_string(&json!({"zones":zones,"stamp":native_viewport::interface_navigation_source(world).0,"camera":camera,"selected":state.modifier_selection,"draft":state.dirty()})).map_err(|e|e.to_string())?;
    if state.modifier_overlays.revision != Some(native_viewport::interface_preview_revision(world))
    {
        state.modifier_overlays.original = None;
        state.modifier_overlays.key.clear();
    }
    if state.modifier_overlays.key == key {
        return Ok(());
    }
    let original = state
        .modifier_overlays
        .original
        .get_or_insert_with(|| native_viewport::interface_preview_snapshot(world))
        .clone();
    let mut preview = original.as_ref().clone();
    let mut labels = vec![];
    for modifier in zones {
        let (vertices, edges) = primitive(&modifier.primitive);
        let local = Transform::from_translation(Vec3::from_array(
            modifier.local_pose.translation_mm.map(|v| v as f32),
        ))
        .with_rotation(Quat::from_array(
            modifier.local_pose.rotation.map(|v| v as f32),
        ));
        for occurrence in native_viewport::interface_visible_occurrences(world, modifier.body_id.0)
        {
            let transform =
                native_viewport::interface_body_transform(world, modifier.body_id.0, occurrence)
                    * local;
            let color = if !modifier.enabled {
                [0.6, 0.6, 0.6, 0.16]
            } else if modifier.id == state.modifier_selection {
                [1., 0.67, 0.2, 0.2]
            } else {
                [0.3, 0.75, 1., 0.16]
            };
            preview.triangles.push(ViewportTriangleLayer {
                color,
                positions: vertices
                    .iter()
                    .flat_map(|p| transform.transform_point(Vec3::from_array(*p)).to_array())
                    .collect::<Vec<_>>()
                    .into(),
                xray: true,
                ..Default::default()
            });
            preview.lines.push(ViewportLineLayer {
                color: [color[0], color[1], color[2], 0.9],
                width: 1.5,
                segments: edges
                    .iter()
                    .flat_map(|p| transform.transform_point(Vec3::from_array(*p)).to_array())
                    .collect::<Vec<_>>()
                    .into(),
                ..Default::default()
            });
            let center = transform.translation.to_array().map(|v| v as f64);
            if let Some(screen) =
                native_viewport::interface_world_point(world, &owner.document_id, center)?
            {
                labels.push((
                    screen,
                    format!(
                        "{} · {}{}",
                        modifier.name,
                        occurrence
                            .map(|id| format!("occurrence {id}"))
                            .unwrap_or("source body".into()),
                        if !modifier.enabled {
                            " · disabled"
                        } else if state.dirty() && modifier.id == state.modifier_selection {
                            " · draft"
                        } else {
                            ""
                        }
                    ),
                ));
            }
        }
    }
    native_viewport::apply_interface_preview(world, &owner.document_id, preview)?;
    state.modifier_overlays.labels = labels;
    state.modifier_overlays.key = key;
    state.modifier_overlays.revision = Some(native_viewport::interface_preview_revision(world));
    Ok(())
}

pub(super) fn paint_labels(world: &mut World, camera: Entity, state: &mut State) {
    let Some(bounds) = world
        .get_resource::<native_viewport::interface_shell::NativeInterfaceHandle>()
        .and_then(|handle| {
            handle
                .read_surface(|owner, frame| {
                    state.owner.as_ref().filter(|current| *current == owner)?;
                    frame
                        .canvases
                        .iter()
                        .find(|canvas| canvas.name == "viewport")
                        .map(|canvas| canvas.bounds)
                })
                .ok()
                .flatten()
        })
    else {
        return;
    };
    for (index, (pixel, text)) in state.modifier_overlays.labels.iter().enumerate() {
        let x = bounds.x as f32 + pixel[0];
        let y = bounds.y as f32 + pixel[1];
        if x < bounds.x as f32
            || x > (bounds.x + bounds.width) as f32
            || y < bounds.y as f32
            || y > (bounds.y + bounds.height) as f32
        {
            continue;
        }
        let width = (text.chars().count() as f32 * 6. + 12.).clamp(100., 320.);
        let left = (x - width / 2.).clamp(
            bounds.x as f32,
            ((bounds.x + bounds.width) as f32 - width).max(bounds.x as f32),
        );
        let top = (y - 28.).max(bounds.y as f32);
        {
            let fill = native_viewport::ui::theme(world).panel.with_alpha(0.9);
            workbench::card(
                (&mut state.widgets, world, camera),
                &format!("modifier-label-background-{index}"),
                chrome::rect(left, top, width, 18.),
                fill,
                3.,
                47,
            )
        };
        state.widgets.text(
            world,
            camera,
            &format!("modifier-label-{index}"),
            chrome::rect(left + 4., top + 2., width - 8., 14.),
            text,
            10.,
            48,
        );
    }
}
fn primitive(shape: &PrintModifierPrimitiveDto) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let (ring, height): (Vec<[f32; 2]>, f32) = match shape {
        PrintModifierPrimitiveDto::Box { size_mm: [x, y, z] } => (
            vec![
                [-(*x as f32) / 2., -(*y as f32) / 2.],
                [*x as f32 / 2., -(*y as f32) / 2.],
                [*x as f32 / 2., *y as f32 / 2.],
                [-(*x as f32) / 2., *y as f32 / 2.],
            ],
            *z as f32,
        ),
        PrintModifierPrimitiveDto::Cylinder {
            radius_mm,
            height_mm,
        } => (
            (0..32)
                .map(|i| {
                    let angle = i as f32 * std::f32::consts::TAU / 32.;
                    [
                        angle.cos() * (*radius_mm as f32),
                        angle.sin() * (*radius_mm as f32),
                    ]
                })
                .collect(),
            *height_mm as f32,
        ),
    };
    let point = |i: usize, z: f32| [ring[i][0], ring[i][1], z];
    let mut triangles = vec![];
    let mut edges = vec![];
    for i in 0..ring.len() {
        let j = (i + 1) % ring.len();
        let a = point(i, -height / 2.);
        let b = point(j, -height / 2.);
        let c = point(j, height / 2.);
        let d = point(i, height / 2.);
        triangles.extend([
            a,
            b,
            c,
            a,
            c,
            d,
            [0., 0., -height / 2.],
            b,
            a,
            [0., 0., height / 2.],
            d,
            c,
        ]);
        edges.extend([a, b, c, d, a, d]);
    }
    (triangles, edges)
}

type Row = (
    String,
    Option<super::Field>,
    Option<super::Command>,
    Option<String>,
);
pub(super) fn rows(state: &State) -> Vec<Row> {
    let field = |label: &str, value: Field| {
        (
            label.into(),
            Some(super::Field::Modifier(value)),
            None,
            None,
        )
    };
    let command = |label: &str, value: Command| {
        (
            label.into(),
            None,
            Some(super::Command::Modifier(value)),
            None,
        )
    };
    let mut rows = vec![
        field("Local print modifier", Field::Saved),
        command("Create local print modifier", Command::Create),
        field("Show local modifier overlays", Field::Overlays),
    ];
    if let Some(draft) = &state.modifier_draft {
        rows.extend([
            field("Local modifier name", Field::Name),
            field("Local modifier enabled", Field::Enabled),
            field("Local modifier shape", Field::Shape),
        ]);
        match draft.modifier.primitive {
            PrintModifierPrimitiveDto::Box { .. } => {
                for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                    rows.push(field(&format!("Modifier box size {axis}"), Field::Size(i)))
                }
            }
            PrintModifierPrimitiveDto::Cylinder { .. } => rows.extend([
                field("Modifier cylinder radius", Field::Radius),
                field("Modifier cylinder height", Field::Height),
            ]),
        }
        for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
            rows.push(field(
                &format!("Modifier local offset {axis}"),
                Field::Translation(i),
            ));
            rows.push(field(
                &format!("Modifier local rotation {axis}"),
                Field::Rotation(i),
            ));
        }
        rows.extend([
            field("Copy modifier to part", Field::CopyTarget),
            command("Copy local print modifier", Command::Copy),
            command("Delete local print modifier", Command::Delete),
        ]);
    }
    rows.push((
        "Local modifier coordinates".into(),
        None,
        None,
        Some(
            "Centered shapes in source-body coordinates; every occurrence inherits the zone."
                .into(),
        ),
    ));
    rows.push((
        "Local modifier preview meaning".into(),
        None,
        None,
        Some(
            "Requested translucent zones. Bambu preview checks actual material intersections."
                .into(),
        ),
    ));
    rows
}
pub(super) fn disabled(
    state: &State,
    field: Option<super::Field>,
    command: Option<super::Command>,
) -> bool {
    if !state.modifier_scope {
        return false;
    }
    matches!(
        command,
        Some(super::Command::Modifier(Command::Copy | Command::Delete))
    ) && (state.modifier_original.is_none() || state.dirty())
        || matches!(field, Some(super::Field::Modifier(Field::Saved))) && state.dirty()
}
pub(super) fn report_rows(state: &State, rows: &mut Vec<Row>) {
    let Some(report) = state.effective["modifiers"].as_array().and_then(|ms| {
        ms.iter()
            .find(|m| m["modifier"]["id"] == state.modifier_selection)
    }) else {
        return;
    };
    for (label, value) in [
        (
            "Local modifier inherited occurrences",
            report["occurrence_ids"].to_string(),
        ),
        (
            "Local modifier conservative bounds",
            report["local_bounds"].to_string(),
        ),
    ] {
        rows.push((label.into(), None, None, Some(value)));
    }
    if let Some(warnings) = report["warnings"].as_array() {
        for (i, warning) in warnings.iter().enumerate() {
            rows.push((
                format!("Local modifier warning {}", i + 1),
                None,
                None,
                Some(warning.as_str().unwrap_or("").into()),
            ));
        }
    }
    rows.push(("Unsupported local controls".into(),None,None,Some("Speed, ironing, support, material, layer height and compensation are unsupported here.".into())));
}
