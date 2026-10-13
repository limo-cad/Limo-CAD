//! Sequential Bevy comparison orchestration; never runs on a user's desktop.
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Clone, Copy)]
struct Case {
    scenario: &'static str,
    source: &'static str,
    repeat: usize,
    instances: usize,
}
impl Case {
    fn relative(&self) -> PathBuf {
        PathBuf::from(self.scenario).join(format!(
            "repeat-{}-instances-{}-{}-native",
            self.repeat, self.instances, self.source
        ))
    }
    fn inputs(&self) -> &'static [&'static str] {
        if self.scenario == "document-tabs" {
            &["part-a", "part-b"]
        } else {
            &["sheets"]
        }
    }
}
fn cases() -> Vec<Case> {
    let mut plan = Vec::new();
    for scenario in ["document-tabs", "drawing-sheets"] {
        for repeat in 1..=2 {
            for instances in 1..=2 {
                for source in if repeat == 1 {
                    ["baseline", "candidate"]
                } else {
                    ["candidate", "baseline"]
                } {
                    plan.push(Case {
                        scenario,
                        source,
                        repeat,
                        instances,
                    });
                }
            }
        }
    }
    plan
}

fn hosted() -> Result<()> {
    ensure!(
        cfg!(target_os = "linux"),
        "Switching comparison requires disposable Linux Xvfb"
    );
    for (name, expected) in [
        ("LIMO_CAD_SWITCHING_CI", "1"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("RUNNER_OS", "Linux"),
        ("GITHUB_REPOSITORY_ID", crate::repository::id()),
    ] {
        ensure!(
            env::var(name).as_deref() == Ok(expected),
            "Disposable hosted comparison requires {name}={expected}"
        );
    }
    Ok(())
}

fn directory(name: &str) -> Result<PathBuf> {
    PathBuf::from(env::var_os(name).with_context(|| format!("Missing {name}"))?)
        .canonicalize()
        .with_context(|| format!("Resolve {name}"))
}
fn logged(command: &mut Command, log: &Path) -> Result<()> {
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(log)?;
    let status = command
        .stdin(Stdio::null())
        .stdout(file.try_clone()?)
        .stderr(file)
        .status()
        .with_context(|| format!("Start {command:?}"))?;
    ensure!(
        status.success(),
        "{command:?} failed ({status}); see {}",
        log.display()
    );
    Ok(())
}
fn sha(root: &Path, source: &str) -> Result<String> {
    let value = fs::read_to_string(root.join("builds").join(format!("{source}.sha")))?
        .trim()
        .to_owned();
    ensure!(
        value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid {source} build SHA"
    );
    Ok(value)
}
fn json_file(path: &Path) -> Result<Value> {
    let mut file = fs::File::open(path)?;
    use std::io::Read;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 16 * 1024 * 1024,
        "Oversized comparison receipt: {}",
        path.display()
    );
    serde_json::from_slice(&bytes).with_context(|| format!("Read {}", path.display()))
}

