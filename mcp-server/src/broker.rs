//! Stateless routing through the desktop's existing document-owned queues.
use super::*;
use serde::{Deserialize, Serialize};
mod batch;

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Submit {
        route: Selectors,
        name: String,
        #[serde(default = "empty_arguments")]
        arguments: Value,
        base_generation: Option<u64>,
    },
    Status {
        ticket: Ticket,
    },
    Batch(batch::Request),
}

fn empty_arguments() -> Value {
    json!({})
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selectors {
    session_id: Option<String>,
    window_id: Option<String>,
    document_id: Option<String>,
    process_instance_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    session_id: String,
    window_id: String,
    document_id: String,
    process_instance_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ticket {
    route: Route,
    operation: Pending,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Pending {
    Inbox { seq: u64 },
    Control { request_id: String, expires_ms: u64 },
}

pub(super) fn specs() -> Vec<ToolSpec> {
    let route = object_schema(
        json!({
            "session_id":{"type":"string"},"window_id":{"type":"string"},
            "document_id":{"type":"string"},"process_instance_id":{"type":"string"}
        }),
        &[],
    );
    let submit = object_schema(
        json!({
            "action":{"type":"string","enum":["submit"]},"route":route,
            "name":{"type":"string"},"arguments":{"type":"object"},
            "base_generation":{"type":"integer","minimum":0}
        }),
        &["action", "route", "name"],
    );
    let status = object_schema(
        json!({
            "action":{"type":"string","enum":["status"]},
            "ticket":{"type":"object","description":"The complete ticket returned by submit or an individual batch receipt, including its original route and operation."}
        }),
        &["action", "ticket"],
    );
    let variants = [submit, status, batch::schema(route)];
    let mut properties = json!({});
    for variant in &variants {
        for (key, value) in variant["properties"].as_object().unwrap() {
            properties[key] = value.clone();
        }
    }
    properties["action"] = json!({"type":"string","enum":["submit","status","batch"]});
    let mut input_schema = object_schema(properties, &["action"]);
    input_schema["oneOf"] = json!(variants);
    vec![ToolSpec::control(
        "cad_route", "Route a live CAD request",
        "One broker addresses several desktop documents without changing cad_attach selection or copying their models. action=submit sends one modeling tool, live query, or cad_interface UI action to an explicit route from cad_list_sessions and returns immediately. action=status polls its complete ticket without waiting or retargeting. action=batch applies 1–16 literal typed modeling operations or live engine queries in order on one explicit route, using route, base_generation, calls and optional timeout_ms/include_values. The batch validates all names and argument envelopes before publication, awaits each normal receipt and stops on failure, owner replacement, intervening edits or its single deadline (maximum 30 seconds). Successful calls remain applied with separate Undo entries; the batch is not atomic. Each result retains its own ticket: poll pending tickets with action=status before retrying any operation, and never repeat the whole batch. include_values=false omits operation values while retaining receipts. Batch calls do not support scripts, generated-ID references, loops, files, UI targets or nested broker actions. All route selectors are intersected; ambiguous, closed, stale and mismatched owners reject before publication. Mutations, batch requests and UI actions other than inspect/capture require the current base_generation. Modeling writes use each document's ordered inbox; queries and UI actions use its separate owner-fenced control queue. Submission does not switch tabs; activate an inactive document before submitting UI controls. Modeling writes can remain queued until activation. Receipts survive replacement, close and process exit. Scripts and offline tools retain their existing explicit attachment workflow.",
        input_schema,
    )]
}

fn resolve(selectors: Selectors) -> Result<Route, String> {
    if [
        selectors.session_id.as_deref(),
        selectors.window_id.as_deref(),
        selectors.document_id.as_deref(),
        selectors.process_instance_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.trim().is_empty())
    {
        return Err("Route selectors must not be empty".into());
    }
    if selectors.session_id.is_none()
        && selectors.window_id.is_none()
        && selectors.document_id.is_none()
        && selectors.process_instance_id.is_none()
    {
        return Err("An explicit document route is required".into());
    }
    if let Some(session_id) = &selectors.session_id {
        session::require_valid_session_id(session_id)?;
    }
    let listing = session::sessions_list_json();
    let matches: Vec<_> = listing["session_details"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|detail| {
            selectors
                .session_id
                .as_deref()
                .is_none_or(|selected| detail["session_id"] == selected)
                && selectors
                    .window_id
                    .as_deref()
                    .is_none_or(|selected| detail["window_id"] == selected)
                && selectors.document_id.as_deref().is_none_or(|selected| {
                    detail["document_id"] == selected || detail["session_id"] == selected
                })
                && selectors
                    .process_instance_id
                    .as_deref()
                    .is_none_or(|selected| detail["process_instance_id"] == selected)
                && detail["closed"] != true
                && detail["live_for_windows"] == true
        })
        .collect();
    let detail = match matches.as_slice() {
        [detail] => *detail,
        [] => {
            return Err("The route has no matching live desktop; refresh cad_list_sessions".into())
        }
        _ => {
            return Err(
                "The route is ambiguous; include process, window and document identities".into(),
            )
        }
    };
    if detail["closed"] == true
        || detail["live_for_windows"] != true
        || detail["heartbeat"]["interface_version"] != 1
    {
        return Err("The route needs a live native desktop; refresh cad_list_sessions".into());
    }
    let process = detail["process_instance_id"]
        .as_str()
        .ok_or("The desktop has no process instance identity")?;
    Ok(Route {
        session_id: detail["session_id"]
            .as_str()
            .ok_or("The desktop has no session identity")?
            .into(),
        window_id: detail["window_id"]
            .as_str()
            .ok_or("The desktop has no window identity")?
            .into(),
        document_id: detail["document_id"]
            .as_str()
            .ok_or("The desktop has no document identity")?
            .into(),
        process_instance_id: process.into(),
    })
}

pub(super) fn call(arguments: Value) -> Result<Value, String> {
    let started = std::time::Instant::now();
    if arguments["action"] == "batch" {
        batch::validate_size(&arguments)?;
    }
    match serde_json::from_value::<Request>(arguments).map_err(|error| error.to_string())? {
        Request::Submit {
            route,
            name,
            arguments,
            base_generation,
        } => submit(resolve(route)?, &name, arguments, base_generation),
        Request::Status { ticket } => status(ticket),
        Request::Batch(request) => batch::call(request, started),
    }
}

/// Summaries inspect the owning desktop's existing scene without loading it into MCP.
/// The original owner and generation stay fenced throughout submission and polling.
pub(super) fn inspect_solid_scene(session_id: &str) -> Result<Value, String> {
    let route = resolve(Selectors {
        session_id: Some(session_id.into()),
        window_id: None,
        document_id: None,
        process_instance_id: None,
    })?;
    let generation = session::read_heartbeat_generation(&route.session_id)?;
    let submitted = submit(route, "solid_scene", json!({}), Some(generation))?;
    let ticket: Ticket = serde_json::from_value(submitted["ticket"].clone()).map_err(|error| {
        json!({"status":"ticket_error","error":error.to_string(),"submitted":submitted}).to_string()
    })?;
    let deadline = std::time::Instant::now() + Duration::from_secs(31);
    loop {
        let mut receipt = status(ticket.clone()).map_err(|error| {
            json!({"status":"receipt_error","error":error,"ticket":ticket,
                "base_generation":generation,"writeback":false})
            .to_string()
        })?;
        receipt["base_generation"] = json!(generation);
        if receipt["status"] == "applied" {
            return Ok(receipt);
        }
        if receipt["status"] != "pending" {
            return Err(receipt.to_string());
        }
        if std::time::Instant::now() >= deadline {
            receipt["status"] = json!("timeout");
            receipt["hint"] = json!("Live scene query is still pending. Poll its retained cad_route ticket; this helper never resubmits or falls back to another scene.");
            return Err(receipt.to_string());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn submit(
    route: Route,
    name: &str,
    mut arguments: Value,
    base_generation: Option<u64>,
) -> Result<Value, String> {
    if !arguments.is_object() {
        return Err("Routed tool arguments must be an object".into());
    }
    let spec = tool_specs()
        .iter()
        .find(|spec| spec.name == name)
        .ok_or_else(|| format!("unknown tool: {name}"))?;
    let mut owner = serde_json::to_value(&route).map_err(|error| error.to_string())?;
    if let Some(base) = base_generation {
        owner["base_generation"] = json!(base);
    }
    let operation = if name == "cad_interface" {
        if arguments
            .get("session_id")
            .is_some_and(|value| value != &route.session_id)
        {
            return Err("UI session_id conflicts with the explicit route".into());
        }
        if !matches!(arguments["action"].as_str(), Some("inspect" | "capture"))
            && base_generation.is_none()
        {
            return Err("Routed UI actions require base_generation from cad_list_sessions".into());
        }
        arguments["session_id"] = json!(route.session_id);
        let control = session::submit_ui(&arguments, &owner)?;
        Pending::Control {
            request_id: control.request_id,
            expires_ms: control.expires_ms,
        }
    } else if is_modeling_mutate(name) {
        let base = base_generation
            .ok_or("Routed mutations require base_generation from cad_list_sessions")?;
        let current = session::read_heartbeat_generation(&route.session_id)?;
        if current != base {
            return Err(session::generation_conflict_error(
                &route.session_id,
                base,
                Some(current),
            ));
        }
        let identity = session::session_identity(&route.session_id);
        if identity.window_id.as_deref() != Some(&route.window_id)
            || identity.document_id.as_deref() != Some(&route.document_id)
        {
            return Err("The document owner changed before submission".into());
        }
        let seq = session::write_inbox_op(
            &route.session_id,
            &session::InboxOp::unstamped(name, arguments, base).with_identity(&identity),
        )?;
        Pending::Inbox { seq }
    } else if limo_cad_mcp_mutate::is_routed_engine_query(spec.engine_method) {
        let payload = limo_cad_mcp_mutate::encode_payload(spec.payload, &arguments)?;
        let control = session::submit_engine_query(spec.engine_method, &payload, &owner)?;
        Pending::Control {
            request_id: control.request_id,
            expires_ms: control.expires_ms,
        }
    } else {
        return Err("Routing supports native modeling tools, live engine queries and cad_interface UI actions".into());
    };
    Ok(
        json!({"status":"submitted","submitted":true,"applied":false,"writeback":false,
        "ticket":Ticket { route, operation }}),
    )
}

fn status(ticket: Ticket) -> Result<Value, String> {
    session::require_valid_session_id(&ticket.route.session_id)?;
    let session_id = &ticket.route.session_id;
    let mut result = match &ticket.operation {
        Pending::Inbox { seq } => {
            let mut receipt = session::await_inbox_apply(session_id, *seq, 0, 1)?;
            if receipt["status"] == "applied" {
                if let Ok(body) =
                    session::read_session_file(session_id, &format!("inbox/results/{seq}.json"))
                {
                    receipt["value"] =
                        serde_json::from_str(&body).map_err(|error| error.to_string())?;
                }
            }
            receipt
        }
        Pending::Control {
            request_id,
            expires_ms,
        } => {
            if request_id.is_empty()
                || !request_id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'-')
            {
                return Err("Invalid control request id".into());
            }
            if let Ok(body) = session::read_session_file(
                session_id,
                &format!("controls/{request_id}.result.json"),
            ) {
                serde_json::from_str(&body).map_err(|error| error.to_string())?
            } else if session::is_session_closed(session_id) {
                json!({"status":"closed","applied":false})
            } else if *expires_ms < session::now_ms() {
                json!({"status":"expired","applied":false})
            } else {
                json!({"status":"pending","applied":false})
            }
        }
    };
    result["ticket"] = serde_json::to_value(ticket).map_err(|error| error.to_string())?;
    if result.get("desktop_build").is_some() {
        build_pair::decorate(&mut result);
    }
    result["writeback"] = json!(false);
    Ok(result)
}

#[cfg(test)]
mod tests;
