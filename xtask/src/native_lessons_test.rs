//! Open the authored short lesson from native Scripts, run in a new design,
//! and verify playback, editable intent, retained documents, capture and Save.
use crate::native_fixture::{controls, start};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};
mod imported;

/// Compare the packaged inventory to the authored lessons in this harness build.
/// Adding a real lesson must not make the native qualification stop at an old count.
fn installed_lessons(entries: &[Value]) -> Result<Vec<&Value>> {
    let authored = limo_cad_recipes::catalog(false);
    let expected: Vec<_> = authored
        .as_array()
        .context("Authored recipe catalog missing")?
        .iter()
        .filter(|entry| entry["kind"] == "lesson")
        .collect();
    ensure!(
        !expected.is_empty(),
        "Authored short lesson catalog is empty"
    );
    let lessons: Vec<_> = entries
        .iter()
        .filter(|entry| entry["kind"] == "lesson")
        .collect();
    let mut actual_ids = BTreeSet::new();
    for entry in &lessons {
        let id = entry["id"].as_str().context("Lesson ID missing")?;
        ensure!(actual_ids.insert(id), "Duplicate installed lesson {id}");
        let source = expected
            .iter()
            .find(|source| source["id"] == id)
            .with_context(|| format!("Unexpected installed lesson {id}"))?;
        for field in ["name", "chapters", "focus_operations"] {
            ensure!(
                entry[field] == source[field],
                "Installed lesson {id} has different {field} from its authored source"
            );
        }
    }
    let expected_ids: BTreeSet<_> = expected
        .iter()
        .map(|entry| entry["id"].as_str().expect("authored lesson ID"))
        .collect();
    ensure!(
        actual_ids == expected_ids,
        "Installed short lessons differ from the authored catalog: expected {expected_ids:?}, got {actual_ids:?}"
    );
    Ok(lessons)
}

/// Inspection returns an excerpt for long editors. Check that reported range
/// against the authored source without pretending it is the entire buffer.
fn source_excerpt_matches(control: &Value, source: &str) -> Result<()> {
    let value = control["value"]
        .as_str()
        .context("Source excerpt missing")?;
    let units: Vec<_> = source.encode_utf16().collect();
    if units.len() <= 4096 {
        ensure!(
            control["value_truncated"].is_null()
                && control["value_length"].is_null()
                && control["value_start"].is_null()
                && value == source,
            "Untruncated source inspection differs from the authored buffer"
        );
        return Ok(());
    }
    ensure!(
        control["value_truncated"] == true
            && control["value_length"].as_u64() == Some(units.len() as u64),
        "Source excerpt length or truncation metadata differs from the authored buffer"
    );
    let start = usize::try_from(
        control["value_start"]
            .as_u64()
            .context("Source excerpt start missing")?,
    )?;
    // Registry may shift a start forward by one UTF-16 unit if the requested
    // position bisects a surrogate pair. The final excerpt can then be 4095.
    ensure!(
        start <= units.len() - 4096 + 1,
        "Source excerpt start is out of range"
    );
    let mut end = (start + 4096).min(units.len());
    if end < units.len() && (0xd800..=0xdbff).contains(&units[end - 1]) {
        end -= 1;
    }
    let expected = String::from_utf16(&units[start..end])
        .context("Source excerpt starts inside a Unicode scalar")?;
    ensure!(
        value == expected,
        "Reported source excerpt differs from its authored UTF-16 range"
    );
    Ok(())
}

fn ui(client: &mut Client, request: Value) -> Result<Value> {
    let label = json!({"action":request["action"],"command":request["command"],
        "path":request["path"],"target":request["target"]});
    crate::native_fixture::ui(client, request)
        .map_err(|error| anyhow::anyhow!("Native Scripts request {label}: {error:#}"))
}
fn control(client: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    crate::native_fixture::control(client, label, value)
        .map_err(|error| anyhow::anyhow!("Native Scripts control {label:?}: {error:#}"))
}
#[cfg(target_os = "linux")]
fn capture_owned_x11_before_bevy(out: &std::path::Path) -> Result<()> {
    let pid = std::env::var("LIMO_CAD_NATIVE_OWNED_PID")
        .context("LIMO_CAD_NATIVE_OWNED_PID")?
        .parse::<u32>()
        .context("LIMO_CAD_NATIVE_OWNED_PID")?;
    ensure!(pid > 1, "Owned native host PID must be greater than one");
    // Invoke the read-only helper directly; the input wrapper also focuses the window.
    let mut child = std::process::Command::new("python3")
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("platform/native-window-capture-linux.py"),
        )
        .arg(pid.to_string())
        .arg(out)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .context("Start owned X11 window capture")?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                child.wait().context("Reap owned X11 window capture")?;
                ensure!(
                    status.success(),
                    "Owned X11 window capture failed: {status}"
                );
                return Ok(());
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("Owned X11 window capture exceeded 20 seconds");
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("Poll owned X11 window capture");
            }
        }
    }
}

