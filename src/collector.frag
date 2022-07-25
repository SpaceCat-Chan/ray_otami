#version 440 core

layout(binding = 0) buffer colors_buf {
    vec4 colors[];
};
layout(binding = 1) buffer accumulate_buf {
    vec4 accumulate[];
};
layout(binding = 2) uniform render_count_buf {
    uint render_count;
    uint frame_width;
    float exposure;
};

layout(location = 0) in vec2 buffer_position;

layout(location = 0) out vec4 out_color;

void main() {
    uint buffer_index = uint(buffer_position.x) + uint(buffer_position.y) * frame_width;
    vec4 final_color = accumulate[buffer_index] + colors[buffer_index];
    accumulate[buffer_index] = final_color;
    final_color /= render_count;

    // yay, better hdr mapping, now with +1 sliders to adjust!
    out_color = vec4(vec3(1) - exp(-final_color.rgb * exposure), 1);
}
