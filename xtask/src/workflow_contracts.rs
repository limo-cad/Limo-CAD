//! Preserve release/ABI/publication guard contracts without a Node test runner.
use anyhow::{ensure, Context, Result};
use regex::Regex;
use serde_yaml_ng::Value;
use std::fs;
fn read(file: &str) -> String {
    fs::read_to_string(crate::release_tooling::root().join(file))
        .unwrap()
        .replace("\r\n", "\n")
}
fn job(source: &str, id: &str) -> String {
    let prefix = format!("  {id}:\n");
    let rest = source
        .split_once(&prefix)
        .unwrap_or_else(|| panic!("missing job {id}"))
        .1;
    let end = Regex::new(r"(?m)^  [\w-]+:\n")
        .unwrap()
        .find(rest)
        .map_or(rest.len(), |m| m.start());
    rest[..end].into()
}
fn matches(text: &str, pattern: &str) {
    assert!(
        Regex::new(pattern).unwrap().is_match(text),
        "missing contract {pattern}"
    );
}
fn ordered(text: &str, first: &str, second: &str) {
    assert!(
        text.find(first).unwrap() < text.find(second).unwrap(),
        "{first} must precede {second}"
    );
}

fn workflow(file: &str) -> Value {
    serde_yaml_ng::from_str(&read(file)).unwrap_or_else(|error| panic!("invalid {file}: {error}"))
}

fn uses_action(step: &Value, action: &str) -> bool {
    step["uses"].as_str().is_some_and(|uses| {
        uses.split_once('@').is_some_and(|(name, revision)| {
            name == action
                && revision.len() == 40
                && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    })
}

#[test]
fn external_actions_are_immutable_and_checkouts_do_not_retain_credentials() {
    fn check(value: &Value, path: &std::path::Path) {
        match value {
            Value::Mapping(mapping) => {
                if let Some(uses) = value["uses"].as_str() {
                    if !uses.starts_with("./") {
                        let (action, _) = uses.split_once('@').expect("action must have revision");
                        assert!(
                            uses_action(value, action),
                            "mutable action in {}",
                            path.display()
                        );
                        if action == "actions/checkout" {
                            assert_eq!(
                                value["with"]["persist-credentials"].as_bool(),
                                Some(false),
                                "checkout retains credentials in {}",
                                path.display()
                            );
                        }
                    }
                }
                for child in mapping.values() {
                    check(child, path);
                }
            }
            Value::Sequence(values) => {
                for child in values {
                    check(child, path);
                }
            }
            _ => {}
        }
    }
    fn walk(path: &std::path::Path) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path);
            } else if matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("yml" | "yaml")
            ) {
                let value: Value =
                    serde_yaml_ng::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
                check(&value, &path);
            }
        }
    }
    walk(&crate::release_tooling::root().join(".github"));
}

#[test]
fn artifact_identity_rejects_mutable_revisions_and_wrong_actions() {
    for uses in [
        "actions/download-artifact@v8",
        "actions/download-artifact@main",
        "actions/download-artifact@123",
        "other/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333",
    ] {
        let step: Value = serde_yaml_ng::from_str(&format!("uses: {uses}")).unwrap();
        assert!(!uses_action(&step, "actions/download-artifact"));
    }
    let step: Value = serde_yaml_ng::from_str(
        "uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333",
    )
    .unwrap();
    assert!(uses_action(&step, "actions/download-artifact"));
}

#[test]
fn knowledge_failures_stop_native_producers_before_sdk_and_package_work() {
    let mcp = workflow(".github/workflows/mcp-server.yml");
    let packages = workflow(".github/workflows/desktop-packages.yml");
    for source in [&mcp, &packages] {
        let preflight = executable_job(source, "knowledge_preflight").unwrap();
        assert_eq!(
            preflight["uses"].as_str(),
            Some("./.github/workflows/knowledge-preflight.yml")
        );
    }
    for (source, id) in [
        (&mcp, "prepare-windows-sdk"),
        (&mcp, "mcp-linux"),
        (&packages, "classify_changes"),
    ] {
        let producer = executable_job(source, id).unwrap();
        assert_eq!(producer["needs"].as_str(), Some("knowledge_preflight"));
        assert!(
            producer["if"].is_null(),
            "producer must not bypass failed preflight"
        );
    }
    let source = workflow(".github/workflows/knowledge-preflight.yml");
    let job = executable_job(&source, "knowledge").unwrap();
    let steps = executable_steps(job).unwrap();
    let checkout = steps
        .iter()
        .find(|step| uses_action(step, "actions/checkout"))
        .unwrap();
    assert_eq!(checkout["with"]["fetch-depth"].as_u64(), Some(0));
    let validation = steps
        .iter()
        .find(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("knowledge check"))
        })
        .unwrap();
    requires_command(validation, "cargo run --locked -p xtask -- knowledge check").unwrap();
    requires_command(
        validation,
        "cargo run --locked -p xtask -- knowledge index --check",
    )
    .unwrap();
}

fn executable_job<'a>(source: &'a Value, id: &str) -> Result<&'a Value> {
    let job = &source["jobs"][id];
    ensure!(job.is_mapping(), "missing executable job {id}");
    require_failure_propagation(job)?;
    if let Some(steps) = job["steps"].as_sequence() {
        for step in steps {
            require_failure_propagation(step)?;
        }
    }
    Ok(job)
}

fn require_failure_propagation(value: &Value) -> Result<()> {
    ensure!(
        value["continue-on-error"].is_null() || value["continue-on-error"].as_bool() == Some(false),
        "acceptance failure must not be converted to success"
    );
    ensure!(
        value["if"].as_bool() != Some(false)
            && value["if"].as_str() != Some("false")
            && value["if"].as_str() != Some("${{ false }}"),
        "acceptance must not be disabled"
    );
    Ok(())
}

fn executable_steps(job: &Value) -> Result<&[Value]> {
    job["steps"]
        .as_sequence()
        .map(Vec::as_slice)
        .context("missing executable steps")
}

fn named_step<'a>(job: &'a Value, name: &str) -> Result<(usize, &'a Value)> {
    let matches = executable_steps(job)?
        .iter()
        .enumerate()
        .filter(|(_, step)| step["name"].as_str() == Some(name))
        .collect::<Vec<_>>();
    ensure!(matches.len() == 1, "expected one executable step {name}");
    require_failure_propagation(matches[0].1)?;
    Ok(matches[0])
}

fn requires_command(step: &Value, command: &str) -> Result<()> {
    let run = step["run"]
        .as_str()
        .context("missing executable run command")?;
    ensure!(
        run.lines().map(str::trim).any(|line| {
            line == command
                || line
                    .strip_prefix(command)
                    .is_some_and(|tail| tail.starts_with(' '))
        }),
        "missing executable command {command}"
    );
    Ok(())
}

fn needs(job: &Value) -> Result<Vec<&str>> {
    if let Some(single) = job["needs"].as_str() {
        return Ok(vec![single]);
    }
    job["needs"]
        .as_sequence()
        .context("missing dependencies")?
        .iter()
        .map(|value| value.as_str().context("invalid dependency"))
        .collect()
}

