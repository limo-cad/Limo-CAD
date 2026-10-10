//! Flat, translucent origin planes do not need the lit body material's shader.

use bevy::{
    asset as bevy_asset,
    asset::uuid_handle,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render as bevy_render,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::{Shader, ShaderRef},
};

const SHADER: Handle<Shader> = uuid_handle!("9fe6726b-94c6-4b98-bd10-bdc821439f82");

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(super) struct ReferencePlaneMaterial {
    #[uniform(0)]
    pub color: LinearRgba,
}

impl Material for ReferencePlaneMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

pub(super) struct ReferencePlanePlugin;

impl Plugin for ReferencePlanePlugin {
    fn build(&self, app: &mut App) {
        let _ = app.world_mut().resource_mut::<Assets<Shader>>().insert(
            SHADER.id(),
            Shader::from_wesl(
                include_str!("reference_planes.wesl"),
                "embedded://limo_cad/reference_planes.wesl",
            ),
        );
        app.add_plugins(MaterialPlugin::<ReferencePlaneMaterial>::default());
    }
}
