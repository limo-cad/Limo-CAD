//! Real desktop regression for the shared playback controls. This fixture
//! attaches to an explicitly named existing window and never launches one.
use crate::replay::Client;
use anyhow::{anyhow, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub fn run(args: &[String]) -> Result<(), String> {
    run_inner(args).map_err(|error| format!("{error:#}"))
}

fn ui(client: &mut Client, args: Value) -> Result<Value> {
    let result = client.call("cad_interface", args)?;
    ensure!(
        result["status"] == "applied",
        "Interface request did not apply: {result}"
    );
    Ok(result)
}

fn status(client: &mut Client) -> Result<Value> {
    Ok(ui(client, json!({"action":"presentation","command":"status"}))?["presentation"].clone())
}

fn control(client: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    // A different client may inspect while the script is running. Explicit
    // stale-ID rejection is safe to retry; successful clicks are never retried.
    for attempt in 0..5 {
        let inspected = ui(client, json!({"action":"inspect"}))?;
        let controls = inspected["ui"]["surfaces"]
            .as_array()
            .context("Missing UI surfaces")?
            .iter()
            .flat_map(|surface| surface["controls"].as_array().into_iter().flatten())
            .filter(|control| control["label"] == label && control["disabled"] == false)
            .collect::<Vec<_>>();
        ensure!(
            controls.len() == 1,
            "Expected one enabled {label} control, got {controls:?}"
        );
        let target = controls[0]["id"].as_str().context("Control has no ID")?;
        let request = if let Some(value) = value {
            json!({"action":"set_value","target":target,"value":value})
        } else {
            json!({"action":"click","target":target})
        };
        match ui(client, request) {
            Err(error) if attempt < 4 && error.to_string().contains("stale") => continue,
            result => return result,
        }
    }
    unreachable!()
}

fn new_design(client: &mut Client) -> Result<String> {
    let reply = control(client, "New design", None)?;
    let session = reply["active_session_id"]
        .as_str()
        .context("New design did not return its session")?
        .to_owned();
    client.call("cad_attach", json!({"session_id":session}))?;
    Ok(session)
}

fn active_sketch(client: &mut Client) -> Result<Value> {
    // The live read endpoint returns the active sketch rather than the last
    // completed project snapshot; a paused in-progress sketch is intentional.
    client.call("sketch_active", json!({}))
}

fn model(client: &mut Client) -> Result<Value> {
    client.call("cad_refresh", json!({}))?;
    let value = client.call("cad_project_model", json!({}))?;
    if let Some(text) = value.as_str() {
        Ok(serde_json::from_str(text)?)
    } else {
        Ok(value)
    }
}

fn wait_until<T>(description: &str, mut read: impl FnMut() -> Result<Option<T>>) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(value) = read()? {
            return Ok(value);
        }
        ensure!(
            Instant::now() < deadline,
            "Timed out waiting for {description}"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

fn save(client: &mut Client, path: &Path) -> Result<()> {
    ui(
        client,
        json!({"action":"file","command":"save","path":path}),
    )?;
    ensure!(
        path.is_file(),
        "Native project was not saved: {}",
        path.display()
    );
    Ok(())
}

fn run_inner(args: &[String]) -> Result<()> {
    let mut options = HashMap::new();
    let mut args = args.iter();
    while let Some(key) = args.next() {
        ensure!(
            ["--server", "--session", "--out"].contains(&key.as_str()),
            "Unknown option {key}"
        );
        let value = args
            .next()
            .with_context(|| format!("Missing value for {key}"))?;
        ensure!(
            options.insert(key.as_str(), value.as_str()).is_none(),
            "Duplicate option {key}"
        );
    }
    let server = options
        .get("--server")
        .context("Use --server PATH to the matching MCP binary")?
        .to_string();
    let original_session = options
        .get("--session")
        .context("Use --session ID for the existing desktop")?
        .to_string();
    let out = options
        .get("--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("nbcad-playback-proof"));
    fs::create_dir_all(&out)?;
    let out = fs::canonicalize(out)?;
    // Native save accepts ordinary absolute paths; remove Windows verbatim
    // path syntax only after canonicalization (no filesystem deletion here).
    let out = PathBuf::from(
        out.to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string(),
    );
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let mut client = Client::start(&server)?;
    client.call("cad_attach", json!({"session_id":original_session}))?;
    let original_model = model(&mut client)?;
    let original_active = active_sketch(&mut client)?;
    fs::write(
        out.join(format!("original-{stamp}-model.json")),
        serde_json::to_vec_pretty(&original_model)?,
    )?;
    fs::write(
        out.join(format!("original-{stamp}-active-sketch.json")),
        serde_json::to_vec_pretty(&original_active)?,
    )?;
    ensure!(original_active.is_null(), "Original window has an in-progress sketch; preserved it for review without changing the window");
    save(&mut client, &out.join(format!("original-{stamp}.nbcad")))?;

    let session = new_design(&mut client)?;
    fs::write(out.join("active-session.txt"), &session)?;
    let chapter = format!("Live playback regression {stamp}");
    let source = json!({"version":1,"name":"Live playback control regression","steps":[
        {"chapter":chapter,"note":"Pause here, then Step begins only the sketch. Another Step draws its rectangle. Resume finishes the sketch.","duration_ms":10000},
        {"id":"begin","call":{"group":"sketch/draw","operation":"sketch_begin","arguments":{"name":"Playback rectangle","plane":{"type":"origin_plane","plane":"xy"}}}},
        {"id":"rectangle","call":{"group":"sketch/draw","operation":"sketch_add_rectangle_locked","arguments":{"mode":"two_point","anchor":{"x":0,"y":0},"corner_hint":{"x":20,"y":10},"width_mm":20,"height_mm":10,"ctrl_held":true}}},
        {"id":"finish","call":{"group":"sketch/draw","operation":"sketch_finish","arguments":{}}},
        {"chapter":"Playback regression complete","note":"The rectangle is complete. Pause, single-step, speed and resume used the same controls as MCP.","duration_ms":0}
    ],"checks":[{"id":"finished","call":{"group":"sketch/draw","operation":"sketch_finished","arguments":{}}}],"exports":{"finished":{"$ref":"finished"}}}).to_string();
    let worker_server = server.clone();
    let worker_session = session.clone();
    let (send, receive) = mpsc::channel();
    let worker = thread::spawn(move || {
        let result = (|| {
            let mut worker = Client::start(&worker_server)?;
            worker.call("cad_attach", json!({"session_id":worker_session}))?;
            worker.call("cad_interface", json!({"action":"script","source":source,"mode":"present","speed":1,"validate":true}))
        })();
        let _ = send.send(result);
    });

    let stepped = (|| -> Result<Value> {
        wait_until("the authored caption", || {
            let state = status(&mut client)?;
            Ok((state["chapter"] == chapter && state["stopped"] == false).then_some(state))
        })?;
        ui(
            &mut client,
            json!({"action":"presentation","command":"pause"}),
        )?;
        control(&mut client, "Presentation speed", Some("4"))?;
        let paused = status(&mut client)?;
        ensure!(
            paused["paused"] == true && paused["speed"] == 4,
            "Speed change lost the pause: {paused}"
        );
        ensure!(
            active_sketch(&mut client)?.is_null(),
            "A mutation ran during the paused caption"
        );

        control(
            &mut client,
            "Apply one modeling operation, then remain paused",
            None,
        )?;
        let empty = wait_until("one sketch-begin mutation", || {
            let sketch = active_sketch(&mut client)?;
            Ok((!sketch.is_null()).then_some(sketch))
        })?;
        ensure!(
            empty["entities"].as_array().is_some_and(Vec::is_empty),
            "Step also ran the rectangle: {empty}"
        );
        thread::sleep(Duration::from_millis(600));
        ensure!(
            active_sketch(&mut client)? == empty,
            "The paused model advanced without a second step"
        );
        ensure!(
            status(&mut client)?["paused"] == true,
            "Single-step must remain paused"
        );

        control(
            &mut client,
            "Apply one modeling operation, then remain paused",
            None,
        )?;
        let rectangle = wait_until("one rectangle mutation", || {
            let sketch = active_sketch(&mut client)?;
            Ok(sketch["entities"]
                .as_array()
                .is_some_and(|entities| !entities.is_empty())
                .then_some(sketch))
        })?;
        thread::sleep(Duration::from_millis(600));
        ensure!(
            active_sketch(&mut client)? == rectangle,
            "Second step also finished the sketch"
        );
        control(&mut client, "Resume", None)?;
        let report = receive
            .recv_timeout(Duration::from_secs(20))
            .context("Script did not finish after Resume")??;
        ensure!(
            report
                .pointer("/exports/finished")
                .and_then(Value::as_array)
                .is_some_and(|rows| rows.len() == 1),
            "Unexpected completed sketch result: {report}"
        );
        ensure!(
            status(&mut client)?["finished"] == true,
            "Successful script did not finish its presentation"
        );
        Ok(report)
    })();
    if stepped.is_err() {
        let _ = ui(
            &mut client,
            json!({"action":"presentation","command":"stop","text":"Playback regression stopped after a failed assertion; current work is retained."}),
        );
        let _ = receive.recv_timeout(Duration::from_secs(5));
    }
    worker
        .join()
        .map_err(|_| anyhow!("Script worker panicked"))?;
    let stepped = stepped?;
    save(
        &mut client,
        &out.join(format!("completed-rectangle-{stamp}.nbcad")),
    )?;
    println!(
        "PASS live caption, native speed control, pause, exactly-one-operation Step, and Resume"
    );

    let final_session = new_design(&mut client)?;
    ui(
        &mut client,
        json!({"action":"presentation","command":"configure","mode":"present","speed":1}),
    )?;
    control(&mut client, "Pause", None)?;
    let before = model(&mut client)?;
    let attach = client.call("cad_attach", json!({"session_id":final_session}))?;
    let generation = attach
        .pointer("/heartbeat/generation")
        .and_then(Value::as_u64)
        .context("Missing current generation")?;
    let submitted = client.call("cad_submit", json!({"name":"sketch_begin","arguments":{"name":"Must never begin","plane":{"type":"origin_plane","plane":"xy"}},"base_generation":generation}))?;
    let seq = submitted["seq"]
        .as_u64()
        .context("Submission has no sequence")?;
    let pending = client.call(
        "cad_await_apply",
        json!({"seq":seq,"timeout_ms":0,"refresh":false}),
    )?;
    ensure!(
        pending["status"] == "timeout" || pending["status"] == "pending",
        "Paused submission was not pending: {pending}"
    );
    control(&mut client, "Stop", None)?;
    let error = client
        .call(
            "cad_await_apply",
            json!({"seq":seq,"timeout_ms":5000,"refresh":false}),
        )
        .expect_err("Stopped submission must fail");
    ensure!(
        error.to_string().contains("Playback stopped"),
        "Wrong stop failure: {error}"
    );
    ensure!(
        status(&mut client)?["stopped"] == true,
        "Native Stop did not update MCP status"
    );
    ensure!(
        model(&mut client)? == before && active_sketch(&mut client)?.is_null(),
        "Stopped queued command changed the model"
    );

    ui(
        &mut client,
        json!({"action":"presentation","command":"configure","mode":"present","speed":1}),
    )?;
    ui(
        &mut client,
        json!({"action":"presentation","command":"note","text":"Maximum speed skips this authored hold.","duration_ms":10000}),
    )?;
    control(&mut client, "Presentation speed", Some("fast"))?;
    let maximum = status(&mut client)?;
    ensure!(
        maximum["mode"] == "fast" && maximum["wait_ms"] == 0,
        "Maximum speed retained a presentation delay: {maximum}"
    );
    ui(
        &mut client,
        json!({"action":"presentation","command":"finish","text":"Playback controls passed. This blank document is ready for the bench replay."}),
    )?;
    let report = json!({"passed":true,"original_session_id":original_session,"test_session_id":session,
        "final_blank_session_id":final_session,"cases":["caption","native-speed","pause","native-step-one-mutation","native-resume","native-stop-queued","maximum-no-delay"],
        "script":stepped,"stopped_sequence":seq,"stop_failure":error.to_string(),"maximum":maximum});
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("PASS queued Stop preserved the blank model; Maximum skipped authored delays");
    println!("Final blank session: {final_session}");
    println!("Proof and preserved native documents: {}", out.display());
    Ok(())
}
