// Made by Claude - test kernel: c = (a + b) * scale
@group(0) @binding(0) var<storage, read_write> a: array<f32>;
@group(0) @binding(1) var<storage, read_write> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;
@group(0) @binding(3) var<storage, read> p: array<f32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x + gid.y * 16776960u;
    if (i >= arrayLength(&c)) { return; }
    c[i] = (a[i] + b[i]) * p[0];
}
// Made by Claude
