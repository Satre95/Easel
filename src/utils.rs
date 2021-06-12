use crate::vector::UIntVector2;
use byteorder::{NativeEndian, WriteBytesExt};
use futures::executor::block_on;
use half::prelude::*;
use image::ImageEncoder;
use image::{codecs::png::PngEncoder, tiff::TiffEncoder};
use log::info;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};
use std::vec::Vec;
use wgpu::{BindGroupLayoutDescriptor, BindGroupLayoutEntry, BlendState};

/// Private helper method to compile text shader using shaderc library.
fn load_shader_source(
    shader_source: &str,
    shader_kind: shaderc::ShaderKind,
    input_filename: &str,
    entrypoint: &str,
    additional_options: Option<&shaderc::CompileOptions>,
) -> Result<shaderc::CompilationArtifact, shaderc::Error> {
    let mut compiler = shaderc::Compiler::new().unwrap();
    compiler.compile_into_spirv(
        shader_source,
        shader_kind,
        input_filename,
        entrypoint,
        additional_options,
    )
}

/// Loads a shader from the given file. Can be either text source or compiled SPIR-V blob.
/// Returns a Result with the binary data of the loaded/compiled shader or an error from ShaderC
/// if unable to compile.
pub fn load_shader(shader_file: &str) -> Result<Vec<u8>, shaderc::Error> {
    // Determine if shader text file provided or SPIR-V binary blob.
    let tokens = shader_file.split(".").collect::<Vec<&str>>();
    assert!(
        *tokens.last().unwrap() == "frag" || *tokens.last().unwrap() == "spv",
        "Invalid shader file/blob provided, must be either \"###.frag\" or \"###.spv\"",
    );

    let fs_spv_data: Vec<u8>;
    let fs_compilation_artifact: shaderc::CompilationArtifact;
    let fpath = Path::new(shader_file);
    let shader_dir = fpath.parent().unwrap();
    if *tokens.last().unwrap() == "frag" {
        let mut shader_compile_options = shaderc::CompileOptions::new().unwrap();
        shader_compile_options.set_include_callback(
            |source_name: &str,
             include_type: shaderc::IncludeType,
             _shader_name: &str,
             _include_depth: usize| {
                // We only support relative includes for now.
                if include_type == shaderc::IncludeType::Standard {
                    return Err("Standard include type (#include <..>) found in shader. Only relative includes (#include \"..\")are currently supported".to_string());
                }
                // Read text data from include file.
                let path_to_file = shader_dir.join(Path::new(source_name));
                let include_src = std::fs::read_to_string(path_to_file.to_str().unwrap()).expect("Unable to find include file.");
                // Return info.
                Ok(shaderc::ResolvedInclude{
                    resolved_name: path_to_file.to_str().unwrap().to_string(),
                    content: include_src
                })
            },
        );
        let fs_src = std::fs::read_to_string(fpath).expect("Unable to find shader");
        fs_compilation_artifact = match load_shader_source(
            &fs_src,
            shaderc::ShaderKind::Fragment,
            shader_file,
            "main",
            Some(&shader_compile_options),
        ) {
            Ok(artifact) => artifact,
            Err(e) => return Result::Err(e),
        };
        fs_spv_data = fs_compilation_artifact.as_binary_u8().to_vec();
    } else {
        fs_spv_data = std::fs::read(fpath).unwrap();
    }
    Result::Ok(fs_spv_data)
}

pub async fn transcode_frame_data_for_movie(
    painting: wgpu::Buffer,
    resolution: UIntVector2,
    pixel_data: &mut Vec<u8>,
) {
    let (width, height) = (resolution.x, resolution.y);
    let slice = painting.slice(0..);
    slice.map_async(wgpu::MapMode::Read).await.unwrap();
    let buf_view = slice.get_mapped_range();
    pixel_data.reserve((width * height * 4) as usize);
    for i in 0..(width * height) {
        // This puts us the beginning of the pixel
        let pixel_idx = (i * 4) as usize;
        // Load each component, excluding alpha
        for component_idx in 0..4 {
            // Load the bytes of each component.
            let component_data = (*buf_view)[pixel_idx + component_idx];
            pixel_data.push(component_data);
        }
    }
}

