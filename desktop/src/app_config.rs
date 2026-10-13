//! One per-user configuration root for desktop settings and libraries.
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub(crate) const IDENTIFIER: &str = "org.limocad.desktop";
const PREVIOUS_IDENTIFIER: &str = "org.nbcad.desktop";

pub(crate) fn directory(identifier: &str) -> Result<PathBuf, String> {
    let configured = std::env::var_os("LIMO_CAD_CONFIG_DIR").map(PathBuf::from);
    let root = dirs::config_dir();
    let path = resolve(identifier, configured.clone(), root.clone())?;
    if configured.is_none() && identifier == IDENTIFIER {
        migrate_profile(
            root.as_deref()
                .ok_or("Could not resolve the per-user config directory")?,
        )?;
    }
    Ok(path)
}

/// The override names the complete app configuration folder, not the OS root.
/// It isolates owned QA hosts and supports portable desktop configurations.
fn resolve(
    identifier: &str,
    configured: Option<PathBuf>,
    default_root: Option<PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(directory) = configured {
        if !directory.is_absolute() {
            return Err(
                "LIMO_CAD_CONFIG_DIR must be an absolute application configuration directory"
                    .into(),
            );
        }
        return Ok(directory);
    }
    default_root
        .map(|root| root.join(identifier))
        .ok_or_else(|| "Could not resolve the per-user config directory".into())
}

/// Move the whole old profile atomically, including private libraries and
/// recovery data. Explicit overrides never migrate another user's profile.
/// Two populated profiles require reconciliation; neither may be overwritten.
fn migrate_profile(root: &Path) -> Result<(), String> {
    let previous = root.join(PREVIOUS_IDENTIFIER);
    let current = root.join(IDENTIFIER);
    let metadata = match fs::symlink_metadata(&previous) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "Could not inspect the previous CAD profile: {error}"
            ))
        }
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("The previous CAD profile must be a real directory".into());
    }
    match fs::symlink_metadata(&current) {
        Ok(_) if !previous.exists() && current.is_dir() => return Ok(()),
        Ok(_) => {
            return Err(format!(
                "Both CAD profiles exist; reconcile {} and {} before starting Limo CAD",
                previous.display(),
                current.display()
            ))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Could not inspect the Limo CAD profile: {error}")),
    }
    if let Err(error) = fs::rename(&previous, &current) {
        if previous.exists() || !current.is_dir() {
            return Err(format!(
                "Could not migrate the CAD profile without overwriting data: {error}"
            ));
        }
    }
    Ok(())
}

pub(crate) fn native_directory() -> Result<PathBuf, String> {
    directory(IDENTIFIER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_configuration_override_is_exact_and_does_not_change_the_default_root() {
        let root = std::env::temp_dir().join("limo-cad-config-test");
        let isolated = root.join("owned-host-config");
        assert_eq!(
            resolve("app.identifier", Some(isolated.clone()), Some(root.clone())).unwrap(),
            isolated
        );
        assert_eq!(
            resolve("app.identifier", None, Some(root.clone())).unwrap(),
            root.join("app.identifier")
        );
    }

    #[test]
    fn invalid_configuration_override_cannot_fall_back_to_the_user_library() {
        for path in [PathBuf::new(), PathBuf::from("relative-library")] {
            let error =
                resolve("app.identifier", Some(path), Some(std::env::temp_dir())).unwrap_err();
            assert!(error.contains("LIMO_CAD_CONFIG_DIR"));
        }
        assert!(resolve("app.identifier", None, None).is_err());
    }

    #[test]
    fn profile_migration_preserves_nested_libraries_and_rejects_collisions() {
        let root =
            std::env::temp_dir().join(format!("limo-cad-profile-test-{}", uuid::Uuid::new_v4()));
        let previous = root.join(PREVIOUS_IDENTIFIER);
        let current = root.join(IDENTIFIER);
        fs::create_dir_all(previous.join("libraries")).unwrap();
        fs::write(previous.join("libraries/private.json"), "private library").unwrap();
        migrate_profile(&root).unwrap();
        assert!(!previous.exists());
        assert_eq!(
            fs::read_to_string(current.join("libraries/private.json")).unwrap(),
            "private library"
        );
        migrate_profile(&root).unwrap();
        fs::create_dir(&previous).unwrap();
        fs::write(previous.join("preferences.json"), "older preferences").unwrap();
        assert!(migrate_profile(&root).is_err());
        assert_eq!(
            fs::read_to_string(previous.join("preferences.json")).unwrap(),
            "older preferences"
        );
        assert_eq!(
            fs::read_to_string(current.join("libraries/private.json")).unwrap(),
            "private library"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
