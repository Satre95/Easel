use crate::texture::default_color_sampler;
use crate::vector::UIntVector2;
use crate::{
    postprocessing,
    recording::{self, MOVIE_TEXTURE_FORMAT},
};
use wgpu::util::{BufferInitDescriptor, DeviceExt};
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingResource, Extent3d, LoadOp, Operations, Origin3d,
};

use super::message::CanvasMessage;
use super::{Canvas, PAINTING_TEXTURE_FORMAT, RENDER_TEXTURE_FORMAT};
use crate::uniforms::Uniforms;
impl Canvas {
    /// Render the shader on the canvas.
    pub fn render_canvas(&mut self) {
        if self.paused {
            return;
        }
        let frame = match self.swap_chain.get_current_frame() {
            Ok(frame) => frame,
            Err(frame_err) => {
                self.transmitter
                    .send(CanvasMessage::SwapChainFrameError(frame_err))
                    .unwrap();
                return;
            }
        };
        // Create the texture to render to.
        let tex_desc = wgpu::TextureDescriptor {
            size: Extent3d {
                width: self.size.width,
                height: self.size.height,
                depth: 1,
            },
            format: RENDER_TEXTURE_FORMAT,
            usage: wgpu::TextureUsage::RENDER_ATTACHMENT | wgpu::TextureUsage::SAMPLED,
            label: Some("Canvas Render"),
            dimension: wgpu::TextureDimension::D2,
            mip_level_count: 1,
            sample_count: 1,
        };
        let render_tex = self.device.create_texture(&tex_desc);
        let render_tex_view = render_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // First, render using the shader.
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                    attachment: &render_tex_view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(self.clear_color),
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
            });

            for i in 0..self.bind_groups.len() {
                render_pass.set_bind_group(i as u32, &self.bind_groups[i], &[]);
            }
            render_pass.set_pipeline(&self.render_pipeline);
            // Set push constants, if any.
            if let Some(constants) = self.push_constants.as_ref() {
                let mut offset: usize = 0;
                for a_constant in constants {
                    let bytes = a_constant.bytes();
                    render_pass.set_push_constants(
                        wgpu::ShaderStage::FRAGMENT,
                        offset as u32,
                        &bytes,
                    );
                    offset += a_constant.size();
                }
            }
            render_pass.draw(0..3, 0..1);
        }

        // We can't create bind groups with swap chain textures, so have to create another temp tex.
        let postprocessing_tex = self.device.create_texture(&tex_desc);
        let postprocessing_tex_view =
            postprocessing_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // Then render any post-processing effects.
        let mut stage_in = &render_tex_view;
        let mut stage_out = &postprocessing_tex_view;
        for i in 0..self.postprocess_ops.len() {
            let postprocess_op = &self.postprocess_ops[i];
            // If user has provided custom uniforms, pass them to the post-processing stage as well.
            let mut custom_data = None;
            if let Some(custom_buffer) = self.user_uniforms_buffer.as_ref() {
                custom_data = Some((custom_buffer, self.user_uniforms_buffer_size.unwrap()));
            }
            postprocess_op.post_process(
                stage_in,
                stage_out,
                (
                    &self.uniforms_device_buffer,
                    std::mem::size_of_val(&self.uniforms),
                ),
                custom_data,
                &self.device,
                &mut encoder,
                self.clear_color,
                postprocessing::PipelineType::Render,
            );
            // Swap input and output textures handles
            std::mem::swap(&mut stage_in, &mut stage_out);
        }
        // Swap one more time to get final output tex (undo last swap).
        std::mem::swap(&mut stage_in, &mut stage_out);

        // Render back to swap chain texture.
        // Build new specialized bind groups for this render pass.
        let sc_layout = self
            .device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
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
            });
        let sc_bind_group = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("Swap Chain Render Pass Bind Group"),
            layout: &sc_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::Sampler(&default_color_sampler(&self.device)),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(stage_out),
                },
            ],
        });
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                    attachment: &frame.output.view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(self.clear_color),
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
            });

            render_pass.set_bind_group(0, &sc_bind_group, &[]);

            render_pass.set_pipeline(&self.swap_chain_pipeline);
            render_pass.draw(0..3, 0..1);
        }

        let command_buffer = encoder.finish();
        self.queue.submit(Some(command_buffer));

        self.transmitter
            .send(CanvasMessage::RenderPassSubmitted)
            .unwrap();
        self.transmitter.send(CanvasMessage::FrameStep).unwrap();
    }

    /// Similar to [Self::render_canvas()], but renders to a very high bit-depth texture and writes output to file.
    /// **Note:** File is written to disk asynchronously.
    pub fn create_painting(&mut self, resolution: UIntVector2) {
        let painting_tex_desc = wgpu::TextureDescriptor {
            size: Extent3d {
                width: resolution.x as u32,
                height: resolution.y as u32,
                depth: 1,
            },
            format: PAINTING_TEXTURE_FORMAT,
            usage: wgpu::TextureUsage::RENDER_ATTACHMENT
                | wgpu::TextureUsage::COPY_SRC
                | wgpu::TextureUsage::SAMPLED,
            label: Some("Painting"),
            dimension: wgpu::TextureDimension::D2,
            mip_level_count: 1,
            sample_count: 1,
        };

        // Texture to render the painting too.
        let painting = self.device.create_texture(&painting_tex_desc);
        // Create the output texture for post-processing.
        let post_process_tex = self.device.create_texture(&painting_tex_desc);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Painting Encoder"),
            });

        // Modify Uniforms as necessary for painting render
        {
            let mut painting_uniforms = self.uniforms.clone();
            let width_ratio = resolution.x as f32 / self.uniforms.resolution.x as f32;
            let height_ratio = resolution.y as f32 / self.uniforms.resolution.y as f32;
            painting_uniforms.mouse_position.x *= width_ratio;
            painting_uniforms.mouse_position.z *= width_ratio;
            painting_uniforms.mouse_position.y *= height_ratio;
            painting_uniforms.mouse_position.w *= height_ratio;
            painting_uniforms.resolution.x = resolution.x as f32;
            painting_uniforms.resolution.y = resolution.y as f32;

            // Copy uniforms from CPU to staging buffer, then copy from staging buffer to main buf.
            let descriptor = BufferInitDescriptor {
                label: Some("Uniforms Buffer"),
                contents: bytemuck::bytes_of(&painting_uniforms),
                usage: wgpu::BufferUsage::COPY_SRC,
            };
            let staging_buffer = self.device.create_buffer_init(&descriptor);

            encoder.copy_buffer_to_buffer(
                &staging_buffer,
                0,
                &self.uniforms_device_buffer,
                0,
                std::mem::size_of::<Uniforms>() as u64,
            );
        }

        // Buffer to copy texture into after all rendering finishes.
        let buffer_desc = wgpu::BufferDescriptor {
            label: Some("Painting Staging Buffer"),
            usage: wgpu::BufferUsage::COPY_DST | wgpu::BufferUsage::MAP_READ,
            size: ((resolution.x * resolution.y) as usize * std::mem::size_of::<half::f16>() * 4)
                as u64,
            mapped_at_creation: false,
        };
        let buffer = self.device.create_buffer(&buffer_desc);

        let painting_start_time = std::time::Instant::now();
        // First run the pipeline.
        {
            let painting_view = painting.create_view(&wgpu::TextureViewDescriptor::default());
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                    attachment: &painting_view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(self.clear_color),
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
            });

            for i in 0..self.bind_groups.len() {
                render_pass.set_bind_group(i as u32, &self.bind_groups[i], &[]);
            }
            render_pass.set_pipeline(&self.painting_pipeline);
            // Set push constants, if any.
            if let Some(constants) = self.push_constants.as_ref() {
                let mut offset: usize = 0;
                for a_constant in constants {
                    let bytes = a_constant.bytes();
                    render_pass.set_push_constants(
                        wgpu::ShaderStage::FRAGMENT,
                        offset as u32,
                        &bytes,
                    );
                    offset += a_constant.size();
                }
            }
            render_pass.draw(0..3, 0..1);
        }

        // Then run all post-processing steps, in order.
        let mut stage_in = &painting;
        let mut stage_out = &post_process_tex;
        let mut custom_data = None;
        if let Some(custom_buffer) = self.user_uniforms_buffer.as_ref() {
            custom_data = Some((custom_buffer, self.user_uniforms_buffer_size.unwrap()));
        }
        for postprocess_op in &mut self.postprocess_ops {
            let input_view = stage_in.create_view(&wgpu::TextureViewDescriptor::default());
            let output_view = stage_out.create_view(&wgpu::TextureViewDescriptor::default());
            postprocess_op.post_process(
                &input_view,
                &output_view,
                (
                    &self.uniforms_device_buffer,
                    std::mem::size_of_val(&self.uniforms),
                ),
                custom_data,
                &self.device,
                &mut encoder,
                self.clear_color,
                postprocessing::PipelineType::Painting,
            );
            // Swap input and output textures handles
            std::mem::swap(&mut stage_in, &mut stage_out);
        }

        // Run one more post-process op, the sRGB conversion.
        {
            let input_view = stage_in.create_view(&wgpu::TextureViewDescriptor::default());
            let output_view = stage_out.create_view(&wgpu::TextureViewDescriptor::default());
            self.srgb_postprocess.post_process(
                &input_view,
                &output_view,
                (
                    &self.uniforms_device_buffer,
                    std::mem::size_of_val(&self.uniforms),
                ),
                custom_data,
                &self.device,
                &mut encoder,
                self.clear_color,
                postprocessing::PipelineType::Painting,
            );
        }

        // Then encode a copy of the texture to the buffer.
        {
            let tex_copy_view = wgpu::TextureCopyView {
                mip_level: 0,
                origin: Origin3d::ZERO,
                texture: stage_out,
            };
            let buf_copy_view = wgpu::BufferCopyView {
                buffer: &buffer,
                layout: wgpu::TextureDataLayout {
                    bytes_per_row: ((resolution.x * 4) as usize * std::mem::size_of::<half::f16>())
                        as u32,
                    offset: 0,
                    rows_per_image: resolution.y as u32,
                },
            };
            encoder.copy_texture_to_buffer(
                tex_copy_view,
                buf_copy_view,
                Extent3d {
                    width: resolution.x as u32,
                    height: resolution.y as u32,
                    depth: 1,
                },
            );
        }

        let command_buffer = encoder.finish();
        self.queue.submit(Some(command_buffer));

        self.transmitter
            .send(CanvasMessage::PaintingStarted(
                buffer,
                resolution,
                painting_start_time,
            ))
            .unwrap();
    }

    /// Expected to be called immediately after the render() function.
    pub fn post_render(&mut self) {
        // Inform Dashboard of each of our user-provided uniforms.
        for a_uniform in &self.user_uniforms {
            let uni = a_uniform.copy();
            self.transmitter
                .send(CanvasMessage::UniformForGUI(uni))
                .unwrap();
        }
        // Inform our window we have new contents for it to draw.
        self.window.request_redraw();
    }

    /// Called when Dashboard requests a movie render frame.
    pub fn create_movie_frame(&mut self, resolution: UIntVector2) {
        let painting_tex_desc = wgpu::TextureDescriptor {
            size: Extent3d {
                width: resolution.x as u32,
                height: resolution.y as u32,
                depth: 1,
            },
            format: MOVIE_TEXTURE_FORMAT,
            usage: wgpu::TextureUsage::RENDER_ATTACHMENT
                | wgpu::TextureUsage::COPY_SRC
                | wgpu::TextureUsage::SAMPLED,
            label: Some("Painting"),
            dimension: wgpu::TextureDimension::D2,
            mip_level_count: 1,
            sample_count: 1,
        };

        // Texture to render the painting too.
        let painting = self.device.create_texture(&painting_tex_desc);
        // Create the output texture for post-processing.
        let post_process_tex = self.device.create_texture(&painting_tex_desc);

        // Buffer to copy texture into after all rendering finishes.
        let buffer_desc = wgpu::BufferDescriptor {
            label: Some("Painting Staging Buffer"),
            usage: wgpu::BufferUsage::COPY_DST | wgpu::BufferUsage::MAP_READ,
            size: ((resolution.x * resolution.y) as usize * std::mem::size_of::<half::f16>() * 4)
                as u64,
            mapped_at_creation: false,
        };
        let buffer = self.device.create_buffer(&buffer_desc);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Movie Frame Encoder"),
            });

        let painting_start_time = std::time::Instant::now();
        // First run the pipeline.
        {
            let painting_view = painting.create_view(&wgpu::TextureViewDescriptor::default());
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                    attachment: &painting_view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(self.clear_color),
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
            });

            for i in 0..self.bind_groups.len() {
                render_pass.set_bind_group(i as u32, &self.bind_groups[i], &[]);
            }
            render_pass.set_pipeline(&self.movie_pipeline);
            // Set push constants, if any.
            if let Some(constants) = self.push_constants.as_ref() {
                let mut offset: usize = 0;
                for a_constant in constants {
                    let bytes = a_constant.bytes();
                    render_pass.set_push_constants(
                        wgpu::ShaderStage::FRAGMENT,
                        offset as u32,
                        &bytes,
                    );
                    offset += a_constant.size();
                }
            }
            render_pass.draw(0..3, 0..1);
        }

        // Then run all post-processing steps, in order.
        let mut stage_in = &painting;
        let mut stage_out = &post_process_tex;
        // If user has provided custom uniforms, pass them to the post-processing stage as well.
        let mut custom_data = None;
        if let Some(custom_buffer) = self.user_uniforms_buffer.as_ref() {
            custom_data = Some((custom_buffer, self.user_uniforms_buffer_size.unwrap()));
        }
        for i in 0..self.postprocess_ops.len() {
            let postprocess_op = &self.postprocess_ops[i];
            let input_view = stage_in.create_view(&wgpu::TextureViewDescriptor::default());
            let output_view = stage_out.create_view(&wgpu::TextureViewDescriptor::default());
            postprocess_op.post_process(
                &input_view,
                &output_view,
                (
                    &self.uniforms_device_buffer,
                    std::mem::size_of_val(&self.uniforms),
                ),
                custom_data,
                &self.device,
                &mut encoder,
                self.clear_color,
                postprocessing::PipelineType::Movie,
            );
            // Swap input and output textures handles
            std::mem::swap(&mut stage_in, &mut stage_out);
        }

        // Run one more post-process op, the sRGB conversion.
        {
            let input_view = stage_in.create_view(&wgpu::TextureViewDescriptor::default());
            let output_view = stage_out.create_view(&wgpu::TextureViewDescriptor::default());
            self.srgb_postprocess.post_process(
                &input_view,
                &output_view,
                (
                    &self.uniforms_device_buffer,
                    std::mem::size_of_val(&self.uniforms),
                ),
                custom_data,
                &self.device,
                &mut encoder,
                self.clear_color,
                postprocessing::PipelineType::Movie,
            );
        }

        // Then encode a copy of the texture to the buffer.
        {
            let tex_copy_view = wgpu::TextureCopyView {
                mip_level: 0,
                origin: Origin3d::ZERO,
                texture: stage_out,
            };
            let buf_copy_view = wgpu::BufferCopyView {
                buffer: &buffer,
                layout: wgpu::TextureDataLayout {
                    bytes_per_row: ((resolution.x * 4) as usize * std::mem::size_of::<half::f16>())
                        as u32,
                    offset: 0,
                    rows_per_image: resolution.y as u32,
                },
            };
            encoder.copy_texture_to_buffer(
                tex_copy_view,
                buf_copy_view,
                Extent3d {
                    width: resolution.x as u32,
                    height: resolution.y as u32,
                    depth: 1,
                },
            );
        }

        let command_buffer = encoder.finish();
        self.queue.submit(Some(command_buffer));

        self.transmitter
            .send(CanvasMessage::MovieFrameStarted(
                buffer,
                resolution,
                painting_start_time,
            ))
            .unwrap();
    }
}
