use super::super::*;
use super::{
    anchors,
    draft::{Draft, Selection},
    fields::{self, Field},
    *,
};
use limo_cad_sketch::{
    DrawingChainDimensionLayout, DrawingDocumentDto, DrawingRadialDimensionMode, DrawingSheetDto,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    Technical(technical::Tool),
    Note,
    HoleNote,
    Linear,
    Radial(DrawingRadialDimensionMode),
    Angular,
    Series(DrawingChainDimensionLayout),
    Ordinate,
    Chamfer,
    RevisionCloud,
    CenterMark,
    CenterLine,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Tool(Tool),
    Field(fields::Id),
    RepairRecord,
    RepairReference,
    Apply,
    Reset,
    Delete,
    Select(u64),
    CloudEdge(u64, usize),
    Anchor(usize),
    Circle(usize),
    Center(usize),
    CenterGrip(u64, usize),
    Line(usize),
    Chamfer(usize),
    Cancel,
    Fields(i32),
}

#[derive(Clone)]
pub(super) struct Target {
    pub view_id: u64,
    pub reference: DrawingTopologyAnchorRefDto,
    pub paper: [f64; 2],
}
pub(super) struct Drag {
    pub stamp: Stamp,
    pub start: [f64; 2],
    pub draft: Draft,
    pub linear_points: Option<[[f64; 2]; 2]>,
    pub radial: Option<drawing_paper::RadialDrag>,
    pub angular: Option<drawing_paper::AngularDrag>,
    pub ordinate_points: Option<[[f64; 2]; 2]>,
    pub moved: bool,
    pub center: Option<center::Grip>,
    pub projection: Option<drawing_paper::ProjectionStamp>,
}
#[derive(Resource, Default)]
pub(super) struct Editor {
    pub stamp: Option<Stamp>,
    pub document: Arc<DrawingDocumentDto>,
    pub serial: u64,
    pub selected: Option<u64>,
    pub pending_selected: Option<u64>,
    pub draft: Option<Draft>,
    pub fields: Vec<Field>,
    pub tool: Option<Tool>,
    pub pair: LinearPlacement,
    pub angular: angular::Placement,
    pub series: series::Placement,
    pub straight: straight::Placement,
    pub lines: Vec<straight::LineTarget>,
    pub line_source: Option<drawing_paper::ProjectionStamp>,
    pub cloud: cloud::Placement,
    pub chamfer: chamfer::Placement,
    pub chamfers: Vec<chamfer::Target>,
    pub chamfer_source: Option<drawing_paper::ProjectionStamp>,
    pub circles: Vec<radial::Target>,
    pub radial_source: Option<drawing_paper::ProjectionStamp>,
    pub hole_source: Option<drawing_paper::ProjectionStamp>,
    pub centers: Vec<radial::Target>,
    pub center: center::Placement,
    pub center_source: Option<drawing_paper::ProjectionStamp>,
    pub targets: Vec<Target>,
    pub point_source: Option<drawing_paper::ProjectionStamp>,
    pub repair: repair::State,
    pub technical: technical::Placement,
    pub technical_source: Option<drawing_paper::ProjectionStamp>,
    pub drag: Option<Drag>,
    pub message: String,
    pub page: usize,
    pub widgets: Widgets,
}
impl Editor {
    fn inactive(&mut self, owner: &DocumentContext) {
        if self
            .stamp
            .as_ref()
            .is_some_and(|stamp| &stamp.owner == owner)
        {
            self.drag = None;
            self.pair.cancel();
            self.angular.cancel();
            self.series.cancel();
            self.straight.cancel();
            self.chamfer.cancel();
            self.cloud.cancel();
            self.center.cancel();
            self.technical.cancel();
        } else {
            self.retire();
        }
    }
    fn retire(&mut self) {
        self.clear();
        if self.stamp.is_some() || !self.document.sheets.is_empty() {
            self.document = Arc::default();
        }
        self.stamp = None;
    }
    pub fn dirty(&self) -> bool {
        self.repair.pending.is_some() || (self.draft.is_some() && fields::dirty(&self.fields))
    }
    pub fn select(&mut self, id: u64) -> Result<(), String> {
        let sheet_id = self.stamp.as_ref().ok_or("Create a sheet first")?.sheet_id;
        let draft = Draft::new(
            &self.document,
            Selection {
                sheet_id,
                annotation_id: id,
            },
        )?;
        self.repair = repair::State::default();
        self.fields = fields::from_annotation(draft.annotation());
        self.draft = Some(draft);
        self.selected = Some(id);
        self.tool = None;
        self.pair.cancel();
        self.angular.cancel();
        self.series.cancel();
        self.straight.cancel();
        self.chamfer.cancel();
        self.cloud.cancel();
        self.center.cancel();
        self.technical.cancel();
        self.page = 0;
        self.serial = self.serial.wrapping_add(1);
        self.message.clear();
        Ok(())
    }
    pub fn clear(&mut self) {
        self.draft = None;
        self.repair = repair::State::default();
        self.fields.clear();
        self.selected = None;
        self.pending_selected = None;
        self.tool = None;
        self.pair.cancel();
        self.angular.cancel();
        self.series.cancel();
        self.straight.cancel();
        self.chamfer.cancel();
        self.cloud.cancel();
        self.center.cancel();
        self.technical.cancel();
        self.chamfers.clear();
        self.chamfer_source = None;
        self.circles.clear();
        self.radial_source = None;
        self.hole_source = None;
        self.centers.clear();
        self.center_source = None;
        self.technical_source = None;
        self.targets.clear();
        self.point_source = None;
        self.lines.clear();
        self.line_source = None;
        self.drag = None;
        self.page = 0;
        self.message.clear();
        self.serial = self.serial.wrapping_add(1);
    }
    pub fn pick_line(&mut self, stamp: &Stamp, index: usize) -> Result<(), String> {
        if self.tool != Some(Tool::Linear) {
            return Err("Choose Dimension first".into());
        }
        let line = self.lines.get(index).ok_or("Projection changed")?.clone();
        let point = self
            .pair
            .first
            .as_ref()
            .and_then(|(saved, view, reference)| {
                (saved == stamp)
                    .then(|| {
                        self.targets
                            .iter()
                            .find(|t| {
                                t.view_id == *view && anchors::same_anchor(&t.reference, reference)
                            })
                            .cloned()
                    })
                    .flatten()
            });
        self.straight.edge(stamp, line, point);
        self.pair.cancel();
        self.message.clear();
        Ok(())
    }
    pub fn pick_chamfer(
        &mut self,
        stamp: &Stamp,
        index: usize,
        size: [f64; 2],
    ) -> Result<(), String> {
        if self.tool != Some(Tool::Chamfer) {
            return Err("Choose Chamfer note first".into());
        }
        self.chamfer.pick(
            stamp,
            self.chamfers
                .get(index)
                .ok_or("Projection changed")?
                .clone(),
        );
        if let Some(limo_cad_sketch::DrawingAnnotationDto::ChamferNote { position, .. }) =
            self.chamfer.annotation(0)
        {
            self.chamfer.move_to(position, size)?;
        }
        self.message.clear();
        Ok(())
    }
}
pub(in super::super) fn native(serial: u64, command: Command) -> NativeCommand {
    NativeCommand::Drawing(drawing_editor::Command::Annotation(serial, command))
}
pub(in super::super) fn owns_panel(world: &World) -> bool {
    world
        .get_resource::<Editor>()
        .is_some_and(|e| e.tool.is_some() || e.selected.is_some())
}
pub(in super::super) fn guard(world: &World) -> Result<(), String> {
    if world.get_resource::<Editor>().is_some_and(Editor::dirty) {
        Err("Apply or reset the annotation edit first".into())
    } else {
        Ok(())
    }
}
pub(in super::super) fn cancel_input(world: &mut World) {
    let changed = if let Some(mut e) = world.get_resource_mut::<Editor>() {
        e.drag.take().is_some()
    } else {
        false
    };
    if changed {
        let _ = super::input::refresh_preview(world);
    }
}
pub(in super::super) fn pointer_active(world: &World) -> bool {
    world
        .get_resource::<Editor>()
        .is_some_and(|editor| editor.drag.is_some())
}
/// A repair sheet may isolate its exact owning view when a broken sibling or
/// derived child prevents the complete sheet from projecting. This is solely
/// presentation; edits still use Editor.document and its original receipt.
pub(in super::super) fn repair_view(
    world: &World,
    sheet: &DrawingSheetDto,
    owner: &DocumentContext,
    revision: u64,
) -> Option<u64> {
    let editor = world.get_resource::<Editor>()?;
    let stamp = editor.stamp.as_ref()?;
    (repair::active(editor)
        && &stamp.owner == owner
        && stamp.revision == revision
        && stamp.sheet_id == sheet.id
        && sheet.views.iter().any(|v| v.id == editor.repair.view_id))
    .then_some(editor.repair.view_id)
}
pub(in super::super) fn preview<'a>(
    world: &World,
    sheet: &'a DrawingSheetDto,
    owner: &DocumentContext,
    revision: u64,
) -> std::borrow::Cow<'a, DrawingSheetDto> {
    use std::borrow::Cow;
    let Some(editor) = world.get_resource::<Editor>() else {
        return Cow::Borrowed(sheet);
    };
    let Some(drag) = &editor.drag else {
        let mut next = Cow::Borrowed(sheet);
        if editor.tool == Some(Tool::Chamfer)
            && editor.stamp.as_ref().is_some_and(|s| {
                s.sheet_id == sheet.id && &s.owner == owner && s.revision == revision
            })
        {
            if let Some(annotation) = editor
                .chamfer
                .annotation(editor.document.next_annotation_id)
            {
                next.to_mut().annotations.push(annotation);
            }
        }
        if editor.tool == Some(Tool::Linear)
            && editor.stamp.as_ref().is_some_and(|s| {
                s.sheet_id == sheet.id && &s.owner == owner && s.revision == revision
            })
            && editor.straight.valid()
        {
            if let Some(annotation) = editor
                .straight
                .annotation(editor.document.next_annotation_id)
            {
                next.to_mut().annotations.push(annotation);
            }
        }
        return next;
    };
    if drag.stamp.sheet_id != sheet.id
        || &drag.stamp.owner != owner
        || drag.stamp.revision != revision
    {
        return Cow::Borrowed(sheet);
    }
    if !drag.draft.dirty() {
        return Cow::Borrowed(sheet);
    }
    let mut next: Cow<'_, DrawingSheetDto> = Cow::Owned(sheet.clone());
    if let Some(a) = next
        .to_mut()
        .annotations
        .iter_mut()
        .find(|a| a.id() == drag.draft.selection().annotation_id)
    {
        *a = drag.draft.annotation().clone();
    }
    next
}
pub(in super::super) fn synchronize(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    owner: &DocumentContext,
    (height, side): (f32, f32),
    active: bool,
    state: &Workbench,
) -> Result<(), String> {
    let mut e = world.remove_resource::<Editor>().unwrap_or_default();
    e.widgets.begin();
    let result = (|| {
        if !active {
            e.inactive(owner);
            return Ok(());
        }
        let receipt = services
            .bridge
            .native_document_receipt(&services.engine, owner)?;
        let document = state
            .paper_document
            .as_ref()
            .filter(|(previous, _)| {
                previous.owner == *owner && previous.revision == receipt.revision
            })
            .map(|(_, document)| document.clone())
            .unwrap_or_else(|| Arc::new(services.engine.drawing_snapshot()));
        let Some(sheet) = document
            .sheets
            .iter()
            .find(|s| Some(s.id) == document.active_sheet_id)
        else {
            e.retire();
            return Ok(());
        };
        let stamp = Stamp {
            owner: owner.clone(),
            revision: receipt.revision,
            sheet_id: sheet.id,
        };
        if e.stamp.as_ref() != Some(&stamp) {
            let same = e
                .stamp
                .as_ref()
                .is_some_and(|s| s.owner == stamp.owner && s.sheet_id == stamp.sheet_id);
            let selected = if same {
                e.pending_selected.take().or(e.selected)
            } else {
                None
            };
            e.clear();
            e.document = Arc::clone(&document);
            e.stamp = Some(stamp.clone());
            if let Some(id) = selected.filter(|id| sheet.annotations.iter().any(|a| a.id() == *id))
            {
                e.select(id)?;
            }
        }
        let point_tool = matches!(
            e.tool,
            Some(Tool::Linear | Tool::Angular | Tool::Series(_) | Tool::Ordinate)
        );
        if !point_tool && !matches!(e.tool, Some(Tool::Technical(_))) {
            e.targets.clear();
            e.point_source = None;
        }
        if point_tool
            && e.point_source
                .as_ref()
                .is_none_or(|source| !drawing_paper::same_projection(state, source))
        {
            e.targets.clear();
            e.point_source = None;
            if let Some(result) = drawing_paper::with_projections(
                world,
                state,
                |projections, bases| -> Result<(), String> {
                    for (view, projection) in projections.values() {
                        let direction = bases
                            .get(&view.id)
                            .ok_or("Drawing projection basis is missing")?
                            .direction;
                        for a in anchors::endpoints(view, projection, direction)? {
                            e.targets.push(Target {
                                view_id: view.id,
                                reference: anchors::endpoint_ref(a, projection),
                                paper: drawing_paper::paper_point(view, a.point, projection),
                            });
                        }
                        if e.tool == Some(Tool::Linear) {
                            for a in anchors::circles(view, projection, direction, false)? {
                                e.targets.push(Target {
                                    view_id: view.id,
                                    reference: anchors::circle_ref(a, projection),
                                    paper: drawing_paper::paper_point(view, a.center, projection),
                                });
                            }
                        }
                    }
                    Ok(())
                },
            ) {
                result?;
                e.point_source = drawing_paper::projection_stamp(state);
                e.serial = e.serial.wrapping_add(1);
            }
        }
        if let Some(Tool::Radial(mode)) = e.tool {
            if e.radial_source
                .as_ref()
                .is_none_or(|source| !drawing_paper::same_projection(state, source))
            {
                e.circles.clear();
                e.radial_source = None;
                if let Some(result) = drawing_paper::with_projections(
                    world,
                    state,
                    |projections, bases| -> Result<(), String> {
                        for (view, projection) in projections.values() {
                            let direction = bases
                                .get(&view.id)
                                .ok_or("Drawing projection basis is missing")?
                                .direction;
                            e.circles
                                .extend(radial::targets(view, projection, direction, mode)?);
                        }
                        Ok(())
                    },
                ) {
                    result?;
                    e.radial_source = drawing_paper::projection_stamp(state);
                    e.serial = e.serial.wrapping_add(1);
                }
            }
        }
        if e.tool == Some(Tool::HoleNote)
            && e.hole_source
                .as_ref()
                .is_none_or(|source| !drawing_paper::same_projection(state, source))
        {
            e.circles.clear();
            e.hole_source = None;
            if let Some(result) =
                drawing_paper::with_projections(world, state, |projections, bases| {
                    let mut targets = Vec::new();
                    for (view, projection) in projections.values() {
                        if projection.circles.len() > 16_384 {
                            return Err("Too many circular hole targets in this view".to_owned());
                        }
                        let direction = bases
                            .get(&view.id)
                            .ok_or("Drawing projection basis is missing")?
                            .direction;
                        targets.extend(radial::targets(
                            view,
                            projection,
                            direction,
                            DrawingRadialDimensionMode::Diameter,
                        )?);
                        if targets.len() > 4096 {
                            return Err("Too many circular hole targets on this sheet".to_owned());
                        }
                    }
                    Ok::<_, String>(targets)
                })
            {
                e.circles = result?;
                e.hole_source = drawing_paper::projection_stamp(state);
                e.serial = e.serial.wrapping_add(1);
            }
        }
        if matches!(e.tool, Some(Tool::CenterMark | Tool::CenterLine))
            && e.center_source
                .as_ref()
                .is_none_or(|source| !drawing_paper::same_projection(state, source))
        {
            e.center.cancel();
            e.centers.clear();
            e.center_source = None;
            if let Some(result) =
                drawing_paper::with_projections(world, state, |projections, bases| {
                    let mut targets = Vec::new();
                    for (view, projection) in projections.values() {
                        let direction = bases
                            .get(&view.id)
                            .ok_or("Drawing projection basis is missing")?
                            .direction;
                        targets.extend(center::targets(view, projection, direction)?);
                        if targets.len() > 4096 {
                            return Err("Too many circular centers on this sheet".to_owned());
                        }
                    }
                    Ok::<_, String>(targets)
                })
            {
                e.centers = result?;
                e.center_source = drawing_paper::projection_stamp(state);
                e.serial = e.serial.wrapping_add(1);
            }
        }
        if e.tool == Some(Tool::Linear)
            && e.line_source
                .as_ref()
                .is_none_or(|source| !drawing_paper::same_projection(state, source))
        {
            e.straight.cancel();
            e.pair.cancel();
            e.lines.clear();
            e.line_source = None;
            if let Some(result) =
                drawing_paper::with_projections(world, state, |projections, bases| {
                    let scene = crate::native_viewport::interface_geometry(world).scene;
                    let mut lines = Vec::new();
                    for (view, projection) in projections.values() {
                        let direction = bases
                            .get(&view.id)
                            .ok_or("Drawing projection basis is missing")?
                            .direction;
                        lines.extend(straight::targets(scene, view, projection, direction)?);
                        if lines.len() > 4096 {
                            return Err("Too many straight-edge targets on this sheet".to_owned());
                        }
                        if lines
                            .iter()
                            .map(|line| line.pick_segments.len())
                            .sum::<usize>()
                            > 16_384
                        {
                            return Err(
                                "Too many rendered straight-edge pick segments on this sheet"
                                    .to_owned(),
                            );
                        }
                    }
                    Ok::<_, String>(lines)
                })
            {
                e.lines = result?;
                e.line_source = drawing_paper::projection_stamp(state);
                e.serial = e.serial.wrapping_add(1);
            }
        }
        if e.tool == Some(Tool::Chamfer)
            && e.chamfer_source
                .as_ref()
                .is_none_or(|source| !drawing_paper::same_projection(state, source))
        {
            e.chamfer.cancel();
            e.chamfers.clear();
            e.chamfer_source = None;
            if let Some(result) =
                drawing_paper::with_projections(world, state, |projections, bases| {
                    let scene = crate::native_viewport::interface_geometry(world).scene;
                    let mut targets = Vec::new();
                    for (view, projection) in projections.values() {
                        let direction = bases
                            .get(&view.id)
                            .ok_or("Drawing projection basis is missing")?
                            .direction;
                        targets.extend(chamfer::targets(scene, view, projection, direction)?);
                        if targets.len() > 4096
                            || targets
                                .iter()
                                .map(|t| t.line.pick_segments.len())
                                .sum::<usize>()
                                > 16_384
                        {
                            return Err("Too many chamfer targets on this sheet".to_owned());
                        }
                    }
                    Ok::<_, String>(targets)
                })
            {
                e.chamfers = result?;
                e.chamfer_source = drawing_paper::projection_stamp(state);
                e.serial = e.serial.wrapping_add(1);
            }
        }
        technical_runtime::synchronize(world, state, &mut e)?;
        super::panel::paint(world, camera, &mut e, height, side, state)?;
        Ok(())
    })();
    e.widgets.finish(world);
    world.insert_resource(e);
    result
}

