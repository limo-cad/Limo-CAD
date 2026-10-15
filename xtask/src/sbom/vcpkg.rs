//! Validate the vcpkg OCCT port's cached build evidence, not a Unix SDK receipt.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};

const SOURCE_SHA512: &str = "bbe7099071cbf5397940ebc6e66ec05f8023d5e5aae6142870e14b93aa6f8f94c30980ef421e717f0fbfbc23b3520c3ccfe8a939c4caba3ccbf325060e26eb52";
const PATCHES: &[&str] = &[
    "0001-cmake-keep-build-use-vcpkg-explicit.patch",
    "0002-cmake-load-exported-package-dependencies.patch",
    "0003-image-remove-freeimage-msvc-autolink.patch",
    "0004-cmake-add-additional-path-extraction-for-OpenCASCADE.patch",
    "0005-drop-bin-letter.patch",
    "math_DoubleTab.hxx",
];

pub(super) fn evidence(sdk: &Path, mut evidence: Value) -> Result<Value> {
    let path = sdk.join("share/opencascade/limo-occt-build.json");
    let manifest = super::json_file(&path)
        .context("rebuild the pinned vcpkg overlay to capture OCCT build evidence")?;
    ensure!(
        manifest["schemaVersion"] == 1
            && manifest["version"] == "8.0.1"
            && manifest["sourceSha512"] == SOURCE_SHA512,
        "unexpected vcpkg OCCT source identity"
    );
    let overlay = crate::release_tooling::root().join("native/occt-overlay/opencascade");
    let patches = PATCHES
        .iter()
        .map(|name| Ok(((*name).into(), crate::hash::file(&overlay.join(name))?)))
        .collect::<Result<BTreeMap<String, String>>>()?;
    ensure!(
        manifest["patches"] == serde_json::to_value(&patches)?,
        "vcpkg OCCT patches differ from this checkout"
    );
    let files = manifest["files"]
        .as_object()
        .context("vcpkg native file receipt")?;
    let mut actual = BTreeMap::new();
    for entry in fs::read_dir(sdk.join("bin"))? {
        let entry = entry?;
        let name = entry
            .file_name()
            .to_str()
            .context("UTF-8 DLL name")?
            .to_owned();
        if name.starts_with("TK") && name.ends_with(".dll") {
            actual.insert(format!("bin/{name}"), crate::hash::file(&entry.path())?);
        }
    }
    ensure!(
        actual.contains_key("bin/TKernel.dll") && actual.contains_key("bin/TKDESTEP.dll"),
        "vcpkg OCCT receipt lacks the required runtime"
    );
    ensure!(
        serde_json::to_value(&actual)? == Value::Object(files.clone()),
        "vcpkg OCCT DLLs do not match the cached port receipt"
    );
    let mut metadata = BTreeMap::new();
    for port in [
        "opencascade",
        "freetype",
        "zlib",
        "bzip2",
        "libpng",
        "brotli",
    ] {
        let path = sdk.join("share").join(port).join("vcpkg.spdx.json");
        if path.is_file() {
            metadata.insert(port, crate::hash::file(&path)?);
        }
    }
    evidence["buildSystem"] = json!("pinned vcpkg overlay; OCCT port DLL receipt");
    evidence["vcpkg"] = json!({"triplet":manifest["triplet"],"occtSourceSha512":SOURCE_SHA512,"overlayPatchesSha256":patches,
        "nativeReceiptSha256":crate::hash::file(&path)?,"packageMetadataSha256":metadata});
    Ok(evidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vcpkg_receipt_requires_the_actual_overlay_and_every_occt_dll() {
        let directory = tempfile::tempdir().unwrap();
        let sdk = directory.path();
        fs::create_dir_all(sdk.join("bin")).unwrap();
        fs::create_dir_all(sdk.join("share/opencascade")).unwrap();
        for name in ["TKernel.dll", "TKDESTEP.dll"] {
            fs::write(sdk.join("bin").join(name), name).unwrap();
        }
        let overlay = crate::release_tooling::root().join("native/occt-overlay/opencascade");
        let patches = PATCHES
            .iter()
            .map(|name| {
                (
                    (*name).to_owned(),
                    crate::hash::file(&overlay.join(name)).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let files = ["TKernel.dll", "TKDESTEP.dll"]
            .iter()
            .map(|name| {
                (
                    format!("bin/{name}"),
                    crate::hash::file(&sdk.join("bin").join(name)).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let path = sdk.join("share/opencascade/limo-occt-build.json");
        let mut manifest = json!({"schemaVersion":1,"version":"8.0.1","triplet":"x64-windows","sourceSha512":SOURCE_SHA512,"patches":patches,"files":files});
        super::super::write_json(&path, &manifest).unwrap();
        let base = crate::occt_sdk::sbom_evidence("").unwrap();
        evidence(sdk, base.clone()).unwrap();
        fs::write(sdk.join("bin/TKernel.dll"), "substituted DLL").unwrap();
        assert!(evidence(sdk, base.clone())
            .unwrap_err()
            .to_string()
            .contains("port receipt"));
        fs::write(sdk.join("bin/TKernel.dll"), "TKernel.dll").unwrap();
        manifest["patches"]["math_DoubleTab.hxx"] = json!("unexpected patch");
        super::super::write_json(&path, &manifest).unwrap();
        assert!(evidence(sdk, base)
            .unwrap_err()
            .to_string()
            .contains("patches differ"));
    }
}
