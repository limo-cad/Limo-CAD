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
        lesson::Id::Revolve => embedded!("revolve"),
        lesson::Id::Sweep => embedded!("sweep"),
        lesson::Id::Loft => embedded!("loft"),
        lesson::Id::Rib => embedded!("rib"),
        lesson::Id::Hole => embedded!("hole"),
        lesson::Id::ExternalThread => embedded!("external_thread"),
        lesson::Id::Chamfer => embedded!("chamfer"),
        lesson::Id::Shell => embedded!("shell"),
        lesson::Id::Mirror => embedded!("mirror"),
        lesson::Id::RectangularPattern => embedded!("rectangular_pattern"),
        lesson::Id::CircularPattern => embedded!("circular_pattern"),
        lesson::Id::MoveCopy => embedded!("move_copy"),
        lesson::Id::Combine => embedded!("combine"),
        lesson::Id::SplitBody => embedded!("split_body"),
        lesson::Id::OffsetPlane => embedded!("offset_plane"),
        lesson::Id::Midplane => embedded!("midplane"),
        lesson::Id::PlaneAtAngle => embedded!("plane_at_angle"),
        lesson::Id::SketchLine => embedded!("sketch_line"),
        lesson::Id::SketchMidpointLine => embedded!("sketch_midpoint_line"),
        lesson::Id::SketchPoint => embedded!("sketch_point"),
        lesson::Id::SketchRectangle => embedded!("sketch_rectangle"),
        lesson::Id::SketchCircle => embedded!("sketch_circle"),
        lesson::Id::SketchArc => embedded!("sketch_arc"),
        lesson::Id::SketchSpline => embedded!("sketch_spline"),
        lesson::Id::SketchPolygon => embedded!("sketch_polygon"),
        lesson::Id::SketchSlot => embedded!("sketch_slot"),
        lesson::Id::SketchMoveCopy => embedded!("sketch_move_copy"),
        lesson::Id::SketchScale => embedded!("sketch_scale"),
        lesson::Id::SketchOffset => embedded!("sketch_offset"),
        lesson::Id::SketchTrim => embedded!("sketch_trim"),
        lesson::Id::SketchExtend => embedded!("sketch_extend"),
        lesson::Id::SketchBreak => embedded!("sketch_break"),
        lesson::Id::SketchFillet => embedded!("sketch_fillet"),
        lesson::Id::SketchChamfer => embedded!("sketch_chamfer"),
        lesson::Id::SketchDimension => embedded!("sketch_dimension"),
        lesson::Id::SketchMirror => embedded!("sketch_mirror"),
        lesson::Id::SketchRectangularPattern => embedded!("sketch_rectangular_pattern"),
        lesson::Id::SketchCircularPattern => embedded!("sketch_circular_pattern"),
        lesson::Id::SketchCoincident => embedded!("sketch_coincident"),
        lesson::Id::SketchHorizontalVertical => embedded!("sketch_horizontal_vertical"),
        lesson::Id::SketchTangent => embedded!("sketch_tangent"),
        lesson::Id::SketchParallel => embedded!("sketch_parallel"),
        lesson::Id::SketchPerpendicular => embedded!("sketch_perpendicular"),
        lesson::Id::SketchEqual => embedded!("sketch_equal"),
        lesson::Id::SketchFixUnfix => embedded!("sketch_fix_unfix"),
        lesson::Id::SketchMidpoint => embedded!("sketch_midpoint"),
        lesson::Id::SketchCollinear => embedded!("sketch_collinear"),
        lesson::Id::SketchConcentric => embedded!("sketch_concentric"),
        lesson::Id::SketchSymmetry => embedded!("sketch_symmetry"),
    }
}

