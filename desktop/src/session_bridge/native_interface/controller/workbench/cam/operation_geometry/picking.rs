//! Typed adapter into the existing form draft. No pick writes a CAM record.
use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(in super::super) struct SelectionState {
    pub selection: Selection,
    pub chain: usize,
    pub source: ChainSource,
    pub mode: ChainMode,
    pub keys: Vec<String>,
    pub reversed: bool,
    pub pocket: bool,
    pub claimed: Vec<String>,
    pub source_epoch: u64,
}
pub(super) fn button_path(index: usize) -> String {
    format!("/native/ui/geometry_pick_{index}")
}
pub(in super::super) fn is_button(path: &str) -> bool {
    path.starts_with("/native/ui/geometry_pick_")
}
fn resolved_path(index: usize) -> String {
    format!("/native/ui/geometry_resolved_{index}")
}
#[derive(serde::Serialize, serde::Deserialize)]
struct SourceDraft {
    keys: std::collections::BTreeMap<usize, String>,
    count: String,
    mode: String,
    reversed: String,
    resolved: String,
    cursor: String,
}
impl Default for SourceDraft {
    fn default() -> Self {
        Self {
            keys: Default::default(),
            count: "0".into(),
            mode: "manual".into(),
            reversed: "false".into(),
            resolved: String::new(),
            cursor: "1".into(),
        }
    }
}
fn source_path(index: usize, field: &str) -> String {
    format!("/native/ui/geometry_source_{index}/{field}")
}
fn raw(draft: &Draft, path: &str) -> String {
    draft
        .fields
        .iter()
        .find(|field| field.path == path)
        .map(|field| field.text.clone())
        .unwrap_or_default()
}
/// Only the visible edge field needs the candidate catalog. Text and original
/// values remain on every key slot, including slots in inactive chains.
pub(super) fn retain_key_options(draft: &mut Draft, active_path: Option<&str>) {
    for field in &mut draft.fields {
        if field.path.starts_with("/native/geometry/chains/")
            && field.path.contains("/keys/")
            && active_path != Some(field.path.as_str())
        {
            field.options = None;
        }
    }
}
fn ui_value(draft: &mut Draft, path: &str, value: &str, units: CamUnits) {
    if !draft.fields.iter().any(|field| field.path == path) {
        form::push(
            draft,
            path,
            "Geometry source draft",
            InputKind::Name,
            json!(""),
            units,
            None,
        );
    }
    form::set(draft, path, value);
}
pub(super) fn initialize(draft: &mut Draft, index: usize, units: CamUnits) {
    let path = source_path(index, "current");
    if !draft.fields.iter().any(|field| field.path == path) {
        let source = chains::source(draft, &chains::prefix(index));
        ui_value(draft, &path, &source, units);
        ui_value(draft, &source_path(index, "epoch"), "0", units);
    }
}
fn source_epoch(draft: &Draft, index: usize) -> Result<u64, String> {
    form::text(draft, &source_path(index, "epoch"))?
        .parse()
        .map_err(|_| "Geometry source draft changed; reopen the editor".into())
}
fn capture_source(draft: &Draft, index: usize, source: &str) -> SourceDraft {
    let prefix = chains::prefix(index);
    let stored = chains::chain(&draft.record, index).unwrap_or_default();
    let mut keys = std::collections::BTreeMap::new();
    if stored["chain_ref"]["source"].as_str() == Some(source) {
        keys.extend(chains::keys(&stored).into_iter().enumerate());
    }
    let key_prefix = format!("{prefix}/keys/");
    for field in &draft.fields {
        if let Some(index) = field
            .path
            .strip_prefix(&key_prefix)
            .and_then(|value| value.parse::<usize>().ok())
        {
            keys.insert(index, field.text.clone());
        }
    }
    SourceDraft {
        keys,
        count: raw(draft, &format!("{prefix}/key_count")),
        mode: raw(draft, &format!("{prefix}/mode")),
        reversed: raw(draft, &format!("{prefix}/reversed")),
        resolved: raw(draft, &resolved_path(index)),
        cursor: raw(draft, &chains::key_cursor(&prefix)),
    }
}
fn switch_source(draft: &mut Draft, cam: &CamDocumentDto, index: usize) -> Result<(), String> {
    let prefix = chains::prefix(index);
    let next = chains::source(draft, &prefix);
    if !matches!(next.as_str(), "model" | "sketch" | "manual") {
        return Err("Choose a geometry source".into());
    }
    let current_path = source_path(index, "current");
    let previous = form::text(draft, &current_path)?.to_owned();
    if previous == next {
        return Ok(());
    }
    let epoch = source_epoch(draft, index)?
        .checked_add(1)
        .ok_or("Geometry source draft is exhausted; reopen the editor")?;
    let cached = raw(draft, &source_path(index, &next));
    let restore: SourceDraft = if cached.is_empty() {
        SourceDraft::default()
    } else {
        serde_json::from_str(&cached)
            .map_err(|_| "Geometry source draft changed; reopen the editor")?
    };
    let save = serde_json::to_string(&capture_source(draft, index, &previous))
        .map_err(|error| error.to_string())?;
    let original = chains::chain(&draft.record, index)
        .map(|record| chains::keys(&record))
        .unwrap_or_default();
    let key_prefix = format!("{prefix}/keys/");
    for field in &mut draft.fields {
        if field.path.starts_with(&key_prefix) {
            field.text.clear();
            field.options = None;
        }
    }
    let mut indices = draft
        .fields
        .iter()
        .enumerate()
        .map(|(i, field)| (field.path.clone(), i))
        .collect::<HashMap<_, _>>();
    for (slot, value) in restore.keys {
        let path = format!("{key_prefix}{slot}");
        let field_index = if let Some(index) = indices.get(&path) {
            *index
        } else {
            let field_index = draft.fields.len();
            form::push(
                draft,
                &path,
                &format!("Edge {}", slot + 1),
                InputKind::Choice,
                json!(original.get(slot).cloned().unwrap_or_default()),
                cam.units,
                None,
            );
            indices.insert(path, field_index);
            field_index
        };
        draft.fields[field_index].text = value;
    }
    for (path, value) in [
        (format!("{prefix}/key_count"), restore.count),
        (format!("{prefix}/mode"), restore.mode),
        (format!("{prefix}/reversed"), restore.reversed),
        (chains::key_cursor(&prefix), restore.cursor),
    ] {
        form::set(draft, &path, &value);
    }
    ui_value(draft, &resolved_path(index), &restore.resolved, cam.units);
    ui_value(draft, &source_path(index, &previous), &save, cam.units);
    ui_value(draft, &current_path, &next, cam.units);
    ui_value(
        draft,
        &source_path(index, "epoch"),
        &epoch.to_string(),
        cam.units,
    );
    Ok(())
}
pub(super) fn changed(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    index: usize,
    path: &str,
) -> Result<(), String> {
    let prefix = chains::prefix(index);
    if path == format!("{prefix}/source") {
        return switch_source(draft, cam, index);
    }
    if path == format!("{prefix}/mode")
        || path == format!("{prefix}/key_count")
        || path.starts_with(&format!("{prefix}/keys/"))
    {
        form::set(draft, &resolved_path(index), "");
    }
    Ok(())
}
pub(super) fn resolved(draft: &Draft, index: usize, keys: &[String]) -> bool {
    form::text(draft, &resolved_path(index))
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<String>>(text).ok())
        .is_some_and(|saved| saved == keys)
}
fn draft_keys(draft: &Draft, index: usize) -> Result<Vec<String>, String> {
    let prefix = chains::prefix(index);
    let saved = chains::chain(&draft.record, index)
        .map(|record| chains::keys(&record))
        .unwrap_or_default();
    let n = count(draft, &format!("{prefix}/key_count"), 20_000).unwrap_or(saved.len());
    let fields = draft
        .fields
        .iter()
        .map(|field| (field.path.as_str(), field.text.trim()))
        .collect::<HashMap<_, _>>();
    (0..n)
        .map(|i| {
            fields
                .get(format!("{prefix}/keys/{i}").as_str())
                .map(|text| (*text).to_owned())
                .or_else(|| saved.get(i).cloned())
                .ok_or_else(|| "Choose every selected edge".into())
        })
        .filter(|key| !key.as_ref().is_ok_and(String::is_empty))
        .collect()
}
pub(in super::super) fn snapshot(draft: &Draft) -> Result<SelectionState, String> {
    if form::text(draft, "/native/ui/operation_section")? != "geometry"
        || !matches!(
            draft.record["kind"].as_str(),
            Some("contour2d" | "chamfer2d" | "pocket2d")
        )
    {
        return Err("Open contour, chamfer or pocket geometry".into());
    }
    let chain = chains::active(draft).ok_or("Choose an available chain")?;
    let prefix = chains::prefix(chain);
    let source = match chains::source(draft, &prefix).as_str() {
        "model" => ChainSource::Model,
        "sketch" => ChainSource::Sketch,
        _ => return Err("Choose model edges or sketch curves before picking".into()),
    };
    let mut claimed = Vec::new();
    for other in 0..count(draft, chains::COUNT, 64)? {
        if other == chain {
            continue;
        }
        let stored = chains::chain(&draft.record, other).unwrap_or_default();
        let other_source = form::text(draft, &format!("{}/source", chains::prefix(other)))
            .ok()
            .or_else(|| stored["chain_ref"]["source"].as_str());
        if other_source
            == Some(if source == ChainSource::Model {
                "model"
            } else {
                "sketch"
            })
        {
            claimed.extend(draft_keys(draft, other)?);
        }
    }
    Ok(SelectionState {
        selection: draft.selection,
        chain,
        source,
        mode: if form::text(draft, &format!("{prefix}/mode"))? == "closed" {
            ChainMode::Closed
        } else {
            ChainMode::Manual
        },
        keys: draft_keys(draft, chain)?,
        reversed: form::text(draft, &format!("{prefix}/reversed"))? == "true",
        pocket: draft.record["kind"] == "pocket2d",
        claimed,
        source_epoch: source_epoch(draft, chain)?,
    })
}
/// Preserve original form baselines and every untouched chain/operation field.
/// Closed-loop results retain their canonical keys, not an arbitrary seed.
pub(in super::super) fn stage(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    expected: &SelectionState,
    keys: Vec<String>,
    individual: bool,
) -> Result<SelectionState, String> {
    if snapshot(draft)? != *expected {
        return Err("CAM geometry changed; start picking again".into());
    }
    if keys.len() > 20_000 {
        return Err("A chain supports at most 20000 edges".into());
    }
    let claimed = expected
        .claimed
        .iter()
        .collect::<std::collections::HashSet<_>>();
    if keys.iter().any(|key| claimed.contains(key)) {
        return Err("That edge belongs to another chain. Select that chain to edit it.".into());
    }
    let context = operation_editor::geometry(draft).ok_or("Reopen the geometry editor")?;
    let options = if expected.source == ChainSource::Model {
        &context.model_options
    } else {
        &context.sketch_options
    }
    .clone();
    let allowed = options
        .iter()
        .filter(|option| !option.disabled)
        .map(|option| option.value.as_str())
        .collect::<std::collections::HashSet<_>>();
    if keys.iter().any(|key| !allowed.contains(key.as_str())) {
        return Err("Selected geometry is no longer available".into());
    }
    let prefix = chains::prefix(expected.chain);
    let original = chains::chain(&draft.record, expected.chain)
        .map(|record| chains::keys(&record))
        .unwrap_or_default();
    let mut indices = draft
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| (field.path.clone(), index))
        .collect::<HashMap<_, _>>();
    for (index, key) in keys.iter().enumerate() {
        let path = format!("{prefix}/keys/{index}");
        let field_index = if let Some(index) = indices.get(&path) {
            *index
        } else {
            let field_index = draft.fields.len();
            form::push(
                draft,
                &path,
                &format!("Edge {}", index + 1),
                InputKind::Choice,
                json!(original.get(index).cloned().unwrap_or_default()),
                cam.units,
                None,
            );
            indices.insert(path, field_index);
            field_index
        };
        draft.fields[field_index].text.clone_from(key);
    }
    form::set(
        draft,
        &format!("{prefix}/key_count"),
        &keys.len().to_string(),
    );
    form::set(
        draft,
        &chains::key_cursor(&prefix),
        &keys.len().max(1).to_string(),
    );
    if individual {
        form::set(draft, &format!("{prefix}/mode"), "manual");
    }
    let path = resolved_path(expected.chain);
    if !draft.fields.iter().any(|field| field.path == path) {
        form::push(
            draft,
            &path,
            "Resolved viewport selection",
            InputKind::Name,
            json!(""),
            cam.units,
            None,
        );
    }
    form::set(
        draft,
        &path,
        &serde_json::to_string(&keys).map_err(|e| e.to_string())?,
    );
    let active_path = keys
        .len()
        .checked_sub(1)
        .map(|index| format!("{prefix}/keys/{index}"));
    retain_key_options(draft, active_path.as_deref());
    if let Some(path) = active_path {
        draft
            .fields
            .iter_mut()
            .find(|field| field.path == path)
            .unwrap()
            .options = Some(options);
    }
    snapshot(draft)
}
