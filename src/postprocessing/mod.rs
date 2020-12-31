use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingResource, InputStepMode, LoadOp, Operations, PipelineLayoutDescriptor,
    RenderPassDescriptor, RenderPipelineDescriptor, VertexBufferDescriptor,
};

/// A struct representing a post-processing shader to run after main fragment shader has finished.
pub struct PostProcess {
    render_pipeline: wgpu::RenderPipeline,
    painting_pipeline: wgpu::RenderPipeline,
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
        let vs_module =
            device.create_shader_module(wgpu::util::make_spirv(crate::canvas::VS_MODULE_BYTES));
        let fs_module = device.create_shader_module(wgpu::util::make_spirv(&shader_module));

        // Create bind group layout and entries
        let num_uniform_bind_group_layout_entries = (custom_uniforms_provided as u32) + 1;
        let mut uniforms_bind_group_layout_entries = vec![];
        for i in 0..num_uniform_bind_group_layout_entries {
            uniforms_bind_group_layout_entries.push(BindGroupLayoutEntry {
                binding: i,
                count: None,
                visibility: wgpu::ShaderStage::FRAGMENT,
                ty: wgpu::BindingType::UniformBuffer {
                    min_binding_size: None,
                    dynamic: false,
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
                        ty: wgpu::BindingType::Sampler { comparison: false },
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        count: None,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::SampledTexture {
                            component_type: wgpu::TextureComponentType::Float,
                            multisampled: true,
                            dimension: wgpu::TextureViewDimension::D2,
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
        let render_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Postprocess sRGB Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex_stage: wgpu::ProgrammableStageDescriptor {
                module: &vs_module,
                entry_point: "main", // 1.
            },
            fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
                // 2.
                module: &fs_module,
                entry_point: "main",
            }),
            rasterization_state: Some(wgpu::RasterizationStateDescriptor {
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: wgpu::CullMode::None,
                depth_bias: 0,
                depth_bias_slope_scale: 0.0,
                depth_bias_clamp: 0.0,
                clamp_depth: false,
            }),
            color_states: &[wgpu::ColorStateDescriptor {
                format: crate::canvas::RENDER_TEXTURE_FORMAT,
                color_blend: wgpu::BlendDescriptor::REPLACE,
                alpha_blend: wgpu::BlendDescriptor::REPLACE,
                write_mask: wgpu::ColorWrite::ALL,
            }],
            primitive_topology: wgpu::PrimitiveTopology::TriangleList, // 1.
            depth_stencil_state: None,                                 // 2.
            vertex_state: wgpu::VertexStateDescriptor {
                index_format: wgpu::IndexFormat::Uint32, // 3.
                vertex_buffers: &[VertexBufferDescriptor {
                    attributes: &[],
                    step_mode: InputStepMode::Vertex,
                    stride: 0,
                }], // 4.
            },
            sample_count: 1,                  // 5.
            sample_mask: !0,                  // 6.
            alpha_to_coverage_enabled: false, // 7.
        });

        let painting_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Postprocess sRGB Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex_stage: wgpu::ProgrammableStageDescriptor {
                module: &vs_module,
                entry_point: "main", // 1.
            },
            fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
                // 2.
                module: &fs_module,
                entry_point: "main",
            }),
            rasterization_state: Some(wgpu::RasterizationStateDescriptor {
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: wgpu::CullMode::None,
                depth_bias: 0,
                depth_bias_slope_scale: 0.0,
                depth_bias_clamp: 0.0,
                clamp_depth: false,
            }),
            color_states: &[wgpu::ColorStateDescriptor {
                format: crate::canvas::PAINTING_TEXTURE_FORMAT,
                color_blend: wgpu::BlendDescriptor::REPLACE,
                alpha_blend: wgpu::BlendDescriptor::REPLACE,
                write_mask: wgpu::ColorWrite::ALL,
            }],
            primitive_topology: wgpu::PrimitiveTopology::TriangleList, // 1.
            depth_stencil_state: None,                                 // 2.
            vertex_state: wgpu::VertexStateDescriptor {
                index_format: wgpu::IndexFormat::Uint32, // 3.
                vertex_buffers: &[VertexBufferDescriptor {
                    attributes: &[],
                    step_mode: InputStepMode::Vertex,
                    stride: 0,
                }], // 4.
            },
            sample_count: 1,                  // 5.
            sample_mask: !0,                  // 6.
            alpha_to_coverage_enabled: false, // 7.
        });
        Self {
            uniforms_bind_group_layout,
            painting_bind_group_layout,
            render_pipeline,
            painting_pipeline,
        }
    }

    /// Encode this post-processing shader into the provided command encoder.
    /// * `input` - Input texture on which to run post-processing.
    /// * `output` - Output texture to render to.
    /// * `uniforms` - Otium-provided uniforms buffer and buffer size in bytes
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
        painting: bool,
    ) {
        let default_sampler = crate::texture::default_color_sampler(device);

        // Create the bind groups
        let mut bind_groups = vec![];

        {
            // First create the uniforms bind group, including the optional custom uniforms.
            let mut entries = vec![];
            entries.push(BindGroupEntry {
                binding: 0,
                resource: BindingResource::Buffer(uniforms.0.slice(0..(uniforms.1 as u64))),
            });
            if let Some(custom) = user_uniforms {
                entries.push(BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Buffer(custom.0.slice(0..(custom.1 as u64))),
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
            color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                attachment: &output,
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
        if painting {
            render_pass.set_pipeline(&self.painting_pipeline);
        } else {
            render_pass.set_pipeline(&self.render_pipeline);
        }
        render_pass.draw(0..3, 0..1);
    }
}
