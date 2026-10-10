use super::*;
use limo_cad_core::{BodyId, PrintIntentTargetDto, PrintModifierDto};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteRequest {
    modifier: PrintModifierDto,
    expected_model_json: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityRequest {
    id: String,
    expected_model_json: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyRequest {
    source_id: String,
    target_body_id: BodyId,
    #[serde(default)]
    name: Option<String>,
    expected_model_json: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectiveRequest {
    #[serde(default)]
    body_ids: Vec<BodyId>,
    #[serde(default)]
    target: PrintIntentTargetDto,
}

pub(super) fn handle_read_only(
    manager: &SketchManager,
    method: &str,
    payload: &str,
) -> Option<String> {
    if payload.len() > 32 * 1024 * 1024 {
        return Some(err_json(
            "Print modifier payload exceeds 32 MiB, including its project snapshot",
        ));
    }
    Some(match method {
        "print_modifier_effective" => with_payload(payload, |request: EffectiveRequest| {
            manager.effective_print_intent(request.body_ids, Some(request.target))
        }),
        _ => return None,
    })
}

pub(super) fn handle(manager: &mut SketchManager, method: &str, payload: &str) -> String {
    if let Some(response) = handle_read_only(manager, method, payload) {
        return response;
    }
    if payload.len() > 32 * 1024 * 1024 {
        return err_json("Print modifier payload exceeds 32 MiB, including its project snapshot");
    }
    match method {
        "print_modifier_create" => with_payload(payload, |request: WriteRequest| {
            print_intent::guard(manager, &request.expected_model_json)?;
            manager.create_print_modifier(request.modifier)
        }),
        "print_modifier_update" => with_payload(payload, |request: WriteRequest| {
            print_intent::guard(manager, &request.expected_model_json)?;
            manager.update_print_modifier(request.modifier)
        }),
        "print_modifier_remove" => with_payload(payload, |request: IdentityRequest| {
            print_intent::guard(manager, &request.expected_model_json)?;
            manager.remove_print_modifier(&request.id)
        }),
        "print_modifier_reset" => with_payload(payload, |request: IdentityRequest| {
            print_intent::guard(manager, &request.expected_model_json)?;
            manager.reset_print_modifier(&request.id)
        }),
        "print_modifier_copy" => with_payload(payload, |request: CopyRequest| {
            print_intent::guard(manager, &request.expected_model_json)?;
            manager.copy_print_modifier(&request.source_id, request.target_body_id, request.name)
        }),
        _ => err_json(format!("unknown engine method: {method}")),
    }
}
