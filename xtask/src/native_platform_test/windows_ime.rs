//! Assertions for the actual Windows host thread, separate from stock feasibility.
use super::*;

pub(super) const SOURCE: &str = "a76c93d9-5523-4e90-aafa-4db112f9ac76";
const CLASS: &str = "03b5835f-f03c-411b-9ce2-aa23e1171e36";

pub(super) fn guard() -> Result<()> {
    ensure!(
        hosted::enabled(
            std::env::consts::OS,
            "windows",
            "Windows",
            ("LIMO_CAD_NATIVE_IME_TEST", "windows-japanese"),
            |key| std::env::var(key).ok(),
        ),
        "Japanese IME input requires the explicit disposable GitHub Windows runner"
    );
    Ok(())
}

pub(super) fn hash(path: &Path) -> Result<String> {
    let file = fs::File::open(path)
        .with_context(|| format!("Cannot open IME provenance file {}", path.display()))?;
    Ok(crate::hash::reader(file)
        .with_context(|| format!("Cannot read IME provenance file {}", path.display()))?
        .to_ascii_uppercase())
}

pub(super) fn stock_success(report: &Value, run: &str) -> bool {
    report["status"] == "stock-control-ime-feasible"
        && report["environment"]["run_id"] == run
        && report["environment"]["runner_os"] == "Windows"
        && report["environment"]["runner_environment"] == "github-hosted"
        && report["environment"]["repository_id"] == crate::repository::id()
        && report["requested"]["exercise_ime"] == true
        && report["japanese_profile_enabled"] == true
        && report["ime"]["status"] == "stock-control-ime-feasible"
        && report["ime"]["native_bevy_validated"] == false
        && report["ime"]["preedit"] == "はる"
        && report["ime"]["results_before_commit"] == 0
        && report["ime"]["committed"] == "はる"
        && report["ime"]["cancelled_text"] == "はる"
        && report["ime"]["final_text"] == "はる"
        && report["ime"]["result_count"] == 1
        && report["ime"]["composition_starts"]
            .as_u64()
            .is_some_and(|count| count >= 2)
        && report["ime"]["composition_ends"]
            .as_u64()
            .is_some_and(|count| count >= 2)
        && report["ime"]["escape_count"]
            .as_u64()
            .is_some_and(|count| count == 1 || count == 2)
        && report["ime"]["received_ime_messages"]
            .as_array()
            .is_some_and(|messages| {
                messages
                    .iter()
                    .filter(|event| event["result"].is_string())
                    .count()
                    == 1
                    && messages.iter().any(|event| event["preedit"] == "はる")
            })
}

fn profile_matches(context: &Value, window: u64) -> bool {
    let profile = &context["active_profile"];
    context["window"] == window
        && context["pid"].as_u64().is_some_and(|id| id > 0)
        && context["window_thread"].as_u64().is_some_and(|id| id > 0)
        && context["foreground"] == true
        && context["focused"] == true
        && context["language"] == 0x411
        && profile["type"] == 1
        && profile["language"] == 0x411
        && profile["class_id"]
            .as_str()
            .is_some_and(|id| id.eq_ignore_ascii_case(CLASS))
        && profile["profile_id"]
            .as_str()
            .is_some_and(|id| id.eq_ignore_ascii_case(SOURCE))
}

pub(super) fn event_context_matches(current: &Value, field: &Value, window: u64) -> bool {
    current["context"].is_object()
        && current["context"] == field["context"]
        && current["control_label"] == "Project name"
        && current["control_key"] == field["control_key"]
        && current["binding"] == field["binding"]
        && profile_matches(&current["win32"], window)
}

