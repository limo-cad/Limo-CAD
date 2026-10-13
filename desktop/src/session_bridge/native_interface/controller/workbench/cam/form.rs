//! Transient values for native CAM forms; persisted values remain CAM DTOs.
use super::*;
use limo_cad_cam::CamUnits;

pub(super) fn options(values: &[(&str, &str)]) -> Vec<ChoiceOption> {
    values
        .iter()
        .map(|(value, label)| ChoiceOption {
            value: (*value).into(),
            label: (*label).into(),
            disabled: false,
        })
        .collect()
}
pub(super) fn push(
    draft: &mut Draft,
    path: &str,
    label: &str,
    kind: InputKind,
    value: Value,
    units: CamUnits,
    options: Option<Vec<ChoiceOption>>,
) {
    let text = if value.is_null() {
        String::new()
    } else if let Some(value) = value.as_str() {
        value.into()
    } else if matches!(
        kind,
        InputKind::Length | InputKind::OptionalLength | InputKind::Feed | InputKind::OptionalFeed
    ) {
        value
            .as_f64()
            .map(|v| units.from_mm(v).to_string())
            .unwrap_or_default()
    } else {
        value.to_string()
    };
    let label = match kind {
        InputKind::Length | InputKind::OptionalLength => {
            format!("{label} ({})", units.length_label())
        }
        InputKind::Feed | InputKind::OptionalFeed => format!("{label} ({})", units.feed_label()),
        _ => label.into(),
    };
    draft.fields.push(DraftField {
        path: path.into(),
        label,
        kind,
        original: text.clone(),
        text,
        options,
    });
}
pub(super) fn text<'a>(draft: &'a Draft, path: &str) -> Result<&'a str, String> {
    draft
        .fields
        .iter()
        .find(|f| f.path == path)
        .map(|f| f.text.trim())
        .ok_or_else(|| format!("CAM field {path} is unavailable"))
}
pub(super) fn number(draft: &Draft, path: &str, units: CamUnits) -> Result<f64, String> {
    let field = draft
        .fields
        .iter()
        .find(|f| f.path == path)
        .ok_or("CAM field is unavailable")?;
    let n = text(draft, path)?
        .parse::<f64>()
        .map_err(|_| format!("Enter {}", field.label))?;
    let n = if matches!(
        field.kind,
        InputKind::Length | InputKind::OptionalLength | InputKind::Feed | InputKind::OptionalFeed
    ) {
        units.to_mm(n)
    } else {
        n
    };
    if !n.is_finite() {
        return Err(format!("{} must be finite", field.label));
    }
    Ok(n)
}
pub(super) fn changed(draft: &Draft, prefix: &str) -> bool {
    draft
        .fields
        .iter()
        .any(|f| f.path.starts_with(prefix) && f.text != f.original)
}
pub(super) fn set(draft: &mut Draft, path: &str, text: &str) {
    if let Some(field) = draft.fields.iter_mut().find(|f| f.path == path) {
        field.text = text.into();
    }
}
