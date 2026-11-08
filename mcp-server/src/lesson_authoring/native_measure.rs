//! The prerequisite and native inspection use exactly the same isolated host.
//! No snapshot import, second replay, session discovery or runtime endpoint.
use limo_cad_mcp_mutate::{encode_payload, lookup_mutate, ExecutionKind};
use limo_cad_native_engine::NativeEngineHost;
use serde_json::Value;

fn unwrap_engine(raw: String) -> Result<Value, String> {
    let mut envelope: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if envelope["ok"] != true {
        return Err(envelope["error"]
            .as_str()
            .unwrap_or("Native authoring failed")
            .into());
    }
    Ok(envelope["value"].take())
}

pub(super) fn run(script: &limo_cad_script::Script) -> Result<Value, String> {
    let host = NativeEngineHost::new();
    if !host.is_blank_for_script() {
        return Err("Offline Measure requires a fresh isolated native host".into());
    }
    let mut report = limo_cad_script::run(
        script,
        |name, arguments| {
            if name != "cad_interface" || arguments["action"] != "execute" {
                return Err(
                    "Offline Measure accepts only its prerequisite modeling operations".into(),
                );
            }
            let operation = arguments["operation"].as_str().ok_or("Missing operation")?;
            let payload = &arguments["arguments"];
            // Deliberately narrow: the bundled recipe cannot attach, export, launch,
            // access files, or redirect execution to another document.
            match operation {
                "solid_scene" => unwrap_engine(host.engine_call("solid_scene", "")),
                "sketch_begin"
                | "sketch_set_grid_snap"
                | "sketch_add_line"
                | "sketch_add_constraint"
                | "sketch_add_constraints"
                | "sketch_add_dimension"
                | "sketch_finish"
                | "solid_extrude" => {
                    let spec =
                        lookup_mutate(operation).ok_or("Missing native operation mapping")?;
                    let encoded = encode_payload(spec.payload, payload)?;
                    unwrap_engine(host.apply_encoded_mutate(
                        spec.engine_method,
                        &encoded,
                        spec.execution == ExecutionKind::SolidReplay,
                    ))
                }
                _ => Err(format!(
                    "Unsupported offline Measure prerequisite: {operation}"
                )),
            }
        },
        limo_cad_script::RunOptions::default(),
    )?;
    super::measure::complete_measure_frames(&host, &mut report["exports"])?;
    // The host stays alive through native prepare/inspect/validate and export
    // completion, then drops here. Only immutable completed data leaves it.
    Ok(report)
}
