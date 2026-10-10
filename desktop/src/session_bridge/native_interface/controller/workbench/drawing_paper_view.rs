//! Cached paper presentation. Idle synchronization refreshes the shared drawing;
//! navigation repaints only retained data, including while OCC owns its locks.
use super::super::drawing_navigation::{Navigation, Pane};
use super::*;
#[path = "drawing_paper_view/diagnostics.rs"]
pub(super) mod diagnostics;

pub(in super::super) struct PaperView {
    pub(in super::super) camera: Entity,
    pub(in super::super) navigation: Navigation,
    pub(super) source: edges::SourceKey,
    width: f32,
    height: f32,
    side: f32,
    pub(super) marks: Vec<AnnotationMark>,
    pub(super) art_context: Option<(u64, limo_cad_core::UnitSystem)>,
    art_failure: Option<ArtFailure>,
}
struct ArtFailure {
    source: edges::SourceKey,
    sheet: DrawingSheetDto,
    units: limo_cad_core::UnitSystem,
    error: String,
}
impl PaperView {
    pub(super) fn annotations(
        &mut self,
        sheet: &DrawingSheetDto,
        projections: &edges::Projections,
        units: limo_cad_core::UnitSystem,
        labels: &[Label],
    ) -> Result<annotations::Art, String> {
        self.annotations_with(sheet, units, || {
            annotations::try_render_decorated(sheet, projections, units, labels)
        })
    }
    fn annotations_with(
        &mut self,
        sheet: &DrawingSheetDto,
        units: limo_cad_core::UnitSystem,
        render: impl FnOnce() -> Result<annotations::Art, String>,
    ) -> Result<annotations::Art, String> {
        if let Some(failed) = &self.art_failure {
            if failed.source == self.source && failed.sheet == *sheet && failed.units == units {
                return Err(failed.error.clone());
            }
        }
        self.art_failure = None;
        match render() {
            Ok(art) => Ok(art),
            Err(error) => {
                self.art_failure = Some(ArtFailure {
                    source: self.source.clone(),
                    sheet: sheet.clone(),
                    units,
                    error: error.clone(),
                });
                Err(error)
            }
        }
    }
}
pub(super) fn publish(
    state: &mut Workbench,
    revision: u64,
    sheet: &DrawingSheetDto,
    units: limo_cad_core::UnitSystem,
    art: annotations::Art,
) {
    if let Some(view) = &mut state.paper_view {
        view.marks = art.marks;
    }
    state.paper = art.segments;
    state.paper_labels = art.labels;
    state.paper_fills = art.fills;
    state.paper_key = Some((revision, sheet.clone(), units));
}
pub(super) fn fail(world: &mut World, state: &mut Workbench, error: &str) {
    state.paper_key = None;
    state.paper.clear();
    state.paper_labels.clear();
    state.paper_fills.clear();
    if let Some(view) = &mut state.paper_view {
        view.marks.clear();
    }
    show_error(world, state, error);
}

pub(in super::super) fn pane(width: f32, height: f32, side: f32) -> Pane {
    Pane {
        bounds: InterfaceRect {
            x: side as f64,
            y: 120.,
            width: (width - side).max(33.) as f64,
            height: (height - 220.).max(13.) as f64,
        },
        padding: [16., 12., 16., 0.],
    }
}
fn render_scale(world: &World, camera: Entity) -> f32 {
    world
        .get::<Camera>(camera)
        .and_then(Camera::target_scaling_factor)
        .unwrap_or(1.)
        * world
            .get_resource::<bevy::ui::UiScale>()
            .map_or(1., |s| s.0)
}
/// Read the completed paper layout, separately from the Fit toggle. These
/// measurements let an observer distinguish a zoom from a state-only change.
pub(in super::super) fn navigation_snapshot(world: &World, state: &Workbench) -> Option<Value> {
    let (_, sheet, _) = state.paper_key.as_ref()?;
    let owner = state.owner.as_ref()?;
    let view = state.paper_view.as_ref()?;
    let transform = view.navigation.transform();
    let paper = state.widgets.entity("drawing-paper")?;
    let node = world.get::<ComputedNode>(paper)?;
    Some(json!({
        "owner": {"window_id":owner.window_id,"document_id":owner.document_id,"epoch":owner.epoch},
        "sheet_id": sheet.id,
        "paper_scale": transform.scale,
        "sheet_mm": transform.sheet_mm,
        "origin": transform.origin,
        "render_scale": node.inverse_scale_factor().recip(),
        "rendered_size_px": node.size().to_array(),
        "client_size": [view.width, view.height]
    }))
}
fn raster(world: &World, view: &PaperView) -> Result<edges::RasterKey, String> {
    let transform = view.navigation.transform();
    Ok(edges::RasterKey {
        sheet_mm: transform.sheet_mm.map(|n| n as f32),
        paper_scale: transform.scale as f32,
        render_scale: render_scale(world, view.camera),
        visible_mm: transform
            .visible_paper()
            .ok_or("Drawing paper is outside its viewport")?,
    })
}

