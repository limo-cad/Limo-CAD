struct FieldFrame {
    origin: vec4<f32>,
    x_axis: vec4<f32>,
    y_axis: vec4<f32>,
    z_axis: vec4<f32>,
    grid: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> frame: FieldFrame;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var field: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var columns: texture_2d<f32>;

fn removal_active() -> bool {
    return frame.origin.w > 0.5;
}

fn to_frame(world: vec3<f32>) -> vec3<f32> {
    let d = world - frame.origin.xyz;
    return vec3<f32>(dot(d, frame.x_axis.xyz), dot(d, frame.y_axis.xyz), dot(d, frame.z_axis.xyz));
}

fn to_world(p: vec3<f32>) -> vec3<f32> {
    return frame.origin.xyz + p.x * frame.x_axis.xyz + p.y * frame.y_axis.xyz + p.z * frame.z_axis.xyz;
}

fn texel_of(p: vec2<f32>) -> vec2<i32> {
    return vec2<i32>(floor((p - frame.grid.xy) / frame.x_axis.w));
}

fn inside(t: vec2<i32>) -> bool {
    return all(t >= vec2<i32>(0)) && t.x < i32(frame.grid.z) && t.y < i32(frame.grid.w);
}

fn clamp_texel(t: vec2<i32>) -> vec2<i32> {
    return clamp(t, vec2<i32>(0), vec2<i32>(i32(frame.grid.z) - 1, i32(frame.grid.w) - 1));
}

fn cut_height(t: vec2<i32>) -> f32 {
    return textureLoad(field, t, 0).x;
}
fn surface_height(t: vec2<i32>) -> f32 {
    return min(cut_height(clamp_texel(t)), frame.z_axis.w);
}

fn column(t: vec2<i32>) -> vec2<f32> {
    return textureLoad(columns, t, 0).xy;
}
