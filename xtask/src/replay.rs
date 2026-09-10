use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

pub(crate) struct Client {
    child: Child,
    input: ChildStdin,
    replies: Receiver<Result<Value, String>>,
    id: u64,
}
impl Client {
    pub(crate) fn start(executable: &str) -> Result<Self> {
        let mut command = Command::new(executable);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().context("Start MCP server")?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let result = line
                    .map_err(|e| e.to_string())
                    .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()));
                if sender.send(result).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            input,
            replies,
            id: 0,
        };
        client.rpc("initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"nbcad-rust-replay","version":"1"}}))?;
        writeln!(
            client.input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )?;
        Ok(client)
    }
    fn rpc(&mut self, method: &str, params: Value) -> Result<Value> {
        self.id += 1;
        let id = self.id;
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )?;
        self.input.flush()?;
        loop {
            match self.replies.recv_timeout(Duration::from_secs(30)) {
                Ok(Ok(reply)) => {
                    if reply["id"] != id {
                        continue;
                    }
                    if let Some(error) = reply.get("error") {
                        bail!("MCP error: {error}");
                    }
                    return Ok(reply["result"].clone());
                }
                Ok(Err(error)) => bail!("Invalid MCP reply: {error}"),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(status) = self.child.try_wait()? {
                        bail!("MCP exited: {status}");
                    }
                    eprintln!("Replay is running; live playback controls remain available.");
                }
                Err(error) => bail!("MCP connection closed: {error}"),
            }
        }
    }
    pub(crate) fn call(&mut self, name: &str, args: Value) -> Result<Value> {
        let result = self.rpc("tools/call", json!({"name":name,"arguments":args}))?;
        if result["isError"] == true {
            bail!("{name}: {}", result["content"]);
        }
        let text = result["content"]
            .as_array()
            .and_then(|a| a.iter().find(|v| v["type"] == "text"))
            .and_then(|v| v["text"].as_str())
            .ok_or_else(|| anyhow!("MCP response has no text"))?;
        let result: Value = serde_json::from_str(text)?;
        if result["status"] == "failed" {
            bail!("{name}: {result}");
        }
        Ok(result)
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn options(
    args: impl Iterator<Item = String>,
) -> Result<(HashMap<String, String>, Option<String>)> {
    let mut args = args.peekable();
    let mut values = HashMap::new();
    let mut file = None;
    while let Some(arg) = args.next() {
        if arg.starts_with("--") {
            if values.contains_key(&arg) {
                bail!("Duplicate option {arg}");
            }
            if matches!(arg.as_str(), "--present" | "--new") {
                values.insert(arg, "true".into());
            } else {
                values.insert(
                    arg.clone(),
                    args.next()
                        .filter(|v| !v.starts_with("--"))
                        .ok_or_else(|| anyhow!("Missing value for {arg}"))?,
                );
            }
        } else if file.replace(arg).is_some() {
            bail!("Only one script path is accepted");
        }
    }
    Ok((values, file))
}
fn known_options(args: &HashMap<String, String>, allowed: &[&str]) -> Result<()> {
    for key in args.keys() {
        if !allowed.contains(&key.as_str()) {
            bail!("Unknown option {key}");
        }
    }
    Ok(())
}
fn required<'a>(args: &'a HashMap<String, String>, name: &str) -> Result<&'a str> {
    args.get(name)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("Missing {name}"))
}
pub fn call(args: impl Iterator<Item = String>) -> Result<()> {
    let (args, file) = options(args)?;
    known_options(
        &args,
        &[
            "--server",
            "--session",
            "--tool",
            "--args",
            "--args-file",
            "--out",
        ],
    )?;
    if file.is_some() || args.contains_key("--args") && args.contains_key("--args-file") {
        bail!("Supply exactly one --args or --args-file");
    }
    let mut client = Client::start(required(&args, "--server")?)?;
    if let Some(session) = args.get("--session") {
        client.call("cad_attach", json!({"session_id":session}))?;
    }
    let arguments = if let Some(path) = args.get("--args-file") {
        fs::read_to_string(path)?
    } else {
        required(&args, "--args")?.to_owned()
    };
    let result = client.call(
        args.get("--tool")
            .map(String::as_str)
            .unwrap_or("cad_interface"),
        serde_json::from_str(&arguments)?,
    )?;
    if let Some(path) = args.get("--out") {
        fs::write(path, serde_json::to_string_pretty(&result)?)?;
    } else {
        println!("{}", serde_json::to_string_pretty(&result)?);
    }
    Ok(())
}
pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let (args, file) = options(args)?;
    known_options(
        &args,
        &[
            "--server",
            "--session",
            "--desktop",
            "--new",
            "--present",
            "--speed",
            "--repeat",
            "--save",
            "--out",
            "--compare",
            "--recipe",
        ],
    )?;
    let live = args.contains_key("--session") || args.contains_key("--desktop");
    if !live
        && (args.contains_key("--save")
            || args.contains_key("--new")
            || args.contains_key("--present"))
    {
        bail!("Save, new-tab and presentation options require a desktop session");
    }
    if args.contains_key("--session") && args.contains_key("--desktop") {
        bail!("Choose existing session or desktop launch");
    }
    let speed = args
        .get("--speed")
        .map(|s| s.parse::<f64>())
        .transpose()?
        .unwrap_or(1.0);
    if !speed.is_finite() || !(0.1..=16.0).contains(&speed) {
        bail!("Speed must be from 0.1 to 16");
    }
    if file.is_some() && args.contains_key("--recipe") {
        bail!("Choose a script path or --recipe ID");
    }
    let path = if args.contains_key("--recipe") {
        None
    } else {
        Some(fs::canonicalize(PathBuf::from(file.ok_or_else(|| {
            anyhow!("Supply a script path or --recipe ID")
        })?))?)
    };
    let server = required(&args, "--server")?;
    let repeat = args
        .get("--repeat")
        .map(|v| v.parse::<usize>())
        .transpose()?
        .unwrap_or(1);
    if repeat == 0 {
        bail!("repeat must be positive");
    }
    if repeat > 1 && (args.contains_key("--session") || args.contains_key("--desktop")) {
        bail!("Determinism repeats use independent headless processes; run the visible demonstration separately");
    }
    let mut baseline = args
        .get("--compare")
        .map(|p| -> Result<Value> {
            semantic_result(&serde_json::from_str(&fs::read_to_string(p)?)?)
        })
        .transpose()?;
    for iteration in 1..=repeat {
        let mut client = Client::start(server)?;
        let mut session = args.get("--session").cloned();
        if let Some(desktop) = args.get("--desktop") {
            if session.is_some() {
                bail!("Choose existing session or desktop launch");
            }
            let launch = client.call(
                "cad_interface",
                json!({"action":"launch","executable":desktop}),
            )?;
            session = launch["session_id"].as_str().map(str::to_owned);
        }
        if let Some(id) = &session {
            client.call("cad_attach", json!({"session_id":id}))?;
        }
        if args.contains_key("--new") {
            if session.is_none() {
                bail!("--new requires an existing desktop session");
            }
            let inspect = client.call("cad_interface", json!({"action":"inspect"}))?;
            let target = inspect["ui"]["surfaces"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|v| v["controls"].as_array().into_iter().flatten())
                .find(|v| v["label"] == "New design" && v["disabled"] == false)
                .and_then(|v| v["id"].as_str())
                .ok_or_else(|| anyhow!("New design control unavailable"))?;
            let reply = client.call("cad_interface", json!({"action":"click","target":target}))?;
            session = reply["active_session_id"].as_str().map(str::to_owned);
            client.call("cad_attach", json!({"session_id":session}))?;
        }
        let label = args
            .get("--recipe")
            .cloned()
            .unwrap_or_else(|| path.as_ref().unwrap().display().to_string());
        eprintln!("Running {label} ({iteration}/{repeat})");
        let mut request = json!({"action":"script","mode":if args.contains_key("--present"){"present"}else{"fast"},"speed":speed,"validate":true});
        if let Some(recipe) = args.get("--recipe") {
            request["recipe"] = json!(recipe);
        } else {
            request["path"] = json!(path);
        }
        let mut report = client.call("cad_interface", request)?;
        if let Some(session) = session {
            report["session_id"] = json!(session);
        }
        if let Some(save) = args.get("--save") {
            client.call(
                "cad_interface",
                json!({"action":"file","command":"save","path":save}),
            )?;
        }
        if let Some(out) = args.get("--out") {
            let out = Path::new(out);
            fs::create_dir_all(out)?;
            fs::write(
                out.join(format!("run-{iteration}.json")),
                serde_json::to_string_pretty(&report)?,
            )?;
            if let Some(model) = report.pointer("/exports/final_model") {
                fs::write(
                    out.join(format!("model-{iteration}.json")),
                    serde_json::to_string_pretty(model)?,
                )?;
            }
        }
        if repeat > 1 || args.contains_key("--compare") {
            let semantic = semantic_result(&report)?;
            if let Some(previous) = &baseline {
                if previous != &semantic {
                    let difference = first_difference(previous, &semantic, "").unwrap_or_default();
                    bail!("Replay {iteration} differs from the comparison model at {difference}");
                }
            } else {
                baseline = Some(semantic);
            }
        }
        eprintln!(
            "Completed {} steps and {} end checks in {} ms",
            report["steps_completed"], report["checks_completed"], report["elapsed_ms"]
        );
    }
    if repeat > 1 {
        println!("PASS: {repeat} independent runs produced identical model, sketches, assembly solution and geometry.");
    }
    if args.contains_key("--compare") {
        println!("PASS: replay matches the comparison model, sketches, assembly and geometry.");
    }
    Ok(())
}

