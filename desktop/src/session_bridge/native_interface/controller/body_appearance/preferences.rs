//! Desktop persistence for the existing shared 3MF slicer target.
//! This is an application preference, never part of body or CAM intent.
use limo_cad_export::SlicerTarget;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static PUBLICATION: AtomicU64 = AtomicU64::new(0);
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Default)]
pub(super) struct Observer {
    path: Option<PathBuf>,
    checked: Option<Instant>,
    publication: u64,
}

impl Observer {
    fn path(&mut self) -> Result<&Path, String> {
        if self.path.is_none() {
            self.path = Some(path()?);
        }
        Ok(self.path.as_deref().unwrap())
    }

    pub(super) fn poll(
        &mut self,
        now: Instant,
        force: bool,
    ) -> Option<Result<SlicerTarget, String>> {
        let publication = PUBLICATION.load(Ordering::Acquire);
        if !force
            && publication == self.publication
            && self
                .checked
                .is_some_and(|checked| now.saturating_duration_since(checked) < REFRESH_INTERVAL)
        {
            return None;
        }
        self.checked = Some(now);
        self.publication = publication;
        Some(self.path().and_then(read_at))
    }

    pub(super) fn write(&mut self, target: SlicerTarget) -> Result<(), String> {
        write_at(self.path()?, target)
    }

    #[cfg(test)]
    pub(super) fn at(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ..Self::default()
        }
    }
}

pub(crate) fn path() -> Result<std::path::PathBuf, String> {
    Ok(crate::app_config::native_directory()?.join("slicer-target.json"))
}

pub(crate) fn read() -> Result<SlicerTarget, String> {
    read_at(&path()?)
}

fn read_at(path: &Path) -> Result<SlicerTarget, String> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SlicerTarget::default());
        }
        Err(error) => return Err(format!("Cannot read the saved slicer target: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read the saved slicer target: {error}"))?;
    if bytes.len() > 4096 {
        return Err("The saved slicer target is too large".into());
    }
    serde_json::from_slice(&bytes)
        .map(canonical_ui_target)
        .map_err(|error| format!("Cannot read the saved slicer target: {error}"))
}

fn write_at(path: &Path, target: SlicerTarget) -> Result<(), String> {
    let bytes =
        serde_json::to_vec(&canonical_ui_target(target)).map_err(|error| error.to_string())?;
    let parent = path
        .parent()
        .ok_or("The slicer preference has no parent directory")?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Cannot create application preferences: {error}"))?;
    limo_cad_project_file::write_binary_file_atomic(path, &bytes)
        .map_err(|error| format!("Cannot save the slicer target: {error}"))?;
    PUBLICATION.fetch_add(1, Ordering::Release);
    Ok(())
}

fn canonical_ui_target(target: SlicerTarget) -> SlicerTarget {
    match target {
        SlicerTarget::BambuStudio | SlicerTarget::OrcaSlicer => SlicerTarget::Standard,
        target => target,
    }
}

pub(crate) fn label(target: SlicerTarget) -> &'static str {
    match target {
        SlicerTarget::Standard | SlicerTarget::BambuStudio | SlicerTarget::OrcaSlicer => {
            "Standard 3MF"
        }
        SlicerTarget::PrusaSlicer => "PrusaSlicer",
        SlicerTarget::Cura => "UltiMaker Cura",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slicer_preference_uses_the_shared_targets_and_rejects_corrupt_values() {
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("limo-cad-slicer-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("slicer-target.json");
        assert_eq!(read_at(&path).unwrap(), SlicerTarget::Standard);
        for target in SlicerTarget::all() {
            write_at(&path, *target).unwrap();
            assert_eq!(read_at(&path).unwrap(), canonical_ui_target(*target));
        }
        std::fs::write(&path, br#""unknown-slicer""#).unwrap();
        assert!(read_at(&path).is_err());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn observers_refresh_publications_immediately_and_external_writes_on_cadence_or_interaction() {
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("limo-cad-slicer-{}", uuid::Uuid::new_v4()));
        let path = root.join("slicer-target.json");
        let mut first = Observer::at(path.clone());
        let mut second = Observer::at(path.clone());
        let now = Instant::now();
        assert_eq!(
            first.poll(now, false).unwrap().unwrap(),
            SlicerTarget::Standard
        );
        assert_eq!(
            second.poll(now, false).unwrap().unwrap(),
            SlicerTarget::Standard
        );
        assert!(second.poll(now, false).is_none());
        first.write(SlicerTarget::Cura).unwrap();
        assert_eq!(
            second.poll(now, false).unwrap().unwrap(),
            SlicerTarget::Cura
        );
        assert!(second.poll(now, false).is_none());

        std::fs::write(
            &path,
            serde_json::to_vec(&SlicerTarget::PrusaSlicer).unwrap(),
        )
        .unwrap();
        assert!(second.poll(now, false).is_none());
        assert_eq!(
            second.poll(now, true).unwrap().unwrap(),
            SlicerTarget::PrusaSlicer
        );
        std::fs::write(&path, serde_json::to_vec(&SlicerTarget::Standard).unwrap()).unwrap();
        assert!(second.poll(now, false).is_none());
        assert_eq!(
            second.poll(now + REFRESH_INTERVAL, false).unwrap().unwrap(),
            SlicerTarget::Standard
        );

        let publication = PUBLICATION.load(Ordering::Acquire);
        let before = std::fs::read(&path).unwrap();
        let mut impossible = Observer::at(path.join("not-a-directory.json"));
        assert!(impossible.write(SlicerTarget::Cura).is_err());
        assert_eq!(PUBLICATION.load(Ordering::Acquire), publication);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
