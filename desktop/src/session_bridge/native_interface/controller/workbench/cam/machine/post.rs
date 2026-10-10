use super::*;
const POST: &str = "/native/machine/post/";

pub(super) fn extend(draft: &mut Draft, record: &Value, units: CamUnits) {
    for (path, label, kind) in [
        (
            "program_number",
            "Program number (optional)",
            InputKind::OptionalInteger,
        ),
        (
            "machine_retract_z",
            "Machine retract Z for posting",
            InputKind::OptionalLength,
        ),
    ] {
        form::push(
            draft,
            &format!("{POST}{path}"),
            label,
            kind,
            record[path].clone(),
            units,
            None,
        );
    }
    let named_tools = record["dialect"]
        .as_str()
        .and_then(|dialect| serde_json::from_value::<PostDialect>(json!(dialect)).ok())
        .is_some_and(|dialect| dialect.supports_named_tools());
    let mut tool_calls = vec![
        ("automatic", "Controller default"),
        ("number", "Project tool number"),
    ];
    if named_tools || record["tool_call_mode"] == "name" {
        tool_calls.push(("name", "Project tool name"));
    }
    choice(
        draft,
        record,
        "tool_call_mode",
        "Controller tool calls",
        &tool_calls,
        units,
    );
    boolean(draft, record, "sequence_numbers", "Sequence numbers", units);
    if record["siemens_828d"].is_object() {
        choice(
            draft,
            record,
            "siemens_828d/atc_style",
            "Tool changer layout",
            &[
                ("double_arm", "Double arm"),
                ("umbrella", "Umbrella"),
                ("carousel_chain", "Carousel / chain"),
                ("other", "Other"),
            ],
            units,
        );
        choice(
            draft,
            record,
            "siemens_828d/tool_change_positioning",
            "Verified tool-change positioning",
            &[
                ("supa_z", "Program SUPA Z"),
                ("controller_managed", "Controller handles station"),
                ("supa_z_then_xy", "SUPA Z, then station XY"),
            ],
            units,
        );
        for (path, label, kind) in [
            (
                "supa_retract_z",
                "Verified SUPA retract Z",
                InputKind::Length,
            ),
            ("station_x", "Verified station X", InputKind::OptionalLength),
            ("station_y", "Verified station Y", InputKind::OptionalLength),
            (
                "tool_length_offset",
                "Tool edge / length offset D",
                InputKind::Integer,
            ),
            (
                "spindle_stop_subprogram",
                "Private spindle-stop subprogram (optional)",
                InputKind::Name,
            ),
        ] {
            form::push(
                draft,
                &format!("{POST}siemens_828d/{path}"),
                label,
                kind,
                record["siemens_828d"][path].clone(),
                units,
                None,
            );
        }
        boolean(
            draft,
            record,
            "siemens_828d/optional_stop_on_tool_change",
            "Optional stop at tool change",
            units,
        );
        boolean(
            draft,
            record,
            "siemens_828d/preload_next_tool",
            "Permit next-tool preload",
            units,
        );
    }
}
fn choice(
    draft: &mut Draft,
    record: &Value,
    path: &str,
    label: &str,
    options: &[(&str, &str)],
    units: CamUnits,
) {
    form::push(
        draft,
        &format!("{POST}{path}"),
        label,
        InputKind::Choice,
        record
            .pointer(&format!("/{path}"))
            .cloned()
            .unwrap_or(Value::Null),
        units,
        Some(form::options(options)),
    );
}
fn boolean(draft: &mut Draft, record: &Value, path: &str, label: &str, units: CamUnits) {
    form::push(
        draft,
        &format!("{POST}{path}"),
        label,
        InputKind::Boolean,
        record
            .pointer(&format!("/{path}"))
            .cloned()
            .unwrap_or(json!(false)),
        units,
        Some(form::options(&[("true", "Yes"), ("false", "No")])),
    );
}
pub(super) fn visible(draft: &Draft, path: &str) -> bool {
    if path == format!("{POST}machine_retract_z") {
        return draft
            .machine
            .as_ref()
            .and_then(|context| context.selected.as_ref())
            .is_some_and(|machine| machine.profile.post.dialect.requires_machine_retract());
    }
    if path == format!("{POST}siemens_828d/station_x")
        || path == format!("{POST}siemens_828d/station_y")
    {
        return form::text(
            draft,
            &format!("{POST}siemens_828d/tool_change_positioning"),
        )
        .is_ok_and(|value| value == "supa_z_then_xy");
    }
    true
}
pub(super) fn apply(draft: &Draft, record: &mut Value, units: CamUnits) -> Result<(), String> {
    for field in draft
        .fields
        .iter()
        .filter(|field| field.path.starts_with(POST) && field.text != field.original)
    {
        let path = format!("/{}", field.path.strip_prefix(POST).unwrap());
        let text = field.text.trim();
        let value = match field.kind {
            InputKind::OptionalInteger | InputKind::OptionalLength if text.is_empty() => {
                Value::Null
            }
            InputKind::Name if text.is_empty() => Value::Null,
            InputKind::Name | InputKind::Choice => json!(text),
            InputKind::Boolean => json!(text
                .parse::<bool>()
                .map_err(|_| format!("Choose {}", field.label))?),
            InputKind::Integer | InputKind::OptionalInteger => json!(text
                .parse::<u32>()
                .map_err(|_| format!("Enter a whole number for {}", field.label))?),
            _ => json!(form::number(draft, &field.path, units)?),
        };
        *record
            .pointer_mut(&path)
            .ok_or("Post profile field is unavailable")? = value;
    }
    Ok(())
}
