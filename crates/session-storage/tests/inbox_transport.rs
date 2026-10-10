//! Exercise the actual MCP publication implementation without its OCCT host.
#[path = "../../../mcp-server/src/inbox.rs"]
mod inbox;

use std::{fs, path::PathBuf};

struct Registry {
    path: PathBuf,
    previous: Option<std::ffi::OsString>,
}

impl Drop for Registry {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var("LIMO_CAD_SESSION_DIR", value),
            None => std::env::remove_var("LIMO_CAD_SESSION_DIR"),
        }
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn production_discovery_checks_registry_and_archives() {
    let path = std::env::temp_dir().join(format!(
        "limo-cad-inbox-registry-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let registry = Registry {
        path,
        previous: std::env::var_os("LIMO_CAD_SESSION_DIR"),
    };
    std::env::set_var("LIMO_CAD_SESSION_DIR", &registry.path);
    let commands = registry.path.join("document/inbox");
    limo_cad_session_storage::atomic_write(&commands.join("7.json"), b"completed command").unwrap();
    assert_eq!(inbox::sequences(&commands).unwrap(), vec![7]);
    #[cfg(unix)]
    {
        let outside = registry.path.join("elsewhere");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(outside, commands.join("applied")).unwrap();
        let mut wrote = false;
        assert!(inbox::publish_with_timeout(
            &registry.path,
            &commands,
            std::time::Duration::from_secs(1),
            |_| {
                wrote = true;
                Ok(())
            }
        )
        .is_err());
        assert!(!wrote);
        assert!(!commands.join("8.json").exists());
    }
}
