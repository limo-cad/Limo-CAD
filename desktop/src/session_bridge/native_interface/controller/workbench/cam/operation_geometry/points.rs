use super::*;

pub(in super::super) fn cursor(prefix: &str) -> String {
    format!(
        "/native/ui/geometry_row{}",
        prefix.strip_prefix(PREFIX).unwrap_or(prefix)
    )
}
pub(in super::super) fn selected(draft: &Draft, prefix: &str) -> Option<usize> {
    let index = form::text(draft, &cursor(prefix))
        .ok()?
        .parse::<usize>()
        .ok()?
        .checked_sub(1)?;
    (index < count(draft, &format!("{prefix}/count"), 250_000).ok()?).then_some(index)
}
pub(in super::super) fn extend(
    draft: &mut Draft,
    prefix: &str,
    label: &str,
    original: &[Value],
    units: CamUnits,
) {
    let count_path = format!("{prefix}/count");
    if !draft.fields.iter().any(|field| field.path == count_path) {
        form::push(
            draft,
            &count_path,
            &format!("{label} count"),
            InputKind::Integer,
            json!(original.len()),
            units,
            None,
        );
        form::push(
            draft,
            &cursor(prefix),
            &format!("{label} number"),
            InputKind::Integer,
            json!(1),
            units,
            None,
        );
    }
    let Some(index) = selected(draft, prefix) else {
        return;
    };
    for axis in ["x", "y"] {
        let path = format!("{prefix}/{index}/{axis}");
        if draft.fields.iter().any(|field| field.path == path) {
            continue;
        }
        form::push(
            draft,
            &path,
            &format!("{label} {} {}", index + 1, axis.to_uppercase()),
            InputKind::Length,
            original
                .get(index)
                .map_or(Value::Null, |value| value[axis].clone()),
            units,
            None,
        );
    }
}
pub(in super::super) fn changed(
    draft: &mut Draft,
    prefix: &str,
    label: &str,
    path: &str,
    original: &[Value],
    units: CamUnits,
) -> Result<(), String> {
    let count = count(draft, &format!("{prefix}/count"), 250_000)?;
    if path == format!("{prefix}/count") {
        let previous = form::text(draft, &cursor(prefix))
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1);
        let next = if count > original.len() {
            count
        } else {
            previous.min(count.max(1))
        };
        form::set(draft, &cursor(prefix), &next.max(1).to_string());
    }
    if count > 0 && selected(draft, prefix).is_none() {
        return Err(format!("Choose a {label} number from 1 to {count}"));
    }
    extend(draft, prefix, label, original, units);
    Ok(())
}
pub(in super::super) fn visible(draft: &Draft, prefix: &str, path: &str) -> bool {
    if path == format!("{prefix}/count") {
        return true;
    }
    let index = selected(draft, prefix);
    if path == cursor(prefix) {
        return count(draft, &format!("{prefix}/count"), 250_000).unwrap_or(0) > 0;
    }
    index.is_some()
        && path
            .strip_prefix(&format!("{prefix}/"))
            .and_then(|path| path.split_once('/'))
            .and_then(|(index, _)| index.parse::<usize>().ok())
            == index
}
pub(in super::super) fn read(
    draft: &Draft,
    prefix: &str,
    original: &[Value],
    units: CamUnits,
) -> Result<Vec<Point2Dto>, String> {
    let count = count(draft, &format!("{prefix}/count"), 250_000)?;
    let fields: HashMap<_, _> = draft
        .fields
        .iter()
        .map(|field| (field.path.as_str(), field))
        .collect();
    (0..count)
        .map(|index| {
            let axis = |axis| -> Result<f64, String> {
                let path = format!("{prefix}/{index}/{axis}");
                let original = original.get(index).and_then(|value| value[axis].as_f64());
                if fields
                    .get(path.as_str())
                    .is_none_or(|field| field.original == field.text)
                {
                    if let Some(original) = original {
                        return Ok(original);
                    }
                }
                number(draft, &path, original, units)
            };
            Ok(Point2Dto::new(axis("x")?, axis("y")?))
        })
        .collect()
}