fn check_package_gates(source: &Value) -> Result<()> {
    ensure!(
        executable_job(source, "version_preflight")?["uses"].as_str()
            == Some("./.github/workflows/version-guard.yml"),
        "packages require the reusable version gate"
    );
    for (id, platform) in [
        ("build-windows-portable", "windows"),
        ("build-linux-ubuntu", "linux"),
        ("build-linux-appimage", "linux"),
        ("build-macos-apple-silicon", "macos"),
    ] {
        let job = executable_job(source, id)?;
        ensure!(
            needs(job)? == ["classify_changes", "version_preflight"],
            "{id} must await classification and version validation"
        );
        ensure!(
            job["if"].as_str()
                == Some(
                    format!("needs.classify_changes.outputs.{platform}_should_build == 'true'")
                        .as_str()
                ),
            "{id} must not override failed dependencies"
        );
    }
    let verification = executable_job(source, "verify-linux-appimage")?;
    ensure!(
        needs(verification)? == ["classify_changes", "build-linux-appimage"],
        "new-host AppImage verification must use the successful build"
    );
    ensure!(
        verification["if"].as_str()
            == Some("needs.classify_changes.outputs.linux_should_build == 'true'"),
        "new-host verification must not override failed dependencies"
    );
    let publish = executable_job(source, "publish_release")?;
    ensure!(
        publish["if"].as_str()
            == Some("github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')"),
        "publication is release-tag-only"
    );
    ensure!(
        needs(publish)?
            == [
                "build-windows-portable",
                "build-linux-ubuntu",
                "build-linux-appimage",
                "verify-linux-appimage",
                "build-macos-apple-silicon",
                "release_security"
            ],
        "publication must await every build and the newer-host AppImage check"
    );
    let (tag_index, tag) = named_step(
        publish,
        "Refuse a release tag that does not name VERSION on main",
    )?;
    requires_command(tag, "cargo xtask check-release-tag")?;
    ensure!(
        tag["if"].is_null(),
        "release tag validation cannot be conditional"
    );
    let (download_index, download) = named_step(publish, "Download this run's packages")?;
    ensure!(
        download["if"].is_null(),
        "package download cannot be conditional"
    );
    ensure!(
        uses_action(download, "actions/download-artifact"),
        "missing package artifact download"
    );
    ensure!(
        download["with"]["merge-multiple"].as_bool() == Some(false),
        "keep package artifacts separate"
    );
    for key in ["run-id", "repository", "github-token"] {
        ensure!(
            download["with"][key].is_null(),
            "packages must come from this run, not {key}"
        );
    }
    let (checksum_index, checksum) =
        named_step(publish, "Verify every package against its checksum")?;
    ensure!(checksum["if"].is_null(), "checksums cannot be conditional");
    requires_command(checksum, "test \"$checked\" -eq 5")?;
    let (publish_index, publication) =
        named_step(publish, "Publish packages, checksums and notes")?;
    ensure!(
        publication["if"].is_null(),
        "publication cannot bypass earlier steps"
    );
    requires_command(publication, "test \"$uploaded\" -eq 41")?;
    ensure!(
        tag_index < download_index
            && download_index < checksum_index
            && checksum_index < publish_index,
        "validate tag, download and verify before publishing"
    );
    Ok(())
}

fn check_release_sbom_gates(source: &Value) -> Result<()> {
    for (id, upload, glob) in [
        (
            "build-windows-portable",
            "Upload portable release",
            "desktop/target/${{ matrix.rust_target }}/release/bundle/portable/*.zip",
        ),
        (
            "build-linux-ubuntu",
            "Upload Ubuntu release",
            "desktop/target/release/bundle/deb/*.deb",
        ),
        (
            "build-linux-appimage",
            "Upload AppImage release",
            "desktop/target/release/bundle/appimage/*.AppImage",
        ),
        (
            "build-macos-apple-silicon",
            "Upload Apple silicon release",
            "desktop/target/release/bundle/dmg/*.dmg",
        ),
    ] {
        let builder = executable_job(source, id)?;
        ensure!(
            builder["permissions"]["contents"].as_str() == Some("read")
                && builder["permissions"]["id-token"].as_str() == Some("write")
                && builder["permissions"]["attestations"].as_str() == Some("write"),
            "builders require scoped signing permissions"
        );
        let (setup_index, setup) = named_step(builder, "Set up release SBOM tools")?;
        ensure!(
            setup["uses"].as_str() == Some("./.github/actions/setup-release-sbom")
                && setup["if"].is_null(),
            "all packages require auditable build tools"
        );
        let (inventory_index, inventory) =
            named_step(builder, "Generate release SBOMs and tag attestations")?;
        ensure!(
            inventory["uses"].as_str() == Some("./.github/actions/release-sbom")
                && inventory["if"].is_null()
                && inventory["with"]["artifact"].as_str() == Some(glob),
            "inventory the actual final archive unconditionally"
        );
        let (upload_index, upload) = named_step(builder, upload)?;
        let paths = upload["with"]["path"]
            .as_str()
            .context("release upload paths")?;
        ensure!(
            paths
                .lines()
                .any(|line| line.trim() == format!("{glob}.sbom.*"))
                && paths
                    .lines()
                    .any(|line| line.trim() == format!("{glob}.provenance.sigstore.json")),
            "upload inventories and portable signatures with each package"
        );
        ensure!(
            setup_index < inventory_index && inventory_index < upload_index,
            "generate inventories before package publication"
        );
    }
    let scanner = executable_job(source, "release_security")?;
    ensure!(
        needs(scanner)?
            == [
                "build-windows-portable",
                "build-linux-ubuntu",
                "build-linux-appimage",
                "build-macos-apple-silicon"
            ],
        "scan every completed platform inventory"
    );
    let (_, rust) = named_step(scanner, "Check all locked Rust dependencies")?;
    ensure!(
        rust["uses"].as_str() == Some("./.github/actions/audit-rust") && rust["if"].is_null(),
        "Rust advisories must block releases"
    );
    let (verify_index, verify) = named_step(
        scanner,
        "Verify complete SBOM coverage and archive identities",
    )?;
    requires_command(
        verify,
        "cargo xtask sbom verify --directory dist --require-release-evidence",
    )?;
    let (scan_index, scan) = named_step(scanner, "Check native and compiled release advisories")?;
    ensure!(
        scan["run"]
            .as_str()
            .is_some_and(|run| run.lines().any(|line| line
                .trim()
                .starts_with("if grype \"sbom:$sbom\" --fail-on high --output json"))),
        "retain all findings and block high/critical advisories"
    );
    requires_command(scan, "test \"$checked\" -eq 5")?;
    requires_command(scan, "exit \"$failed\"")?;
    ensure!(
        verify_index < scan_index && verify["if"].is_null() && scan["if"].is_null(),
        "scan only verified inventories and propagate failures"
    );
    let publisher = executable_job(source, "publish_release")?;
    ensure!(
        needs(publisher)?.contains(&"release_security"),
        "publication requires successful advisory scans"
    );
    let (signature_index, signature) = named_step(
        publisher,
        "Verify inventories and their signed release identity",
    )?;
    requires_command(
        signature,
        "cargo xtask sbom verify --directory dist --require-release-evidence",
    )?;
    requires_command(signature,"gh attestation verify \"$archive\" --bundle \"$archive.sbom.sigstore.json\" --predicate-type https://cyclonedx.org/bom")?;
    requires_command(
        signature,
        "gh attestation verify \"$subject\" --bundle \"$archive.provenance.sigstore.json\"",
    )?;
    ensure!(signature["if"].is_null() && signature["run"].as_str().is_some_and(|run|run.contains("--source-ref \"$GITHUB_REF\" --source-digest \"$GITHUB_SHA\" --deny-self-hosted-runners") && run.contains("$GITHUB_REPOSITORY/.github/workflows/desktop-packages.yml")),"enforce source/tag/workflow signer identity");
    let (publish_index, _) = named_step(publisher, "Publish packages, checksums and notes")?;
    ensure!(
        signature_index < publish_index,
        "authenticate evidence before publication"
    );
    Ok(())
}

#[test]
fn release_sbom_gates_reject_omitted_inventories_bypassed_scans_and_unsigned_publication() {
    let source = workflow(".github/workflows/desktop-packages.yml");
    check_release_sbom_gates(&source).unwrap();
    let inventory_index = named_step(
        &source["jobs"]["build-linux-appimage"],
        "Generate release SBOMs and tag attestations",
    )
    .unwrap()
    .0;
    let mut conditional = source.clone();
    conditional["jobs"]["build-linux-appimage"]["steps"][inventory_index]["if"] =
        Value::String("github.event_name == 'push'".into());
    assert!(check_release_sbom_gates(&conditional).is_err());
    let upload_index = named_step(
        &source["jobs"]["build-windows-portable"],
        "Upload portable release",
    )
    .unwrap()
    .0;
    let mut omitted = source.clone();
    omitted["jobs"]["build-windows-portable"]["steps"][upload_index]["with"]["path"] =
        Value::String("desktop/target/**/*.zip".into());
    assert!(check_release_sbom_gates(&omitted).is_err());
    let scan_index = named_step(
        &source["jobs"]["release_security"],
        "Check native and compiled release advisories",
    )
    .unwrap()
    .0;
    let mut ignored = source.clone();
    ignored["jobs"]["release_security"]["steps"][scan_index]["continue-on-error"] =
        Value::Bool(true);
    assert!(check_release_sbom_gates(&ignored).is_err());
    let signature_index = named_step(
        &source["jobs"]["publish_release"],
        "Verify inventories and their signed release identity",
    )
    .unwrap()
    .0;
    let mut unsigned = source;
    unsigned["jobs"]["publish_release"]["steps"][signature_index]["run"] = Value::String("cargo xtask sbom verify --directory dist --require-release-evidence\n# gh attestation verify example only".into());
    assert!(check_release_sbom_gates(&unsigned).is_err());
}

