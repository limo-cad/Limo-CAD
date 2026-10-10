//! One Bevy readback. Window capture, script preview, and the UI lab all
//! encode the image Bevy already rendered. Nothing here grabs the desktop.

use bevy::image::Image;
use std::io::Cursor;

/// PNG bytes of a rendered Bevy image. Callers write the file or return the bytes.
pub(crate) fn png_bytes(image: &Image) -> Result<Vec<u8>, String> {
    let dynamic = image
        .clone()
        .try_into_dynamic()
        .map_err(|error| format!("Capture image: {error}"))?;
    let mut png = Cursor::new(Vec::new());
    dynamic
        .write_to(
            &mut png,
            bevy::image::ImageFormat::Png
                .as_image_crate_format()
                .expect("PNG support is enabled"),
        )
        .map_err(|error| format!("Encode capture: {error}"))?;
    Ok(png.into_inner())
}

/// Pump until `step` reports the screenshot observer has the image.
/// There is no warmup delay: the observer is the completion signal.
pub(crate) fn until_captured(mut step: impl FnMut() -> Result<bool, String>) -> Result<(), String> {
    for _ in 0..60 {
        if step()? {
            return Ok(());
        }
    }
    Err("The renderer did not complete the capture".into())
}
