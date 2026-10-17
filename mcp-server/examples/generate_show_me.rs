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
    let source = limo_cad_recipes::find("extrude-basics")?.source;
    let source_hash = Sha256::digest(source.replace("\r\n", "\n").as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut report = limo_cad_mcp::preview_script(source)?;
    let mut assets = Vec::new();
    let mut total = 0;
    for (lesson, count) in [("sketch", 5), ("extrude", 5), ("cut", 6), ("fillet", 4)] {
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
        if frames.len() != count {
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
    println!("Source SHA-256 (LF): {source_hash}; total {total} bytes");
    Ok(())
}