fn build() -> Result<()> {
    hosted()?;
    let workspace = directory("GITHUB_WORKSPACE")?;
    let temporary = directory("RUNNER_TEMP")?;
    let evidence = temporary.join("native-switching");
    fs::create_dir(&evidence)?;
    let builds = evidence.join("builds");
    fs::create_dir(&builds)?;
    let binaries = temporary.join("switching-binaries");
    fs::create_dir(&binaries)?;
    for (program, arguments, name) in [
        ("rustc", vec!["-Vv"], "rustc.txt"),
        ("uname", vec!["-a"], "kernel.txt"),
        ("lscpu", vec![], "cpu.txt"),
        ("dpkg-query", vec!["-W"], "packages.txt"),
    ] {
        logged(Command::new(program).args(arguments), &builds.join(name))?;
    }
    let target = temporary.join("switching-target");
    let mut hashes = String::new();
    for source in ["baseline", "candidate"] {
        let checkout = workspace.join(source).canonicalize()?;
        let commit = crate::linux_fixture::output(
            Command::new("git")
                .current_dir(&checkout)
                .args(["rev-parse", "HEAD"]),
            Duration::from_secs(10),
        )?;
        fs::write(builds.join(format!("{source}.sha")), format!("{commit}\n"))?;
        for (from, name) in [
            ("desktop/Cargo.lock", format!("{source}.Cargo.lock")),
            ("Cargo.lock", format!("{source}.root.Cargo.lock")),
        ] {
            let copy = builds.join(name);
            fs::copy(checkout.join(from), &copy)?;
            hashes.push_str(&format!(
                "{}  {}\n",
                crate::hash::file(&copy)?,
                copy.file_name().unwrap().to_string_lossy()
            ));
        }
        let arguments = [
            "build",
            "--locked",
            "--manifest-path",
            "desktop/Cargo.toml",
            "--release",
            "--bin",
            "limo-cad",
        ];
        fs::write(
            builds.join(format!("{source}-native.command")),
            format!("cargo {}\n", arguments.join(" ")),
        )?;
        logged(
            crate::build_tools::cargo()
                .current_dir(&checkout)
                .env("CARGO_TARGET_DIR", &target)
                .args(arguments),
            &builds.join(format!("{source}-native.log")),
        )?;
        fs::copy(
            target.join("release/limo-cad"),
            binaries.join(format!("{source}-native")),
        )?;
    }
    fs::copy(env::current_exe()?, binaries.join("xtask"))?;
    fs::write(
        builds.join("driver.command"),
        "cargo build --locked -p xtask\n",
    )?;
    fs::copy(
        temporary.join("switching-driver.log"),
        builds.join("driver.log"),
    )?;
    fs::write(builds.join("source-inputs.sha256"), hashes)?;
    let mut hashes = String::new();
    for name in ["baseline-native", "candidate-native", "xtask"] {
        hashes.push_str(&format!(
            "{}  {name}\n",
            crate::hash::file(&binaries.join(name))?
        ));
    }
    fs::write(builds.join("binaries.sha256"), hashes)?;
    logged(
        Command::new("df").arg("-h"),
        &builds.join("disk-after-build.txt"),
    )
}

fn host(args: impl Iterator<Item = String>) -> Result<()> {
    hosted()?;
    crate::linux_fixture::verify_private_display()?;
    let arguments: Vec<_> = args.collect();
    let out = arguments
        .windows(2)
        .find(|pair| pair[0] == "--out")
        .map(|pair| PathBuf::from(&pair[1]))
        .context("Missing owned --out")?;
    let temporary = directory("RUNNER_TEMP")?;
    ensure!(
        out.is_absolute()
            && out
                .parent()
                .context("Missing output parent")?
                .canonicalize()?
                .starts_with(temporary.join("native-switching")),
        "Host evidence must stay inside RUNNER_TEMP/native-switching"
    );
    let wm_log = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(out.with_extension("openbox.log"))?;
    let _window_manager = OwnedChild(
        Command::new("openbox")
            .stdin(Stdio::null())
            .stdout(wm_log.try_clone()?)
            .stderr(wm_log)
            .spawn()?,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let property = crate::linux_fixture::output(
            Command::new("xprop").args(["-root", "_NET_SUPPORTING_WM_CHECK"]),
            deadline.saturating_duration_since(Instant::now()),
        );
        if property.is_ok_and(|value| value.contains("window id # 0x") && !value.ends_with("0x0")) {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Owned Openbox did not become ready"
        );
        thread::sleep(Duration::from_millis(100));
    }
    logged(
        Command::new("vulkaninfo").arg("--summary"),
        &out.with_extension("vulkan.txt"),
    )?;
    crate::native_switching_test::run(arguments.into_iter())
}

