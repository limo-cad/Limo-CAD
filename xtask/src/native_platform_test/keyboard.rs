//! Refocus an observed owned window before a key, without retrying input.
use anyhow::{ensure, Result};
use serde_json::{json, Value};

pub(super) fn focus_target(
    mut observe: impl FnMut() -> Result<Value>,
    mut send: impl FnMut(&Value, Value) -> Result<Value>,
) -> Result<(Value, Value)> {
    let observed = observe()?;
    activate_target(&observed, &mut observe, &mut send)
}

fn activate_target(
    observed: &Value,
    observe: &mut impl FnMut() -> Result<Value>,
    send: &mut impl FnMut(&Value, Value) -> Result<Value>,
) -> Result<(Value, Value)> {
    let receipt = send(observed, json!({"action":"focus"}))?;
    ensure!(
        receipt["status"] == "focused" && receipt["owner"] == observed["owner"],
        "Owned CAD activation did not complete; no key was sent: {receipt}"
    );
    let activated = observe()?;
    require_same_target(observed, &activated)?;
    ensure!(
        activated["foreground"] == true,
        "CAD lost foreground after activation; no key was sent"
    );
    Ok((activated, receipt))
}

pub(super) fn send_key(
    key: &str,
    intended: &Value,
    mut observe: impl FnMut() -> Result<Value>,
    mut send: impl FnMut(&Value, Value) -> Result<Value>,
) -> Result<Value> {
    let mut observed = observe()?;
    require_same_target(intended, &observed)?;
    let foreground = observed["foreground"]
        .as_bool()
        .ok_or_else(|| anyhow::anyhow!("Owned observation omitted foreground state"))?;
    if !foreground {
        // Nothing has been typed. Activation consumes its own observation;
        // the key must use a new observation of the same document and window.
        observed = activate_target(&observed, &mut observe, &mut send)?.0;
    }
    require_text_target(intended, &observed)?;
    // Any denial or partial receipt remains fatal. Repeating a key could edit
    // twice or act on a different control, even if the next observation looks OK.
    let receipt = send(&observed, json!({"action":"key","key":key}))?;
    ensure!(
        receipt["status"] == "input_sent",
        "Key did not complete; do not retry blindly: {receipt}"
    );
    Ok(receipt)
}

/// Pin the actual focused Rename field once, before this scenario's first key.
/// Text, selection and snapshot IDs may change; widget binding and owner may not.
pub(super) fn text_target(observed: &Value) -> Result<Value> {
    let ui = &observed["inspection"]["ui"];
    let binding = &ui["focused_binding"];
    ensure!(
        observed["editable_focus"] == true
            && ui["focused_control"].is_string()
            && binding["control_key"].is_u64()
            && binding["binding"].is_u64()
            && binding["context"].is_object()
            && binding["context"]["epoch"].is_u64()
            && binding["context"]["document_id"] == observed["owner"]["document_id"]
            && binding["context"]["window_id"] == observed["owner"]["window_id"],
        "The intended editable CAD field has no current retained binding; no key was sent"
    );
    let mut focused = ui["surfaces"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|surface| surface["controls"].as_array().into_iter().flatten())
        .filter(|control| control["id"] == ui["focused_control"]);
    let field = focused.next().ok_or_else(|| {
        anyhow::anyhow!("The intended editable CAD field is absent; no key was sent")
    })?;
    ensure!(
        focused.next().is_none()
            && field["label"] == "Project name"
            && matches!(
                field["role"].as_str(),
                Some("textbox" | "multiline_textbox")
            )
            && field["disabled"] == false
            && field["read_only"] == false,
        "Focus is not the intended editable Project name field; no key was sent"
    );
    Ok(
        json!({"owner":observed["owner"],"window_handle":observed["window_handle"],
        "target_kind":observed["target_kind"],"binding":binding}),
    )
}

fn require_text_target(intended: &Value, observed: &Value) -> Result<()> {
    require_same_target(intended, observed)?;
    let current = text_target(observed)?;
    ensure!(
        current["binding"] == intended["binding"],
        "The intended editable CAD field changed during keyboard preparation; no key was sent"
    );
    Ok(())
}

