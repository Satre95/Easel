#version 450

// uniform sampler2D SPIRV_Cross_Combinedtexture_0sampler_0;

layout(location = 0) in vec2 tex_coords;
layout(location = 0) out vec4 f_color;

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

layout(set = 1, binding = 0) uniform sampler sampler_0;
layout(set = 1, binding = 1) uniform texture2D texture_0;

void convert_linear_to_sRGB_precise(inout float val) { 
    if (val < 0.0031308f) {
        val = val * 12.92f;
    }
    else {
        val = 1.055f * pow(val, 1.f / 2.4f) - 0.055f;
    }
}

void main()
{
    f_color = texture(sampler2D(texture_0, sampler_0), tex_coords);
    convert_linear_to_sRGB_precise(f_color.r);
    convert_linear_to_sRGB_precise(f_color.g);
    convert_linear_to_sRGB_precise(f_color.b);
    convert_linear_to_sRGB_precise(f_color.a);
}