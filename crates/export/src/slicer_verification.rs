//! Local, opt-in Bambu CLI verification. The only child commands import and slice owned copies.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_PROJECT_BYTES: usize = 128 * 1024 * 1024;
const MAX_LOG_BYTES: usize = 64 * 1024;
const MAX_OUTPUT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_JOBS: usize = 16;
const QUALIFIED_VERSION: &str = "02.08.02.61";

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Hashes captured by the owning engine, not supplied as unverified labels by the caller.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationIdentity {
    pub source_document_id: String,
    pub project_sha256: String,
    pub source_model_sha256: String,
    #[serde(default)]
    pub source_geometry_revision: Option<u64>,
    pub print_intent_sha256: String,
    pub source_layout_sha256: String,
    pub resolved_layout_sha256: String,
    pub named_view: Option<String>,
    pub profile_sha256: String,
}
impl VerificationIdentity {
    pub fn from_owned_export(
        project: &[u8],
        model_json: &str,
        layout: &serde_json::Value,
        exported_layout: &serde_json::Value,
        profile_sha256: String,
        source_document_id: String,
        named_view: Option<String>,
    ) -> Result<Self, String> {
        let model: serde_json::Value =
            serde_json::from_str(model_json).map_err(|e| e.to_string())?;
        if model["print_intent"]["source_document_id"].as_str() != Some(source_document_id.as_str())
        {
            return Err(
                "Verification source identity does not match the owning CAD document".into(),
            );
        }
        Ok(Self {
            source_document_id,
            project_sha256: sha256(project),
            source_model_sha256: model_sha256(model_json)?,
            source_geometry_revision: None,
            print_intent_sha256: sha256(
                &serde_json::to_vec(&model["print_intent"]).map_err(|e| e.to_string())?,
            ),
            source_layout_sha256: sha256(&serde_json::to_vec(layout).map_err(|e| e.to_string())?),
            resolved_layout_sha256: sha256(
                &serde_json::to_vec(exported_layout).map_err(|e| e.to_string())?,
            ),
            profile_sha256,
            named_view,
        })
    }
}
/// Hash the actual target transforms and plate/group assignments written by the shared writer.
pub fn resolved_bambu_layout(
    report: &crate::bambu_project::BambuProjectReport,
) -> serde_json::Value {
    serde_json::json!({"placement":report.placement,"parts":report.parts.iter().map(|part| serde_json::json!({
        "source_body":part.binding.body_id,"source_occurrence":part.binding.occurrence_id,
        "slicer_object":part.binding.object_id,"instance":part.binding.instance_id,"volume":part.binding.part_id,
        "target_uuid":part.target_uuid,"plate":part.plate_index,"transform":part.world_transform
    })).collect::<Vec<_>>()})
}