/// A lesson may only decode snapshots authored by its own bundled source.
/// Adding an Id requires an explicit asset and source entry; no generic recipe fallback.
pub(super) fn known_source(id: lesson::Id) -> &'static str {
    macro_rules! source {
        ($name:literal) => {
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../examples/scripts/",
                $name,
                ".limo.jsonc"
            ))
        };
    }
    match id {
        lesson::Id::Sketch => source!("extrude-basics"),
        lesson::Id::Extrude => source!("extrude-basics"),
        lesson::Id::Cut => source!("extrude-basics"),
        lesson::Id::Fillet => source!("extrude-basics"),
        lesson::Id::Revolve => source!("show-me-revolve"),
        lesson::Id::Sweep => source!("show-me-sweep"),
        lesson::Id::Loft => source!("show-me-loft"),
        lesson::Id::Rib => source!("show-me-rib"),
        lesson::Id::Hole => source!("show-me-hole"),
        lesson::Id::ExternalThread => source!("show-me-external-thread"),
        lesson::Id::Chamfer => source!("show-me-chamfer"),
        lesson::Id::Shell => source!("show-me-shell"),
        lesson::Id::Mirror => source!("show-me-mirror"),
        lesson::Id::RectangularPattern => source!("show-me-rectangular-pattern"),
        lesson::Id::CircularPattern => source!("show-me-circular-pattern"),
        lesson::Id::MoveCopy => source!("show-me-move-copy"),
        lesson::Id::Combine => source!("show-me-combine"),
        lesson::Id::SplitBody => source!("show-me-split-body"),
        lesson::Id::OffsetPlane => source!("show-me-offset-plane"),
        lesson::Id::Midplane => source!("show-me-midplane"),
        lesson::Id::PlaneAtAngle => source!("show-me-plane-at-angle"),
        lesson::Id::SketchLine => source!("show-me-sketch-draw"),
        lesson::Id::SketchMidpointLine => source!("show-me-sketch-draw"),
        lesson::Id::SketchPoint => source!("show-me-sketch-draw"),
        lesson::Id::SketchRectangle => source!("show-me-sketch-draw"),
        lesson::Id::SketchCircle => source!("show-me-sketch-draw"),
        lesson::Id::SketchArc => source!("show-me-sketch-draw"),
        lesson::Id::SketchSpline => source!("show-me-sketch-draw"),
        lesson::Id::SketchPolygon => source!("show-me-sketch-draw"),
        lesson::Id::SketchSlot => source!("show-me-sketch-draw"),
        lesson::Id::SketchMoveCopy => source!("show-me-sketch-edit"),
        lesson::Id::SketchScale => source!("show-me-sketch-edit"),
        lesson::Id::SketchOffset => source!("show-me-sketch-edit"),
        lesson::Id::SketchTrim => source!("show-me-sketch-edit"),
        lesson::Id::SketchExtend => source!("show-me-sketch-edit"),
        lesson::Id::SketchBreak => source!("show-me-sketch-edit"),
        lesson::Id::SketchFillet => source!("show-me-sketch-edit"),
        lesson::Id::SketchChamfer => source!("show-me-sketch-edit"),
        lesson::Id::SketchDimension => source!("show-me-sketch-repeat"),
        lesson::Id::SketchMirror => source!("show-me-sketch-repeat"),
        lesson::Id::SketchRectangularPattern => source!("show-me-sketch-repeat"),
        lesson::Id::SketchCircularPattern => source!("show-me-sketch-repeat"),
        lesson::Id::SketchCoincident => source!("show-me-sketch-constraints"),
        lesson::Id::SketchHorizontalVertical => source!("show-me-sketch-constraints"),
        lesson::Id::SketchTangent => source!("show-me-sketch-constraints"),
        lesson::Id::SketchParallel => source!("show-me-sketch-constraints"),
        lesson::Id::SketchPerpendicular => source!("show-me-sketch-constraints"),
        lesson::Id::SketchEqual => source!("show-me-sketch-constraints"),
        lesson::Id::SketchFixUnfix => source!("show-me-sketch-constraints"),
        lesson::Id::SketchMidpoint => source!("show-me-sketch-constraints"),
        lesson::Id::SketchCollinear => source!("show-me-sketch-constraints"),
        lesson::Id::SketchConcentric => source!("show-me-sketch-constraints"),
        lesson::Id::SketchSymmetry => source!("show-me-sketch-constraints"),
    }
}

pub(super) fn load(source: &str, id: lesson::Id) -> Result<Vec<Frame>, String> {
    decode(asset(id), source, id)
}

fn decode(bytes: &[u8], source: &str, id: lesson::Id) -> Result<Vec<Frame>, String> {
    if bytes.is_empty() || bytes.len() > MAX_ASSET_BYTES || source.len() > MAX_SOURCE_BYTES {
        return Err("Bundled lesson exceeds its input budget".into());
    }
    if source.replace("\r\n", "\n") != known_source(id).replace("\r\n", "\n") {
        return Err("Bundled lesson source does not match its authored recipe".into());
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
    if !bundle
        .frames
        .iter()
        .map(|frame| frame.caption.as_str())
        .eq(id.captions().iter().copied())
    {
        return Err("Bundled lesson captions do not match its teaching catalog".into());
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
        known_source(lesson::Id::Sketch)
    }

    #[test]
    fn native_script_preview_bundled_assets_match_source_and_geometry_budgets() {
        let service = PreviewService::default();
        let mut total = 0;
        for id in lesson::ALL {
            total += asset(id).len();
            let source = known_source(id);
            assert!(catalog::examples()
                .iter()
                .any(|example| example.id == id.recipe() && example.source == source));
            let frames = load(source, id).unwrap();
            assert_eq!(
                frames
                    .iter()
                    .map(|frame| frame.caption.as_str())
                    .collect::<Vec<_>>(),
                id.captions()
            );
            assert!(frames.iter().all(|frame| frame
                .profile
                .as_ref()
                .is_none_or(|profile| profile.projected_edges.is_empty()
                    && profile.reference_midpoints.is_empty())));
            let descriptor = service.retain(frames).unwrap();
            service.release(&descriptor.preview_id);
            assert!(load(&format!("{source}\n"), id).is_err());
            let crlf = source.replace("\r\n", "\n").replace('\n', "\r\n");
            assert!(
                load(&crlf, id).is_ok(),
                "Git checkout line endings are not a recipe change"
            );
        }
        assert!(
            total <= 8 * 1024 * 1024,
            "Authored assets exceed the aggregate budget"
        );
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
