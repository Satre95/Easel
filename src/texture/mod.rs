use image::{DynamicImage, GenericImageView};
use wgpu::{Extent3d, Origin3d, TextureCopyView, TextureDataLayout};

/// Construct a [wgpu::Sampler] object using our defaults.
pub fn default_color_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Default"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        lod_min_clamp: 0.0,
        lod_max_clamp: std::f32::MAX,
        compare: None,
        anisotropy_clamp: None,
        border_color: None,
    })
}

/// Represents an image loaded into a [wgpu::Texture] from a file.
/// Currently, only 2D textures are supported.
pub struct AssetTexture {
    handle: wgpu::Texture,
    pub format: wgpu::TextureFormat,
}

impl AssetTexture {
    /// Construct an [AssetTexture] object from an [image::DynamicImage].
    /// Allocates memory on the GPU device and copies data into it.
    pub fn new_with_image(
        image: &DynamicImage,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> AssetTexture {
        let tex_desc = wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: image.width(),
                height: image.height(),
                depth: 1,
            },
            mip_level_count: 1,
            usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            sample_count: 1,
        };
        let texture = device.create_texture(&tex_desc);

        let image_data = image.as_rgba8().unwrap();
        let bytes_per_row = image.width() as u32 * image::ColorType::Rgba8.bytes_per_pixel() as u32;
        queue.write_texture(
            TextureCopyView {
                origin: Origin3d::ZERO,
                mip_level: 0,
                texture: &texture,
            },
            &image_data,
            TextureDataLayout {
                bytes_per_row,
                offset: 0,
                rows_per_image: image.height(),
            },
            Extent3d {
                width: image.width(),
                height: image.height(),
                depth: 1,
            },
        );

        AssetTexture {
            handle: texture,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
        }
    }

    pub fn get_view(&self, mip_level: u32) -> wgpu::TextureView {
        self.handle.create_view(&wgpu::TextureViewDescriptor {
            label: None,
            format: Some(self.format),
            dimension: Some(wgpu::TextureViewDimension::D2),
            aspect: wgpu::TextureAspect::All,
            base_mip_level: mip_level,
            level_count: None,
            base_array_layer: 0,
            array_layer_count: None,
        })
    }

    // pub fn get_handle(&self) -> &wgpu::Texture {
    //     &self.handle
    // }
}
