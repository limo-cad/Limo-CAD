//! Application appearance/navigation preferences shared by desktop hosts.
//!
//! Missing fields follow current OS defaults until the user saves a choice.
//! These preferences are never written into a CAD document or its Undo history.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

pub(crate) mod locale;
pub(crate) mod palette;

const FILE_NAME: &str = "app-preferences.json";
const LOCK_NAME: &str = ".app-preferences.lock";
const SCHEMA_VERSION: u32 = 1;
const MAX_BYTES: u64 = 64 * 1024;
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);
pub(crate) const DEFAULT_SIX_DOF_SPEED: f64 = 1.5;
pub(crate) const DEFAULT_UI_SCALE: f64 = 1.;
pub(crate) const UI_SCALE_OPTIONS: [f64; 6] = [0.9, 1., 1.1, 1.25, 1.5, 1.75];
pub(crate) const MIN_SIX_DOF_SPEED: f64 = 0.25;
pub(crate) const MAX_SIX_DOF_SPEED: f64 = 3.;

static STORAGE: Mutex<()> = Mutex::new(());
static PUBLICATION: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ResolvedTheme {
    Light,
    Dark,
}

impl ThemePreference {
    pub(crate) fn resolve(self, system_dark: bool) -> ResolvedTheme {
        match self {
            Self::Dark => ResolvedTheme::Dark,
            Self::Light => ResolvedTheme::Light,
            Self::System if system_dark => ResolvedTheme::Dark,
            Self::System => ResolvedTheme::Light,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) enum Locale {
    #[default]
    #[serde(rename = "en")]
    En,
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "es")]
    Es,
    #[serde(rename = "de")]
    De,
}

/// Only persisted, explicit choices. None also means "leave unchanged" in a
/// patch; Reset is an explicit System/default-speed choice, not field deletion.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Preferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemePreference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<Locale>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub six_dof_speed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_stock_removal: Option<bool>,
}

impl Preferences {
    pub(crate) fn effective(&self, detected_locale: Locale) -> Effective {
        Effective {
            theme: self.theme.unwrap_or_default(),
            locale: self.locale.unwrap_or(detected_locale),
            six_dof_speed: self.six_dof_speed.unwrap_or(DEFAULT_SIX_DOF_SPEED),
            ui_scale: self.ui_scale.map(snap_ui_scale).unwrap_or(DEFAULT_UI_SCALE),
            gpu_stock_removal: self.gpu_stock_removal.unwrap_or(true),
        }
    }

    fn normalized(mut self) -> Self {
        self.six_dof_speed = self.six_dof_speed.map(clamp_six_dof_speed);
        self.ui_scale = self.ui_scale.map(snap_ui_scale);
        self
    }

