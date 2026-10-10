//! In-memory sketch sessions retained when a host releases derived geometry.
use super::{FinishedSketch, SessionError, SketchManager};
use std::collections::BTreeMap;

/// Move-only editing state, including Undo/Redo and entity identity high-water
/// marks. It owns no solid scene or native kernel and is not part of project files.
#[derive(Debug)]
pub struct RetainedSketchSessions {
    finished: Vec<FinishedSketch>,
    grid_step: f64,
}

impl SketchManager {
    /// Detach finished sessions immediately before dropping this manager. The
    /// host must first save its parametric model and fence document mutations.
    pub fn take_sketch_session_retention(
        &mut self,
    ) -> Result<RetainedSketchSessions, SessionError> {
        self.require_retention_boundary()?;
        Ok(RetainedSketchSessions {
            finished: std::mem::take(&mut self.finished),
            grid_step: self.grid_step,
        })
    }

    /// Restore only onto the successfully rebuilt, matching sketches. All
    /// checks finish before ownership moves, so a failed replay can be retried
    /// without losing any commands from the retained sessions.
    pub fn restore_sketch_session_retention(
        &mut self,
        retained: &mut RetainedSketchSessions,
    ) -> Result<(), SessionError> {
        self.require_retention_boundary()?;
        let rebuilt: BTreeMap<_, _> = self
            .finished
            .iter()
            .map(|sketch| (sketch.feature_id, sketch))
            .collect();
        if rebuilt.len() != retained.finished.len() {
            return Err(retention_mismatch());
        }
        for sketch in &retained.finished {
            let Some(rebuilt) = rebuilt.get(&sketch.feature_id) else {
                return Err(retention_mismatch());
            };
            let state = |sketch: &FinishedSketch| {
                serde_json::to_value(sketch.session.project_state(sketch.feature_id)).map_err(
                    |error| SessionError::Solid(format!("Invalid retained sketch: {error}")),
                )
            };
            if state(sketch)? != state(rebuilt)? {
                return Err(retention_mismatch());
            }
        }
        self.finished = std::mem::take(&mut retained.finished);
        self.grid_step = retained.grid_step;
        Ok(())
    }

    fn require_retention_boundary(&self) -> Result<(), SessionError> {
        if self.active.is_some() || self.pending_project.is_some() {
            return Err(SessionError::Solid(
                "Cannot retain sketch sessions during editing or project replacement".into(),
            ));
        }
        Ok(())
    }
}

fn retention_mismatch() -> SessionError {
    SessionError::Solid(
        "Rebuilt sketches do not match their retained editing sessions; the snapshot is retained"
            .into(),
    )
}