// Check the real caller bindings before expanding the shared job for the existing
// geometry/publication contracts. Expansion substitutes only the platform name;
// the executable steps and their failure conditions remain the actual template.
fn mcp_acceptance_workflow(source: &Value, acceptance: &Value) -> Result<Value> {
    let producer = executable_job(source, "prepare-windows-sdk")?;
    ensure!(
        producer["outputs"]["sdk-artifact-id"].as_str()
            == Some("${{ steps.artifact.outputs.artifact-id }}")
            && producer["outputs"]["sdk-toolset"].as_str()
                == Some("${{ steps.occt.outputs.toolset }}"),
        "share this producer's artifact identity and compiler"
    );
    let (_, upload) = named_step(producer, "Share SDK with this run's Windows shards")?;
    ensure!(
        upload["id"].as_str() == Some("artifact")
            && upload["if"].is_null()
            && uses_action(upload, "actions/upload-artifact")
            && upload["with"]["path"].as_str() == Some("vcpkg_installed/x64-windows/")
            && upload["with"]["if-no-files-found"].as_str() == Some("error"),
        "publish the complete SDK only after successful preparation"
    );
    let template = executable_job(acceptance, "acceptance")?;
    let (_, download) = named_step(template, "Download this run's Windows SDK")?;
    ensure!(
        download["if"].as_str() == Some("inputs.platform == 'windows'")
            && uses_action(download, "actions/download-artifact")
            && download["with"]["artifact-ids"].as_str() == Some("${{ inputs.sdk-artifact-id }}"),
        "download the exact current-run SDK artifact"
    );
    for key in ["run-id", "repository", "github-token", "pattern"] {
        ensure!(
            download["with"][key].is_null(),
            "SDK cannot come from {key}"
        );
    }
    let (compatibility_index, compatibility) =
        named_step(template, "MSVC environment and SDK compatibility")?;
    ensure!(
        compatibility["if"].as_str() == Some("inputs.platform == 'windows'")
            && compatibility["env"]["SDK_TOOLSET"].as_str() == Some("${{ inputs.sdk-toolset }}")
            && compatibility["run"]
                .as_str()
                .is_some_and(|run| { run.contains("if ($toolset -ne $env:SDK_TOOLSET) { throw") }),
        "reject an SDK from a different compiler"
    );
    let (qualification_index, qualification) =
        named_step(template, "Qualify downloaded Windows SDK")?;
    ensure!(
        qualification["if"].as_str() == Some("inputs.platform == 'windows'"),
        "every Windows consumer must qualify the downloaded runtime"
    );
    requires_command(
        qualification,
        "cargo xtask verify-occt-storage --prefix $env:OCCT_ROOT",
    )?;
    let (lint_index, _) = named_step(template, "Lint native Rust workspaces")?;
    ensure!(
        compatibility_index < qualification_index && qualification_index < lint_index,
        "qualify the shared SDK before native acceptance"
    );
    let mut expanded = source.clone();
    for (id, platform) in [("mcp-windows", "windows"), ("mcp-linux", "linux")] {
        let caller = executable_job(source, id)?;
        ensure!(
            caller["uses"].as_str() == Some("./.github/workflows/mcp-acceptance.yml")
                && caller["with"]["platform"].as_str() == Some(platform)
                && caller["if"].is_null(),
            "both platforms must invoke the complete shared acceptance"
        );
        if platform == "windows" {
            ensure!(
                needs(caller)? == ["prepare-windows-sdk"]
                    && caller["with"]["sdk-artifact-id"].as_str()
                        == Some("${{ needs.prepare-windows-sdk.outputs.sdk-artifact-id }}")
                    && caller["with"]["sdk-toolset"].as_str()
                        == Some("${{ needs.prepare-windows-sdk.outputs.sdk-toolset }}"),
                "Windows depends on this run's successful SDK producer"
            );
        } else {
            ensure!(
                needs(caller)? == ["knowledge_preflight"],
                "Linux must depend only on cheap source validation, independently of Windows"
            );
        }
        let concrete =
            serde_yaml_ng::to_string(template)?.replace("${{ inputs.platform }}", platform);
        expanded["jobs"][id] = serde_yaml_ng::from_str(&concrete)?;
    }
    Ok(expanded)
}

fn check_mcp_provenance(source: &Value) -> Result<()> {
    let expected: Value = serde_yaml_ng::from_str(
        "[{shard: core, project: garden-bench}, {shard: turbine, project: vertical-axis-turbine}, {shard: vise, project: d-screw-vise}]",
    )?;
    for (id, platform) in [("mcp-windows", "windows"), ("mcp-linux", "linux")] {
        let job = executable_job(source, id)?;
        ensure!(
            job["strategy"]["fail-fast"].as_bool() == Some(false),
            "retain independent shard outcomes"
        );
        ensure!(
            job["strategy"]["matrix"]["include"] == expected,
            "{id} must run every named acceptance shard"
        );
        let (lint_index, lint) = named_step(job, "Lint native Rust workspaces")?;
        ensure!(
            lint["id"].as_str() == Some("native-lint")
                && lint["if"].as_str() == Some("matrix.shard == 'core'"),
            "core native lint must retain the geometry prerequisite identity"
        );
        requires_command(lint, "cargo clippy --locked --workspace --all-targets --all-features --no-deps -- -D warnings")?;
        requires_command(lint, "cargo clippy --locked --manifest-path mcp-server/Cargo.toml --all-targets --all-features --no-deps -- -D warnings")?;
        let (test_index, tests) = named_step(job, "MCP server tests")?;
        requires_command(tests, "cargo xtask ci mcp-shard ${{ matrix.shard }}")?;
        ensure!(
            tests["if"].is_null(),
            "every shard must run its server tests"
        );
        let (geometry_index, geometry) =
            named_step(job, "Native geometry integration regressions")?;
        ensure!(
            geometry["if"].as_str() == Some("${{ !cancelled() && matrix.shard == 'core' && steps.native-lint.outcome == 'success' }}"),
            "native geometry must survive MCP failure after successful native lint, but stop on cancellation"
        );
        requires_command(geometry, "cargo test --locked -p limo-cad-occt --features native-occt --tests -- --test-threads=1")?;
        ensure!(
            geometry["env"]["CARGO_TARGET_DIR"].as_str()
                == Some("${{ github.workspace }}/mcp-server/target"),
            "reuse the native ABI build target"
        );
        let (_, workshop) = named_step(job, "MCP bench and complete feature workshop")?;
        ensure!(
            workshop["if"].as_str() == Some("matrix.shard == 'core'"),
            "the complete workshop must run in core"
        );
        requires_command(workshop, "cargo xtask test-mcp bench")?;
        let (upload_index, upload) = named_step(job, "Upload successful demo input")?;
        ensure!(
            upload["if"].is_null(),
            "only default success flow may upload demo inputs"
        );
        ensure!(
            uses_action(upload, "actions/upload-artifact"),
            "missing shard input upload"
        );
        ensure!(
            upload["with"]["name"].as_str()
                == Some(format!("mcp-demo-{platform}-${{{{ matrix.shard }}}}").as_str()),
            "demo input must retain platform/shard identity"
        );
        ensure!(
            upload["with"]["if-no-files-found"].as_str() == Some("error"),
            "missing demo input must fail"
        );
        ensure!(
            lint_index < test_index && test_index < geometry_index && geometry_index < upload_index,
            "native lint and acceptance must finish before uploading a demo"
        );
    }
    for (id, native, platform) in [
        ("mcp-tests", "mcp-windows", "windows"),
        ("mcp-tests-linux", "mcp-linux", "linux"),
    ] {
        let job = executable_job(source, id)?;
        ensure!(
            needs(job)? == [native],
            "aggregate requires all native platform shards"
        );
        ensure!(
            job["if"].as_str() == Some("${{ !cancelled() }}"),
            "aggregate must report failed native results, but not cancelled runs"
        );
        ensure!(
            job["env"]["NATIVE_RESULT"].as_str()
                == Some(format!("${{{{ needs.{native}.result }}}}").as_str()),
            "gate must inspect this platform's aggregate result"
        );
        ensure!(
            job["env"]["MCP_PLATFORM"].as_str() == Some(platform),
            "wrong aggregate platform"
        );
        let (gate_index, gate) = named_step(job, "Require every platform acceptance shard")?;
        ensure!(
            gate["if"].is_null(),
            "platform success gate cannot be conditional"
        );
        requires_command(gate, "cargo xtask ci require-platform")?;
        let downloads = executable_steps(job)?
            .iter()
            .enumerate()
            .filter(|(_, step)| uses_action(step, "actions/download-artifact"))
            .collect::<Vec<_>>();
        ensure!(
            downloads.len() == 3,
            "download exactly the three verified shard inputs"
        );
        for ((index, download), shard) in downloads.iter().zip(["core", "turbine", "vise"]) {
            ensure!(
                gate_index < *index && download["if"].is_null(),
                "gate must pass before every download"
            );
            ensure!(
                download["with"]["name"].as_str()
                    == Some(format!("mcp-demo-${{{{ env.MCP_PLATFORM }}}}-{shard}").as_str()),
                "download exact platform/shard input"
            );
            for key in ["run-id", "repository", "github-token", "pattern"] {
                ensure!(
                    download["with"][key].is_null(),
                    "demo input must be from this run, not {key}"
                );
            }
        }
        let (stage_index, stage) = named_step(job, "Stage verified editable demo projects")?;
        requires_command(stage, "cargo xtask ci stage-demo-projects")?;
        ensure!(
            stage["if"].is_null() && downloads.iter().all(|(index, _)| *index < stage_index),
            "stage only after all verified downloads"
        );
        let (upload_index, upload) = named_step(job, "Upload verified editable demo projects")?;
        ensure!(
            stage_index < upload_index && upload["if"].is_null(),
            "publish only staged projects"
        );
        ensure!(
            upload["with"]["name"].as_str()
                == Some("Limo-CAD-demo-projects-${{ env.MCP_PLATFORM }}-${{ github.sha }}"),
            "demo publication must identify platform and exact commit"
        );
    }
    Ok(())
}