    fn merge(&mut self, patch: Self) {
        if patch.theme.is_some() {
            self.theme = patch.theme;
        }
        if patch.locale.is_some() {
            self.locale = patch.locale;
        }
        if patch.six_dof_speed.is_some() {
            self.six_dof_speed = patch.six_dof_speed;
        }
        if patch.gpu_stock_removal.is_some() {
            self.gpu_stock_removal = patch.gpu_stock_removal;
        }
        if patch.ui_scale.is_some() {
            self.ui_scale = patch.ui_scale;
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub(crate) struct Effective {
    pub theme: ThemePreference,
    pub locale: Locale,
    pub six_dof_speed: f64,
    pub ui_scale: f64,
    pub gpu_stock_removal: bool,
}

pub(crate) fn snap_ui_scale(value: f64) -> f64 {
    if !value.is_finite() {
        return DEFAULT_UI_SCALE;
    }
    let mut best = DEFAULT_UI_SCALE;
    let mut distance = f64::INFINITY;
    for option in UI_SCALE_OPTIONS {
        let next = (option - value).abs();
        if next < distance {
            best = option;
            distance = next;
        }
    }
    best
}

pub(crate) fn step_ui_scale(value: f64, increase: bool) -> f64 {
    let value = snap_ui_scale(value);
    let index = UI_SCALE_OPTIONS
        .iter()
        .position(|option| *option == value)
        .expect("snapped interface size is an available step");
    let next = if increase {
        (index + 1).min(UI_SCALE_OPTIONS.len() - 1)
    } else {
        index.saturating_sub(1)
    };
    UI_SCALE_OPTIONS[next]
}

pub(crate) fn clamp_six_dof_speed(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(MIN_SIX_DOF_SPEED, MAX_SIX_DOF_SPEED)
    } else {
        DEFAULT_SIX_DOF_SPEED
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct Stored {
    schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    theme: Option<ThemePreference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    locale: Option<Locale>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    six_dof_speed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ui_scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gpu_stock_removal: Option<bool>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

impl Default for Stored {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            theme: None,
            locale: None,
            six_dof_speed: None,
            ui_scale: None,
            gpu_stock_removal: None,
            extra: BTreeMap::new(),
        }
    }
}

impl Stored {
    fn preferences(&self) -> Preferences {
        Preferences {
            theme: self.theme,
            locale: self.locale,
            six_dof_speed: self.six_dof_speed,
            ui_scale: self.ui_scale.map(snap_ui_scale),
            gpu_stock_removal: self.gpu_stock_removal,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "Unsupported application preference schema {}",
                self.schema_version
            ));
        }
        if self.six_dof_speed.is_some_and(|speed| {
            !speed.is_finite() || !(MIN_SIX_DOF_SPEED..=MAX_SIX_DOF_SPEED).contains(&speed)
        }) {
            return Err("Saved 6DoF speed must be between 0.25 and 3".into());
        }
        if self.ui_scale.is_some_and(|scale| {
            !scale.is_finite() || !(UI_SCALE_OPTIONS[0]..=UI_SCALE_OPTIONS[5]).contains(&scale)
        }) {
            return Err("Saved interface size must be between 90% and 175%".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Store {
    path: PathBuf,
}

impl Store {
    pub(crate) fn native() -> Result<Self, String> {
        Self::at(crate::app_config::native_directory()?)
    }

    /// The directory is the complete application config folder, including the
    /// existing LIMO_CAD_CONFIG_DIR override. Merely reading creates no files.
    pub(crate) fn at(directory: PathBuf) -> Result<Self, String> {
        if !directory.is_absolute() {
            return Err("Application preference directory must be absolute".into());
        }
        Ok(Self {
            path: directory.join(FILE_NAME),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn read(&self) -> Result<Preferences, String> {
        self.read_stored().map(|stored| stored.preferences())
    }

    pub(crate) fn patch(&self, patch: Preferences) -> Result<Preferences, String> {
        self.update(patch.normalized())
    }

    fn read_stored(&self) -> Result<Stored, String> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Stored::default())
            }
            Err(error) => return Err(format!("Cannot open application preferences: {error}")),
        };
        let metadata = file
            .metadata()
            .map_err(|error| format!("Cannot inspect application preferences: {error}"))?;
        if !metadata.is_file() || metadata.len() > MAX_BYTES {
            return Err("Application preferences must be a regular file within 64 KiB".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Cannot read application preferences: {error}"))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Application preferences exceed 64 KiB".into());
        }
        let stored: Stored = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Cannot parse application preferences: {error}"))?;
        stored.validate()?;
        Ok(stored)
    }

    fn update(&self, patch: Preferences) -> Result<Preferences, String> {
        if patch == Preferences::default() {
            return self.read();
        }
        let _process = STORAGE
            .lock()
            .map_err(|_| "Application preference storage lock is unavailable")?;
        let directory = self
            .path
            .parent()
            .ok_or("Preference directory is missing")?;
        fs::create_dir_all(directory)
            .map_err(|error| format!("Cannot create application preference directory: {error}"))?;
        let writer = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join(LOCK_NAME))
            .map_err(|error| format!("Cannot open application preference lock: {error}"))?;
        writer.try_lock().map_err(|error| {
            format!("Application preferences are busy or unavailable; retry after the other writer finishes: {error}")
        })?;
        let mut stored = self.read_stored()?;
        let before = stored.preferences();
        let mut after = before.clone();
        after.merge(patch);
        if after == before {
            return Ok(before);
        }
        stored.theme = after.theme;
        stored.locale = after.locale;
        stored.six_dof_speed = after.six_dof_speed;
        stored.ui_scale = after.ui_scale;
        stored.gpu_stock_removal = after.gpu_stock_removal;
        stored.validate()?;
        let bytes = serde_json::to_vec_pretty(&stored)
            .map_err(|error| format!("Cannot serialize application preferences: {error}"))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Application preferences exceed 64 KiB".into());
        }
        limo_cad_project_file::write_binary_file_atomic(&self.path, &bytes)
            .map_err(|error| format!("Cannot save application preferences: {error}"))?;
        PUBLICATION.fetch_add(1, Ordering::Release);
        Ok(after)
    }
}

/// Same-process changes publish immediately. Other processes are observed at
/// a bounded cadence; force a fresh read before interpreting a Settings edit.
pub(crate) struct Observer {
    store: Store,
    checked: Option<Instant>,
    publication: u64,
}

impl Observer {
    pub(crate) fn new(store: Store) -> Self {
        Self {
            store,
            checked: None,
            publication: 0,
        }
    }

    pub(crate) fn poll(
        &mut self,
        now: Instant,
        force: bool,
    ) -> Option<Result<Preferences, String>> {
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
        Some(self.store.read())
    }
}

#[cfg(test)]
mod tests;
