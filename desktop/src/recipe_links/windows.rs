//! Register recipe and project handlers without replacing a valid installed runtime.
use std::path::PathBuf;
use windows::{
    core::{HSTRING, PCWSTR},
    Win32::System::Registry::*,
};

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
fn set(path: &str, name: &str, value: &str) -> Result<(), String> {
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(path),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()
        .map_err(|error| error.to_string())?;
    }
    let key = Key(key);
    let bytes: Vec<u8> = value
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    unsafe {
        RegSetValueExW(key.0, &HSTRING::from(name), None, REG_SZ, Some(&bytes))
            .ok()
            .map_err(|error| error.to_string())
    }
}

fn registered_executable(class: &str) -> Option<PathBuf> {
    let key = HSTRING::from(format!(r"Software\Classes\{class}\shell\open\command"));
    let mut size = 0;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    if size == 0 || size > 65_536 {
        return None;
    }
    let mut value = vec![0u16; (size as usize).div_ceil(2)];
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    let end = value.iter().position(|unit| *unit == 0)?;
    let command = String::from_utf16(&value[..end]).ok()?;
    let (executable, arguments) = command.strip_prefix('"')?.split_once('"')?;
    if arguments.trim() != "\"%1\"" {
        return None;
    }
    let executable = PathBuf::from(executable);
    (executable.is_absolute() && executable.is_file()).then_some(executable)
}

fn handler_executable() -> Result<PathBuf, String> {
    if let Some(installed) = dirs::data_local_dir()
        .map(|directory| directory.join("limo-cad/bevy/Limo-CAD.exe"))
        .filter(|path| path.is_file())
    {
        return Ok(installed);
    }
    for class in ["limo-cad", "LimoCAD.Project"] {
        if let Some(executable) = registered_executable(class) {
            return Ok(executable);
        }
    }
    std::env::current_exe().map_err(|error| error.to_string())
}

pub(super) fn register() -> Result<(), String> {
    let executable = handler_executable()?;
    let executable = executable
        .to_str()
        .ok_or("The recipe handler executable path must be Unicode")?;
    if executable.contains(['"', '\0']) {
        return Err("Invalid recipe handler executable path".into());
    }
    for scheme in ["limo-cad", "nbcad"] {
        let key = format!(r"Software\Classes\{scheme}");
        set(
            &format!(r"{key}\shell\open\command"),
            "",
            &format!("\"{executable}\" \"%1\""),
        )?;
        set(&key, "", "URL:Limo CAD Recipe")?;
        set(&key, "URL Protocol", "")?;
    }
    let project = r"Software\Classes\LimoCAD.Project";
    set(project, "", "Limo CAD project")?;
    set(
        &format!(r"{project}\DefaultIcon"),
        "",
        &format!("\"{executable}\",0"),
    )?;
    set(
        &format!(r"{project}\shell\open\command"),
        "",
        &format!("\"{executable}\" \"%1\""),
    )?;
    for extension in [".limo", ".nbcad"] {
        set(
            &format!(r"Software\Classes\{extension}"),
            "",
            "LimoCAD.Project",
        )?;
    }
    Ok(())
}
