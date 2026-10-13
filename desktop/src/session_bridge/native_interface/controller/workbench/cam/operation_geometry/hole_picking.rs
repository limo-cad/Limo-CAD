//! Transient row identity for the existing hole form. No click changes CAM DTOs.
use super::*;
use std::hash::{Hash, Hasher};

pub(super) const BUTTON: &str = "/native/ui/geometry_pick_holes";
const ORDER: &str = "/native/geometry/hole_order";
const MAX_PICK_ROWS: usize = 4096;
const MAX_PICK_TEXT: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(in super::super) struct FaceKey {
    pub body_id: u64,
    pub face_id: u64,
}
impl FaceKey {
    pub fn parse(reference: &str) -> Result<Self, String> {
        let (body, face) = reference
            .split_once(':')
            .ok_or("Choose a body:face hole reference")?;
        Ok(Self {
            body_id: body.parse().map_err(|_| "Invalid hole body identity")?,
            face_id: face.parse().map_err(|_| "Invalid hole face identity")?,
        })
    }
    pub fn reference(self) -> String {
        format!("{}:{}", self.body_id, self.face_id)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(in super::super) struct SelectionState {
    pub selection: Selection,
    pub keys: Vec<FaceKey>,
    pub epoch: u64,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum Row {
    Saved(usize),
    Picked(FaceKey),
    Empty,
}
fn stored(draft: &Draft) -> &[Value] {
    draft.record["holes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
}
fn order(draft: &Draft) -> Result<Vec<Row>, String> {
    let length = count(draft, holes::COUNT, 250_000)?;
    let mut rows = if let Ok(value) = form::text(draft, ORDER) {
        serde_json::from_str::<Vec<Row>>(value)
            .map_err(|_| "Hole row order changed; reopen Geometry")?
    } else {
        (0..length)
            .map(|i| {
                if i < stored(draft).len() {
                    Row::Saved(i)
                } else {
                    Row::Empty
                }
            })
            .collect()
    };
    rows.resize(length, Row::Empty);
    Ok(rows)
}
fn baseline_for(draft: &Draft, context: &Context, row: &Row) -> Result<Value, String> {
    match row {
        Row::Saved(index) => stored(draft)
            .get(*index)
            .cloned()
            .ok_or_else(|| "Original hole was removed; reopen Geometry".into()),
        Row::Picked(key) => {
            let reference = key.reference();
            let hole = context
                .holes
                .iter()
                .find(|(_, hole)| hole.face_key.as_deref() == Some(reference.as_str()))
                .ok_or("Picked hole is unavailable; remove or reselect its face")?;
            serde_json::to_value(&hole.1).map_err(|error| error.to_string())
        }
        Row::Empty => Ok(Value::Null),
    }
}
pub(super) fn baseline(draft: &Draft, context: &Context, index: usize) -> Result<Value, String> {
    let rows = order(draft)?;
    baseline_for(
        draft,
        context,
        rows.get(index).ok_or("Select an available hole row")?,
    )
}
pub(super) fn baselines(draft: &Draft, context: &Context) -> Result<Vec<Value>, String> {
    let canonical = holes::canonical_holes(context);
    order(draft)?
        .iter()
        .map(|row| match row {
            Row::Picked(key) => canonical
                .get(key)
                .ok_or_else(|| "Picked hole is unavailable; remove or reselect its face".to_owned())
                .and_then(|hole| serde_json::to_value(hole).map_err(|error| error.to_string())),
            _ => baseline_for(draft, context, row),
        })
        .collect()
}
fn raw_fields(draft: &Draft) -> HashMap<&str, &str> {
    draft
        .fields
        .iter()
        .filter(|field| field.path.starts_with("/native/geometry/holes/"))
        .map(|field| (field.path.as_str(), field.text.trim()))
        .collect()
}
fn reference(
    draft: &Draft,
    fields: &HashMap<&str, &str>,
    index: usize,
    row: &Row,
) -> Result<Option<FaceKey>, String> {
    let prefix = holes::prefix(index);
    let picked = match row {
        Row::Picked(key) => Some(key.reference()),
        _ => None,
    };
    let baseline = match row {
        Row::Saved(slot) => stored(draft)
            .get(*slot)
            .ok_or("Original hole was removed; reopen Geometry")?["face_key"]
            .as_str(),
        Row::Picked(_) => picked.as_deref(),
        Row::Empty => None,
    };
    let source = if baseline.is_some() || matches!(row, Row::Empty) {
        "face"
    } else {
        "manual"
    };
    match fields
        .get(format!("{prefix}/source").as_str())
        .copied()
        .unwrap_or(source)
    {
        "manual" => Ok(None),
        "face" => match fields
            .get(format!("{prefix}/face").as_str())
            .copied()
            .or(baseline)
        {
            Some(value) if !value.is_empty() => FaceKey::parse(value).map(Some),
            _ => Ok(None),
        },
        _ => Err("Choose a hole source".into()),
    }
}
fn epoch(draft: &Draft) -> Result<u64, String> {
    if !matches!(draft.record["kind"].as_str(), Some("drill" | "thread"))
        || form::text(draft, "/native/ui/operation_section")? != "geometry"
    {
        return Err("Open drill or thread Geometry before picking holes".into());
    }
    if count(draft, holes::COUNT, 250_000)? > MAX_PICK_ROWS {
        return Err(
            "Viewport picking supports at most 4096 hole rows; use the existing geometry fields"
                .into(),
        );
    }
    let context = operation_editor::geometry(draft).ok_or("Reopen the geometry editor")?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    context.setup.id.hash(&mut hash);
    (Arc::as_ptr(&context.scene) as usize).hash(&mut hash);
    for body in &context.setup.body_ids {
        body.0.hash(&mut hash);
    }
    for value in [
        context.setup.wcs.origin.x,
        context.setup.wcs.origin.y,
        context.setup.wcs.origin.z,
    ]
    .into_iter()
    .chain(context.setup.wcs.x_axis)
    .chain(context.setup.wcs.y_axis)
    .chain(context.setup.wcs.z_axis)
    {
        value.to_bits().hash(&mut hash);
    }
    let mut bytes = 0usize;
    let mut fields = 0usize;
    for field in &draft.fields {
        if field.path.starts_with(PREFIX) || field.path == holes::CURRENT {
            bytes = bytes
                .saturating_add(field.path.len())
                .saturating_add(field.text.len());
            fields += 1;
            if bytes > MAX_PICK_TEXT || fields > 32_768 {
                return Err("Hole form exceeds the viewport-picking text budget; use the existing geometry fields".into());
            }
            field.path.hash(&mut hash);
            field.text.hash(&mut hash);
        }
    }
    Ok(hash.finish())
}
pub(in super::super) fn unchanged(
    draft: &Draft,
    expected: &SelectionState,
) -> Result<bool, String> {
    Ok(draft.selection == expected.selection && epoch(draft)? == expected.epoch)
}
pub(in super::super) fn snapshot(draft: &Draft) -> Result<SelectionState, String> {
    let epoch = epoch(draft)?;
    let fields = raw_fields(draft);
    let keys = order(draft)?
        .iter()
        .enumerate()
        .map(|(index, row)| reference(draft, &fields, index, row))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    Ok(SelectionState {
        selection: draft.selection,
        keys,
        epoch,
    })
}

pub(in super::super) fn stage(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    expected: &SelectionState,
    key: FaceKey,
) -> Result<SelectionState, String> {
    if !unchanged(draft, expected)? {
        return Err("CAM hole draft changed; start picking again".into());
    }
    let context = operation_editor::geometry(draft).ok_or("Reopen the geometry editor")?;
    let reference_key = key.reference();
    if !context
        .holes
        .iter()
        .any(|(_, hole)| hole.face_key.as_deref() == Some(reference_key.as_str()))
    {
        return Err("Choose an available cylindrical face aligned with setup Z".into());
    }
    let mut rows = order(draft)?;
    let fields = raw_fields(draft);
    let mut retained = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        if reference(draft, &fields, index, row)? != Some(key) {
            retained.push(index);
        }
    }
    let append = retained.len() == rows.len();
    if append && rows.len() == MAX_PICK_ROWS {
        return Err(
            "Viewport picking supports at most 4096 hole rows; use the existing geometry fields"
                .into(),
        );
    }
    let original_order = serde_json::to_value(&rows).map_err(|error| error.to_string())?;
    let old_count = rows.len();
    if !append {
        rows = retained.iter().map(|index| rows[*index].clone()).collect();
    } else {
        rows.push(Row::Picked(key));
    }
    let next_order = serde_json::to_string(&rows).map_err(|error| error.to_string())?;
    let relevant = draft
        .fields
        .iter()
        .filter(|field| field.path.starts_with(PREFIX));
    let bytes = relevant.clone().fold(0usize, |n, field| {
        n.saturating_add(field.path.len())
            .saturating_add(field.text.len())
    });
    if bytes.saturating_add(next_order.len()).saturating_add(2048) > MAX_PICK_TEXT
        || relevant.count().saturating_add(8) > 32_768
    {
        return Err(
            "Hole form exceeds the viewport-picking text budget; use the existing geometry fields"
                .into(),
        );
    }
    let mut remap = vec![None; old_count];
    for (next, previous) in retained.into_iter().enumerate() {
        remap[previous] = Some(next);
    }
    draft.fields.retain_mut(|field| {
        let Some(suffix) = field.path.strip_prefix("/native/geometry/holes/") else {
            return true;
        };
        let Some((index, leaf)) = suffix.split_once('/') else {
            return false;
        };
        let Some(next) = index
            .parse::<usize>()
            .ok()
            .and_then(|i| remap.get(i))
            .copied()
            .flatten()
        else {
            return false;
        };
        field.path = format!("{}/{leaf}", holes::prefix(next));
        true
    });
    if !draft.fields.iter().any(|field| field.path == ORDER) {
        form::push(
            draft,
            ORDER,
            "Hole row identity",
            InputKind::Name,
            Value::String(
                serde_json::to_string(&original_order).map_err(|error| error.to_string())?,
            ),
            cam.units,
            None,
        );
    }
    form::set(draft, ORDER, &next_order);
    form::set(draft, holes::COUNT, &rows.len().to_string());
    form::set(
        draft,
        holes::CURRENT,
        &(if append { rows.len() } else { 1 }).max(1).to_string(),
    );
    operation_editor::changed(draft, cam, BUTTON)?;
    snapshot(draft)
}
