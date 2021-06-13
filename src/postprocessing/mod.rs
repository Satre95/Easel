use std::num::NonZeroU64;

use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingResource, BlendState, BufferBinding, LoadOp, Operations, PipelineLayoutDescriptor,
    RenderPassDescriptor, RenderPipelineDescriptor,
};

pub enum PipelineType {
    Render,
    Painting,
    Movie,
}

/// A struct representing a post-processing shader to run after main fragment shader has finished.
pub struct PostProcess {
    render_pipeline: wgpu::RenderPipeline,
    painting_pipeline: wgpu::RenderPipeline,
    movie_pipeline: wgpu::RenderPipeline,
    uniforms_bind_group_layout: wgpu::BindGroupLayout,
    painting_bind_group_layout: wgpu::BindGroupLayout,
}

impl PostProcess {
    /// Construct a new object using the provided compiled shader data.
    pub fn new(
        device: &wgpu::Device,
        shader_module: Vec<u8>,
        custom_uniforms_provided: bool,
    ) -> Self {
        // Load shaders
        let vs_module = device.create_shader_module(&wgpu::ShaderModuleDescriptor {
            label: Some("Vertex Shader"),
            source: wgpu::util::make_spirv(crate::canvas::VS_MODULE_BYTES),
            flags: wgpu::ShaderFlags::VALIDATION,
        });
        let fs_module = device.create_shader_module(&wgpu::ShaderModuleDescriptor {
            label: Some("sRGB Fragment Shader"),
            source: wgpu::util::make_spirv(&shader_module),
            flags: wgpu::ShaderFlags::VALIDATION,
        });

        // Create bind group layout and entries
        let num_uniform_bind_group_layout_entries = (custom_uniforms_provided as u32) + 1;
        let mut uniforms_bind_group_layout_entries = vec![];
        for i in 0..num_uniform_bind_group_layout_entries {
            uniforms_bind_group_layout_entries.push(BindGroupLayoutEntry {
                binding: i,
                count: None,
                visibility: wgpu::ShaderStage::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    min_binding_size: None,
                    has_dynamic_offset: false,
                },
            });
        }
        let uniforms_bind_group_layout =
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("Postprocess Uniforms Bind Group Layout"),
                entries: &uniforms_bind_group_layout_entries,
            });

        let painting_bind_group_layout =
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("Postprocess Texture Bind Group Layout"),
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
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                    },
                ],
            });

        // Create render pipeline
        let render_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Postprocess sRGB Pipeline Layout"),
            bind_group_layouts: &[&uniforms_bind_group_layout, &painting_bind_group_layout],
            push_constant_ranges: &[],
        });
        let render_frag_state = wgpu::FragmentState {
            module: &fs_module,
            entry_point: "main",
            targets: &[wgpu::ColorTargetState {
                format: crate::canvas::RENDER_TEXTURE_FORMAT,
                blend: Some(BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrite::ALL,
            }],
        };
        let render_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Postprocess sRGB Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vs_module,
                entry_point: "main",
                buffers: &[],
            },
            fragment: Some(render_frag_state),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                clamp_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
        });
        let painting_frag_state = wgpu::FragmentState {
            module: &fs_module,
            entry_point: "main",
            targets: &[wgpu::ColorTargetState {
                format: crate::canvas::PAINTING_TEXTURE_FORMAT,
                blend: Some(BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrite::ALL,
            }],
        };
        let painting_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Postprocess sRGB Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vs_module,
                entry_point: "main",
                buffers: &[],
            },
            fragment: Some(painting_frag_state),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                clamp_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
        });
        let movie_frag_state = wgpu::FragmentState {
            module: &fs_module,
            entry_point: "main",
            targets: &[wgpu::ColorTargetState {
                format: crate::recording::MOVIE_TEXTURE_FORMAT,
                blend: Some(BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrite::ALL,
            }],
        };
        let movie_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Postprocess sRGB Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vs_module,
                entry_point: "main",
                buffers: &[],
            },
            fragment: Some(movie_frag_state),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                clamp_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
        });
        Self {
            uniforms_bind_group_layout,
            painting_bind_group_layout,
            render_pipeline,
            painting_pipeline,
            movie_pipeline,
        }
    }

    /// Encode this post-processing shader into the provided command encoder.
    /// * `input` - Input texture on which to run post-processing.
    /// * `output` - Output texture to render to.
    /// * `uniforms` - Easel-provided uniforms buffer and buffer size in bytes
    /// * `user_uniforms` - Optional buffer of user-specified uniforms and buffer size in bytes.
    /// * `device` - [wgpu::Device] to use for rendering.
    /// * `encoder` - [wgpu::CommandEncoder] on which to encode this draw call.
    /// * `clear_color` - Color to clear the textures when loaded as render attachments.
    /// * `painting` - Whether this postprocess op is being performed on a painting.
    pub fn post_process(
        &self,
        input: &wgpu::TextureView,
        output: &wgpu::TextureView,
        uniforms: (&wgpu::Buffer, usize),
        user_uniforms: Option<(&wgpu::Buffer, usize)>,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        clear_color: wgpu::Color,
        pipeline_type: PipelineType,
    ) {
        let default_sampler = crate::texture::default_color_sampler(device);

        // Create the bind groups
        let mut bind_groups = vec![];

        {
            // First create the uniforms bind group, including the optional custom uniforms.
            let mut entries = vec![];
            entries.push(BindGroupEntry {
                binding: 0,
                resource: BindingResource::Buffer(BufferBinding {
                    buffer: &uniforms.0,
                    offset: 0,
                    size: NonZeroU64::new(uniforms.1 as u64),
                }),
            });
            if let Some(custom) = user_uniforms {
                entries.push(BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Buffer(BufferBinding {
                        buffer: &custom.0,
                        offset: 0,
                        size: NonZeroU64::new(custom.1 as u64),
                    }),
                });
            }
            bind_groups.push(device.create_bind_group(&BindGroupDescriptor {
                label: Some("Postprocess Uniforms Bind Group"),
                layout: &self.uniforms_bind_group_layout,
                entries: &entries,
            }));
        }
        // Then bind the painting textures bind group.
        bind_groups.push(device.create_bind_group(&BindGroupDescriptor {
            label: Some("Postprocess Painting Texture Bind Group"),
            layout: &self.painting_bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::Sampler(&default_sampler),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(input),
                },
            ],
        }));

        // Encode render commands
        let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[wgpu::RenderPassColorAttachment {
                view: &output,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(clear_color),
                    store: true,
                },
            }],
            depth_stencil_attachment: None,
        });
        for i in 0..bind_groups.len() {
            render_pass.set_bind_group(i as u32, &bind_groups[i], &[]);
        }

        match pipeline_type {
            PipelineType::Render => render_pass.set_pipeline(&self.render_pipeline),
            PipelineType::Painting => render_pass.set_pipeline(&self.painting_pipeline),
            PipelineType::Movie => render_pass.set_pipeline(&self.movie_pipeline),
        }
        render_pass.draw(0..3, 0..1);
    }
}
