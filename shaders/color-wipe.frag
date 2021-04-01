#version 450

layout(set = 0, binding = 0) uniform Uniforms {
    vec4 u_resolution;
    vec4 u_mouse_info;
    ivec4 u_mouse_button_pressed;
    ivec4 u_date;
    float u_time;
    float u_time_delta;
    uint u_frame_num;
    uint u_num_textures;
};

layout(set = 0, binding = 1) uniform CustomUniforms { bool vertical_wipe; };

layout(location = 0) in vec2 tex_coords;
layout(location = 0) out vec4 f_color;

void main() {
    float window_wiper = sin(0.5 * u_time);
    window_wiper *= window_wiper;

    bool color_condition = (vertical_wipe) ? tex_coords.y < window_wiper
                                           : tex_coords.x < window_wiper;

    if (color_condition) {
        f_color = vec4(0.f, 0.5f, 0.5f, 1.f);
    } else {
        f_color = vec4(0.5f, 0.f, 1.f, 1.f);
    }
}
