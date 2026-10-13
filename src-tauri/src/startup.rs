//! Window selection is independent of the always-available stdio transport.
use crate::launch_requests::LaunchTarget;
use std::{ffi::OsString, path::Path};

pub const USAGE: &str =
    "Usage: Limo CAD [--new-window | --reuse-window] [FILE | nbcad://recipe/ID ...]\n\
    With no arguments, show the existing window for this executable, or open it.\n\
    --new-window opens an independent window.\n\
    --reuse-window explicitly enables reuse within an isolated diagnostic profile.\n\
    --headless runs local stdio MCP without a window.\n\
    A recipe URL opens editable source for review; it does not run it.";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct DesktopLaunch {
    pub new_window: bool,
    pub reuse_window: bool,
    pub targets: Vec<LaunchTarget>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Startup {
    Desktop(DesktopLaunch),
    Headless,
    Help,
}

pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Startup, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    parse_in(arguments, &cwd)
}

fn parse_in(arguments: impl IntoIterator<Item = OsString>, cwd: &Path) -> Result<Startup, String> {
    let arguments: Vec<_> = arguments.into_iter().collect();
    if arguments.len() == 1 {
        match arguments[0].to_str() {
            Some("--headless") => return Ok(Startup::Headless),
            Some("--help" | "-h") => return Ok(Startup::Help),
            Some("--mcp") => {
                return Err(format!(
                    "Stdio MCP is always enabled. Replace --mcp with --headless.\n{USAGE}"
                ))
            }
            _ => {}
        }
    }
    if arguments.len() > 18 {
        return Err("Too many launch arguments".into());
    }
    let mut launch = DesktopLaunch::default();
    let mut files_only = false;
    for argument in arguments {
        let value = argument
            .to_str()
            .ok_or("Launch arguments must be valid Unicode")?;
        match value {
            "--" if !files_only => {
                files_only = true;
                continue;
            }
            "--new-window" if !files_only && !launch.new_window => {
                launch.new_window = true;
                continue;
            }
            "--reuse-window" if !files_only && !launch.reuse_window => {
                launch.reuse_window = true;
                continue;
            }
            flag if !files_only && flag.starts_with('-') => {
                return Err(format!("Unrecognized argument: {flag}\n{USAGE}"))
            }
            _ => {}
        }
        launch
            .targets
            .push(LaunchTarget::from_argument(value, cwd)?);
        if launch.targets.len() > 16 {
            return Err("Too many launch targets".into());
        }
    }
    if launch.new_window && launch.reuse_window {
        return Err("Choose either --new-window or --reuse-window".into());
    }
    Ok(Startup::Desktop(launch))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(arguments: &[&str]) -> Result<Startup, String> {
        parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn only_headless_suppresses_the_desktop() {
        assert_eq!(args(&[]), Ok(Startup::Desktop(DesktopLaunch::default())));
        assert_eq!(args(&["--headless"]), Ok(Startup::Headless));
        assert_eq!(args(&["--help"]), Ok(Startup::Help));
        assert!(args(&["--mcp"]).unwrap_err().contains("--headless"));
        assert!(args(&["--headless", "nbcad://recipe/fillet-basics"]).is_err());
        assert!(args(&["nbcad://recipe/fillet-basics", "--headless"]).is_err());
        assert!(args(&["--headless", "--headless"]).is_err());
        assert!(args(&["--unknown"]).is_err());
    }

    #[test]
    fn recipe_launches_keep_the_installed_source_contract() {
        let Startup::Desktop(launch) = args(&[
            "--new-window",
            "part Ω.nbcad",
            "nbcad://recipe/fillet-basics",
        ])
        .unwrap() else {
            panic!()
        };
        assert!(launch.new_window);
        assert_eq!(launch.targets.len(), 2);
        assert!(
            matches!(&launch.targets[0], LaunchTarget::File { path } if Path::new(path).is_absolute() && path.ends_with("part Ω.nbcad"))
        );
        assert_eq!(
            launch.targets[1],
            LaunchTarget::Recipe {
                recipe: "fillet-basics".into()
            }
        );
        assert!(args(&["nbcad://recipe/fillet-basics?run=true"]).is_err());
        assert!(args(&["nbcad://recipe/not-installed"]).is_err());
        assert!(args(&["--new-window", "--reuse-window"]).is_err());
        assert!(args(&["--reuse-window"]).is_ok());
        assert!(args(&["--", "-part.nbcad"]).is_ok());
    }
}
