// Mask generation shader.

#import bevy_pbr::mesh_types::Mesh
#import bevy_render::view::View

@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var<storage> mesh: array<Mesh>;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) object_origin: vec4<f32>,
}

fn affine3_to_square(affine: mat3x4<f32>) -> mat4x4<f32> {
    return transpose(mat4x4<f32>(
        affine[0],
        affine[1],
        affine[2],
        vec4<f32>(0.0, 0.0, 0.0, 1.0),
    ));
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    out.position = view.view_proj * affine3_to_square(mesh[vertex.instance_index].model) * vec4<f32>(vertex.position, 1.0);
    // use the object's origin, normalized into screenspace, as its identity for now
    out.object_origin = view.view_proj * affine3_to_square(mesh[vertex.instance_index].model) * vec4<f32>(0.0, 0.0, 0.0, 1.0);
    return out;
}

@fragment
fn fragment(fragment: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(abs(fragment.object_origin).x / 10.0, abs(fragment.object_origin).y / 10.0,  abs(fragment.object_origin).z / 10.0, 1.0);
}