#[test]
fn embedded_catalogs_are_verified_before_engine_tests_and_desktop_packages() {
    let engine = read(".github/workflows/linux-engine-tests.yml");
    let tooling = read(".github/workflows/rust-web.yml");
    let desktop = read(".github/workflows/desktop-packages.yml");
    ordered(
        &job(&desktop, "build-windows-portable"),
        "uses: ./.github/actions/setup-rust",
        "uses: ./.github/actions/setup-windows-occt",
    );
    for command in [
        "cargo xtask materials --fetch --check",
        "cargo xtask printer-profiles --fetch --check",
    ] {
        ordered(&engine, command, "cargo test --locked --workspace");
        assert!(job(&tooling, "repository-tooling").contains(command));
        for name in [
            "build-windows-portable",
            "build-linux-ubuntu",
            "build-linux-appimage",
            "build-macos-apple-silicon",
        ] {
            let config = job(&desktop, name);
            ordered(&config, command, "cargo xtask package");
        }
    }
}

#[test]
fn rust_setup_and_wasm_tools_use_repository_pins() {
    let action = read(".github/actions/setup-rust/action.yml");
    assert!(
        action.contains("rustup show")
            && action.contains("working-directory: ${{ inputs.directory }}")
    );
    assert!(!action.contains("stable"));
    let workflow = "session-storage";
    let source = read(&format!(".github/workflows/{workflow}.yml"));
    assert!(source.contains("uses: ./.github/actions/setup-rust"));
    assert!(!source.contains("dtolnay/rust-toolchain@"));
    for input in [
        "rust-toolchain.toml",
        ".cargo/**",
        ".github/actions/setup-rust/**",
    ] {
        assert!(
            source.contains(&format!("'{input}'")),
            "{workflow} must check {input} changes"
        );
    }
    let web = read(".github/workflows/rust-web.yml");
    assert!(
        web.contains("cargo xtask bootstrap --wasm") && !web.contains("cargo install wasm-pack")
    );
    let desktop = read(".github/workflows/desktop-packages.yml");
    assert!(
        desktop.contains("cargo xtask ci desktop-changes")
            && !desktop.contains("actions/github-script")
    );
    let matched = read(".github/workflows/native-switching.yml");
    assert!(matched.contains("uses: ./candidate/.github/actions/setup-rust"));
    assert!(matched.contains("RUSTUP_TOOLCHAIN=${{ steps.rust.outputs.toolchain }}"));
    assert!(matched.contains("uses: ./candidate/.github/actions/setup-unix-occt"));
    let unix_sdk = read(".github/actions/setup-unix-occt/action.yml");
    assert!(unix_sdk.contains("libfontconfig-dev libfreetype6-dev"));
    assert!(unix_sdk.contains("libx11-dev"));
    for image in ["ubuntu-26.04", "appimage-ubuntu-22.04"] {
        assert!(read(&format!("scripts/docker/{image}.Dockerfile")).contains("libfontconfig-dev"));
    }
    assert_eq!(
        unix_sdk
            .matches("working-directory: ${{ inputs.directory }}")
            .count(),
        2
    );
}

#[test]
fn package_and_publication_cannot_bypass_version_or_failed_builds() {
    // Parse executable YAML first: commented examples and disabled steps cannot
    // satisfy the release gates. The remaining source checks protect shell details.
    check_package_gates(&workflow(".github/workflows/desktop-packages.yml")).unwrap();
    let desktop = read(".github/workflows/desktop-packages.yml");
    assert!(!desktop.contains("frontend_regressions") && !desktop.contains("npm ci"));
    assert!(
        job(&desktop, "version_preflight").contains("uses: ./.github/workflows/version-guard.yml")
    );
    let bypass = Regex::new(r"(?m)^    if:.*(?:always|cancelled|failure)\(").unwrap();
    for name in [
        "build-windows-portable",
        "build-linux-ubuntu",
        "build-linux-appimage",
        "build-macos-apple-silicon",
    ] {
        let config = job(&desktop, name);
        assert!(config.contains("needs: [classify_changes, version_preflight]"));
        matches(
            &config,
            r"if: needs\.classify_changes\.outputs\.\w+_should_build == 'true'",
        );
        assert!(!bypass.is_match(&config));
    }
    let publish = job(&desktop, "publish_release");
    assert!(publish
        .contains("if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')"));
    for name in [
        "build-windows-portable",
        "build-linux-ubuntu",
        "build-linux-appimage",
        "verify-linux-appimage",
        "build-macos-apple-silicon",
    ] {
        assert!(publish.contains(&format!("      - {name}")));
    }
    ordered(&publish, "check-release-tag", "actions/download-artifact");
    assert!(
        publish.contains("merge-multiple: false")
            && publish.contains("--draft")
            && publish.contains("test \"$uploaded\" -eq 41")
    );
    assert!(publish.contains("test \"$checked\" -eq 5"));
    assert!(publish.contains("perl -pi -e 's/\\r$//'"));
    assert_eq!(
        desktop
            .lines()
            .filter(|line| line.trim() == "contents: write")
            .count(),
        1
    );
    let version = read(".github/workflows/version-guard.yml");
    matches(&version, r"(?m)^  workflow_call:");
    assert!(version.contains("group: version-guard-${{ github.workflow }}-${{ github.ref }}"));
    assert!(version.contains("cargo test --locked -p xtask 'release_tooling::'"));
    assert!(version.contains("cargo xtask check-release-tag \"$GITHUB_REF_NAME\" \"$GITHUB_SHA\""));
    assert!(!Regex::new(r"setup-node|\bnode\b|\bnpm\b")
        .unwrap()
        .is_match(&version));
}