pub(super) fn created_annotation(
    document: DrawingDocumentDto,
    stamp: &Stamp,
) -> Result<Value, String> {
    let id = document
        .next_annotation_id
        .checked_sub(1)
        .ok_or("Annotation ID was not allocated")?;
    let annotation = document
        .sheets
        .into_iter()
        .find(|s| s.id == stamp.sheet_id)
        .ok_or("Drawing sheet was removed")?
        .annotations
        .into_iter()
        .find(|a| a.id() == id)
        .ok_or("Created annotation was removed")?;
    Ok(json!({"sheet_id":stamp.sheet_id,"annotation":annotation}))
}

pub(in super::super) fn submit(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    stamp: &Stamp,
    operation: &'static str,
    args: Value,
) -> Result<Value, String> {
    if worker::available(world) {
        let expected = stamp.owner.clone();
        return worker::enqueue_operation(
            world,
            stamp.owner.clone(),
            stamp.revision,
            operation.into(),
            args,
            move |world, services, result| {
                let result = result.inspect_err(|error| {
                    if let Some(mut e) = world.get_resource_mut::<Editor>() {
                        if e.stamp.as_ref().is_some_and(|s| s.owner == expected) {
                            e.message = error.clone();
                        }
                    }
                })?;
                Ok(finish_mutation(
                    &services.engine,
                    &services.bridge,
                    world,
                    operation,
                    result,
                ))
            },
        );
    }
    let result = bridge.apply_native_mutation_at(
        engine,
        &stamp.owner,
        stamp.revision,
        operation,
        &args,
        || {
            if handle.frame().is_some_and(|f| f.context == stamp.owner) {
                Ok(())
            } else {
                Err("Drawing document changed".into())
            }
        },
    )?;
    Ok(finish_mutation(engine, bridge, world, operation, result))
}

