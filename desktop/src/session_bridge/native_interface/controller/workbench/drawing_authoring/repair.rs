//! Explicit reassociation changes one chosen shared reference. A replacement
//! stays disposable until Apply; stale document/projection receipts retire it.
use super::super::*;
use super::{
    runtime::{Command, Editor},
    technical, Tool,
};
use limo_cad_interface::{ChoiceOption, ControlInput};
use limo_cad_occt::drawing_presentation::references::Resolver;
use limo_cad_sketch::*;
use serde_json::{json, Value};
mod panel;
#[cfg(test)]
mod tests;
pub(super) use panel::paint;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Anchor,
    Circle,
    Line,
}
pub(super) struct Reference {
    pub path: String,
    pub label: String,
    pub kind: Kind,
}
pub(super) struct Replacement {
    value: Value,
    command: Command,
}
#[derive(Default)]
pub(super) struct State {
    pub record: String,
    pub view_id: u64,
    pub reference: usize,
    pub references: Vec<Reference>,
    source: Value,
    pub pending: Option<Replacement>,
}
pub(super) fn selected(e: &Editor, command: &Command) -> bool {
    active(e)
        && e.repair
            .pending
            .as_ref()
            .is_some_and(|p| &p.command == command)
}
pub(super) fn active(e: &Editor) -> bool {
    e.tool == Some(Tool::Technical(technical::Tool::Repair))
}
pub(super) fn allows(e: &Editor, kind: Kind) -> bool {
    !active(e)
        || e.repair
            .references
            .get(e.repair.reference)
            .is_some_and(|r| r.kind == kind)
}
fn label(a: &DrawingAnnotationDto) -> &'static str {
    use DrawingAnnotationDto::*;
    match a {
        LinearDimension { .. } => "Linear dimension",
        LineDimension { .. } => "Edge dimension",
        PointLineDimension { .. } => "Point-line dimension",
        Note { .. } => "Note",
        RadialDimension { .. } => "Radial dimension",
        AngularDimension { .. } => "Angle",
        HoleNote { .. } => "Hole note",
        ChamferNote { .. } => "Chamfer note",
        CenterMark { .. } => "Center mark",
        CenterLine { .. } => "Centerline",
        CenterLineBetweenEdges { .. } => "Edge centerline",
        AutomaticSymmetryAxis { .. } => "Symmetry axis",
        BoltCircleCenterLine { .. } => "Bolt circle",
        ChainDimension { .. } => "Dimension series",
        OrdinateDimension { .. } => "Ordinate dimension",
        ArcLengthDimension { .. } => "Arc length",
        JoggedRadiusDimension { .. } => "Jogged radius",
        DatumFeature { .. } => "Datum",
        GdtFrame { .. } => "GD&T",
        SurfaceTexture { .. } => "Surface texture",
        EdgeRequirement { .. } => "Edge requirement",
        WeldSymbol { .. } => "Weld",
        ItemBalloon { .. } => "Balloon",
        RevisionCloud { .. } => "Revision cloud",
    }
}
pub(super) fn options(document: &DrawingDocumentDto, sheet_id: u64) -> Vec<ChoiceOption> {
    let Some(sheet) = document.sheets.iter().find(|s| s.id == sheet_id) else {
        return vec![];
    };
    let mut options: Vec<_> = sheet
        .annotations
        .iter()
        .filter(|a| {
            !matches!(
                a,
                DrawingAnnotationDto::Note { .. }
                    | DrawingAnnotationDto::AutomaticSymmetryAxis { .. }
                    | DrawingAnnotationDto::RevisionCloud { .. }
            )
        })
        .map(|a| ChoiceOption {
            value: format!("annotation:{}", a.id()),
            label: format!("{} · {}", a.id(), label(a)),
            disabled: false,
        })
        .collect();
    options.extend(
        sheet
            .views
            .iter()
            .filter(|v| {
                matches!(
                    v.derivation,
                    Some(
                        DrawingViewDerivationDto::Section { .. }
                            | DrawingViewDerivationDto::RemovedSection { .. }
                            | DrawingViewDerivationDto::Detail { .. }
                            | DrawingViewDerivationDto::Auxiliary { .. }
                    )
                )
            })
            .map(|v| ChoiceOption {
                value: format!("view:{}", v.id),
                label: format!(
                    "View {} · {}",
                    v.id,
                    v.name.chars().take(80).collect::<String>()
                ),
                disabled: false,
            }),
    );
    options
}
fn record(document: &DrawingDocumentDto, sheet_id: u64, key: &str) -> Result<Value, String> {
    let sheet = document
        .sheets
        .iter()
        .find(|s| s.id == sheet_id)
        .ok_or("Sheet was removed")?;
    let (family, id) = key
        .split_once(':')
        .ok_or("Choose an annotation or derived view")?;
    let id = id.parse::<u64>().map_err(|_| "Invalid drawing selection")?;
    match family {
        "annotation" => serde_json::to_value(
            sheet
                .annotations
                .iter()
                .find(|a| a.id() == id)
                .ok_or("Annotation was removed")?,
        ),
        "view" => serde_json::to_value(
            sheet
                .views
                .iter()
                .find(|v| v.id == id)
                .ok_or("View was removed")?,
        ),
        _ => return Err("Choose an annotation or derived view".into()),
    }
    .map_err(|e| e.to_string())
}
fn references(value: &Value, derived: bool) -> Vec<Reference> {
    let mut refs = Vec::new();
    let mut add = |path: &str, label: &str, kind| {
        refs.push(Reference {
            path: path.into(),
            label: label.into(),
            kind,
        })
    };
    if derived {
        match value["derivation"]["type"].as_str().unwrap_or("") {
            "section" | "removed_section" => {
                add("/derivation/first", "Cutting plane start", Kind::Anchor);
                add("/derivation/second", "Cutting plane end", Kind::Anchor);
            }
            "detail" => add("/derivation/center", "Detail center", Kind::Anchor),
            "auxiliary" => add(
                "/derivation/reference",
                "Auxiliary reference edge",
                Kind::Line,
            ),
            _ => {}
        }
        return refs;
    }
    match value["kind"].as_str().unwrap_or("") {
        "linear_dimension" | "chamfer_note" => {
            add("/first", "First endpoint", Kind::Anchor);
            add("/second", "Second endpoint", Kind::Anchor);
        }
        "line_dimension" | "center_line_between_edges" => {
            add("/first", "First edge", Kind::Line);
            if !value["second"].is_null() {
                add("/second", "Second edge", Kind::Line);
            }
        }
        "point_line_dimension" => {
            add("/point", "Dimension point", Kind::Anchor);
            add("/line", "Dimension edge", Kind::Line);
        }
        "angular_dimension" => {
            add("/vertex", "Vertex", Kind::Anchor);
            add("/first", "First ray", Kind::Anchor);
            add("/second", "Second ray", Kind::Anchor);
        }
        "chain_dimension" => {
            for i in 0..value["anchors"].as_array().map_or(0, Vec::len) {
                add(
                    &format!("/anchors/{i}"),
                    &format!("Dimension point {}", i + 1),
                    Kind::Anchor,
                );
            }
        }
        "ordinate_dimension" => {
            add("/origin", "Origin", Kind::Anchor);
            add("/target", "Target", Kind::Anchor);
        }
        "arc_length_dimension" => {
            add("/feature", "Arc feature", Kind::Circle);
            add("/first", "Arc start", Kind::Anchor);
            add("/second", "Arc end", Kind::Anchor);
        }
        "radial_dimension" | "hole_note" | "center_mark" | "jogged_radius_dimension" => {
            add("/feature", "Circular feature", Kind::Circle)
        }
        "center_line" => {
            add("/first", "First center", Kind::Circle);
            add("/second", "Second center", Kind::Circle);
        }
        "bolt_circle_center_line" => {
            for i in 0..value["features"].as_array().map_or(0, Vec::len) {
                add(
                    &format!("/features/{i}"),
                    &format!("Bolt circle center {}", i + 1),
                    Kind::Circle,
                );
            }
        }
        "edge_requirement" | "weld_symbol" => add("/attachment", "Attachment edge", Kind::Line),
        "datum_feature" | "gdt_frame" | "surface_texture" | "item_balloon" => {
            let kind = match value["attachment"]["type"].as_str() {
                Some("anchor") => Kind::Anchor,
                Some("circle") => Kind::Circle,
                _ => Kind::Line,
            };
            add("/attachment/reference", "Attachment", kind);
        }
        _ => {}
    }
    refs
}
pub(super) fn select(world: &World, e: &mut Editor, key: String) -> Result<(), String> {
    let stamp = e.stamp.as_ref().ok_or("Create a sheet first")?;
    if !options(&e.document, stamp.sheet_id)
        .iter()
        .any(|o| o.value == key)
    {
        return Err("Choose an available annotation or derived view".into());
    }
    let source = record(&e.document, stamp.sheet_id, &key)?;
    let derived = key.starts_with("view:");
    let view_id = if derived {
        source["derivation"]["parent_view_id"].as_u64()
    } else {
        source["view_id"].as_u64()
    }
    .ok_or("Reference view was removed")?;
    let refs = references(&source, derived);
    let reference =
        drawing_paper::with_projections(world, world.resource::<Workbench>(), |projections, _| {
            let (view, projection) = projections.get(&view_id)?;
            let resolver = Resolver { view, projection };
            refs.iter().position(|r| {
                let value = source.pointer(&r.path).cloned().unwrap_or(Value::Null);
                match r.kind {
                    Kind::Anchor => serde_json::from_value(value)
                        .ok()
                        .and_then(|r| resolver.anchor(&r))
                        .is_none(),
                    Kind::Circle => serde_json::from_value(value)
                        .ok()
                        .and_then(|r| resolver.circle(&r))
                        .is_none(),
                    Kind::Line => serde_json::from_value(value)
                        .ok()
                        .and_then(|r| resolver.line(&r))
                        .is_none(),
                }
            })
        })
        .flatten()
        .unwrap_or(0);
    e.repair = State {
        record: key,
        view_id,
        reference,
        references: refs,
        source,
        pending: None,
    };
    e.technical_source = None;
    e.technical.cancel();
    e.serial = e.serial.wrapping_add(1);
    Ok(())
}
pub(super) fn choose(
    world: &World,
    e: &mut Editor,
    command: &Command,
    input: &ControlInput,
) -> Result<(), String> {
    if !active(e) {
        return Err("Choose Reassociate references first".into());
    }
    if e.repair.pending.is_some() {
        return Err("Apply or reset the replacement first".into());
    }
    match command {
        Command::RepairRecord => {
            let options = options(
                &e.document,
                e.stamp.as_ref().ok_or("Create a sheet first")?.sheet_id,
            );
            let key = cam::choose(&options, &e.repair.record, input)
                .map_err(|_| "Choose an available drawing annotation or derived view".to_owned())?;
            select(world, e, key)?;
        }
        Command::RepairReference => {
            let options = reference_options(&e.repair);
            let value = cam::choose(&options, &e.repair.reference.to_string(), input)
                .map_err(|_| "Choose an available drawing reference".to_owned())?;
            e.repair.reference = value.parse().map_err(|_| "Choose a reference")?;
            e.technical_source = None;
            e.serial = e.serial.wrapping_add(1);
        }
        _ => return Err("Choose a repair field".into()),
    }
    Ok(())
}
fn reference_options(state: &State) -> Vec<ChoiceOption> {
    state
        .references
        .iter()
        .enumerate()
        .map(|(i, r)| ChoiceOption {
            value: i.to_string(),
            label: r.label.clone(),
            disabled: false,
        })
        .collect()
}
pub(super) fn pick(e: &mut Editor, command: &Command) -> Result<(), String> {
    let expected = e
        .repair
        .references
        .get(e.repair.reference)
        .ok_or("Choose a reference")?;
    let (view_id, kind, value) = match command {
        Command::Anchor(i) => {
            let t = e.targets.get(*i).ok_or("Anchor changed")?;
            (t.view_id, Kind::Anchor, json!(t.reference))
        }
        Command::Circle(i) => {
            let t = e.circles.get(*i).ok_or("Circle changed")?;
            (t.view_id, Kind::Circle, json!(t.reference))
        }
        Command::Line(i) => {
            let t = e.lines.get(*i).ok_or("Edge changed")?;
            (t.view_id, Kind::Line, json!(t.reference))
        }
        _ => return Err("Choose a projected reference".into()),
    };
    if view_id != e.repair.view_id || kind != expected.kind {
        return Err("Choose the requested geometry in its owning view".into());
    }
    if e.repair.pending.is_some() {
        return Err("Apply or reset the replacement first".into());
    }
    e.repair.pending = Some(Replacement {
        value,
        command: command.clone(),
    });
    Ok(())
}
pub(super) fn apply(e: &Editor) -> Result<DrawingDocumentDto, String> {
    let stamp = e.stamp.as_ref().ok_or("Create a sheet first")?;
    if record(&e.document, stamp.sheet_id, &e.repair.record)? != e.repair.source {
        return Err("Drawing changed; choose the refreshed reference".into());
    }
    let mut value = e.repair.source.clone();
    let reference = e
        .repair
        .references
        .get(e.repair.reference)
        .ok_or("Choose a reference")?;
    *value
        .pointer_mut(&reference.path)
        .ok_or("Reference was removed")? = e
        .repair
        .pending
        .as_ref()
        .ok_or("Choose replacement geometry first")?
        .value
        .clone();
    let mut next = e.document.as_ref().clone();
    let sheet = next
        .sheets
        .iter_mut()
        .find(|s| s.id == stamp.sheet_id)
        .ok_or("Sheet was removed")?;
    if e.repair.record.starts_with("view:") {
        let view: DrawingViewDto = serde_json::from_value(value).map_err(|e| e.to_string())?;
        let id = view.id;
        *sheet
            .views
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or("View was removed")? = view;
    } else {
        let annotation: DrawingAnnotationDto =
            serde_json::from_value(value).map_err(|e| e.to_string())?;
        let id = annotation.id();
        *sheet
            .annotations
            .iter_mut()
            .find(|a| a.id() == id)
            .ok_or("Annotation was removed")? = annotation;
    }
    if sheet.release.status == DrawingReleaseStatus::Released {
        sheet.release.status = DrawingReleaseStatus::Draft;
    }
    next.validate()?;
    Ok(next)
}
