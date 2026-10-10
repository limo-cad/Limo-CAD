//! Disposable geometric section review. Source definitions, never exploded poses.
//! Kernel work uses the ordered query worker; results are owner/revision fenced.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Body,
    Plane,
    Offset,
    Probe,
    Side,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Open,
    Field(Field),
    Inspect,
    CopySvg,
    Close,
    ToggleView,
}
#[derive(Resource, Default)]
struct State {
    owner: Option<DocumentContext>,
    revision: u64,
    generation: u64,
    visible: bool,
    bodies: Vec<(u64, String)>,
    body: String,
    plane: String,
    offset: String,
    probe: String,
    side: String,
    in_3d: bool,
    cutaway: Option<Arc<native_viewport::section_view::PreparedCutaway>>,
    report: Option<SectionReview>,
    image: Option<Handle<Image>>,
    message: String,
    widgets: chrome::Widgets,
}
impl State {
    fn invalidate(&mut self, message: &str) {
        self.generation = self.generation.saturating_add(1);
        self.report = None;
        self.image = None;
        self.cutaway = None;
        self.message = message.into();
    }
    fn current(&self, owner: &DocumentContext, revision: u64, generation: u64) -> bool {
        self.visible
            && self.owner.as_ref() == Some(owner)
            && self.revision == revision
            && self.generation == generation
    }
    fn value(&self, field: Field) -> &str {
        match field {
            Field::Body => &self.body,
            Field::Plane => &self.plane,
            Field::Offset => &self.offset,
            Field::Probe => &self.probe,
            Field::Side => &self.side,
        }
    }
    fn choices(&self, field: Field) -> Option<Vec<ChoiceOption>> {
        let items = match field {
            Field::Body => self
                .bodies
                .iter()
                .map(|(id, name)| (id.to_string(), format!("{name} (body {id})")))
                .collect(),
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
        let body_id = self.body.parse::<u64>().map_err(|_| "Choose a body")?;
        if !self.bodies.iter().any(|(id, _)| *id == body_id) {
            return Err("Choose a current body".into());
        }
        let plane = match self.plane.as_str() {
            "xy" => SectionPlane::Xy,
            "xz" => SectionPlane::Xz,
            "yz" => SectionPlane::Yz,
            _ => return Err("Choose a section plane".into()),
        };
        let req = SectionReviewRequest {
            body_id: BodyId(body_id),
            plane,
            offset_mm: coordinate(&self.offset, units)?,
            probe_mm: if self.probe.trim().is_empty() {
                None
            } else {
                Some(coordinate(&self.probe, units)?)
            },
            deflection_mm: 0.01,
            include_cutaway: self.in_3d,
            keep_positive: self.side == "positive",
        };
        req.validate()?;
        Ok(req)
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
        let selected = &native_viewport::interface_view(world).2.selected_body_ids;
        let body = scene
            .bodies
            .iter()
            .find(|b| selected.contains(&b.id.0))
            .or(scene.bodies.first())
            .ok_or("Create a solid body first")?;
        let center = native_viewport::interface_body_local_center(
            world,
            &receipt.owner.document_id,
            &scene,
            body.id.0,
        )
        .map(|center| f64::from(center[0]))
        .unwrap_or(0.);
        let mut state = world.remove_resource::<State>().unwrap_or_default();
        state.invalidate("Choose the plane and coordinate, then Inspect. Probe is optional.");
        state.owner = Some(receipt.owner);
        state.revision = receipt.revision;
        state.visible = true;
        state.bodies = scene
            .bodies
            .iter()
            .map(|b| (b.id.0, b.name.clone()))
            .collect();
        state.body = body.id.0.to_string();
        state.plane = "yz".into();
        state.side = "negative".into();
        state.in_3d = false;
        state.offset = MeasurementInput::new(
            DimensionKind::Length,
            if center.is_finite() { center } else { 0. },
            engine.document_units(),
        )
        .text()
        .into();
        state.probe.clear();
        world.insert_resource(state);
        workbench::execute(world, &workbench::Command::Dismiss)?;
        return Ok(json!({"opened":true}));
    }
    let state = world
        .get_resource::<State>()
        .ok_or("Open Section Analysis")?;
    if !state.current(&action.context, receipt.revision, generation) {
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
        if value.len() > 256 {
            return Err("Section field exceeds 256 characters".into());
        }
        if let Some(options) = state.choices(*field) {
            if !options.iter().any(|o| o.value == value) {
                return Err("Choose a current section option".into());
            }
        }
        let mut state = world.resource_mut::<State>();
        match field {
            Field::Body => state.body = value,
            Field::Plane => state.plane = value,
            Field::Offset => state.offset = value,
            Field::Probe => state.probe = value,
            Field::Side => state.side = value,
        }
        state.invalidate("Section inputs changed. Inspect to refresh.");
        return Ok(json!({"handled":true}));
    }
    if !super::super::is_activation(&action.control.input) {
        return Err("Activate a section control".into());
    }
    if *command == Command::Close {
        let mut state = world.resource_mut::<State>();
        state.visible = false;
        state.invalidate("");
        native_viewport::section_view::clear(world);
        return Ok(json!({"closed":true}));
    }
    if *command == Command::ToggleView {
        let mut state = world.resource_mut::<State>();
        state.in_3d = !state.in_3d;
        if !state.in_3d
            || state.cutaway.is_some()
            || state
                .report
                .as_ref()
                .is_some_and(|report| report.outcome != SectionOutcome::MaterialSection)
        {
            return Ok(json!({"view":if state.in_3d {"cutaway"} else {"diagram"}}));
        }
    }
    if *command == Command::CopySvg {
        let svg = world
            .resource::<State>()
            .report
            .as_ref()
            .filter(|r| !r.svg.is_empty())
            .ok_or("Inspect a nonempty section first")?
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
    let req = match world.resource::<State>().request(engine.document_units()) {
        Ok(req) => req,
        Err(error) => {
            world.resource_mut::<State>().message = error.clone();
            return Err(error);
        }
    };
    let owner = receipt.owner.clone();
    let revision = receipt.revision;
    worker::enqueue_prepared_query(
        world,
        receipt.owner,
        revision,
        "solid_section_review".into(),
        serde_json::to_value(req).map_err(|e| e.to_string())?,
        |value| {
            let mut report: SectionReview =
                serde_json::from_value(value).map_err(|e| e.to_string())?;
            let image = if report.svg.is_empty() {
                None
            } else {
                Some(rasterize(&report.svg)?)
            };
            let cutaway = report
                .cutaway
                .take()
                .map(|body| Arc::new(native_viewport::section_view::PreparedCutaway::new(body)));
            Ok((report, image, cutaway))
        },
        move |world, services, result| {
            services.bridge.with_native_document_receipt(
                &services.engine, &owner, |current| {
                    if current != revision || !world.get_resource::<State>()
                        .is_some_and(|s| s.current(&owner, current, generation)) {
                        return Err("The section review changed before the result arrived".into());
                    }
                    let (report,image,cutaway)=match result {
                        Ok((_,prepared))=>prepared,
                        Err(error)=> {
                            world.resource_mut::<State>().message=error.clone();
                            return Err(error);
                        }
                    };
                    world.init_resource::<Assets<Image>>();
                    let image=image.map(|image|world.resource_mut::<Assets<Image>>().add(image));
                    let response=json!({"inspected":true,"source_revision":revision,
                        "body_id":report.request.body_id,"outcome":report.outcome,
                        "bounds_mm":report.bounds_mm,"probe_spans":report.probe_spans});
                    let mut state=world.resource_mut::<State>();
                    state.message=if report.outcome == SectionOutcome::NoIntersection {
                        "Plane does not intersect this body".into()
                    } else if report.outcome == SectionOutcome::BoundaryContact {
                        "Plane touches the boundary. Choose an interior coordinate for material spans and a 3D cutaway.".into()
                    } else if report.request.probe_mm.is_some() {
                        format!("{} material {}. Diagram and distances are in mm; source-body coordinates.",report.probe_spans.len(),if report.probe_spans.len()==1 {"span"} else {"spans"})
                    } else {
                        "Source-body section in mm. Enter a probe height to measure material spans.".into()
                    };
                    state.cutaway=cutaway;
                    state.report=Some(report);state.image=image;
                    Ok(response)
                },
            )
        },
    )
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
            state.invalidate("");
            state.owner = Some(owner.clone());
        }
        if state.in_3d && workbench::workspace(world) == workbench::Workspace::Drawing {
            state.visible = false;
            state.invalidate("");
        }
        if !state.visible {
            native_viewport::section_view::clear(world);
            return Ok(());
        }
        let revision = services
            .bridge
            .native_document_receipt(&services.engine, owner)?
            .revision;
        if state.revision != revision {
            state.revision = revision;
            state.invalidate("Source model changed. Inspect to refresh.");
            state.bodies = services
                .engine
                .solid_scene_snapshot()
                .bodies
                .iter()
                .map(|b| (b.id.0, b.name.clone()))
                .collect();
            if !state
                .bodies
                .iter()
                .any(|(id, _)| id.to_string() == state.body)
            {
                state.body = state
                    .bodies
                    .first()
                    .map(|(id, _)| id.to_string())
                    .unwrap_or_default();
            }
        }
        if state.in_3d {
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
        state.invalidate("");
        native_viewport::section_view::clear(world);
        true
    } else {
        false
    }
}

pub(crate) fn modal(world: &World) -> Option<&'static str> {
    world
        .get_resource::<State>()
        .filter(|s| s.visible && !s.in_3d)
        .map(|_| "section-review")
}
pub(crate) fn in_3d(world: &World) -> bool {
    world
        .get_resource::<State>()
        .is_some_and(|s| s.visible && s.in_3d)
}

