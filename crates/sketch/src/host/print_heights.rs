use super::*;
use limo_cad_core::{
    BodyId, PrintHeightLayoutDto, PrintHeightRangeDraftDto, PrintLayerHeightProfileDraftDto,
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingRequest {
    body_id: BodyId,
    layout: PrintHeightLayoutDto,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeRequest {
    range: PrintHeightRangeDraftDto,
    expected_model_json: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileRequest {
    profile: PrintLayerHeightProfileDraftDto,
    expected_model_json: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveRequest {
    id: String,
    expected_model_json: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RebindRequest {
    id: String,
    layout: PrintHeightLayoutDto,
    #[serde(default)]
    interval: Option<limo_cad_core::PrintHeightIntervalDto>,
    #[serde(default)]
    points: Option<Vec<limo_cad_core::PrintLayerHeightPointDto>>,
    expected_model_json: String,
}

fn guard(manager: &SketchManager, expected: &str) -> Result<(), SessionError> {
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
        "print_intent_height_binding" => with_payload(payload, |request: BindingRequest| {
            manager.print_height_binding(request.body_id, request.layout)
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
        "print_intent_upsert_height_range" => with_payload(payload, |request: RangeRequest| {
            guard(manager, &request.expected_model_json)?;
            manager.upsert_print_height_range(request.range)
        }),
        "print_intent_upsert_layer_profile" => with_payload(payload, |request: ProfileRequest| {
            guard(manager, &request.expected_model_json)?;
            manager.upsert_print_layer_profile(request.profile)
        }),
        "print_intent_remove_height" => with_payload(payload, |request: RemoveRequest| {
            guard(manager, &request.expected_model_json)?;
            manager.remove_print_height(&request.id)
        }),
        "print_intent_rebind_height" => with_payload(payload, |request: RebindRequest| {
            guard(manager, &request.expected_model_json)?;
            manager.rebind_print_height(
                &request.id,
                request.layout,
                request.interval,
                request.points,
            )
        }),
        _ => err_json(format!("unknown engine method: {method}")),
    }
}
