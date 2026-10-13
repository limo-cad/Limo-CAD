//! Report the actual GUI and MCP identities on live inspection. Protocol
//! compatibility remains available independently of qualification provenance.
use serde_json::{json, Value};

/// Only identical, clean, known builds can claim a matched qualification pair.
pub(super) fn decorate(result: &mut Value) {
    let desktop = result.get("desktop_build").cloned().unwrap_or(Value::Null);
    let mcp = json!(limo_cad_build_info::build_info());
    let known = |build: &Value| {
        build["revision"].as_str().is_some_and(|revision| {
            revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        }) && build["modified"].is_boolean()
    };
    let status = if !known(&desktop) || !known(&mcp) {
        "unknown"
    } else if desktop["modified"] != false || mcp["modified"] != false {
        "unverified_modified"
    } else if desktop == mcp {
        "matched"
    } else {
        "different"
    };
    result["build_pair"] = json!({"status":status,"desktop":desktop,"mcp":mcp});
}
