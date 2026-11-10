//! Explicit project-backed replay. Baseline data and document ownership remain
//! with the host; this module orders preflight, baseline commit and normal steps.
use crate::evaluation::ExecutionHost;
use crate::{run_steps, EvaluationLimits, RunOptions, RunProgress, Script};
use serde_json::Value;

pub trait ProjectReplayHost {
    /// Cooperative cancellation/deadline check, also used during bounded data
    /// evaluation when no CAD operation runs. It must be cheap and nonblocking.
    fn checkpoint(&mut self) -> Result<(), String> {
        Ok(())
    }
    /// Check catalog membership/availability without mutating the document.
    fn validate_call(&mut self, group: &str, operation: &str) -> Result<(), String>;
    /// Validate and commit the associated package archive transactionally.
    /// Check cancellation, domain semantics and the owner/revision fence; adopt
    /// ancillary data and transfer the playback owner only on success. Return
    /// an error for any unsuccessful or ambiguous commit; later steps won't run.
    fn load_project_baseline(&mut self) -> Result<(), String>;
    /// Execute through the existing interface with the adopted owner. The host
    /// must continue rejecting cancellation and external ownership changes.
    fn call(
        &mut self,
        name: &str,
        arguments: Value,
        progress: RunProgress,
    ) -> Result<Value, String>;
}

#[derive(Debug)]
pub struct ProjectScript {
    script: Script,
}
impl ProjectScript {
    pub fn parse(source: &str) -> Result<Self, String> {
        Ok(Self {
            script: Script::parse_version(source, true)?,
        })
    }
    /// Expose shared metadata/catalog checks. Ordinary run APIs reject this
    /// version, so borrowing the script cannot accidentally skip its prelude.
    pub fn script(&self) -> &Script {
        &self.script
    }
    pub fn run(
        &self,
        host: &mut impl ProjectReplayHost,
        options: RunOptions,
    ) -> Result<Value, String> {
        self.run_inner(host, options, None)
    }
    /// Opt into cumulative allocation/work bounds in the existing interpreter.
    /// Calls `host.checkpoint()` during steps, reference copying, predicates and
    /// exports. Synchronous host/kernel calls remain cooperatively cancellable
    /// only at their boundaries; hosts must limit their own result allocations.
    pub fn run_bounded(
        &self,
        host: &mut impl ProjectReplayHost,
        options: RunOptions,
        limits: EvaluationLimits,
    ) -> Result<Value, String> {
        self.run_inner(host, options, Some(limits))
    }
    fn run_inner(
        &self,
        host: &mut impl ProjectReplayHost,
        options: RunOptions,
        limits: Option<EvaluationLimits>,
    ) -> Result<Value, String> {
        if let Some(limits) = limits {
            host.checkpoint()?;
            if !(1..=128).contains(&limits.max_depth) {
                return Err("Evaluation depth limit must be in 1..=128".into());
            }
            if !self.script.document["verification"].is_null() {
                return Err(
                    "Bounded evaluation does not support the manufacturing verification gate"
                        .into(),
                );
            }
        }
        self.script.validate_options(options)?;
        self.script.validate_calls(|group, operation| {
            if limits.is_some() {
                host.checkpoint()?;
            }
            host.validate_call(group, operation)
        })?;
        if limits.is_some() {
            host.checkpoint()?;
        }
        host.load_project_baseline()?;
        let mut report = run_steps(&self.script, &mut ReplayExecution(host), options, limits)?;
        if limits.is_some() {
            host.checkpoint()?;
        }
        report["project_baseline_loaded"] = Value::Bool(true);
        Ok(report)
    }
}

