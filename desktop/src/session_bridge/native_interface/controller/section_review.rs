//! Disposable geometric section review. Source definitions, never exploded poses.
//! Copied-body preview work stays off the live engine; results are draft fenced.
use super::*;
use crate::native_forms::{DimensionKind, MeasurementInput};
use bevy::{
    asset::RenderAssetUsages,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use limo_cad_core::{BodyId, UnitSystem};
use limo_cad_interface::{ChoiceOption, Field as ControlField, KeyChord};
use limo_cad_occt::section_review::{
    SectionOutcome, SectionPlane, SectionReview, SectionReviewRequest,
};
mod preview;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Body,
    Plane,
    Offset,
    Position,
    Probe,
    Side,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Open,
    Field(Field),
    CopySvg,
    Close,
    ToggleView,
    Bodies,
    ToggleBody(u64),
    AllBodies(bool),
    BodyPage(bool),
}
#[derive(Resource, Default)]
struct State {
    owner: Option<DocumentContext>,
    revision: u64,
    // Draft edits invalidate analysis without replacing a retained editor's
    // semantic binding while a blur commit is being reduced.
    generation: u64,
    controls_generation: u64,
    visible: bool,
    bodies: Vec<(u64, String)>,
    body: String,
    choosing_bodies: bool,
    body_page: usize,
    plane: String,
    offset: String,
    probe: String,
    side: String,
    show_diagram: bool,
    cutaway: Option<Arc<native_viewport::section_view::PreparedCutaway>>,
    report: Option<SectionReview>,
    image: Option<Handle<Image>>,
    prepared_image: Option<Image>,
    finished_generation: Option<u64>,
    preview: Option<preview::Worker>,
    bounds: Option<([f64; 3], [f64; 3])>,
    units: UnitSystem,
    message: String,
    widgets: chrome::Widgets,
}
impl State {
    fn invalidate(&mut self, message: &str) {
        self.generation = self.generation.saturating_add(1);
        self.report = None;
        self.image = None;
        self.prepared_image = None;
        self.cutaway = None;
        self.finished_generation = None;
        self.message = message.into();
    }
    fn reset_controls(&mut self, message: &str) {
        self.controls_generation = self.controls_generation.saturating_add(1);
        self.invalidate(message);
    }
    fn controls_current(&self, owner: &DocumentContext, revision: u64, generation: u64) -> bool {
        self.visible
            && self.owner.as_ref() == Some(owner)
            && self.revision == revision
            && self.controls_generation == generation
    }
    fn current(&self, owner: &DocumentContext, revision: u64, generation: u64) -> bool {
        self.visible
            && self.owner.as_ref() == Some(owner)
            && self.revision == revision
            && self.generation == generation
    }
    fn set_value(&mut self, field: Field, value: String) {
        if self.value(field) == value {
            return;
        }
        match field {
            Field::Body => self.body = value,
            Field::Plane => self.plane = value,
            Field::Offset | Field::Position => self.offset = value,
            Field::Probe => self.probe = value,
            Field::Side => self.side = value,
        }
        if matches!(field, Field::Body | Field::Plane) {
            self.invalidate("Updating section…");
        } else {
            // Keep the last completed view during a drag. It cannot be copied
            // as the new draft. Compatible intermediate previews can update
            // the viewport while the latest position is still computing.
            self.generation = self.generation.saturating_add(1);
            self.finished_generation = None;
            self.message = "Updating section…".into();
        }
    }
    fn toggle_view(&mut self) {
        self.show_diagram = !self.show_diagram;
        self.controls_generation = self.controls_generation.saturating_add(1);
        // Existing geometry is still useful in the other view, but an older
        // query must not complete into the replacement presentation scope.
        self.generation = self.generation.saturating_add(1);
        self.finished_generation = None;
    }
    fn value(&self, field: Field) -> &str {
        match field {
            Field::Body => &self.body,
            Field::Plane => &self.plane,
            Field::Offset | Field::Position => &self.offset,
            Field::Probe => &self.probe,
            Field::Side => &self.side,
        }
    }
    fn plane_axis(&self) -> usize {
        match self.plane.as_str() {
            "xy" => 2,
            "xz" => 1,
            _ => 0,
        }
    }
    fn range(&self) -> Option<(f64, f64)> {
        let (min, max) = self.bounds?;
        let axis = self.plane_axis();
        (min[axis].is_finite() && max[axis].is_finite() && min[axis] < max[axis])
            .then_some((min[axis], max[axis]))
    }
    fn source(&mut self, scene: &limo_cad_solid::SolidSceneDto, recenter: bool) {
        let ids = self.body_ids().unwrap_or_default();
        self.bounds = {
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for point in scene
                .bodies
                .iter()
                .filter(|body| ids.contains(&body.id))
                .flat_map(|body| body.mesh.positions.as_chunks::<3>().0)
            {
                for axis in 0..3 {
                    min[axis] = min[axis].min(f64::from(point[axis]));
                    max[axis] = max[axis].max(f64::from(point[axis]));
                }
            }
            min.iter()
                .chain(&max)
                .all(|v| v.is_finite())
                .then_some((min, max))
        };
        if recenter {
            if let Some((min, max)) = self.range() {
                self.offset =
                    MeasurementInput::new(DimensionKind::Length, (min + max) * 0.5, self.units)
                        .text()
                        .into();
            }
        }
    }
    fn report_current(&self) -> bool {
        self.report.is_some() && self.finished_generation == Some(self.generation)
    }
    fn set_position(&mut self, mm: f64) -> Result<(), String> {
        if !mm.is_finite() {
            return Err("Choose a finite section position".into());
        }
        let (min, max) = self.range().ok_or("Section body has no usable range")?;
        let value = MeasurementInput::new(DimensionKind::Length, mm.clamp(min, max), self.units)
            .text()
            .into();
        self.set_value(Field::Position, value);
        Ok(())
    }
    fn accepts_preview(
        &self,
        owner: &DocumentContext,
        revision: u64,
        requests: &[SectionReviewRequest],
        diagram: bool,
    ) -> bool {
        if !self.visible
            || self.owner.as_ref() != Some(owner)
            || self.revision != revision
            || self.show_diagram != diagram
        {
            return false;
        }
        self.requests(self.units).is_ok_and(|current| {
            current.len() == requests.len()
                && current.iter().zip(requests).all(|(current, request)| {
                    current.body_id == request.body_id
                        && current.plane == request.plane
                        && current.keep_positive == request.keep_positive
                        && current.include_cutaway == request.include_cutaway
                })
        })
    }
    fn choices(&self, field: Field) -> Option<Vec<ChoiceOption>> {
        let items = match field {
            Field::Plane => vec![
                ("xy".into(), "XY - through Z".into()),
                ("xz".into(), "XZ - through Y".into()),
                ("yz".into(), "YZ - through X".into()),
            ],
            Field::Side => vec![
                ("negative".into(), "Below plane".into()),
                ("positive".into(), "Above plane".into()),
            ],
            _ => return None,
        };
        Some(
            items
                .into_iter()
                .map(|(value, label)| ChoiceOption {
                    value,
                    label,
                    disabled: false,
                })
                .collect(),
        )
    }
    fn request(&self, units: UnitSystem) -> Result<SectionReviewRequest, String> {
        let body_id = self.body_ids()?[0];
        let plane = match self.plane.as_str() {
            "xy" => SectionPlane::Xy,
            "xz" => SectionPlane::Xz,
            "yz" => SectionPlane::Yz,
            _ => return Err("Choose a section plane".into()),
        };
        let req = SectionReviewRequest {
            body_id,
            plane,
            offset_mm: coordinate(&self.offset, units)?,
            probe_mm: if self.probe.trim().is_empty() {
                None
            } else {
                Some(coordinate(&self.probe, units)?)
            },
            deflection_mm: 0.01,
            include_cutaway: true,
            keep_positive: self.side == "positive",
        };
        req.validate()?;
        Ok(req)
    }
    fn body_ids(&self) -> Result<Vec<BodyId>, String> {
        let ids: Vec<_> = self
            .body
            .split(',')
            .map(|id| id.parse::<u64>().map(BodyId))
            .collect::<Result<_, _>>()
            .map_err(|_| "Choose at least one body")?;
        if ids.is_empty()
            || ids.len() > 128
            || ids
                .iter()
                .any(|id| !self.bodies.iter().any(|(current, _)| *current == id.0))
        {
            return Err("Choose between 1 and 128 current bodies".into());
        }
        if ids.iter().collect::<std::collections::BTreeSet<_>>().len() != ids.len() {
            return Err("Choose each section body once".into());
        }
        Ok(ids)
    }
    fn requests(&self, units: UnitSystem) -> Result<Vec<SectionReviewRequest>, String> {
        let request = self.request(units)?;
        Ok(self
            .body_ids()?
            .into_iter()
            .map(|body_id| SectionReviewRequest {
                body_id,
                ..request.clone()
            })
            .collect())
    }
    fn toggle_body(&mut self, id: u64) -> Result<(), String> {
        if !self.bodies.iter().any(|(body, _)| *body == id) {
            return Err("Choose a current body".into());
        }
        let mut ids = self.body_ids().unwrap_or_default();
        if ids.contains(&BodyId(id)) {
            ids.retain(|body| body.0 != id);
        } else {
            if ids.len() >= 128 {
                return Err("Select up to 128 section bodies".into());
            }
            ids.push(BodyId(id));
        }
        let value = self
            .bodies
            .iter()
            .filter(|(id, _)| ids.contains(&BodyId(*id)))
            .map(|(id, _)| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        self.set_value(Field::Body, value);
        Ok(())
    }
}
fn coordinate(text: &str, units: UnitSystem) -> Result<f64, String> {
    let mut input = MeasurementInput::new(DimensionKind::Length, 0., units);
    input.set_text(text.into());
    input.evaluate(units, &[])
}

pub(crate) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    generation: u64,
    command: &Command,
) -> Result<Value, String> {
    handle.validate_action(action)?;
    let receipt = bridge.native_document_receipt(engine, &action.context)?;
    if *command == Command::Open {
        if !super::super::is_activation(&action.control.input) {
            return Err("Activate Section Analysis".into());
        }
        let scene = engine.solid_scene_snapshot();
        let selected = native_viewport::interface_view(world)
            .2
            .selected_body_ids
            .clone();
        let body = scene
            .bodies
            .iter()
            .find(|b| selected.contains(&b.id.0))
            .or(scene.bodies.first())
            .ok_or("Create a solid body first")?;
        let mut state = world.remove_resource::<State>().unwrap_or_default();
        state.preview = None;
        state.reset_controls("Updating section…");
        state.owner = Some(receipt.owner);
        state.revision = receipt.revision;
        state.visible = true;
        state.bodies = scene
            .bodies
            .iter()
            .map(|b| (b.id.0, b.name.clone()))
            .collect();
        state.body = scene
            .bodies
            .iter()
            .filter(|b| selected.contains(&b.id.0))
            .map(|b| b.id.0.to_string())
            .take(128)
            .collect::<Vec<_>>()
            .join(",");
        if state.body.is_empty() {
            state.body = body.id.0.to_string();
        }
        state.choosing_bodies = false;
        state.body_page = 0;
        state.plane = "yz".into();
        state.side = "negative".into();
        state.show_diagram = false;
        state.units = engine.document_units();
        state.offset = "0".into();
        state.source(&scene, true);
        state.probe.clear();
        world.insert_resource(state);
        workbench::execute(world, &workbench::Command::Dismiss)?;
        return Ok(json!({"opened":true}));
    }
    let state = world
        .get_resource::<State>()
        .ok_or("Open Section Analysis")?;
    if !state.controls_current(&action.context, receipt.revision, generation) {
        return Err("The section controls or source model changed; inspect again".into());
    }
    if let Command::Field(field) = command {
        let value = if let ControlInput::SetValue(value) = &action.control.input {
            value.clone()
        } else if let Some(options) = state.choices(*field) {
            workbench::cam::choose(&options, state.value(*field), &action.control.input)?
        } else {
            return Ok(json!({"focused":true}));
        };
        if value.len() > if *field == Field::Body { 2048 } else { 256 } {
            return Err("Section field is too long".into());
        }
        if let Some(options) = state.choices(*field) {
            if !options.iter().any(|o| o.value == value) {
                return Err("Choose a current section option".into());
            }
        }
        if *field == Field::Position {
            let mm = coordinate(&value, UnitSystem::Mm)?;
            world.resource_mut::<State>().set_position(mm)?;
            return Ok(json!({"handled":true}));
        }
        let mut state = world.resource_mut::<State>();
        let recenter = matches!(field, Field::Body | Field::Plane) && state.value(*field) != value;
        state.set_value(*field, value);
        if recenter {
            state.source(&engine.solid_scene_snapshot(), true);
        }
        return Ok(json!({"handled":true}));
    }
    if let Command::ToggleBody(id) = command {
        let mut state = world.resource_mut::<State>();
        let checked = state.body_ids().unwrap_or_default().contains(&BodyId(*id));
        let changed = if let ControlInput::SetValue(value) = &action.control.input {
            value
                .parse::<bool>()
                .map_err(|_| "Choose a checked or unchecked section body")?
                != checked
        } else if super::super::is_activation(&action.control.input) {
            true
        } else {
            return Ok(json!({"focused":true}));
        };
        if changed {
            state.toggle_body(*id)?;
            state.source(&engine.solid_scene_snapshot(), false);
        }
        return Ok(json!({"handled":true}));
    }
    if !super::super::is_activation(&action.control.input) {
        return Err("Activate a section control".into());
    }
    match command {
        Command::Bodies => {
            let mut state = world.resource_mut::<State>();
            state.choosing_bodies = !state.choosing_bodies;
            return Ok(json!({"handled":true}));
        }
        Command::AllBodies(all) => {
            let mut state = world.resource_mut::<State>();
            if *all && state.bodies.len() > 128 {
                return Err("Select up to 128 section bodies".into());
            }
            let value = if *all {
                state
                    .bodies
                    .iter()
                    .map(|(id, _)| id.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            } else {
                String::new()
            };
            state.set_value(Field::Body, value);
            state.source(&engine.solid_scene_snapshot(), false);
            return Ok(json!({"handled":true}));
        }
        Command::BodyPage(forward) => {
            let mut state = world.resource_mut::<State>();
            state.body_page = if *forward {
                state.body_page.saturating_add(1)
            } else {
                state.body_page.saturating_sub(1)
            };
            return Ok(json!({"handled":true}));
        }
        _ => {}
    }
    if *command == Command::Close {
        let mut state = world.resource_mut::<State>();
        state.visible = false;
        state.reset_controls("");
        state.preview = None;
        native_viewport::section_view::clear(world);
        return Ok(json!({"closed":true}));
    }
    if *command == Command::ToggleView {
        let mut state = world.resource_mut::<State>();
        state.toggle_view();
        return Ok(json!({"view":if state.show_diagram {"cutaway_and_diagram"} else {"cutaway"}}));
    }
    if *command == Command::CopySvg {
        let state = world.resource::<State>();
        let svg = state
            .report
            .as_ref()
            .filter(|r| state.report_current() && !r.svg.is_empty())
            .ok_or("Wait for a current nonempty section first")?
            .svg
            .clone();
        world
            .get_resource_mut::<bevy::clipboard::Clipboard>()
            .ok_or("Native clipboard unavailable")?
            .set_text(svg)
            .map_err(|e| format!("Cannot copy SVG: {e:?}"))?;
        world.resource_mut::<State>().message =
            "SVG copied. Paste into an SVG file to save this diagram.".into();
        return Ok(json!({"copied":true,"format":"svg"}));
    }
    Err("Unsupported section control".into())
}

fn rasterize(svg: &str) -> Result<Image, String> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    // usvg's default Arial/Times families need not exist on Linux. Resolve an
    // installed family explicitly, including the generic used by our SVGs.
    let families = [
        "Segoe UI",
        "Arial",
        "DejaVu Sans",
        "Noto Sans",
        "Liberation Sans",
    ];
    let family = families
        .iter()
        .find_map(|wanted| {
            options
                .fontdb
                .faces()
                .flat_map(|f| &f.families)
                .find(|(name, _)| name == wanted)
                .map(|(name, _)| name.clone())
        })
        .or_else(|| {
            options
                .fontdb
                .faces()
                .next()
                .and_then(|f| f.families.first())
                .map(|(n, _)| n.clone())
        })
        .ok_or("Install a system font to render section diagram labels")?;
    options.fontdb_mut().set_sans_serif_family(&family);
    options.font_family = family;
    let tree = resvg::usvg::Tree::from_str(svg, &options).map_err(|e| e.to_string())?;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(800, 600).ok_or("Unable to allocate section image")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    let mut rgba = Vec::with_capacity(800 * 600 * 4);
    for p in pixmap.pixels() {
        let p = p.demultiply();
        rgba.extend_from_slice(&[p.red(), p.green(), p.blue(), p.alpha()]);
    }
    Ok(Image::new(
        Extent3d {
            width: 800,
            height: 600,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ))
}

pub(crate) fn synchronize(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    owner: &DocumentContext,
    width: f32,
    height: f32,
) -> Result<(), String> {
    let mut state = world.remove_resource::<State>().unwrap_or_default();
    state.widgets.begin();
    let result = (|| {
        if state.owner.as_ref() != Some(owner) {
            state.visible = false;
            state.reset_controls("");
            state.preview = None;
            state.owner = Some(owner.clone());
        }
        if workbench::workspace(world) != workbench::Workspace::Solid {
            state.visible = false;
            state.reset_controls("");
        }
        if !state.visible {
            state.preview = None;
            native_viewport::section_view::clear(world);
            return Ok(());
        }
        let revision = services
            .bridge
            .native_document_receipt(&services.engine, owner)?
            .revision;
        if state.revision != revision {
            state.revision = revision;
            state.reset_controls("Updating section…");
            state.units = services.engine.document_units();
            let scene = services.engine.solid_scene_snapshot();
            state.bodies = scene
                .bodies
                .iter()
                .map(|b| (b.id.0, b.name.clone()))
                .collect();
            let previous = state.body.clone();
            state.body = previous
                .split(',')
                .filter(|id| {
                    state
                        .bodies
                        .iter()
                        .any(|(current, _)| current.to_string() == *id)
                })
                .collect::<Vec<_>>()
                .join(",");
            state.source(&scene, false);
        }
        if let Err(error) = preview::synchronize(world, &mut state, services, owner) {
            state.invalidate(&error);
            state.finished_generation = Some(state.generation);
            state.preview = None;
        }
        if let Some(image) = state.prepared_image.take() {
            world.init_resource::<Assets<Image>>();
            state.image = Some(world.resource_mut::<Assets<Image>>().add(image));
        }
        if let Some(report) = &state.report {
            native_viewport::section_view::show(
                world,
                &owner.document_id,
                &report.request,
                state.cutaway.clone(),
            )?;
        } else {
            native_viewport::section_view::clear(world);
        }
        paint(world, camera, &mut state, width, height)
    })();
    state.widgets.finish(world);
    world.insert_resource(state);
    result
}

pub(crate) fn escape(world: &mut World) -> bool {
    if let Some(mut state) = world.get_resource_mut::<State>().filter(|s| s.visible) {
        state.visible = false;
        state.reset_controls("");
        state.preview = None;
        native_viewport::section_view::clear(world);
        true
    } else {
        false
    }
}

pub(crate) fn modal(_world: &World) -> Option<&'static str> {
    None
}
pub(crate) fn active(world: &World) -> bool {
    world.get_resource::<State>().is_some_and(|s| s.visible)
}
#[cfg(test)]
fn in_3d(world: &World) -> bool {
    world.get_resource::<State>().is_some_and(|s| s.visible)
}

