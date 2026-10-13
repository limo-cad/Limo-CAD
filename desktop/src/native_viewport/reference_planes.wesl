import bevy_pbr::render::{forward_io::{VertexOutput, FragmentOutput}, mesh_view_bindings::view};
@if(TONEMAP_IN_SHADER) import bevy_core_pipeline::tonemapping::{tone_mapping, screen_space_dither};
@if(DEBAND_DITHER) import bevy_render::maths::powsafe;

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> color: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
    out.color = color;
    // Match the unlit body material's color processing without its lighting,
    // texture, shadow, fog and transmission code. Origin planes have no fog.
    @if(TONEMAP_IN_SHADER) {
        out.color = tone_mapping(out.color, view.color_grading);
        @if(DEBAND_DITHER) {
            var rgb = powsafe(out.color.rgb, 1.0 / 2.2);
            rgb += screen_space_dither(in.position.xy);
            out.color = vec4(powsafe(rgb, 2.2), out.color.a);
        }
    }
    return out;
}
