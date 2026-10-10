//! Real input only, through the existing verified fixture-owned window helper.
use super::super::cloud;
use super::chamfer_input::pointer;
use super::*;

pub(in super::super) fn exercise(
    c: &mut Client,
    out: &Path,
    server: &str,
    baseline: &Value,
) -> Result<Value> {
    ensure!(
        cfg!(target_os = "windows") || cfg!(target_os = "linux"),
        "Cloud OS proof requires owned Windows or disposable private Linux Xvfb"
    );
    let initial = inspect(c)?;
    let session = initial["active_session_id"]
        .as_str()
        .context("Owned cloud session")?;
    let driver = Driver::new(owned_pid(out, session, server)?, out)?;
    control(c, "Fit sheet", None)?;
    let paper = Paper::fitted(&inspect(c)?)?;
    let loaded = annotations(baseline)?
        .iter()
        .find(|a| a["id"] == 2)
        .context("Loaded seven-vertex cloud")?;
    let start = center(&inspect(c)?, "Edit annotation 2")?;
    let end = [start[0] + paper.scale * 8., start[1] + paper.scale * 5.];
    let evidence = pointer(&driver, c, out, "cloud-os-loaded-drag", start, end, false)?;
    let loaded_dragged = check_drag(c, out, "loaded", baseline, loaded, &paper, &evidence)?;
    history(c, baseline, &loaded_dragged)?;
    capture(c, out, "cloud-os-loaded-dragged")?;
    curved::save_exact(c, out, "cloud-os-loaded-dragged", &loaded_dragged)?;
    control(c, "Undo", None)?;
    ensure!(
        &model(c)? == baseline,
        "Loaded-cloud drag did not restore the exact issued baseline"
    );
    cloud::open(c)?;
    let point = paper.screen([145., 45.]);
    pointer(&driver, c, out, "cloud-os-cancel-stage", point, point, true)?;
    ensure!(
        &observed_model(c)? == baseline,
        "Escape committed cloud staging or consumed an ID"
    );
    let mut reports = Vec::new();
    for (name, corners) in [
        ("triangle", vec![[145., 45.], [210., 45.], [175., 95.]]),
        (
            "quad",
            vec![[145., 115.], [205., 115.], [210., 160.], [150., 155.]],
        ),
    ] {
        cloud::open(c)?;
        let mut actual_points = Vec::new();
        for (index, point) in corners.iter().enumerate() {
            let screen = paper.screen(*point);
            let evidence = pointer(
                &driver,
                c,
                out,
                &format!("cloud-os-{name}-point-{index}"),
                screen,
                screen,
                false,
            )?;
            actual_points.push(paper.paper(observed_point(&evidence, "logical_start")?));
            if index < 3 {
                ensure!(
                    &observed_model(c)? == baseline,
                    "Partial cloud {name} mutated document or counter"
                );
                if index == 2 {
                    capture(c, out, &format!("cloud-os-{name}-staged"))?;
                }
            }
        }
        if name == "triangle" {
            let closing = paper.screen([actual_points[0][0] + 1., actual_points[0][1]]);
            let evidence = pointer(
                &driver,
                c,
                out,
                "cloud-os-triangle-close",
                closing,
                closing,
                false,
            )?;
            let observed = paper.paper(observed_point(&evidence, "logical_start")?);
            ensure!(
                (observed[0] - actual_points[0][0]).hypot(observed[1] - actual_points[0][1]) <= 4.,
                "OS closing click was not within four paper millimetres"
            );
        }
        let created = changed_model(c, baseline, out, &format!("cloud-os-{name}-create"))?;
        let annotation =
            exact_one_added(baseline, &created, out, &format!("cloud-os-{name}-created"))?;
        ensure!(
            annotation["kind"] == "revision_cloud" && annotation["revision"] == "c\u{03c9}",
            "Created cloud lost the sheet revision or selected the wrong family"
        );
        let vertices = annotation["points"]
            .as_array()
            .context("Created cloud vertices")?;
        ensure!(
            vertices.len() == corners.len(),
            "Closing click was incorrectly saved as another vertex"
        );
        for (actual, expected) in vertices.iter().zip(&actual_points) {
            close_point(actual, *expected, "Cloud paper vertex")?;
        }
        let id = annotation["id"].as_u64().context("Cloud ID")?;
        history(c, baseline, &created)?;
        capture(c, out, &format!("cloud-os-{name}-created"))?;

        let a = actual_points[0];
        let b = actual_points[1];
        let direction = [b[0] - a[0], b[1] - a[1]];
        let length = direction[0].hypot(direction[1]);
        let direction = direction.map(|v| v / length);
        let count = (length / 5.).ceil();
        let step = length / count;
        let radius = (step * 0.58).max(1.4);
        let sagitta = radius - (radius * radius - step * step * 0.25).sqrt();
        let along = (count * 0.5).floor() * step + step * 0.5;
        let stroke = paper.screen([
            a[0] + direction[0] * along + direction[1] * sagitta,
            a[1] + direction[1] * along - direction[0] * sagitta,
        ]);
        let end = [stroke[0] + paper.scale * 7., stroke[1] + paper.scale * 6.];
        pointer(
            &driver,
            c,
            out,
            &format!("cloud-os-{name}-drag-cancel"),
            stroke,
            end,
            true,
        )?;
        ensure!(
            observed_model(c)? == created,
            "Escape committed a partial cloud drag"
        );
        let evidence = pointer(
            &driver,
            c,
            out,
            &format!("cloud-os-{name}-path-drag"),
            stroke,
            end,
            false,
        )?;
        let dragged = check_drag(c, out, name, &created, &annotation, &paper, &evidence)?;
        history(c, &created, &dragged)?;
        capture(c, out, &format!("cloud-os-{name}-dragged"))?;
        curved::save_exact(c, out, &format!("cloud-os-{name}-dragged"), &dragged)?;

        let label = center(&inspect(c)?, &format!("Edit annotation {id}"))?;
        let label_end = [label[0] + paper.scale * 3., label[1] - paper.scale * 4.];
        let moved = pointer(
            &driver,
            c,
            out,
            &format!("cloud-os-{name}-label-drag"),
            label,
            label_end,
            false,
        )?;
        let current_annotation = annotations(&dragged)?
            .iter()
            .find(|a| a["id"] == id)
            .context("Dragged cloud")?;
        let label_dragged = check_drag(
            c,
            out,
            &format!("{name}-label"),
            &dragged,
            current_annotation,
            &paper,
            &moved,
        )?;
        history(c, &dragged, &label_dragged)?;
        control(c, "Undo", None)?;
        ensure!(model(c)? == dragged, "Label drag Undo lost cloud vertices");
        if name == "quad" {
            control(c, &format!("Edit annotation {id}"), None)?;
            field(c, "Revision", "b\n\u{96f6}\u{4ef6}")?;
            control(c, "Apply annotation", None)?;
            let multiline = model(c)?;
            ensure!(
                multiline
                    == replace_expected(&dragged, id, |a| a["revision"] =
                        json!("B\n\u{96f6}\u{4ef6}")),
                "Multiline cloud edit changed paper vertices or unrelated intent"
            );
            history(c, &dragged, &multiline)?;
            capture(c, out, "cloud-os-quad-multiline")?;
            curved::save_exact(c, out, "cloud-os-quad-multiline", &multiline)?;
            control(c, "Undo", None)?;
            ensure!(
                model(c)? == dragged,
                "Multiline Undo lost the exact dragged cloud"
            );
        }
        curved::delete_and_restore(c, id, &created, &dragged, baseline)?;
        reports.push(json!({"shape":name,"vertices":actual_points,"exact_model_history_archive":true,
            "actual_path_drag":true,"actual_label_drag":true,"escape_cancels_drag":true,
            "captures":[format!("cloud-os-{name}-staged.png"),format!("cloud-os-{name}-created.png"),format!("cloud-os-{name}-dragged.png")]}));
    }
    Ok(
        json!({"actual_os_paper_authoring":reports,"partial_escape_keeps_exact_model_and_counter":true,
        "loaded_seven_vertex_label_drag_exact_history_archive":true,"loaded_capture":"cloud-os-loaded-dragged.png",
        "multiline_revision_published_control_edit":true,"multiline_capture":"cloud-os-quad-multiline.png",
        "multiline_keyboard_input_proven":false}),
    )
}