fn control(label: &str) -> InterfaceControl {
    InterfaceControl::button("document/section", label)
}

fn paint(
    world: &mut World,
    camera: Entity,
    state: &mut State,
    width: f32,
    height: f32,
) -> Result<(), String> {
    let w = (width - 24.).clamp(240., 312.);
    let x = (width - w - 16.).max(12.);
    let y = 130.;
    let rows = ((height - y - 392.) / 28.).clamp(1., 6.) as usize;
    let pages = state.bodies.len().div_ceil(rows).max(1);
    state.body_page = state.body_page.min(pages - 1);
    let shown = state
        .bodies
        .len()
        .saturating_sub(state.body_page * rows)
        .min(rows);
    let body_extra = if state.choosing_bodies {
        shown as f32 * 28. + 34. + if pages > 1 { 26. } else { 0. }
    } else {
        0.
    };
    let preview_height = if state.show_diagram && state.image.is_some() {
        ((w - 24.) * 0.75).min((height - y - 16. - 368. - body_extra).max(0.))
    } else {
        0.
    };
    let h = 356.
        + body_extra
        + if preview_height > 20. {
            preview_height + 12.
        } else {
            0.
        };
    let theme = native_viewport::ui::theme(world);
    state.widgets.panel(
        world,
        camera,
        "section-card",
        chrome::rect(x, y, w, h),
        theme.panel.with_alpha(1.),
        70,
    );
    state.widgets.text(
        world,
        camera,
        "section-title",
        chrome::rect(x + 12., y + 9., w - 62., 22.),
        "Section Analysis",
        14.,
        72,
    );
    state.widgets.button(
        world,
        camera,
        "section-close",
        control("Close section analysis"),
        Some("×"),
        NativeCommand::SectionReview(state.controls_generation, Command::Close),
        chrome::rect(x + w - 38., y + 8., 26., 24.),
        None,
        72,
    )?;
    let labels = match state.plane.as_str() {
        "xy" => ["X", "Y", "Z"],
        "xz" => ["X", "Z", "Y"],
        _ => ["Y", "Z", "X"],
    };
    state.widgets.text(
        world,
        camera,
        "section-label-0",
        chrome::rect(x + 12., y + 44., 72., 28.),
        "Bodies",
        11.,
        72,
    );
    let selected = state.body_ids().unwrap_or_default();
    let caption = if selected.len() == 1 {
        state
            .bodies
            .iter()
            .find(|(id, _)| *id == selected[0].0)
            .map(|(_, name)| format!("{name} ▾"))
            .unwrap_or_default()
    } else {
        format!("{} selected ▾", selected.len())
    };
    let mut bodies_control = control("Choose section bodies");
    bodies_control.expanded = Some(state.choosing_bodies);
    state.widgets.button(
        world,
        camera,
        "section-field-0",
        bodies_control,
        Some(&caption),
        NativeCommand::SectionReview(state.controls_generation, Command::Bodies),
        chrome::rect(x + 88., y + 44., w - 100., 28.),
        None,
        72,
    )?;
    if state.choosing_bodies {
        for (index, (id, name)) in state
            .bodies
            .iter()
            .skip(state.body_page * rows)
            .take(rows)
            .enumerate()
        {
            let checked = selected.contains(&BodyId(*id));
            let mut body = control(&format!("Include {name} (body {id}) in section"));
            body.role = "checkbox".into();
            body.field = ControlField::Toggle(checked);
            body.selected = Some(checked);
            let caption = format!("{} {name} (body {id})", if checked { "☑" } else { "☐" });
            state.widgets.button(
                world,
                camera,
                &format!("section-body-{id}"),
                body,
                Some(&caption),
                NativeCommand::SectionReview(state.controls_generation, Command::ToggleBody(*id)),
                chrome::rect(x + 12., y + 80. + index as f32 * 28., w - 24., 24.),
                None,
                72,
            )?;
        }
        let footer_y = y + 80. + shown as f32 * 28.;
        let third = (w - 32.) / 3.;
        for (index, key, caption, command) in [
            (0, "section-all", "Select all", Command::AllBodies(true)),
            (1, "section-none", "Clear", Command::AllBodies(false)),
            (2, "section-bodies-done", "Done", Command::Bodies),
        ] {
            let mut button = control(caption);
            button.disabled = index == 0 && state.bodies.len() > 128;
            state.widgets.button(
                world,
                camera,
                key,
                button,
                Some(caption),
                NativeCommand::SectionReview(state.controls_generation, command),
                chrome::rect(x + 12. + index as f32 * (third + 4.), footer_y, third, 24.),
                None,
                72,
            )?;
        }
        if pages > 1 {
            for (forward, key, caption, left) in [
                (false, "section-bodies-prev", "Previous", x + 12.),
                (true, "section-bodies-next", "Next", x + w * 0.5),
            ] {
                let mut button = control(caption);
                button.disabled = if forward {
                    state.body_page + 1 == pages
                } else {
                    state.body_page == 0
                };
                state.widgets.button(
                    world,
                    camera,
                    key,
                    button,
                    Some(caption),
                    NativeCommand::SectionReview(
                        state.controls_generation,
                        Command::BodyPage(forward),
                    ),
                    chrome::rect(left, footer_y + 26., w * 0.5 - 14., 24.),
                    None,
                    72,
                )?;
            }
        }
    }
    for (index, field, label, caption, row_y) in [
        (1, Field::Plane, "Section plane".into(), "Plane".into(), 80.),
        (
            2,
            Field::Offset,
            format!("{} offset (document units or explicit mm/in)", labels[2]),
            format!("Offset {}", labels[2]),
            116.,
        ),
        (
            4,
            Field::Side,
            "Retained side".into(),
            "Keep side".into(),
            200.,
        ),
        (
            3,
            Field::Probe,
            format!("Probe {} coordinate (optional)", labels[1]),
            format!("Probe {}", labels[1]),
            236.,
        ),
    ] {
        let row_y = row_y + body_extra;
        state.widgets.text(
            world,
            camera,
            &format!("section-label-{index}"),
            chrome::rect(x + 12., y + row_y, 72., 28.),
            &caption,
            11.,
            72,
        );
        let mut control = control(&label);
        let mut choice_caption = None;
        if let Some(options) = state.choices(field) {
            choice_caption = options
                .iter()
                .find(|o| o.value == state.value(field))
                .map(|o| o.label.clone());
            control.role = "combobox".into();
            control.owned_keys = [
                "ArrowUp",
                "ArrowDown",
                "ArrowLeft",
                "ArrowRight",
                "Home",
                "End",
            ]
            .map(KeyChord::plain)
            .into();
            control.field = ControlField::Choice {
                value: state.value(field).into(),
                options,
            };
        } else {
            control.field = ControlField::Text {
                value: state.value(field).into(),
                read_only: false,
                selection: None,
            };
        }
        state.widgets.button(
            world,
            camera,
            &format!("section-field-{index}"),
            control,
            choice_caption.as_deref(),
            NativeCommand::SectionReview(state.controls_generation, Command::Field(field)),
            chrome::rect(x + 88., y + row_y, w - 100., 28.),
            None,
            72,
        )?;
    }
    let range = state.range();
    let (min, max) = range.unwrap_or((-1., 1.));
    let mut slider = control(&format!("Section {} position slider (mm)", labels[2]));
    slider.disabled = range.is_none();
    slider.field = ControlField::Range {
        value: coordinate(&state.offset, state.units)
            .unwrap_or((min + max) * 0.5)
            .clamp(min, max),
        min,
        max,
        step: (max - min) / 400.,
    };
    state.widgets.button(
        world,
        camera,
        "section-slider",
        slider,
        None,
        NativeCommand::SectionReview(state.controls_generation, Command::Field(Field::Position)),
        chrome::rect(x + 12., y + 148. + body_extra, w - 24., 26.),
        None,
        72,
    )?;
    for (key, value, left) in [
        ("section-min", min, x + 12.),
        ("section-max", max, x + w * 0.5),
    ] {
        let text = if range.is_some() {
            let value = MeasurementInput::new(DimensionKind::Length, value, state.units);
            format!(
                "{} {}",
                value.text(),
                match state.units {
                    UnitSystem::Mm => "mm",
                    UnitSystem::Cm => "cm",
                    UnitSystem::In => "in",
                }
            )
        } else {
            "".into()
        };
        state.widgets.text(
            world,
            camera,
            key,
            chrome::rect(left, y + 174. + body_extra, w * 0.5 - 12., 16.),
            &text,
            9.,
            72,
        );
    }
    let half = (w - 30.) * 0.5;
    state.widgets.button(
        world,
        camera,
        "section-mode",
        control("Show or hide section diagram"),
        Some(if state.show_diagram {
            "Hide 2D diagram"
        } else {
            "Show 2D diagram"
        }),
        NativeCommand::SectionReview(state.controls_generation, Command::ToggleView),
        chrome::rect(x + 12., y + 274. + body_extra, half, 28.),
        None,
        72,
    )?;
    let mut copy = control("Copy section SVG");
    copy.disabled =
        !state.report_current() || state.report.as_ref().is_none_or(|r| r.svg.is_empty());
    state.widgets.button(
        world,
        camera,
        "section-copy",
        copy,
        Some("Copy SVG"),
        NativeCommand::SectionReview(state.controls_generation, Command::CopySvg),
        chrome::rect(x + 18. + half, y + 274. + body_extra, half, 28.),
        None,
        72,
    )?;
    state.widgets.text(
        world,
        camera,
        "section-message",
        chrome::rect(x + 12., y + 310. + body_extra, w - 24., 34.),
        &state.message,
        10.,
        72,
    );
    if preview_height > 20. {
        if let Some(image) = &state.image {
            let iw = preview_height * 4. / 3.;
            state.widgets.panel(
                world,
                camera,
                "section-preview",
                chrome::rect(x + (w - iw) / 2., y + 354. + body_extra, iw, preview_height),
                Color::WHITE,
                71,
            );
            if let Some(entity) = state.widgets.entity("section-preview") {
                world
                    .entity_mut(entity)
                    .insert(ImageNode::new(image.clone()));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "dev-ui-lab")]
pub(crate) fn paint_capture(
    world: &mut World,
    camera: Entity,
    engine: &AppState,
    width: f32,
    height: f32,
) -> Result<(), String> {
    let scene = engine.solid_scene_snapshot();
    if scene.bodies.is_empty() {
        return Err("Section capture needs a saved solid".into());
    }
    let session = engine.viewport_frame().session_id;
    let owner = DocumentContext {
        window_id: "section-lab".into(),
        document_id: session.clone(),
        epoch: 1,
    };
    let mut state = State {
        owner: Some(owner.clone()),
        visible: true,
        body: std::env::var("LIMO_CAD_SECTION_LAB_BODIES").unwrap_or_else(|_| {
            scene
                .bodies
                .iter()
                .map(|b| b.id.0.to_string())
                .collect::<Vec<_>>()
                .join(",")
        }),
        choosing_bodies: std::env::var_os("LIMO_CAD_SECTION_LAB_CHOOSER").is_some(),
        bodies: scene
            .bodies
            .iter()
            .map(|body| (body.id.0, body.name.clone()))
            .collect(),
        plane: std::env::var("LIMO_CAD_SECTION_LAB_PLANE").unwrap_or_else(|_| "xy".into()),
        side: "negative".into(),
        units: engine.document_units(),
        show_diagram: std::env::var_os("LIMO_CAD_SECTION_LAB_DIAGRAM").is_some(),
        ..Default::default()
    };
    state.source(&scene, true);
    if let Ok(offset) = std::env::var("LIMO_CAD_SECTION_LAB_OFFSET") {
        state.offset = offset;
    }
    let requests = state.requests(state.units)?;
    let source = engine.prepare_section_reviews(&requests)?;
    let prepared = preview::prepare(&source, &requests, state.show_diagram)?;
    engine.validate_section_reviews(&prepared.finished)?;
    let report = prepared.report;
    state.cutaway = prepared.cutaway;
    native_viewport::section_view::show(world, &session, &report.request, state.cutaway.clone())?;
    if let Some(image) = prepared.image {
        state.image = Some(world.resource_mut::<Assets<Image>>().add(image));
    }
    state.message = report
        .source_notice()
        .unwrap_or_else(|| "Drag the bar to move the cut. Orange faces show the section.".into());
    state.report = Some(report);
    state.finished_generation = Some(state.generation);
    let bounds = InterfaceRect {
        x: 0.,
        y: 0.,
        width: f64::from(width),
        height: f64::from(height),
    };
    world
        .resource::<NativeInterfaceHandle>()
        .present(InterfaceFrame {
            context: owner,
            client: bounds,
            surface: bounds,
            canvases: Vec::new(),
            surfaces: Vec::new(),
            modal_stack: Vec::new(),
            document_visible: true,
        })?;
    if let Some((min, max)) = state.bounds {
        let center = Vec3::from_array(min.map(|v| v as f32))
            .lerp(Vec3::from_array(max.map(|v| v as f32)), 0.5);
        let span =
            Vec3::from_array(max.map(|v| v as f32)) - Vec3::from_array(min.map(|v| v as f32));
        let mut view = native_viewport::interface_view_snapshot(world).1;
        let distance = span.length().max(1.)
            / (2. * (view.vertical_fov_degrees.to_radians() * 0.5).sin())
            * 1.15;
        view.target = center.to_array();
        view.position = (center + Vec3::new(0.7, -1., 0.8).normalize() * distance).to_array();
        view.up = [0., 0., 1.];
        native_viewport::apply_interface_view(world, &session, Some(view), None)?;
    }
    state.widgets.begin();
    paint(world, camera, &mut state, width, height)?;
    state.widgets.finish(world);
    world.insert_resource(state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multi_body_picker_has_independent_checkboxes_and_one_shared_slider_range() {
        let mut world = World::new();
        world.init_resource::<ViewportUiAssets>();
        let camera = world.spawn_empty().id();
        let mut state = State {
            visible: true,
            choosing_bodies: true,
            body: "1".into(),
            bodies: vec![(1, "Upper".into()), (2, "Lower".into()), (3, "Pin".into())],
            plane: "xy".into(),
            offset: "1".into(),
            side: "negative".into(),
            ..Default::default()
        };
        let mut scene = limo_cad_solid::SolidSceneDto::default();
        for (id, z) in [(1, [0., 8.]), (2, [-12., 0.]), (3, [-2., 9.])] {
            let mut body = limo_cad_solid::BodyDto {
                id: BodyId(id),
                topology_signature: String::new(),
                display_warnings: vec![],
                name: format!("Body{id}"),
                feature_id: limo_cad_core::FeatureId(id),
                faces: vec![],
                edges: vec![],
                mesh: limo_cad_solid::MeshDto {
                    positions: vec![],
                    normals: vec![],
                    indices: vec![],
                },
            };
            body.mesh.positions = vec![0., 0., z[0], 20., 12., z[1]];
            scene.bodies.push(body);
        }
        state.source(&scene, false);
        assert_eq!(state.range(), Some((0., 8.)));
        state.toggle_body(2).unwrap();
        state.source(&scene, false);
        assert_eq!(state.range(), Some((-12., 8.)));
        let requests = state.requests(UnitSystem::Mm).unwrap();
        assert_eq!(
            requests.iter().map(|r| r.body_id.0).collect::<Vec<_>>(),
            [1, 2]
        );
        assert!(requests.iter().all(|r| r.offset_mm == 1.));
        state.widgets.begin();
        paint(&mut world, camera, &mut state, 1200., 900.).unwrap();
        state.widgets.finish(&mut world);
        for (id, checked) in [(1, true), (2, true), (3, false)] {
            let entity = state.widgets.entity(&format!("section-body-{id}")).unwrap();
            let control = world.get::<InterfaceControl>(entity).unwrap();
            assert_eq!(control.role, "checkbox");
            assert_eq!(control.field, ControlField::Toggle(checked));
            assert!(!control.disabled);
        }
        let card = world
            .get::<Node>(state.widgets.entity("section-card").unwrap())
            .unwrap();
        assert_eq!(card.width, px(312.));
        state.toggle_body(1).unwrap();
        state.toggle_body(2).unwrap();
        assert!(state.requests(UnitSystem::Mm).is_err());
        state.toggle_body(3).unwrap();
        assert_eq!(state.body_ids().unwrap(), [BodyId(3)]);
        assert!(state.toggle_body(999).is_err());
    }
    #[test]
    fn compact_right_panel_has_a_live_slider_and_preserves_inch_offset_precision() {
        let mut world = World::new();
        world.init_resource::<ViewportUiAssets>();
        let camera = world.spawn_empty().id();
        let mut state = State {
            visible: true,
            show_diagram: false,
            body: "1".into(),
            plane: "xy".into(),
            side: "negative".into(),
            bodies: vec![(1, "Fixture".into())],
            units: UnitSystem::In,
            offset: "1".into(),
            bounds: Some(([-20., -10., 0.], [20., 10., 50.8])),
            ..Default::default()
        };
        state.widgets.begin();
        paint(&mut world, camera, &mut state, 1200., 800.).unwrap();
        state.widgets.finish(&mut world);
        let card = world
            .get::<Node>(state.widgets.entity("section-card").unwrap())
            .unwrap();
        assert_eq!(card.left, px(872.));
        assert_eq!(card.width, px(312.));
        assert_eq!(card.height, px(356.));
        let slider = state.widgets.entity("section-slider").unwrap();
        let offset = state.widgets.entity("section-field-2").unwrap();
        let binding = world.get::<InterfaceControl>(offset).unwrap().binding;
        assert!(
            matches!(world.get::<InterfaceControl>(slider).unwrap().field,
            ControlField::Range { value, min: 0., max: 50.8, .. } if value == 25.4)
        );
        assert!(!world.get::<InterfaceControl>(slider).unwrap().disabled);
        state.set_position(38.1).unwrap();
        assert!((state.request(UnitSystem::In).unwrap().offset_mm - 38.1).abs() < 1e-10);
        state.widgets.begin();
        paint(&mut world, camera, &mut state, 1200., 800.).unwrap();
        state.widgets.finish(&mut world);
        assert_eq!(state.widgets.entity("section-slider"), Some(slider));
        assert_eq!(
            world.get::<InterfaceControl>(offset).unwrap().binding,
            binding
        );
        assert!(!world.get::<InterfaceControl>(offset).unwrap().disabled);
        state.plane = "xz".into();
        assert_eq!(state.range(), Some((-10., 10.)));
        state.plane = "yz".into();
        assert_eq!(state.range(), Some((-20., 20.)));
        state.set_position(-50.).unwrap();
        assert!((state.request(UnitSystem::In).unwrap().offset_mm + 20.).abs() < 1e-10);
        assert!(state.set_position(f64::NAN).is_err());
    }

    #[test]
    fn intermediate_drag_preview_cannot_cross_body_plane_side_mode_revision_or_owner() {
        let owner = DocumentContext {
            window_id: "w".into(),
            document_id: "d".into(),
            epoch: 1,
        };
        let mut state = State {
            owner: Some(owner.clone()),
            revision: 8,
            visible: true,
            show_diagram: false,
            body: "1".into(),
            bodies: vec![(1, "Fixture".into())],
            plane: "xy".into(),
            offset: "5".into(),
            side: "negative".into(),
            ..Default::default()
        };
        let request = state.request(UnitSystem::Mm).unwrap();
        state.set_value(Field::Offset, "6".into());
        assert!(state.accepts_preview(&owner, 8, std::slice::from_ref(&request), false));
        assert!(!state.report_current());
        assert!(!state.accepts_preview(&owner, 9, std::slice::from_ref(&request), false));
        let mut other = owner.clone();
        other.epoch += 1;
        assert!(!state.accepts_preview(&other, 8, std::slice::from_ref(&request), false));
        for change in [Field::Body, Field::Plane, Field::Side] {
            let old = state.value(change).to_owned();
            let changed = match change {
                Field::Body => "2",
                Field::Plane => "xz",
                _ => "positive",
            };
            state.set_value(change, changed.into());
            assert!(!state.accepts_preview(&owner, 8, std::slice::from_ref(&request), false));
            state.set_value(change, old);
        }
        state.toggle_view();
        assert!(!state.accepts_preview(&owner, 8, std::slice::from_ref(&request), false));
        state.toggle_view();
        state.visible = false;
        assert!(!state.accepts_preview(&owner, 8, std::slice::from_ref(&request), false));
    }
    #[test]
    fn three_d_inspection_keeps_camera_navigation_available_and_escape_closes() {
        let mut world = World::new();
        world.insert_resource(State {
            visible: true,
            show_diagram: false,
            ..Default::default()
        });
        assert_eq!(modal(&world), None);
        assert!(in_3d(&world));
        assert!(control("Inspect").modal_scope.is_none());
        assert!(escape(&mut world));
        assert!(!in_3d(&world));
        world.resource_mut::<State>().visible = true;
        world.resource_mut::<State>().show_diagram = true;
        assert_eq!(modal(&world), None);
        assert!(active(&world));
        assert!(
            in_3d(&world),
            "The diagram must not turn off the viewport cut"
        );
        assert!(control("Inspect").modal_scope.is_none());
        assert!(escape(&mut world));
        assert!(!active(&world));
    }
    #[test]
    fn prepared_query_runs_off_thread_without_replacing_scene_or_advancing_history() {
        use crate::session_bridge::native_interface::tests::Fixture;
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let services = NativeServices {
            engine: fixture.engine.clone(),
            bridge: fixture.bridge.clone(),
        };
        let mut world = World::new();
        worker::install(
            &mut world,
            services.clone(),
            NativeInterfaceHandle::new(|| {}),
        )
        .unwrap();
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &fixture.owner())
            .unwrap();
        let before = fixture.engine.engine_call("project_export_model", "");
        let input_thread = std::thread::current().id();
        worker::enqueue_prepared_query(
            &mut world,
            receipt.owner.clone(),
            receipt.revision,
            "project_visibility".into(),
            json!({}),
            move |_| {
                assert_ne!(std::thread::current().id(), input_thread);
                Ok(vec![1_u8, 2, 3])
            },
            |_, _, result| {
                let (receipt, pixels) = result?;
                assert!(
                    receipt.value.is_null(),
                    "Query JSON must be consumed on the worker"
                );
                assert_eq!(pixels, vec![1, 2, 3]);
                Ok(json!({"previewed":true}))
            },
        )
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(outcome) = worker::poll(&mut world, &services) {
                assert_eq!(outcome.value.unwrap()["previewed"], true);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Query worker did not complete"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            fixture.engine.engine_call("project_export_model", ""),
            before
        );
        assert_eq!(
            fixture
                .bridge
                .native_document_receipt(&fixture.engine, &fixture.owner())
                .unwrap(),
            receipt
        );
        assert!(!world.contains_resource::<super::super::super::PreparedNativePresentation>());
    }
    #[test]
    fn preview_guard_rejects_changed_model_draft_closed_panel_and_other_owner() {
        let owner = DocumentContext {
            window_id: "w".into(),
            document_id: "d".into(),
            epoch: 1,
        };
        let mut state = State {
            owner: Some(owner.clone()),
            revision: 8,
            generation: 3,
            controls_generation: 9,
            visible: true,
            ..Default::default()
        };
        assert!(state.current(&owner, 8, 3));
        assert!(state.controls_current(&owner, 8, 9));
        assert!(!state.current(&owner, 9, 3));
        assert!(!state.controls_current(&owner, 9, 9));
        let mut other = owner.clone();
        other.epoch = 2;
        assert!(!state.current(&other, 8, 3));
        assert!(!state.controls_current(&other, 8, 9));
        state.set_value(Field::Offset, "10".into());
        assert!(!state.current(&owner, 8, 3));
        assert!(state.current(&owner, 8, 4));
        assert!(state.controls_current(&owner, 8, 9));
        state.toggle_view();
        assert!(!state.current(&owner, 8, 4));
        assert!(!state.controls_current(&owner, 8, 9));
        assert!(state.current(&owner, 8, 5));
        assert!(state.controls_current(&owner, 8, 10));
        state.reset_controls("replacement");
        assert!(!state.current(&owner, 8, 5));
        assert!(!state.controls_current(&owner, 8, 10));
        state.visible = false;
        assert!(!state.current(&owner, 8, 6));
        assert!(!state.controls_current(&owner, 8, 11));
    }
    #[test]
    fn offset_blur_commit_preserves_the_probe_editor_and_first_signed_character() {
        use crate::native_viewport::interface_shell::{fields, tests::fixture};
        use bevy::{
            input::{
                keyboard::{Key, KeyCode, KeyboardInput, NativeKeyCode},
                ButtonState,
            },
            text::EditableText,
            ui::{ComputedStackIndex, ComputedUiRenderTargetInfo, UiGlobalTransform, UiScale},
            window::WindowEvent,
        };
        use limo_cad_interface::ControlKey;

        let (mut app, handle, _, _) = fixture();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::text::TextPlugin,
        ))
        .init_resource::<bevy::input_focus::InputFocus>()
        .init_resource::<UiScale>()
        .init_resource::<ViewportUiAssets>()
        .init_resource::<Assets<Image>>();
        fields::install(&mut app);
        let camera = app.world_mut().spawn_empty().id();
        let mut frame = handle.frame().unwrap();
        frame.modal_stack.clear();
        frame.surfaces.push(limo_cad_interface::Surface {
            name: "document/section".into(),
            text: None,
        });
        frame.surfaces.push(limo_cad_interface::Surface {
            name: "section-review".into(),
            text: None,
        });
        let mut state = State {
            owner: Some(frame.context.clone()),
            revision: 8,
            generation: 3,
            controls_generation: 1,
            visible: true,
            body: "1".into(),
            bodies: vec![(1, "QA body".into())],
            plane: "xz".into(),
            side: "negative".into(),
            ..Default::default()
        };
        state.widgets.begin();
        paint(app.world_mut(), camera, &mut state, 1000., 800.).unwrap();
        state.widgets.finish(app.world_mut());
        let offset = state.widgets.entity("section-field-2").unwrap();
        let probe = state.widgets.entity("section-field-3").unwrap();
        for (entity, x) in [(offset, 100.), (probe, 350.)] {
            app.world_mut().entity_mut(entity).insert((
                ComputedNode {
                    size: Vec2::new(160., 28.),
                    inverse_scale_factor: 1.,
                    ..default()
                },
                UiGlobalTransform::from_translation(Vec2::new(x, 100.)),
                ComputedStackIndex(3),
                InheritedVisibility::VISIBLE,
                ComputedUiRenderTargetInfo::default(),
            ));
        }
        handle.present(frame).unwrap();
        app.update();
        app.world_mut()
            .run_system_cached(bevy::ui::widget::update_editable_text_styles)
            .unwrap();
        app.world_mut()
            .run_system_cached(bevy::ui::widget::update_editable_text_layout)
            .unwrap();
        let packet = |value: &str| {
            WindowEvent::KeyboardInput(KeyboardInput {
                key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
                logical_key: Key::Character(value.into()),
                text: Some(value.into()),
                state: ButtonState::Pressed,
                repeat: false,
                window: Entity::PLACEHOLDER,
            })
        };
        let focus = handle
            .resolve_retained(ControlKey(offset.to_bits()))
            .unwrap();
        handle.prepare_activation(&focus).unwrap();
        fields::after_window_input(app.world_mut(), &handle).unwrap();
        for value in ["1", "8", "0"] {
            assert!(fields::before_window_input(
                app.world_mut(),
                &handle,
                &packet(value),
                None,
                default()
            )
            .unwrap());
        }

        // The human focus transition precedes reduction of the old field's
        // queued blur value. The new editor must survive that later repaint.
        let focus = handle
            .resolve_retained(ControlKey(probe.to_bits()))
            .unwrap();
        let commits = fields::prepare_control_input(app.world_mut(), &handle, &focus).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(
            commits[0].control.input,
            ControlInput::SetValue("180".into())
        );
        handle.prepare_activation(&focus).unwrap();
        fields::after_window_input(app.world_mut(), &handle).unwrap();
        state.set_value(Field::Offset, "180".into());
        fields::acknowledge_control_input(app.world_mut(), &commits[0], true);
        state.widgets.begin();
        paint(app.world_mut(), camera, &mut state, 1000., 800.).unwrap();
        state.widgets.finish(app.world_mut());
        app.update();
        assert_eq!(handle.focused_key(), Some(ControlKey(probe.to_bits())));
        for (value, expected) in [("-", "-"), ("1", "-1"), ("0", "-10")] {
            assert!(
                fields::before_window_input(
                    app.world_mut(),
                    &handle,
                    &packet(value),
                    None,
                    default()
                )
                .unwrap(),
                "The first character must reach the already focused probe editor"
            );
            fields::after_window_input(app.world_mut(), &handle).unwrap();
            assert_eq!(
                app.world().get::<EditableText>(probe).unwrap().value(),
                expected
            );
        }
        assert!(
            !state.current(&focus.context, 8, 3),
            "The old report remains stale even though the editor binding stayed current"
        );
        let enter = WindowEvent::KeyboardInput(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            text: None,
            state: ButtonState::Pressed,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        fields::before_window_input(app.world_mut(), &handle, &enter, None, default()).unwrap();
        let commits = handle.take_actions().unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(
            commits[0].control.input,
            ControlInput::SetValue("-10".into())
        );
        state.set_value(Field::Probe, "-10".into());
        assert_eq!(state.request(UnitSystem::Mm).unwrap().probe_mm, Some(-10.));
    }
    #[test]
    fn diagram_rasterizes_with_text_and_units() {
        assert!((coordinate("0.04 in", UnitSystem::Mm).unwrap() - 1.016).abs() < 1e-9);
        let image=rasterize(r#"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="600"><rect width="800" height="600" fill="white"/><text x="30" y="30">Section 1 mm</text></svg>"#).unwrap();
        assert_eq!(image.width(), 800);
        assert_eq!(image.height(), 600);
        assert!(image
            .data
            .as_ref()
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] < 100));
        if let Ok(path) = std::env::var("LIMO_SECTION_QA_SVG") {
            let svg = std::fs::read_to_string(&path).unwrap();
            let image = rasterize(&svg).unwrap();
            let pixmap = resvg::tiny_skia::Pixmap::from_vec(
                image.data.unwrap(),
                resvg::tiny_skia::IntSize::from_wh(800, 600).unwrap(),
            )
            .unwrap();
            pixmap
                .save_png(std::path::Path::new(&path).with_extension("png"))
                .unwrap();
        }
    }
    #[test]
    fn refreshing_fields_preserves_controls_and_removes_invalidated_preview() {
        let mut world = World::new();
        world.init_resource::<ViewportUiAssets>();
        world.init_resource::<Assets<Image>>();
        let camera = world.spawn_empty().id();
        let image=world.resource_mut::<Assets<Image>>().add(rasterize(r#"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="600"><rect width="800" height="600" fill="white"/></svg>"#).unwrap());
        let mut state = State {
            visible: true,
            body: "1".into(),
            bodies: vec![(1, "QA body".into())],
            plane: "xy".into(),
            offset: "3 mm".into(),
            image: Some(image),
            show_diagram: true,
            ..Default::default()
        };
        state.widgets.begin();
        paint(&mut world, camera, &mut state, 1200., 900.).unwrap();
        state.widgets.finish(&mut world);
        let field = state.widgets.entity("section-field-2").unwrap();
        let probe = state.widgets.entity("section-field-3").unwrap();
        let field_binding = world.get::<InterfaceControl>(field).unwrap().binding;
        let probe_binding = world.get::<InterfaceControl>(probe).unwrap().binding;
        let preview = state.widgets.entity("section-preview").unwrap();
        assert!(world.get::<ImageNode>(preview).is_some());
        state.offset = "4 mm".into();
        state.invalidate("changed");
        state.widgets.begin();
        paint(&mut world, camera, &mut state, 1200., 900.).unwrap();
        state.widgets.finish(&mut world);
        assert_eq!(state.widgets.entity("section-field-2"), Some(field));
        assert_eq!(
            world.get::<InterfaceControl>(field).unwrap().binding,
            field_binding
        );
        assert_eq!(
            world.get::<InterfaceControl>(probe).unwrap().binding,
            probe_binding
        );
        assert!(world.get_entity(preview).is_err());
        assert!(
            matches!(&world.get::<InterfaceControl>(field).unwrap().field,ControlField::Text{value,..} if value=="4 mm")
        );
        assert!(
            world
                .get::<InterfaceControl>(state.widgets.entity("section-copy").unwrap())
                .unwrap()
                .disabled
        );
    }
}
