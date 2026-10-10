//! Window selection is independent of the always-available stdio transport.
use std::{ffi::OsString, path::PathBuf};

pub const USAGE: &str = "Usage: Limo CAD [--headless | PROJECT.limo | limo-cad://recipe/ID]\n\
    With no arguments, open the desktop with local stdio MCP available.\n\
    --headless runs the same MCP interface without a window.\n\
    A recipe URL opens editable source for review; it does not run it.";

#[derive(Debug, PartialEq, Eq)]
pub enum Startup {
    Desktop,
    Recipe(&'static str),
    Project(PathBuf),
    Headless,
    Help,
}

pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Startup, String> {
    let mut arguments = arguments.into_iter();
    let Some(first) = arguments.next() else {
        return Ok(Startup::Desktop);
    };
    if arguments.next().is_some() {
        return Err(format!("Expected at most one argument.\n{USAGE}"));
    }
    match first.to_str() {
        Some("--headless") => Ok(Startup::Headless),
        Some("--help" | "-h") => Ok(Startup::Help),
        Some("--mcp") => Err(format!(
            "Stdio MCP is always enabled. Replace --mcp with --headless to run without a window.\n{USAGE}"
        )),
        Some(uri) if uri.starts_with("limo-cad:") || uri.starts_with("nbcad:") => {
            limo_cad_mcp::recipe_id_from_uri(uri).map(Startup::Recipe)
        }
        _ => {
            let path = PathBuf::from(first);
            if !path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension|
                ["limo", "nbcad", "tfcad"].iter().any(|allowed| extension.eq_ignore_ascii_case(allowed))) {
                return Err(format!("Unrecognized argument.\n{USAGE}"));
            }
            let path = path.canonicalize().map_err(|error| format!("Cannot open project: {error}"))?;
            if !path.is_file() { return Err("Choose a regular project file".into()); }
            Ok(Startup::Project(path))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(arguments: &[&str]) -> Result<Startup, String> {
        parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn only_headless_suppresses_the_desktop() {
        assert_eq!(args(&[]), Ok(Startup::Desktop));
        assert_eq!(args(&["--headless"]), Ok(Startup::Headless));
        assert_eq!(args(&["--help"]), Ok(Startup::Help));
        assert!(args(&["--mcp"]).unwrap_err().contains("--headless"));
        assert!(args(&["--headless", "limo-cad://recipe/fillet-basics"]).is_err());
        assert!(args(&["limo-cad://recipe/fillet-basics", "--headless"]).is_err());
        assert!(args(&["--headless", "--headless"]).is_err());
        assert!(args(&["--unknown"]).is_err());
    }

    #[test]
    fn recipe_launches_keep_the_installed_source_contract() {
        assert_eq!(
            args(&["limo-cad://recipe/fillet-basics"]),
            Ok(Startup::Recipe("fillet-basics"))
        );
        assert!(args(&["limo-cad://recipe/fillet-basics?run=true"]).is_err());
        assert!(args(&["limo-cad://recipe/not-installed"]).is_err());
        assert!(args(&["limo-cad://recipe/fillet-basics", "extra"]).is_err());
    }

    #[test]
    fn project_launches_accept_current_and_previous_extensions() {
        let directory =
            std::env::temp_dir().join(format!("limo-cad-startup-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        for name in ["design with spaces.limo", "part.NBCAD", "previous.tfcad"] {
            let path = directory.join(name);
            std::fs::write(&path, b"owned parser fixture").unwrap();
            assert_eq!(
                parse([path.clone().into_os_string()]).unwrap(),
                Startup::Project(path.canonicalize().unwrap())
            );
            std::fs::remove_file(path).unwrap();
        }
        assert!(parse([directory.join("missing.limo").into_os_string()]).is_err());
        assert!(parse([directory.join("unsupported.txt").into_os_string()]).is_err());
        std::fs::remove_dir(directory).unwrap();
    }
}
