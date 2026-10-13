
struct Params {
    rect_min: vec2<u32>,
    rect_size: vec2<u32>,
    grid_min: vec2<f32>,
    texel: f32,
    count: u32,
};
struct Segment {
    a: vec4<f32>,
    b: vec4<f32>,
};
struct Profile {
    radius: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
    heights: array<vec4<f32>, 16>,
};

const UNCUT: f32 = 1.0e9;

@group(0) @binding(0) var field: texture_storage_2d<r32float, read_write>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var<storage, read> segments: array<Segment>;
@group(0) @binding(3) var<storage, read> profiles: array<Profile>;

fn profile_sample(profile: u32, index: u32) -> f32 {
    return profiles[profile].heights[index / 4u][index % 4u];
}

fn cutter_surface(profile: u32, radial: f32) -> f32 {
    let radius = profiles[profile].radius;
    if radial > radius {
        return UNCUT;
    }
    let position = clamp(radial / radius, 0.0, 1.0) * 63.0;
    let index = min(u32(floor(position)), 62u);
    let blend = position - f32(index);
    return mix(profile_sample(profile, index), profile_sample(profile, index + 1u), blend);
}

@compute @workgroup_size(8, 8, 1)
fn clear(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= params.rect_size.x || id.y >= params.rect_size.y {
        return;
    }
    textureStore(field, vec2<i32>(params.rect_min + id.xy), vec4<f32>(UNCUT, 0.0, 0.0, 1.0));
}

@compute @workgroup_size(8, 8, 1)
fn stamp(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= params.rect_size.x || id.y >= params.rect_size.y {
        return;
    }
    let texel = vec2<i32>(params.rect_min + id.xy);
    let q = params.grid_min + (vec2<f32>(params.rect_min + id.xy) + 0.5) * params.texel;
    var height = textureLoad(field, texel).x;
    for (var i = 0u; i < params.count; i++) {
        let segment = segments[i];
        let profile = u32(segment.a.w);
        let radius = profiles[profile].radius;
        let low = min(segment.a.xy, segment.b.xy) - vec2<f32>(radius);
        let high = max(segment.a.xy, segment.b.xy) + vec2<f32>(radius);
        if any(q < low) || any(q > high) {
            continue;
        }
        let delta = segment.b.xyz - segment.a.xyz;
        let length_squared = dot(delta.xy, delta.xy);
        var t = 0.0;
        if length_squared > 1.0e-12 {
            t = clamp(dot(q - segment.a.xy, delta.xy) / length_squared, 0.0, 1.0);
        }
        var lowest = segment.a.z + t * delta.z
            + cutter_surface(profile, length(q - (segment.a.xy + t * delta.xy)));
        if abs(delta.z) > 1.0e-6 {
            for (var k = 0u; k <= 16u; k++) {
                let s = f32(k) / 16.0;
                lowest = min(lowest, segment.a.z + s * delta.z
                    + cutter_surface(profile, length(q - (segment.a.xy + s * delta.xy))));
            }
        }
        height = min(height, lowest);
    }
    textureStore(field, texel, vec4<f32>(height, 0.0, 0.0, 1.0));
}