fn semantic_result(report: &Value) -> Result<Value> {
    let exports = report
        .get("exports")
        .ok_or_else(|| anyhow!("Script did not return exports"))?;
    let mut selected = serde_json::Map::new();
    for key in [
        "final_model",
        "final_scene",
        "final_solution",
        "final_sketches",
    ] {
        if let Some(value) = exports.get(key) {
            let mut value = value.clone();
            // Tool-disclosure hints describe the client session, not the CAD.
            if let Some(object) = value.as_object_mut() {
                object.remove("_disclosure");
            }
            if key == "final_sketches" {
                if let Some(sketches) = value.as_array_mut() {
                    for sketch in sketches {
                        if let Some(sketch) = sketch.as_object_mut() {
                            // Restoring a project intentionally clears editing undo
                            // and regenerates snap candidates on sketch entry. The
                            // persisted sketch constraints/references remain intact.
                            for transient in ["can_undo", "can_redo", "reference_midpoints"] {
                                sketch.remove(transient);
                            }
                        }
                    }
                }
            }
            selected.insert(key.into(), value);
        }
    }
    if selected.is_empty() {
        bail!("Script must export a final model or scene for determinism comparison");
    }
    Ok(Value::Object(selected))
}
fn first_difference(a: &Value, b: &Value, path: &str) -> Option<String> {
    if a == b {
        return None;
    }
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                let next = format!("{path}/{key}");
                if let Some(other) = b.get(key) {
                    if let Some(found) = first_difference(value, other, &next) {
                        return Some(found);
                    }
                } else {
                    return Some(next);
                }
            }
            b.keys()
                .find(|key| !a.contains_key(*key))
                .map(|key| format!("{path}/{key}"))
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Some(format!("{path}/length"));
            }
            a.iter()
                .zip(b)
                .enumerate()
                .find_map(|(i, (a, b))| first_difference(a, b, &format!("{path}/{i}")))
        }
        _ => Some(path.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_model_difference_without_dropping_geometry() {
        let a =
            json!({"exports":{"final_scene":{"_disclosure":{"focus":"a"},"bodies":[{"id":1}]}}});
        let b =
            json!({"exports":{"final_scene":{"_disclosure":{"focus":"b"},"bodies":[{"id":1}]}}});
        assert_eq!(semantic_result(&a).unwrap(), semantic_result(&b).unwrap());
        let b = json!({"exports":{"final_scene":{"bodies":[{"id":2}]}}});
        assert_eq!(
            first_difference(
                &semantic_result(&a).unwrap(),
                &semantic_result(&b).unwrap(),
                ""
            ),
            Some("/final_scene/bodies/0/id".into())
        );
    }
    #[test]
    fn rejects_ignored_and_duplicate_options() {
        let args = HashMap::from([("--speed".to_owned(), "2".to_owned())]);
        assert!(known_options(&args, &["--server"]).is_err());
        assert!(options(
            ["--server", "a", "--server", "b"]
                .into_iter()
                .map(str::to_owned)
        )
        .is_err());
    }
    #[test]
    fn comparison_preserves_small_geometry_normals_across_json_roundtrips() {
        let value = json!({"normal":-1.1728120758078999e-17_f64});
        let restored: Value =
            serde_json::from_str(&serde_json::to_string_pretty(&value).unwrap()).unwrap();
        assert_eq!(value, restored);
    }
    #[test]
    fn comparison_keeps_persisted_sketch_references_but_ignores_editing_cache() {
        let constraint =
            json!({"type":"reference_midpoint","edge_id":42,"position":{"x":2.0,"y":3.0}});
        let a = json!({"exports":{"final_sketches":[{"can_undo":true,"reference_midpoints":[42],"constraints":[constraint]}]}});
        let b = json!({"exports":{"final_sketches":[{"can_undo":false,"reference_midpoints":[],"constraints":[constraint]}]}});
        assert_eq!(semantic_result(&a).unwrap(), semantic_result(&b).unwrap());
        let mut changed = b;
        changed["exports"]["final_sketches"][0]["constraints"][0]["position"]["x"] = json!(2.1);
        assert_ne!(
            semantic_result(&a).unwrap(),
            semantic_result(&changed).unwrap()
        );
    }
}
