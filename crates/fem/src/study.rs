//! Owned inputs and race-safe result adoption for a host's thermal study.
//! This is an in-memory domain model, not project persistence or a UI controller.
use crate::{solve_steady_with_surfaces, SolveOptions, ThermalMesh, ThermalResult, ThermalSurface};
use limo_cad_core::{MaterialDetails, MaterialPropertyFilter};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// A retained catalog record and explicit selection, shared with other domains.
#[derive(Debug, Clone, PartialEq)]
pub struct ThermalMaterial {
    pub details: MaterialDetails,
    pub context: Option<String>,
    pub source_id: Option<String>,
}

/// Every numerical input is owned. Hosts resolve CAD references and units before
/// constructing this snapshot; no pointers into a mutable document are retained.
#[derive(Debug, Clone, PartialEq)]
pub struct ThermalStudyInput {
    pub mesh: ThermalMesh,
    pub materials: Vec<ThermalMaterial>,
    pub prescribed_k: Vec<(usize, f64)>,
    pub nodal_load_w: Vec<f64>,
    pub surfaces: Vec<ThermalSurface>,
    pub options: SolveOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThermalStudyStatus {
    Idle,
    Running,
    Cancelled,
    Solved,
    Failed(String),
}

/// A successful result keeps its exact inputs and material provenance, even when
/// later edits make it stale. It cannot be forged through the public API.
#[derive(Debug)]
pub struct ThermalStudyResult {
    input: Arc<ThermalStudyInput>,
    result: ThermalResult,
}
impl ThermalStudyResult {
    pub fn input(&self) -> &ThermalStudyInput {
        &self.input
    }
    pub fn result(&self) -> &ThermalResult {
        &self.result
    }
}

/// The host owns one study per document/study identity. Do not clone ownership
/// across documents. Dropping or replacing it cancels the outstanding attempt.
#[derive(Debug)]
pub struct ThermalStudy {
    input: Arc<ThermalStudyInput>,
    pending: Option<Arc<AtomicBool>>,
    status: ThermalStudyStatus,
    result: Option<ThermalStudyResult>,
}
impl ThermalStudy {
    /// Draft inputs may be incomplete or invalid. The solver validates them;
    /// draft edits never manufacture a successful result.
    pub fn new(input: ThermalStudyInput) -> Self {
        Self {
            input: Arc::new(input),
            pending: None,
            status: ThermalStudyStatus::Idle,
            result: None,
        }
    }
    pub fn input(&self) -> &ThermalStudyInput {
        &self.input
    }
    pub fn status(&self) -> &ThermalStudyStatus {
        &self.status
    }
    pub fn result(&self) -> Option<&ThermalStudyResult> {
        self.result.as_ref()
    }
    /// Exact numerical-input equality only. The host must separately resolve
    /// current CAD references; this does not certify a stale CAD mesh as current.
    pub fn result_is_current(&self) -> bool {
        self.result.as_ref().is_some_and(|r| r.input == self.input)
    }
    /// Atomically replace the draft. A no-op preserves an active attempt.
    /// A real edit cancels it but retains the previous successful result.
    pub fn replace_input(&mut self, input: ThermalStudyInput) -> bool {
        if *self.input == input {
            return false;
        }
        self.cancel();
        self.input = Arc::new(input);
        self.status = ThermalStudyStatus::Idle;
        true
    }
    /// A new attempt supersedes the previous one, even with identical inputs.
    /// The returned job may run on a worker without locking the live document.
    pub fn begin_solve(&mut self) -> ThermalSolveJob {
        self.cancel();
        let token = Arc::new(AtomicBool::new(false));
        self.pending = Some(token.clone());
        self.status = ThermalStudyStatus::Running;
        ThermalSolveJob {
            input: self.input.clone(),
            token,
        }
    }
    pub fn cancel(&mut self) -> bool {
        if let Some(token) = self.pending.take() {
            token.store(true, Ordering::Relaxed);
            self.status = ThermalStudyStatus::Cancelled;
            true
        } else {
            false
        }
    }
    /// Accept only the currently owned attempt. Foreign, cancelled, replaced or
    /// late completions have no effect on status or retained results.
    pub fn finish(&mut self, completion: ThermalSolveCompletion) -> bool {
        if !self
            .pending
            .as_ref()
            .is_some_and(|p| Arc::ptr_eq(p, &completion.token))
        {
            return false;
        }
        self.pending = None;
        match completion.outcome {
            Ok(result) => {
                self.result = Some(ThermalStudyResult {
                    input: completion.input,
                    result,
                });
                self.status = ThermalStudyStatus::Solved;
            }
            Err(error) => self.status = ThermalStudyStatus::Failed(error),
        }
        true
    }
}
impl Drop for ThermalStudy {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Owned, single-use worker request. There is no public constructor or clone:
/// only the originating study can adopt its completion.
#[derive(Debug)]
pub struct ThermalSolveJob {
    input: Arc<ThermalStudyInput>,
    token: Arc<AtomicBool>,
}
impl ThermalSolveJob {
    pub fn is_cancelled(&self) -> bool {
        self.token.load(Ordering::Relaxed)
    }
    pub fn run(self) -> ThermalSolveCompletion {
        let outcome = self.solve();
        ThermalSolveCompletion {
            input: self.input,
            token: self.token,
            outcome,
        }
    }
    fn solve(&self) -> Result<ThermalResult, String> {
        let mut materials = Vec::with_capacity(self.input.materials.len());
        for material in &self.input.materials {
            if self.is_cancelled() {
                return Err("Thermal solve cancelled".into());
            }
            materials.push(
                material
                    .details
                    .isotropic_conduction(MaterialPropertyFilter {
                        context: material.context.as_deref(),
                        source_id: material.source_id.as_deref(),
                    })?,
            );
        }
        solve_steady_with_surfaces(
            &self.input.mesh,
            &materials,
            &self.input.prescribed_k,
            &self.input.nodal_load_w,
            &self.input.surfaces,
            self.input.options,
            || self.is_cancelled(),
        )
    }
}

#[derive(Debug)]
pub struct ThermalSolveCompletion {
    input: Arc<ThermalStudyInput>,
    token: Arc<AtomicBool>,
    outcome: Result<ThermalResult, String>,
}