#[test]
fn sdk_cache_keys_keep_all_abi_inputs_and_arm_runner_is_default_branch_only() {
    for source in [
        ".github/actions/setup-windows-occt/action.yml",
        ".github/actions/setup-unix-occt/action.yml",
        ".github/actions/setup-linux-desktop/action.yml",
        ".github/workflows/desktop-packages.yml",
        ".github/workflows/mcp-server.yml",
        ".github/workflows/native-host-tests.yml",
        ".github/workflows/native-switching.yml",
        ".github/workflows/native-visual.yml",
        ".github/workflows/windows-occt-cache.yml",
    ] {
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&read(source))
            .unwrap_or_else(|error| panic!("invalid SDK workflow {source}: {error}"));
    }
    let warmer = read(".github/workflows/windows-occt-cache.yml");
    assert!(
        warmer.contains("  push:\n    branches: [main]")
            && warmer.contains("  workflow_dispatch:")
            && warmer.contains("  schedule:")
    );
    assert!(!warmer.contains("  pull_request"));
    assert!(job(&warmer, "warm-arm64").contains(
        "if: github.ref == format('refs/heads/{0}', github.event.repository.default_branch)"
    ));
    let desktop = read(".github/workflows/desktop-packages.yml");
    for value in [
        "windows-11-vs2026-arm",
        "windows-11-vs2026-arm-arm64-windows-msvc",
        "arm64-windows",
        "Microsoft.VisualStudio.Component.VC.Tools.ARM64",
    ] {
        assert!(warmer.contains(value) && desktop.contains(value));
    }
    let sdk = read(".github/actions/setup-windows-occt/action.yml");
    let manifest: serde_json::Value = serde_json::from_str(&read("vcpkg.json")).unwrap();
    let commit = manifest["builtin-baseline"].as_str().unwrap();
    assert!(commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(sdk.contains(&format!("ref: {commit}")));
    assert!(!sdk.contains("vcpkg-commit"));
    for prefix in ["vcpkg-installed-v2", "vcpkg-binary-v3"] {
        let key = format!("key: {prefix}-${{{{ inputs.runner-cache-key }}}}-${{{{ steps.msvc.outputs.toolset }}}}-{commit}-${{{{ hashFiles('vcpkg.json', 'vcpkg-configuration.json', 'native/occt-overlay/**') }}}}");
        assert_eq!(sdk.matches(&key).count(), 2);
    }
    assert!(!sdk.contains("restore-keys:"));
    ordered(
        &sdk,
        "- name: Qualify checked header and actual runtime",
        "- name: Save installed OpenCASCADE tree",
    );
}

#[test]
fn native_shards_keep_geometry_workshop_and_exact_same_run_artifact_provenance() {
    let caller = workflow(".github/workflows/mcp-server.yml");
    let shared = workflow(".github/workflows/mcp-acceptance.yml");
    check_mcp_provenance(&mcp_acceptance_workflow(&caller, &shared).unwrap()).unwrap();
    let mcp = read(".github/workflows/mcp-server.yml");
    for input in [
        "crates/**",
        "xtask/**",
        ".github/actions/setup-windows-occt/**",
        ".github/actions/setup-unix-occt/**",
        ".github/workflows/mcp-acceptance.yml",
    ] {
        assert_eq!(mcp.matches(&format!("- '{input}'")).count(), 2);
    }
    assert!(job(&mcp, "mcp-tests")
        .contains("key: demo-publication-registry\n          cache-targets: false"));
}

#[test]
fn shared_native_acceptance_rejects_foreign_sdk_skipped_callers_and_wrong_compilers() {
    let caller = workflow(".github/workflows/mcp-server.yml");
    let shared = workflow(".github/workflows/mcp-acceptance.yml");
    mcp_acceptance_workflow(&caller, &shared).unwrap();
    for (key, value) in [
        ("sdk-artifact-id", "an-artifact-from-a-different-run"),
        ("sdk-toolset", "a-different-compiler"),
    ] {
        let mut foreign = caller.clone();
        foreign["jobs"]["mcp-windows"]["with"][key] = Value::String(value.into());
        assert!(mcp_acceptance_workflow(&foreign, &shared).is_err(), "{key}");
    }
    let mut skipped = caller.clone();
    skipped["jobs"]["mcp-linux"]["if"] = Value::Bool(false);
    assert!(mcp_acceptance_workflow(&skipped, &shared).is_err());
    let mut dependent = caller.clone();
    dependent["jobs"]["mcp-linux"]["needs"] = Value::String("prepare-windows-sdk".into());
    assert!(mcp_acceptance_workflow(&dependent, &shared).is_err());
    let download = named_step(
        &shared["jobs"]["acceptance"],
        "Download this run's Windows SDK",
    )
    .unwrap()
    .0;
    for key in ["run-id", "repository", "github-token", "pattern"] {
        let mut foreign = shared.clone();
        foreign["jobs"]["acceptance"]["steps"][download]["with"][key] =
            Value::String("foreign".into());
        assert!(mcp_acceptance_workflow(&caller, &foreign).is_err(), "{key}");
    }
    let compatibility = named_step(
        &shared["jobs"]["acceptance"],
        "MSVC environment and SDK compatibility",
    )
    .unwrap()
    .0;
    let mut mismatch = shared.clone();
    mismatch["jobs"]["acceptance"]["steps"][compatibility]["run"] =
        Value::String("Write-Output 'SDK available'".into());
    assert!(mcp_acceptance_workflow(&caller, &mismatch).is_err());
    let qualification = named_step(
        &shared["jobs"]["acceptance"],
        "Qualify downloaded Windows SDK",
    )
    .unwrap()
    .0;
    let mut unqualified = shared;
    unqualified["jobs"]["acceptance"]["steps"][qualification]["if"] =
        Value::String("inputs.platform == 'linux'".into());
    assert!(mcp_acceptance_workflow(&caller, &unqualified).is_err());
}

#[test]
fn windows_package_keyboard_checks_use_an_isolated_control_host() {
    let workflow = read(".github/workflows/desktop-packages.yml");
    let windows = job(&workflow, "build-windows-portable");
    ordered(
        &windows,
        "git status --porcelain --untracked-files=normal",
        "cargo xtask package --target",
    );
    assert!(windows.contains("cargo xtask package --target \"${{ matrix.rust_target }}\""));
    assert!(!windows
        .contains("cargo xtask package --target \"${{ matrix.rust_target }}\" --computer-control"));
    ordered(
        &windows,
        "Expand-Archive -LiteralPath $archive.FullName",
        "$archiveHash = (Get-FileHash",
    );
    ordered(
        &windows,
        "$archiveHash = (Get-FileHash",
        "cargo build --locked --release --manifest-path desktop/Cargo.toml --bin limo-cad --features native-computer-control",
    );
    ordered(
        &windows,
        "Copy-Item -LiteralPath $executable.DirectoryName -Destination $probeRoot -Recurse",
        "-PackageDirectory $probeRoot",
    );
    assert!(windows.contains("-ControlledSourceHost"));
    for failure in [
        "Source host qualification changed the default packaged executable",
        "Source host qualification changed the default portable ZIP",
    ] {
        assert!(windows.contains(failure));
    }
    let verify = read("scripts/verify-windows-viewport.ps1");
    assert!(verify.contains("cargo run --quiet --locked -p xtask --features native-control-harness -- test-mcp native-platform"));
    ordered(
        &verify,
        "prepare-hosted-arm-desktop.ps1",
        "cargo run --quiet --locked",
    );
    assert!(verify.contains(
        "if ($LASTEXITCODE -ne 0) { throw 'Controlled source host native input verification failed' }"
    ));
    assert!(verify.contains("if (-not $ControlledSourceHost) { throw"));
    assert!(!verify.contains("continue-on-error"));
    let fixture = read("xtask/src/native_platform_test.rs");
    ordered(
        &fixture,
        "wait_for_interface(&mut client, &session)",
        "Driver::new(client.process_id(), out)",
    );
    let driver = read("xtask/src/native_platform_test/windows.rs");
    ordered(
        &driver,
        "Client::start_command(command",
        "prepare_hosted_arm_desktop(out)?",
    );
    assert!(
        driver.contains("runner-ready-desktop.json")
            && driver.contains(".creation_flags(0x08000000)")
    );
}

