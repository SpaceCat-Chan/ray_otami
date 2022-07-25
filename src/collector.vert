#version 440 core

layout(location = 0) in vec3 coord;
layout(location = 1) in vec2 in_buffer_position;

layout(location = 0) out vec2 buffer_position;

void main() {
    gl_Position = vec4(coord, 0.5);
    buffer_position = in_buffer_position;
}
