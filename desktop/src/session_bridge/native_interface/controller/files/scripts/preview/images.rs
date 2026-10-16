//! A bounded set of uploaded frames. Replay reuses textures instead of GPU jobs.
use super::*;
use std::collections::VecDeque;

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_FRAMES: usize = 16;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Key {
    index: usize,
    size: [u32; 2],
    camera: [u32; 2],
    palette: crate::native_viewport::ViewportPalette,
}

impl Key {
    pub fn current(state: &State) -> Self {
        Self {
            index: state.index,
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
        assets: &mut Assets<Image>,
        key: Key,
        image: Image,
    ) -> Result<Handle<Image>, String> {
        let bytes = image.data.as_ref().map_or(0, Vec::len);
        if bytes > MAX_BYTES {
            return Err("Preview image exceeds its 16 MiB texture budget".into());
        }
        if let Some(index) = self.frames.iter().position(|(k, _, _)| *k == key) {
            let (_, old, size) = self.frames.remove(index).unwrap();
            assets.remove(old.id());
            self.bytes -= size;
        }
        while self.frames.len() >= MAX_FRAMES || self.bytes + bytes > MAX_BYTES {
            let (_, old, size) = self.frames.pop_front().unwrap();
            assets.remove(old.id());
            self.bytes -= size;
        }
        let image = assets.add(image);
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
        let mut assets = Assets::<Image>::default();
        let mut cache = Cache::default();
        let state = State::default();
        let mut key = Key::current(&state);
        for index in 0..5 {
            key.index = index;
            let image = Image::new_fill(
                Extent3d {
                    width: 1024,
                    height: 1024,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                &[64, 128, 192, 255],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::MAIN_WORLD,
            );
            cache.store(&mut assets, key, image).unwrap();
        }
        assert_eq!(assets.len(), 4);
        assert_eq!(cache.footprint(), (4, MAX_BYTES));
        key.index = 0;
        assert!(cache.get(key).is_none());
        for image in cache.drain() {
            assets.remove(image.id());
        }
        assert!(assets.is_empty());
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
            cache.store(&mut assets, key, image).unwrap();
        }
        assert_eq!(assets.len(), MAX_FRAMES);
    }
}
