use super::*;
use std::path::Path;

pub(super) fn safe_stem(value: &str) -> String {
    let mut result = String::new();
    for c in value.trim().chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            result.push(c);
        } else if !result.ends_with('-') {
            result.push('-');
        }
    }
    let result = result.trim_matches(['-', '.']);
    if result.is_empty() {
        "program".into()
    } else {
        result.into()
    }
}

pub(super) fn choose(
    world: &World,
    handle: &NativeInterfaceHandle,
    state: &mut State,
) -> Result<Value, String> {
    let prepared = state
        .prepared
        .as_ref()
        .ok_or("Prepare the output first")?
        .clone();
    if state.picker.is_some() {
        return Err("A save chooser is already open".into());
    }
    if files::awaiting(world) {
        return Err("Finish the File dialog first".into());
    }
    let (send, receive) = mpsc::channel();
    let wake = handle.clone();
    let file_name = prepared.file_name.clone();
    let extension = prepared.extension.clone();
    let (dialog, parent) = files::parented_dialog(
        world,
        rfd::FileDialog::new()
            .add_filter("CAM output", &[extension.as_str()])
            .set_file_name(file_name),
    )?;
    std::thread::Builder::new()
        .name("cad-nc-save-picker".into())
        .spawn(move || {
            let _parent = parent;
            let path = dialog.save_file();
            let _ = send.send(path);
            wake.request_redraw();
        })
        .map_err(|e| format!("Cannot open save chooser: {e}"))?;
    state.picker = Some(Picker {
        serial: state.serial,
        prepared,
        receive: Mutex::new(receive),
    });
    Ok(json!({"awaiting_input":true}))
}

fn check_path(path: &Path, extension: &str) -> Result<(), String> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err("Choose an absolute output filename".into());
    }
    if !path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
    {
        return Err(format!("Choose an output filename ending in .{extension}"));
    }
    Ok(())
}

fn write_prepared(
    services: &NativeServices,
    prepared: &Prepared,
    path: &Path,
    overwrite: bool,
    validate: impl FnOnce() -> Result<(), String>,
) -> Result<NativeMutationResult, String> {
    check_path(path, &prepared.extension)?;
    services.bridge.with_native_document_receipt(
        &services.engine,
        &prepared.receipt.owner,
        |revision| {
            current(&prepared.receipt, revision)?;
            validate()?;
            if overwrite {
                limo_cad_project_file::write_binary_file_atomic(path, &prepared.bytes)
            } else {
                limo_cad_project_file::write_binary_file_new(path, &prepared.bytes)
            }
            .map_err(|e| e.to_string())?;
            Ok(NativeMutationResult {
                context: prepared.receipt.owner.clone(),
                engine_revision: revision,
                value: json!({"exported":true,"path":path,"bytes":prepared.bytes.len()}),
            })
        },
    )
}

pub(super) fn save(
    world: &mut World,
    state: &mut State,
    prepared: Prepared,
    path: PathBuf,
    overwrite: bool,
) -> Result<Value, String> {
    if state
        .draft
        .as_ref()
        .is_none_or(|d| d.receipt != prepared.receipt)
    {
        return Err("Prepared output is no longer current".into());
    }
    check_path(&path, &prepared.extension)?;
    let serial = state.serial;
    state.error.clear();
    let result = worker::enqueue_document_io(
        world,
        "export_cam_output".into(),
        move |services, guard| {
            write_prepared(services, &prepared, &path, overwrite, || guard.validate())
        },
        move |world, _, result| {
            if let Some(mut state) = world
                .get_resource_mut::<State>()
                .filter(|s| s.serial == serial)
            {
                state.saving = false;
                match &result {
                    Ok(result) => {
                        state.status = format!(
                            "Saved {}",
                            result.value["path"].as_str().unwrap_or("CAM output")
                        )
                    }
                    Err(error) => state.error = error.clone(),
                }
            }
            Ok(result?.value)
        },
    );
    state.saving = result.is_ok();
    result
}

pub(super) fn poll(world: &mut World, state: &mut State) -> Result<(), String> {
    let ready =
        state
            .picker
            .as_ref()
            .and_then(|picker| match picker.receive.lock().unwrap().try_recv() {
                Ok(path) => Some(Ok(path)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The save chooser stopped unexpectedly".to_owned()))
                }
            });
    let Some(ready) = ready else {
        return Ok(());
    };
    let picker = state.picker.take().unwrap();
    if picker.serial != state.serial
        || state
            .draft
            .as_ref()
            .is_none_or(|d| d.receipt != picker.prepared.receipt)
    {
        return Ok(());
    }
    if let Some(path) = ready? {
        save(world, state, picker.prepared, path, true)?;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn write_for_test(
    services: &NativeServices,
    prepared: &Prepared,
    path: &Path,
    overwrite: bool,
) -> Result<NativeMutationResult, String> {
    write_prepared(services, prepared, path, overwrite, || Ok(()))
}
