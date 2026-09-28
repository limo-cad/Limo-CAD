//! One per-user configuration root for the React and native desktop hosts.
use std::path::PathBuf;

pub(crate) fn directory(identifier: &str) -> Result<PathBuf, String> {
    resolve(
        identifier,
        std::env::var_os("NBCAD_CONFIG_DIR").map(PathBuf::from),
        dirs::config_dir(),
    )
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
                "NBCAD_CONFIG_DIR must be an absolute application configuration directory".into(),
            );
        }
        return Ok(directory);
    }
    default_root
        .map(|root| root.join(identifier))
        .ok_or_else(|| "Could not resolve the per-user config directory".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_configuration_override_is_exact_and_does_not_change_the_default_root() {
        let root = std::env::temp_dir().join("nbcad-config-test");
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
            assert!(error.contains("NBCAD_CONFIG_DIR"));
        }
        assert!(resolve("app.identifier", None, None).is_err());
    }
}

pub(crate) const IDENTIFIER: &str = "org.nbcad.desktop";

pub(crate) fn native_directory() -> Result<PathBuf,String> { directory(IDENTIFIER) }
