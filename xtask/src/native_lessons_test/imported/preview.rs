//! The same product PNG preview is inspected over MCP; no desktop capture.
use super::*;

fn wait_frame(c: &mut Client, index: usize) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(90);
    let expected = format!("Lesson preview model: {index}/");
    loop {
        let state = ui(c, json!({"action":"inspect"}))?;
        if controls(&state).any(|row| {
            row["label"]
                .as_str()
                .is_some_and(|label| label.starts_with(&expected))
        }) {
            return Ok(state);
        }
        ensure!(
            Instant::now() < deadline,
            "Native lesson preview pixels did not arrive for frame {index}: {state}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub(super) fn exercise(c: &mut Client, out: &Path, model: &Value) -> Result<Value> {
    control(c, "Preview lesson", None)?;
    let first = wait_frame(c, 1)?;
    capture(c, out, "scripts-preview-first")?;
    control(c, "Next preview frame", None)?;
    let second = wait_frame(c, 2)?;
    capture(c, out, "scripts-preview-second")?;
    let input_state = ui(c, json!({"action":"inspect"}))?;
    let target = controls(&input_state)
        .find(|row| {
            row["surface"] == "document/scripts"
                && row["disabled"] == false
                && row["label"]
                    .as_str()
                    .is_some_and(|label| label.starts_with("Lesson preview model: 2/"))
        })
        .context("Preview model control missing")?["id"]
        .clone();
    ui(
        c,
        json!({"action":"key","target":target,"key":"ArrowRight"}),
    )?;
    wait_frame(c, 2)?;
    capture(c, out, "scripts-preview-turned")?;
    control(c, "Fit preview", None)?;
    wait_frame(c, 2)?;
    capture(c, out, "scripts-preview-fit")?;
    control(c, "Previous preview frame", None)?;
    let restored = wait_frame(c, 1)?;
    capture(c, out, "scripts-preview-restored")?;
    ensure!(
        c.call("cad_project_model", json!({}))? == *model,
        "Isolated preview or its camera changed the retained design"
    );
    control(c, "Back to script source", None)?;
    let source = wait_source(c, "(fillet-basics)")?;
    ensure!(
        c.call("cad_project_model", json!({}))? == *model,
        "Closing the preview changed the retained design"
    );
    Ok(
        json!({"first":first,"second":second,"restored":restored,"source_after_close":source,
        "state_checks_passed":true,"pixel_review":"required",
        "not_proven":["Physical preview drag and arrow keys","Preview reduced-motion preference"]}),
    )
}