#[test]
fn appimage_keeps_oldest_glibc_and_minimal_host_input_runtime() {
    let desktop = read(".github/workflows/desktop-packages.yml");
    let build = job(&desktop, "build-linux-appimage");
    let verify = job(&desktop, "verify-linux-appimage");
    assert!(
        build.contains("container: ubuntu:22.04")
            && build.contains("cargo xtask package --bundle appimage")
    );
    assert!(build.contains("cargo xtask build-occt --prefix /opt/opencascade"));
    assert!(build.contains("steps.occt_key.outputs.sdk_key"));
    ordered(&build, "Save verified OCCT", "Build and audit the AppImage");
    let cache = read("xtask/src/occt_cache.rs");
    assert!(cache.contains("FREETYPE_LIBRARIES") && cache.contains("CMAKE_CXX_COMPILER_VERSION"));
    assert!(build.contains("GLIBC_2.35 | sort -V | tail -n 1"));
    assert!(
        verify.contains("runs-on: ubuntu-26.04")
            && verify.contains("needs: [classify_changes, build-linux-appimage]")
    );
    for config in [&build, &verify] {
        assert!(config.contains("scripts/verify-linux-viewport.sh") && config.contains("x11"));
    }
    let docker = read("scripts/docker/appimage-ubuntu-22.04.Dockerfile");
    let packages = |text: &str| {
        Regex::new(r"(?m)^ +([a-z0-9][a-z0-9.+-]*) \\")
            .unwrap()
            .captures_iter(text)
            .map(|c| c[1].to_owned())
            .filter(|s| s != "zstd")
            .collect::<Vec<_>>()
    };
    assert_eq!(
        packages(docker.split("rm -rf /var/lib/apt/lists").next().unwrap()),
        packages(build.split("- name: Check out Limo CAD").next().unwrap())
    );
    for runtime in [
        "libegl1",
        "libx11-6",
        "libx11-xcb1",
        "libxcursor1",
        "libxi6",
        "libvulkan1",
        "libwayland-client0",
        "libwayland-cursor0",
        "libwayland-egl1",
        "xclip",
        "xdotool",
    ] {
        assert!(verify.contains(runtime));
    }
    let deb = job(&desktop, "build-linux-ubuntu");
    assert!(deb.contains("cargo xtask package --bundle deb") && !deb.contains(".AppImage"));
    let bundler = read("xtask/src/package/linux.rs");
    let sdk = read(".github/actions/setup-linux-desktop/action.yml");
    for dependency in [
        "libx11-xcb1",
        "libxcursor1",
        "libxi6",
        "libdbus-1-3",
        "zenity",
    ] {
        assert!(
            docker.contains(dependency) && sdk.contains(dependency) && bundler.contains(dependency)
        );
    }
    for library in ["client", "cursor", "egl"] {
        assert!(bundler.contains(&format!("\"libwayland-{library}.so\"")));
    }
    assert!(!bundler.contains("\"libwayland-server.so\""));
}

#[test]
fn release_inventory_trusts_the_exact_checkout_before_reading_git_metadata() {
    let action = read(".github/actions/release-sbom/action.yml");
    ordered(
        &action,
        "git config --global --add safe.directory \"$GITHUB_WORKSPACE\"",
        "cargo xtask sbom release",
    );
    assert!(!action.contains("safe.directory '*'"));
    assert!(!action.contains("safe.directory \"*\""));
}

#[test]
fn package_gates_reject_dependencies_disabled_commands_and_foreign_artifacts() {
    let valid = workflow(".github/workflows/desktop-packages.yml");
    check_package_gates(&valid).unwrap();
    let mut missing_version = valid.clone();
    missing_version["jobs"]["build-windows-portable"]["needs"]
        .as_sequence_mut()
        .unwrap()
        .pop();
    assert!(check_package_gates(&missing_version).is_err());
    let mut ignored_build = valid.clone();
    ignored_build["jobs"]["build-linux-ubuntu"]["continue-on-error"] = Value::Bool(true);
    assert!(check_package_gates(&ignored_build).is_err());
    let tag_index = named_step(
        &valid["jobs"]["publish_release"],
        "Refuse a release tag that does not name VERSION on main",
    )
    .unwrap()
    .0;
    for condition in [Value::Bool(false), Value::String("${{ false }}".into())] {
        let mut disabled_tag = valid.clone();
        disabled_tag["jobs"]["publish_release"]["steps"][tag_index]["if"] = condition;
        assert!(check_package_gates(&disabled_tag).is_err());
    }
    let mut comment_only = valid.clone();
    comment_only["jobs"]["publish_release"]["steps"][tag_index]["run"] = Value::String(
        "# cargo xtask check-release-tag \"$GITHUB_REF_NAME\" \"$GITHUB_SHA\"\necho example only"
            .into(),
    );
    assert!(check_package_gates(&comment_only).is_err());
    let download_index = named_step(
        &valid["jobs"]["publish_release"],
        "Download this run's packages",
    )
    .unwrap()
    .0;
    let mut foreign_artifact = valid.clone();
    foreign_artifact["jobs"]["publish_release"]["steps"][download_index]["with"]["run-id"] =
        Value::String("1234".into());
    assert!(check_package_gates(&foreign_artifact).is_err());
}

#[test]
fn mcp_provenance_rejects_missing_shards_bypassed_geometry_and_stale_inputs() {
    let valid = mcp_acceptance_workflow(
        &workflow(".github/workflows/mcp-server.yml"),
        &workflow(".github/workflows/mcp-acceptance.yml"),
    )
    .unwrap();
    check_mcp_provenance(&valid).unwrap();
    let mut missing_shard = valid.clone();
    missing_shard["jobs"]["mcp-linux"]["strategy"]["matrix"]["include"]
        .as_sequence_mut()
        .unwrap()
        .pop();
    assert!(check_mcp_provenance(&missing_shard).is_err());
    let geometry_index = named_step(
        &valid["jobs"]["mcp-windows"],
        "Native geometry integration regressions",
    )
    .unwrap()
    .0;
    for condition in [
        "matrix.shard == 'core'",
        "${{ !cancelled() && matrix.shard == 'core' }}",
        "${{ matrix.shard == 'core' && steps.native-lint.outcome == 'success' }}",
        "${{ always() && matrix.shard == 'core' && steps.native-lint.outcome == 'success' }}",
    ] {
        let mut suppressed_or_unprepared_geometry = valid.clone();
        suppressed_or_unprepared_geometry["jobs"]["mcp-windows"]["steps"][geometry_index]["if"] =
            Value::String(condition.into());
        assert!(
            check_mcp_provenance(&suppressed_or_unprepared_geometry).is_err(),
            "{condition}"
        );
    }
    let lint_index = named_step(&valid["jobs"]["mcp-windows"], "Lint native Rust workspaces")
        .unwrap()
        .0;
    let mut missing_lint_identity = valid.clone();
    missing_lint_identity["jobs"]["mcp-windows"]["steps"][lint_index]["id"] = Value::Null;
    assert!(check_mcp_provenance(&missing_lint_identity).is_err());
    let mut ignored_geometry = valid.clone();
    ignored_geometry["jobs"]["mcp-windows"]["steps"][geometry_index]["continue-on-error"] =
        Value::Bool(true);
    assert!(check_mcp_provenance(&ignored_geometry).is_err());
    let mut comment_geometry = valid.clone();
    comment_geometry["jobs"]["mcp-windows"]["steps"][geometry_index]["run"] = Value::String(
        "# cargo test --locked -p limo-cad-occt --features native-occt --tests -- --test-threads=1"
            .into(),
    );
    assert!(check_mcp_provenance(&comment_geometry).is_err());
    let mut wrong_platform = valid.clone();
    wrong_platform["jobs"]["mcp-tests-linux"]["env"]["NATIVE_RESULT"] =
        Value::String("${{ needs.mcp-windows.result }}".into());
    assert!(check_mcp_provenance(&wrong_platform).is_err());
    let gate_index = named_step(
        &valid["jobs"]["mcp-tests"],
        "Require every platform acceptance shard",
    )
    .unwrap()
    .0;
    let mut disabled_gate = valid.clone();
    disabled_gate["jobs"]["mcp-tests"]["steps"][gate_index]["if"] = Value::Bool(false);
    assert!(check_mcp_provenance(&disabled_gate).is_err());
    let download_index = named_step(&valid["jobs"]["mcp-tests"], "Download verified bench")
        .unwrap()
        .0;
    for key in ["run-id", "repository", "github-token", "pattern"] {
        let mut stale_inputs = valid.clone();
        stale_inputs["jobs"]["mcp-tests"]["steps"][download_index]["with"][key] =
            Value::String("foreign".into());
        assert!(check_mcp_provenance(&stale_inputs).is_err(), "{key}");
    }
}