fn control(label: &str, in_3d: bool) -> InterfaceControl {
    let mut control = InterfaceControl::button("document/section", label);
    control.modal_scope = (!in_3d).then(|| "section-review".into());
    control
}

fn paint(
    world: &mut World,
    camera: Entity,
    state: &mut State,
    width: f32,
    height: f32,
) -> Result<(), String> {
    let w = (width - 32.).clamp(280., if state.in_3d { 480. } else { 760. });
    let x = if state.in_3d { 16. } else { (width - w) / 2. };
    let y = 130.;
    let h = if state.in_3d {
        280.
    } else {
        (height - y - 24.).max(280.)
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
        chrome::rect(x + 12., y + 8., w - 200., 20.),
        "Section Analysis - source body",
        14.,
        72,
    );
    let close = control("Close section analysis", state.in_3d);
    state.widgets.button(
        world,
        camera,
        "section-close",
        close,
        Some("Close"),
        NativeCommand::SectionReview(state.generation, Command::Close),
        chrome::rect(x + w - 72., y + 6., 60., 24.),
        None,
        72,
    )?;
    let mut copy = control("Copy section SVG", state.in_3d);
    copy.disabled = state.report.as_ref().is_none_or(|r| r.svg.is_empty()) || worker::busy(world);
    state.widgets.button(
        world,
        camera,
        "section-copy",
        copy,
        Some("Copy SVG"),
        NativeCommand::SectionReview(state.generation, Command::CopySvg),
        chrome::rect(x + w - 174., y + 6., 94., 24.),
        None,
        72,
    )?;
    let col = (w - 36.) / 2.;
    let plane = match state.plane.as_str() {
        "xy" => SectionPlane::Xy,
        "xz" => SectionPlane::Xz,
        _ => SectionPlane::Yz,
    };
    let labels = plane.labels();
    for (index, field, label) in [
        (0, Field::Body, "Source body".into()),
        (1, Field::Plane, "Section plane".into()),
        (
            2,
            Field::Offset,
            format!("{} offset (document units or explicit mm/in)", labels[2]),
        ),
        (
            3,
            Field::Probe,
            format!("Probe {} coordinate (optional)", labels[1]),
        ),
        (4, Field::Side, "Retained side".into()),
    ] {
        let fx = x + 12. + (index % 2) as f32 * (col + 12.);
        let fy = y + 38. + (index / 2) as f32 * 51.;
        state.widgets.text(
            world,
            camera,
            &format!("section-label-{index}"),
            chrome::rect(fx, fy, col, 16.),
            &label,
            10.,
            72,
        );
        let mut control = control(&label, state.in_3d);
        control.disabled = worker::busy(world);
        let mut caption = None;
        if let Some(options) = state.choices(field) {
            caption = options
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
            caption.as_deref(),
            NativeCommand::SectionReview(state.generation, Command::Field(field)),
            chrome::rect(fx, fy + 17., col, 28.),
            None,
            72,
        )?;
    }
    state.widgets.button(
        world,
        camera,
        "section-mode",
        control("Switch section view", state.in_3d),
        Some(if state.in_3d { "Diagram" } else { "3D cutaway" }),
        NativeCommand::SectionReview(state.generation, Command::ToggleView),
        chrome::rect(x + col + 24., y + 157., col, 28.),
        None,
        72,
    )?;
    let mut inspect = control("Inspect section", state.in_3d);
    inspect.disabled = worker::busy(world) || state.bodies.is_empty();
    state.widgets.button(
        world,
        camera,
        "section-inspect",
        inspect,
        Some("Inspect"),
        NativeCommand::SectionReview(state.generation, Command::Inspect),
        chrome::rect(x + 12., y + 195., 92., 28.),
        None,
        72,
    )?;
    state.widgets.text(
        world,
        camera,
        "section-message",
        chrome::rect(x + 116., y + 194., w - 128., 46.),
        &state.message,
        10.,
        72,
    );
    if state.in_3d {
        state.widgets.text(
            world,
            camera,
            "section-3d-hint",
            chrome::rect(x + 12., y + 247., w - 24., 25.),
            "Source body only. Orange = cut face. Orbit / pan / zoom; close to restore.",
            10.,
            72,
        );
    } else if let Some(image) = &state.image {
        let iw = (w - 24.).min((h - 250.).max(0.) * 4. / 3.);
        let ih = iw * 3. / 4.;
        if ih > 20. {
            state.widgets.panel(
                world,
                camera,
                "section-preview",
                chrome::rect(x + (w - iw) / 2., y + 244., iw, ih),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn three_d_inspection_keeps_camera_navigation_available_and_escape_closes() {
        let mut world = World::new();
        world.insert_resource(State {
            visible: true,
            in_3d: true,
            ..Default::default()
        });
        assert_eq!(modal(&world), None);
        assert!(in_3d(&world));
        assert!(control("Inspect", true).modal_scope.is_none());
        assert!(escape(&mut world));
        assert!(!in_3d(&world));
        world.resource_mut::<State>().visible = true;
        world.resource_mut::<State>().in_3d = false;
        assert_eq!(modal(&world), Some("section-review"));
        assert_eq!(
            control("Inspect", false).modal_scope.as_deref(),
            Some("section-review")
        );
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
            visible: true,
            ..Default::default()
        };
        assert!(state.current(&owner, 8, 3));
        assert!(!state.current(&owner, 9, 3));
        let mut other = owner.clone();
        other.epoch = 2;
        assert!(!state.current(&other, 8, 3));
        state.invalidate("changed");
        assert!(!state.current(&owner, 8, 3));
        state.visible = false;
        assert!(!state.current(&owner, 8, 4));
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
            ..Default::default()
        };
        state.widgets.begin();
        paint(&mut world, camera, &mut state, 1200., 900.).unwrap();
        state.widgets.finish(&mut world);
        let field = state.widgets.entity("section-field-2").unwrap();
        let preview = state.widgets.entity("section-preview").unwrap();
        assert!(world.get::<ImageNode>(preview).is_some());
        state.offset = "4 mm".into();
        state.invalidate("changed");
        state.widgets.begin();
        paint(&mut world, camera, &mut state, 1200., 900.).unwrap();
        state.widgets.finish(&mut world);
        assert_eq!(state.widgets.entity("section-field-2"), Some(field));
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
