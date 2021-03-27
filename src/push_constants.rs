use std::{mem, slice};

pub trait PushConstant {
    fn size(&self) -> usize;
    fn name(&self) -> &str;
    fn bytes(&self) -> Vec<u8>;
}

#[repr(C)]
struct TypedPushConstant<T> {
    /// The data,
    value: T,
    /// Name as referenced in the JSON file.
    name: String,
}

impl<T> TypedPushConstant<T> {
    fn new(value: T, name: String) -> Self {
        Self { value, name }
    }
}

impl<T> PushConstant for TypedPushConstant<T> {
    fn size(&self) -> usize {
        std::mem::size_of_val(&self.value)
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let vp: *const T = &self.value;
        let bp: *const u8 = vp as *const _;
        let bs: &[u8] =
            unsafe { slice::from_raw_parts(bp, mem::size_of::<T>() / mem::size_of::<u8>()) };
        bytes.extend_from_slice(&bs);
        bytes
    }
}

/// Loads user-specified push constants from a given JSON file on disk.
/// Currently, the following data formats are supported:
///   - f32
///   - f64
///   - u32
///   - u64
///   - i32
///   - i64
///   - bool (bound as u32 in shader)
///
/// The JSON file must follow a specific format, where each constant is given a name followed by the type and value.
/// Example valid format:
/// ```text
/// "push constants": {
///     "antialiasing": ["bool", false],
///     "samples per pixel": ["u32", 4]
/// }
/// ```
/// Returns a vector of [PushConstant] objects that provided everything needed to bind to a shader.
pub fn load_push_constants_from_json(data: &json::JsonValue) -> Vec<Box<dyn PushConstant>> {
    let mut uniforms: Vec<Box<dyn PushConstant>> = Vec::new();
    let uniforms_json = &data["push constants"];
    if !uniforms_json.is_null() {
        let entries = uniforms_json.entries();
        for entry in entries {
            let name = entry.0;
            let mut array_itr = entry.1.members();
            let type_str = array_itr.next().unwrap().as_str().unwrap();
            let value = array_itr.next().unwrap();
            if type_str == "f32" {
                uniforms.push(Box::new(TypedPushConstant::new(
                    value.as_f32().unwrap(),
                    String::from(name),
                )));
            } else if type_str == "f64" {
                uniforms.push(Box::new(TypedPushConstant::new(
                    value.as_f64().unwrap(),
                    String::from(name),
                )));
            } else if type_str == "u32" {
                uniforms.push(Box::new(TypedPushConstant::new(
                    value.as_u32().unwrap(),
                    String::from(name),
                )));
            } else if type_str == "u64" {
                uniforms.push(Box::new(TypedPushConstant::new(
                    value.as_u64().unwrap(),
                    String::from(name),
                )));
            } else if type_str == "i32" {
                uniforms.push(Box::new(TypedPushConstant::new(
                    value.as_i32().unwrap(),
                    String::from(name),
                )));
            } else if type_str == "i64" {
                uniforms.push(Box::new(TypedPushConstant::new(
                    value.as_i64().unwrap(),
                    String::from(name),
                )));
            } else if type_str == "bool" {
                // Note we bind booleans as u32
                uniforms.push(Box::new(TypedPushConstant::new(
                    value.as_bool().unwrap() as u32,
                    String::from(name),
                )));
            }
        }
    }

    uniforms
}
