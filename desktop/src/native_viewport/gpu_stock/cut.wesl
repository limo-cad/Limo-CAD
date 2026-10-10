import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
};
import limo_cad::gpu_stock::common::{
    removal_active, to_frame, to_world, texel_of, inside, surface_height, column, frame,
};

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    var out: VertexOutput;
    let t = vec2<i32>(i32(vertex.position.x), i32(vertex.position.y));
    let xy = frame.grid.xy + (vec2<f32>(t) + 0.5) * frame.x_axis.w;
    let world = to_world(vec3<f32>(xy, surface_height(t)));
    out.world_position = vec4<f32>(world, 1.0);
    out.position = position_world_to_clip(world);
    out.world_normal = frame.z_axis.xyz;
    @if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
    return out;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    if !removal_active() {
        discard;
    }
    let p = to_frame(in.world_position.xyz);
    let t = texel_of(p.xy);
    if !inside(t) {
        discard;
    }
    let c = column(t);
    if c.x < c.y || p.z > c.x - frame.y_axis.w || p.z < c.y + frame.y_axis.w {
        discard;
    }
    let texel = frame.x_axis.w;
    let dx = surface_height(t + vec2<i32>(1, 0)) - surface_height(t - vec2<i32>(1, 0));
    let dy = surface_height(t + vec2<i32>(0, 1)) - surface_height(t - vec2<i32>(0, 1));
    let n = normalize(vec3<f32>(-dx / (2.0 * texel), -dy / (2.0 * texel), 1.0));
    var surface = in;
    surface.world_normal = n.x * frame.x_axis.xyz + n.y * frame.y_axis.xyz + n.z * frame.z_axis.xyz;
    var pbr_input = pbr_input_from_standard_material(surface, is_front);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color
    );
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
