//! Offline authoring tool: runtime lessons never execute the construction recipe.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--out") {
        return Err("Usage: generate_show_me --out OUTPUT_DIRECTORY".into());
    }
    let output = PathBuf::from(args.next().ok_or("Missing output directory")?);
    if args.next().is_some() {
        return Err("Unexpected argument".into());
    }
    let mut ids = limo_cad_mcp::lessons::ALL.to_vec();
    ids.sort_by_key(|id| id.recipe());
    let mut recipe = None;
    let mut source_hash = String::new();
    let mut report = Value::Null;
    let mut assets = Vec::new();
    let mut asset_records = Vec::new();
    let mut recipe_records = Vec::new();
    let mut total = 0;
    for id in ids {
        if recipe != Some(id.recipe()) {
            let source = limo_cad_recipes::find(id.recipe())?.source;
            source_hash = Sha256::digest(source.replace("\r\n", "\n").as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            // These are trusted bundled recipes, executed offline. The public
            // miniature-preview limit stays at 80 operations; a recipe shared
            // by several focused guides needs a separate bounded authoring run.
            let metadata = limo_cad_script::Script::parse(source)?.metadata();
            let operations = metadata["step_count"].as_u64().unwrap_or(0)
                + metadata["check_count"].as_u64().unwrap_or(0);
            if source.len() > 2 * 1024 * 1024 || operations > 512 {
                return Err(format!("{} exceeds the offline authoring budget", id.recipe()).into());
            }
            report = limo_cad_mcp::run_script(source, None, None, "fast", 1.0)?;
            recipe_records.push(json!({"recipe":id.recipe(), "source_sha256_lf":source_hash,
                "checks_completed":report["checks_completed"], "steps_completed":report["steps_completed"]}));
            recipe = Some(id.recipe());
        }
        let lesson = id.key();
        let mut frames = report["exports"]["lessons"][lesson].take();
        let frames = frames.as_array_mut().ok_or("Missing lesson frames")?;
        if lesson == "cut" {
            let measured = report["exports"]["lessons"]["cut_measured"].take();
            let measured = measured.as_array().ok_or("Missing measured cut")?;
            if measured.len() != 4 || frames.len() != 4 {
                return Err("Cut requires four frames in each scenario".into());
            }
            frames.extend_from_slice(&measured[2..]);
        }
        if frames.len() != id.frame_count() {
            return Err(format!("Unexpected {lesson} frame count").into());
        }
        for frame in frames.iter_mut() {
            if let Some(profile) = frame.get_mut("profile").and_then(Value::as_object_mut) {
                profile.insert("projected_edges".into(), json!([]));
                profile.insert("reference_midpoints".into(), json!([]));
            }
        }
        let envelope =
            json!({"version":1,"lesson":lesson,"source_sha256_lf":source_hash,"frames":frames});
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&envelope, &mut bytes)?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(format!("{lesson} asset exceeds 2 MiB").into());
        }
        total += bytes.len();
        let asset_hash = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        asset_records.push(json!({"lesson":lesson, "recipe":id.recipe(),
            "source_sha256_lf":source_hash, "frame_count":frames.len(),
            "sha256":asset_hash, "bytes":bytes.len()}));
        assets.push((lesson, bytes));
    }
    if total > 8 * 1024 * 1024 {
        return Err("Authored assets exceed 8 MiB".into());
    }
    std::fs::create_dir_all(&output)?;
    for (lesson, bytes) in assets {
        let path = output.join(format!("{lesson}.cbor"));
        std::fs::write(&path, &bytes)?;
        println!("{}: {} bytes", path.display(), bytes.len());
    }
    let provenance = json!({"version":1, "source_revision":std::env::var("GITHUB_SHA").ok(),
        "generator_version":env!("CARGO_PKG_VERSION"), "total_asset_bytes":total,
        "recipes":recipe_records, "assets":asset_records});
    std::fs::write(
        output.join("provenance.json"),
        serde_json::to_vec_pretty(&provenance)?,
    )?;
    println!("Total authored geometry: {total} bytes");
    Ok(())
}