struct ReplayExecution<'a, H>(&'a mut H);
impl<H: ProjectReplayHost> ExecutionHost for ReplayExecution<'_, H> {
    fn checkpoint(&mut self) -> Result<(), String> {
        self.0.checkpoint()
    }
    fn call(&mut self, name: &str, args: Value, progress: RunProgress) -> Result<Value, String> {
        self.0.call(name, args, progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[derive(Default)]
    struct Host {
        events: Vec<String>,
        reject_catalog: bool,
        reject_baseline: bool,
        reject_call: bool,
    }
    impl ProjectReplayHost for Host {
        fn validate_call(&mut self, _: &str, op: &str) -> Result<(), String> {
            self.events.push(format!("validate:{op}"));
            if self.reject_catalog {
                Err("unavailable".into())
            } else {
                Ok(())
            }
        }
        fn load_project_baseline(&mut self) -> Result<(), String> {
            self.events.push("baseline".into());
            if self.reject_baseline {
                Err("baseline rejected".into())
            } else {
                Ok(())
            }
        }
        fn call(&mut self, _: &str, args: Value, _: RunProgress) -> Result<Value, String> {
            self.events
                .push(format!("call:{}", args["operation"].as_str().unwrap()));
            if self.reject_call {
                Err("owner changed".into())
            } else {
                Ok(json!({}))
            }
        }
    }
    fn source() -> String {
        json!({"version":2,"name":"Project replay","starting_state":"project","steps":[
            {"call":{"group":"document/files","operation":"cad_set_document_name","arguments":{"name":"A"}}},
            {"call":{"group":"document/files","operation":"cad_set_document_name","arguments":{"name":"B"}}}
        ]}).to_string()
    }
    #[test]
    fn legacy_entry_points_reject_project_script_without_calling_host() {
        assert!(Script::parse(&source()).is_err());
        let script = ProjectScript::parse(&source()).unwrap();
        let mut called = false;
        assert!(crate::run(
            script.script(),
            |_, _| {
                called = true;
                Ok(json!({}))
            },
            RunOptions::default()
        )
        .is_err());
        assert!(!called);
    }
    #[test]
    fn preflight_precedes_single_baseline_and_ordered_steps() {
        let script = ProjectScript::parse(&source()).unwrap();
        let mut host = Host::default();
        let report = script.run(&mut host, RunOptions::default()).unwrap();
        assert_eq!(
            host.events,
            [
                "validate:cad_set_document_name",
                "validate:cad_set_document_name",
                "baseline",
                "call:cad_set_document_name",
                "call:cad_set_document_name"
            ]
        );
        assert_eq!(report["project_baseline_loaded"], true);
        assert_eq!(report["steps_completed"], 2);
    }
    #[test]
    fn catalog_baseline_or_step_failure_stops_at_its_boundary() {
        let script = ProjectScript::parse(&source()).unwrap();
        for case in 0..3 {
            let mut host = Host {
                reject_catalog: case == 0,
                reject_baseline: case == 1,
                reject_call: case == 2,
                ..Host::default()
            };
            assert!(script.run(&mut host, RunOptions::default()).is_err());
            assert_eq!(host.events.len(), [1, 3, 4][case]);
        }
    }
    #[test]
    fn invalid_run_options_reject_before_catalog_or_baseline() {
        let mut value: Value = serde_json::from_str(&source()).unwrap();
        value["verification"] = json!("garden-bench");
        let script = ProjectScript::parse(&value.to_string()).unwrap();
        let mut host = Host::default();
        assert!(script
            .run(
                &mut host,
                RunOptions {
                    presentation: false,
                    validate: false
                }
            )
            .is_err());
        assert!(host.events.is_empty());
    }

    #[test]
    fn baseline_only_is_valid_but_missing_starting_state_and_wrong_versions_reject() {
        let value =
            json!({"version":2,"name":"Baseline only","starting_state":"project","steps":[]});
        let script = ProjectScript::parse(&value.to_string()).unwrap();
        let mut host = Host::default();
        let report = script.run(&mut host, RunOptions::default()).unwrap();
        assert_eq!(host.events, ["baseline"]);
        assert_eq!(report["steps_completed"], 0);
        for bad in [
            json!({"version":2,"name":"Missing","steps":[]}),
            json!({"version":1,"name":"Old","starting_state":"project","steps":[]}),
            json!({"version":2,"name":"Empty","starting_state":"empty","steps":[]}),
        ] {
            assert!(ProjectScript::parse(&bad.to_string()).is_err());
        }
    }
}