pub(super) fn native_context_ready(current: &Value, field: &Value, window: u64) -> bool {
    current["context"].is_object()
        && current["context"] == field["context"]
        && current["control_key"] == field["control_key"]
        && current["binding"] == field["binding"]
        && current["window"]["ime_enabled"] == true
        && current["window"]["focused"] == true
        && profile_matches(&current["win32"], window)
        && current["win32"]["imm"]["context_present"] == true
        && current["win32"]["imm"]["open"] == true
        && current["win32"]["imm"]["context_released"] == true
        && [
            "native_text_field",
            "editable_text",
            "computed_node",
            "ui_transform",
            "render_target",
        ]
        .iter()
        .all(|key| current["native_field_components"][*key] == true)
}

pub(super) fn check_restored(client: &mut Client, before: &Value, out: &Path) -> Result<()> {
    let previous = &before["ui"]["ime_diagnostics"]["configuration"]["win32"];
    ensure!(
        previous["layout"].as_u64().is_some_and(|id| id > 0)
            && previous["active_profile"]["type"]
                .as_u64()
                .is_some_and(|kind| kind == 1 || kind == 2),
        "No exact initial Windows input context was recorded: {previous}"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = ui(client, json!({"action":"inspect"}))?;
        let current = &snapshot["ui"]["ime_diagnostics"]["configuration"]["win32"];
        fs::write(
            out.join("ime-source-restored.json"),
            serde_json::to_vec_pretty(&snapshot)?,
        )?;
        if current["window"] == previous["window"]
            && current["pid"] == previous["pid"]
            && current["window_thread"] == previous["window_thread"]
            && current["layout"] == previous["layout"]
            && current["active_profile"] == previous["active_profile"]
        {
            return Ok(());
        }
        ensure!(Instant::now() < deadline, "Owned Bevy thread did not restore its exact original input source: previous={previous}, current={current}");
        thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn validate_initial(snapshot: &Value, pid: u32) -> Result<()> {
    let current = &snapshot["ui"]["ime_diagnostics"]["configuration"]["win32"];
    ensure!(current["pid"] == pid && current["foreground"] == true && current["focused"] == true
        && current["layout"].as_u64().is_some_and(|id| id > 0)
        && current["active_profile"]["type"].as_u64().is_some_and(|kind| kind == 1 || kind == 2),
        "Cannot preserve the owned Bevy thread's initial Windows input context; no IME keys sent: {current}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn windows_path_preflight_accepts_canonical_owners_and_rejects_escapes() {
        let output = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("platform/test_native_windows_paths.ps1"),
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("PASS: canonical owned paths"));
    }

    struct HashFixture(PathBuf);

    impl HashFixture {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "limo-cad-ime-provenance-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir(&directory).unwrap();
            Self(directory)
        }

        fn file(&self) -> PathBuf {
            self.0.join("[owned] $hash あ.json")
        }
    }

    impl Drop for HashFixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(self.file());
            let _ = fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn provenance_hash_reads_canonical_unicode_paths_and_complete_file_bytes() {
        let fixture = HashFixture::new();
        for (bytes, expected) in [
            (
                Vec::new(),
                "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
            ),
            (
                b"abc".to_vec(),
                "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD",
            ),
            (
                vec![b'a'; 1_000_000],
                "CDC76E5C9914FB9281A1C7E284D73E67F1809A48A497200E046D39CCC7112CD0",
            ),
        ] {
            fs::write(fixture.file(), bytes).unwrap();
            let canonical = fixture.file().canonicalize().unwrap();
            #[cfg(windows)]
            assert!(canonical.as_os_str().to_string_lossy().starts_with(r"\\?\"));
            assert_eq!(hash(&canonical).unwrap(), expected);
        }
    }

    #[test]
    fn provenance_hash_reports_missing_file_without_accepting_an_empty_digest() {
        let fixture = HashFixture::new();
        let missing = fixture.0.canonicalize().unwrap().join("missing.json");
        let error = hash(&missing).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("Cannot open IME provenance file {}", missing.display())
        );
        assert!(error.chain().any(|cause| cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)));
    }

    fn ready() -> (Value, Value) {
        let field = json!({"control_key":7,"binding":3,"context":{"document_id":"doc","window_id":"main","epoch":2}});
        let value = json!({"control_key":7,"binding":3,"control_label":"Project name","context":field["context"],
            "window":{"ime_enabled":true,"focused":true},
            "native_field_components":{"native_text_field":true,"editable_text":true,"computed_node":true,"ui_transform":true,"render_target":true},
            "win32":{"window":32,"pid":91,"window_thread":92,"foreground":true,"focused":true,"language":0x411,
                "imm":{"context_present":true,"open":true,"context_released":true},
                "active_profile":{"type":1,"language":0x411,"class_id":CLASS,"profile_id":SOURCE}}});
        (field, value)
    }

    #[test]
    fn windows_receipt_requires_actual_profile_focus_and_entire_document_context() {
        let (field, value) = ready();
        assert!(event_context_matches(&value, &field, 32));
        assert!(native_context_ready(&value, &field, 32));
        for pointer in [
            "/context/document_id",
            "/context/window_id",
            "/context/epoch",
            "/binding",
            "/control_key",
            "/win32/window",
            "/win32/foreground",
            "/win32/focused",
            "/win32/language",
            "/win32/active_profile/type",
            "/win32/active_profile/language",
            "/win32/active_profile/class_id",
            "/win32/active_profile/profile_id",
        ] {
            let mut wrong = value.clone();
            *wrong.pointer_mut(pointer).unwrap() = Value::Null;
            assert!(!event_context_matches(&wrong, &field, 32), "{pointer}");
            assert!(!native_context_ready(&wrong, &field, 32), "{pointer}");
        }
        for pointer in [
            "/window/ime_enabled",
            "/win32/imm/context_present",
            "/win32/imm/open",
            "/win32/imm/context_released",
            "/native_field_components/editable_text",
        ] {
            let mut wrong = value.clone();
            *wrong.pointer_mut(pointer).unwrap() = json!(false);
            assert!(!native_context_ready(&wrong, &field, 32), "{pointer}");
        }
    }

    #[test]
    fn stock_inventory_and_zero_key_diagnosis_do_not_pass_actual_input_prerequisite() {
        let report = json!({"status":"profile-diagnosis-complete", "japanese_profile_enabled":true,
            "environment":{"run_id":"123","runner_os":"Windows","runner_environment":"github-hosted","repository":crate::repository::slug(),"repository_id":crate::repository::id()}});
        assert!(!stock_success(&report, "123"));
    }

    #[test]
    fn stock_contract_rejects_late_commit_and_unfinished_cancellation() {
        let report = json!({"status":"stock-control-ime-feasible", "japanese_profile_enabled":true,
            "requested":{"exercise_ime":true},
            "environment":{"run_id":"123","runner_os":"Windows","runner_environment":"github-hosted","repository":crate::repository::slug(),"repository_id":crate::repository::id()},
            "ime":{"status":"stock-control-ime-feasible","native_bevy_validated":false,
                "preedit":"はる","results_before_commit":0,"committed":"はる","cancelled_text":"はる","final_text":"はる",
                "result_count":1,"composition_starts":2,"composition_ends":2,"escape_count":2,
                "received_ime_messages":[{"preedit":"はる"},{"result":"はる"}]}});
        assert!(stock_success(&report, "123"));
        let mut transferred = report.clone();
        transferred["environment"]["repository"] = json!("limo-cad/Limo-CAD");
        assert!(stock_success(&transferred, "123"));
        assert!(!stock_success(&report, "other-run"));
        for (pointer, bad) in [
            ("/environment/repository_id", json!(null)),
            ("/environment/repository_id", json!("1313334316")),
            ("/ime/result_count", json!(2)),
            ("/ime/composition_ends", json!(1)),
            ("/ime/escape_count", json!(3)),
            ("/ime/cancelled_text", json!("はるはる")),
        ] {
            let mut invalid = report.clone();
            *invalid.pointer_mut(pointer).unwrap() = bad;
            assert!(!stock_success(&invalid, "123"), "{pointer}");
        }
    }
}