pub async fn transcode_painting_data(
    painting: wgpu::Buffer,
    resolution: UIntVector2,
    pixel_data: &mut Vec<u8>,
) {
    let (width, height) = (resolution.x, resolution.y);
    let slice = painting.slice(0..);
    slice.map_async(wgpu::MapMode::Read).await.unwrap();
    let buf_view = slice.get_mapped_range();
    pixel_data.reserve((width * height * 4) as usize * std::mem::size_of::<u16>());
    for i in 0..(width * height) {
        // This puts us the beginning of the pixel
        let pixel_idx = (i * 8) as usize;

        let mut bytes_workarea = Vec::with_capacity(2);
        // Load each component
        for component_idx in 0..4 {
            // Load the bytes of each component.
            let component_data = [
                (*buf_view)[pixel_idx + (2 * component_idx) + 0],
                (*buf_view)[pixel_idx + (2 * component_idx) + 1],
            ];

            // Convert bytes to f16.
            let component_f16 = unsafe { std::mem::transmute::<[u8; 2], f16>(component_data) };
            // Convert to 16 bit uint and write.
            let component_u16 = (component_f16.to_f32() * 65535.0) as u16;
            bytes_workarea.clear();
            bytes_workarea
                .write_u16::<NativeEndian>(component_u16)
                .unwrap();
            pixel_data.extend_from_slice(&bytes_workarea);
        }
    }
}

#[allow(dead_code)]
pub fn encode_image_buffer_to_png(
    pixel_data: &Vec<u8>,
    resolution: UIntVector2,
    output_file: File,
) {
    let encoder = PngEncoder::new(output_file);
    encoder
        .encode(
            pixel_data,
            resolution.x,
            resolution.y,
            image::ColorType::Rgba8,
        )
        .unwrap();
}

/// An enum used by the [AsyncTiffWriter] class to signify a write operation has finished.
pub enum WriteFinished {
    Finished,
}

/// A struct used to write a painting to disk after rendering.
pub struct AsyncTiffWriter {}

impl AsyncTiffWriter {
    /// Private helper method called by [AsyncTiffWriter::write]
    async fn write_painting_to_disk(
        painting: wgpu::Buffer,
        resolution: UIntVector2,
        filename: &str,
        _open_external_app: bool,
    ) {
        let width = resolution.x;
        let height = resolution.y;
        let mut pixel_data = Vec::<u8>::new();
        transcode_painting_data(painting, resolution, &mut pixel_data).await;

        {
            let file = File::create(Path::new(filename)).unwrap();
            let buf_writer = BufWriter::new(file);
            TiffEncoder::new(buf_writer)
                .write_image(&pixel_data, width, height, image::ColorType::Rgba16)
                .unwrap();
        }
        // Once writing has finished, open in external app if specified.
        #[cfg(target_os = "macos")]
        if _open_external_app {
            std::process::Command::new("open")
                .arg(filename)
                .spawn()
                .expect("Error launching external app to display painting.");
        }
    }

    /// Given a painting present in GPU memory, copy to CPU, construct a TIFF painting and write to disk.
    /// Paintings are written with uncompressed 16-bit uint TIFF encoding.
    /// **Note:** This function launches an async task and returns immediately.
    /// Use the returned [std::sync::mpsc::Receiver] object which can be used to poll for status updates.
    /// * `painting` - WGPU buffer holding the image data.
    /// * `resolution` - The width and height of the image.
    /// * `filename` - File will be written relative to working directory and with .tiff extension.
    /// * `open_external_app` - Optionally launch external program to view the image. Only supported on macOS and Windows.
    pub fn write(
        buffer: wgpu::Buffer,
        resolution: UIntVector2,
        filename: String,
        open_external_app: bool,
    ) -> Receiver<WriteFinished> {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            block_on(AsyncTiffWriter::write_painting_to_disk(
                buffer,
                resolution,
                &filename,
                open_external_app,
            ));
            info!("Wrote painting {} to disk", filename);
            tx.send(WriteFinished::Finished).unwrap();
        });
        rx
    }
}

