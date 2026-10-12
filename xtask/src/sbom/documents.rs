use super::native::Library;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(super) fn auditable_root(cdx: &Value, version: &str) -> Result<String> {
    let root = cdx["components"].as_array().context("compiled Rust inventory is empty")?.iter().find(|component| {
        component["name"] == "limo-cad" && component["version"] == version
            && component["properties"].as_array().is_some_and(|properties| properties.iter().any(|property| {
                property["name"] == "syft:package:foundBy" && property["value"] == "cargo-auditable-binary-cataloger"
            }))
    }).context("release executable lacks the cargo-auditable dependency graph; build with LIMO_CAD_AUDITABLE=1")?;
    Ok(root["bom-ref"]
        .as_str()
        .context("compiled root reference")?
        .into())
}

pub(super) fn enrich_crate_licenses(root: &Path, target: &str, cdx: &mut Value) -> Result<()> {
    let text = super::output(crate::build_tools::cargo().current_dir(root).args([
        "metadata",
        "--locked",
        "--format-version",
        "1",
        "--manifest-path",
        "desktop/Cargo.toml",
        "--filter-platform",
        target,
    ]))?;
    let metadata: Value = serde_json::from_str(&text)?;
    let packages: BTreeMap<_, _> = metadata["packages"]
        .as_array()
        .context("Cargo package metadata")?
        .iter()
        .map(|package| {
            (
                (
                    package["name"].as_str().unwrap_or(""),
                    package["version"].as_str().unwrap_or(""),
                ),
                package,
            )
        })
        .collect();
    for component in cdx["components"]
        .as_array_mut()
        .context("SBOM components")?
    {
        let key = (
            component["name"].as_str().unwrap_or(""),
            component["version"].as_str().unwrap_or(""),
        );
        if let Some(package) = packages.get(&key) {
            if let Some(license) = package["license"]
                .as_str()
                .filter(|value| !value.is_empty())
            {
                component["licenses"] = json!([{"expression":license}]);
            }
        }
    }
    Ok(())
}

