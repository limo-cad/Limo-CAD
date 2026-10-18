//! Retained authoring baseline shared by editor generations and file jobs.
use super::*;
use limo_cad_project_file::{ReplayPackage, ReplayPackageIdentity};

pub(super) const PACKAGE_SUFFIX: &str = ".limo.replay.cbor";
#[derive(Clone, Default)]
pub(super) enum Artifact {
    #[default]
    Text,
    Project {
        package: Arc<ReplayPackage>,
        opened: ReplayPackageIdentity,
        saved: ReplayPackageIdentity,
    },
}
impl Artifact {
    pub fn is_project(&self) -> bool {
        matches!(self, Self::Project { .. })
    }
    pub fn same_baseline(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text, Self::Text) => true,
            (
                Self::Project {
                    package: a,
                    opened: ai,
                    ..
                },
                Self::Project {
                    package: b,
                    opened: bi,
                    ..
                },
            ) => Arc::ptr_eq(a, b) && ai == bi,
            _ => false,
        }
    }
    pub fn inspect(&self, path: Option<PathBuf>, source: &str) -> Result<Loaded, String> {
        let inspection = match self {
            Self::Text => {
                let mut arguments = json!({"source":source});
                if let Some(path) = &path {
                    let base = path
                        .parent()
                        .and_then(|p| p.to_str())
                        .ok_or("Script include directory must be valid Unicode")?;
                    arguments["include_base"] = json!(base);
                }
                limo_cad_mcp::inspect_script(arguments)?
            }
            Self::Project { .. } => {
                limo_cad_mcp::inspect_project_replay_source(source, false, true)?
            }
        };
        let mut inspection = inspection;
        if let Self::Project { opened, saved, .. } = self {
            inspection["opened_package_identity"] = json!(opened);
            inspection["saved_package_identity"] = json!(saved);
        }
        let mut loaded = inspected(path, inspection)?;
        loaded.artifact = self.clone();
        Ok(loaded)
    }
    pub fn save(
        &self,
        path: &std::path::Path,
        source: &str,
        maximum: usize,
    ) -> Result<Self, String> {
        if source.len() > maximum {
            return Err(format!("Script source exceeds {maximum} bytes"));
        }
        match self {
            Self::Text => {
                editor::write_source(path, source, maximum)?;
                Ok(Self::Text)
            }
            Self::Project {
                package, opened, ..
            } => {
                if !path.is_absolute()
                    || !path.to_str().is_some_and(|p| p.ends_with(PACKAGE_SUFFIX))
                {
                    return Err(
                        "Save a project replay to an absolute .limo.replay.cbor path".into(),
                    );
                }
                let saved = package
                    .write_source_atomic(path, source)
                    .map_err(|e| e.to_string())?;
                Ok(Self::Project {
                    package: Arc::clone(package),
                    opened: opened.clone(),
                    saved,
                })
            }
        }
    }
    pub fn prepare(
        &self,
        source: &str,
        presentation: bool,
    ) -> Result<Option<limo_cad_mcp::PreparedProjectReplay>, String> {
        match self {
            Self::Text => Ok(None),
            Self::Project { package, .. } => {
                limo_cad_mcp::prepare_project_replay_source(package, source, presentation, true)
                    .map(Some)
            }
        }
    }
}
pub(super) fn inspect_package(path: PathBuf) -> Result<Loaded, String> {
    if !path.is_absolute() {
        return Err("Open a project replay from an absolute path".into());
    }
    let (package, identity) =
        ReplayPackage::read_file_with_identity(&path).map_err(|e| e.to_string())?;
    let package = Arc::new(package);
    let artifact = Artifact::Project {
        package: Arc::clone(&package),
        opened: identity.clone(),
        saved: identity,
    };
    artifact.inspect(Some(path), package.script())
}

pub(super) fn message(locale: crate::app_preferences::Locale, text: &str) -> String {
    let key = match text {
        "Limo CAD project replay (.limo.replay.cbor)" => "projectReplay.filter",
        "Save a project replay to an absolute .limo.replay.cbor path" => "projectReplay.savePath",
        "Open a project replay from an absolute path" => "projectReplay.openPath",
        "Project replay saved with its retained baseline. Validate before Run." => "projectReplay.saved",
        "Script changed before its prepared run started" => "projectReplay.changed",
        "Prepared script handoff is unavailable" => "projectReplay.handoff",
        "Script preparation handoff retired" => "projectReplay.retired",
        "Replay source is valid. Run uses this source and the retained project baseline; validation does not save edits." => "projectReplay.validSource",
        _ => return text.to_owned(),
    };
    crate::app_preferences::locale::translate(locale, key).to_owned()
}
