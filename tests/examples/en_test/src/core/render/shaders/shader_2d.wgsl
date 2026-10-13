struct Camera { vp: mat4x4<f32>, };
@group(0) @binding(0) var<uniform> camera: Camera;

struct VSOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) vpos: vec2<f32>,
    @location(1) vuv: vec2<f32>,
    @location(2) i_transform: vec4<f32>,
    @location(3) i_color: vec4<f32>,
    @location(4) i_rot: vec2<f32>,
) -> VSOut {
    var out: VSOut;
    let scaled = vpos * i_transform.zw;
    let c = i_rot.x;
    let s = i_rot.y;
    let rotated = vec2<f32>(
        scaled.x * c - scaled.y * s,
        scaled.x * s + scaled.y * c
    );
    let world = rotated + i_transform.xy;
    out.pos = camera.vp * vec4<f32>(world, 0.0, 1.0);
    out.uv = vuv;
    out.color = i_color;
    return out;
}

@group(1) @binding(0) var t_diffuse: texture_2d<f32>;
@group(1) @binding(1) var s_diffuse: sampler;

@fragment
fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
    return textureSample(t_diffuse, s_diffuse, in.uv) * in.color;
}