fn paint_backdrop(
    world: &mut World,
    camera: Entity,
    state: &mut Workbench,
    width: f32,
    height: f32,
    side: f32,
) {
    let theme = crate::native_viewport::ui::theme(world);
    state.widgets.panel(
        world,
        camera,
        "drawing-backdrop",
        rect(side, 120., width - side, height - 168.),
        theme.viewport.with_alpha(1.),
        6,
    );
}

pub(in super::super) fn paint(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    state: &mut Workbench,
    (width, height, side): (f32, f32, f32),
    controls: &HashMap<String, Entity>,
) -> Result<(), String> {
    paint_backdrop(world, camera, state, width, height, side);
    let owner = state
        .owner
        .clone()
        .ok_or("Drawing paper has no document owner")?;
    let (receipt, drawing) =
        services
            .bridge
            .with_native_document_receipt(&services.engine, &owner, |revision| {
                let receipt = workspace::DocumentReceipt {
                    owner: owner.clone(),
                    revision,
                };
                if state
                    .paper_document
                    .as_ref()
                    .is_none_or(|(previous, _)| previous != &receipt)
                {
                    state.paper_document = Some((
                        receipt.clone(),
                        Arc::new(services.engine.drawing_snapshot()),
                    ));
                }
                Ok((receipt, state.paper_document.as_ref().unwrap().1.clone()))
            })?;
    let Some(sheet) = drawing.sheets.iter().find(|sheet| {
        drawing.active_sheet_id == Some(sheet.id)
            || (drawing.active_sheet_id.is_none()
                && drawing.sheets.last().map(|s| s.id) == Some(sheet.id))
    }) else {
        state.paper_key = None;
        state.paper.clear();
        state.paper_labels.clear();
        state.paper_fills.clear();
        state.paper_view = None;
        world.remove_resource::<edges::EdgeCache>();
        return Ok(());
    };
    let repair_view =
        super::super::drawing_authoring::repair_view(world, sheet, &owner, receipt.revision);
    let original_views = &sheet.views;
    let mut preview =
        super::super::drawing_authoring::preview(world, sheet, &owner, receipt.revision);
    if let Some(id) = repair_view {
        let preview = preview.to_mut();
        preview.views.retain(|v| v.id == id);
        for view in &mut preview.views {
            view.derivation = None;
        }
        preview.annotations.clear();
    }
    let sheet = preview.as_ref();
    let revision = services.engine.geometry_revision();
    let units = services.engine.document_units();
    let (sheet_w, sheet_h) = sheet_size(sheet);
    let sheet_mm = [sheet_w as f64, sheet_h as f64];
    if let Some(view) = &mut state.paper_view {
        view.navigation
            .observe(owner.clone(), sheet.id, sheet_mm, pane(width, height, side))?;
        view.camera = camera;
        view.source
            .refresh(&owner, receipt.revision, revision, sheet);
        view.width = width;
        view.height = height;
        view.side = side;
    } else {
        state.paper_view = Some(PaperView {
            camera,
            source: edges::SourceKey::new(owner.clone(), receipt.revision, revision, sheet),
            width,
            height,
            side,
            marks: Vec::new(),
            art_context: None,
            art_failure: None,
            navigation: Navigation::new(owner, sheet.id, sheet_mm, pane(width, height, side))?,
        });
    }
    state.paper_view.as_mut().unwrap().art_context = Some((revision, units));
    let view = state.paper_view.as_ref().unwrap();
    let raster = raster(world, view)?;
    let source = view.source.clone();
    let art_changed = state
        .paper_key
        .as_ref()
        .is_none_or(|(r, s, u)| *r != revision || s != sheet || *u != units);
    world.init_resource::<edges::EdgeCache>();
    let prepared = world.resource_scope(|world, mut cache: Mut<edges::EdgeCache>| {
        let mut images = world.resource_mut::<Assets<Image>>();
        let ready = cache.prepare_sheet(
            &mut images,
            source,
            raster,
            |view| {
                if repair_view.is_some() {
                    let saved = original_views
                        .iter()
                        .find(|saved| saved.id == view.id)
                        .ok_or("Repair view was removed")?;
                    return services
                        .engine
                        .project_sheet_view_resolved(saved, original_views);
                }
                services
                    .engine
                    .project_sheet_view_resolved(view, &sheet.views)
            },
            |projections, budget| {
                services.engine.section_source_graphics(
                    sheet,
                    |id| projections.get(&id).map(|(_, p)| p),
                    budget,
                )
            },
        )?;
        if art_changed || ready.source_changed {
            let art = state.paper_view.as_mut().unwrap().annotations(
                sheet,
                ready.projections,
                units,
                ready.source_labels,
            )?;
            publish(state, revision, sheet, units, art);
        }
        Ok::<_, String>((ready.image, ready.region))
    });
    if let Err(error) = prepared.and_then(|(image, region)| draw(world, state, image, region)) {
        fail(world, state, &error);
    }
    if let Some(id) = repair_view {
        state.widgets.text(
            world,
            camera,
            "drawing-repair-preview",
            rect(side + 16., 122., (width - side - 32.).max(1.), 24.),
            &format!(
                "Reference repair: showing only view {id}. Sheet setup restores the full sheet."
            ),
            12.,
            46,
        );
    }
    toolbar(world, state, controls)
}

