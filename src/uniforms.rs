use std::any::TypeId;

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

/// Trait to represent uniform of any type loaded from file.
pub trait UserUniform {
    /// Size in bytes of the data to be bound to the shader.
    fn size(&self) -> usize;
    /// Name as referenced in the JSON file.
    fn name(&self) -> String;
    /// The byte data to be bound to the shader.
    fn bytes(&self) -> Vec<u8>;
    /// The Rust [std::any::TypeId] associated with this uniform.
    fn type_id(&self) -> std::any::TypeId;
    /// Get a copy of this underlying uniform on the heap.
    fn copy(&self) -> Box<dyn UserUniform>;
    /// Set the given bytes for this uniform.
    fn set(&mut self, bytes: &[u8]);
}

#[repr(C)]
/// Internal struct used for loading uniforms from JSON.
pub struct TypedUniform<RealType, BoundType> {
    /// The data
    value: BoundType,
    /// Name as referenced in the JSON file.
    name: String,
    phantom: std::marker::PhantomData<RealType>,
}

impl<RealType: 'static, BoundType: 'static> TypedUniform<RealType, BoundType> {
    fn new(value: BoundType, name: String) -> Self {
        Self {
            value,
            name,
            phantom: std::marker::PhantomData,
        }
    }
}

impl<RealType: 'static, BoundType: 'static + Clone + Copy> UserUniform
    for TypedUniform<RealType, BoundType>
{
    fn size(&self) -> usize {
        std::mem::size_of_val(&self.value)
    }

    fn name(&self) -> String {
        self.name.to_string()
    }

    fn bytes(&self) -> Vec<u8> {
        convert_value_to_bytes(self.value.clone())
    }

    fn type_id(&self) -> std::any::TypeId {
        return std::any::TypeId::of::<TypedUniform<RealType, BoundType>>();
    }

    fn copy(&self) -> Box<dyn UserUniform> {
        Box::new(TypedUniform::<RealType, BoundType>::new(
            self.value.clone(),
            self.name.clone(),
        ))
    }

    fn set(&mut self, bytes: &[u8]) {
        self.value = convert_bytes_to_value(bytes).unwrap();
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
pub fn load_uniforms_from_json(data: &json::JsonValue) -> Vec<Box<dyn UserUniform>> {
    let mut uniforms: Vec<Box<dyn UserUniform>> = Vec::new();
    let uniforms_json = &data["uniforms"];
    if !uniforms_json.is_null() {
        let entries = uniforms_json.entries();
        for entry in entries {
            let name = entry.0;
            let mut array_itr = entry.1.members();
            let type_str = array_itr.next().unwrap().as_str().unwrap();
            let value = array_itr.next().unwrap();
            if type_str == "f32" {
                uniforms.push(Box::new(TypedUniform::<f32, f32>::new(
                    value.as_f32().unwrap(),
                    String::from(name),
                )));
            } else if type_str == "f64" {
                uniforms.push(Box::new(TypedUniform::<f64, f64>::new(
                    value.as_f64().unwrap(),
                    String::from(name),
                )));
            } else if type_str == "u32" {
                uniforms.push(Box::new(TypedUniform::<u32, u32>::new(
                    value.as_u32().unwrap(),
                    String::from(name),
                )));
            } else if type_str == "u64" {
                uniforms.push(Box::new(TypedUniform::<u64, u64>::new(
                    value.as_u64().unwrap(),
                    String::from(name),
                )));
            } else if type_str == "i32" {
                uniforms.push(Box::new(TypedUniform::<i32, i32>::new(
                    value.as_i32().unwrap(),
                    String::from(name),
                )));
            } else if type_str == "i64" {
                uniforms.push(Box::new(TypedUniform::<i64, i64>::new(
                    value.as_i64().unwrap(),
                    String::from(name),
                )));
            } else if type_str == "bool" {
                // Note we bind booleans as u32
                uniforms.push(Box::new(TypedUniform::<bool, u32>::new(
                    value.as_bool().unwrap() as u32,
                    String::from(name),
                )));
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
pub fn update_user_uniform_ui(ui: &imgui::Ui, uniform: &mut Box<dyn UserUniform>) {
    if uniform.type_id() == TypeId::of::<TypedUniform<f32, f32>>() {
        let mut value: f32 = convert_bytes_to_value(&uniform.bytes()).unwrap();
        ui.input_float(&ImString::from(uniform.name()), &mut value)
            .build();
        uniform.set(&convert_value_to_bytes(value));
    } else if uniform.type_id() == TypeId::of::<TypedUniform<i32, i32>>() {
        let mut value: i32 = convert_bytes_to_value(&uniform.bytes()).unwrap();
        ui.input_int(&ImString::from(uniform.name()), &mut value)
            .build();
        uniform.set(&convert_value_to_bytes(value));
    } else if uniform.type_id() == TypeId::of::<TypedUniform<u32, u32>>() {
        let value: u32 = convert_bytes_to_value(&uniform.bytes()).unwrap();
        let mut value_i32 = value as i32;
        ui.input_int(&ImString::from(uniform.name()), &mut value_i32)
            .build();
        uniform.set(&convert_value_to_bytes(value));
    }
    // 64-bit types
    else if uniform.type_id() == TypeId::of::<TypedUniform<f64, f64>>() {
        let mut value: f32 = convert_bytes_to_value(&uniform.bytes()).unwrap();
        ui.input_float(&ImString::from(uniform.name()), &mut value)
            .build();
        uniform.set(&convert_value_to_bytes(value as f64));
    } else if uniform.type_id() == TypeId::of::<TypedUniform<i64, i64>>() {
        let mut value: i32 = convert_bytes_to_value(&uniform.bytes()).unwrap();
        ui.input_int(&ImString::from(uniform.name()), &mut value)
            .build();
        uniform.set(&convert_value_to_bytes(value as i64));
    } else if uniform.type_id() == TypeId::of::<TypedUniform<u64, u64>>() {
        let value: u32 = convert_bytes_to_value(&uniform.bytes()).unwrap();
        let mut value_i32 = value as i32;
        ui.input_int(&ImString::from(uniform.name()), &mut value_i32)
            .build();
        uniform.set(&convert_value_to_bytes(value_i32 as u64));
    // Special case: bool
    } else if uniform.type_id() == TypeId::of::<TypedUniform<bool, u32>>() {
        let value: u32 = convert_bytes_to_value(&uniform.bytes()).unwrap();
        let mut value_bool = value != 0;
        ui.checkbox(&ImString::from(uniform.name()), &mut value_bool);
        uniform.set(&convert_value_to_bytes(value_bool as u32));
    }
}
