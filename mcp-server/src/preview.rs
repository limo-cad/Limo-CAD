//! Preflight for isolated teaching previews. The general product catalog also
//! contains session, process and file operations; catalog validity alone does
//! not establish preview isolation. New operations are unavailable here until
//! their detached execution and resource costs have been reviewed.
use limo_cad_project_file::{ProjectArchive, ReplayPackage};
use limo_cad_script::{ProjectScript, RunOptions, Script};

mod frames;
mod host;
pub use frames::{
    ProjectPreviewCoverage, ProjectPreviewFrame, ProjectPreviewResult, ProjectPreviewStage,
};
pub use host::run_prepared_project_preview;

const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: usize = 16 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 32 * 1024 * 1024;

/// Admitted preview input, distinct from a live replay/publication request.
/// No kernel has been created and domain semantics have not been staged.
/// The detached host must retain cancellation and validate the paired snapshot
/// transaction before running steps. Admission does not bound kernel runtime.
pub struct PreparedProjectPreview {
    script: ProjectScript,
    archive: ProjectArchive,
}
impl PreparedProjectPreview {
    pub fn into_parts(self) -> (ProjectScript, ProjectArchive, RunOptions) {
        (
            self.script,
            self.archive,
            RunOptions {
                presentation: false,
                validate: true,
            },
        )
    }
}

/// Prepare retained package source without file reads or live document access.
/// Callers serialize jobs and retain their editor package across success/failure.
/// Source/operation/archive admission precedes the archive copy. The shared ZIP
/// decoder checks expanded entry sizes before reading model/manifest content.
/// No canonical envelope or publication identity is needed for a private preview.
pub fn prepare_project_preview_source(
    package: &ReplayPackage,
    source: &str,
) -> Result<PreparedProjectPreview, String> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err("Preview scripts must be no larger than 2 MiB".into());
    }
    if package.project_bytes().len() > MAX_ARCHIVE_BYTES {
        return Err("Preview project archives must be no larger than 16 MiB".into());
    }
    let script = ProjectScript::parse(source)?;
    script.script().validate_options(RunOptions {
        presentation: false,
        validate: true,
    })?;
    validate(script.script())?;
    let archive = ProjectArchive::decode_with_limits(
        package.project_bytes().to_vec(),
        MAX_ARCHIVE_BYTES as u64,
        MAX_EXPANDED_BYTES,
    )
    .map_err(|error| error.to_string())?;
    archive.snapshot().map_err(|error| error.to_string())?;
    Ok(PreparedProjectPreview { script, archive })
}

pub(super) fn validate(script: &Script) -> Result<(), String> {
    let metadata = script.metadata();
    let steps = metadata["step_count"].as_u64().unwrap_or(0)
        + metadata["check_count"].as_u64().unwrap_or(0);
    if steps > 80 {
        return Err(
            "Preview supports at most 80 steps and checks; run this script in a new design instead"
                .into(),
        );
    }
    // Walk every call, including late checks, before any engine construction or
    // command execution. Do not infer permission from group or name prefixes.
    script.validate_calls(|group, operation| {
        crate::interface::validate_script_call(group, operation)?;
        if !supported(operation) {
            return Err(format!("Operation {operation} is not available in isolated preview; run the script explicitly in a new design"));
        }
        Ok(())
    })
}

