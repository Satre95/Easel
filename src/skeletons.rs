/// Source string of the skeleton of a typical Easel shader.
pub static SHADER_SKELETON: &str = r#"
#version 450
#ifndef MY_SHADER_FRAG
#define MY_SHADER_FRAG

layout(set = 0, binding = 0) uniform Uniforms {
    vec4 u_resolution;
    float u_time;
    float u_time_delta;
    uint u_frame_num;
    vec4 u_mouse_info;
};

layout(location = 0) in vec2 tex_coords;
layout(location = 0) out vec4 output_color;

void main() {
    float window_wiper = sin(0.5f * u_time);
    window_wiper *= window_wiper;

    if (tex_coords.x < window_wiper) {
        output_color = vec4(0.f, 0.5f, 0.5f, 1.f);
    } else {
        output_color = vec4(0.5f, 0.f, 1.f, 1.f);
    }
}

#endif

"#;
