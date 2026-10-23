//! Trusted, versioned construction snapshots generated offline from the recipe.
//! No document engine is created when a built-in lesson is opened.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MAX_ASSET_BYTES: usize = 2 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    version: u32,
    lesson: lesson::Id,
    source_sha256_lf: String,
    frames: Vec<Frame>,
}

fn asset(id: lesson::Id) -> &'static [u8] {
    macro_rules! embedded {
        ($name:literal) => {
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/show-me/",
                $name,
                ".cbor"
            ))
        };
    }
    match id {
        lesson::Id::Sketch => embedded!("sketch"),
        lesson::Id::Extrude => embedded!("extrude"),
        lesson::Id::Cut => embedded!("cut"),
        lesson::Id::Fillet => embedded!("fillet"),
    }
}

pub(super) fn load(source: &str, id: lesson::Id) -> Result<Vec<Frame>, String> {
    decode(asset(id), source, id)
}

fn decode(bytes: &[u8], source: &str, id: lesson::Id) -> Result<Vec<Frame>, String> {
    if bytes.is_empty() || bytes.len() > MAX_ASSET_BYTES || source.len() > MAX_SOURCE_BYTES {
        return Err("Bundled lesson exceeds its input budget".into());
    }
    let mut reader = std::io::Cursor::new(bytes);
    let bundle: Bundle = ciborium::de::from_reader_with_recursion_limit(&mut reader, 64)
        .map_err(|error| format!("Bundled lesson cannot be decoded: {error}"))?;
    if reader.position() != bytes.len() as u64 {
        return Err("Bundled lesson has trailing data".into());
    }
    if bundle.version != 1 || bundle.lesson != id {
        return Err("Bundled lesson version or identity does not match".into());
    }
    let hash = Sha256::digest(source.replace("\r\n", "\n").as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if bundle.source_sha256_lf != hash {
        return Err("Bundled lesson is stale; regenerate the authored snapshots".into());
    }
    if bundle.frames.len() != id.frame_count() {
        return Err("Bundled lesson has an unexpected number of frames".into());
    }
    // PreviewService::retain checks geometry/mesh/point/selection budgets before
    // any decoded frame reaches the renderer. These bytes are embedded assets,
    // never a path, network response, or user-supplied CBOR payload.
    Ok(bundle.frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> &'static str {
        &catalog::examples()
            .iter()
            .find(|example| example.id == lesson::RECIPE)
            .unwrap()
            .source
    }

    #[test]
    fn native_script_preview_bundled_assets_match_source_and_geometry_budgets() {
        let service = PreviewService::default();
        for id in lesson::ALL {
            let frames = load(source(), id).unwrap();
            assert!(frames.iter().all(|frame| frame
                .profile
                .as_ref()
                .is_none_or(|profile| profile.projected_edges.is_empty()
                    && profile.reference_midpoints.is_empty())));
            let descriptor = service.retain(frames).unwrap();
            service.release(&descriptor.preview_id);
            assert!(load(&format!("{}\n", source()), id).is_err());
            let crlf = source().replace("\r\n", "\n").replace('\n', "\r\n");
            assert!(
                load(&crlf, id).is_ok(),
                "Git checkout line endings are not a recipe change"
            );
        }
    }

    #[test]
    fn native_script_preview_bundled_assets_reject_malformed_or_mismatched_payloads() {
        let id = lesson::Id::Sketch;
        let bytes = asset(id);
        assert!(decode(&[], source(), id).is_err());
        assert!(decode(&bytes[..bytes.len() / 2], source(), id).is_err());
        assert!(decode(bytes, source(), lesson::Id::Cut).is_err());
        let mut trailing = bytes.to_vec();
        trailing.push(0);
        assert!(decode(&trailing, source(), id).is_err());
        let mut value: serde_json::Value = ciborium::de::from_reader(bytes).unwrap();
        value["version"] = json!(2);
        let mut altered = Vec::new();
        ciborium::ser::into_writer(&value, &mut altered).unwrap();
        assert!(decode(&altered, source(), id).is_err());
        value["version"] = json!(1);
        value["frames"] = json!([]);
        altered.clear();
        ciborium::ser::into_writer(&value, &mut altered).unwrap();
        assert!(decode(&altered, source(), id).is_err());
        assert!(decode(&vec![0; MAX_ASSET_BYTES + 1], source(), id).is_err());
    }
}
