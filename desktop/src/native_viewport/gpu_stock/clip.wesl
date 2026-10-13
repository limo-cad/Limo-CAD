import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
};
import limo_cad::gpu_stock::common::{removal_active, to_frame, texel_of, inside, cut_height, frame};

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    if removal_active() {
        let p = to_frame(in.world_position.xyz);
        let t = texel_of(p.xy);
        if inside(t) && p.z > cut_height(t) + frame.y_axis.w {
            discard;
        }
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
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
