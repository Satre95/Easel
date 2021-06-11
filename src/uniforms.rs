use crate::utils::{convert_bytes_to_value, convert_value_to_bytes};
use crate::vector::{IntVector4, Vector4};
use bytemuck::{Pod, Zeroable};
use imgui::ImString;
use log::{debug, error};

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
/// A struct of uniforms provided by Easel and bound to every shader.
pub struct Uniforms {
    /// Viewport resolution (in pixels)
    pub resolution: Vector4,
    /// Current mouse pixel coordinates
    /// xy: current, zw: last position.
    pub mouse_position: Vector4,
    // Whether the mouse button is pressed or not.
    // x: left, y: right, z: middle mouse button, w: other
    pub mouse_button: IntVector4,
    /// Year, month, day, w is unused.
    pub date: IntVector4,
    /// Elapsed time since program start, in seconds.
    pub time: f32,
    /// Time since last frame, in seconds
    pub time_delta: f32,
    /// Shader playback frame
    pub frame_num: u32,
    /// Number of textures bound.
    pub num_textures: u32,
}

impl Uniforms {
    pub fn new() -> Uniforms {
        debug!(
            "Uniforms struct is {} bytes",
            std::mem::size_of::<Uniforms>()
        );

        Uniforms {
            resolution: Vector4::zero(),
            time: 0.0,
            time_delta: 0.0,
            frame_num: 0,
            mouse_position: Vector4::zero(),
            mouse_button: IntVector4::zero(),
            num_textures: 0,
            date: IntVector4::zero(),
        }
    }
}
#[derive(Clone, Copy)]
pub enum UserUniformType {
    Float32,
    Float64,
    UInt32,
    UInt64,
    Int32,
    Int64,
    Bool,
}

#[repr(C)]
pub struct UserUniform {
    pub bytes: Vec<u8>,
    pub name: String,
    pub inherent_type: UserUniformType,
}

impl UserUniform {
    pub fn get_value<T: Copy>(&self) -> Result<T, &str> {
        convert_bytes_to_value(&self.bytes)
    }
}

impl Clone for UserUniform {
    fn clone(&self) -> Self {
        UserUniform {
            bytes: self.bytes.clone(),
            name: self.name.clone(),
            inherent_type: self.inherent_type,
        }
    }
}

/// Loads user-specified uniforms from a given JSON file on disk.
/// Currently, the following data formats are supported:
///   - f32
///   - f64
///   - u32
///   - u64
///   - i32
///   - i64
///   - bool (bound as u32 in shader)
///
/// The JSON file must follow a specific format, where each uniform is given a name followed by the type and value.
/// Example valid format:
/// ```text
/// "uniforms": {
///     "dynamic": ["bool", false],
///     "ground_truth": ["f32", 4.0]
/// }
/// ```
/// Returns a vector of [UserUniform] objects that provided everything needed to bind to a shader.
pub fn load_uniforms_from_json(data: &json::JsonValue) -> Vec<UserUniform> {
    let mut uniforms: Vec<UserUniform> = Vec::new();
    let uniforms_json = &data["uniforms"];
    if !uniforms_json.is_null() {
        let entries = uniforms_json.entries();
        for entry in entries {
            let name = entry.0;
            let mut array_itr = entry.1.members();
            let type_str = array_itr.next().unwrap().as_str().unwrap();
            let value = array_itr.next().unwrap();
            if type_str == "f32" {
                uniforms.push(UserUniform {
                    bytes: convert_value_to_bytes(value.as_f32().unwrap()),
                    name: String::from(name),
                    inherent_type: UserUniformType::Float32,
                });
            } else if type_str == "f64" {
                uniforms.push(UserUniform {
                    bytes: convert_value_to_bytes(value.as_f64().unwrap()),
                    name: String::from(name),
                    inherent_type: UserUniformType::Float64,
                });
            } else if type_str == "u32" {
                uniforms.push(UserUniform {
                    bytes: convert_value_to_bytes(value.as_u32().unwrap()),
                    name: String::from(name),
                    inherent_type: UserUniformType::UInt32,
                });
            } else if type_str == "u64" {
                uniforms.push(UserUniform {
                    bytes: convert_value_to_bytes(value.as_u64().unwrap()),
                    name: String::from(name),
                    inherent_type: UserUniformType::UInt64,
                });
            } else if type_str == "i32" {
                uniforms.push(UserUniform {
                    bytes: convert_value_to_bytes(value.as_i32().unwrap()),
                    name: String::from(name),
                    inherent_type: UserUniformType::Int32,
                });
            } else if type_str == "i64" {
                uniforms.push(UserUniform {
                    bytes: convert_value_to_bytes(value.as_i64().unwrap()),
                    name: String::from(name),
                    inherent_type: UserUniformType::Int64,
                });
            } else if type_str == "bool" {
                // Note we bind booleans as u32
                uniforms.push(UserUniform {
                    bytes: convert_value_to_bytes(value.as_bool().unwrap()),
                    name: String::from(name),
                    inherent_type: UserUniformType::Bool,
                });
            } else {
                error!("Uniform with invalid type {} found, ignoring.", type_str);
            }
        }
    }

    uniforms
}

/// Builds the UI element for the given uniform and updates it with the latest value.
///
/// * `ui` - Reference to [imgui::Ui] object.
/// * `uniform` - The [UserUniform] object to visualise and update.
pub fn update_user_uniform_ui(ui: &imgui::Ui, uniform: &mut UserUniform) {
    match uniform.inherent_type {
        // 32 bit types
        UserUniformType::Float32 => {
            let mut value = uniform.get_value::<f32>().unwrap();
            ui.input_float(&ImString::from(uniform.name.clone()), &mut value)
                .build();
            uniform.bytes = convert_value_to_bytes(value);
        }
        UserUniformType::Int32 => {
            let mut value = uniform.get_value::<i32>().unwrap();
            ui.input_int(&ImString::from(uniform.name.clone()), &mut value)
                .build();
            uniform.bytes = convert_value_to_bytes(value);
        }
        UserUniformType::UInt32 => {
            let value = uniform.get_value::<u32>().unwrap();
            let mut value_i32 = value as i32;
            ui.input_int(&ImString::from(uniform.name.clone()), &mut value_i32)
                .build();
            uniform.bytes = convert_value_to_bytes(value);
        }
        // 64 bit types
        UserUniformType::Float64 => {
            let mut value = uniform.get_value::<f32>().unwrap();
            ui.input_float(&ImString::from(uniform.name.clone()), &mut value)
                .build();
            uniform.bytes = convert_value_to_bytes(value as f64);
        }
        UserUniformType::Int64 => {
            let mut value = uniform.get_value::<i32>().unwrap();
            ui.input_int(&ImString::from(uniform.name.clone()), &mut value)
                .build();
            uniform.bytes = convert_value_to_bytes(value as i64);
        }
        UserUniformType::UInt64 => {
            let value = uniform.get_value::<u32>().unwrap();
            let mut value_i32 = value as i32;
            ui.input_int(&ImString::from(uniform.name.clone()), &mut value_i32)
                .build();
            uniform.bytes = convert_value_to_bytes(value_i32 as u64);
        }
        // Bool is a special case
        UserUniformType::Bool => {
            let value = uniform.get_value::<u32>().unwrap();
            let mut value_bool = value != 0;
            ui.checkbox(&ImString::from(uniform.name.clone()), &mut value_bool);
            uniform.bytes = convert_value_to_bytes(value_bool as u32);
        }
    }
}
