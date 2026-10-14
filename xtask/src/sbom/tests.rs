use super::*;

#[test]
fn source_inventory_and_release_identity_cover_all_owned_lockfiles() {
    let root = crate::release_tooling::root();
    let owned = output(Command::new("git").current_dir(root).args([
        "ls-files",
        "Cargo.lock",
        "desktop/Cargo.lock",
        "mcp-server/Cargo.lock",
        "experiments/*/Cargo.lock",
    ]))
    .unwrap();
    let mut tracked: Vec<_> = owned.lines().collect();
    tracked.sort_unstable();
    let mut inventoried = LOCKFILES.to_vec();
    inventoried.sort_unstable();
    assert_eq!(tracked, inventoried);
    let identity = input_hashes(root).unwrap();
    for path in tracked {
        assert!(root.join(path).is_file());
        assert!(identity.contains_key(path), "release identity omits {path}");
    }
}

#[test]
fn release_input_hashes_match_windows_and_unix_checkout_settings() {
    let root = crate::release_tooling::root();
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    fs::create_dir(&source).unwrap();
    let mut paths: Vec<_> = input_hashes(root).unwrap().into_keys().collect();
    if !paths.iter().any(|path| path == ".gitattributes") {
        paths.push(".gitattributes".into());
    }
    // Start with the same LF source blobs, then exercise Git's real checkout
    // filters for every tracked release input, rather than normalizing hashes.
    for path in paths {
        let destination = source.join(&path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        let contents = fs::read_to_string(root.join(path)).unwrap();
        fs::write(destination, contents.replace("\r\n", "\n")).unwrap();
    }
    let hooks = temporary.path().join("empty-hooks");
    fs::create_dir(&hooks).unwrap();
    let git = || {
        let mut command = Command::new("git");
        command
            .current_dir(&source)
            .args(["-c", "commit.gpgsign=false", "-c", "user.name=SBOM fixture"])
            .args(["-c", "user.email=sbom-fixture@example.invalid", "-c"])
            .arg(format!("core.hooksPath={}", hooks.display()));
        command
    };
    execute(git().args(["init", "--quiet"])).unwrap();
    execute(git().args(["add", "--all"])).unwrap();
    execute(git().args(["commit", "--quiet", "-m", "Release input fixture"])).unwrap();
    let checkouts: Vec<_> = ["true", "false"]
        .into_iter()
        .map(|autocrlf| {
            let checkout = temporary.path().join(format!("autocrlf-{autocrlf}"));
            execute(
                git()
                    .args(["clone", "--quiet", "--no-local", "--config"])
                    .arg(format!("core.autocrlf={autocrlf}"))
                    .arg(&source)
                    .arg(&checkout),
            )
            .unwrap();
            checkout
        })
        .collect();
    let windows = input_hashes(&checkouts[0]).unwrap();
    let unix = input_hashes(&checkouts[1]).unwrap();
    assert_eq!(windows, unix);
    assert!(windows.contains_key(".gitattributes"));
    assert_eq!(
        fs::read(checkouts[0].join("VERSION")).unwrap(),
        fs::read(checkouts[1].join("VERSION")).unwrap()
    );
    let manifest = checkouts[0].join("Cargo.toml");
    let mut contents = fs::read_to_string(&manifest).unwrap();
    contents.push_str("\n# Substantive input edit must still change release identity.\n");
    fs::write(manifest, contents).unwrap();
    assert_ne!(input_hashes(&checkouts[0]).unwrap(), unix);
}

fn cdx() -> Value {
    json!({"bomFormat":"CycloneDX","specVersion":"1.6","version":1,
        "metadata":{"component":{"bom-ref":"archive","type":"file","name":"package","version":"0.2.2"}},
        "components":[{"bom-ref":"rust-main","type":"library","name":"limo-cad","version":"0.2.2",
            "purl":"pkg:cargo/limo-cad@0.2.2","properties":[{"name":"syft:package:foundBy","value":"cargo-auditable-binary-cataloger"}]}],
        "dependencies":[{"ref":"rust-main","dependsOn":[]}]})
}

fn library(name: &str, occt: bool) -> native::Library {
    native::Library {
        path: format!("usr/lib/{name}"),
        name: name.into(),
        package_name: name.into(),
        purl: None,
        kind: "library".into(),
        sha256: "a".repeat(64),
        version: if occt { "8.0.1" } else { "unknown" }.into(),
        producer: if occt { "Open Cascade SAS" } else { "unknown" }.into(),
        license: if occt {
            "LGPL-2.1-only WITH OCCT-exception-1.0"
        } else {
            "NOASSERTION"
        }
        .into(),
        identification: "fixture".into(),
        imports: Vec::new(),
        occt,
    }
}

#[test]
fn release_inventory_requires_an_embedded_compiled_graph_not_a_lockfile_list() {
    let mut report = cdx();
    assert_eq!(
        documents::auditable_root(&report, "0.2.2").unwrap(),
        "rust-main"
    );
    assert!(documents::auditable_root(&report, "0.2.3").is_err());
    report["components"][0]["properties"][0]["value"] = json!("rust-cargo-lock-cataloger");
    assert!(documents::auditable_root(&report, "0.2.2").is_err());
}

#[test]
fn native_inventory_keeps_unknowns_file_hashes_and_real_loader_relationships() {
    let mut report = cdx();
    let mut kernel = library("libTKernel.so.8.0.1", true);
    kernel.imports = vec!["libfreetype.so.6".into(), "libc.so.6".into()];
    let libraries = vec![kernel, library("libfreetype.so.6", false)];
    let evidence = crate::occt_sdk::sbom_evidence("FREETYPE_VERSION_STRING=2.14.0\n").unwrap();
    documents::add_release(
        &mut report,
        &libraries,
        &evidence,
        "0.2.2",
        &"b".repeat(40),
        "x86_64-unknown-linux-gnu",
        &"c".repeat(64),
        "rust-main",
    )
    .unwrap();
    documents::validate(&report).unwrap();
    documents::verify_native(&report, &libraries).unwrap();
    let reference = documents::native_ref(&libraries[0]).unwrap();
    let dependency = report["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|dep| dep["ref"] == reference)
        .unwrap();
    assert_eq!(dependency["dependsOn"].as_array().unwrap().len(), 1);
    let unknown = report["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|component| component["name"] == "libfreetype.so.6")
        .unwrap();
    assert_eq!(unknown["version"], "unknown");
    assert!(unknown["supplier"].is_null());
    assert!(unknown["licenses"].is_null());
    report["components"]
        .as_array_mut()
        .unwrap()
        .retain(|component| component["name"] != "libfreetype.so.6");
    assert!(documents::verify_native(&report, &libraries).is_err());
    assert!(documents::validate(&report).is_err());
}

#[test]
fn broken_and_duplicate_references_are_rejected() {
    let mut report = cdx();
    report["dependencies"][0]["dependsOn"] = json!(["missing"]);
    assert!(documents::validate(&report).is_err());
    let mut report = cdx();
    let duplicate = report["components"][0].clone();
    report["components"].as_array_mut().unwrap().push(duplicate);
    assert!(documents::validate(&report).is_err());
}

#[test]
fn sdk_evidence_identifies_patches_separately_from_the_upstream_release() {
    let report = crate::occt_sdk::sbom_evidence("FREETYPE_VERSION_STRING=2.14.0\nCMAKE_CXX_COMPILER_ID=Clang\nCMAKE_CXX_COMPILER_VERSION=19.1\n").unwrap();
    assert_eq!(report["occt"]["version"], "8.0.1");
    assert_eq!(report["occt"]["patches"].as_array().unwrap().len(), 1);
    assert_eq!(
        report["occt"]["patches"][0]["path"],
        "src/FoundationClasses/TKMath/math/math_DoubleTab.hxx"
    );
    assert_eq!(report["dependencies"][0]["version"], "2.14.0");
    assert_ne!(
        report["occt"]["patches"][0]["sha256"],
        report["occt"]["patches"][0]["originalSha256"]
    );
    assert_eq!(report["compiler"]["id"], "Clang");
}

#[test]
fn zip_extraction_rejects_traversal_and_does_not_write_outside_the_payload() {
    let temporary = tempfile::tempdir().unwrap();
    let archive = temporary.path().join("bad.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    zip.start_file("../escaped", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"unsafe").unwrap();
    zip.finish().unwrap();
    assert!(unpack(&archive, &temporary.path().join("payload")).is_err());
    assert!(!temporary.path().join("escaped").exists());
}

#[test]
fn unsupported_or_duplicate_options_are_rejected() {
    for args in [
        vec!["--target"],
        vec!["--artifact"],
        vec!["--github-output", "--github-output"],
        vec!["--directory", "one", "--directory", "two"],
    ] {
        assert!(Options::parse(args.into_iter().map(str::to_owned)).is_err());
    }
}

#[test]
fn spdx_conversion_retains_native_identities_hashes_and_loader_edges() {
    let mut kernel = library("libTKernel.so.8.0", true);
    kernel.imports = vec!["libfreetype.so.6".into()];
    let mut freetype = library("libfreetype.so.6", false);
    freetype.package_name = "freetype".into();
    freetype.version = "2.14.0".into();
    freetype.purl = Some("pkg:generic/freetype@2.14.0".into());
    let libraries = vec![kernel, freetype];
    let mut report = json!({"spdxVersion":"SPDX-2.3","creationInfo":{"creators":["Tool: syft-1.54.1"]},
        "packages":[{"SPDXID":"SPDXRef-OCCT","name":"Open CASCADE Technology","versionInfo":"8.0.1"},
            {"SPDXID":"SPDXRef-converted-limo-native-placeholder","name":"converter native file without hash"}],
        "relationships":[{"spdxElementId":"SPDXRef-OCCT","relatedSpdxElement":"SPDXRef-converted-limo-native-placeholder","relationshipType":"DEPENDS_ON"}]});
    documents::enrich_spdx(
        &mut report,
        &libraries,
        "package.deb",
        &"b".repeat(64),
        "0.2.2",
        &"c".repeat(40),
        "x86_64-unknown-linux-gnu",
    )
    .unwrap();
    documents::verify_spdx(&report, &libraries, "package.deb", &"b".repeat(64)).unwrap();
    assert!(report["packages"]
        .as_array()
        .unwrap()
        .iter()
        .all(|package| package["SPDXID"] != "SPDXRef-converted-limo-native-placeholder"));
    assert!(report["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edge| edge["spdxElementId"]
            == format!("SPDXRef-{}", documents::native_ref(&libraries[0]).unwrap())
            && edge["relatedSpdxElement"]
                == format!("SPDXRef-{}", documents::native_ref(&libraries[1]).unwrap())));
    let package = report["packages"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|package| package["name"] == "freetype")
        .unwrap();
    assert_eq!(
        package["externalRefs"][0]["referenceLocator"],
        "pkg:generic/freetype@2.14.0"
    );
    package["checksums"][0]["checksumValue"] = json!("d".repeat(64));
    assert!(documents::verify_spdx(&report, &libraries, "package.deb", &"b".repeat(64)).is_err());
}

fn sealed_fixture(directory: &Path) -> Evidence {
    let root = crate::release_tooling::root();
    let artifact = "fixture.zip";
    fs::write(directory.join(artifact), b"finished archive").unwrap();
    let artifact_sha256 = crate::hash::file(&directory.join(artifact)).unwrap();
    let libraries = vec![
        library("libTKernel.so.8.0", true),
        library("libTKDESTEP.so.8.0", true),
    ];
    let native_sdk = crate::occt_sdk::sbom_evidence("").unwrap();
    let version = crate::release_tooling::version::read(root).unwrap();
    let (source_revision, source_modified) = revision(root).unwrap();
    let mut cdx = cdx();
    cdx["components"][0]["version"] = json!(version);
    cdx["components"][0]["purl"] = json!(format!("pkg:cargo/limo-cad@{version}"));
    documents::add_release(
        &mut cdx,
        &libraries,
        &native_sdk,
        &version,
        &source_revision,
        "x86_64-unknown-linux-gnu",
        &artifact_sha256,
        "rust-main",
    )
    .unwrap();
    let mut spdx = json!({"spdxVersion":"SPDX-2.3","creationInfo":{"creators":[]},"packages":[{"SPDXID":"SPDXRef-OCCT","name":"Open CASCADE Technology","versionInfo":"8.0.1"}],"relationships":[]});
    spdx["packages"].as_array_mut().unwrap().push(json!({"SPDXID":"SPDXRef-RustMain","name":"limo-cad","versionInfo":version,"externalRefs":[{"referenceType":"purl","referenceLocator":format!("pkg:cargo/limo-cad@{version}")}]}));
    documents::enrich_spdx(
        &mut spdx,
        &libraries,
        artifact,
        &artifact_sha256,
        &version,
        &source_revision,
        "x86_64-unknown-linux-gnu",
    )
    .unwrap();
    let cdx_name = format!("{artifact}.sbom.cdx.json");
    let spdx_name = format!("{artifact}.sbom.spdx.json");
    write_json(&directory.join(&cdx_name), &cdx).unwrap();
    write_json(&directory.join(&spdx_name), &spdx).unwrap();
    let evidence = Evidence {
        schema_version: 1,
        generation_context: "after build; fixture".into(),
        version,
        source_revision,
        source_modified,
        target: "x86_64-unknown-linux-gnu".into(),
        artifact: artifact.into(),
        artifact_sha256,
        syft_version: SYFT_VERSION.into(),
        cargo_auditable_version: AUDITABLE_VERSION.into(),
        input_sha256: input_hashes(root).unwrap(),
        report_sha256: BTreeMap::from([
            (
                cdx_name,
                crate::hash::file(&directory.join(format!("{artifact}.sbom.cdx.json"))).unwrap(),
            ),
            (
                spdx_name,
                crate::hash::file(&directory.join(format!("{artifact}.sbom.spdx.json"))).unwrap(),
            ),
        ]),
        native_sdk,
        native_libraries: libraries,
        unknown_information: Vec::new(),
    };
    write_json(
        &directory.join(format!("{artifact}.sbom.metadata.json")),
        &evidence,
    )
    .unwrap();
    let sums = ["cdx.json", "spdx.json", "metadata.json"]
        .iter()
        .map(|suffix| {
            let name = format!("{artifact}.sbom.{suffix}");
            format!(
                "{}  {name}\n",
                crate::hash::file(&directory.join(&name)).unwrap()
            )
        })
        .collect::<String>();
    fs::write(
        directory.join(format!("{artifact}.sbom.SHA256SUMS.txt")),
        sums,
    )
    .unwrap();
    evidence
}

#[test]
fn verification_rejects_archive_report_and_input_tampering() {
    let directory = tempfile::tempdir().unwrap();
    let root = crate::release_tooling::root();
    let mut evidence = sealed_fixture(directory.path());
    verify_one(root, directory.path(), &evidence).unwrap();
    evidence
        .input_sha256
        .insert("Cargo.lock".into(), "bad".into());
    assert!(verify_one(root, directory.path(), &evidence)
        .unwrap_err()
        .to_string()
        .contains("inputs have changed"));
    let evidence = sealed_fixture(directory.path());
    fs::write(
        directory.path().join(&evidence.artifact),
        b"substituted package",
    )
    .unwrap();
    assert!(verify_one(root, directory.path(), &evidence)
        .unwrap_err()
        .to_string()
        .contains("archive hash mismatch"));
    let evidence = sealed_fixture(directory.path());
    fs::write(
        directory
            .path()
            .join(format!("{}.sbom.spdx.json", evidence.artifact)),
        b"{}",
    )
    .unwrap();
    assert!(verify_one(root, directory.path(), &evidence)
        .unwrap_err()
        .to_string()
        .contains("report hash mismatch"));
    let evidence = sealed_fixture(directory.path());
    fs::write(
        directory
            .path()
            .join(format!("{}.sbom.SHA256SUMS.txt", evidence.artifact)),
        b"forged checksum list",
    )
    .unwrap();
    assert!(verify_one(root, directory.path(), &evidence)
        .unwrap_err()
        .to_string()
        .contains("checksum manifest mismatch"));
}

/// Exercises extraction, native identity checks, Syft's compiled graph and both
/// export formats without building or launching the desktop. The OCCT-named
/// libraries are deliberately synthetic parser/packaging fixtures.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires pinned Syft/cargo-auditable tools and the macOS compiler"]
fn final_archive_fixture_preserves_compiled_graph_native_files_and_both_formats() {
    let root = crate::release_tooling::root();
    let version = crate::release_tooling::version::read(root).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let project = temporary.path().join("rust-fixture");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("Cargo.toml"),format!("[package]\nname = \"limo-cad\"\nversion = \"{version}\"\nedition = \"2024\"\n[dependencies]\nsbom-fixture-dependency = {{ path = \"dependency\" }}\n[workspace]\n[profile.release]\nstrip = \"symbols\"\n")).unwrap();
    fs::create_dir_all(project.join("dependency/src")).unwrap();
    fs::write(project.join("dependency/Cargo.toml"),"[package]\nname = \"sbom-fixture-dependency\"\nversion = \"0.0.1\"\nedition = \"2024\"\nlicense = \"MIT\"\n").unwrap();
    fs::write(
        project.join("dependency/src/lib.rs"),
        "pub fn value() -> u8 { 1 }\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.rs"),
        "fn main() { println!(\"{}\", sbom_fixture_dependency::value()); }\n",
    )
    .unwrap();
    execute(
        Command::new("cargo")
            .current_dir(&project)
            .args(["generate-lockfile", "--offline"]),
    )
    .unwrap();
    execute(
        Command::new("cargo")
            .current_dir(&project)
            .args(["auditable", "build", "--release", "--locked", "--offline"])
            .env("CARGO_TARGET_DIR", project.join("target")),
    )
    .unwrap();
    let sdk = temporary.path().join("sdk");
    crate::occt_cache::prepare(&sdk, "fixture").unwrap();
    fs::create_dir(sdk.join("include")).unwrap();
    fs::create_dir(sdk.join("lib")).unwrap();
    fs::write(
        sdk.join("include/Standard_Version.hxx"),
        "#define OCC_VERSION_COMPLETE \"8.0.1\"\n",
    )
    .unwrap();
    fs::write(
        sdk.join("include/math_DoubleTab.hxx"),
        include_bytes!("../../../native/occt-overlay/opencascade/math_DoubleTab.hxx"),
    )
    .unwrap();
    let source = temporary.path().join("native.c");
    for (name, body) in [
        (
            "libTKernel.8.0.dylib",
            "int kernel_fixture(void) { return 1; }",
        ),
        (
            "libTKDESTEP.8.0.dylib",
            "int step_fixture(void) { return 2; }",
        ),
        (
            "libunidentified.1.dylib",
            "int unknown_fixture(void) { return 3; }",
        ),
    ] {
        fs::write(&source, body).unwrap();
        execute(
            Command::new("clang")
                .args(["-dynamiclib", "-Wl,-install_name"])
                .arg(format!("-Wl,@rpath/{name}"))
                .arg(&source)
                .arg("-o")
                .arg(sdk.join("lib").join(name)),
        )
        .unwrap();
    }
    write_json(
        &sdk.join(".limo-sdk-sbom.json"),
        &crate::occt_sdk::sbom_evidence("").unwrap(),
    )
    .unwrap();
    crate::occt_cache::publish(&sdk, "fixture").unwrap();
    let archive = temporary.path().join("Limo-CAD-fixture.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    let mut files = vec![("limo-cad".into(), project.join("target/release/limo-cad"))];
    for entry in fs::read_dir(sdk.join("lib")).unwrap() {
        let path = entry.unwrap().path();
        files.push((
            format!("lib/{}", path.file_name().unwrap().to_str().unwrap()),
            path,
        ));
    }
    for (name, path) in files {
        zip.start_file(
            format!("Limo-CAD/{name}"),
            zip::write::SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
        zip.write_all(&fs::read(path).unwrap()).unwrap();
    }
    zip.finish().unwrap();
    release(
        root,
        Options {
            artifact: Some(archive.clone()),
            sdk: Some(sdk),
            target: Some("aarch64-apple-darwin".into()),
            ..Options::default()
        },
    )
    .unwrap();
    let metadata = json_file(
        &temporary
            .path()
            .join("Limo-CAD-fixture.zip.sbom.metadata.json"),
    )
    .unwrap();
    assert_eq!(metadata["native_libraries"].as_array().unwrap().len(), 4);
    let cdx = json_file(&temporary.path().join("Limo-CAD-fixture.zip.sbom.cdx.json")).unwrap();
    let compiled_root = documents::auditable_root(&cdx, &version).unwrap();
    let dependency = cdx["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|component| component["name"] == "sbom-fixture-dependency")
        .unwrap();
    assert!(cdx["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edge| edge["ref"] == compiled_root
            && edge["dependsOn"]
                .as_array()
                .unwrap()
                .contains(&dependency["bom-ref"])));
    let spdx = json_file(&temporary.path().join("Limo-CAD-fixture.zip.sbom.spdx.json")).unwrap();
    let dependency = spdx["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == "sbom-fixture-dependency")
        .unwrap();
    assert!(spdx["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edge| edge["relationshipType"] == "DEPENDS_ON"
            && edge["relatedSpdxElement"] == dependency["SPDXID"]));
    assert!(metadata["unknown_information"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value.as_str().unwrap().contains("libunidentified")));
    if let Some(destination) = env::var_os("LIMO_SBOM_FIXTURE_OUTPUT") {
        let directory = PathBuf::from(destination);
        fs::create_dir_all(&directory).unwrap();
        for entry in fs::read_dir(temporary.path()).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("Limo-CAD-fixture.zip")
            {
                fs::copy(entry.path(), directory.join(entry.file_name())).unwrap();
            }
        }
    }
    fs::write(&archive, "tampered fixture").unwrap();
    let evidence: Evidence = serde_json::from_value(metadata).unwrap();
    assert!(verify_one(root, temporary.path(), &evidence).is_err());
}