fn check_security_scan_triggers(source: &Value) -> Result<()> {
    ensure!(
        source["on"].as_mapping().is_some_and(|events| events
            .contains_key(Value::String("pull_request".into()))
            && events.contains_key(Value::String("workflow_dispatch".into()))
            && events.contains_key(Value::String("schedule".into()))),
        "retain PR, manual and scheduled security scans"
    );
    let branches = source["on"]["push"]["branches"]
        .as_sequence()
        .context("missing post-merge security scan")?;
    ensure!(
        branches.len() == 1 && branches[0].as_str() == Some("main"),
        "scan main pushes while Bevy's open PR supplies candidate coverage once"
    );
    Ok(())
}

fn check_superseded_pr_policy(source: &Value) -> Result<()> {
    ensure!(
        source["concurrency"]["group"]
            .as_str()
            .is_some_and(|group| group.contains("github.ref")),
        "isolate distinct PR/branch receipts"
    );
    ensure!(
        source["concurrency"]["cancel-in-progress"].as_str()
            == Some("${{ github.event_name == 'pull_request' }}"),
        "cancel superseded PR receipts, preserve main/tag/manual qualification"
    );
    Ok(())
}

fn check_sdk_owner_policy(concurrency: &Value, expected_group: &str) -> Result<()> {
    ensure!(
        concurrency["group"].as_str() == Some(expected_group),
        "keep the stable SDK-owner namespace and its branch/manual-probe isolation"
    );
    ensure!(
        concurrency["cancel-in-progress"].as_bool() == Some(false),
        "finish the running SDK qualification and cache publication before supersession"
    );
    Ok(())
}

