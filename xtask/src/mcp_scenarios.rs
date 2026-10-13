//! Product scenarios use the same owned Rust stdio transport as command replay.
mod bench;
mod controls;
mod drawing;
mod exit;
mod live;
mod workshop;

use crate::replay::Client;
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Scenario {
    client: Client,
    options: HashMap<String, String>,
    report: Value,
    tools: Vec<Value>,
    session: Option<String>,
}

pub fn run(suite: &str, args: impl Iterator<Item = String>) -> Result<()> {
    let options = options(args)?;
    let server = options
        .get("--server")
        .context("Use --server PATH for the MCP executable")?;
    ensure!(
        !(options.contains_key("--desktop") && options.contains_key("--session")),
        "Choose --desktop or --session"
    );
    if let Some(save) = options.get("--save") {
        ensure!(
            PathBuf::from(save).is_absolute(),
            "Save paths must be absolute"
        );
        ensure!(
            !PathBuf::from(save).exists(),
            "Choose a new save path; existing projects are preserved"
        );
    }
    let client = Client::start_command(
        Client::worker_command(server),
        Some(Duration::from_secs(60)),
    )?;
    let session = options.get("--session").cloned();
    let mut scenario = Scenario {
        client,
        options,
        session,
        tools: vec![],
        report: json!({"calls":[],"parts":[],"checks":[],"views":[],"cases":[]}),
    };
    let result = match suite {
        "bench" => bench::run(&mut scenario),
        "drawing" => drawing::run(&mut scenario),
        "controls" => controls::run(&mut scenario),
        "live" => live::run(&mut scenario),
        "exit" => exit::run(&mut scenario),
        _ => bail!("Unknown Rust MCP scenario {suite}"),
    };
    scenario.report["status"] = json!(if result.is_ok() { "passed" } else { "failed" });
    if let Err(error) = &result {
        scenario.report["error"] = json!(format!("{error:#}"));
    }
    if let Some(out) = scenario.options.get("--out") {
        let path = if suite == "bench" {
            fs::create_dir_all(out)?;
            PathBuf::from(out).join("bench-report.json")
        } else {
            PathBuf::from(out)
        };
        fs::write(&path, serde_json::to_vec_pretty(&scenario.report)?)
            .with_context(|| format!("Write {}", path.display()))?;
    }
    result
}

fn options(args: impl Iterator<Item = String>) -> Result<HashMap<String, String>> {
    let mut args = args.peekable();
    let mut result = HashMap::new();
    while let Some(key) = args.next() {
        let value = match key.as_str() {
            "--part" | "--drawing" | "--idle" => String::new(),
            "--server" | "--desktop" | "--session" | "--out" | "--save" | "--pace"
            | "--workshop" | "--model" | "--case" => {
                args.next()
                    .filter(|v| !v.starts_with("--"))
                    .with_context(|| format!("Missing value for {key}"))?
            }
            _ => bail!("Unknown MCP scenario option {key}"),
        };
        ensure!(
            result.insert(key.clone(), value).is_none(),
            "Duplicate option {key}"
        );
    }
    if let Some(value) = result.get("--pace") {
        ensure!(
            value.parse::<u64>().is_ok_and(|n| n <= 2000),
            "--pace must be 0..2000 milliseconds"
        );
    }
    if let Some(value) = result.get("--workshop") {
        ensure!(value == "all", "--workshop accepts all");
    }
    Ok(result)
}

