//! Bounded, ordered calls through the existing live document broker. Each
//! operation retains its normal receipt and document history entry.
use super::*;
use std::time::{Duration, Instant};

const MAX_CALLS: usize = 16;
const MAX_BYTES: usize = 256 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    route: Selectors,
    base_generation: u64,
    calls: Vec<Call>,
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
    #[serde(default = "include_values")]
    include_values: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    name: String,
    #[serde(default = "empty_arguments")]
    arguments: Value,
}

fn default_timeout() -> u64 {
    30_000
}

fn include_values() -> bool {
    true
}

pub(super) fn schema(route: Value) -> Value {
    object_schema(
        json!({
            "action":{"type":"string","enum":["batch"]},
            "route":route,
            "base_generation":{"type":"integer","minimum":0},
            "calls":{"type":"array","minItems":1,"maxItems":MAX_CALLS,
                "items":object_schema(json!({"name":{"type":"string","minLength":1},"arguments":{"type":"object"}}), &["name"])},
            "timeout_ms":{"type":"integer","minimum":1,"maximum":30_000,"default":30_000},
            "include_values":{"type":"boolean","default":true}
        }),
        &["action", "route", "base_generation", "calls"],
    )
}

/// Check the entire request envelope before its first live write. Domain
/// validation still belongs to the owning engine at each operation.
fn validate(request: &Request) -> Result<(), String> {
    if request.calls.is_empty() || request.calls.len() > MAX_CALLS {
        return Err(format!(
            "cad_route action=batch requires 1–{MAX_CALLS} calls"
        ));
    }
    if !(1..=30_000).contains(&request.timeout_ms) {
        return Err("cad_route action=batch timeout_ms must be from 1 to 30000".into());
    }
    for (index, call) in request.calls.iter().enumerate() {
        let spec = tool_specs()
            .iter()
            .find(|spec| spec.name == call.name)
            .ok_or_else(|| {
                format!(
                    "cad_route action=batch call {index}: unknown tool {}",
                    call.name
                )
            })?;
        if !limo_cad_mcp_mutate::lookup_mutate(&call.name)
            .is_some_and(|mapping| !mapping.is_read_only())
            && !limo_cad_mcp_mutate::is_routed_engine_query(spec.engine_method)
        {
            return Err(format!("cad_route action=batch call {index}: {} is not a native modeling operation or live engine query", call.name));
        }
        let arguments = call.arguments.as_object().ok_or_else(|| {
            format!("cad_route action=batch call {index}: arguments must be an object")
        })?;
        for key in spec.input_schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !arguments.contains_key(key) {
                return Err(format!(
                    "cad_route action=batch call {index}: {} requires {key}",
                    call.name
                ));
            }
        }
        if spec.input_schema["additionalProperties"] == false {
            for key in arguments.keys() {
                if spec.input_schema["properties"].get(key).is_none() {
                    return Err(format!(
                        "cad_route action=batch call {index}: {} has no argument {key}",
                        call.name
                    ));
                }
            }
        }
        limo_cad_mcp_mutate::encode_payload(spec.payload, &call.arguments)?;
    }
    Ok(())
}

fn selectors(route: &Route) -> Selectors {
    Selectors {
        session_id: Some(route.session_id.clone()),
        window_id: Some(route.window_id.clone()),
        document_id: Some(route.document_id.clone()),
        process_instance_id: Some(route.process_instance_id.clone()),
    }
}

pub(super) fn validate_size(arguments: &Value) -> Result<(), String> {
    if serde_json::to_vec(arguments)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_BYTES
    {
        return Err("cad_route action=batch request exceeds 256 KiB".into());
    }
    Ok(())
}

/// Publication fences from each applied receipt authorize only this batch's
/// next call; a different document or intervening edit stops publication.
pub(super) fn call(request: Request, started: Instant) -> Result<Value, String> {
    validate(&request)?;
    let route = resolve(request.route)?;
    let timeout = Duration::from_millis(request.timeout_ms);
    let mut generation = request.base_generation;
    let mut results = Vec::with_capacity(request.calls.len());
    for (index, call) in request.calls.iter().enumerate() {
        let outcome = (|| -> Result<Value, String> {
            if started.elapsed() >= timeout {
                return Err(
                    "cad_route action=batch deadline reached; this call was not submitted".into(),
                );
            }
            resolve(selectors(&route))?;
            let current = session::read_heartbeat_generation(&route.session_id)?;
            if current != generation {
                return Err(session::generation_conflict_error(
                    &route.session_id,
                    generation,
                    Some(current),
                ));
            }
            if started.elapsed() >= timeout {
                return Err(
                    "cad_route action=batch deadline reached; this call was not submitted".into(),
                );
            }
            let submitted = submit(
                route.clone(),
                &call.name,
                call.arguments.clone(),
                Some(generation),
            )?;
            let ticket: Ticket = serde_json::from_value(submitted["ticket"].clone())
                .map_err(|error| error.to_string())?;
            loop {
                let mut receipt = match status(ticket.clone()) {
                    Ok(receipt) => receipt,
                    Err(error) => {
                        return Ok(
                            json!({"status":"receipt_error","error":error,"ticket":ticket,"applied":null}),
                        )
                    }
                };
                if !matches!(receipt["status"].as_str(), Some("pending" | "timeout")) {
                    if receipt["status"] == "applied"
                        && matches!(ticket.operation, Pending::Inbox { .. })
                    {
                        if let Some(published) = receipt["published_generation"].as_u64() {
                            generation = published;
                        } else {
                            receipt["batch_stopped"] = json!(true);
                            receipt["batch_error"] = json!("Applied receipt omitted its publication fence; do not retry the operation");
                        }
                    }
                    if !request.include_values {
                        receipt.as_object_mut().unwrap().remove("value");
                    }
                    return Ok(receipt);
                }
                if started.elapsed() >= timeout {
                    receipt["status"] = json!("pending");
                    receipt["batch_error"] = json!("Batch deadline reached; poll this cad_route ticket before deciding whether to retry");
                    return Ok(receipt);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })();
        let receipt =
            outcome.unwrap_or_else(|error| json!({"status":"not_submitted","error":error}));
        let applied = receipt["status"] == "applied";
        let stop = !applied || receipt["batch_stopped"] == true;
        results.push(json!({"index":index,"name":call.name,"submitted":receipt["ticket"].is_object(),"receipt":receipt}));
        if stop {
            break;
        }
    }
    let completed = results
        .iter()
        .filter(|result| result["receipt"]["status"] == "applied")
        .count();
    let all_applied = completed == request.calls.len()
        && results
            .iter()
            .all(|result| result["receipt"]["batch_stopped"] != true);
    let next_index = results.len();
    Ok(json!({
        "status":if all_applied {"applied"} else {"stopped"},
        "route":route,"base_generation":request.base_generation,"next_generation":generation,
        "completed":completed,"results":results,
        "not_submitted":request.calls.iter().enumerate().skip(next_index).map(|(index, call)| json!({"index":index,"name":call.name})).collect::<Vec<_>>(),
        "elapsed_ms":started.elapsed().as_millis(),"writeback":false,
        "hint":"Inspect the receipts and live document before planning another batch. Completed calls remain applied; poll pending cad_route tickets instead of repeating the batch."
    }))
}