pub(in super::super) fn repaint(world: &mut World, state: &mut Workbench) -> Result<(), String> {
    let view = state.paper_view.as_ref().ok_or("Open a drawing sheet")?;
    if let Some(failed) = &view.art_failure {
        if failed.source == view.source {
            let error = failed.error.clone();
            fail(world, state, &error);
            return Err(error);
        }
    }
    let raster = raster(world, view)?;
    let source = view.source.clone();
    if !world.contains_resource::<edges::EdgeCache>() {
        return Err("Drawing projection is not ready".into());
    }
    let prepared = world.resource_scope(|world, mut cache: Mut<edges::EdgeCache>| {
        let mut images = world.resource_mut::<Assets<Image>>();
        let ready = cache.prepare_sheet(
            &mut images,
            source,
            raster,
            |_| Err("Drawing projection changed; wait for the document refresh".into()),
            |_, _| Err("Drawing source marks changed; wait for the document refresh".into()),
        )?;
        Ok::<_, String>((ready.image, ready.region))
    });
    match prepared.and_then(|(image, region)| draw(world, state, image, region)) {
        Ok(()) => Ok(()),
        Err(error) => {
            fail(world, state, &error);
            Err(error)
        }
    }
}

fn show_error(world: &mut World, state: &mut Workbench, error: &str) {
    if let Some(entity) = state.widgets.entity("drawing-paper") {
        if let Some(mut node) = world.get_mut::<Node>(entity) {
            node.display = Display::None;
        }
    }
    if let Some(view) = &state.paper_view {
        state.widgets.text(
            world,
            view.camera,
            "drawing-render-error",
            rect(
                view.side + 24.,
                150.,
                (view.width - view.side - 48.).max(1.),
                90.,
            ),
            error,
            13.,
            20,
        );
        let entity = state.widgets.entity("drawing-render-error").unwrap();
        world.entity_mut(entity).insert((
            TextColor(Ink::Overflow.color()),
            TextLayout {
                linebreak: bevy::text::LineBreak::WordOrCharacter,
                ..default()
            },
        ));
    }
}

