//! Import shared-device preview textures without readback or a second upload.
use super::RenderedTexture;
use bevy::{
    asset::{AssetId, RenderAssetUsages},
    prelude::*,
    render::{
        render_asset::{prepare_assets, RenderAssets},
        texture::GpuImage,
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
    },
};
use std::collections::{HashMap, HashSet};

#[derive(Resource, Default)]
pub(crate) struct Registry(HashMap<AssetId<Image>, RenderedTexture>);

#[cfg(test)]
pub(crate) fn retained_count(world: &World) -> usize {
    world
        .get_resource::<Registry>()
        .map_or(0, |registry| registry.0.len())
}

/// These handles keep worker pool slots immutable until extraction retires them.
#[derive(Resource, Default)]
struct Extracted(HashMap<AssetId<Image>, RenderedTexture>);

#[derive(Resource, Default)]
struct Installed(HashSet<AssetId<Image>>);

#[cfg(test)]
pub(crate) fn imported_count(world: &World) -> usize {
    world
        .get_resource::<Installed>()
        .map_or(0, |installed| installed.0.len())
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<Registry>();
    let render = app.sub_app_mut(RenderApp);
    render
        .init_resource::<Installed>()
        .add_systems(ExtractSchedule, extract)
        .add_systems(
            Render,
            prepare
                .in_set(RenderSystems::PrepareAssets)
                .after(prepare_assets::<GpuImage>),
        );
}

pub(crate) fn insert(world: &mut World, texture: RenderedTexture) -> Handle<Image> {
    let descriptor = &texture.image.texture_descriptor;
    // MAIN_WORLD keeps layout metadata available without allocating/uploading pixels.
    let image = Image::new_uninit(
        descriptor.size,
        descriptor.dimension,
        descriptor.format,
        RenderAssetUsages::MAIN_WORLD,
    );
    let handle = world.resource_mut::<Assets<Image>>().add(image);
    world.init_resource::<Registry>();
    world
        .resource_mut::<Registry>()
        .0
        .insert(handle.id(), texture);
    handle
}

pub(crate) fn remove(world: &mut World, id: AssetId<Image>) {
    if let Some(mut registry) = world.get_resource_mut::<Registry>() {
        registry.0.remove(&id);
    }
    if let Some(mut images) = world.get_resource_mut::<Assets<Image>>() {
        images.remove(id);
    }
}

fn extract(mut commands: Commands, registry: Extract<Res<Registry>>) {
    // Always replace, including the empty snapshot after close. The render world
    // retains the prior leases until its previous render has submitted commands.
    commands.insert_resource(Extracted(
        registry
            .0
            .iter()
            .map(|(id, frame)| {
                (
                    *id,
                    RenderedTexture {
                        image: frame.image.clone(),
                        lease: frame.lease.clone(),
                    },
                )
            })
            .collect(),
    ));
}

fn prepare(
    textures: Option<Res<Extracted>>,
    mut installed: ResMut<Installed>,
    mut images: ResMut<RenderAssets<GpuImage>>,
    mut bindings: ResMut<bevy::ui_render::ImageNodeBindGroups>,
) {
    let Some(textures) = textures else {
        return;
    };
    installed.0.retain(|id| {
        if textures.0.contains_key(id) {
            return true;
        }
        images.remove(*id);
        bindings.values.remove(id);
        false
    });
    for (id, texture) in &textures.0 {
        if images
            .get(*id)
            .is_none_or(|image| image.texture_view.id() != texture.image.texture_view.id())
        {
            images.insert(*id, texture.image.clone());
            bindings.values.remove(id);
        }
        installed.0.insert(*id);
    }
}