fn check_codeql_supersession_policies(source: &Value) -> Result<()> {
    ensure!(
        source["concurrency"].is_null(),
        "workflow-level cancellation must not override protected native CodeQL jobs"
    );
    for (id, group, cancel, languages, timeout) in [
        (
            "native",
            "codeql-${{ github.workflow }}-${{ github.ref }}-native-${{ matrix.language }}",
            false,
            &["rust", "c-cpp"][..],
            240,
        ),
        (
            "scripts",
            "codeql-${{ github.workflow }}-${{ github.ref }}-scripts-${{ matrix.language }}",
            true,
            &["actions", "python", "javascript-typescript"][..],
            20,
        ),
    ] {
        let job = executable_job(source, id)?;
        ensure!(
            job["concurrency"]["group"].as_str() == Some(group),
            "{id} must isolate each CodeQL language and PR without a commit/run-specific group"
        );
        ensure!(
            job["concurrency"]["cancel-in-progress"].as_bool() == Some(cancel),
            "protect native SDK owners while cancelling superseded script checks"
        );
        let actual_languages = job["strategy"]["matrix"]["include"]
            .as_sequence()
            .context("missing normal CodeQL matrix")?
            .iter()
            .map(|entry| {
                entry["language"]
                    .as_str()
                    .context("missing CodeQL language")
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            actual_languages.len() == languages.len()
                && languages
                    .iter()
                    .all(|language| actual_languages.contains(language)),
            "retain all five CodeQL languages"
        );
        ensure!(
            job["name"].as_str() == Some("CodeQL (${{ matrix.language }})")
                && job["if"].is_null()
                && job["strategy"]["fail-fast"].as_bool() == Some(false)
                && job["timeout-minutes"].as_u64() == Some(timeout)
                && job["permissions"]["contents"].as_str() == Some("read")
                && job["permissions"]["security-events"].as_str() == Some("write"),
            "preserve CodeQL check identities, failure coverage, bounded timeouts and permissions"
        );
    }
    Ok(())
}

#[test]
fn sdk_owners_and_fast_scripts_keep_distinct_supersession_policies() {
    // Native SDK builds must finish qualification/cache publication. Stable ref
    // groups still consolidate pending revisions; old-head success never waives
    // the newest head's normal acceptance jobs. The package case below retains
    // its separately qualified existing contract.
    let host_group = concat!(
        "native-host-tests-${{ github.workflow }}-${{ github.ref }}",
        "${{ inputs.ime-probe-only && (inputs.ime-probe-macos && '-ime-probe-macos' || '-ime-probe') ",
        "|| inputs.ime-native-macos && '-ime-native-macos' || inputs.input-family == 'windows-ime' ",
        "&& '-windows-ime' || inputs.input-family == 'windows-print' && '-windows-print' ",
        "|| inputs.input-family == 'switching' && '-switching' || inputs.linux-only ",
        "&& format('-linux-only-{0}', inputs.input-family || 'all') || '' }}"
    );
    for (file, group) in [
        ("mcp-server", "mcp-server-${{ github.ref }}"),
        ("native-host-tests", host_group),
    ] {
        let policy = workflow(&format!(".github/workflows/{file}.yml"));
        check_sdk_owner_policy(&policy["concurrency"], group).unwrap();
        for cancellation in [
            Value::Bool(true),
            Value::String("${{ github.event_name == 'pull_request' }}".into()),
            Value::String("false".into()),
            Value::Null,
        ] {
            let mut drift = policy["concurrency"].clone();
            drift["cancel-in-progress"] = cancellation;
            assert!(check_sdk_owner_policy(&drift, group).is_err(), "{file}");
        }
        for changed_group in [
            group.replace("github.ref", "github.sha"),
            format!("{group}-${{{{ github.run_id }}}}"),
            "native-sdk".into(),
            group.replace("${{ github.ref }}", ""),
        ] {
            let mut drift = policy["concurrency"].clone();
            drift["group"] = Value::String(changed_group);
            assert!(check_sdk_owner_policy(&drift, group).is_err(), "{file}");
        }
    }

    let codeql = workflow(".github/workflows/codeql.yml");
    check_codeql_supersession_policies(&codeql).unwrap();
    let mut reordered = codeql.clone();
    for id in ["native", "scripts"] {
        reordered["jobs"][id]["strategy"]["matrix"]["include"]
            .as_sequence_mut()
            .unwrap()
            .reverse();
    }
    check_codeql_supersession_policies(&reordered).unwrap();
    let mut workflow_override = codeql.clone();
    workflow_override["concurrency"] =
        workflow(".github/workflows/linux-engine-tests.yml")["concurrency"].clone();
    assert!(check_codeql_supersession_policies(&workflow_override).is_err());
    for id in ["native", "scripts"] {
        for cancellation in [
            Value::Bool(id == "native"),
            Value::String("${{ github.event_name == 'pull_request' }}".into()),
            Value::String("false".into()),
            Value::Null,
        ] {
            let mut drift = codeql.clone();
            drift["jobs"][id]["concurrency"]["cancel-in-progress"] = cancellation;
            assert!(check_codeql_supersession_policies(&drift).is_err(), "{id}");
        }
        let group = codeql["jobs"][id]["concurrency"]["group"].as_str().unwrap();
        for changed_group in [
            group.replace("-${{ matrix.language }}", ""),
            group.replace("github.ref", "github.sha"),
            format!("{group}-${{{{ github.run_id }}}}"),
            group.replace("${{ github.ref }}", ""),
        ] {
            let mut drift = codeql.clone();
            drift["jobs"][id]["concurrency"]["group"] = Value::String(changed_group);
            assert!(check_codeql_supersession_policies(&drift).is_err(), "{id}");
        }
        let mut missing_language = codeql.clone();
        missing_language["jobs"][id]["strategy"]["matrix"]["include"]
            .as_sequence_mut()
            .unwrap()
            .pop()
            .unwrap();
        assert!(
            check_codeql_supersession_policies(&missing_language).is_err(),
            "{id}"
        );
        let mut duplicate_language = codeql.clone();
        let entries = duplicate_language["jobs"][id]["strategy"]["matrix"]["include"]
            .as_sequence_mut()
            .unwrap();
        let first = entries[0].clone();
        *entries.last_mut().unwrap() = first;
        assert!(
            check_codeql_supersession_policies(&duplicate_language).is_err(),
            "{id}"
        );
        for condition in [
            Value::Bool(false),
            Value::String("${{ github.event_name == 'workflow_dispatch' }}".into()),
        ] {
            let mut disabled_job = codeql.clone();
            disabled_job["jobs"][id]["if"] = condition;
            assert!(
                check_codeql_supersession_policies(&disabled_job).is_err(),
                "{id}"
            );
        }
    }
}

fn check_sdk_preserving_package_policy(source: &Value) -> Result<()> {
    ensure!(
        source["concurrency"]["group"].as_str() == Some("desktop-packages-${{ github.ref }}"),
        "serialize package revisions by branch/PR so pending work consolidates"
    );
    ensure!(
        source["concurrency"]["cancel-in-progress"].as_bool() == Some(false),
        "finish active SDK qualification and cache publication before the next package revision"
    );
    Ok(())
}

#[test]
fn desktop_package_sdk_qualification_survives_superseding_revisions() {
    let policy = workflow(".github/workflows/desktop-packages.yml");
    check_sdk_preserving_package_policy(&policy).unwrap();
    for cancellation in [
        Value::Bool(true),
        Value::String("${{ github.event_name == 'pull_request' }}".into()),
        Value::String("false".into()),
        Value::Null,
    ] {
        let mut drift = policy.clone();
        drift["concurrency"]["cancel-in-progress"] = cancellation;
        assert!(check_sdk_preserving_package_policy(&drift).is_err());
    }
    for group in [
        "desktop-packages-${{ github.sha }}",
        "desktop-packages-${{ github.ref }}-${{ github.run_id }}",
        "desktop-packages",
    ] {
        let mut drift = policy.clone();
        drift["concurrency"]["group"] = Value::String(group.into());
        assert!(check_sdk_preserving_package_policy(&drift).is_err());
    }
}

#[test]
fn security_and_heavy_ci_keep_coverage_without_duplicate_or_stale_pr_work() {
    let codeql = workflow(".github/workflows/codeql.yml");
    check_security_scan_triggers(&codeql).unwrap();
    let mut duplicate_scan = codeql.clone();
    duplicate_scan["on"]["push"]["branches"]
        .as_sequence_mut()
        .unwrap()
        .push(Value::String("feat/bevy-interface".into()));
    assert!(check_security_scan_triggers(&duplicate_scan).is_err());
    let mut missing_pr = codeql;
    missing_pr["on"]
        .as_mapping_mut()
        .unwrap()
        .remove(Value::String("pull_request".into()));
    assert!(check_security_scan_triggers(&missing_pr).is_err());
    // Short PR checks cancel stale work. SDK-backed workflows have their own
    // strict owner-preserving contract above rather than this cancellation rule.
    for file in ["linux-engine-tests", "required-interface"] {
        let policy = workflow(&format!(".github/workflows/{file}.yml"));
        check_superseded_pr_policy(&policy).unwrap();
        for bypass in [Value::Bool(true), Value::Bool(false)] {
            let mut drift = policy.clone();
            drift["concurrency"]["cancel-in-progress"] = bypass;
            assert!(check_superseded_pr_policy(&drift).is_err(), "{file}");
        }
    }
    let transport = workflow(".github/workflows/session-storage.yml");
    let branches = transport["on"]["push"]["branches"].as_sequence().unwrap();
    assert_eq!(branches, &[Value::String("main".into())]);
    let events = transport["on"].as_mapping().unwrap();
    assert!(events.contains_key(Value::String("pull_request".into())));
    assert!(events.contains_key(Value::String("workflow_dispatch".into())));
}

#[test]
fn native_font_and_visual_commands_require_nonempty_compiled_inventory() {
    let native = workflow(".github/workflows/native-host-tests.yml");
    for id in [
        "native-host-tests",
        "macos-native-host-tests",
        "windows-native-host-tests",
    ] {
        let job = &native["jobs"][id];
        let (_, font) = named_step(
            job,
            "Verify installed CJK and emoji fonts through Bevy shaping",
        )
        .unwrap();
        requires_command(font, "cargo xtask ci native-ignored-test fonts").unwrap();
        assert!(font["if"]
            .as_str()
            .unwrap()
            .contains("inputs.desktop-input"));
    }
    let visual = workflow(".github/workflows/native-visual.yml");
    check_visual_acceptance(&visual).unwrap();
    for field in ["if", "continue-on-error"] {
        let mut bypass = visual.clone();
        bypass["jobs"]["native-visual"][field] = Value::Bool(field == "continue-on-error");
        assert!(check_visual_acceptance(&bypass).is_err());
    }
    let mut missing_renderer = visual.clone();
    missing_renderer["on"]["pull_request"]["paths"] = Value::Sequence(vec![]);
    assert!(check_visual_acceptance(&missing_renderer).is_err());
    let mut missing_evidence = visual.clone();
    let steps = missing_evidence["jobs"]["native-visual"]["steps"]
        .as_sequence_mut()
        .unwrap();
    let upload = steps
        .iter_mut()
        .find(|step| step["name"].as_str() == Some("Retain native visual evidence"))
        .unwrap();
    upload["with"]["if-no-files-found"] = Value::String("warn".into());
    assert!(check_visual_acceptance(&missing_evidence).is_err());
}

fn check_visual_acceptance(visual: &Value) -> Result<()> {
    let job = &visual["jobs"]["native-visual"];
    ensure!(
        job["if"].is_null() && job["continue-on-error"].is_null(),
        "GPU acceptance must execute and propagate failures on relevant PRs"
    );
    let (_, check) = named_step(
        job,
        "Verify projected boundaries and thin-wall occlusion on the GPU",
    )?;
    requires_command(check, "cargo xtask ci native-ignored-test sketch-visual")?;
    let (_, evidence) = named_step(job, "Retain native visual evidence")?;
    ensure!(
        evidence["with"]["if-no-files-found"].as_str() == Some("error"),
        "A successful GPU matrix must retain its captures"
    );
    ensure!(
        check["if"].is_null(),
        "GPU matrix must not be disabled by a step condition"
    );
    let events = visual["on"].as_mapping().context("visual triggers")?;
    ensure!(
        events.contains_key(Value::String("workflow_dispatch".into())),
        "retain manual visual qualification"
    );
    let paths = visual["on"]["pull_request"]["paths"]
        .as_sequence()
        .context("renderer PR paths")?;
    for path in [
        "desktop/src/native_viewport/**",
        "crates/occt/**",
        "crates/sketch/**",
        "xtask/src/repository_ci.rs",
        ".github/workflows/native-visual.yml",
    ] {
        ensure!(
            paths.contains(&Value::String(path.into())),
            "missing affected GPU input {path}"
        );
    }
    ensure!(
        visual["jobs"]["native-platform-input"]["if"].as_str()
            == Some("github.event_name == 'workflow_dispatch'"),
        "retain full OS qualification on manual runs without duplicating unrelated PR fixtures"
    );
    check_superseded_pr_policy(visual)?;
    Ok(())
}
