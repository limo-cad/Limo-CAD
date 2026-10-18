//! Two GPU-resident teaching frames; no PNG decode, readback, or image upload.
use super::*;
use crate::native_viewport::script_preview::textures;
use std::collections::VecDeque;

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_FRAMES: usize = 2;

pub(super) enum FrameImage {
    Gpu(RenderedTexture),
    /// CPU-only controller tests exercise fencing/eviction without a GPU device.
    #[cfg(test)]
    Fixture(Image),
}
impl FrameImage {
    fn bytes(&self) -> Result<usize, String> {
        match self {
            Self::Gpu(texture) => {
                use bevy::render::render_resource::{TextureDimension, TextureFormat};
                let descriptor = &texture.image.texture_descriptor;
                if descriptor.dimension != TextureDimension::D2
                    || descriptor.size.depth_or_array_layers != 1
                    || descriptor.mip_level_count != 1
                    || descriptor.sample_count != 1
                    || !matches!(
                        descriptor.format,
                        TextureFormat::Rgba8UnormSrgb
                            | TextureFormat::Rgba8Unorm
                            | TextureFormat::Bgra8UnormSrgb
                            | TextureFormat::Bgra8Unorm
                    )
                {
                    return Err("Unsupported preview texture format".into());
                }
                (descriptor.size.width as usize)
                    .checked_mul(descriptor.size.height as usize)
                    .and_then(|pixels| pixels.checked_mul(4))
                    .ok_or_else(|| "Preview texture size overflow".into())
            }
            #[cfg(test)]
            Self::Fixture(image) => Ok(image.data.as_ref().map_or(0, Vec::len)),
        }
    }
    fn insert(self, world: &mut World) -> Handle<Image> {
        match self {
            Self::Gpu(texture) => textures::insert(world, texture),
            #[cfg(test)]
            Self::Fixture(image) => world.resource_mut::<Assets<Image>>().add(image),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Key {
    index: usize,
    progress: u8,
    size: [u32; 2],
    camera: [u32; 2],
    palette: crate::native_viewport::ViewportPalette,
}

impl Key {
    pub fn current(state: &State) -> Self {
        Self {
            index: state.frame_index(),
            progress: state.animation_progress,
            size: state.size,
            camera: [state.yaw.to_bits(), state.pitch.to_bits()],
            palette: state.palette,
        }
    }
}

#[derive(Default)]
pub(super) struct Cache {
    frames: VecDeque<(Key, Handle<Image>, usize)>,
    bytes: usize,
}
impl Cache {
    #[cfg(test)]
    pub fn footprint(&self) -> (usize, usize) {
        (self.frames.len(), self.bytes)
    }
    pub fn get(&self, key: Key) -> Option<Handle<Image>> {
        self.frames
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, image, _)| image.clone())
    }
    pub fn store(
        &mut self,
        world: &mut World,
        key: Key,
        image: FrameImage,
    ) -> Result<Handle<Image>, String> {
        let bytes = image.bytes()?;
        if bytes > MAX_BYTES {
            return Err("Preview image exceeds its 2 MiB texture budget".into());
        }
        if let Some(index) = self.frames.iter().position(|(k, _, _)| *k == key) {
            let (_, old, size) = self.frames.remove(index).unwrap();
            textures::remove(world, old.id());
            self.bytes -= size;
        }
        while self.frames.len() >= MAX_FRAMES || self.bytes + bytes > MAX_BYTES {
            let (_, old, size) = self.frames.pop_front().unwrap();
            textures::remove(world, old.id());
            self.bytes -= size;
        }
        let image = image.insert(world);
        self.frames.push_back((key, image.clone(), bytes));
        self.bytes += bytes;
        Ok(image)
    }
    pub fn drain(&mut self) -> Vec<Handle<Image>> {
        self.bytes = 0;
        self.frames.drain(..).map(|(_, image, _)| image).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    #[test]
    fn native_script_preview_texture_budget_evicts_and_releases_owned_assets() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        let mut cache = Cache::default();
        let state = State::default();
        let mut key = Key::current(&state);
        for index in 0..5 {
            key.index = index;
            let image = Image::new_fill(
                Extent3d {
                    width: 512,
                    height: 512,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                &[64, 128, 192, 255],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::MAIN_WORLD,
            );
            cache
                .store(&mut world, key, FrameImage::Fixture(image))
                .unwrap();
        }
        assert_eq!(world.resource::<Assets<Image>>().len(), 2);
        assert_eq!(cache.footprint(), (2, MAX_BYTES));
        key.index = 0;
        assert!(cache.get(key).is_none());
        for image in cache.drain() {
            textures::remove(&mut world, image.id());
        }
        assert!(world.resource::<Assets<Image>>().is_empty());
        for index in 0..17 {
            key.index = index;
            let image = Image::new_fill(
                Extent3d {
                    width: 2,
                    height: 2,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                &[0, 0, 0, 255],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::MAIN_WORLD,
            );
            cache
                .store(&mut world, key, FrameImage::Fixture(image))
                .unwrap();
        }
        assert_eq!(world.resource::<Assets<Image>>().len(), MAX_FRAMES);
    }
}
