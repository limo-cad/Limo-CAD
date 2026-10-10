use super::*;
use limo_cad_core::{
    BodyId, PrintIntentDocumentDto, PrintIntentPresetDto, PrintIntentTargetDto, PrintSettingsDto,
    PrintTargetHandoffDto,
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectiveRequest {
    #[serde(default)]
    body_ids: Vec<BodyId>,
    #[serde(default)]
    target: PrintIntentTargetDto,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SetPartRequest {
    body_id: BodyId,
    settings: PrintSettingsDto,
    expected_model_json: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ResetPartRequest {
    body_id: BodyId,
    expected_model_json: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyPartRequest {
    source_body_id: BodyId,
    target_body_ids: Vec<BodyId>,
    expected_model_json: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SetDocumentRequest {
    document: PrintIntentDocumentDto,
    expected_model_json: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct UpsertPresetRequest {
    preset: PrintIntentPresetDto,
    expected_model_json: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RemovePresetRequest {
    name: String,
    expected_model_json: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct UpsertHandoffRequest {
    handoff: PrintTargetHandoffDto,
    expected_model_json: String,
}
pub(super) fn guard(manager: &SketchManager, expected: &str) -> Result<(), SessionError> {
    limo_cad_solid::check_export_model_snapshot(Some(expected), &manager.export_project_model()?)
        .map_err(|error| SessionError::Solid(error.into()))
}

pub(super) fn handle_read_only(
    manager: &SketchManager,
    method: &str,
    payload: &str,
) -> Option<String> {
    if payload.len() > 32 * 1024 * 1024 {
        return Some(err_json(
            "Print-intent payload exceeds 32 MiB, including its project snapshot",
        ));
    }
    Some(match method {
        "print_intent_get" if matches!(payload.trim(), "" | "null" | "{}") => {
            ok_json(manager.print_intent())
        }
        "print_intent_get" => err_json("print_intent_get takes no arguments"),
        "print_intent_effective" => with_payload(payload, |request: EffectiveRequest| {
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
        return err_json("Print-intent payload exceeds 32 MiB, including its project snapshot");
    }
    match method {
        "print_intent_set_part" => with_payload(payload, |request: SetPartRequest| {
            guard(manager, &request.expected_model_json)?;
            manager.set_part_print_intent(request.body_id, request.settings)
        }),
        "print_intent_reset_part" => with_payload(payload, |request: ResetPartRequest| {
            guard(manager, &request.expected_model_json)?;
            manager.reset_part_print_intent(request.body_id)
        }),
        "print_intent_copy_part" => with_payload(payload, |request: CopyPartRequest| {
            guard(manager, &request.expected_model_json)?;
            manager.copy_part_print_intent(request.source_body_id, request.target_body_ids)
        }),
        "print_intent_set_document" => with_payload(payload, |request: SetDocumentRequest| {
            guard(manager, &request.expected_model_json)?;
            manager.set_print_intent_document(request.document)
        }),
        "print_intent_upsert_preset" => with_payload(payload, |request: UpsertPresetRequest| {
            guard(manager, &request.expected_model_json)?;
            manager.upsert_print_intent_preset(request.preset)
        }),
        "print_intent_remove_preset" => with_payload(payload, |request: RemovePresetRequest| {
            guard(manager, &request.expected_model_json)?;
            manager.remove_print_intent_preset(&request.name)
        }),
        "print_intent_upsert_handoff" => with_payload(payload, |request: UpsertHandoffRequest| {
            guard(manager, &request.expected_model_json)?;
            manager.upsert_print_intent_handoff(request.handoff)
        }),
        "print_intent_remove_handoff" => with_payload(payload, |request: RemovePresetRequest| {
            guard(manager, &request.expected_model_json)?;
            manager.remove_print_intent_handoff(&request.name)
        }),
        _ => err_json(format!("unknown engine method: {method}")),
    }
}
