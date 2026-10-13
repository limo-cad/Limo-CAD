//! Bind the packaged logo to each native window. The executable's Windows
//! resource supplies the taskbar icon; Winit needs a separate title-bar binding.

use bevy::{
    asset::RenderAssetUsages,
    ecs::system::NonSendMarker,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
    render::render_resource::TextureFormat,
    window::WindowCreated,
    winit::WINIT_WINDOWS,
};
use winit::window::Icon;

#[derive(Resource)]
struct WindowIcon(Icon);

pub(super) fn install(app: &mut App) {
    match decode_icon() {
        Ok(icon) => {
            app.insert_resource(WindowIcon(icon))
                .add_systems(First, bind_created_windows);
        }
        Err(error) => eprintln!("Cannot load native window icon: {error}"),
    }
}

fn decode_icon() -> Result<Icon, String> {
    let image = Image::from_buffer(
        include_bytes!("../../../icons/128x128@2x.png"),
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::MAIN_WORLD,
    )
    .map_err(|error| error.to_string())?
    .convert(TextureFormat::Rgba8UnormSrgb)
    .ok_or("Logo cannot be converted to RGBA8")?;
    let size = image.texture_descriptor.size;
    let pixels = image.data.ok_or("Logo has no pixels")?;
    Icon::from_rgba(pixels, size.width, size.height).map_err(|error| error.to_string())
}

fn bind_created_windows(
    mut created: MessageReader<WindowCreated>,
    icon: Res<WindowIcon>,
    _main_thread: NonSendMarker,
) {
    for event in created.read() {
        WINIT_WINDOWS.with_borrow(|windows| {
            if let Some(window) = windows.get_window(event.window) {
                window.set_window_icon(Some(icon.0.clone()));
            }
        });
    }
}
