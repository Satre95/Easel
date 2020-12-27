use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
/// A simple 4D single-precision floating point vector struct.
pub struct Vector4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
/// A simple 3D single-precision floating point vector struct.
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
/// A simple 2D single-precision floating point vector struct.
pub struct Vector2 {
    pub x: f32,
    pub y: f32,
}

// Fns that can be commonly implemented, trying to keep DRY
macro_rules! impl_vector {
    ($VectorN:ident { $($field:ident),+ }, $n:expr, $constructor:ident) => {
        impl $VectorN {
            #[inline]
            #[allow(dead_code)]
            pub const fn new($($field: f32),+) -> $VectorN {
                $VectorN { $($field: $field),+}
            }

            #[inline]
            #[allow(dead_code)]
            pub const fn zero() -> $VectorN {
                $VectorN { $($field: 0.0),+}
            }
        }

    };
}

impl_vector!(Vector2 { x, y }, 2, vec2);
impl_vector!(Vector3 { x, y, z }, 3, vec3);
impl_vector!(Vector4 { x, y, z, w }, 4, vec4);

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
/// A simple 4D 32-bit integer vector struct.
pub struct IntVector4 {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub w: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
/// A simple 3D integer vector struct.
pub struct IntVector3 {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
/// A simple 2D integer vector struct.
pub struct IntVector2 {
    pub x: i32,
    pub y: i32,
}

// Fns that can be commonly implemented, trying to keep DRY
macro_rules! impl_intvector {
    ($IntVectorN:ident { $($field:ident),+ }, $n:expr, $constructor:ident) => {
        impl $IntVectorN {
            #[inline]
            #[allow(dead_code)]
            pub const fn new($($field: i32),+) -> $IntVectorN {
                $IntVectorN { $($field: $field),+}
            }

            #[inline]
            #[allow(dead_code)]
            pub const fn zero() -> $IntVectorN {
                $IntVectorN { $($field: 0),+}
            }
        }

    };
}

impl_intvector!(IntVector2 { x, y }, 2, intvec2);
impl_intvector!(IntVector3 { x, y, z }, 3, intvec3);
impl_intvector!(IntVector4 { x, y, z, w }, 4, intvec4);