pub(super) fn native_ref(library: &Library) -> Result<String> {
    Ok(format!(
        "limo-native-{}",
        super::super::hash::reader(library.path.as_bytes())?
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn add_release(
    cdx: &mut Value,
    libraries: &[Library],
    sdk: &Value,
    version: &str,
    revision: &str,
    target: &str,
    artifact_hash: &str,
    executable: &str,
) -> Result<()> {
    let root = cdx["metadata"]["component"]["bom-ref"]
        .as_str()
        .context("SBOM root")?
        .to_owned();
    cdx["metadata"]["component"]["type"] = json!("application");
    cdx["metadata"]["component"]["version"] = json!(version);
    cdx["metadata"]["component"]["supplier"] = json!({"name":"Limo CAD project"});
    cdx["metadata"]["component"]["licenses"] = json!([{"expression":"LGPL-2.1-or-later"}]);
    cdx["metadata"]["component"]["hashes"] = json!([{"alg":"SHA-256","content":artifact_hash}]);
    cdx["metadata"]["component"]["externalReferences"] =
        json!([{"type":"vcs","url":format!("{}/tree/{revision}",super::REPOSITORY)}]);
    cdx["metadata"]["authors"] = json!([{"name":"Limo CAD project"}]);
    cdx["metadata"]["lifecycles"] = json!([{"phase":"post-build"}]);
    cdx["metadata"]["tools"] = json!({"components":[
        {"type":"application","name":"syft","version":super::SYFT_VERSION},
        {"type":"application","name":"cargo-auditable","version":super::AUDITABLE_VERSION},
        {"type":"application","name":"Limo CAD xtask SBOM generator","version":version,
          "externalReferences":[{"type":"vcs","url":format!("{}/tree/{revision}/xtask/src/sbom",super::REPOSITORY)}]}
    ]});
    cdx["metadata"]["properties"] = json!([
        {"name":"limo:source:revision","value":revision}, {"name":"limo:build:target","value":target},
        {"name":"limo:sbom:scope","value":"Extracted release archive, compiled Rust graph and every shipped native executable and dynamic library. Consumer OS/drivers are outside the artifact scope."},
        {"name":"limo:occt:source-sha256","value":sdk["occt"]["sourceSha256"].as_str().unwrap_or("unknown")},
        {"name":"limo:occt:patches","value":serde_json::to_string(&sdk["occt"]["patches"])?}
    ]);
    if !sdk["vcpkg"].is_null() {
        cdx["metadata"]["properties"].as_array_mut().unwrap().push(json!({"name":"limo:occt:vcpkg-evidence","value":serde_json::to_string(&sdk["vcpkg"])?}));
    }
    let components = cdx["components"]
        .as_array_mut()
        .context("SBOM components")?;
    components.push(json!({"bom-ref":"limo-occt","type":"framework","name":"Open CASCADE Technology","version":sdk["occt"]["version"],
        "supplier":{"name":"Open Cascade SAS"},"purl":"pkg:generic/opencascade@7.9.3",
        "cpe":"cpe:2.3:a:opencascade:open_cascade_technology:7.9.3:*:*:*:*:*:*:*",
        "licenses":[{"expression":"LGPL-2.1-only WITH OCCT-exception-1.0"}],
        "externalReferences":[{"type":"distribution","url":sdk["occt"]["source"]}],
        "properties":[{"name":"limo:component:variant","value":"Checked-storage source patches recorded in the accompanying build metadata; no CVE remediation claim is made."}]}));
    let mut references = BTreeMap::new();
    for library in libraries {
        let reference = native_ref(library)?;
        references.insert(library.name.to_ascii_lowercase(), reference.clone());
        let mut component = json!({"bom-ref":reference,"type":library.kind,"name":library.package_name,"version":library.version,
            "hashes":[{"alg":"SHA-256","content":library.sha256}],
            "properties":[{"name":"limo:native:path","value":library.path},{"name":"limo:native:identification","value":library.identification}]});
        if library.producer != "unknown" {
            component["supplier"] = json!({"name":library.producer});
        }
        if let Some(purl) = &library.purl {
            component["purl"] = json!(purl);
        }
        if library.license != "NOASSERTION" {
            component["licenses"] = json!([{"expression":library.license}]);
        }
        components.push(component);
    }
    if !cdx["dependencies"].is_array() {
        cdx["dependencies"] = json!([]);
    }
    let dependencies = cdx["dependencies"].as_array_mut().unwrap();
    let mut contents = libraries
        .iter()
        .map(native_ref)
        .collect::<Result<Vec<_>>>()?;
    contents.extend([executable.into(), "limo-occt".into()]);
    dependencies.push(json!({"ref":root,"dependsOn":contents}));
    dependencies.push(json!({"ref":"limo-occt","dependsOn":libraries.iter().filter(|library|library.occt).map(native_ref).collect::<Result<Vec<_>>>()?}));
    for library in libraries {
        let imported = library
            .imports
            .iter()
            .filter_map(|import| {
                references.get(
                    &import
                        .replace('\\', "/")
                        .rsplit('/')
                        .next()
                        .unwrap_or(import)
                        .to_ascii_lowercase(),
                )
            })
            .collect::<BTreeSet<_>>();
        dependencies.push(json!({"ref":native_ref(library)?,"dependsOn":imported}));
    }
    // Inventory membership does not prove unknown producer/license fields or
    // the consumer's runtime. Do not imply full minimum-elements conformance.
    cdx["compositions"] = json!([{"aggregate":"incomplete","assemblies":[root]}]);
    Ok(())
}

pub(super) fn validate(cdx: &Value) -> Result<()> {
    ensure!(
        cdx["bomFormat"] == "CycloneDX" && cdx["specVersion"] == "1.6",
        "incorrect CycloneDX format"
    );
    let components = cdx["components"].as_array().context("missing inventory")?;
    ensure!(!components.is_empty(), "empty inventory");
    let mut refs = BTreeSet::new();
    refs.insert(
        cdx["metadata"]["component"]["bom-ref"]
            .as_str()
            .context("missing root reference")?,
    );
    for component in components {
        ensure!(
            refs.insert(
                component["bom-ref"]
                    .as_str()
                    .context("missing component reference")?
            ),
            "duplicate component reference"
        );
    }
    for dependency in cdx["dependencies"]
        .as_array()
        .context("missing dependency graph")?
    {
        ensure!(
            refs.contains(dependency["ref"].as_str().context("dependency reference")?),
            "unknown dependency reference"
        );
        for child in dependency["dependsOn"]
            .as_array()
            .context("dependency children")?
        {
            ensure!(
                refs.contains(child.as_str().context("dependency child")?),
                "dangling dependency reference"
            );
        }
    }
    Ok(())
}

pub(super) fn verify_native(cdx: &Value, libraries: &[Library]) -> Result<()> {
    let components = cdx["components"].as_array().context("missing components")?;
    ensure!(components.iter().any(|component| component["bom-ref"] == "limo-occt" && component["version"] == "7.9.3"), "OCCT component missing");
    for library in libraries {
        let reference = native_ref(library)?;
        let component = components
            .iter()
            .find(|component| component["bom-ref"] == reference)
            .context("bundled native file omitted from SBOM")?;
        ensure!(
            component["hashes"][0]["content"] == library.sha256,
            "native component hash mismatch"
        );
    }
    Ok(())
}

fn spdx_native_id(library: &Library) -> Result<String> {
    Ok(format!("SPDXRef-{}", native_ref(library)?))
}

/// Syft's conversion preserves Cargo identities but does not retain generic
/// native-file hashes. Explicitly restore those files and their loader graph.
pub(super) fn enrich_spdx(
    spdx: &mut Value,
    libraries: &[Library],
    archive: &str,
    archive_hash: &str,
    version: &str,
    revision: &str,
    target: &str,
) -> Result<()> {
    ensure!(spdx["spdxVersion"] == "SPDX-2.3", "incorrect SPDX format");
    let packages = spdx["packages"].as_array_mut().context("SPDX packages")?;
    // Replace converted native entries by exact file identity, independent of
    // the converter's synthesized SPDXID naming convention.
    packages.retain(|package| {
        !package["SPDXID"]
            .as_str()
            .is_some_and(|id| id.contains("limo-native-"))
    });
    for library in libraries {
        let mut package = json!({"SPDXID":spdx_native_id(library)?,"name":library.package_name,"versionInfo":library.version,
            "supplier":if library.producer == "unknown" { "NOASSERTION".into() } else { format!("Organization: {}",library.producer) },
            "downloadLocation":"NOASSERTION","filesAnalyzed":false,"licenseConcluded":"NOASSERTION",
            "licenseDeclared":library.license,"copyrightText":"NOASSERTION",
            "checksums":[{"algorithm":"SHA256","checksumValue":library.sha256}],
            "sourceInfo":format!("Packaged file: {}. {}",library.path,library.identification)});
        if let Some(purl) = &library.purl {
            package["externalRefs"] = json!([{"referenceCategory":"PACKAGE-MANAGER","referenceType":"purl","referenceLocator":purl}]);
        }
        packages.push(package);
    }
    let contents = packages
        .iter()
        .filter_map(|package| package["SPDXID"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let occt = packages
        .iter()
        .find(|package| {
            package["name"] == "Open CASCADE Technology" && package["versionInfo"] == "7.9.3"
        })
        .and_then(|package| package["SPDXID"].as_str())
        .context("OCCT lost during SPDX conversion")?
        .to_owned();
    packages.push(json!({"SPDXID":"SPDXRef-LimoReleaseArchive","name":archive,"versionInfo":version,
        "supplier":"Organization: Limo CAD project","downloadLocation":"NOASSERTION","filesAnalyzed":false,
        "licenseConcluded":"NOASSERTION","licenseDeclared":"LGPL-2.1-or-later","copyrightText":"NOASSERTION",
        "checksums":[{"algorithm":"SHA256","checksumValue":archive_hash}],
        "sourceInfo":format!("After build; extracted release archive; source revision {revision}; target {target}. External consumer OS/drivers and unknown producers/licenses remain outside the known inventory.")}));
    let relationships = spdx["relationships"]
        .as_array_mut()
        .context("SPDX relationships")?;
    // Remove edges to the replaced converter entries before restoring edges.
    relationships.retain(|edge| {
        !["spdxElementId", "relatedSpdxElement"].iter().any(|key| {
            edge[*key]
                .as_str()
                .is_some_and(|id| id.contains("limo-native-"))
        })
    });
    relationships.push(json!({"spdxElementId":"SPDXRef-DOCUMENT","relationshipType":"DESCRIBES","relatedSpdxElement":"SPDXRef-LimoReleaseArchive"}));
    for child in contents {
        relationships.push(json!({"spdxElementId":"SPDXRef-LimoReleaseArchive","relationshipType":"CONTAINS","relatedSpdxElement":child}));
    }
    let references = libraries
        .iter()
        .map(|library| Ok((library.name.to_ascii_lowercase(), spdx_native_id(library)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    for library in libraries {
        let reference = spdx_native_id(library)?;
        if library.occt {
            relationships.push(json!({"spdxElementId":occt,"relationshipType":"DEPENDS_ON","relatedSpdxElement":reference}));
        }
        for child in library
            .imports
            .iter()
            .filter_map(|import| {
                references.get(
                    &import
                        .replace('\\', "/")
                        .rsplit('/')
                        .next()
                        .unwrap()
                        .to_ascii_lowercase(),
                )
            })
            .collect::<BTreeSet<_>>()
        {
            relationships.push(json!({"spdxElementId":reference,"relationshipType":"DEPENDS_ON","relatedSpdxElement":child}));
        }
    }
    let creators = spdx["creationInfo"]["creators"]
        .as_array_mut()
        .context("SPDX creators")?;
    creators.extend([
        json!("Organization: Limo CAD project"),
        json!(format!(
            "Tool: cargo-auditable-{}",
            super::AUDITABLE_VERSION
        )),
        json!(format!("Tool: Limo-CAD-xtask-{version}")),
    ]);
    Ok(())
}

pub(super) fn verify_spdx(
    spdx: &Value,
    libraries: &[Library],
    archive: &str,
    archive_hash: &str,
) -> Result<()> {
    ensure!(spdx["spdxVersion"] == "SPDX-2.3", "incorrect SPDX format");
    let packages = spdx["packages"].as_array().context("SPDX packages")?;
    for item in libraries
        .iter()
        .map(|library| -> Result<_> {
            Ok((
                spdx_native_id(library)?,
                library.package_name.as_str(),
                library.sha256.as_str(),
            ))
        })
        .chain(std::iter::once(Ok((
            "SPDXRef-LimoReleaseArchive".into(),
            archive,
            archive_hash,
        ))))
    {
        let (id, name, hash) = item?;
        let package = packages
            .iter()
            .find(|package| package["SPDXID"] == id && package["name"] == name)
            .context("SPDX omits a native file or release archive")?;
        ensure!(
            package["checksums"]
                .as_array()
                .is_some_and(|checksums| checksums
                    .iter()
                    .any(|checksum| checksum["algorithm"] == "SHA256"
                        && checksum["checksumValue"] == hash)),
            "SPDX native/archive hash mismatch"
        );
    }
    ensure!(
        packages
            .iter()
            .any(|package| package["name"] == "Open CASCADE Technology"
                && package["versionInfo"] == "7.9.3"),
        "SPDX omits OCCT"
    );
    let mut ids = BTreeSet::from(["SPDXRef-DOCUMENT"]);
    for entry in packages
        .iter()
        .chain(spdx["files"].as_array().into_iter().flatten())
    {
        ensure!(
            ids.insert(
                entry["SPDXID"]
                    .as_str()
                    .context("SPDX component reference")?
            ),
            "duplicate SPDX reference"
        );
    }
    for edge in spdx["relationships"]
        .as_array()
        .context("SPDX relationships")?
    {
        for key in ["spdxElementId", "relatedSpdxElement"] {
            ensure!(
                ids.contains(edge[key].as_str().context("SPDX relationship reference")?),
                "dangling SPDX reference"
            );
        }
    }
    Ok(())
}

fn rust_references(cdx: &Value, spdx: &Value) -> Result<BTreeMap<String, String>> {
    let packages = spdx["packages"].as_array().context("SPDX packages")?;
    let mut references = BTreeMap::new();
    for component in cdx["components"]
        .as_array()
        .context("compiled components")?
    {
        let Some(purl) = component["purl"]
            .as_str()
            .filter(|purl| purl.starts_with("pkg:cargo/"))
        else {
            continue;
        };
        let reference = component["bom-ref"]
            .as_str()
            .context("compiled component reference")?;
        let candidates = packages
            .iter()
            .filter(|package| {
                package["name"] == component["name"]
                    && package["versionInfo"] == component["version"]
                    && package["externalRefs"].as_array().is_some_and(|refs| {
                        refs.iter().any(|entry| {
                            entry["referenceType"] == "purl" && entry["referenceLocator"] == purl
                        })
                    })
            })
            .collect::<Vec<_>>();
        // Syft distinguishes equal name/version packages by their original
        // cataloger identity. Resolve ambiguous PURLs by that retained suffix.
        let normalized = reference
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect::<String>();
        let package = if candidates.len() == 1 {
            candidates[0]
        } else {
            candidates
                .into_iter()
                .find(|package| {
                    package["SPDXID"]
                        .as_str()
                        .is_some_and(|id| id.ends_with(&normalized))
                })
                .context("SPDX lost or ambiguously mapped a compiled Cargo identity")?
        };
        references.insert(
            reference.into(),
            package["SPDXID"]
                .as_str()
                .context("SPDX Cargo reference")?
                .into(),
        );
    }
    Ok(references)
}

pub(super) fn preserve_rust_graph(cdx: &Value, spdx: &mut Value) -> Result<()> {
    let references = rust_references(cdx, spdx)?;
    let edges = spdx["relationships"]
        .as_array_mut()
        .context("SPDX relationships")?;
    for dependency in cdx["dependencies"].as_array().context("compiled graph")? {
        let Some(parent) = dependency["ref"].as_str().and_then(|id| references.get(id)) else {
            continue;
        };
        for child in dependency["dependsOn"]
            .as_array()
            .context("compiled dependencies")?
        {
            let child = references
                .get(child.as_str().context("compiled dependency reference")?)
                .context("compiled Rust dependency missing from SPDX")?;
            let edge = json!({"spdxElementId":parent,"relationshipType":"DEPENDS_ON","relatedSpdxElement":child});
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
    }
    verify_rust_graph(cdx, spdx)
}

pub(super) fn verify_rust_graph(cdx: &Value, spdx: &Value) -> Result<()> {
    let references = rust_references(cdx, spdx)?;
    let edges = spdx["relationships"]
        .as_array()
        .context("SPDX relationships")?;
    for dependency in cdx["dependencies"].as_array().context("compiled graph")? {
        let Some(parent) = dependency["ref"].as_str().and_then(|id| references.get(id)) else {
            continue;
        };
        for child in dependency["dependsOn"]
            .as_array()
            .context("compiled dependencies")?
        {
            let child = references
                .get(child.as_str().context("compiled dependency reference")?)
                .context("compiled Rust dependency missing from SPDX")?;
            ensure!(
                edges.iter().any(|edge| edge["spdxElementId"] == *parent
                    && edge["relatedSpdxElement"] == *child
                    && edge["relationshipType"] == "DEPENDS_ON"),
                "compiled Rust dependency edge missing from SPDX"
            );
        }
    }
    Ok(())
}
