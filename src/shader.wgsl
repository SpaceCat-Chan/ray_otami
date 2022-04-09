struct VertOutput {
    [[builtin(position)]] position: vec4<f32>;
};



[[stage(vertex)]]
fn vert_main([[builtin(vertex_index)]] id: u32) -> VertOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0)
    );
    var out: VertOutput;
    out.position = vec4<f32>(positions[id] - vec2<f32>(0.5, 0.3), 0.0, 1.0);
    return out;
}

[[group(0), binding(0)]]
var image_view: texture_2d<f32>;
[[group(0), binding(1)]]
var image_sampler: sampler;

[[stage(fragment)]]
fn frag_main(in: VertOutput) -> [[location(0)]] vec4<f32> {
    return textureSample(image_view, image_sampler, in.position.xy);
}
