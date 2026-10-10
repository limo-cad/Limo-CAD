//! Register portable releases for this user, without depending on an installer.
use std::{path::PathBuf, process::Command};

const DESKTOP_ID: &str = "limo-cad.desktop";

fn executable() -> Result<PathBuf, String> {
    let executable = match std::env::var_os("APPIMAGE") {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => std::env::current_exe().map_err(|error| error.to_string())?,
    };
    let executable = if executable.is_absolute() {
        executable
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(executable)
    };
    if !executable.is_file() {
        return Err("The recipe handler executable no longer exists".into());
    }
    Ok(executable)
}

fn exec_argument(path: &str) -> Result<String, String> {
    if path.contains('=') || path.chars().any(char::is_control) {
        return Err("The executable path cannot be represented by a desktop Exec entry".into());
    }
    let mut quoted = String::from("\"");
    for character in path.chars() {
        match character {
            '\\' => quoted.push_str("\\\\\\\\"),
            '"' | '`' | '$' => {
                quoted.push_str("\\\\");
                quoted.push(character);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

pub(super) fn register() -> Result<(), String> {
    let executable = executable()?;
    let executable = executable
        .to_str()
        .ok_or("The recipe handler executable path must be Unicode")?;
    let exec = exec_argument(executable)?;
    let contents = format!(
        "[Desktop Entry]\nType=Application\nName=Limo CAD\n\
         Exec={exec} %u\nIcon=limo-cad\nTerminal=false\n\
         Categories=Graphics;Engineering;\nStartupWMClass=limo-cad\n\
         MimeType=x-scheme-handler/limo-cad;x-scheme-handler/nbcad;\n"
    );
    let applications = dirs::data_dir()
        .ok_or("Could not locate the user's application data directory")?
        .join("applications");
    std::fs::create_dir_all(&applications).map_err(|error| error.to_string())?;
    let desktop = applications.join(DESKTOP_ID);
    if std::fs::read(&desktop).ok().as_deref() != Some(contents.as_bytes()) {
        limo_cad_project_file::write_binary_file_atomic(&desktop, contents.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    let status = Command::new("xdg-mime")
        .args([
            "default",
            DESKTOP_ID,
            "x-scheme-handler/limo-cad",
            "x-scheme-handler/nbcad",
        ])
        .status()
        .map_err(|error| format!("Could not start xdg-mime: {error}"))?;
    if !status.success() {
        return Err(format!(
            "xdg-mime could not register recipe links: {status}"
        ));
    }
    Ok(())
}
