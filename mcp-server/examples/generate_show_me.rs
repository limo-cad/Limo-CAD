//! Offline authoring tool: runtime lessons never execute the construction recipe.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let (output, selected_recipe) = options(std::env::args().skip(1))?;
    let recipes = recipe_ids(selected_recipe.as_deref())?;
    let mut assets = Vec::new();
    let mut asset_records = Vec::new();
    let mut recipe_records = Vec::new();
    let mut total = 0;
    for recipe in recipes {
        let source = limo_cad_recipes::find(recipe)?.source;
        let source_hash = Sha256::digest(source.replace("\r\n", "\n").as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        // Trusted, separate 512-operation offline budget. Existing recipes keep
        // run_script; Measure completes on its prerequisite's exact native host.
        let mut report = limo_cad_mcp::lesson_authoring::run_bundled(recipe)?;
        recipe_records.push(json!({"recipe":recipe, "source_sha256_lf":source_hash,
            "checks_completed":report["checks_completed"], "steps_completed":report["steps_completed"]}));
        let lessons: Vec<String> = if selected_recipe.is_some() {
            report["exports"]["lessons"]
                .as_object()
                .ok_or("Missing lesson exports")?
                .keys()
                .filter(|key| !(recipe == "extrude-basics" && *key == "cut_measured"))
                .cloned()
                .collect()
        } else {
            limo_cad_mcp::lessons::ALL
                .iter()
                .filter(|id| id.recipe() == recipe)
                .map(|id| id.key().to_owned())
                .collect()
        };
        if lessons.is_empty() || lessons.len() > 128 {
            return Err("Invalid lesson export count".into());
        }
        for lesson in lessons {
            if lesson.is_empty()
                || lesson.len() > 80
                || !lesson
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
            {
                return Err("Unsafe lesson export name".into());
            }
            let mut frames = report["exports"]["lessons"][&lesson].take();
            let frames = frames.as_array_mut().ok_or("Missing lesson frames")?;
            if lesson == "cut" {
                let measured = report["exports"]["lessons"]["cut_measured"].take();
                let measured = measured.as_array().ok_or("Missing measured cut")?;
                if measured.len() != 4 || frames.len() != 4 {
                    return Err("Cut requires four frames in each scenario".into());
                }
                frames.extend_from_slice(&measured[2..]);
            }
            if limo_cad_mcp::lessons::Id::from_key(&lesson)
                .is_some_and(|id| frames.len() != id.frame_count())
            {
                return Err(format!("Unexpected {lesson} frame count").into());
            }
            for frame in frames.iter_mut() {
                if let Some(profile) = frame.get_mut("profile").and_then(Value::as_object_mut) {
                    profile.insert("projected_edges".into(), json!([]));
                    profile.insert("reference_midpoints".into(), json!([]));
                }
            }
            let decoded: Vec<limo_cad_native_engine::teaching::Frame> =
                serde_json::from_value(json!(frames))?;
            limo_cad_native_engine::teaching::validate_frames(&decoded)?;
            let envelope =
                json!({"version":1,"lesson":lesson,"source_sha256_lf":source_hash,"frames":decoded});
            let mut bounded = AssetBytes(Vec::new());
            ciborium::ser::into_writer(&envelope, &mut bounded)?;
            let bytes = bounded.0;
            if bytes.len() > 2 * 1024 * 1024 {
                return Err(format!("{lesson} asset exceeds 2 MiB").into());
            }
            total += bytes.len();
            if total > 8 * 1024 * 1024 {
                return Err("Authored assets exceed 8 MiB".into());
            }
            let asset_hash = Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            asset_records.push(json!({"lesson":lesson, "recipe":recipe,
            "source_sha256_lf":source_hash, "frame_count":frames.len(),
            "sha256":asset_hash, "bytes":bytes.len()}));
            assets.push((lesson, bytes));
        }
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

fn recipe_ids(selected: Option<&str>) -> Result<Vec<&'static str>, String> {
    let mut recipes = if let Some(id) = selected {
        vec![limo_cad_recipes::find(id)?.id]
    } else {
        limo_cad_mcp::lessons::ALL
            .iter()
            .map(|id| id.recipe())
            .collect()
    };
    recipes.sort_unstable();
    recipes.dedup();
    Ok(recipes)
}

fn options(args: impl Iterator<Item = String>) -> Result<(PathBuf, Option<String>), String> {
    let mut args = args;
    let (mut output, mut recipe) = (None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" if output.is_none() => {
                output = Some(PathBuf::from(
                    args.next().ok_or("Missing output directory")?,
                ))
            }
            "--recipe" if recipe.is_none() => {
                recipe = Some(args.next().ok_or("Missing bundled recipe ID")?)
            }
            _ => return Err(format!("Unexpected or duplicate argument: {arg}")),
        }
    }
    Ok((
        output.ok_or("Usage: generate_show_me --out OUTPUT_DIRECTORY [--recipe BUNDLED_ID]")?,
        recipe,
    ))
}

struct AssetBytes(Vec<u8>);
impl std::io::Write for AssetBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self
            .0
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > 2 * 1024 * 1024)
        {
            return Err(std::io::Error::other("Lesson asset exceeds 2 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundle_first_arguments_do_not_require_a_help_id() {
        let (out, recipe) = options(
            ["--recipe", "show-me-measure", "--out", "out"]
                .into_iter()
                .map(String::from),
        )
        .unwrap();
        assert_eq!(out, PathBuf::from("out"));
        assert_eq!(recipe.as_deref(), Some("show-me-measure"));
        // This installed recipe has no Help::ALL entry. Source selection must
        // work before any new guide is advertised or any asset is embedded.
        assert!(!limo_cad_mcp::lessons::ALL
            .iter()
            .any(|id| id.recipe() == "angle-bracket"));
        assert_eq!(
            recipe_ids(Some("angle-bracket")).unwrap(),
            ["angle-bracket"]
        );
        assert!(recipe_ids(Some("unknown-not-bundled")).is_err());
        for args in [
            vec!["--out"],
            vec!["--recipe", "x"],
            vec!["--out", "a", "--out", "b"],
            vec!["--out", "a", "--recipe", "x", "--recipe", "y"],
        ] {
            assert!(options(args.into_iter().map(String::from)).is_err());
        }
    }
    #[test]
    fn pending_native_measurement_is_never_packaged() {
        let value = json!({"caption":"measure.distance", "scene":{"bodies":[],"errors":[]},
            "inspection":{"native_measurement_pending":true}});
        assert!(serde_json::from_value::<limo_cad_native_engine::teaching::Frame>(value).is_err());
        assert!(
            serde_json::from_value::<limo_cad_native_engine::teaching::Frame>(json!({
            "caption":"pending", "scene":{"bodies":[],"errors":[]}, "future_pending":true}))
            .is_err()
        );
    }
    #[test]
    fn asset_writer_stops_at_the_limit() {
        use std::io::Write;
        let mut writer = AssetBytes(vec![0; 2 * 1024 * 1024 - 1]);
        writer.write_all(&[1]).unwrap();
        assert!(writer.write_all(&[2]).is_err());
        assert_eq!(writer.0.len(), 2 * 1024 * 1024);
    }
}