fn supported(operation: &str) -> bool {
    // These commands operate on the detached manager/kernel used by the two
    // existing preview lessons. None can attach a session, route a request,
    // start a process or select a file destination.
    matches!(
        operation,
        "assembly_create_component"
            | "assembly_create_occurrence"
            | "assembly_set_occurrence_grounded"
            | "assembly_solution"
            | "cad_project_model"
            | "sketch_add_constraint"
            | "sketch_add_rectangle_locked"
            | "sketch_begin"
            | "sketch_edit"
            | "sketch_edit_dimension"
            | "sketch_finish"
            | "sketch_finished"
            | "sketch_undo"
            | "solid_edit_extrude"
            | "solid_extrude"
            | "solid_fillet"
            | "solid_recompute"
            | "solid_scene"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project_source(operation: Option<&str>) -> String {
        let steps = operation.map_or_else(Vec::new, |operation| vec![json!({
            "call":{"group":crate::interface::group_for(operation).unwrap(),"operation":operation,"arguments":{}}
        })]);
        json!({"version":2,"name":"Preview baseline","starting_state":"project","steps":steps})
            .to_string()
    }

    #[test]
    fn package_preview_admission_precedes_archive_decode() {
        let package = ReplayPackage::new("original".into(), vec![255]).unwrap();
        let operation = "solid_export_step";
        let error = prepare_project_preview_source(&package, &project_source(Some(operation)))
            .err()
            .unwrap();
        assert!(
            error.contains(operation) && error.contains("isolated preview"),
            "{operation} must be rejected before archive decoding"
        );
        assert!(
            prepare_project_preview_source(&package, &" ".repeat(MAX_SOURCE_BYTES + 1))
                .err()
                .unwrap()
                .contains("2 MiB")
        );
        let large = ReplayPackage::new("original".into(), vec![0; MAX_ARCHIVE_BYTES + 1]).unwrap();
        assert!(
            prepare_project_preview_source(&large, &project_source(None))
                .err()
                .unwrap()
                .contains("16 MiB")
        );
        assert_eq!(package.project_bytes(), &[255]);
    }

    #[test]
    fn prepared_preview_preserves_baseline_and_uses_fast_checked_steps() {
        let mut archive = ProjectArchive::new(
            r#"{"format":"limo-cad-project","schema_version":15}"#.into(),
            limo_cad_project_file::SaveMetadata {
                application_version: "test",
                saved_at: "2026-10-17T00:00:00Z",
            },
        )
        .unwrap();
        archive
            .set_intent("intent/future.cbor", Some(vec![0, 255, 7]))
            .unwrap();
        let bytes = archive.encode().unwrap();
        let package = ReplayPackage::new("retained authorship".into(), bytes.clone()).unwrap();
        let prepared = prepare_project_preview_source(&package, &project_source(None)).unwrap();
        let (script, retained, options) = prepared.into_parts();
        assert_eq!(script.script().metadata()["step_count"], 0);
        assert!(!options.presentation);
        assert!(options.validate);
        assert_eq!(retained.encode().unwrap(), bytes);
        assert_eq!(
            retained.snapshot().unwrap().intents()[0].payload,
            [0, 255, 7]
        );
        assert_eq!(package.script(), "retained authorship");
        assert_eq!(package.project_bytes(), bytes);
    }

    #[test]
    fn existing_preview_recipes_pass_the_explicit_operation_policy() {
        let mut count = 0;
        for recipe in limo_cad_recipes::RECIPES
            .iter()
            .filter(|recipe| recipe.preview)
        {
            validate(&Script::parse(recipe.source).unwrap())
                .unwrap_or_else(|error| panic!("{}: {error}", recipe.id));
            count += 1;
        }
        assert!(count > 0);
    }

    #[test]
    fn external_effect_operations_fail_before_preview_engine_construction() {
        for operation in [
            "cad_attach",
            "cad_route",
            "bambu_local_verification_start",
            "print_calibrate",
            "solid_export_step",
        ] {
            let group = crate::interface::group_for(operation).unwrap();
            crate::interface::validate_script_call(group, operation).unwrap();
            let rejection = if operation == "cad_attach" {
                "session ownership and transport belong to the host"
            } else {
                "isolated preview"
            };
            for late in [false, true] {
                let call = json!({"id":"forbidden","call":{"group":group,"operation":operation,"arguments":{}}});
                let mut document = json!({"version":1,"name":"Isolated preview policy","steps":[{"note":"No commands should execute"}]});
                if late {
                    document["checks"] = json!([call]);
                } else {
                    document["steps"].as_array_mut().unwrap().push(call);
                }
                let source = document.to_string();
                // Catalog membership is checked above. The parser owns session
                // rejection; preview admission rejects the other external effects.
                let error = crate::preview_script(&source)
                    .err()
                    .expect("Forbidden operation unexpectedly passed preview admission");
                assert!(
                    error.contains(operation) && error.contains(rejection),
                    "{operation} must be rejected before preview execution (late={late})"
                );
            }
        }
    }

    #[test]
    fn preview_limits_and_catalog_identity_remain_required() {
        let source =
            json!({"version":1,"name":"Too many steps","steps":vec![json!({"note":"Step"});81]})
                .to_string();
        assert!(crate::preview_script(&source)
            .unwrap_err()
            .contains("80 steps"));
        assert!(crate::preview_script(&" ".repeat(2 * 1024 * 1024 + 1))
            .unwrap_err()
            .contains("2 MiB"));
        let source = json!({"version":1,"name":"Wrong group","steps":[{"call":{"group":"wrong/group","operation":"solid_scene","arguments":{}}}]}).to_string();
        assert!(crate::preview_script(&source)
            .unwrap_err()
            .contains("belongs to"));
        assert!(!supported("future_external_operation"));
    }
}
