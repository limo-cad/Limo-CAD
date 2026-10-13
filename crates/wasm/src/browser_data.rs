//! Reuse the host-neutral archive boundary and a Rust triangulator in browsers.
use limo_cad_project_file::{ProjectArchive, SaveMetadata};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn project_archive_encode(
    model_json: &str,
    application_version: &str,
    saved_at: &str,
) -> Result<Vec<u8>, String> {
    ProjectArchive::new(
        model_json.to_owned(),
        SaveMetadata {
            application_version,
            saved_at,
        },
    )
    .and_then(|archive| archive.encode())
    .map_err(|error| error.to_string())
}

#[wasm_bindgen]
pub fn project_archive_decode(bytes: Vec<u8>) -> Result<String, String> {
    let archive = ProjectArchive::decode(bytes).map_err(|error| error.to_string())?;
    serde_json::to_string(
        &serde_json::json!({"manifest":archive.manifest(), "modelJson":archive.model_json()}),
    )
    .map_err(|error| error.to_string())
}

#[wasm_bindgen]
pub fn triangulate_profile(vertices: &[f64], hole_indices: &[u32]) -> Result<Vec<u32>, String> {
    if vertices.len() < 6
        || !vertices.len().is_multiple_of(2)
        || vertices.iter().any(|value| !value.is_finite())
    {
        return Err("profile requires finite 2D vertices".into());
    }
    let count = vertices.len() / 2;
    let holes: Vec<usize> = hole_indices.iter().map(|value| *value as usize).collect();
    let mut start = 0;
    for end in holes.iter().copied().chain([count]) {
        if end > count || end < start || end - start < 3 {
            return Err("profile loop requires at least three vertices".into());
        }
        start = end;
    }
    earcutr::earcut(vertices, &holes, 2)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|index| u32::try_from(index).map_err(|_| "profile index overflow".into()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_bridge_roundtrips_rejects_bad_models_and_completes_zip64_sentinel() {
        let model = r#"{"format":"limo-cad-project","schema_version":3}"#;
        let mut bytes = project_archive_encode(model, "0.2.2", "2026-10-02T00:00:00Z").unwrap();
        let decoded: serde_json::Value =
            serde_json::from_str(&project_archive_decode(bytes.clone()).unwrap()).unwrap();
        assert_eq!(decoded["modelJson"].as_str().unwrap().trim(), model);
        assert_eq!(decoded["manifest"]["model_schema_version"], 3);
        assert!(project_archive_encode("{}", "0.2.2", "date").is_err());
        assert!(project_archive_decode(vec![]).is_err());
        let offset = bytes
            .windows(4)
            .position(|window| window == b"PK\x01\x02")
            .unwrap();
        bytes[offset + 20..offset + 24].copy_from_slice(&u32::MAX.to_le_bytes());
        if let Ok(decoded) = project_archive_decode(bytes) {
            let decoded: serde_json::Value = serde_json::from_str(&decoded).unwrap();
            assert_eq!(decoded["modelJson"].as_str().unwrap().trim(), model);
        }
    }
    #[test]
    fn holed_profile_has_correct_area_and_indices_and_rejects_bad_loops() {
        let vertices = [
            0., 0., 10., 0., 10., 10., 0., 10., 3., 3., 3., 7., 7., 7., 7., 3.,
        ];
        let indices = triangulate_profile(&vertices, &[4]).unwrap();
        let area: f64 = indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| {
                let at = |index: u32| {
                    (
                        vertices[index as usize * 2],
                        vertices[index as usize * 2 + 1],
                    )
                };
                let (a, b, c) = (at(triangle[0]), at(triangle[1]), at(triangle[2]));
                ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs() / 2.
            })
            .sum();
        assert!((area - 84.).abs() < 1e-9);
        assert!(indices
            .iter()
            .all(|index| (*index as usize) < vertices.len() / 2));
        for holes in [&[2][..], &[9], &[4, 4], &[4, 3]] {
            assert!(triangulate_profile(&vertices, holes).is_err());
        }
        assert!(triangulate_profile(&[f64::NAN, 0., 1., 0., 0., 1.], &[]).is_err());
    }
}
