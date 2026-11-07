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
    fn compatible(self, other: Self) -> bool {
        self.size == other.size && self.camera == other.camera && self.palette == other.palette
    }
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
    pub fn get_handle(&self, image: &Handle<Image>, target: Key) -> Option<Handle<Image>> {
        self.frames
            .iter()
            .find(|(key, handle, _)| handle == image && key.compatible(target))
            .map(|(_, handle, _)| handle.clone())
    }
    /// Reads for display promote a resident; probes in render() stay immutable.
    pub fn promote(&mut self, image: &Handle<Image>, target: Key) -> Option<Handle<Image>> {
        let index = self
            .frames
            .iter()
            .position(|(key, handle, _)| handle == image && key.compatible(target))?;
        let entry = self.frames.remove(index).unwrap();
        let handle = entry.1.clone();
        self.frames.push_back(entry);
        Some(handle)
    }
    /// A pending GPU result needs the second texture slot. Preserve the current
    /// visible image, including while the camera or palette replacement is pending.
    pub fn reserve(&mut self, world: &mut World, displayed: Option<&Handle<Image>>) {
        self.frames.retain(|(_, image, bytes)| {
            if Some(image) == displayed {
                true
            } else {
                textures::remove(world, image.id());
                self.bytes -= *bytes;
                false
            }
        });
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
    fn image() -> FrameImage {
        FrameImage::Fixture(Image::new_fill(
            Extent3d {
                width: 512,
                height: 512,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[64, 128, 192, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD,
        ))
    }
    #[test]
    fn native_preview_blend_source_is_promoted_and_pending_target_reserves_the_second_slot() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        let mut cache = Cache::default();
        let mut key = Key::current(&State::default());
        let source = cache.store(&mut world, key, image()).unwrap();
        key.index = 2;
        let spare = cache.store(&mut world, key, image()).unwrap();
        key.index = 1;
        assert!(cache.promote(&source, key).is_some());
        let target = cache.store(&mut world, key, image()).unwrap();
        assert!(world.resource::<Assets<Image>>().contains(source.id()));
        assert!(!world.resource::<Assets<Image>>().contains(spare.id()));
        assert_eq!(cache.footprint(), (2, MAX_BYTES));
        cache.reserve(&mut world, Some(&target));
        assert!(!world.resource::<Assets<Image>>().contains(source.id()));
        assert!(world.resource::<Assets<Image>>().contains(target.id()));
        assert_eq!(cache.footprint(), (1, MAX_BYTES / 2));
        key.index = 0;
        cache.store(&mut world, key, image()).unwrap();
        assert_eq!(cache.footprint(), (2, MAX_BYTES));
        let mut incompatible = key;
        incompatible.size[0] += 1;
        assert!(cache.promote(&target, incompatible).is_none());
        incompatible = key;
        incompatible.camera[0] += 1;
        assert!(cache.promote(&target, incompatible).is_none());
        incompatible = key;
        incompatible.palette.body[0] += 0.05;
        assert!(cache.promote(&target, incompatible).is_none());
        assert!(
            cache.promote(&source, key).is_none(),
            "Missing source skips the blend"
        );
    }
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