fn capture(client: &mut Client, out: &std::path::Path, name: &str) -> Result<()> {
    let result: Result<()> = (|| {
        let before = ui(client, json!({"action":"inspect"}))?;
        #[cfg(target_os = "linux")]
        if name == "native-lessons"
            && std::env::var("LIMO_CAD_NATIVE_SCRIPT_INPUT").as_deref() == Ok("1")
        {
            capture_owned_x11_before_bevy(out)?;
        }
        let captured = ui(
            client,
            json!({"action":"capture","path":out.join(format!("{name}.png"))}),
        )?;
        std::fs::write(
            out.join(format!("{name}.capture.json")),
            serde_json::to_vec_pretty(&json!({"before_capture":before,"capture":captured}))?,
        )?;
        ensure!(
            captured["presented"] == true && captured["render_status"] == "submitted",
            "Native window capture has no matching submitted frame: {captured}"
        );
        Ok(())
    })();
    result.map_err(|error| anyhow::anyhow!("Native Scripts capture {name:?}: {error:#}"))
}

fn inspect_example_library(c: &mut Client, entries: &[Value]) -> Result<Value> {
    control(c, "Browse examples", None)?;
    // The Scripts workspace retains its last page across tab switches.
    for _ in 0..entries.len() {
        let state = ui(c, json!({"action":"inspect"}))?;
        let previous = controls(&state)
            .find(|row| row["label"] == "Previous examples")
            .context("Previous examples control missing")?;
        if previous["disabled"] == true {
            break;
        }
        control(c, "Previous examples", None)?;
    }
    let first = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&first).any(|row| row["label"] == "Previous examples" && row["disabled"] == true),
        "Example library did not start at its first page"
    );
    let mut seen = BTreeSet::new();
    let mut pages = 0;
    loop {
        let state = ui(c, json!({"action":"inspect"}))?;
        for entry in entries {
            let name = entry["name"].as_str().context("Recipe title missing")?;
            let matching: Vec<_> = controls(&state)
                .filter(|row| row["label"] == name)
                .collect();
            ensure!(matching.len() <= 1, "Duplicate example control {name}");
            if let Some(row) = matching.first() {
                ensure!(row["disabled"] == false, "Example {name} is disabled");
                ensure!(
                    seen.insert(name.to_owned()),
                    "Example {name} appears on multiple pages"
                );
            }
        }
        pages += 1;
        ensure!(pages <= entries.len(), "Example library repeated a page");
        let next = controls(&state)
            .find(|row| {
                row["label"]
                    .as_str()
                    .is_some_and(|label| label.starts_with("Next examples ("))
            })
            .context("Next examples control missing")?;
        if next["disabled"] == true {
            break;
        }
        control(
            c,
            next["label"].as_str().context("Next examples label")?,
            None,
        )?;
    }
    let expected: BTreeSet<_> = entries
        .iter()
        .map(|entry| {
            entry["name"]
                .as_str()
                .expect("checked recipe title")
                .to_owned()
        })
        .collect();
    ensure!(
        seen == expected,
        "Example library omitted authored recipes: expected {expected:?}, got {seen:?}"
    );
    // Return to the first page: the first-part lesson is the Start here example.
    for _ in 0..pages {
        let state = ui(c, json!({"action":"inspect"}))?;
        let previous = controls(&state)
            .find(|row| row["label"] == "Previous examples")
            .context("Previous examples control missing")?;
        if previous["disabled"] == true {
            return Ok(json!({"pages":pages,"titles":seen,"first_page":state}));
        }
        control(c, "Previous examples", None)?;
    }
    anyhow::bail!("Example library did not return to its first page")
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-lessons")?;
    let c = &mut fixture.client;
    let catalog = c.call("cad_interface", json!({"action":"recipes"}))?;
    let entries = catalog.as_array().context("Recipe catalog missing")?;
    let lessons = installed_lessons(entries)?;
    let lesson = lessons
        .iter()
        .find(|entry| entry["id"] == "fillet-basics")
        .context("Fillet lesson missing")?;
    let title = lesson["name"].as_str().context("Lesson title missing")?;
    ui(
        c,
        json!({"action":"file","command":"rename","name":"Lesson launch baseline"}),
    )?;
    let before = c.call("cad_project_model", json!({}))?;
    control(c, "Scripts", None)?;
    let library = inspect_example_library(c, entries)?;
    ensure!(
        c.call("cad_project_model", json!({}))? == before,
        "Opening Scripts changed the blank document"
    );
    capture(c, &fixture.out, "lessons-blank-catalog")?;
    control(c, title, None)?;
    let source = imported::wait_source(c, "(fillet-basics)")?;
    let source_control = controls(&source)
        .find(|row| row["label"] == "Authored script source")
        .context("Authored source editor missing")?;
    source_excerpt_matches(
        source_control,
        limo_cad_recipes::find("fillet-basics")
            .map_err(anyhow::Error::msg)?
            .source,
    )?;
    ensure!(
        c.call("cad_project_model", json!({}))? == before,
        "Opening the first-part source changed the design"
    );
    // The owned fixture may have retained launch preferences. Use real controls
    // to choose Present and the slowest rate before testing the first caption.
    let mode = control(c, "Script run mode", None)?;
    if mode["value"]["mode"] != "present" {
        let mode = control(c, "Script run mode", None)?;
        ensure!(
            mode["value"]["mode"] == "present",
            "Present launch mode unavailable"
        );
    }
    for _ in 0..7 {
        let state = ui(c, json!({"action":"inspect"}))?;
        if controls(&state)
            .any(|row| row["label"] == "Slower script presentation" && row["disabled"] == true)
        {
            break;
        }
        control(c, "Slower script presentation", None)?;
    }
    let launch = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&launch)
            .any(|row| row["label"] == "Slower script presentation" && row["disabled"] == true),
        "Lesson launch rate did not reach the supported minimum"
    );
    let started = control(c, "Run in new design", None)?;
    ensure!(
        started["value"]["script_started"]["name"] == title
            && started["value"]["script_error"].is_null(),
        "Run in new design did not start the inspected lesson: {started}"
    );
    let lesson_session = started["active_session_id"]
        .as_str()
        .context("New lesson session missing")?
        .to_owned();
    ensure!(
        lesson_session != fixture.session,
        "Lesson reused the original document"
    );
    c.call("cad_attach", json!({"session_id":lesson_session}))?;
    ui(c, json!({"action":"presentation","command":"pause"}))?;
    let caption_deadline = Instant::now() + Duration::from_secs(20);
    let paused = loop {
        let response = ui(c, json!({"action":"presentation","command":"status"}))?;
        let state = &response["presentation"];
        ensure!(
            state["stopped"] != true,
            "Lesson stopped before its first caption: {state}"
        );
        if state["chapter"] == "Locate the stock" {
            ensure!(
                state["paused"] == true && state["step_index"] == 1,
                "Lesson did not pause at its first caption: {state}"
            );
            break state.clone();
        }
        ensure!(
            Instant::now() < caption_deadline,
            "First lesson caption did not arrive: {state}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let blank = c.call("cad_project_model", json!({}))?;
    let blank_model: Value =
        serde_json::from_str(blank.as_str().context("Paused lesson model export")?)?;
    ensure!(
        blank_model["document"]["history"]["features"]
            .as_array()
            .is_some_and(Vec::is_empty)
            && c.call("sketch_active", json!({}))?.is_null(),
        "The new lesson design was not blank at its first paused caption"
    );
    std::thread::sleep(Duration::from_millis(600));
    let still_paused = ui(c, json!({"action":"presentation","command":"status"}))?;
    ensure!(
        still_paused["presentation"]["paused"] == true
            && still_paused["presentation"]["step_index"] == paused["step_index"]
            && still_paused["presentation"]["wait_ms"] == paused["wait_ms"],
        "Paused caption advanced: {still_paused}"
    );
    ensure!(
        c.call("cad_project_model", json!({}))? == blank,
        "The real lesson edited the blank document while paused"
    );
    capture(c, &fixture.out, "lessons-paused-caption")?;

    ui(c, json!({"action":"presentation","command":"step"}))?;
    let step_deadline = Instant::now() + Duration::from_secs(30);
    let stepped = loop {
        let response = ui(c, json!({"action":"presentation","command":"status"}))?;
        let state = &response["presentation"];
        ensure!(
            state["stopped"] != true,
            "Single-step stopped the lesson: {state}"
        );
        if state["step_pending"] == false && state["operation"] == "begin" {
            ensure!(
                state["paused"] == true,
                "Single-step resumed the entire lesson: {state}"
            );
            break state.clone();
        }
        ensure!(
            Instant::now() < step_deadline,
            "Single-step did not apply exactly sketch_begin: {state}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let stepped_model = c.call("cad_project_model", json!({}))?;
    let stepped_active = c.call("sketch_active", json!({}))?;
    ensure!(
        stepped_model == blank && !stepped_active.is_null(),
        "Single-step did not begin the actual editable sketch"
    );
    std::thread::sleep(Duration::from_millis(600));
    let held = ui(c, json!({"action":"presentation","command":"status"}))?;
    ensure!(
        held["presentation"]["paused"] == true
            && held["presentation"]["step_index"] == stepped["step_index"]
            && held["presentation"]["operation"] == stepped["operation"]
            && c.call("sketch_active", json!({}))? == stepped_active
            && c.call("cad_project_model", json!({}))? == stepped_model,
        "Single-step allowed a second modeling operation: {held}"
    );
    control(c, "Resume", None)?;
    let deadline = Instant::now() + Duration::from_secs(180);
    let presentation = loop {
        let response = ui(c, json!({"action":"presentation","command":"status"}))?;
        let state = &response["presentation"];
        ensure!(
            state["stopped"] != true,
            "Lesson presentation stopped: {state}"
        );
        if state["finished"] == true && state["chapter"] == "Three editable features" {
            break state.clone();
        }
        ensure!(Instant::now() < deadline, "Lesson did not finish: {state}");
        std::thread::sleep(Duration::from_millis(100));
    };
    let scene = c.call("solid_scene", json!({}))?;
    ensure!(
        scene["errors"].as_array().is_some_and(Vec::is_empty),
        "Lesson left solid errors"
    );
    ensure!(
        scene["bodies"]
            .as_array()
            .is_some_and(|rows| rows.len() == 1),
        "Lesson did not create one real solid"
    );
    let sketches = c.call("sketch_finished", json!({}))?;
    ensure!(
        sketches.as_array().is_some_and(|rows| rows.len() == 1) && sketches[0]["dof"]["value"] == 0,
        "Lesson did not keep its fully constrained sketch"
    );
    ui(
        c,
        json!({"action":"file","command":"rename","name":"Completed fillet lesson"}),
    )?;
    let exported = c.call("cad_project_model", json!({}))?;
    let model: Value = serde_json::from_str(exported.as_str().context("Model export missing")?)?;
    ensure!(
        model["document"]["history"]["features"]
            .as_array()
            .is_some_and(|rows| rows.len() == 3),
        "Lesson lost its three editable features"
    );
    ensure!(
        model["extrudes"][0]["extent"]["distance"] == 12. && model["fillets"][0]["radius"] == 2.,
        "Lesson did not restore its 12 mm extrusion and 2 mm fillet"
    );
    control(c, "Scripts", None)?;
    ui(c, json!({"action":"open_recipe","recipe":"fillet-basics"}))?;
    let nonblank_source = imported::wait_source(c, "(fillet-basics)")?;
    ensure!(
        controls(&nonblank_source)
            .any(|row| row["label"] == "Run in new design" && row["disabled"] == false),
        "Nonblank design cannot explicitly launch a separate new design"
    );
    ensure!(
        c.call("cad_project_model", json!({}))? == exported,
        "Opening source replaced or edited the completed design"
    );
    capture(c, &fixture.out, "lessons-nonblank-source")?;
    control(c, "Scripts", None)?;
    let restored = control(c, "Lesson launch baseline", None)?;
    ensure!(
        restored["active_session_id"] == fixture.session,
        "Original lesson tab ownership changed"
    );
    c.call(
        "cad_attach",
        json!({"session_id":restored["active_session_id"]}),
    )?;
    ensure!(
        c.call("cad_project_model", json!({}))? == before,
        "Lesson changed the retained original design"
    );
    let completed = control(c, "Completed fillet lesson", None)?;
    ensure!(
        completed["active_session_id"] == lesson_session,
        "Completed lesson tab ownership changed"
    );
    c.call(
        "cad_attach",
        json!({"session_id":completed["active_session_id"]}),
    )?;
    ensure!(
        c.call("cad_project_model", json!({}))? == exported,
        "Restoring the completed lesson changed its model"
    );
    capture(c, &fixture.out, "native-lessons")?;
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&fixture.project)?)?;
    let saved: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    ensure!(saved == model, "Save lost the lesson's editable model");
    for name in [
        "lessons-blank-catalog.png",
        "lessons-paused-caption.png",
        "lessons-nonblank-source.png",
        "native-lessons.png",
    ] {
        let png = std::fs::read(fixture.out.join(name))?;
        ensure!(
            png.starts_with(b"\x89PNG\r\n\x1a\n") && png.len() > 1024,
            "Live window capture {name} missing or empty"
        );
    }
    let imported = imported::exercise(c, &fixture.out)?;
    std::fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&json!({
            "state_checks_passed":true,"pixel_review":"required","session":lesson_session,
            "original_session":fixture.session,"original_model":before,"library":library,"source":source,"launch":launch,
            "lesson":"fillet-basics","installed_lessons":lessons.iter().map(|entry| &entry["id"]).collect::<Vec<_>>(),
            "presentation":presentation,"paused":paused,"stepped":stepped,
            "stepped_active_sketch":stepped_active,"model":model,
            "scene":scene,"sketches":sketches,"imported_script":imported,
            "checks":["authored-short-lessons-only","paginated-example-library","open-source-does-not-run","explicit-new-design",
                "shared-runner-presentation","pause-holds-caption-and-blank-model",
                "single-step-one-owned-model-operation","native-resume-button","editable-solid-and-fully-constrained-sketch",
                "nonblank-source-preserves-completed-model","retained-original-tab-unchanged",
                "restored-completed-tab-unchanged","live-window-captures","saved-model-equality"],
        }))?,
    )?;
    println!(
        "PASS native Scripts lesson, editable model, retained documents, window capture and Save"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_inspection_matches_utf16_excerpt_and_rejects_wrong_buffer() {
        let source = format!("{}{}", "😀".repeat(1024), "a".repeat(4096));
        let excerpt = json!({"value":format!("😀{}", "a".repeat(4094)),
            "value_truncated":true,"value_length":6144,"value_start":2046});
        source_excerpt_matches(&excerpt, &source).unwrap();
        for (field, value) in [
            ("value_start", json!(2047)),
            ("value_length", json!(6143)),
            ("value_truncated", json!(false)),
            ("value", json!("another source")),
        ] {
            let mut wrong = excerpt.clone();
            wrong[field] = value;
            assert!(
                source_excerpt_matches(&wrong, &source).is_err(),
                "accepted wrong {field}"
            );
        }
        source_excerpt_matches(&json!({"value":"short source"}), "short source").unwrap();
        assert!(source_excerpt_matches(&json!({"value":"wrong source"}), "short source").is_err());

        let authored = limo_cad_recipes::find("fillet-basics").unwrap().source;
        let units: Vec<_> = authored.encode_utf16().collect();
        assert!(
            units.len() > 4096,
            "fixture must exercise the real long editor contract"
        );
        let actual_range = json!({"value":String::from_utf16(&units[..4096]).unwrap(),
            "value_truncated":true,"value_length":units.len(),"value_start":0});
        source_excerpt_matches(&actual_range, authored).unwrap();
    }

    #[test]
    fn installed_lesson_catalog_preserves_recovery_and_rejects_bad_inventory() {
        // Catalog inspection performs source preflight, not a lesson replay or
        // native model construction. This catches the former four-lesson gate.
        let catalog = limo_cad_recipes::catalog(false);
        let entries = catalog.as_array().unwrap();
        let lessons = installed_lessons(entries).unwrap();
        assert!(lessons
            .iter()
            .any(|entry| entry["id"] == "component-edit-recovery"));

        let missing = entries
            .iter()
            .filter(|entry| entry["id"] != "component-edit-recovery")
            .cloned()
            .collect::<Vec<_>>();
        assert!(installed_lessons(&missing)
            .unwrap_err()
            .to_string()
            .contains("differ"));

        let mut duplicate = entries.clone();
        duplicate.push((*lessons[0]).clone());
        assert!(installed_lessons(&duplicate)
            .unwrap_err()
            .to_string()
            .contains("Duplicate"));

        let mut stale = entries.clone();
        let recovery = stale
            .iter_mut()
            .find(|entry| entry["id"] == "component-edit-recovery")
            .unwrap();
        recovery["chapters"] = json!([]);
        assert!(installed_lessons(&stale)
            .unwrap_err()
            .to_string()
            .contains("chapters"));
    }
}