pub fn model_sha256(model_json: &str) -> Result<String, String> {
    let model: serde_json::Value = serde_json::from_str(model_json).map_err(|e| e.to_string())?;
    Ok(sha256(
        &serde_json::to_vec(&model).map_err(|e| e.to_string())?,
    ))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalSlicerStartRequest {
    pub project: crate::BambuExportRequest,
    pub options: LocalSlicerOptions,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalSlicerOptions {
    /// Explicit local executable. Arguments and output paths cannot be supplied by the caller.
    pub executable: PathBuf,
    #[serde(default = "default_timeout")]
    pub timeout_seconds_per_plate: u32,
}
fn default_timeout() -> u32 {
    120
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationState {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlateVerificationState {
    NotRun,
    ToolpathsGenerated,
    Failed,
    TimedOut,
    Cancelled,
    MissingSlicer,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlateVerification {
    pub plate: u32,
    pub state: PlateVerificationState,
    pub exit_status: Option<i32>,
    pub elapsed_milliseconds: u64,
    pub slicer_version: Option<String>,
    pub toolpath_sha256: Option<String>,
    pub native_result: Option<serde_json::Value>,
    pub native_effective_settings: Option<serde_json::Value>,
    pub toolpaths_generated: bool,
    pub native_project_read_back: bool,
    pub compatibility_issues: Vec<String>,
    pub setting_changes: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub message: Option<String>,
}
impl PlateVerification {
    fn not_run(plate: u32) -> Self {
        Self {
            plate,
            state: PlateVerificationState::NotRun,
            exit_status: None,
            elapsed_milliseconds: 0,
            slicer_version: None,
            toolpath_sha256: None,
            native_result: None,
            native_effective_settings: None,
            toolpaths_generated: false,
            native_project_read_back: false,
            compatibility_issues: Vec::new(),
            setting_changes: Vec::new(),
            stdout: String::new(),
            stderr: String::new(),
            message: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalSlicerReport {
    pub job_id: u64,
    pub state: VerificationState,
    pub identity: VerificationIdentity,
    pub executable: PathBuf,
    pub executable_sha256: Option<String>,
    pub plates: Vec<PlateVerification>,
    pub stale: bool,
    pub physical_qualification: String,
    pub warnings: Vec<String>,
}

impl LocalSlicerReport {
    pub fn check_current_geometry_revision(&mut self, revision: u64) {
        self.stale |= self
            .identity
            .source_geometry_revision
            .is_some_and(|captured| captured != revision);
    }
    /// Resolve the same saved/current presentation layout on the owning engine during polling.
    pub fn check_current_layout(&mut self, layout: Result<serde_json::Value, String>) {
        match layout.and_then(|value| serde_json::to_vec(&value).map_err(|e| e.to_string())) {
            Ok(bytes) => self.stale |= sha256(&bytes) != self.identity.source_layout_sha256,
            Err(error) => {
                self.stale = true;
                self.warnings
                    .push(format!("Current layout unavailable: {error}"));
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CancellationReceipt {
    pub job_id: u64,
    pub cancel_requested: bool,
}

struct Job {
    owner_key: String,
    report: Mutex<LocalSlicerReport>,
    cancel: AtomicBool,
}
#[derive(Default)]
pub struct LocalSlicerService {
    jobs: Mutex<BTreeMap<u64, Arc<Job>>>,
    next: AtomicU64,
}
pub fn local_slicer_service() -> &'static LocalSlicerService {
    static SERVICE: OnceLock<LocalSlicerService> = OnceLock::new();
    SERVICE.get_or_init(LocalSlicerService::default)
}

pub fn new_verification_owner() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "headless-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

impl LocalSlicerService {
    /// Start validation of a validated unsliced project. This never opens the user's active file.
    pub fn start(
        &self,
        bytes: Vec<u8>,
        identity: VerificationIdentity,
        plate_count: u32,
        options: LocalSlicerOptions,
        owner_key: String,
    ) -> Result<LocalSlicerReport, String> {
        self.start_with_warnings(bytes, identity, plate_count, options, owner_key, Vec::new())
    }

    /// Retain the shared writer's unsupported/unmanaged limitations in local evidence.
    pub fn start_with_warnings(
        &self,
        bytes: Vec<u8>,
        identity: VerificationIdentity,
        plate_count: u32,
        options: LocalSlicerOptions,
        owner_key: String,
        source_warnings: Vec<String>,
    ) -> Result<LocalSlicerReport, String> {
        if bytes.is_empty() || bytes.len() > MAX_PROJECT_BYTES {
            return Err("Project must be 1–128 MiB".into());
        }
        if identity.project_sha256 != sha256(&bytes) {
            return Err("Project hash does not match the exact validation artifact".into());
        }
        if !(1..=64).contains(&plate_count) {
            return Err("Local verification supports 1–64 plates".into());
        }
        if !(1..=600).contains(&options.timeout_seconds_per_plate) {
            return Err("Per-plate timeout must be 1–600 seconds".into());
        }
        if !options.executable.is_absolute() {
            return Err("Choose an absolute local Bambu Studio executable path".into());
        }
        let mut jobs = self
            .jobs
            .lock()
            .map_err(|_| "Verification registry lock poisoned")?;
        if jobs
            .values()
            .filter(|job| {
                job.report.lock().is_ok_and(|r| {
                    matches!(
                        r.state,
                        VerificationState::Queued | VerificationState::Running
                    )
                })
            })
            .count()
            >= 2
        {
            return Err("Two local validations are already active; cancel or await one".into());
        }
        while jobs.len() >= MAX_JOBS {
            let completed = jobs
                .iter()
                .find(|(_, job)| {
                    job.report.lock().is_ok_and(|r| {
                        !matches!(
                            r.state,
                            VerificationState::Queued | VerificationState::Running
                        )
                    })
                })
                .map(|(id, _)| *id);
            if let Some(id) = completed {
                jobs.remove(&id);
            } else {
                return Err("Verification registry is full".into());
            }
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let mut report = LocalSlicerReport { job_id: id, state: VerificationState::Queued, identity,
            executable: options.executable.clone(), executable_sha256: None,
            plates: (1..=plate_count).map(PlateVerification::not_run).collect(), stale: false,
            physical_qualification: "not_run".into(), warnings: vec![
                "Toolpaths do not qualify physical fit or strength. Requested walls are not guaranteed realized loops in thin sections.".into(),
                "Local slicing operates on a temporary copy; no print, printer or cloud command is issued.".into()] };
        let mut omitted = source_warnings.len() > 64;
        for warning in source_warnings.into_iter().take(64) {
            let mut characters = warning.chars();
            let warning: String = characters.by_ref().take(2048).collect();
            omitted |= characters.next().is_some();
            if !report.warnings.contains(&warning) {
                report.warnings.push(warning);
            }
        }
        if omitted {
            report.warnings.push("Additional writer limitations were truncated; review the complete export report before using this evidence".into());
        }
        let job = Arc::new(Job {
            owner_key,
            report: Mutex::new(report.clone()),
            cancel: AtomicBool::new(false),
        });
        jobs.insert(id, job.clone());
        thread::Builder::new()
            .name(format!("local-slicer-{id}"))
            .spawn(move || run_job(&job, bytes, options))
            .map_err(|e| {
                jobs.remove(&id);
                format!("Cannot start verification worker: {e}")
            })?;
        Ok(report)
    }

    /// A newly written artifact invalidates evidence for another output of the same owned CAD document.
    /// Prior identities, toolpaths and reports remain available as explicitly stale evidence.
    pub fn note_owned_export(
        &self,
        owner_key: &str,
        source_document_id: &str,
        artifact_sha256: &str,
    ) -> Result<(), String> {
        let jobs = self
            .jobs
            .lock()
            .map_err(|_| "Verification registry lock poisoned")?;
        for job in jobs.values().filter(|job| job.owner_key == owner_key) {
            let mut report = job
                .report
                .lock()
                .map_err(|_| "Verification report lock poisoned")?;
            if report.identity.source_document_id == source_document_id
                && report.identity.project_sha256 != artifact_sha256
            {
                report.stale = true;
            }
        }
        Ok(())
    }

    /// Observe committed owning-model changes even when no poll occurs before Undo.
    /// Serialize lazily only if this private owner has captured evidence.
    pub fn observe_owned_model(
        &self,
        owner_key: &str,
        model: impl FnOnce() -> Result<String, String>,
    ) -> Result<(), String> {
        let jobs = self
            .jobs
            .lock()
            .map_err(|_| "Verification registry lock poisoned")?;
        if !jobs.values().any(|job| job.owner_key == owner_key) {
            return Ok(());
        }
        let current = model().and_then(|model| model_sha256(&model));
        for job in jobs.values().filter(|job| job.owner_key == owner_key) {
            let mut report = job
                .report
                .lock()
                .map_err(|_| "Verification report lock poisoned")?;
            match &current {
                Ok(hash) => report.stale |= report.identity.source_model_sha256 != *hash,
                Err(_) => {
                    report.stale = true;
                    let warning =
                        "Current owning model could not be observed; prior evidence is stale";
                    if report.warnings.len() < 64
                        && !report.warnings.iter().any(|value| value == warning)
                    {
                        report.warnings.push(warning.into());
                    }
                }
            }
        }
        Ok(())
    }

    /// Persist a layout/geometry observation without changing the original identity.
    pub fn note_owned_stale(
        &self,
        id: u64,
        source_document_id: &str,
        owner_key: &str,
    ) -> Result<(), String> {
        let job = self.job(id)?;
        if job.owner_key != owner_key {
            return Err("Verification belongs to a different owning engine session".into());
        }
        let mut report = job
            .report
            .lock()
            .map_err(|_| "Verification report lock poisoned")?;
        if report.identity.source_document_id != source_document_id {
            return Err("Verification belongs to a different CAD document".into());
        }
        report.stale = true;
        Ok(())
    }

    /// Current engine hashes mark evidence stale without changing or reinterpreting prior results.
    pub fn poll(
        &self,
        id: u64,
        current: Option<&VerificationIdentity>,
    ) -> Result<LocalSlicerReport, String> {
        let job = self.job(id)?;
        let mut report = job
            .report
            .lock()
            .map_err(|_| "Verification report lock poisoned")?;
        if let Some(current) = current {
            report.stale |= report.identity != *current;
        }
        Ok(report.clone())
    }
    pub fn cancel(&self, id: u64) -> Result<LocalSlicerReport, String> {
        let job = self.job(id)?;
        job.cancel.store(true, Ordering::Release);
        self.poll(id, None)
    }
    pub fn poll_owned(
        &self,
        id: u64,
        source_document_id: &str,
        current_model_json: &str,
        owner_key: &str,
        cancel: bool,
    ) -> Result<LocalSlicerReport, String> {
        let job = self.job(id)?;
        if job.owner_key != owner_key {
            return Err("Verification belongs to a different owning engine session".into());
        }
        let mut report = job
            .report
            .lock()
            .map_err(|_| "Verification report lock poisoned")?;
        if report.identity.source_document_id != source_document_id {
            return Err("Verification belongs to a different CAD document".into());
        }
        report.stale |= report.identity.source_model_sha256 != model_sha256(current_model_json)?;
        if cancel {
            job.cancel.store(true, Ordering::Release);
        }
        Ok(report.clone())
    }
    /// The owning engine tab can cancel its child after replacing its CAD document.
    /// A receipt reveals no prior document's report or settings.
    pub fn cancel_owned(&self, id: u64, owner_key: &str) -> Result<CancellationReceipt, String> {
        let job = self.job(id)?;
        if job.owner_key != owner_key {
            return Err("Verification belongs to a different owning engine session".into());
        }
        job.cancel.store(true, Ordering::Release);
        Ok(CancellationReceipt {
            job_id: id,
            cancel_requested: true,
        })
    }
    fn job(&self, id: u64) -> Result<Arc<Job>, String> {
        self.jobs
            .lock()
            .map_err(|_| "Verification registry lock poisoned")?
            .get(&id)
            .cloned()
            .ok_or_else(|| "Unknown or expired local verification job".into())
    }
}

fn update(job: &Job, f: impl FnOnce(&mut LocalSlicerReport)) {
    if let Ok(mut report) = job.report.lock() {
        f(&mut report);
    }
}

fn run_job(job: &Job, bytes: Vec<u8>, options: LocalSlicerOptions) {
    update(job, |report| report.state = VerificationState::Running);
    let result = run_job_inner(job, bytes, &options);
    update(job, |report| {
        if let Err(error) = result {
            report.warnings.push(error);
            report.state = VerificationState::Failed;
        } else if job.cancel.load(Ordering::Acquire) {
            report.state = VerificationState::Cancelled;
        } else if report
            .plates
            .iter()
            .all(|p| p.state == PlateVerificationState::ToolpathsGenerated)
        {
            report.state = VerificationState::Completed;
        } else {
            report.state = VerificationState::Failed;
        }
    });
}

struct OwnedDirectory(PathBuf);
impl OwnedDirectory {
    fn create() -> Result<Self, String> {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let root = std::env::temp_dir()
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let path = root.join(format!(
            "limo-cad-slice-{}-{stamp}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)
            .map_err(|e| format!("Cannot create temporary validation directory: {e}"))?;
        Ok(Self(path))
    }
}
impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        // Only the directory created above is removed, after our child has exited and been reaped.
        if self
            .0
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("limo-cad-slice-"))
        {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn cli_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        // Canonical Rust paths use the Win32 extended namespace, unsupported by the slicer's parser.
        let text = path.to_string_lossy();
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        if let Some(local) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(local);
        }
    }
    path.to_owned()
}

fn fingerprint_executable(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("Cannot fingerprint selected slicer: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_OUTPUT_BYTES {
            return Err("Selected executable exceeds the 512 MiB fingerprint limit".into());
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn run_job_inner(job: &Job, bytes: Vec<u8>, options: &LocalSlicerOptions) -> Result<(), String> {
    if job.cancel.load(Ordering::Acquire) {
        update(job, |report| {
            for plate in &mut report.plates {
                plate.state = PlateVerificationState::Cancelled;
            }
        });
        return Ok(());
    }
    if !options.executable.is_file() {
        update(job, |report| {
            for plate in &mut report.plates {
                plate.state = PlateVerificationState::MissingSlicer;
                plate.message = Some(
                    "Bambu Studio was not found; export remains usable without local verification"
                        .into(),
                );
            }
        });
        return Ok(());
    }
    if std::fs::metadata(&options.executable)
        .map_err(|e| e.to_string())?
        .len()
        > MAX_OUTPUT_BYTES
    {
        return Err("Selected executable exceeds the 512 MiB fingerprint limit".into());
    }
    let executable_sha256 = fingerprint_executable(&options.executable)?;
    update(job, |report| {
        report.executable_sha256 = Some(executable_sha256)
    });
    let directory = OwnedDirectory::create()?;
    let input = directory.0.join("input.3mf");
    std::fs::write(&input, bytes).map_err(|e| e.to_string())?;
    let count = job
        .report
        .lock()
        .map_err(|_| "Verification lock poisoned")?
        .plates
        .len();
    for index in 0..count {
        if job.cancel.load(Ordering::Acquire) {
            update(job, |r| {
                for p in &mut r.plates[index..] {
                    p.state = PlateVerificationState::Cancelled;
                }
            });
            break;
        }
        let output = directory.0.join(format!("plate-{}", index + 1));
        std::fs::create_dir(&output).map_err(|e| e.to_string())?;
        let mut command = Command::new(&options.executable);
        command
            .args(["--arrange", "0", "--slice"])
            .arg((index + 1).to_string())
            .arg("--outputdir")
            .arg(cli_path(&output))
            .arg("--export-3mf")
            .arg("resliced.3mf")
            .arg(cli_path(&input))
            .current_dir(&output)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut result = run_plate(
            command,
            &output,
            index as u32 + 1,
            Duration::from_secs(options.timeout_seconds_per_plate.into()),
            &job.cancel,
        );
        if result.state == PlateVerificationState::ToolpathsGenerated {
            apply_native_readback(
                &mut result,
                native_setting_readback(&input, &output.join("resliced.3mf"), index as u32 + 1),
            );
        }
        update(job, |r| r.plates[index] = result);
    }
    Ok(())
}

fn apply_native_readback(
    result: &mut PlateVerification,
    readback: Result<(serde_json::Value, Vec<String>, Vec<String>), String>,
) {
    match readback {
        Ok((values, changes, issues)) => {
            result.native_project_read_back = true;
            result.native_effective_settings = Some(values);
            result.setting_changes = changes;
            if !issues.is_empty() {
                result.state = PlateVerificationState::Failed;
                result.message=Some("Native geometry, placement, mappings, quantity or managed settings changed; review compatibility issues before using these toolpaths".into());
                result.compatibility_issues = issues;
            }
        }
        Err(error) => {
            result.state = PlateVerificationState::Failed;
            result.message = Some(format!(
                "Native project readback failed: {error}; generated toolpaths are retained as unverified evidence"
            ));
            result.compatibility_issues.push(error);
        }
    }
}

fn native_setting_readback(
    input: &Path,
    output: &Path,
    plate: u32,
) -> Result<(serde_json::Value, Vec<String>, Vec<String>), String> {
    let read = |path: &Path, selected_plate: Option<u32>| -> Result<_, String> {
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > MAX_PROJECT_BYTES as u64 {
            return Err("Native saved project exceeds 128 MiB readback limit".into());
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let summary = match selected_plate {
            Some(plate) => crate::bambu_project::inspect_bambu_plate(&bytes, plate),
            None => crate::bambu_project::inspect_bambu_template(&bytes),
        }
        .map_err(|error| error.to_string())?;
        Ok((bytes, summary))
    };
    let (before_bytes, before) = read(input, Some(plate))?;
    let (after_bytes, after) = read(output, None)?;
    let collect = |summary: &crate::bambu_project::BambuTemplateSummary| -> Result<BTreeMap<String,serde_json::Value>,String> {
        let defaults = serde_json::to_value(&summary.process_defaults).map_err(|e| e.to_string())?;
        let mut settings = BTreeMap::from([("process_defaults".into(), defaults),
            ("native_process_settings".into(),serde_json::json!(summary.native_process_settings)),
            ("process_capability_warnings".into(),serde_json::json!(summary.process_capability_warnings)),
            ("profile_mappings".into(),serde_json::json!({"printer_settings_id":summary.printer_settings_id,
            "printer_model":summary.printer_model,"printer_variant":summary.printer_variant,"process_settings_id":summary.process_settings_id,"nozzle_diameter_mm":summary.nozzle_diameter_mm,
            "filament_settings_ids":summary.filament_settings_ids,"filament_types":summary.filament_types,"filament_colors":summary.filament_colors,
            "support_filament":summary.support_filament,"support_interface_filament":summary.support_interface_filament,
            "filament_map":summary.filament_map,"filament_nozzle_map":summary.filament_nozzle_map}))]);
        for object in &summary.objects {
            for part in &object.parts {
                let uuid = part.uuid.as_ref().ok_or("Native volume lacks a stable UUID")?;
                let (native_values, native_sources) = crate::bambu_project::native_effective_settings(
                    &summary.native_process_settings, &object.settings, &part.settings,
                ).map_err(|error| error.to_string())?;
                let mut values = BTreeMap::new();
                for key in crate::bambu_project::SETTING_KEYS {
                    let value = summary.native_process_settings.get(key)
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| format!("Native process readback requires scalar '{key}'; this handoff is unqualified"))?;
                    values.insert(key.to_owned(), value.to_owned());
                }
                for scoped in [&object.settings, &part.settings] {
                    for key in ["wall_loops","sparse_infill_density","sparse_infill_pattern","top_shell_layers","bottom_shell_layers"] {
                        if let Some(value) = scoped.get(key) { values.insert(key.to_owned(), value.clone()); }
                    }
                }
                let key = format!("volume:{uuid}");
                if settings.insert(key,serde_json::json!({"settings":values,"native_settings":native_values,"native_sources":native_sources,"instance_count":object.instance_count,"subtype":part.subtype})).is_some() {
                    return Err("Native volume identity is ambiguous; effective readback requires review".into());
                }
            }
        }
        Ok(settings)
    };
    let expected = collect(&before)?;
    let actual = collect(&after)?;
    let mut issues = mapping_compatibility_issues(&expected, &actual);
    let (geometry, geometry_issues) = native_geometry_readback(&before_bytes, &after_bytes, plate)?;
    issues.extend(geometry_issues);
    let changes = expected
        .keys()
        .chain(actual.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|key| expected.get(*key) != actual.get(*key))
        .map(|key| {
            format!(
                "{key}: writer {} → native {}",
                expected.get(key).unwrap_or(&serde_json::Value::Null),
                actual.get(key).unwrap_or(&serde_json::Value::Null)
            )
        })
        .collect();
    Ok((
        serde_json::json!({"effective":actual,"geometry":geometry}),
        changes,
        issues,
    ))
}

fn owned_refresh_reference(
    bytes: &[u8],
) -> Result<crate::bambu_project::BambuRefreshReference, String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|error| error.to_string())?;
    let mut entry = archive
        .by_name("Metadata/limo_cad_project.json")
        .map_err(|_| "Owned verification artifact is missing its CAD refresh identity")?;
    if entry.size() > 16 * 1024 * 1024 {
        return Err("Owned verification reference exceeds 16 MiB".into());
    }
    let mut metadata = Vec::new();
    entry
        .read_to_end(&mut metadata)
        .map_err(|error| error.to_string())?;
    let reference: crate::bambu_project::BambuRefreshReference =
        serde_json::from_slice(&metadata).map_err(|error| error.to_string())?;
    reference.validate()?;
    Ok(reference)
}

// Native --slice N keeps only N. Project cross-plate repeats through source
// identities while retaining every normal sibling of each selected instance.
fn project_height_reference(
    reference: &mut crate::bambu_project::BambuRefreshReference,
) -> Result<serde_json::Value, String> {
    use limo_cad_core::PrintSourceOccurrenceDto;
    let pair = |part: &limo_cad_core::BambuRefreshPart| PrintSourceOccurrenceDto {
        body_id: part.binding.body_id,
        occurrence_id: part.binding.occurrence_id,
    };
    let selected: std::collections::BTreeSet<_> = reference.parts.iter().map(pair).collect();
    let mut groups: BTreeMap<_, std::collections::BTreeSet<_>> = BTreeMap::new();
    for part in &reference.parts {
        groups
            .entry((part.binding.object_id, part.binding.instance_id))
            .or_default()
            .insert(pair(part));
    }
    let mut projected = Vec::new();
    let mut evidence = Vec::new();
    for original in &reference.height_objects {
        let mut record = original.clone();
        record
            .source_bindings
            .retain(|source| selected.contains(source));
        if record.source_bindings.is_empty() {
            continue;
        }
        let members: std::collections::BTreeSet<_> =
            record.source_bindings.iter().copied().collect();
        for group in groups.values() {
            if !group.is_disjoint(&members) && !group.is_subset(&members) {
                return Err("Selected native height object omits normal CAD siblings; review the complete multipart group before verification".into());
            }
        }
        evidence.push(
            serde_json::json!({"original_source_bindings":original.source_bindings,
            "selected_source_bindings":record.source_bindings}),
        );
        projected.push(record);
    }
    reference.height_objects = projected;
    Ok(serde_json::Value::Array(evidence))
}

fn native_geometry_readback(
    before_bytes: &[u8],
    after_bytes: &[u8],
    plate: u32,
) -> Result<(serde_json::Value, Vec<String>), String> {
    use crate::bambu_project::{
        equivalent_bambu_world_geometry, inspect_bambu_height_reference,
        read_bambu_volume_geometry, verify_bambu_modifier_reference, BambuVolumeGeometry,
    };
    let mut reference = owned_refresh_reference(before_bytes)?;
    let before: Vec<_> = read_bambu_volume_geometry(before_bytes)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|volume| volume.plate_index == plate)
        .collect();
    let after = read_bambu_volume_geometry(after_bytes).map_err(|error| error.to_string())?;
    if before.is_empty() {
        return Err("Selected source plate contains no reviewed geometry".into());
    }
    let expected_normal: std::collections::BTreeSet<_> = before
        .iter()
        .filter(|volume| volume.subtype == "normal_part")
        .map(|volume| (volume.target_uuid.as_deref(), volume.instance_identify_id))
        .collect();
    reference.parts.retain(|part| {
        expected_normal.contains(&(Some(part.target_uuid.as_str()), part.instance_identify_id))
    });
    let parent_uuids: std::collections::BTreeSet<_> = reference
        .parts
        .iter()
        .map(|part| part.target_uuid.as_str())
        .collect();
    reference
        .modifiers
        .retain(|modifier| parent_uuids.contains(modifier.parent_volume_uuid.as_str()));
    let height_projection = project_height_reference(&mut reference)?;
    reference.validate()?;
    let written_heights = inspect_bambu_height_reference(before_bytes, &reference)
        .map_err(|error| format!("Reviewed height metadata is invalid: {error}"))?;
    let mut issues = Vec::new();
    let native_heights = match inspect_bambu_height_reference(after_bytes, &reference) {
        Ok(values) => Some(values),
        Err(error) => {
            issues.push(format!(
                "Native height ranges, speeds or layer profile changed: {error}"
            ));
            None
        }
    };
    if let Err(error) = verify_bambu_modifier_reference(after_bytes, &reference) {
        issues.push(format!(
            "Native modifier identity, geometry, attachment or settings changed: {error}"
        ));
    }
    let key = |volume: &BambuVolumeGeometry| -> Result<(String, u32), String> {
        Ok((
            volume
                .target_uuid
                .clone()
                .ok_or("Native volume lacks a stable UUID for geometry verification")?,
            volume.instance_identify_id,
        ))
    };
    let groups = |volumes: &[BambuVolumeGeometry]| {
        let mut by_object: BTreeMap<u32, std::collections::BTreeSet<String>> = BTreeMap::new();
        for volume in volumes
            .iter()
            .filter(|volume| volume.subtype == "normal_part")
        {
            if let Some(uuid) = &volume.target_uuid {
                by_object
                    .entry(volume.object_id)
                    .or_default()
                    .insert(uuid.clone());
            }
        }
        by_object
    };
    let before_groups = groups(&before);
    let after_groups = groups(&after);
    let native_plates: std::collections::BTreeSet<_> =
        after.iter().map(|volume| volume.plate_index).collect();
    let selected_plate_normalized =
        native_plates == std::collections::BTreeSet::from([1]) && plate != 1;
    let mut actual = BTreeMap::new();
    for volume in &after {
        if actual.insert(key(volume)?, volume).is_some() {
            return Err("Native volume/instance identity is ambiguous after slicing".into());
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut records = Vec::new();
    for source in &before {
        let identity = key(source)?;
        if !seen.insert(identity.clone()) {
            return Err("Reviewed volume/instance identity is ambiguous".into());
        }
        let target = actual.remove(&identity);
        let binding = if source.subtype == "normal_part" {
            reference
                .parts
                .iter()
                .find(|part| {
                    part.target_uuid == identity.0 && part.instance_identify_id == identity.1
                })
                .map(|part| part.binding.clone())
        } else {
            reference
                .modifiers
                .iter()
                .find(|modifier| modifier.target_uuid == identity.0)
                .and_then(|modifier| {
                    reference.parts.iter().find(|part| {
                        part.target_uuid == modifier.parent_volume_uuid
                            && part.instance_identify_id == identity.1
                    })
                })
                .map(|part| part.binding.clone())
        };
        if binding.is_none() {
            issues.push(format!(
                "Reviewed native volume {} instance{} has no explicit CAD source binding",
                identity.0, identity.1
            ));
        }
        let geometry_matches = match target {
            Some(target) => {
                if source.subtype != target.subtype {
                    issues.push(format!("Native volume type changed: {}", identity.0));
                }
                if source.plate_index != target.plate_index && !selected_plate_normalized {
                    issues.push(format!(
                        "Native plate assignment changed: {} instance{}",
                        identity.0, identity.1
                    ));
                }
                if source.subtype == "normal_part"
                    && before_groups.get(&source.object_id) != after_groups.get(&target.object_id)
                {
                    issues.push(format!("Native multipart grouping changed: {}", identity.0));
                }
                equivalent_bambu_world_geometry(source, target)
                    .map_err(|error| error.to_string())?
            }
            None => {
                issues.push(format!("Native volume or instance identity missing: {} identify{}; inspect/rebind instead of guessing native order",identity.0,identity.1));
                false
            }
        };
        if !geometry_matches {
            issues.push(format!("Native world geometry or placement could not be verified within 0.001 mm: {} instance{}",identity.0,identity.1));
        }
        let describe = |volume: &BambuVolumeGeometry| {
            let mut geometry = Sha256::new();
            for value in &volume.positions {
                geometry.update(value.to_le_bytes());
            }
            for index in &volume.indices {
                geometry.update(index.to_le_bytes());
            }
            let geometry_sha256 = geometry
                .clone()
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            for value in volume.world_transform {
                geometry.update(value.to_le_bytes());
            }
            serde_json::json!({"object_id":volume.object_id,"part_id":volume.part_id,"instance_id":volume.instance_id,"instance_identify_id":volume.instance_identify_id,"target_uuid":volume.target_uuid,
                "subtype":volume.subtype,"plate":volume.plate_index,"triangle_count":volume.indices.len()/3,"geometry_sha256":geometry_sha256,
                "resolved_export_sha256":geometry.finalize().iter().map(|byte|format!("{byte:02x}")).collect::<String>(),"world_transform":volume.world_transform,"world_bounds":volume.world_bounds})
        };
        records.push(serde_json::json!({"source_binding":binding,"written":describe(source),"native":target.map(describe),"world_geometry_verified":geometry_matches}));
    }
    for ((uuid, identify), _) in actual {
        issues.push(format!(
            "Unexpected native volume or instance added: {uuid} identify{identify}"
        ));
    }
    Ok((
        serde_json::json!({"source_plate":plate,"native_plate_index_normalized":selected_plate_normalized,"written_project_sha256":sha256(before_bytes),"native_project_sha256":sha256(after_bytes),"tolerance_mm":0.001,"volumes":records,"modifier_reference_verified":!issues.iter().any(|issue|issue.starts_with("Native modifier")),"height_reference_verified":native_heights.is_some(),"heights":{"source_projection":height_projection,"written":written_heights,"native":native_heights},"physical_qualification":"not_run"}),
        issues,
    ))
}

fn mapping_compatibility_issues(
    expected: &BTreeMap<String, serde_json::Value>,
    actual: &BTreeMap<String, serde_json::Value>,
) -> Vec<String> {
    let mut issues = Vec::new();
    if expected.get("profile_mappings") != actual.get("profile_mappings") {
        issues.push("Native printer, nozzle, process, filament or support mapping differs from the reviewed project".into());
    }
    if actual
        .get("process_capability_warnings")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|warnings| !warnings.is_empty())
    {
        issues.push("Native process contains values outside typed CAD capability; read-only inspection is available but this managed handoff is not qualified".into());
    }
    for key in expected
        .keys()
        .chain(actual.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        if key.starts_with("volume:") {
            match (expected.get(key), actual.get(key)) {
                (Some(before), Some(after))
                    if before["instance_count"] == after["instance_count"] => {}
                _ => issues.push(format!("Native volume identity or quantity changed: {key}")),
            }
            if let (Some(before), Some(after)) = (expected.get(key), actual.get(key)) {
                if before["subtype"] != "normal_part" {
                    continue;
                }
                for field in [
                    "wall_loops",
                    "sparse_infill_density",
                    "sparse_infill_pattern",
                    "top_shell_layers",
                    "bottom_shell_layers",
                ] {
                    let left = &before["settings"][field];
                    let right = &after["settings"][field];
                    let same = if field == "sparse_infill_pattern" {
                        left == right
                    } else {
                        let number = |value: &serde_json::Value| {
                            value
                                .as_str()
                                .and_then(|value| value.trim_end_matches('%').parse::<f64>().ok())
                                .filter(|value| value.is_finite())
                        };
                        match (number(left), number(right)) {
                            (Some(left), Some(right)) => (left - right).abs() <= 1e-6,
                            _ => left == right,
                        }
                    };
                    if !same {
                        issues.push(format!("Native effective setting changed: {key} {field}: writer {left} -> native {right}; review/reslice before qualifying the requested project"));
                    }
                }
                for field in crate::bambu_project::NATIVE_PROCESS_KEYS {
                    if [
                        "wall_loops",
                        "sparse_infill_density",
                        "sparse_infill_pattern",
                        "top_shell_layers",
                        "bottom_shell_layers",
                    ]
                    .contains(&field)
                    {
                        continue;
                    }
                    let left = &before["native_settings"][field];
                    let right = &after["native_settings"][field];
                    if !native_metadata_values_equal(left, right) {
                        issues.push(format!("Native read-only process setting changed: {key} {field}: writer {left} -> native {right}; review/reslice before qualifying this handoff"));
                    }
                }
            }
        }
    }
    issues
}

fn native_metadata_values_equal(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    match (left, right) {
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| native_metadata_values_equal(left, right))
        }
        (serde_json::Value::String(left), serde_json::Value::String(right)) => {
            let number = |value: &str| {
                value
                    .trim_end_matches('%')
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
            };
            match (number(left), number(right)) {
                (Some(left_number), Some(right_number))
                    if left.ends_with('%') == right.ends_with('%') =>
                {
                    (left_number - right_number).abs() <= 1e-6
                }
                _ => left == right,
            }
        }
        _ => left == right,
    }
}

struct CapturedPipe {
    captured: Arc<Mutex<Vec<u8>>>,
    worker: thread::JoinHandle<()>,
}
fn bounded_pipe(mut pipe: impl Read + Send + 'static) -> CapturedPipe {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let copy = captured.clone();
    let worker = thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        while let Ok(count) = pipe.read(&mut buffer) {
            if count == 0 {
                break;
            }
            if let Ok(mut captured) = copy.lock() {
                let keep = count.min(MAX_LOG_BYTES.saturating_sub(captured.len()));
                captured.extend_from_slice(&buffer[..keep]);
            }
        }
    });
    CapturedPipe { captured, worker }
}
fn finish_pipe(pipe: Option<CapturedPipe>) -> String {
    let Some(pipe) = pipe else {
        return String::new();
    };
    // A descendant inheriting a pipe must never extend the validation time bound.
    let deadline = Instant::now() + Duration::from_millis(100);
    while !pipe.worker.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    if pipe.worker.is_finished() {
        let _ = pipe.worker.join();
    }
    pipe.captured
        .lock()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

fn output_size(path: &Path) -> u64 {
    let mut pending = vec![path.to_owned()];
    let mut entries_seen = 0usize;
    let mut total = 0u64;
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return MAX_OUTPUT_BYTES + 1;
        };
        for entry in entries {
            entries_seen += 1;
            if entries_seen > 10_000 {
                return MAX_OUTPUT_BYTES + 1;
            }
            let Ok(entry) = entry else {
                return MAX_OUTPUT_BYTES + 1;
            };
            let Ok(kind) = entry.file_type() else {
                return MAX_OUTPUT_BYTES + 1;
            };
            if kind.is_symlink() {
                return MAX_OUTPUT_BYTES + 1;
            }
            if kind.is_dir() {
                pending.push(entry.path());
            } else {
                let Ok(metadata) = entry.metadata() else {
                    return MAX_OUTPUT_BYTES + 1;
                };
                total = total.saturating_add(metadata.len());
                if total > MAX_OUTPUT_BYTES {
                    return total;
                }
            }
        }
    }
    total
}

fn run_plate(
    mut command: Command,
    directory: &Path,
    plate: u32,
    timeout: Duration,
    cancel: &AtomicBool,
) -> PlateVerification {
    let mut result = PlateVerification::not_run(plate);
    let start = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            result.state = PlateVerificationState::Failed;
            result.message = Some(format!("Cannot launch Bambu Studio: {error}"));
            return result;
        }
    };
    let stdout = child.stdout.take().map(bounded_pipe);
    let stderr = child.stderr.take().map(bounded_pipe);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                result.exit_status = status.code();
                result.state = if status.success() {
                    PlateVerificationState::ToolpathsGenerated
                } else {
                    PlateVerificationState::Failed
                };
                break;
            }
            Ok(None) => {}
            Err(error) => {
                result.state = PlateVerificationState::Failed;
                result.message = Some(format!("Cannot await slicer: {error}"));
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
        let reason = if cancel.load(Ordering::Acquire) {
            Some((PlateVerificationState::Cancelled, "Cancelled by user"))
        } else if start.elapsed() >= timeout {
            Some((
                PlateVerificationState::TimedOut,
                "Local slicer exceeded its time limit",
            ))
        } else if output_size(directory) > MAX_OUTPUT_BYTES {
            Some((
                PlateVerificationState::Failed,
                "Local slicer exceeded the 512 MiB per-plate output limit",
            ))
        } else {
            None
        };
        if let Some((state, message)) = reason {
            let _ = child.kill();
            let _ = child.wait();
            result.state = state;
            result.message = Some(message.into());
            break;
        }
        thread::sleep(Duration::from_millis(40));
    }
    result.elapsed_milliseconds = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
    result.stdout = finish_pipe(stdout);
    result.stderr = finish_pipe(stderr);
    if result.state == PlateVerificationState::Failed && result.message.is_none() {
        retain_failed_plate_result(directory, &mut result);
    }
    if result.state == PlateVerificationState::ToolpathsGenerated {
        if output_size(directory) > MAX_OUTPUT_BYTES {
            result.state = PlateVerificationState::Failed;
            result.message = Some("Local slicer exceeded the bounded output limit".into());
        } else {
            inspect_plate_output(directory, &mut result);
        }
    }
    result
}

fn retain_failed_plate_result(directory: &Path, result: &mut PlateVerification) {
    let native = std::fs::File::open(directory.join("result.json"))
        .ok()
        .and_then(|file| {
            let mut bytes = Vec::new();
            file.take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            (bytes.len() <= 4 * 1024 * 1024).then_some(bytes)
        })
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let mut message = native
        .as_ref()
        .and_then(|value| value["error_string"].as_str())
        .filter(|message| !message.trim().is_empty())
        .map(|message| message.chars().take(2048).collect::<String>())
        .unwrap_or_else(|| {
            format!(
                "Local Bambu exited with status {:?}; no native error report was available",
                result.exit_status
            )
        });
    if let Some(selected) = native
        .as_ref()
        .and_then(|value| value["sliced_plates"].as_array())
        .and_then(|plates| {
            plates
                .iter()
                .find(|plate| plate["id"].as_u64() == Some(result.plate as u64))
        })
    {
        if let Some(warning) = selected["warning_message"]
            .as_str()
            .filter(|warning| !warning.trim().is_empty())
        {
            message.push('\n');
            message.extend(warning.chars().take(8192));
        }
        result.native_result = Some(selected.clone());
    }
    result.message = Some(message);
}

fn inspect_plate_output(directory: &Path, result: &mut PlateVerification) {
    let validation = (|| -> Result<(), String> {
        let native_path = directory.join("result.json");
        if std::fs::metadata(&native_path)
            .map_err(|e| format!("Native slice result is missing: {e}"))?
            .len()
            > 4 * 1024 * 1024
        {
            return Err("Native result exceeds 4 MiB".into());
        }
        let native: serde_json::Value =
            serde_json::from_slice(&std::fs::read(native_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if native["return_code"].as_i64() != Some(0) {
            return Err(format!(
                "Native slicing rejected the project: {}",
                native["error_string"]
            ));
        }
        let plates = native["sliced_plates"]
            .as_array()
            .ok_or("Native output has no sliced plates")?;
        let selected = plates
            .iter()
            .find(|p| p["id"].as_u64() == Some(result.plate as u64))
            .ok_or("Requested plate is missing from native slice results")?;
        result.native_result = Some(selected.clone());
        let path = directory.join(format!("plate_{}.gcode", result.plate));
        if std::fs::metadata(&path)
            .map_err(|e| format!("Generated toolpath is missing: {e}"))?
            .len()
            > MAX_OUTPUT_BYTES
        {
            return Err("Generated toolpath exceeds the output limit".into());
        }
        let gcode = std::fs::read(path).map_err(|e| e.to_string())?;
        let header = String::from_utf8_lossy(&gcode[..gcode.len().min(4096)]);
        let version = header
            .lines()
            .find_map(|line| line.strip_prefix("; BambuStudio "))
            .ok_or("Native toolpath lacks a Bambu Studio version header")?
            .trim();
        result.slicer_version = Some(version.into());
        if version != QUALIFIED_VERSION {
            return Err(format!(
                "Bambu Studio {version} is not qualified by this adapter; supported version is {QUALIFIED_VERSION}"
            ));
        }
        if !gcode.windows(3).any(|window| window == b"G1 ") {
            return Err("Native output contains no generated moves".into());
        }
        result.toolpath_sha256 = Some(sha256(&gcode));
        result.toolpaths_generated = true;
        Ok(())
    })();
    if let Err(error) = validation {
        result.state = PlateVerificationState::Failed;
        result.message = Some(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identity(bytes: &[u8]) -> VerificationIdentity {
        VerificationIdentity {
            source_document_id: "01234567-89ab-4cde-8123-456789abcdef".into(),
            project_sha256: sha256(bytes),
            source_model_sha256: "a".repeat(64),
            source_geometry_revision: None,
            print_intent_sha256: "b".repeat(64),
            source_layout_sha256: "c".repeat(64),
            resolved_layout_sha256: "f".repeat(64),
            named_view: None,
            profile_sha256: "d".repeat(64),
        }
    }
    #[test]
    fn model_observation_is_lazy_private_and_remains_stale_after_restore() {
        let service = LocalSlicerService::default();
        service
            .observe_owned_model("no-job", || panic!("no evidence must not serialize"))
            .unwrap();
        let source = "01234567-89ab-4cde-8123-456789abcdef";
        let model = serde_json::json!({"print_intent":{"source_document_id":source,"defaults":{"wall_count":2}}}).to_string();
        let bytes = b"owned model lifecycle".to_vec();
        let captured = VerificationIdentity::from_owned_export(
            &bytes,
            &model,
            &serde_json::json!({}),
            &serde_json::json!({}),
            "a".repeat(64),
            source.into(),
            None,
        )
        .unwrap();
        let started = service
            .start(
                bytes,
                captured.clone(),
                1,
                LocalSlicerOptions {
                    executable: std::env::temp_dir().join("missing-model-observation.exe"),
                    timeout_seconds_per_plate: 1,
                },
                "private-owner".into(),
            )
            .unwrap();
        service
            .observe_owned_model("another-owner", || {
                panic!("another owner must not serialize")
            })
            .unwrap();
        service
            .observe_owned_model("private-owner", || Ok(model.clone()))
            .unwrap();
        assert!(!service.poll(started.job_id, None).unwrap().stale);
        let changed = model.replace("\"wall_count\":2", "\"wall_count\":6");
        assert_ne!(changed, model);
        service
            .observe_owned_model("private-owner", || Ok(changed))
            .unwrap();
        service
            .observe_owned_model("private-owner", || Ok(model))
            .unwrap();
        let report = service.poll(started.job_id, None).unwrap();
        assert!(report.stale);
        assert_eq!(report.identity, captured);
        assert!(service
            .note_owned_stale(started.job_id, source, "another-owner")
            .is_err());
        assert!(service
            .note_owned_stale(started.job_id, "different-document", "private-owner")
            .is_err());
        for _ in 0..3 {
            service
                .observe_owned_model("private-owner", || Err("recompute unavailable".into()))
                .unwrap();
        }
        assert_eq!(
            service
                .poll(started.job_id, None)
                .unwrap()
                .warnings
                .iter()
                .filter(|warning| warning.starts_with("Current owning model could not"))
                .count(),
            1
        );
    }

    #[test]
    fn missing_slicer_staleness_and_request_limits() {
        let service = LocalSlicerService::default();
        let bytes = b"project".to_vec();
        let options = LocalSlicerOptions {
            executable: std::env::temp_dir().join("no-such-bambu-tool.exe"),
            timeout_seconds_per_plate: 1,
        };
        assert!(service
            .start(
                bytes.clone(),
                identity(b"wrong"),
                4,
                options.clone(),
                "test-owner".into()
            )
            .is_err());
        assert!(service
            .start(
                bytes.clone(),
                identity(&bytes),
                0,
                options.clone(),
                "test-owner".into()
            )
            .is_err());
        let started = service
            .start(
                bytes.clone(),
                identity(&bytes),
                4,
                options,
                "test-owner".into(),
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let done = loop {
            let report = service.poll(started.job_id, None).unwrap();
            if report.state == VerificationState::Failed {
                break report;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        };
        assert!(done
            .plates
            .iter()
            .all(|p| p.state == PlateVerificationState::MissingSlicer));
        assert!(service
            .poll_owned(
                started.job_id,
                &identity(&bytes).source_document_id,
                "{}",
                "other-owner",
                true
            )
            .is_err());
        let mut changed = identity(&bytes);
        changed.print_intent_sha256 = "e".repeat(64);
        assert!(service.poll(started.job_id, Some(&changed)).unwrap().stale);
        assert!(
            service
                .poll(started.job_id, Some(&identity(&bytes)))
                .unwrap()
                .stale
        );
    }
    #[test]
    fn a_transient_layout_edit_invalidates_evidence_even_when_saved_model_is_unchanged() {
        let service = LocalSlicerService::default();
        let bytes = b"owned-project".to_vec();
        let layout = serde_json::json!({"rotation":[0.,0.,0.,1.],"translation":[0.,0.,0.]});
        let mut captured = identity(&bytes);
        captured.source_layout_sha256 = sha256(&serde_json::to_vec(&layout).unwrap());
        let mut report = service
            .start(
                bytes,
                captured,
                1,
                LocalSlicerOptions {
                    executable: std::env::temp_dir().join("absent-bambu-layout-fixture.exe"),
                    timeout_seconds_per_plate: 1,
                },
                "layout-owner".into(),
            )
            .unwrap();
        report.check_current_layout(Ok(layout));
        assert!(!report.stale);
        report.check_current_layout(Ok(
            serde_json::json!({"rotation":[0.,1.,0.,0.],"translation":[0.,0.,0.]}),
        ));
        assert!(report.stale);
    }

    #[test]
    fn one_failing_plate_does_not_invent_toolpath_evidence() {
        let directory = OwnedDirectory::create().unwrap();
        std::fs::write(
            directory.0.join("result.json"),
            br#"{"return_code":0,"sliced_plates":[{"id":1,"filaments":[]}]}"#,
        )
        .unwrap();
        std::fs::write(
            directory.0.join("plate_1.gcode"),
            "; BambuStudio 02.08.02.61\nG1 X10 E1\n",
        )
        .unwrap();
        let mut first = PlateVerification::not_run(1);
        first.state = PlateVerificationState::ToolpathsGenerated;
        inspect_plate_output(&directory.0, &mut first);
        assert_eq!(first.state, PlateVerificationState::ToolpathsGenerated);
        assert!(first.toolpath_sha256.is_some());
        let mut second = PlateVerification::not_run(2);
        second.state = PlateVerificationState::ToolpathsGenerated;
        inspect_plate_output(&directory.0, &mut second);
        assert_eq!(second.state, PlateVerificationState::Failed);
        assert!(second.toolpath_sha256.is_none());
    }
    #[test]
    fn failed_native_plate_retains_selected_empty_layer_diagnostic_without_toolpath_claim() {
        let directory = OwnedDirectory::create().unwrap();
        std::fs::write(directory.0.join("result.json"), br#"{"return_code":-100,"error_string":"Failed slicing","sliced_plates":[{"id":1,"warning_message":"other plate"},{"id":2,"warning_message":"Object cannot be printed for empty layer between 0 and 12.2. One-piece auger"}]}"#).unwrap();
        let mut result = PlateVerification::not_run(2);
        result.state = PlateVerificationState::Failed;
        result.exit_status = Some(-100);
        retain_failed_plate_result(&directory.0, &mut result);
        assert_eq!(result.state, PlateVerificationState::Failed);
        assert_eq!(result.native_result.as_ref().unwrap()["id"], 2);
        assert!(result
            .message
            .as_ref()
            .unwrap()
            .contains("empty layer between 0 and 12.2"));
        assert!(!result.message.as_ref().unwrap().contains("other plate"));
        assert!(!result.toolpaths_generated && !result.native_project_read_back);
        assert!(result.toolpath_sha256.is_none());
        std::fs::write(
            directory.0.join("result.json"),
            vec![b' '; 4 * 1024 * 1024 + 1],
        )
        .unwrap();
        let mut oversized = PlateVerification::not_run(2);
        oversized.state = PlateVerificationState::Failed;
        retain_failed_plate_result(&directory.0, &mut oversized);
        assert!(oversized.native_result.is_none());
        assert!(oversized
            .message
            .unwrap()
            .contains("no native error report"));
    }
    #[test]
    fn native_setting_clamps_and_mapping_changes_fail_qualification() {
        let before = BTreeMap::from([
            (
                "profile_mappings".into(),
                serde_json::json!({"printer_model":"Bambu X2D"}),
            ),
            (
                "volume:stable".into(),
                serde_json::json!({"settings":{"wall_loops":"6","sparse_infill_density":"15%"},"instance_count":2,"subtype":"normal_part"}),
            ),
        ]);
        let mut after = before.clone();
        after.get_mut("volume:stable").unwrap()["settings"]["sparse_infill_density"] =
            serde_json::json!("15.0%");
        assert!(mapping_compatibility_issues(&before, &after).is_empty());
        after.get_mut("volume:stable").unwrap()["settings"]["wall_loops"] = serde_json::json!("4");
        assert_eq!(mapping_compatibility_issues(&before, &after).len(), 1);
        after.get_mut("volume:stable").unwrap()["instance_count"] = serde_json::json!(1);
        assert_eq!(mapping_compatibility_issues(&before, &after).len(), 2);
        after.insert(
            "profile_mappings".into(),
            serde_json::json!({"printer_model":"unreviewed"}),
        );
        assert_eq!(mapping_compatibility_issues(&before, &after).len(), 3);
    }

    #[test]
    fn native_layer_width_and_support_changes_cannot_hide_behind_unchanged_typed_settings() {
        let before = BTreeMap::from([(
            "volume:stable".into(),
            serde_json::json!({
                "settings":{"wall_loops":"2","sparse_infill_density":"5%"},
                "native_settings":{"layer_height":"0.28","line_width":["0.50","0.45"],
                    "top_shell_thickness":"1.2","enable_support":"0","enable_prime_tower":"0"},
                "instance_count":2,"subtype":"normal_part"
            }),
        )]);
        let mut after = before.clone();
        after.get_mut("volume:stable").unwrap()["native_settings"]["line_width"] =
            serde_json::json!(["0.5", "0.450"]);
        assert!(mapping_compatibility_issues(&before, &after).is_empty());
        for (field, changed) in [
            ("layer_height", serde_json::json!("0.2")),
            ("line_width", serde_json::json!(["0.5", "0.40"])),
            ("top_shell_thickness", serde_json::json!("1.6")),
            ("enable_support", serde_json::json!("1")),
            ("enable_prime_tower", serde_json::json!("1")),
        ] {
            let mut after = before.clone();
            after.get_mut("volume:stable").unwrap()["native_settings"][field] = changed;
            let issues = mapping_compatibility_issues(&before, &after);
            assert_eq!(issues.len(), 1, "{field}: {issues:?}");
            assert!(issues[0].contains(field));
        }
        assert!(!native_metadata_values_equal(
            &serde_json::json!("50%"),
            &serde_json::json!("50")
        ));
        assert!(!native_metadata_values_equal(
            &serde_json::json!(["0.50"]),
            &serde_json::json!("0.50")
        ));
        let mut after = before.clone();
        after.insert(
            "process_capability_warnings".into(),
            serde_json::json!(["Unsupported native pattern retained for inspection"]),
        );
        assert_eq!(mapping_compatibility_issues(&before, &after).len(), 1);
    }

    #[test]
    fn owning_tab_can_cancel_after_replacing_document_without_leaking_prior_report() {
        let service = LocalSlicerService::default();
        let bytes = b"owned artifact".to_vec();
        let owner = new_verification_owner();
        let other = new_verification_owner();
        let started = service
            .start(
                bytes.clone(),
                identity(&bytes),
                1,
                LocalSlicerOptions {
                    executable: std::env::temp_dir().join("absent-owner-cancel-bambu.exe"),
                    timeout_seconds_per_plate: 1,
                },
                format!("{owner}:same-tab"),
            )
            .unwrap();
        assert!(service
            .poll_owned(
                started.job_id,
                "new-document",
                "{}",
                &format!("{owner}:same-tab"),
                false
            )
            .is_err());
        assert!(service
            .cancel_owned(started.job_id, &format!("{other}:same-tab"))
            .is_err());
        assert!(!service
            .job(started.job_id)
            .unwrap()
            .cancel
            .load(Ordering::Acquire));
        let receipt = service
            .cancel_owned(started.job_id, &format!("{owner}:same-tab"))
            .unwrap();
        assert_eq!(
            serde_json::to_value(receipt).unwrap(),
            serde_json::json!({"job_id":started.job_id,"cancel_requested":true})
        );
        assert_eq!(
            service.poll(started.job_id, None).unwrap().identity,
            identity(&bytes)
        );
    }

    #[test]
    fn writing_another_target_artifact_marks_prior_owned_evidence_stale_without_mutating_it() {
        let service = LocalSlicerService::default();
        let bytes = b"old exported template".to_vec();
        let captured = identity(&bytes);
        let started = service
            .start(
                bytes,
                captured.clone(),
                1,
                LocalSlicerOptions {
                    executable: std::env::temp_dir().join("missing-reexport-bambu.exe"),
                    timeout_seconds_per_plate: 1,
                },
                "artifact-owner".into(),
            )
            .unwrap();
        service
            .note_owned_export(
                "other-owner",
                &captured.source_document_id,
                &sha256(b"new template"),
            )
            .unwrap();
        assert!(!service.poll(started.job_id, None).unwrap().stale);
        service
            .note_owned_export(
                "artifact-owner",
                "another-document",
                &sha256(b"new template"),
            )
            .unwrap();
        assert!(!service.poll(started.job_id, None).unwrap().stale);
        service
            .note_owned_export(
                "artifact-owner",
                &captured.source_document_id,
                &captured.project_sha256,
            )
            .unwrap();
        assert!(!service.poll(started.job_id, None).unwrap().stale);
        service
            .note_owned_export(
                "artifact-owner",
                &captured.source_document_id,
                &sha256(b"new template"),
            )
            .unwrap();
        let stale = service.poll(started.job_id, Some(&captured)).unwrap();
        assert!(stale.stale);
        assert_eq!(stale.identity, captured);
    }

    #[test]
    fn native_recompute_invalidates_evidence_even_when_model_and_layout_match() {
        let mut report = LocalSlicerReport {
            job_id: 1,
            state: VerificationState::Completed,
            identity: identity(b"project"),
            executable: std::env::temp_dir().join("bambu.exe"),
            executable_sha256: None,
            plates: vec![],
            stale: false,
            physical_qualification: "not_run".into(),
            warnings: vec![],
        };
        report.identity.source_geometry_revision = Some(7);
        report.check_current_geometry_revision(7);
        assert!(!report.stale);
        report.check_current_geometry_revision(8);
        assert!(report.stale);
    }

    fn edit_test_project(
        bytes: &[u8],
        edit: impl FnOnce(&mut BTreeMap<String, Vec<u8>>),
    ) -> Vec<u8> {
        use std::io::{Cursor, Write};
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut entries = BTreeMap::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            entries.insert(entry.name().to_string(), data);
        }
        edit(&mut entries);
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            writer.write_all(&data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn shift_test_normal_mesh(bytes: &[u8], recenter: bool) -> Vec<u8> {
        edit_test_project(bytes, |entries| {
            if recenter {
                let source = String::from_utf8(entries["3D/Objects/a.model"].clone()).unwrap();
                let document = roxmltree::Document::parse(&source).unwrap();
                let mut edits: Vec<_> = document
                    .descendants()
                    .filter(|node| node.tag_name().name() == "vertex")
                    .map(|node| {
                        (
                            node.range(),
                            format!(
                                "<vertex x=\"{}\" y=\"{}\" z=\"{}\"/>",
                                node.attribute("x").unwrap().parse::<f64>().unwrap() - 5.,
                                node.attribute("y").unwrap(),
                                node.attribute("z").unwrap()
                            ),
                        )
                    })
                    .collect();
                edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
                let mut changed = source.clone();
                for (range, replacement) in edits {
                    changed.replace_range(range, &replacement);
                }
                entries.insert("3D/Objects/a.model".into(), changed.into_bytes());
            }
            let source = String::from_utf8(entries["3D/3dmodel.model"].clone()).unwrap();
            let document = roxmltree::Document::parse(&source).unwrap();
            let node = document
                .descendants()
                .find(|node| {
                    node.tag_name().name() == "component" && node.attribute("objectid") == Some("7")
                })
                .unwrap();
            let old = node.attribute("transform").unwrap();
            let mut values: Vec<f64> = old
                .split_whitespace()
                .map(|value| value.parse().unwrap())
                .collect();
            values[9] += if recenter { 5. } else { 1. };
            let new = values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            let mut changed = source.clone();
            changed.replace_range(node.range(), &source[node.range()].replace(old, &new));
            entries.insert("3D/3dmodel.model".into(), changed.into_bytes());
        })
    }

    #[test]
    fn native_readback_accepts_recentered_mesh_but_rejects_moved_geometry_and_changed_zones() {
        let (template, meshes, appearances, instances, structure, mut intent, request) =
            crate::bambu_project::tests::fixture();
        intent.modifiers.push(limo_cad_core::PrintModifierDto {
            id: "01234567-89ab-4cde-8123-456789abcdef".into(),
            name: "Managed zone".into(),
            body_id: limo_cad_core::BodyId(1),
            enabled: true,
            local_pose: limo_cad_core::PrintLocalPoseDto {
                translation_mm: [5.; 3],
                ..Default::default()
            },
            primitive: limo_cad_core::PrintModifierPrimitiveDto::Box { size_mm: [2.; 3] },
            settings: limo_cad_core::PrintSettingsDto {
                wall_count: Some(6),
                ..Default::default()
            },
        });
        let written = crate::bambu_project::write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let (baseline, issues) =
            native_geometry_readback(&written.bytes, &written.bytes, 1).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(baseline["volumes"].as_array().unwrap().len(), 6);
        let recentered = shift_test_normal_mesh(&written.bytes, true);
        let (values, issues) = native_geometry_readback(&written.bytes, &recentered, 1).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert!(values["volumes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|value| value["world_geometry_verified"] == true));
        assert!(
            values["volumes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value["written"]["geometry_sha256"]
                    != value["native"]["geometry_sha256"])
        );
        let moved = shift_test_normal_mesh(&written.bytes, false);
        let (_, issues) = native_geometry_readback(&written.bytes, &moved, 1).unwrap();
        assert!(issues.iter().any(|issue| issue.contains("world geometry")));
        let changed = edit_test_project(&written.bytes, |entries| {
            let source =
                String::from_utf8(entries["Metadata/model_settings.config"].clone()).unwrap();
            entries.insert(
                "Metadata/model_settings.config".into(),
                source
                    .replace(
                        "key=\"wall_loops\" value=\"6\"",
                        "key=\"wall_loops\" value=\"8\"",
                    )
                    .into_bytes(),
            );
        });
        let (_, issues) = native_geometry_readback(&written.bytes, &changed, 1).unwrap();
        assert!(issues.iter().any(|issue| issue.contains("Native modifier")));
        let directory = OwnedDirectory::create().unwrap();
        let input = directory.0.join("input.3mf");
        let output = directory.0.join("resliced.3mf");
        std::fs::write(&input, &written.bytes).unwrap();
        std::fs::write(&output, b"not a native ZIP").unwrap();
        let mut plate = PlateVerification::not_run(1);
        plate.state = PlateVerificationState::ToolpathsGenerated;
        plate.toolpaths_generated = true;
        plate.toolpath_sha256 = Some("a".repeat(64));
        apply_native_readback(&mut plate, native_setting_readback(&input, &output, 1));
        assert_eq!(plate.state, PlateVerificationState::Failed);
        assert!(plate.toolpaths_generated);
        assert!(plate.toolpath_sha256.is_some());
        assert!(!plate.native_project_read_back);
        assert!(!plate.compatibility_issues.is_empty());
    }

    #[test]
    fn owned_verification_retains_bounded_unmanaged_height_writer_warnings() {
        let (template, meshes, appearances, instances, structure, intent, request) =
            crate::bambu_project::tests::fixture();
        let native_ranges = b"<objects><object id=\"1\"><range min_z=\"0\" max_z=\"5\"><option opt_key=\"unknown_native_setting\">keep</option></range></object></objects>";
        let template = edit_test_project(&template, |entries| {
            entries.insert(
                "Metadata/layer_config_ranges.xml".into(),
                native_ranges.to_vec(),
            );
        });
        let written = crate::bambu_project::write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let mut package = zip::ZipArchive::new(std::io::Cursor::new(&written.bytes)).unwrap();
        let mut retained = Vec::new();
        package
            .by_name("Metadata/layer_config_ranges.xml")
            .unwrap()
            .read_to_end(&mut retained)
            .unwrap();
        assert_eq!(retained, native_ranges);
        let warning = written
            .report
            .warnings
            .iter()
            .find(|warning| warning.starts_with("Unmanaged native height metadata"))
            .unwrap()
            .clone();
        let model = serde_json::json!({"print_intent":intent}).to_string();
        let captured = VerificationIdentity::from_owned_export(
            &written.bytes,
            &model,
            &serde_json::json!({}),
            &serde_json::json!({}),
            "a".repeat(64),
            written.report.source_document_id.clone(),
            None,
        )
        .unwrap();
        let service = LocalSlicerService::default();
        let mut warnings = written.report.warnings;
        warnings.push(warning.clone());
        warnings.extend(std::iter::repeat_n("é".repeat(3000), 65));
        let started = service
            .start_with_warnings(
                written.bytes,
                captured,
                1,
                LocalSlicerOptions {
                    executable: std::env::temp_dir().join("missing-unmanaged-height-verifier.exe"),
                    timeout_seconds_per_plate: 1,
                },
                "warning-owner".into(),
                warnings,
            )
            .unwrap();
        assert_eq!(
            started
                .warnings
                .iter()
                .filter(|value| **value == warning)
                .count(),
            1
        );
        assert!(started
            .warnings
            .iter()
            .any(|value| value.starts_with("Additional writer limitations")));
        assert!(started
            .warnings
            .iter()
            .all(|value| value.chars().count() <= 2048));
        assert!(started.warnings.len() <= 67);
        assert_eq!(
            service.poll(started.job_id, None).unwrap().warnings,
            started.warnings
        );
        assert_eq!(started.physical_qualification, "not_run");
    }

    #[test]
    fn native_effective_wall_clamp_fails_but_retains_actual_values_and_toolpaths() {
        let (template, meshes, appearances, instances, structure, intent, request) =
            crate::bambu_project::tests::fixture();
        let written = crate::bambu_project::write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let changed = edit_test_project(&written.bytes, |entries| {
            let source =
                String::from_utf8(entries["Metadata/model_settings.config"].clone()).unwrap();
            let changed = source.replace(
                "key=\"wall_loops\" value=\"6\"",
                "key=\"wall_loops\" value=\"2\"",
            );
            assert_ne!(source, changed);
            entries.insert(
                "Metadata/model_settings.config".into(),
                changed.into_bytes(),
            );
        });
        let directory = OwnedDirectory::create().unwrap();
        let input = directory.0.join("written.3mf");
        let output = directory.0.join("native.3mf");
        std::fs::write(&input, &written.bytes).unwrap();
        std::fs::write(&output, &changed).unwrap();
        let mut plate = PlateVerification::not_run(1);
        plate.state = PlateVerificationState::ToolpathsGenerated;
        plate.toolpaths_generated = true;
        plate.toolpath_sha256 = Some("a".repeat(64));
        apply_native_readback(&mut plate, native_setting_readback(&input, &output, 1));
        assert_eq!(plate.state, PlateVerificationState::Failed);
        assert!(plate.toolpaths_generated);
        assert_eq!(plate.toolpath_sha256, Some("a".repeat(64)));
        assert!(plate.native_project_read_back);
        assert!(plate
            .compatibility_issues
            .iter()
            .any(|issue| issue.contains("Native effective setting changed")
                && issue.contains("wall_loops")));
        assert!(!plate.setting_changes.is_empty());
        let key = format!(
            "volume:{}",
            written.report.parts[0].target_uuid.as_ref().unwrap()
        );
        assert_eq!(
            plate.native_effective_settings.as_ref().unwrap()["effective"][&key]["settings"]
                ["wall_loops"],
            "2"
        );
    }

    #[test]
    fn native_height_readback_checks_ranges_profiles_and_complete_plate_groups() {
        use limo_cad_core::*;
        let (template, meshes, appearances, instances, structure, mut intent, request) =
            crate::bambu_project::tests::fixture();
        let template = edit_test_project(&template, |entries| {
            let mut profile: serde_json::Value =
                serde_json::from_slice(&entries["Metadata/project_settings.config"]).unwrap();
            for (key, value) in [
                ("min_layer_height", serde_json::json!(["0.08", "0.08"])),
                ("max_layer_height", serde_json::json!(["0.28", "0.28"])),
                ("enable_support", serde_json::json!("0")),
                ("enable_prime_tower", serde_json::json!("0")),
                ("raft_layers", serde_json::json!("0")),
            ] {
                profile[key] = value;
            }
            entries.insert(
                "Metadata/project_settings.config".into(),
                serde_json::to_vec(&profile).unwrap(),
            );
        });
        let groups: Vec<_> = [100, 200]
            .into_iter()
            .map(|root| PrintHeightGroupDto {
                root_occurrence_id: root,
                members: instances
                    .iter()
                    .filter(|instance| (instance.occurrence_id < 20) == (root == 100))
                    .map(|instance| PrintSourceOccurrenceDto {
                        body_id: instance.body_id,
                        occurrence_id: instance.occurrence_id,
                    })
                    .collect(),
                min_z_mm: 0.,
                max_z_mm: 10.,
            })
            .collect();
        for body in [BodyId(1), BodyId(2)] {
            let binding = PrintHeightBindingDto {
                layout: PrintHeightLayoutDto::Assembly,
                groups: groups.clone(),
                occurrences: instances
                    .iter()
                    .filter(|instance| instance.body_id == body)
                    .map(|instance| PrintHeightOccurrenceDto {
                        body_id: body,
                        occurrence_id: instance.occurrence_id,
                        root_occurrence_id: if instance.occurrence_id < 20 {
                            100
                        } else {
                            200
                        },
                        pose: PrintLocalPoseDto {
                            translation_mm: instance.translation,
                            rotation: instance.rotation,
                        },
                        min_z_mm: 0.,
                        max_z_mm: 10.,
                    })
                    .collect(),
            };
            intent.height_ranges.push(PrintHeightRangeDto {
                id: format!("01234567-89ab-4cde-8123-{:012}", body.0),
                name: "Verified band".into(),
                body_id: body,
                enabled: true,
                coordinate: PrintHeightCoordinateDto::ObjectBottom,
                min_z_mm: 2.,
                max_z_mm: 7.,
                binding: binding.clone(),
                settings: PrintSettingsDto {
                    wall_count: Some(6),
                    ..Default::default()
                },
                speeds: Default::default(),
            });
            intent
                .layer_height_profiles
                .push(PrintLayerHeightProfileDto {
                    id: format!("11234567-89ab-4cde-8123-{:012}", body.0),
                    name: "Verified schedule".into(),
                    body_id: body,
                    enabled: true,
                    binding,
                    points: vec![
                        PrintLayerHeightPointDto {
                            z_mm: 0.,
                            height_mm: 0.2,
                        },
                        PrintLayerHeightPointDto {
                            z_mm: 5.,
                            height_mm: 0.12,
                        },
                        PrintLayerHeightPointDto {
                            z_mm: 10.,
                            height_mm: 0.12,
                        },
                    ],
                });
        }
        let written = crate::bambu_project::write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let (baseline, issues) =
            native_geometry_readback(&written.bytes, &written.bytes, 1).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(baseline["height_reference_verified"], true);
        assert_eq!(
            baseline["heights"]["native"][0]["ranges"][0]["settings"]["wall_count"],
            6
        );
        assert_eq!(
            baseline["heights"]["native"][0]["profile"][1]["height_mm"],
            0.12
        );
        for entry in [
            "Metadata/layer_config_ranges.xml",
            "Metadata/layer_heights_profile.txt",
        ] {
            let changed = edit_test_project(&written.bytes, |entries| {
                let source = String::from_utf8(entries[entry].clone()).unwrap();
                let target = if entry.ends_with(".xml") {
                    source.replace(">6<", ">8<")
                } else {
                    source.replace("0.12", "0.14")
                };
                assert_ne!(source, target);
                entries.insert(entry.into(), target.into_bytes());
            });
            let (readback, issues) = native_geometry_readback(&written.bytes, &changed, 1).unwrap();
            assert!(
                issues
                    .iter()
                    .any(|issue| issue.starts_with("Native height")),
                "{issues:?}"
            );
            assert_eq!(readback["height_reference_verified"], false);
            assert!(readback["heights"]["native"].is_null());
        }
        // The same native object resource can occur on multiple plates. Keep a
        // complete selected instance without retaining another plate's IDs.
        let mut selected = written.report.refresh_reference.clone();
        selected.parts.retain(|part| part.binding.instance_id == 1);
        let evidence = project_height_reference(&mut selected).unwrap();
        selected.validate().unwrap();
        assert_eq!(
            evidence[0]["original_source_bindings"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert_eq!(
            evidence[0]["selected_source_bindings"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        crate::bambu_project::verify_bambu_height_reference(&written.bytes, &selected).unwrap();
        let mut incomplete = written.report.refresh_reference.clone();
        incomplete.height_objects[0]
            .source_bindings
            .retain(|source| source.body_id == BodyId(1));
        assert!(project_height_reference(&mut incomplete)
            .unwrap_err()
            .contains("normal CAD siblings"));
    }

    #[test]
    fn child_timeout_and_cancellation_are_bounded() {
        // Re-exec this test binary's ignored sleeper. No shell, active slicer or external file is used.
        let executable = std::env::current_exe().unwrap();
        for cancel_first in [false, true] {
            let directory = OwnedDirectory::create().unwrap();
            let mut command = Command::new(&executable);
            command
                .args([
                    "--ignored",
                    "--exact",
                    "slicer_verification::tests::verification_child_sleeper",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let cancel = AtomicBool::new(cancel_first);
            let result = run_plate(
                command,
                &directory.0,
                1,
                Duration::from_millis(100),
                &cancel,
            );
            assert_eq!(
                result.state,
                if cancel_first {
                    PlateVerificationState::Cancelled
                } else {
                    PlateVerificationState::TimedOut
                }
            );
            assert!(result.elapsed_milliseconds < 2000);
            assert!(result.toolpath_sha256.is_none());
        }
    }
    #[test]
    #[ignore = "child process fixture used only by the bounded lifecycle test"]
    fn verification_child_sleeper() {
        thread::sleep(Duration::from_secs(30));
    }

    #[test]
    #[ignore = "requires LIMO_BAMBU_VERIFY_PROJECT, LIMO_BAMBU_VERIFY_REPORT and installed Bambu 2.8.2.61"]
    fn installed_bambu_verifies_each_owned_plate() {
        let path = std::env::var_os("LIMO_BAMBU_VERIFY_PROJECT")
            .expect("owned writer-produced synthetic project");
        let sidecar =
            std::env::var_os("LIMO_BAMBU_VERIFY_REPORT").expect("matching writer report JSON");
        let bytes = std::fs::read(path).unwrap();
        let summary: serde_json::Value =
            serde_json::from_slice(&std::fs::read(sidecar).unwrap()).unwrap();
        let report: crate::bambu_project::BambuProjectReport =
            serde_json::from_value(summary["report"].clone()).unwrap();
        assert_eq!(report.output_sha256, sha256(&bytes));
        assert!(report.metadata_readback_verified);
        let model =
            serde_json::json!({"print_intent":{"source_document_id":report.source_document_id}})
                .to_string();
        let identity = VerificationIdentity::from_owned_export(
            &bytes,
            &model,
            &serde_json::json!({"placement":report.placement}),
            &resolved_bambu_layout(&report),
            report.refresh_reference.profile_sha256,
            report.source_document_id,
            None,
        )
        .unwrap();
        let executable = std::env::var_os("LIMO_BAMBU_EXECUTABLE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:/Program Files/Bambu Studio/bambu-studio.exe"));
        let service = LocalSlicerService::default();
        let started = service
            .start_with_warnings(
                bytes,
                identity,
                report.template.plate_count as u32,
                LocalSlicerOptions {
                    executable,
                    timeout_seconds_per_plate: 60,
                },
                "qualification-owner".into(),
                report.warnings,
            )
            .unwrap();
        let deadline =
            Instant::now() + Duration::from_secs(65 * report.template.plate_count as u64);
        let result = loop {
            let result = service.poll(started.job_id, None).unwrap();
            if !matches!(
                result.state,
                VerificationState::Queued | VerificationState::Running
            ) {
                break result;
            }
            if Instant::now() >= deadline {
                service.cancel(started.job_id).unwrap();
                panic!("Installed-tool verification exceeded its outer bound");
            }
            thread::sleep(Duration::from_millis(50));
        };
        if let Some(output) = std::env::var_os("LIMO_BAMBU_VERIFY_EVIDENCE") {
            std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        }
        assert_eq!(
            result.state,
            VerificationState::Completed,
            "{}",
            serde_json::to_string_pretty(&result).unwrap()
        );
        assert!(result.plates.iter().all(|plate| plate.state
            == PlateVerificationState::ToolpathsGenerated
            && plate.exit_status == Some(0)
            && plate.slicer_version.as_deref() == Some(QUALIFIED_VERSION)
            && plate.toolpath_sha256.is_some()
            && plate.native_result.is_some()));
        assert_eq!(result.physical_qualification, "not_run");
    }
}