/// Convenience method for constructing render and painting pipelines.
pub fn create_pipelines(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    vs_module: &wgpu::ShaderModule,
    fs_module: &wgpu::ShaderModule,
    texture_formats: (
        wgpu::TextureFormat,
        wgpu::TextureFormat,
        wgpu::TextureFormat,
    ),
) -> (
    wgpu::RenderPipeline,
    wgpu::RenderPipeline,
    wgpu::RenderPipeline,
) {
    let vertex_state = wgpu::VertexState {
        module: &vs_module,
        entry_point: "main",
        buffers: &[],
    };
    let primitive_state = wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        cull_mode: None,
        polygon_mode: wgpu::PolygonMode::Fill,
        conservative: true,
        ..Default::default()
    };
    let multisample_state = wgpu::MultisampleState {
        count: 1,
        mask: !0,
        alpha_to_coverage_enabled: false,
    };
    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Canvas Pipeline"),
        layout: Some(&layout),
        vertex: vertex_state.clone(),
        fragment: Some(wgpu::FragmentState {
            module: &fs_module,
            entry_point: "main",
            targets: &[wgpu::ColorTargetState {
                format: texture_formats.0,
                blend: Some(BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrite::ALL,
            }],
        }),
        primitive: primitive_state.clone(),
        depth_stencil: None,
        multisample: multisample_state.clone(),
    });

    let painting_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Painting Pipeline"),
        layout: Some(&layout),
        vertex: vertex_state.clone(),
        fragment: Some(wgpu::FragmentState {
            module: &fs_module,
            entry_point: "main",
            targets: &[wgpu::ColorTargetState {
                format: texture_formats.1,
                blend: Some(BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrite::ALL,
            }],
        }),
        primitive: primitive_state.clone(),
        depth_stencil: None,
        multisample: multisample_state.clone(),
    });

    let movie_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Movie Pipeline"),
        layout: Some(&layout),
        vertex: vertex_state.clone(),
        fragment: Some(wgpu::FragmentState {
            module: &fs_module,
            entry_point: "main",
            targets: &[wgpu::ColorTargetState {
                format: texture_formats.2,
                blend: Some(BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrite::ALL,
            }],
        }),
        primitive: primitive_state,
        depth_stencil: None,
        multisample: multisample_state.clone(),
    });

    (render_pipeline, painting_pipeline, movie_pipeline)
}

static RENDER_TO_SWAP_CHAIN_TEX_SHADER_BYTES: &[u8] =
    include_bytes!("../shaders/render-postprocess-to-swapchain.spv");
pub fn create_swap_chain_pipeline(
    device: &wgpu::Device,
    vs_module: &wgpu::ShaderModule,
    sc_tex_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Swap Chain Pipeline Layout"),
        push_constant_ranges: &[],
        bind_group_layouts: &[
            &device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: None,
                entries: &[
                    BindGroupLayoutEntry {
                        binding: 0,
                        count: None,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::Sampler {
                            filtering: true,
                            comparison: false,
                        },
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        count: None,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                    },
                ],
            }),
        ],
    });

    let vertex_state = wgpu::VertexState {
        module: &vs_module,
        entry_point: "main",
        buffers: &[],
    };
    let primitive_state = wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        strip_index_format: None,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: None,
        polygon_mode: wgpu::PolygonMode::Fill,
        ..Default::default()
    };
    let multisample_state = wgpu::MultisampleState {
        count: 1,
        mask: !0,
        alpha_to_coverage_enabled: false,
    };

    let fs_module = device.create_shader_module(&wgpu::ShaderModuleDescriptor {
        label: Some("Painting Fragment Shader"),
        source: wgpu::util::make_spirv(RENDER_TO_SWAP_CHAIN_TEX_SHADER_BYTES),
        flags: wgpu::ShaderFlags::VALIDATION,
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Swap Chain Pipeline"),
        layout: Some(&layout),
        vertex: vertex_state,
        fragment: Some(wgpu::FragmentState {
            module: &fs_module,
            entry_point: "main",
            targets: &[wgpu::ColorTargetState {
                format: sc_tex_format,
                blend: Some(BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrite::ALL,
            }],
        }),
        primitive: primitive_state,
        depth_stencil: None,
        multisample: multisample_state,
    });

    pipeline
}

pub fn convert_bytes_to_value<'a, T: Copy>(bytes: &'a [u8]) -> Result<T, &str> {
    if bytes.len() != std::mem::size_of::<T>() {
        return Err("Amount of bytes in slice incorrect for size of given type.");
    }

    let bp: *const u8 = bytes.as_ptr();
    let vp: *const T = bp as *const _;
    let value = unsafe { *vp };
    Ok(value)
}

pub fn convert_value_to_bytes<'a, T>(value: T) -> Vec<u8> {
    let mut bytes = Vec::new();
    let vp: *const T = &value;
    let bp: *const u8 = vp as *const _;
    let bs: &[u8] = unsafe { std::slice::from_raw_parts(bp, std::mem::size_of::<T>()) };
    bytes.extend_from_slice(&bs);
    bytes
}
