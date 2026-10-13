//! Production open/save uses the OS file dialog. Tests install the choice
//! before the picker thread starts, so a library test never shows a modal.
use bevy::{prelude::World, window::RawHandleWrapper};
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Mutex;

#[cfg(test)]
static PREPARED: Mutex<Option<Option<PathBuf>>> = Mutex::new(None);

/// A prepared test choice never creates an OS dialog, so it needs no window.
/// Actual dialogs still qualify and retain the exact owning desktop window.
pub(super) fn parented(
    world: &World,
    dialog: rfd::FileDialog,
) -> Result<(rfd::FileDialog, Option<RawHandleWrapper>), String> {
    #[cfg(test)]
    if PREPARED.lock().expect("script dialog choice").is_some() {
        return Ok((dialog, None));
    }
    super::super::parented_dialog(world, dialog).map(|(dialog, parent)| (dialog, Some(parent)))
}

pub(super) fn pick(dialog: rfd::FileDialog, save: bool) -> Option<PathBuf> {
    #[cfg(test)]
    {
        if let Some(choice) = PREPARED.lock().expect("script dialog choice").take() {
            return choice;
        }
    }
    if save {
        dialog.save_file()
    } else {
        dialog.pick_file()
    }
}

/// `None` is Cancel. A path is the file the dialog would return.
#[cfg(test)]
pub(crate) fn prepare(path: Option<PathBuf>) {
    *PREPARED.lock().expect("script dialog choice") = Some(path);
}