impl Scenario {
    fn call(&mut self, name: &str, arguments: Value) -> Result<Value> {
        let start = Instant::now();
        let index = self.report["calls"].as_array().unwrap().len();
        self.report["calls"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":name,"arguments":arguments}));
        let result = if let Some(tool) = self
            .tools
            .iter()
            .find(|tool| tool["name"] == name && name != "cad_interface")
        {
            self.client.call("cad_interface", json!({"action":"execute","group":tool["group"],"operation":name,"arguments":arguments}))?
        } else {
            self.client.call(name, arguments.clone())?
        };
        if let Some(errors) = result.pointer("/scene/errors") {
            ensure!(
                errors.as_array().is_some_and(Vec::is_empty),
                "{name} returned geometry errors: {errors}"
            );
        }
        self.report["calls"][index]["elapsed_ms"] = json!(start.elapsed().as_millis());
        Ok(result)
    }
    fn interface(&mut self, mut arguments: Value) -> Result<Value> {
        if let Some(session) = &self.session {
            arguments["session_id"] = json!(session);
        }
        let index = self.report["calls"].as_array().unwrap().len();
        self.report["calls"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"cad_interface","arguments":arguments}));
        let raw = self.client.rpc(
            "tools/call",
            json!({"name":"cad_interface","arguments":arguments}),
        )?;
        ensure!(raw["isError"] != true, "Interface tool failed: {raw}");
        let value: Value = serde_json::from_str(
            raw["content"]
                .as_array()
                .and_then(|rows| rows.iter().find(|r| r["type"] == "text"))
                .and_then(|row| row["text"].as_str())
                .context("Interface response has no text")?,
        )?;
        if let Some(session) = value["active_session_id"].as_str() {
            self.session = Some(session.into());
        }
        self.report["calls"][index]["result"] = value.clone();
        Ok(value)
    }
    fn ui(&mut self, arguments: Value) -> Result<Value> {
        let result = self.interface(arguments)?;
        ensure!(
            result["status"] == "applied",
            "Interface did not apply: {result}"
        );
        Ok(result)
    }
    fn model(&mut self) -> Result<Value> {
        let text = self.call("cad_project_model", json!({}))?;
        serde_json::from_str(text.as_str().context("Project model is not text")?)
            .context("Parse model")
    }
    fn control(&mut self, label: &str, value: Option<&str>) -> Result<Value> {
        let state = self.ui(json!({"action":"inspect"}))?;
        let found: Vec<_> = controls_in(&state)
            .filter(|row| row["label"] == label && row["disabled"] != true)
            .collect();
        ensure!(
            found.len() == 1,
            "Expected one enabled {label}, got {found:?}"
        );
        self.ui(if let Some(value) = value {
            json!({"action":"set_value","target":found[0]["id"],"value":value})
        } else {
            json!({"action":"click","target":found[0]["id"]})
        })
    }
    fn attach_or_launch(&mut self) -> Result<()> {
        if let Some(desktop) = self.options.get("--desktop") {
            let launch = self.call(
                "cad_interface",
                json!({"action":"launch","executable":desktop}),
            )?;
            ensure!(
                launch["status"] == "ready" && launch["attached"] == true,
                "Desktop did not bind its owned session: {launch}"
            );
            self.session = Some(
                launch["session_id"]
                    .as_str()
                    .context("Launch session missing")?
                    .into(),
            );
            self.report["pid"] = launch["pid"].clone();
        } else if let Some(session) = self.session.clone() {
            self.call("cad_attach", json!({"session_id":session}))?;
        }
        self.report["session_id"] = json!(self.session);
        Ok(())
    }
    fn check(&mut self, name: &str) {
        self.report["checks"]
            .as_array_mut()
            .unwrap()
            .push(json!(name));
        println!("PASS {name}");
    }
}

fn array<'a>(value: &'a Value, name: &str) -> Result<&'a [Value]> {
    value[name]
        .as_array()
        .map(Vec::as_slice)
        .with_context(|| format!("Missing {name}: {value}"))
}
fn last(value: &Value, name: &str) -> Result<Value> {
    array(value, name)?
        .last()
        .cloned()
        .with_context(|| format!("Empty {name}"))
}
fn number(value: &Value) -> Result<f64> {
    value
        .as_f64()
        .filter(|n| n.is_finite())
        .context("Expected finite number")
}
fn controls_in(value: &Value) -> impl Iterator<Item = &Value> {
    value["ui"]["surfaces"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|s| s["controls"].as_array().into_iter().flatten())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scenario_options_reject_missing_duplicate_or_unbounded_input() {
        for args in [
            vec!["--server"],
            vec!["--server", "--out", "x"],
            vec!["--pace", "2001"],
            vec!["--pace", "NaN"],
            vec!["--server", "a", "--server", "b"],
            vec!["--workshop", "partial"],
        ] {
            assert!(options(args.into_iter().map(str::to_owned)).is_err());
        }
        assert_eq!(
            options(
                ["--server", "path with spaces", "--pace", "2000", "--part"]
                    .into_iter()
                    .map(str::to_owned)
            )
            .unwrap()["--server"],
            "path with spaces"
        );
    }
}