fn draw(
    world: &mut World,
    state: &mut Workbench,
    image: Handle<Image>,
    region: edges::RasterRegion,
) -> Result<(), String> {
    let view = state.paper_view.as_ref().ok_or("Open a drawing sheet")?;
    let camera = view.camera;
    let transform = view.navigation.transform();
    let scale = transform.scale as f32;
    let dpi = render_scale(world, camera);
    let sheet = &state
        .paper_key
        .as_ref()
        .ok_or("Drawing annotations are not ready")?
        .1;
    world.init_resource::<FrameCache>();
    let error = {
        let mut cache = world.resource_mut::<FrameCache>();
        let source = frame::Source::new(sheet, transform.sheet_mm);
        if cache.0.as_ref().is_none_or(|(saved, _)| saved != &source) {
            match frame::try_render(&source) {
                Ok(art) => {
                    cache.0 = Some((source.into_owned(), art));
                    None
                }
                Err(error) => Some(error),
            }
        } else {
            None
        }
    };
    if let Some(error) = error {
        let (_, failed_sheet, units) = state.paper_key.take().unwrap();
        let view = state.paper_view.as_mut().unwrap();
        view.art_failure = Some(ArtFailure {
            source: view.source.clone(),
            sheet: failed_sheet,
            units,
            error: error.clone(),
        });
        return Err(error);
    }
    let clip = transform.clip;
    state.widgets.panel(
        world,
        camera,
        "drawing-content-clip",
        rect(
            clip.x as f32,
            clip.y as f32,
            clip.width as f32,
            clip.height as f32,
        ),
        Color::NONE,
        7,
    );
    let parent = state.widgets.entity("drawing-content-clip").unwrap();
    mark_paper_input(world, parent);
    world.get_mut::<Node>(parent).unwrap().overflow = Overflow::clip();
    world.entity_mut(parent).insert(bevy::ui::LayoutConfig {
        use_rounding: false,
    });
    state.widgets.panel(
        world,
        camera,
        "drawing-paper",
        rect(
            (transform.origin[0] - clip.x) as f32,
            (transform.origin[1] - clip.y) as f32,
            (transform.sheet_mm[0] * transform.scale) as f32,
            (transform.sheet_mm[1] * transform.scale) as f32,
        ),
        Color::WHITE,
        8,
    );
    state.widgets.parent(world, "drawing-paper", parent);
    let paper = state.widgets.entity("drawing-paper").unwrap();
    mark_paper_input(world, paper);
    world.get_mut::<Node>(paper).unwrap().overflow = Overflow::clip();
    world.entity_mut(paper).insert(bevy::ui::LayoutConfig {
        use_rounding: false,
    });
    if let Some(entity) = state.widgets.entity("drawing-render-error") {
        if let Some(mut node) = world.get_mut::<Node>(entity) {
            node.display = Display::None;
        }
    }
    state.widgets.panel(
        world,
        camera,
        "drawing-projected-edges",
        rect(
            (region.origin_mm[0] * transform.scale) as f32,
            (region.origin_mm[1] * transform.scale) as f32,
            (region.size_mm[0] * transform.scale) as f32,
            (region.size_mm[1] * transform.scale) as f32,
        ),
        Color::NONE,
        12,
    );
    state
        .widgets
        .parent(world, "drawing-projected-edges", paper);
    world
        .entity_mut(state.widgets.entity("drawing-projected-edges").unwrap())
        .insert(ImageNode::new(image));
    mark_paper_input(
        world,
        state.widgets.entity("drawing-projected-edges").unwrap(),
    );
    world.resource_scope(|world, cache: Mut<FrameCache>| {
        let art = &cache.0.as_ref().unwrap().1;
        paint_primitives(
            world,
            camera,
            &mut state.widgets,
            paper,
            scale,
            dpi,
            "drawing-frame",
            &art.segments,
            &art.labels,
            &art.fills,
            10,
            9,
            11,
        );
    });
    paint_primitives(
        world,
        camera,
        &mut state.widgets,
        paper,
        scale,
        dpi,
        "drawing",
        &state.paper,
        &state.paper_labels,
        &state.paper_fills,
        13,
        14,
        16,
    );
    if let Some(entity) = state.widgets.entity("drawing-zoom-value") {
        let value = format!("{}%", (view.navigation.zoom * 100.).round());
        if world.get::<Text>(entity).is_none_or(|text| text.0 != value) {
            world.entity_mut(entity).insert(Text::new(value));
        }
    }
    Ok(())
}

