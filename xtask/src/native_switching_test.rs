//! Bounded semantic switching measurements on an owned disposable Xvfb desktop.
//! The same archived inputs drive the baseline and candidate Bevy executables.
//! Request completion is measured; GPU presentation and physical Alt+Tab are not.
use crate::{
    native_fixture::{controls, ui},
    native_platform_test::{wait_for_interface, wait_for_owned_window},
    replay::Client,
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
mod processes;
mod settle;

struct Options {
    server: PathBuf,
    out: PathBuf,
    models: Vec<PathBuf>,
    sheets: bool,
    commit: String,
    profile: String,
    cycles: usize,
    instances: usize,
}
impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut values = HashMap::new();
        while let Some(key) = args.next() {
            ensure!(
                [
                    "--server",
                    "--out",
                    "--model-a",
                    "--model-b",
                    "--commit",
                    "--profile",
                    "--cycles",
                    "--instances",
                    "--scenario"
                ]
                .contains(&key.as_str()),
                "Unknown switching option {key}"
            );
            let value = args
                .next()
                .with_context(|| format!("Missing {key} value"))?;
            ensure!(
                values.insert(key.clone(), value).is_none(),
                "Duplicate {key}"
            );
        }
        let required = |key| {
            values
                .get(key)
                .cloned()
                .with_context(|| format!("Use {key}"))
        };
        let cycles = values
            .get("--cycles")
            .map_or(Ok(20), |v| v.parse::<usize>())?;
        let instances = values
            .get("--instances")
            .map_or(Ok(1), |v| v.parse::<usize>())?;
        ensure!(
            (1..=50).contains(&cycles) && (1..=2).contains(&instances),
            "Use 1-50 cycles and 1-2 instances"
        );
        let commit = required("--commit")?;
        ensure!(
            commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
            "--commit needs the exact 40-character build source SHA"
        );
        let sheets = match values
            .get("--scenario")
            .map(String::as_str)
            .unwrap_or("document-tabs")
        {
            "document-tabs" => false,
            "drawing-sheets" => true,
            _ => bail!("--scenario must be document-tabs or drawing-sheets"),
        };
        let mut models = vec![PathBuf::from(required("--model-a")?).canonicalize()?];
        if sheets {
            ensure!(
                !values.contains_key("--model-b"),
                "drawing-sheets uses one --model-a archive; omit --model-b"
            );
        } else {
            models.push(PathBuf::from(required("--model-b")?).canonicalize()?);
        }
        Ok(Self {
            server: PathBuf::from(required("--server")?).canonicalize()?,
            out: PathBuf::from(required("--out")?),
            models,
            sheets,
            commit,
            profile: required("--profile")?,
            cycles,
            instances,
        })
    }
}