fn check_drag(
    c: &mut Client,
    out: &Path,
    stage: &str,
    before: &Value,
    annotation: &Value,
    paper: &Paper,
    evidence: &Value,
) -> Result<Value> {
    let id = annotation["id"].as_u64().context("Cloud ID missing")?;
    let from = observed_point(evidence, "logical_start")?;
    let to = observed_point(evidence, "logical_end")?;
    let delta = [
        (to[0] - from[0]) / paper.scale,
        (to[1] - from[1]) / paper.scale,
    ];
    let after = changed_model(c, before, out, &format!("cloud-os-{stage}-drag"))?;
    let row = annotations(&after)?
        .iter()
        .find(|a| a["id"] == id)
        .context("Dragged cloud")?;
    let old = annotation["points"]
        .as_array()
        .context("Original cloud vertices")?;
    let new = row["points"].as_array().context("Moved cloud vertices")?;
    ensure!(old.len() == new.len(), "Cloud drag changed vertex count");
    for (a, b) in old.iter().zip(new) {
        close_point(
            b,
            [
                (a[0].as_f64().unwrap() + delta[0]).clamp(5., 292.),
                (a[1].as_f64().unwrap() + delta[1]).clamp(5., 205.),
            ],
            "Cumulative cloud drag",
        )?;
    }
    ensure!(
        after == replace_expected(before, id, |a| a["points"] = row["points"].clone()),
        "Cloud drag changed revision, paper presentation or unrelated shared intent"
    );
    Ok(after)
}