fn toolbar(
    world: &mut World,
    state: &mut Workbench,
    controls: &HashMap<String, Entity>,
) -> Result<(), String> {
    let view = state.paper_view.as_ref().ok_or("Open a drawing sheet")?;
    let (camera, width, height, side, zoom, fitted) = (
        view.camera,
        view.width,
        view.height,
        view.side,
        view.navigation.zoom,
        view.navigation.fitted,
    );
    let theme = crate::native_viewport::ui::theme(world);
    let x = side + (width - side - 246.) * 0.5;
    let y = height - 94.;
    card(
        (&mut state.widgets, world, camera),
        "navigation",
        rect(x, y, 246., 34.),
        theme.header,
        5.,
        25,
    );
    for (index, (key, icon)) in [("undo", Icon::Undo), ("redo", Icon::Redo)]
        .into_iter()
        .enumerate()
    {
        if let Some(&entity) = controls.get(key) {
            world.entity_mut(entity).insert((
                rect(x + 6. + index as f32 * 26., y + 5., 24., 24.),
                interface_shell::InterfaceFlat,
                interface_shell::InterfaceCaption(String::new()),
            ));
            {
                let tint = if world.get::<InterfaceControl>(entity).unwrap().disabled {
                    theme.edge
                } else {
                    theme.mute
                };
                state.widgets.glyph(
                    (world, camera),
                    &format!("nav-{key}-glyph"),
                    rect(x + 10. + index as f32 * 26., y + 9., 16., 16.),
                    icon,
                    tint,
                    31,
                )
            };
        }
    }
    for (key, label, caption, command, left, w, selected) in [
        (
            "drawing-fit",
            "Fit sheet",
            "Fit",
            Command::DrawingFit,
            64.,
            48.,
            Some(fitted),
        ),
        (
            "drawing-zoom-out",
            "Zoom drawing out",
            "−",
            Command::DrawingZoom(-1),
            116.,
            26.,
            None,
        ),
        (
            "drawing-zoom-in",
            "Zoom drawing in",
            "+",
            Command::DrawingZoom(1),
            210.,
            26.,
            None,
        ),
    ] {
        centered_button(
            (&mut state.widgets, world, camera),
            (key, label, caption),
            NativeCommand::Workbench(command),
            rect(x + left, y + 5., w, 24.),
            selected,
            false,
            31,
        )?;
    }
    state.widgets.text(
        world,
        camera,
        "drawing-zoom-value",
        rect(x + 148., y + 5., 58., 24.),
        &format!("{}%", (zoom * 100.).round()),
        11.,
        31,
    );
    Ok(())
}

pub(in super::super) fn canvas(state: &Workbench) -> Option<Canvas> {
    if state.workspace != Workspace::Drawing || state.paper_key.is_none() {
        return None;
    }
    let t = state.paper_view.as_ref()?.navigation.transform();
    let visible = t.visible_paper()?;
    let origin = t.to_screen([visible[0], visible[1]]);
    Some(Canvas {
        name: "drawing".into(),
        bounds: InterfaceRect {
            x: origin[0],
            y: origin[1],
            width: visible[2] * t.scale,
            height: visible[3] * t.scale,
        },
    })
}

#[cfg(test)]
#[path = "drawing_paper_view/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "drawing_paper_view/recipe_repro.rs"]
mod recipe_repro;