fn verify_private_display() -> Result<()> {
    crate::linux_fixture::verify_private_display().map(|_| ())
}
fn hash(path: &Path) -> Result<String> {
    crate::hash::file(path)
}
fn model(client: &mut Client) -> Result<Value> {
    let text = client.call("cad_project_model", json!({}))?;
    serde_json::from_str(text.as_str().context("Project model missing")?)
        .context("Parse project model")
}
fn reattach(client: &mut Client, response: &Value) -> Result<()> {
    let session = response["active_session_id"]
        .as_str()
        .context("Transition has no active session receipt")?;
    client.call("cad_attach", json!({"session_id":session}))?;
    Ok(())
}
fn target(client: &mut Client, matches: impl Fn(&Value) -> bool) -> Result<Value> {
    let observed = ui(client, json!({"action":"inspect"}))?;
    let found = controls(&observed)
        .filter(|c| c["disabled"] == false && matches(c))
        .collect::<Vec<_>>();
    ensure!(
        found.len() == 1,
        "Expected one enabled switching control, found {found:?}"
    );
    Ok(found[0].clone())
}
fn click(client: &mut Client, label: &str) -> Result<Value> {
    let control = target(client, |c| c["label"] == label)?;
    ui(client, json!({"action":"click", "target":control["id"]}))
}
fn proc_sample(pid: u32) -> Value {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    let columns = stat
        .rsplit_once(')')
        .map(|(_, tail)| tail.split_whitespace().collect::<Vec<_>>())
        .unwrap_or_default();
    let number = |index: usize| columns.get(index).and_then(|s| s.parse::<u64>().ok());
    json!({"user_ticks":number(11),"system_ticks":number(12),"rss_pages":number(21),
        "io":fs::read_to_string(format!("/proc/{pid}/io")).ok()})
}
struct Host {
    client: Client,
    labels: [String; 2],
    models: [Value; 2],
    sheet_ids: Option<[u64; 2]>,
}
fn sheet_targets(model: &Value) -> Result<([String; 2], [Value; 2], [u64; 2])> {
    let definitions = model["drawings"]["sheets"]
        .as_array()
        .context("Drawing sheets missing")?;
    ensure!(
        definitions.len() == 2,
        "drawing-sheets requires exactly two sheets"
    );
    let mut ids = [0; 2];
    let mut labels = [String::new(), String::new()];
    for (n, sheet) in definitions.iter().enumerate() {
        ids[n] = sheet["id"].as_u64().context("Sheet identity missing")?;
        let name = sheet["name"].as_str().context("Sheet name missing")?;
        ensure!(
            !name.is_empty()
                && sheet["views"]
                    .as_array()
                    .is_some_and(|views| !views.is_empty()),
            "Both sheets need distinct names and nonempty projected views"
        );
        labels[n] = name.to_owned();
    }
    ensure!(
        ids[0] != ids[1] && labels[0] != labels[1],
        "Sheet identities and names must differ"
    );
    let mut models = [model.clone(), model.clone()];
    for n in 0..2 {
        models[n]["drawings"]["active_sheet_id"] = json!(ids[n]);
    }
    Ok((labels, models, ids))
}
fn launch(options: &Options, index: usize, inputs: &[PathBuf]) -> Result<Host> {
    let directory = options.out.join(format!("instance-{index}"));
    fs::create_dir(&directory)?;
    let sessions = options.out.join("sessions");
    let mut command = Command::new(&options.server);
    command
        .current_dir(&directory)
        .env("LIMO_CAD_SESSION_DIR", &sessions)
        .env("LIMO_CAD_CONFIG_DIR", options.out.join("config"));
    let mut client =
        Client::start_command_logged(command, Some(Duration::from_secs(45)), &directory)?;
    let session = wait_for_owned_window(&mut client, &sessions)?;
    client.call("cad_attach", json!({"session_id":session}))?;
    wait_for_interface(&mut client, &session)?;
    fs::write(
        directory.join("initial-ui.json"),
        serde_json::to_vec_pretty(&ui(&mut client, json!({"action":"inspect"}))?)?,
    )?;
    ensure!(
        client.call("cad_document", json!({}))?["features"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "Owned startup document is not blank"
    );
    let mut labels = [String::new(), String::new()];
    let mut models = [Value::Null, Value::Null];
    for n in 0..inputs.len() {
        if n != 0 {
            let created = click(&mut client, "New design")?;
            reattach(&mut client, &created)?;
        }
        let opened = ui(
            &mut client,
            json!({"action":"file","command":"open","path":inputs[n]}),
        )?;
        reattach(&mut client, &opened)?;
        models[n] = model(&mut client)?;
        ensure!(
            models[n]["document"]["features"]
                .as_array()
                .is_some_and(|f| !f.is_empty())
                || client.call("cad_document", json!({}))?["features"]
                    .as_array()
                    .is_some_and(|f| !f.is_empty()),
            "Switching input needs a real nonempty design"
        );
        let document = client.call("cad_document", json!({}))?;
        labels[n] = document["name"]
            .as_str()
            .context("Loaded document name")?
            .into();
        fs::write(
            directory.join(format!("loaded-{n}.json")),
            serde_json::to_vec_pretty(&models[n])?,
        )?;
    }
    let sheet_ids = if options.sheets {
        let (sheet_labels, expected, ids) = sheet_targets(&models[0])?;
        labels = sheet_labels;
        models = expected;
        click(&mut client, "Switch workspace")?;
        click(&mut client, "Drawing")?;
        fs::write(
            directory.join("drawing-ui.json"),
            serde_json::to_vec_pretty(&ui(&mut client, json!({"action":"inspect"}))?)?,
        )?;
        Some(ids)
    } else {
        None
    };
    ensure!(
        labels[0] != labels[1],
        "Inputs must have distinct document names"
    );
    fs::write(
        directory.join("host.json"),
        serde_json::to_vec_pretty(
            &json!({"pid":client.process_id(),"exe":options.server,"labels":labels}),
        )?,
    )?;
    Ok(Host {
        client,
        labels,
        models,
        sheet_ids,
    })
}

fn measure(
    options: &Options,
    raw: &mut fs::File,
    host: &mut Host,
    instance: usize,
    cycle: usize,
    n: usize,
    warmup: bool,
) -> Result<(f64, f64)> {
    let selected = target(&mut host.client, |c| {
        c["label"].as_str().is_some_and(|label| {
            if options.sheets {
                label.split_whitespace().collect::<String>()
                    == host.labels[n].split_whitespace().collect::<String>()
            } else {
                label == host.labels[n]
            }
        }) && if options.sheets {
            c["role"] == "button"
        } else {
            c["surface"] == "document/session"
        }
    })?;
    let before = proc_sample(host.client.process_id());
    let tree_before = processes::sample(host.client.process_id());
    let started = Instant::now();
    let result = ui(
        &mut host.client,
        json!({"action":"click","target":selected["id"]}),
    );
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.;
    let after = proc_sample(host.client.process_id());
    let tree_after = processes::sample(host.client.process_id());
    let receipt = result.as_ref().map(|value| json!({"status":value["status"],"active_session_id":value["active_session_id"],
        "document_id":value["document_id"],"presented":value["presented"],"render_status":value["render_status"],
        "native_layout_revision":value["native_layout_revision"],"native_submitted_revision":value["native_submitted_revision"]})).unwrap_or(Value::Null);
    writeln!(
        raw,
        "{}",
        json!({"kind":if options.sheets {"sheet"} else {"tab"},"instance":instance,"cycle":cycle,"target":n,
        "sheet_id":host.sheet_ids.map(|ids|ids[n]),"warmup":warmup,
        "elapsed_ms":elapsed_ms,"cpu_before":before,"cpu_after":after,
        "process_tree_before":tree_before,"process_tree_after":tree_after,"control":selected,"receipt":receipt,
        "error":result.as_ref().err().map(|e| format!("{e:#}"))})
    )?;
    raw.flush()?;
    let response = result?;
    reattach(&mut host.client, &response)?;
    let settled = settle::navigation(options, host, &response, instance, cycle, n);
    let settled_elapsed_ms = started.elapsed().as_secs_f64() * 1000.;
    writeln!(
        raw,
        "{}",
        json!({"kind":"navigation-settled","instance":instance,"cycle":cycle,
        "target":n,"warmup":warmup,"elapsed_ms":settled_elapsed_ms,"acknowledgment_ms":elapsed_ms,
        "observation":settled.as_ref().ok(),"error":settled.as_ref().err().map(|e|format!("{e:#}"))})
    )?;
    raw.flush()?;
    settled?;
    Ok((elapsed_ms, settled_elapsed_ms))
}
fn statistics(values: &[f64]) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let percentile =
        |percent: usize| sorted[(sorted.len() * percent).div_ceil(100).saturating_sub(1)];
    let middle = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.
    } else {
        sorted[middle]
    };
    json!({"count":sorted.len(),"minimum_ms":sorted[0],"median_ms":median,"p95_ms":percentile(95),"maximum_ms":sorted[sorted.len()-1]})
}
pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut options = Options::parse(args)?;
    verify_private_display()?;
    ensure!(
        options.out.is_absolute()
            && (!options.out.exists() || fs::read_dir(&options.out)?.next().is_none()),
        "Choose an empty absolute evidence directory"
    );
    fs::create_dir_all(&options.out)?;
    options.out = options.out.canonicalize()?;
    limo_cad_session_storage::create_registry(options.out.join("sessions"))?;
    fs::create_dir(options.out.join("config"))?;
    let inputs = (0..options.models.len())
        .map(|n| {
            options.out.join(if n == 0 {
                "Switch-A.limo"
            } else {
                "Switch-B.limo"
            })
        })
        .collect::<Vec<_>>();
    for (source, destination) in options.models.iter().zip(&inputs) {
        let bytes = fs::read(source)?;
        let _ = crate::project_archive::model(&bytes)?;
        fs::write(destination, bytes)?;
    }
    let metadata = json!({"declared_commit":options.commit,"declared_build_profile":options.profile,"shell":"native",
        "binary_sha256":hash(&options.server)?,"input_sha256":inputs.iter().map(|p|hash(p)).collect::<Result<Vec<_>>>()?,
        "scenario":if options.sheets {"drawing-sheets"} else {"document-tabs"},
        "expected_model_change":if options.sheets {"Only drawings.active_sheet_id becomes the selected sheet ID"} else {"None"},
        "cycles":options.cycles,"instances":options.instances,"warmup_cycles":2,"request_deadline_seconds":45,
        "measurement_budget_seconds":900,
        "post_acknowledgment_observation_budget_seconds":5,
        "requested_x11_scale":std::env::var("WINIT_X11_SCALE_FACTOR").ok(),"display":std::env::var("DISPLAY").ok(),
        "measurement":"semantic click request to application acknowledgment; inspect, attach, model verification and focus requests excluded",
        "navigation_settled_measurement":"request start to exact model and selected sheet observation; includes process sampling, attach, inspect, model reads and bounded observation overhead",
        "foreground_settled_measurement":"one foreground request to matching application and actual owned X11 focus observation; includes observation overhead; no replayed focus request",
        "not_proven":["physical input latency","compositor presentation","GPU time or memory","hardware/monitor DPI","user-reported cause"],
        "comparison_warning":"GPU submission receipts do not prove compositor presentation or physical input latency."});
    fs::write(
        options.out.join("metadata.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    let mut raw = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(options.out.join("samples.jsonl"))?;
    let result = (|| {
        let mut hosts = (0..options.instances)
            .map(|i| launch(&options, i, &inputs))
            .collect::<Result<Vec<_>>>()?;
        let mut samples = vec![Vec::new(); options.instances];
        let mut settled_samples = vec![Vec::new(); options.instances];
        let mut foreground_samples = vec![Vec::new(); options.instances];
        let deadline = Instant::now() + Duration::from_secs(900);
        for cycle in 0..options.cycles + 2 {
            for n in 0..2 {
                for (instance, host) in hosts.iter_mut().enumerate() {
                    ensure!(
                        Instant::now() < deadline,
                        "Switching measurement exceeded its 15-minute budget"
                    );
                    if options.instances == 2 {
                        let before = proc_sample(host.client.process_id());
                        let tree_before = processes::sample(host.client.process_id());
                        let start = Instant::now();
                        let response = ui(
                            &mut host.client,
                            json!({"action":"window","mode":"foreground"}),
                        );
                        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
                        let after = proc_sample(host.client.process_id());
                        let tree_after = processes::sample(host.client.process_id());
                        writeln!(
                            raw,
                            "{}",
                            json!({"kind":"foreground","instance":instance,"cycle":cycle,"target":n,"warmup":cycle<2,
                            "elapsed_ms":elapsed_ms,"cpu_before":before,"cpu_after":after,"process_tree_before":tree_before,"process_tree_after":tree_after,
                            "receipt":response.as_ref().ok(),"error":response.as_ref().err().map(|e|format!("{e:#}"))})
                        )?;
                        raw.flush()?;
                        let foreground = response?;
                        let settled = settle::foreground(&mut host.client, &foreground);
                        let settled_elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
                        writeln!(
                            raw,
                            "{}",
                            json!({"kind":"foreground-settled","instance":instance,"cycle":cycle,
                            "target":n,"warmup":cycle<2,"elapsed_ms":settled_elapsed_ms,"acknowledgment_ms":elapsed_ms,
                            "observation":settled.as_ref().ok(),"error":settled.as_ref().err().map(|e|format!("{e:#}"))})
                        )?;
                        raw.flush()?;
                        settled?;
                        if cycle >= 2 {
                            foreground_samples[instance].push(settled_elapsed_ms);
                        }
                    }
                    let (elapsed, settled_elapsed) =
                        measure(&options, &mut raw, host, instance, cycle, n, cycle < 2)?;
                    if cycle >= 2 {
                        samples[instance].push(elapsed);
                        settled_samples[instance].push(settled_elapsed);
                    }
                }
            }
        }
        Ok::<_, anyhow::Error>(
            json!({"completed":true,"exact_expected_models_preserved":true,"exact_models_preserved":!options.sheets,
            "expected_model_change":if options.sheets {"Only drawings.active_sheet_id"} else {"None"},"performance_acceptance":"not established",
            "statistics":samples.iter().map(|s|statistics(s)).collect::<Vec<_>>(),
            "navigation_settled_statistics":settled_samples.iter().map(|s|statistics(s)).collect::<Vec<_>>(),
            "foreground_settled_statistics":foreground_samples.iter().map(|s|statistics(s)).collect::<Vec<_>>()}),
        )
    })();
    let report = match &result {
        Ok(value) => value.clone(),
        Err(error) => {
            json!({"completed":false,"error":format!("{error:#}"),"performance_acceptance":"not established"})
        }
    };
    fs::write(
        options.out.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    result.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timing_summary_preserves_tail_and_empty_samples() {
        assert!(statistics(&[]).is_null());
        let samples = (1..=20).rev().map(|n| n as f64).collect::<Vec<_>>();
        assert_eq!(
            statistics(&samples),
            json!({"count":20,"minimum_ms":1.,"median_ms":10.5,"p95_ms":19.,"maximum_ms":20.})
        );
    }
    #[test]
    fn sheet_switch_expects_only_the_requested_active_sheet_change() {
        let model = json!({"document":{"name":"Saved part"},"geometry":{"preserve":[1,2,3]},
            "drawings":{"active_sheet_id":7,"settings":{"preserve":"all"},"sheets":[
                {"id":7,"name":"Simple","format":"a4","views":[{"id":3}]},
                {"id":11,"name":"Dense","format":"a3","views":[{"id":4}]}]}});
        let (labels, expected, ids) = sheet_targets(&model).unwrap();
        assert_eq!(ids, [7, 11]);
        assert_eq!(labels, ["Simple", "Dense"]);
        assert_eq!(expected[0], model);
        let mut selected = model.clone();
        selected["drawings"]["active_sheet_id"] = json!(11);
        assert_eq!(expected[1], selected);
        selected["drawings"]["sheets"][1]["views"] = json!([]);
        assert!(sheet_targets(&selected).is_err());
    }
}