fn compare() -> Result<()> {
    hosted()?;
    let temporary = directory("RUNNER_TEMP")?;
    let evidence = temporary.join("native-switching").canonicalize()?;
    ensure!(
        evidence.starts_with(&temporary),
        "Evidence escapes RUNNER_TEMP"
    );
    let workspace = directory("GITHUB_WORKSPACE")?;
    let candidate = workspace.join("candidate").canonicalize()?;
    ensure!(
        candidate == crate::build_tools::root().canonicalize()?,
        "Run the candidate driver"
    );
    let commit = crate::linux_fixture::output(
        Command::new("git")
            .current_dir(&candidate)
            .args(["rev-parse", "HEAD"]),
        Duration::from_secs(10),
    )?;
    ensure!(
        commit == sha(&evidence, "candidate")?,
        "Candidate differs from the recorded build"
    );
    let inputs = evidence.join("inputs");
    fs::create_dir(&inputs)?;
    let fixtures = candidate.join("xtask/fixtures/switching");
    for name in [
        "part-a.nbcad",
        "part-b.nbcad",
        "manifest.json",
        "sheets.nbcad",
        "sheets-manifest.json",
    ] {
        fs::copy(fixtures.join(name), inputs.join(name))?;
    }
    let mut hashes = String::new();
    for name in ["part-a.nbcad", "part-b.nbcad", "sheets.nbcad"] {
        hashes.push_str(&format!(
            "{}  {name}\n",
            crate::hash::file(&inputs.join(name))?
        ));
    }
    fs::write(inputs.join("archive.sha256"), hashes)?;
    for scenario in ["document-tabs", "drawing-sheets"] {
        fs::create_dir(evidence.join(scenario))?;
    }
    let binaries = temporary.join("switching-binaries").canonicalize()?;
    ensure!(
        binaries.starts_with(&temporary),
        "Binaries escape RUNNER_TEMP"
    );
    let mut failures = 0;
    for case in cases() {
        let out = evidence.join(case.relative());
        let mut command = Command::new("dbus-run-session");
        command
            .args([
                "--",
                "xvfb-run",
                "--auto-servernum",
                "--server-args=-screen 0 3200x2160x24",
            ])
            .arg(binaries.join("xtask"))
            .args(["switching-comparison", "--host"])
            .arg("--server")
            .arg(binaries.join(format!("{}-native", case.source)))
            .arg("--out")
            .arg(&out)
            .args(["--scenario", case.scenario])
            .arg("--model-a")
            .arg(inputs.join(format!("{}.nbcad", case.inputs()[0])));
        if case.inputs().len() == 2 {
            command.arg("--model-b").arg(inputs.join("part-b.nbcad"));
        }
        command
            .args([
                "--commit",
                &sha(&evidence, case.source)?,
                "--profile",
                "release",
                "--cycles",
                "20",
                "--instances",
                &case.instances.to_string(),
            ])
            .env("WINIT_X11_SCALE_FACTOR", "1")
            .env("WINIT_UNIX_BACKEND", "x11");
        if let Err(error) = logged(&mut command, &out.with_extension("driver.log")) {
            eprintln!("{error:#}");
            failures += 1;
        }
    }
    let summary = aggregate(&evidence, &binaries, failures)?;
    fs::write(
        evidence.join("comparison.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    ensure!(
        summary["complete_matched_measurement"] == true,
        "Matched comparison is incomplete; retained all available evidence"
    );
    Ok(())
}

fn aggregate(evidence: &Path, binaries: &Path, failures: usize) -> Result<Value> {
    let baseline_hash = crate::hash::file(&binaries.join("baseline-native"))?;
    let candidate_hash = crate::hash::file(&binaries.join("candidate-native"))?;
    let mut scenarios = Map::new();
    for scenario in ["document-tabs", "drawing-sheets"] {
        let plan: Vec<_> = cases()
            .into_iter()
            .filter(|case| case.scenario == scenario)
            .collect();
        let hashes = plan[0]
            .inputs()
            .iter()
            .map(|name| crate::hash::file(&evidence.join("inputs").join(format!("{name}.nbcad"))))
            .collect::<Result<Vec<_>>>()?;
        let mut reports = Map::new();
        let mut errors = Vec::new();
        let mut inputs_match = true;
        let mut provenance_matches = true;
        let mut counts = vec![0; hashes.len()];
        let mut expected = vec![None; hashes.len()];
        let mut equal = true;
        for case in plan {
            let out = evidence.join(case.relative());
            match json_file(&out.join("report.json")) {
                Ok(report) => {
                    reports.insert(case.relative().to_string_lossy().into_owned(), report);
                }
                Err(error) => errors.push(format!("{error:#}")),
            }
            match json_file(&out.join("metadata.json")) {
                Ok(metadata) => {
                    inputs_match &= metadata["input_sha256"] == json!(hashes)
                        && metadata["scenario"] == scenario;
                    provenance_matches &= metadata["declared_commit"]
                        == sha(evidence, case.source)?
                        && metadata["declared_build_profile"] == "release"
                        && metadata["shell"] == "native"
                        && metadata["instances"] == case.instances
                        && metadata["cycles"] == 20
                        && metadata["binary_sha256"]
                            == if case.source == "baseline" {
                                baseline_hash.as_str()
                            } else {
                                candidate_hash.as_str()
                            };
                }
                Err(error) => {
                    inputs_match = false;
                    provenance_matches = false;
                    errors.push(format!("{error:#}"));
                }
            }
            for instance in 0..case.instances {
                for model in 0..hashes.len() {
                    match json_file(&out.join(format!("instance-{instance}/loaded-{model}.json"))) {
                        Ok(value) => {
                            counts[model] += 1;
                            if let Some(first) = &expected[model] {
                                equal &= first == &value;
                            } else {
                                expected[model] = Some(value);
                            }
                        }
                        Err(error) => {
                            equal = false;
                            errors.push(format!("{error:#}"));
                        }
                    }
                }
            }
        }
        let complete = reports.len() == 8
            && counts.iter().all(|count| *count == 12)
            && equal
            && inputs_match
            && provenance_matches
            && reports.values().all(|report| report["completed"] == true);
        scenarios.insert(scenario.into(), json!({"reports":reports,"errors":errors,
            "all_expected_reports_present":reports.len()==8,"loaded_model_counts":counts,
            "all_expected_loaded_models_present":counts.iter().all(|count|*count==12),
            "exact_loaded_models_equal_across_hosts":equal,"all_input_hashes_match_frozen_archives":inputs_match,
            "all_build_receipts_match":provenance_matches,"complete":complete}));
    }
    let complete = failures == 0 && scenarios.values().all(|value| value["complete"] == true);
    Ok(
        json!({"failed_invocations":failures,"scenarios":scenarios,"complete_matched_measurement":complete,
        "process_statistics_scope":"bounded owned process tree plus top-level counters; inspect partial observations; summed RSS is not unique physical memory",
        "measurement":"application acknowledgment; no equivalent compositor/GPU presentation claim",
        "performance_acceptance":"not established","user_report_cause":"not established"}),
    )
}

pub(crate) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    match args.next().as_deref() {
        None => compare(),
        Some("--build") if args.next().is_none() => build(),
        Some("--host") => host(args),
        Some("--plan") if args.next().is_none() => {
            println!("{}", serde_json::to_string_pretty(&cases().iter().map(|case| json!({"scenario":case.scenario,"source":case.source,"repeat":case.repeat,"instances":case.instances,"out":case.relative()})).collect::<Vec<_>>())?);
            Ok(())
        }
        _ => bail!(
            "Use switching-comparison [--build|--plan]; execution requires disposable hosted Linux"
        ),
    }
}

#[cfg(test)]
mod tests;