pub(in super::super) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    serial: u64,
    command: &Command,
) -> Result<Value, String> {
    bridge
        .with_native_document_owner(engine, &action.context, || handle.validate_action(action))?;
    let receipt = bridge.native_document_receipt(engine, &action.context)?;
    let mut e = world
        .remove_resource::<Editor>()
        .ok_or("Open the Drawing workspace")?;
    let result = (|| {
        let stamp = e.stamp.clone().ok_or("Create a sheet first")?;
        if workspace(world) != Workspace::Drawing
            || stamp.owner != receipt.owner
            || stamp.revision != receipt.revision
            || (serial != e.serial && !matches!(command, Command::Tool(_)))
        {
            return Err("Drawing changed; use the refreshed controls".into());
        }
        if matches!(
            command,
            Command::Center(_)
                | Command::CenterGrip(_, _)
                | Command::Circle(_)
                | Command::Line(_)
                | Command::Chamfer(_)
                | Command::CloudEdge(_, _)
        ) && matches!(
            action.control.input,
            limo_cad_interface::ControlInput::DoubleClick
        ) {
            return Ok(json!({"handled":true}));
        }
        if matches!(command, Command::RepairRecord | Command::RepairReference) {
            repair::choose(world, &mut e, command, &action.control.input)?;
            e.message.clear();
            handle.invalidate_presentation();
            return Ok(json!({"updated":true}));
        }
        if let Command::Field(id) = command {
            let bom_input = if *id == fields::Id::Technical("/bom_item_id") {
                let current = e
                    .fields
                    .iter()
                    .find(|f| f.id == *id)
                    .ok_or("BOM field was removed")?
                    .text
                    .clone();
                let value = cam::choose(
                    &fields::bom_options(&e.document, stamp.sheet_id),
                    &current,
                    &action.control.input,
                )
                .map_err(|_| "Choose a BOM item from this sheet".to_owned())?;
                Some(limo_cad_interface::ControlInput::SetValue(value))
            } else {
                None
            };
            let changed = fields::edit(
                &mut e.fields,
                *id,
                bom_input.as_ref().unwrap_or(&action.control.input),
            )?;
            e.message.clear();
            return Ok(json!({"changed":changed}));
        }
        if !super::super::super::super::is_activation(&action.control.input) {
            return Ok(json!({"handled":true}));
        }
        if e.dirty()
            && matches!(
                command,
                Command::Tool(_)
                    | Command::Select(_)
                    | Command::CloudEdge(_, _)
                    | Command::Cancel
                    | Command::Anchor(_)
                    | Command::Center(_)
                    | Command::CenterGrip(_, _)
                    | Command::Circle(_)
                    | Command::Line(_)
                    | Command::Chamfer(_)
            )
        {
            return Err("Apply or reset the annotation edit first".into());
        }
        let mut request = None;
        if matches!(e.tool, Some(Tool::Technical(_)))
            && matches!(
                command,
                Command::Anchor(_) | Command::Circle(_) | Command::Line(_)
            )
        {
            drawing_editor::guard_sheet_edit(world)?;
            if let Some(next) = technical_runtime::pick(world, &mut e, &stamp, command)? {
                return submit(
                    world,
                    handle,
                    engine,
                    bridge,
                    &stamp,
                    "drawing_add_annotation",
                    created_annotation(next, &stamp)?,
                );
            }
            handle.invalidate_presentation();
            return Ok(json!({"updated":true}));
        }
        match command {
            Command::Tool(tool) => {
                drawing_editor::guard_sheet_edit(world)?;
                let previous = e.selected;
                e.clear();
                e.tool = Some(*tool);
                if repair::active(&e) {
                    let options = repair::options(&e.document, stamp.sheet_id);
                    let selected = previous.map(|id| format!("annotation:{id}"));
                    if let Some(key) = selected
                        .filter(|k| options.iter().any(|o| &o.value == k))
                        .or_else(|| options.first().map(|o| o.value.clone()))
                    {
                        repair::select(world, &mut e, key)?;
                    }
                }
                if *tool == Tool::Note {
                    let size = drawing_paper::transform(world.resource::<Workbench>())
                        .ok_or("Open drawing paper")?
                        .sheet_mm;
                    e.fields = fields::note_creation(size.map(|n| n * 0.5));
                }
            }
            Command::Select(id) | Command::CloudEdge(id, _) | Command::CenterGrip(id, _) => {
                drawing_editor::guard_sheet_edit(world)?;
                if e.selected != Some(*id) {
                    e.select(*id)?;
                }
            }
            Command::Anchor(index) => {
                let target = e.targets.get(*index).ok_or("Projection changed")?;
                match e.tool {
                    Some(Tool::Linear) => {
                        if e.straight.anchor(&stamp, target.clone()) {
                            e.pair.cancel();
                        } else if let Some(args) =
                            e.pair
                                .click(&stamp, target.view_id, target.reference.clone())
                        {
                            e.pending_selected = Some(e.document.next_annotation_id);
                            request = Some((
                                "drawing_add_linear_dimension",
                                serde_json::to_value(args).map_err(|x| x.to_string())?,
                            ));
                        }
                    }
                    Some(Tool::Angular) => {
                        if let Some(args) = e.angular.click(
                            &stamp,
                            target.view_id,
                            target.reference.clone(),
                            target.paper,
                        )? {
                            e.pending_selected = Some(e.document.next_annotation_id);
                            request = Some((
                                "drawing_add_angular_dimension",
                                serde_json::to_value(args).map_err(|x| x.to_string())?,
                            ));
                        }
                    }
                    Some(Tool::Series(_) | Tool::Ordinate) => {
                        let layout = match e.tool {
                            Some(Tool::Series(layout)) => Some(layout),
                            _ => None,
                        };
                        if let Some(next) = e.series.click(
                            &stamp,
                            target.view_id,
                            target.reference.clone(),
                            layout,
                            &e.document,
                        )? {
                            e.pending_selected = Some(e.document.next_annotation_id);
                            request =
                                Some(("drawing_add_annotation", created_annotation(next, &stamp)?));
                        }
                    }
                    _ => return Err("Choose a dimension anchor tool first".into()),
                }
            }
            Command::Center(index) => {
                drawing_editor::guard_sheet_edit(world)?;
                if !matches!(e.tool, Some(Tool::CenterMark | Tool::CenterLine)) {
                    return Err("Choose Center mark or Centerline between circles first".into());
                }
                if e.center_source.as_ref().is_none_or(|s| {
                    !drawing_paper::same_projection(world.resource::<Workbench>(), s)
                }) {
                    return Err("Projection changed; choose refreshed circles".into());
                }
                let target = e.centers.get(*index).ok_or("Projected center changed")?;
                if let Some(next) = e.center.click(
                    &stamp,
                    target,
                    e.tool == Some(Tool::CenterLine),
                    &e.document,
                )? {
                    e.pending_selected = Some(e.document.next_annotation_id);
                    request = Some(("drawing_add_annotation", created_annotation(next, &stamp)?));
                }
            }
            Command::Circle(index) => {
                if e.tool == Some(Tool::HoleNote) {
                    drawing_editor::guard_sheet_edit(world)?;
                    if e.hole_source.as_ref().is_none_or(|source| {
                        !drawing_paper::same_projection(world.resource::<Workbench>(), source)
                    }) {
                        return Err("Projection changed; choose the refreshed hole circle".into());
                    }
                    let target = e.circles.get(*index).ok_or("Projected circle changed")?;
                    e.pending_selected = Some(e.document.next_annotation_id);
                    return hole::submit(world, handle, engine, bridge, &stamp, target);
                }
                let Some(Tool::Radial(mode)) = e.tool else {
                    return Err("Choose Radius or Diameter first".into());
                };
                let target = e.circles.get(*index).ok_or("Projection changed")?;
                let args = radial::request(&stamp, target, mode)?;
                e.pending_selected = Some(e.document.next_annotation_id);
                request = Some((
                    "drawing_add_radial_dimension",
                    serde_json::to_value(args).map_err(|x| x.to_string())?,
                ));
            }
            Command::Line(index) => {
                drawing_editor::guard_sheet_edit(world)?;
                if e.line_source.as_ref().is_none_or(|source| {
                    !drawing_paper::same_projection(world.resource::<Workbench>(), source)
                }) {
                    return Err("Projection changed; choose refreshed geometry".into());
                }
                e.pick_line(&stamp, *index)?;
            }
            Command::Chamfer(index) => {
                drawing_editor::guard_sheet_edit(world)?;
                if e.chamfer_source.as_ref().is_none_or(|source| {
                    !drawing_paper::same_projection(world.resource::<Workbench>(), source)
                }) {
                    return Err("Projection changed; choose refreshed geometry".into());
                }
                let size = drawing_paper::transform(world.resource::<Workbench>())
                    .ok_or("Open drawing paper")?
                    .sheet_mm;
                e.pick_chamfer(&stamp, *index, size)?;
            }
            Command::Apply => {
                if repair::active(&e) {
                    if e.technical_source.as_ref().is_none_or(|s| {
                        !drawing_paper::same_projection(world.resource::<Workbench>(), s)
                    }) {
                        return Err("Projection changed; choose refreshed geometry".into());
                    }
                    let next = repair::apply(&e)?;
                    e.pending_selected = e
                        .repair
                        .record
                        .strip_prefix("annotation:")
                        .and_then(|id| id.parse().ok());
                    request = Some((
                        "drawing_set_document",
                        serde_json::to_value(next).map_err(|x| x.to_string())?,
                    ));
                } else if e.tool == Some(Tool::Note) {
                    let note = fields::note_request(stamp.sheet_id, &e.fields)?;
                    e.pending_selected = Some(e.document.next_annotation_id);
                    request = Some((
                        "drawing_add_note",
                        serde_json::to_value(note).map_err(|x| x.to_string())?,
                    ));
                } else if e.tool == Some(Tool::Linear) && e.straight.active() {
                    if e.line_source.as_ref().is_none_or(|source| {
                        !drawing_paper::same_projection(world.resource::<Workbench>(), source)
                    }) {
                        return Err("Projection changed; choose refreshed geometry".into());
                    }
                    let next = e.straight.create(&e.document, &stamp)?;
                    e.pending_selected = Some(e.document.next_annotation_id);
                    request = Some(("drawing_add_annotation", created_annotation(next, &stamp)?));
                    e.straight.cancel();
                } else if e.tool == Some(Tool::Chamfer) && e.chamfer.active() {
                    if e.chamfer_source.as_ref().is_none_or(|source| {
                        !drawing_paper::same_projection(world.resource::<Workbench>(), source)
                    }) {
                        return Err("Projection changed; choose refreshed geometry".into());
                    }
                    let next = e.chamfer.create(&e.document, &stamp)?;
                    e.pending_selected = Some(e.document.next_annotation_id);
                    request = Some(("drawing_add_annotation", created_annotation(next, &stamp)?));
                    e.chamfer.cancel();
                } else if let Some(draft) = &mut e.draft {
                    fields::apply(draft, &e.fields)?;
                    if draft.dirty() {
                        draft.verify(&e.document)?;
                        request = Some((
                            "drawing_update_annotation",
                            json!({"sheet_id":draft.selection().sheet_id,"annotation":draft.annotation()}),
                        ));
                    } else {
                        e.fields = fields::from_annotation(draft.annotation());
                    }
                }
            }
            Command::Reset => {
                if let Some(id) = e.selected {
                    e.select(id)?;
                } else if e.tool == Some(Tool::Note) {
                    let size = drawing_paper::transform(world.resource::<Workbench>())
                        .ok_or("Open drawing paper")?
                        .sheet_mm;
                    e.fields = fields::note_creation(size.map(|n| n * 0.5));
                } else if e.tool == Some(Tool::Linear) {
                    e.straight.cancel();
                    e.pair.cancel();
                } else if e.tool == Some(Tool::Chamfer) {
                    e.chamfer.cancel();
                } else if matches!(e.tool, Some(Tool::CenterMark | Tool::CenterLine)) {
                    e.center.cancel();
                } else if matches!(e.tool, Some(Tool::Technical(_))) {
                    e.technical.cancel();
                    e.repair.pending = None;
                } else if e.tool == Some(Tool::RevisionCloud) {
                    e.cloud.cancel();
                }
            }
            Command::Delete => {
                let draft = e.draft.as_ref().ok_or("Select an annotation")?;
                draft.verify(&e.document)?;
                request = Some((
                    "drawing_delete_annotation",
                    json!({"sheet_id":draft.selection().sheet_id,"annotation_id":draft.selection().annotation_id}),
                ));
                e.pending_selected = None;
            }
            Command::Cancel => e.clear(),
            Command::Fields(delta) => e.page = e.page.saturating_add_signed(*delta as isize),
            Command::Field(_) | Command::RepairRecord | Command::RepairReference => unreachable!(),
        }
        e.message.clear();
        if let Some((operation, args)) = request {
            return submit(world, handle, engine, bridge, &stamp, operation, args);
        }
        handle.invalidate_presentation();
        Ok(json!({"updated":true}))
    })();
    if let Err(error) = &result {
        e.message = error.clone();
    }
    world.insert_resource(e);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn annotation_editor_shares_paper_snapshots_and_releases_retired_documents() {
        use crate::session_bridge::native_interface::tests::Fixture;
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let owner = fixture.owner();
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &owner,
                "drawing_set_document",
                &serde_json::to_value(super::super::tests::document()).unwrap(),
                || Ok(()),
            )
            .unwrap();
        let services = NativeServices {
            engine: fixture.engine.clone(),
            bridge: fixture.bridge.clone(),
        };
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap();
        let document = Arc::new(fixture.engine.drawing_snapshot());
        let old = Arc::downgrade(&document);
        let mut state = Workbench {
            paper_document: Some((receipt, document)),
            ..default()
        };
        let mut app = crate::native_viewport::interface_scene_fixture();
        let world = app.world_mut();
        let camera = world.spawn(InterfaceCamera).id();
        synchronize(world, camera, &services, &owner, (800., 240.), true, &state).unwrap();
        let document = &state.paper_document.as_ref().unwrap().1;
        assert!(Arc::ptr_eq(&world.resource::<Editor>().document, document));
        world.resource_mut::<Editor>().select(1).unwrap();
        world.resource_mut::<Editor>().fields[0].text = "Unapplied note".into();
        synchronize(world, camera, &services, &owner, (800., 240.), true, &state).unwrap();
        assert!(Arc::ptr_eq(&world.resource::<Editor>().document, document));
        assert_eq!(world.resource::<Editor>().fields[0].text, "Unapplied note");
        let mut revised = document.as_ref().clone();
        revised.sheets[0].name = "Revised sheet".into();
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &owner,
                "drawing_set_document",
                &serde_json::to_value(revised).unwrap(),
                || Ok(()),
            )
            .unwrap();
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap();
        state.paper_document = Some((receipt, Arc::new(fixture.engine.drawing_snapshot())));
        synchronize(world, camera, &services, &owner, (800., 240.), true, &state).unwrap();
        assert!(old.upgrade().is_none());
        let document = &state.paper_document.as_ref().unwrap().1;
        assert!(Arc::ptr_eq(&world.resource::<Editor>().document, document));
        assert_eq!(
            world.resource::<Editor>().document.sheets[0].name,
            "Revised sheet"
        );
        let retired = Arc::downgrade(document);
        state.paper_document = None;
        let mut replacement = owner;
        replacement.epoch += 1;
        synchronize(
            world,
            camera,
            &services,
            &replacement,
            (800., 240.),
            false,
            &state,
        )
        .unwrap();
        assert!(retired.upgrade().is_none());
        assert!(world.resource::<Editor>().document.sheets.is_empty());
    }
    #[test]
    fn idle_annotation_preview_borrows_the_saved_sheet() {
        let document = super::super::tests::document();
        let sheet = &document.sheets[0];
        let owner = DocumentContext {
            window_id: "main".into(),
            document_id: "drawing".into(),
            epoch: 1,
        };
        let mut world = World::new();
        for install_editor in [false, true] {
            if install_editor {
                world.insert_resource(Editor::default());
            }
            let rendered = preview(&world, sheet, &owner, 1);
            assert!(matches!(rendered, std::borrow::Cow::Borrowed(_)));
            assert!(std::ptr::eq(rendered.as_ref(), sheet));
        }
    }

    #[test]
    fn busy_read_cancels_pointer_drag_but_preserves_the_valid_anchor_pair() {
        let document = super::super::tests::document();
        let projection = super::super::tests::projection();
        let stamp = Stamp {
            owner: DocumentContext {
                window_id: "main".into(),
                document_id: "drawing".into(),
                epoch: 1,
            },
            revision: 12,
            sheet_id: 1,
        };
        let first = anchors::endpoint_ref(&projection.anchors[4], &projection);
        let second = anchors::endpoint_ref(&projection.anchors[5], &projection);
        let mut e = Editor {
            stamp: Some(stamp.clone()),
            document: Arc::new(document.clone()),
            ..default()
        };
        e.cloud.click(&stamp, [30., 40.], &document).unwrap();
        e.cloud.click(&stamp, [60., 40.], &document).unwrap();
        e.pair.click(&stamp, 1, first.clone());
        e.series
            .click(
                &stamp,
                1,
                first.clone(),
                Some(DrawingChainDimensionLayout::Baseline),
                &document,
            )
            .unwrap();
        e.angular.click(&stamp, 1, first.clone(), [0., 0.]).unwrap();
        e.angular
            .click(&stamp, 1, second.clone(), [40., 0.])
            .unwrap();
        e.drag = Some(Drag {
            stamp: stamp.clone(),
            start: [20., 30.],
            draft: Draft::new(
                &document,
                Selection {
                    sheet_id: 1,
                    annotation_id: 1,
                },
            )
            .unwrap(),
            linear_points: None,
            radial: None,
            angular: None,
            ordinate_points: None,
            moved: false,
            center: None,
            projection: None,
        });
        let mut world = World::new();
        world.insert_resource(e);
        cancel_input(&mut world);
        let mut e = world.resource_mut::<Editor>();
        assert!(e.drag.is_none());
        assert_eq!(
            e.cloud.points,
            vec![[30., 40.], [60., 40.]],
            "Read-only workers preserve cloud staging"
        );
        assert_eq!(
            e.series.picks,
            vec![first.clone()],
            "Read-only workers preserve a stamped series selection"
        );
        assert_eq!(
            e.angular.picks.len(),
            2,
            "Read-only work must preserve both angular picks"
        );
        let mut third = second.clone();
        third.edge_id = limo_cad_core::EdgeId(7);
        third.edge_key = "vertical".into();
        third.fallback_point = [0., 30., 6.];
        let angle = e
            .angular
            .click(&stamp, 1, third.clone(), [0., 30.])
            .unwrap()
            .unwrap();
        assert_eq!(
            (angle.vertex, angle.first, angle.second),
            (first.clone(), second.clone(), third)
        );
        let added = e
            .pair
            .click(&stamp, 1, second)
            .expect("Read-only work must preserve the first anchor");
        assert_eq!(added.first, first);
        e.pair.click(&stamp, 1, first.clone());
        let mut changed = stamp.clone();
        changed.revision += 1;
        e.pair.observe(&changed);
        assert!(e.pair.first.is_none());
        e.pair.click(&stamp, 1, first);
        changed = stamp;
        changed.owner.epoch += 1;
        e.pair.observe(&changed);
        assert!(e.pair.first.is_none());
    }
    #[test]
    fn workspace_switch_keeps_same_owner_form_but_retires_old_document_draft() {
        let owner = DocumentContext {
            window_id: "main".into(),
            document_id: "drawing".into(),
            epoch: 1,
        };
        let mut e = Editor {
            document: Arc::new(super::super::tests::document()),
            stamp: Some(Stamp {
                owner: owner.clone(),
                revision: 12,
                sheet_id: 1,
            }),
            ..default()
        };
        e.select(1).unwrap();
        e.fields[0].text = "Unapplied \u{96f6}\u{4ef6}".into();
        assert!(e.dirty());
        let serial = e.serial;
        e.inactive(&owner);
        assert!(e.dirty());
        assert_eq!(e.selected, Some(1));
        assert_eq!(e.fields[0].text, "Unapplied \u{96f6}\u{4ef6}");
        assert_eq!(e.serial, serial);
        let mut replacement = owner;
        replacement.epoch += 1;
        e.inactive(&replacement);
        assert!(!e.dirty());
        assert!(e.fields.is_empty());
        assert!(e.stamp.is_none());
        assert!(e.selected.is_none());
    }
}