pub(super) fn require_same_target(expected: &Value, actual: &Value) -> Result<()> {
    ensure!(
        actual["owner"] == expected["owner"]
            && actual["window_handle"] == expected["window_handle"]
            && actual["target_kind"] == expected["target_kind"],
        "CAD document or window changed during foreground preparation; no key was sent"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::VecDeque};

    fn observation(foreground: bool, token: &str) -> Value {
        json!({"owner":{"pid":91,"process_instance_id":"owned-process",
            "session_id":"owned-session","document_id":"owned-document","window_id":"main","generation":7},
            "window_handle":32,"target_kind":"bevy_window","foreground":foreground,
            "editable_focus":true,
            "inspection":{"ui":{"focused_control":format!("field-{token}"),
                "focused_binding":{"control_key":17,"binding":2,
                    "context":{"document_id":"owned-document","window_id":"main","epoch":1}},
                "surfaces":[{"controls":[{"id":format!("field-{token}"),"label":"Project name",
                    "role":"textbox","disabled":false,"read_only":false,"value":"Untitled"}]}]}},
            "observation":token})
    }

    fn exercise(
        observations: Vec<Value>,
        receipts: Vec<Result<Value>>,
    ) -> (Result<Value>, Vec<Value>) {
        let mut observations = VecDeque::from(observations);
        let mut receipts = VecDeque::from(receipts);
        let calls = RefCell::new(Vec::new());
        let result = send_key(
            "Ctrl+V",
            &text_target(&observation(true, "pinned")).unwrap(),
            || {
                calls.borrow_mut().push(json!({"action":"observe"}));
                Ok(observations
                    .pop_front()
                    .expect("Unexpected extra observation"))
            },
            |observed, mut request| {
                request["observation"] = observed["observation"].clone();
                calls.borrow_mut().push(request);
                receipts.pop_front().expect("Unexpected input retry")
            },
        );
        (result, calls.into_inner())
    }

    fn focused() -> Result<Value> {
        Ok(json!({"status":"focused","owner":observation(false, "old")["owner"]}))
    }

    #[test]
    fn explicit_field_preparation_reobserves_the_same_target_without_sending_keys() {
        let mut observations =
            VecDeque::from([observation(false, "old"), observation(true, "prepared")]);
        let calls = RefCell::new(Vec::new());
        let (prepared, receipt) = focus_target(
            || {
                calls.borrow_mut().push("observe");
                Ok(observations.pop_front().expect("Unexpected observation"))
            },
            |observed, request| {
                assert_eq!(observed["observation"], "old");
                assert_eq!(request, json!({"action":"focus"}));
                calls.borrow_mut().push("focus");
                focused()
            },
        )
        .unwrap();
        assert_eq!(prepared["observation"], "prepared");
        assert_eq!(receipt["status"], "focused");
        assert_eq!(calls.into_inner(), ["observe", "focus", "observe"]);
    }

    #[test]
    fn explicit_field_preparation_rejects_denial_and_changed_owner_or_foreground() {
        for change in ["denied", "document", "foreground"] {
            let mut prepared = observation(true, "prepared");
            match change {
                "document" => prepared["owner"]["document_id"] = json!("other-document"),
                "foreground" => prepared["foreground"] = json!(false),
                _ => (),
            }
            let mut observations = VecDeque::from([observation(false, "old"), prepared]);
            let mut activations = 0;
            let result = focus_target(
                || Ok(observations.pop_front().expect("Unexpected observation")),
                |_, request| {
                    assert_eq!(request["action"], "focus");
                    activations += 1;
                    if change == "denied" {
                        Ok(json!({"status":"failed"}))
                    } else {
                        focused()
                    }
                },
            );
            assert!(result.is_err(), "{change}");
            assert_eq!(activations, 1, "{change}");
        }
    }

    #[test]
    fn already_foreground_sends_the_key_once_without_activation() {
        let (result, calls) = exercise(
            vec![observation(true, "ready")],
            vec![Ok(json!({"status":"input_sent"}))],
        );
        assert!(result.is_ok());
        assert_eq!(
            calls,
            vec![
                json!({"action":"observe"}),
                json!({"action":"key","key":"Ctrl+V","observation":"ready"})
            ]
        );
    }

    #[test]
    fn lost_foreground_requires_activation_and_a_new_observation_before_one_key() {
        let (result, calls) = exercise(
            vec![observation(false, "old"), observation(true, "new")],
            vec![focused(), Ok(json!({"status":"input_sent"}))],
        );
        assert!(result.is_ok());
        assert_eq!(
            calls,
            vec![
                json!({"action":"observe"}),
                json!({"action":"focus","observation":"old"}),
                json!({"action":"observe"}),
                json!({"action":"key","key":"Ctrl+V","observation":"new"})
            ]
        );
    }

    #[test]
    fn denied_activation_never_sends_or_retries_a_key() {
        for receipt in [
            Err(anyhow::anyhow!("Windows denied activation")),
            Ok(json!({"status":"failed"})),
            Ok(json!({"status":"focused","owner":{"pid":92}})),
        ] {
            let (result, calls) = exercise(vec![observation(false, "old")], vec![receipt]);
            assert!(result.is_err());
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[1]["action"], "focus");
        }
    }

    #[test]
    fn replaced_owner_document_or_window_after_activation_never_receives_a_key() {
        for (pointer, replacement) in [
            ("/owner/pid", json!(92)),
            ("/owner/process_instance_id", json!("replaced")),
            ("/owner/session_id", json!("replaced")),
            ("/owner/document_id", json!("another-tab")),
            ("/owner/generation", json!(8)),
            ("/window_handle", json!(33)),
            ("/target_kind", json!("native_dialog")),
            ("/foreground", json!(false)),
        ] {
            let mut activated = observation(true, "new");
            *activated.pointer_mut(pointer).unwrap() = replacement;
            let (result, calls) =
                exercise(vec![observation(false, "old"), activated], vec![focused()]);
            assert!(result.is_err(), "{pointer}");
            assert_eq!(calls.len(), 3, "{pointer}");
            assert_eq!(calls[2]["action"], "observe");
        }
    }

    #[test]
    fn failed_or_partial_key_is_never_repeated() {
        for receipt in [
            Err(anyhow::anyhow!("Foreground changed before input")),
            Ok(json!({"status":"partial_input","completed":1})),
            Ok(json!({"status":"failed"})),
        ] {
            let (result, calls) = exercise(vec![observation(true, "ready")], vec![receipt]);
            assert!(result.is_err());
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[1]["action"], "key");
        }
    }

    #[test]
    fn missing_foreground_state_never_sends_anything() {
        let mut observed = observation(true, "ready");
        observed.as_object_mut().unwrap().remove("foreground");
        let (result, calls) = exercise(vec![observed], vec![]);
        assert!(result.is_err());
        assert_eq!(calls, vec![json!({"action":"observe"})]);
    }

    #[test]
    fn missing_replaced_or_uneditable_field_never_receives_a_key() {
        for (pointer, replacement) in [
            ("/editable_focus", json!(false)),
            ("/inspection/ui/focused_control", Value::Null),
            ("/inspection/ui/focused_binding", Value::Null),
            ("/inspection/ui/focused_binding/context/epoch", Value::Null),
            ("/inspection/ui/focused_binding/control_key", json!(18)),
            ("/inspection/ui/focused_binding/binding", json!(3)),
            ("/inspection/ui/focused_binding/context/epoch", json!(2)),
            ("/inspection/ui/surfaces/0/controls", json!([])),
            (
                "/inspection/ui/surfaces/0/controls/0/id",
                json!("different-field"),
            ),
            (
                "/inspection/ui/surfaces/0/controls/0/label",
                json!("Other name"),
            ),
            ("/inspection/ui/surfaces/0/controls/0/role", json!("button")),
            ("/inspection/ui/surfaces/0/controls/0/disabled", json!(true)),
            (
                "/inspection/ui/surfaces/0/controls/0/read_only",
                json!(true),
            ),
        ] {
            for activate in [false, true] {
                let mut observed = observation(true, "after");
                *observed.pointer_mut(pointer).unwrap() = replacement.clone();
                let (observations, receipts) = if activate {
                    (
                        vec![observation(false, "before"), observed],
                        vec![focused()],
                    )
                } else {
                    (vec![observed], vec![])
                };
                let (result, calls) = exercise(observations, receipts);
                assert!(result.is_err(), "{pointer}, activation={activate}");
                assert!(
                    calls.iter().all(|call| call["action"] != "key"),
                    "{pointer}, activation={activate}: {calls:?}"
                );
            }
        }
    }

    #[test]
    fn inspection_ids_text_and_selection_can_change_on_the_intended_binding() {
        let mut observed = observation(true, "fresh-inspection");
        observed["inspection"]["ui"]["surfaces"][0]["controls"][0]["value"] = json!("Changed");
        observed["inspection"]["ui"]["surfaces"][0]["controls"][0]["selection"] =
            json!({"start":2,"end":4});
        let (result, calls) = exercise(vec![observed], vec![Ok(json!({"status":"input_sent"}))]);
        assert!(result.is_ok());
        assert_eq!(
            calls.iter().filter(|call| call["action"] == "key").count(),
            1
        );
    }
}
