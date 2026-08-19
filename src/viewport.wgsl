struct Uniforms {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    clip_plane: vec4<f32>,
    clip_enabled: f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
}

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) world_pos: vec3<f32>,
}

fn clipped(world_pos: vec3<f32>) -> bool {
    return u.clip_enabled > 0.5 && dot(world_pos, u.clip_plane.xyz) + u.clip_plane.w < 0.0;
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip_position = u.view_proj * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    out.normal = in.normal;
    out.world_pos = in.position;
    return out;
}

@vertex
fn vs_line(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip_position = u.view_proj * vec4<f32>(in.position, 1.0);
    // wgpu forbids depth bias on LineList; pull slightly toward the camera instead.
    out.clip_position.z -= 0.0005 * out.clip_position.w;
    out.color = in.color;
    out.normal = in.normal;
    out.world_pos = in.position;
    return out;
}

@fragment
fn fs_shaded(in: VsOut) -> @location(0) vec4<f32> {
    if clipped(in.world_pos) {
        discard;
    }
    let n = normalize(in.normal);
    let l = normalize(u.light_dir.xyz);
    let shade = 0.45 + 0.55 * max(dot(n, l), 0.0);
    return vec4<f32>(in.color.rgb * shade, in.color.a);
}

@fragment
fn fs_flat(in: VsOut) -> @location(0) vec4<f32> {
    if clipped(in.world_pos) {
        discard;
    }
    return in.color;
}
