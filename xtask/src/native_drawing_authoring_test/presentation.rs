//! Published linear-presentation controls, exact history, and archive intent.
use super::*;

pub(super) fn exercise(c: &mut Client, out: &Path, id: u64, initial: &Value) -> Result<()> {
    let mut before = initial.clone();
    for (stage, mode, upper, lower, basic, dual, fit, unit, precision, placement) in [
        (
            "symmetric",
            "symmetric",
            0.1,
            -0.05,
            true,
            true,
            "H7",
            "inch",
            4,
            "bracketed",
        ),
        (
            "deviation",
            "deviation",
            0.2,
            -0.1,
            false,
            true,
            "g6",
            "centimetre",
            3,
            "stacked",
        ),
        (
            "limits",
            "limits",
            0.3,
            -0.2,
            true,
            false,
            "",
            "centimetre",
            3,
            "stacked",
        ),
        (
            "none",
            "none",
            0.3,
            -0.2,
            false,
            false,
            "",
            "centimetre",
            3,
            "stacked",
        ),
    ] {
        control(c, &format!("Edit annotation {id}"), None)?;
        field(c, "Tolerance mode", mode)?;
        if mode != "none" {
            field(c, "Upper tolerance", &upper.to_string())?;
            field(c, "Lower tolerance", &lower.to_string())?;
        }
        field(
            c,
            if basic {
                "Basic dimension"
            } else {
                "Reference dimension"
            },
            "true",
        )?;
        field(c, "Fit class", fit)?;
        field(c, "Dual units", if dual { "true" } else { "false" })?;
        if dual {
            field(c, "Secondary unit", unit)?;
            field(c, "Dual precision", &precision.to_string())?;
            field(c, "Dual placement", placement)?;
        }
        ensure!(
            model(c)? == before,
            "Presentation fields committed before Apply"
        );
        control(c, "Apply annotation", None)?;
        let after = model(c)?;
        let expected = replace_expected(&before, id, |a| {
            a["presentation"] = json!({
                "tolerance":{"mode":mode,"upper":upper,"lower":lower},
                "basic":basic,"reference":!basic,"fit_class":fit,
                "dual_units":if dual { json!({"unit":unit,"precision":precision,"placement":placement}) } else { Value::Null },
            })
        });
        std::fs::write(
            out.join(format!("author-presentation-{stage}.json")),
            serde_json::to_vec_pretty(&after)?,
        )?;
        ensure!(
            after == expected,
            "Presentation {stage} changed unrelated model intent or did not preserve exact typed values"
        );
        history(c, &before, &after)?;
        control(c, &format!("Edit annotation {id}"), None)?;
        field(c, "Tolerance mode", mode)?;
        capture(c, out, &format!("author-presentation-{stage}"))?;
        if stage == "deviation" {
            field(c, "Secondary unit", unit)?;
            capture(c, out, "author-presentation-dual")?;
            let path = out.join("author-presentation.limo");
            ui(c, json!({"action":"file","command":"save","path":path}))?;
            let mut saved = zip::ZipArchive::new(std::fs::File::open(&path)?)?;
            let archived: Value = serde_json::from_reader(saved.by_name("model.json")?)?;
            ensure!(
                archived == after && model(c)? == after,
                "Saving lost exact dimension presentation intent"
            );
        }
        before = after;
    }
    for _ in 0..4 {
        control(c, "Undo", None)?;
    }
    ensure!(
        &model(c)? == initial,
        "Presentation history did not restore the exact prior dimension"
    );
    control(c, &format!("Edit annotation {id}"), None)?;
    Ok(())
}
