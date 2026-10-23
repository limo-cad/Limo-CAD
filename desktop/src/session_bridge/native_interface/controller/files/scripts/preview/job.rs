//! Retained preview inputs and cooperative preparation lifetime.
use super::*;
use crate::app_preferences::{locale::translate, Locale};

pub(super) enum Source {
    Example(&'static catalog::Example),
    Lesson {
        source: String,
        lesson: Option<lesson::Id>,
    },
    Project(Arc<Loaded>),
}
impl Source {
    pub fn is_project(&self) -> bool {
        matches!(self, Self::Project(_))
    }
    pub fn prepare(
        self,
        locale: Locale,
        cache: &Mutex<prepared::Cache>,
        mut check: impl FnMut() -> Result<(), String>,
    ) -> Result<Vec<Frame>, String> {
        check()?;
        let frames = match self {
            Self::Example(example) => prepared::prepare(cache, &example.source, None)?,
            Self::Lesson { source, lesson } => prepared::prepare(cache, &source, lesson)?,
            Self::Project(loaded) => {
                let Artifact::Project { package, .. } = &loaded.artifact else {
                    return Err("Project preview baseline is unavailable".into());
                };
                let prepared =
                    limo_cad_mcp::prepare_project_preview_source(package, loaded.authored())?;
                check()?;
                let result = limo_cad_mcp::run_prepared_project_preview(prepared, &mut check)?;
                match result.coverage {
                    limo_cad_mcp::ProjectPreviewCoverage::SolidGeometryAndAssembly => {}
                }
                result
                    .frames
                    .into_iter()
                    .map(|frame| Frame {
                        caption: translate(
                            locale,
                            match frame.stage {
                                limo_cad_mcp::ProjectPreviewStage::Baseline => {
                                    "projectReplay.previewBaseline"
                                }
                                limo_cad_mcp::ProjectPreviewStage::Final => {
                                    "projectReplay.previewFinal"
                                }
                            },
                        )
                        .to_owned(),
                        scene: frame.scene,
                        assembly: Some(frame.assembly),
                        ..Default::default()
                    })
                    .collect()
            }
        };
        check()?;
        Ok(frames)
    }
}

pub(super) fn source(state: &super::super::State) -> Result<Source, String> {
    if state.library.pending().is_some() {
        return Err("Finish opening the selected script before previewing".into());
    }
    if state
        .loaded
        .as_ref()
        .is_some_and(|loaded| loaded.artifact.is_project())
    {
        return state.selected(state.generation).map(Source::Project);
    }
    super::eligible(state).map(Source::Example)
}

pub(super) struct Preparation {
    pub cancelled: Arc<AtomicBool>,
    pub deadline: Instant,
}
impl Preparation {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + Duration::from_secs(30),
        }
    }
}
impl Drop for Preparation {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
pub(super) fn check(cancelled: &AtomicBool, deadline: Instant) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        return Err("Preview preparation cancelled".into());
    }
    if Instant::now() >= deadline {
        return Err("Preview preparation timed out".into());
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests;
