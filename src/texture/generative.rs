use crate::texture::Texture;
use byteorder::{NativeEndian, WriteBytesExt};
use noise::{utils::*, Perlin};
use std::vec::Vec;
use wgpu::{Extent3d, ImageCopyTexture, Origin3d, TextureDataLayout};

pub enum GenerativeTextureType {
    // Perlin(usize, usize, bool),
    Perlin(usize, usize, bool),
}

pub struct NoiseTexture2D {
    pub noise_handle: Perlin,
    pub texture_handle: wgpu::Texture,
    pub dimension: wgpu::TextureDimension,
}

impl NoiseTexture2D {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        variant: GenerativeTextureType,
    ) -> NoiseTexture2D {
        // Create perlin noise fn.
        let (noise_handle, width, height, seamless) = match variant {
            GenerativeTextureType::Perlin(w, h, s) => (Perlin::new(), w, h, s),
        };
        let noise_map = PlaneMapBuilder::new(&noise_handle)
            .set_size(width, height)
            .set_x_bounds(-5.0, 5.0)
            .set_y_bounds(-5.0, 5.0)
            // .set_x_bounds(-(width as f64) / 50.0, (width as f64) / 50.0)
            // .set_y_bounds(-(height as f64) / 50.0, (height as f64) / 50.0)
            .set_is_seamless(seamless)
            .build();
        // Generate texture.
        let mut noise_data: Vec<u8> =
            Vec::with_capacity(width * height * std::mem::size_of::<f32>());
        for y in 0..height {
            for x in 0..width {
                let noise_val = noise_map.get_value(x, y) as f32;
                noise_data.write_f32::<NativeEndian>(noise_val).unwrap();
            }
        }

        // Create device texture handle.
        let bytes_per_row = width as u32 * 4;
        let tex_desc = wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: width as u32,
                height: height as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            sample_count: 1,
        };
        let texture_handle = device.create_texture(&tex_desc);

        // Copy data over.
        queue.write_texture(
            ImageCopyTexture {
                origin: Origin3d::ZERO,
                mip_level: 0,
                texture: &texture_handle,
            },
            &noise_data,
            TextureDataLayout {
                bytes_per_row,
                offset: 0,
                rows_per_image: height as u32,
            },
            Extent3d {
                width: width as u32,
                height: height as u32,
                depth_or_array_layers: 1,
            },
        );

        NoiseTexture2D {
            noise_handle,
            texture_handle,
            dimension: wgpu::TextureDimension::D2,
        }
    }

    pub fn get_color_sampler(device: &wgpu::Device) -> wgpu::Sampler {
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Perlin Default"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: std::f32::MAX,
            compare: None,
            anisotropy_clamp: None,
        })
    }
}

impl Texture for NoiseTexture2D {
    fn get_view(&self, mip_level: u32) -> wgpu::TextureView {
        self.texture_handle
            .create_view(&wgpu::TextureViewDescriptor {
                label: None,
                format: Some(wgpu::TextureFormat::R32Float),
                dimension: Some(wgpu::TextureViewDimension::D2), // TODO: Match descriptor's dimension.
                aspect: wgpu::TextureAspect::All,
                base_mip_level: mip_level,
                level_count: None,
                base_array_layer: 0,
                array_layer_count: None,
            })
    }

    fn get_handle(&self) -> &wgpu::Texture {
        &self.texture_handle
    }
}
