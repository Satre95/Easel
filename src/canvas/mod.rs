use crate::dashboard::DashboardMessage;
use crate::push_constants::{load_push_constants_from_json, PushConstant};
use crate::texture::{default_color_sampler, AssetTexture, Texture};
use crate::uniforms::{load_uniforms_from_json, Uniforms, UserUniform};
use crate::vector::{IntVector2, IntVector4, UIntVector2, Vector2, Vector4};
use chrono::Datelike;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::vec::Vec;
use stopwatch::Stopwatch;
use wgpu::util::{BufferInitDescriptor, DeviceExt};
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingResource, Extent3d, LoadOp, Operations, Origin3d, PowerPreference,
    RequestAdapterOptions,
};
use winit::{event::*, window::Window};

pub mod message;
use crate::postprocessing::PostProcess;
use log::{error, info, warn};
use message::CanvasMessage;
use notify::{DebouncedEvent, RecommendedWatcher, Watcher};

/// Pre-compile vertex shader that renders a full-screen quad.
pub static VS_MODULE_BYTES: &[u8] = include_bytes!("../../shaders/vert.spv");
/// The [wgpu::TextureFormat] used when rendering to screen.
pub static RENDER_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// The [wgpu::TextureFormat] used when rendering off-screen painting to write to disk.
pub static PAINTING_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Built-in shader used as a post-processing effect to apply gamma sRGB conversion for painting.
/// This is needed as the [PAINTING_TEXTURE_FORMAT] does not perform automatic sRGB conversion for us.
static POST_PROCESS_SRGB_SHADER_BYTES: &[u8] =
    include_bytes!("../../shaders/post-process-srgb.spv");

/// Central class for the painting on the Easel.
/// Sends & receives messages to/from Dashboard.
/// Provides file watching capabilities for shader and/or custom uniforms.
pub struct Canvas {
    /// Handle to winit Window.
    pub window: Window,
    /// Handle to WebGPU Instance
    pub instance: wgpu::Instance,
    /// Handle to WebGPU render surface
    pub surface: wgpu::Surface,
    /// Handle to WebGPU Adapter
    pub adapter: wgpu::Adapter,
    /// Handle to WebGPU Device. Attempts to use highest performance GPU on system.
    pub device: wgpu::Device,
    /// Handle to command dispatch queue on device.
    pub queue: wgpu::Queue,
    /// Descriptor is kept around for window resizing events.
    sc_desc: wgpu::SwapChainDescriptor,
    /// Handle to swap chain for on-screen rendering.
    swap_chain: wgpu::SwapChain,
    /// Render pipeline used for on-screen rendering. May include post-processing effects, if provided.
    render_pipeline: wgpu::RenderPipeline,
    /// Render pipeline used for off-screen rendering. Will always include sRGB conversion post-processing effect.
    /// May also include other post-processing effects, if provided.
    painting_pipeline: wgpu::RenderPipeline,
    /// The pipeline use to render output of [Self::render_pipeline] to screen.
    swap_chain_pipeline: wgpu::RenderPipeline,
    /// Color with which to [wgpu::LoadOp::Clear] attachments to render passes.
    clear_color: wgpu::Color,
    /// Resolution of render canvas.
    /// **Note:** Distinct from the painting render resolution.
    size: winit::dpi::PhysicalSize<u32>,
    /// Uniforms provided by Canvas to all shaders.
    uniforms: Uniforms,
    /// Handle to device buffer where [Self::uniforms] are copied over.
    uniforms_device_buffer: wgpu::Buffer,
    /// Optional device buffer of user-provided uniforms.
    user_uniforms_buffer: Option<wgpu::Buffer>,
    /// Optional size of device buffer holding user-provided uniforms.
    user_uniforms_buffer_size: Option<usize>,
    /// Optional list of user-provided uniforms from JSON file.
    user_uniforms: Vec<Box<dyn UserUniform>>,
    /// Optional list of user-provided push constants from JSON file.
    push_constants: Option<Vec<Box<dyn PushConstant>>>,

    bind_groups: [wgpu::BindGroup; 2],
    bind_group_layouts: [wgpu::BindGroupLayout; 2],

    /// List of texture handles and their destination binding locations in the shader.
    #[allow(dead_code)]
    textures: Vec<Box<dyn Texture>>,
    /// List of post-processing shaders.
    /// By default includes sRGB Gamma application.
    postprocess_ops: Vec<PostProcess>,
    /// Stopwatch used for calculating time elapsed and other uniforms.
    stop_watch: Stopwatch,
    /// Pause/Play state. Also pauses [Self::stop_watch], which sets time data in [Self::uniforms].
    paused: bool,
    /// Time of last update. Use to calculate time deltas in [Self::uniforms].
    last_update: std::time::Instant,

    /// Used to send messages to Dashboard.
    transmitter: Sender<CanvasMessage>,
    /// Use to receive messages from Dashboard.
    receiver: Receiver<DashboardMessage>,
    /// Whether to show the window titlebar.
    show_titlebar: bool,

    /// Optional file watcher used to watch the fragment shader.
    shader_file_watcher: Option<RecommendedWatcher>,
    /// Optional receiver of file watcher events for the fragment shader.
    shader_file_watcher_receiver: Option<Receiver<DebouncedEvent>>,
    /// Optional file watcher used to watch the JSON file.
    json_file_watcher: Option<RecommendedWatcher>,
    /// Optional receiver of file watcher events for the JSON file.
    json_file_watcher_receiver: Option<Receiver<DebouncedEvent>>,
}

impl Canvas {
    /// Construct a new Canvas object
    /// * `window` - [winit::window::Window] to render to. Takes ownership
    /// * `fs_spirv_data` - Binary data of compiled fragment shader
    /// * `images` - Optional array of images to bind to shader. Images are bound in the same order as specified here.
    /// * `user_uniforms` - Optional array of user-specified uniforms to bind in shader. Uniforms are bound in same order as specified here.
    /// * `push_constants` - Optional array of push constants to bind in shader. Constants are bound in same order as specified here.
    /// * `transmitter` - [std::sync::mpsc::Sender] object used for sending [CanvasMessage]s to interested parties.
    /// * `receiver` - [std::sync::mpsc::Receiver] object used to received messages from [crate::dashboard::Dashboard]
    pub async fn new(
        window: Window,
        fs_spirv_data: Vec<u8>,
        images: Option<Vec<image::DynamicImage>>,
        user_uniforms: Option<Vec<Box<dyn UserUniform>>>,
        push_constants: Option<Vec<Box<dyn PushConstant>>>,
        transmitter: Sender<CanvasMessage>,
        receiver: Receiver<DashboardMessage>,
    ) -> Self {
        let instance = wgpu::Instance::new(wgpu::BackendBit::PRIMARY);
        let size = window.inner_size();

        let surface: wgpu::Surface;
        unsafe {
            surface = instance.create_surface(&window);
        }

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                compatible_surface: Some(&surface),
                power_preference: PowerPreference::HighPerformance,
            })
            .await
            .unwrap();
        // From: https://docs.rs/wgpu/0.6.2/wgpu/struct.Limits.html#structfield.max_push_constant_size
        let max_push_constant_size = match wgpu::BackendBit::PRIMARY {
            wgpu::BackendBit::VULKAN => 256,
            wgpu::BackendBit::DX12 => 256,
            wgpu::BackendBit::METAL => 4096,
            _ => 128,
        };
        let limits = wgpu::Limits {
            max_push_constant_size,
            ..Default::default()
        };
        let device_desc = wgpu::DeviceDescriptor {
            features: adapter.features(),
            limits,
            shader_validation: true,
        };

        let (device, queue) = adapter.request_device(&device_desc, None).await.unwrap();

        //------------------------------------------------------------------------------------------
        // Create uniforms, device buffer, and bindings.
        let mut uniforms = Uniforms::new();
        uniforms.resolution = Vector4::new(size.width as f32, size.height as f32, 0.0, 0.0);
        uniforms.num_textures = match &images {
            Some(vec) => vec.len() as u32,
            None => 0,
        };
        let descriptor = BufferInitDescriptor {
            label: Some("Uniforms Buffer"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
        };
        let u_buffer = device.create_buffer_init(&descriptor);

        //------------------------------------------------------------------------------------------
        // Bind custom uniforms, if provided
        let mut custom_uniforms_buffer = None;
        let mut custom_uniforms_buffer_size = 0;
        if let Some(dem_uniforms) = &user_uniforms {
            let mut total_size = 0;
            for a_uniform in dem_uniforms {
                total_size += a_uniform.size();
            }

            custom_uniforms_buffer_size = total_size;
            let mut bytes = Vec::with_capacity(total_size);
            for a_uniform in dem_uniforms {
                bytes.extend_from_slice(&a_uniform.bytes());
            }

            let desc = BufferInitDescriptor {
                label: Some("Custom Uniforms Buffer"),
                contents: &bytes,
                usage: wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
            };

            custom_uniforms_buffer = Some(device.create_buffer_init(&desc));
        }

        //------------------------------------------------------------------------------------------
        // Load textures.
        let mut asset_textures = Vec::<Box<dyn Texture>>::new();
        if let Some(vec) = images {
            for an_image in &vec {
                asset_textures.push(Box::new(AssetTexture::new_with_image(
                    an_image, &device, &queue,
                )));
            }
        }

        //------------------------------------------------------------------------------------------
        // Setup swap chain
        let sc_desc = wgpu::SwapChainDescriptor {
            usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
        };
        let swap_chain = device.create_swap_chain(&surface, &sc_desc);

        //------------------------------------------------------------------------------------------
        // Load shaders.
        let vs_module = device.create_shader_module(wgpu::util::make_spirv(VS_MODULE_BYTES));
        let fs_module = device.create_shader_module(wgpu::util::make_spirv(&fs_spirv_data));

        //------------------------------------------------------------------------------------------
        // Create the bind group layout and entries.
        // Uniforms and our generated textures are set 0
        let primary_bind_group_layout: wgpu::BindGroupLayout;
        {
            let mut bind_group_layout_entries = Vec::<wgpu::BindGroupLayoutEntry>::new();
            // Uniforms are first.
            bind_group_layout_entries.push(wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStage::FRAGMENT,
                ty: wgpu::BindingType::UniformBuffer {
                    dynamic: false,
                    min_binding_size: None,
                },
                count: None,
            });
            if let Some(_) = custom_uniforms_buffer {
                bind_group_layout_entries.push(wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStage::FRAGMENT,
                    ty: wgpu::BindingType::UniformBuffer {
                        dynamic: false,
                        min_binding_size: None,
                    },
                    count: None,
                });
            }
            primary_bind_group_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &bind_group_layout_entries,
                });
        }

        // In set 1, bind provided textures.
        let secondary_bind_group_layout: wgpu::BindGroupLayout;
        {
            let mut bind_group_layout_entries = Vec::<wgpu::BindGroupLayoutEntry>::new();
            // For now, we only have 1 sampler per set
            bind_group_layout_entries.push(BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStage::FRAGMENT,
                ty: wgpu::BindingType::Sampler { comparison: false },
                count: None,
            });
            for i in 1..=asset_textures.len() {
                bind_group_layout_entries.push(BindGroupLayoutEntry {
                    binding: i as u32,
                    visibility: wgpu::ShaderStage::FRAGMENT,
                    ty: wgpu::BindingType::SampledTexture {
                        component_type: wgpu::TextureComponentType::Float,
                        multisampled: true,
                        dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                });
            }
            // Create the Bind Group Layout.
            secondary_bind_group_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &bind_group_layout_entries,
                });
        }

        //------------------------------------------------------------------------------------------
        // Create Bind Groups from layouts.
        let primary_bind_group: wgpu::BindGroup;
        {
            let mut primary_bind_group_entries: Vec<BindGroupEntry> = Vec::new();
            // Provided Uniforms first.
            primary_bind_group_entries.push(wgpu::BindGroupEntry {
                binding: 0,
                resource: BindingResource::Buffer(
                    u_buffer.slice(0..(std::mem::size_of_val(&uniforms) as u64)),
                ),
            });
            // Custom Uniforms next, if enabled.
            if let Some(cu_buffer) = &custom_uniforms_buffer {
                primary_bind_group_entries.push(wgpu::BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Buffer(
                        cu_buffer.slice(0..(custom_uniforms_buffer_size as u64)),
                    ),
                });
            }

            // Finally create the bind group.
            primary_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Primary Bind Group"),
                layout: &primary_bind_group_layout,
                entries: &primary_bind_group_entries,
            });
        }

        let secondary_bind_group: wgpu::BindGroup;
        {
            let mut secondary_bind_group_entries: Vec<BindGroupEntry> = Vec::new();
            let default_sampler = default_color_sampler(&device);
            secondary_bind_group_entries.push(BindGroupEntry {
                binding: 0,
                resource: BindingResource::Sampler(&default_sampler),
            });
            // Create texture views.
            let mut tex_views = Vec::<wgpu::TextureView>::new();
            for tex in &asset_textures {
                let texture_view = tex.get_view(0);
                tex_views.push(texture_view);
            }
            // Add texture view bindings.
            for tex_bind_idx in 1..=tex_views.len() {
                secondary_bind_group_entries.push(BindGroupEntry {
                    binding: tex_bind_idx as u32,
                    resource: BindingResource::TextureView(&tex_views[tex_bind_idx - 1]),
                });
            }
            secondary_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Secondary Bind Group"),
                layout: &secondary_bind_group_layout,
                entries: &secondary_bind_group_entries,
            });
        }

        //------------------------------------------------------------------------------------------
        // Create render pipeline.
        let mut constants_for_pipeline = vec![];
        if let Some(constants) = push_constants.as_ref() {
            let mut size = 0;
            for a_constant in constants {
                size += a_constant.size();
            }
            constants_for_pipeline.push(wgpu::PushConstantRange {
                stages: wgpu::ShaderStage::FRAGMENT,
                range: 0..(size as u32),
            });
        }
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Canvas Pipeline Layout"),
                bind_group_layouts: &[&primary_bind_group_layout, &secondary_bind_group_layout],
                push_constant_ranges: &constants_for_pipeline,
            });
        let (render_pipeline, painting_pipeline) = crate::utils::create_pipelines(
            &device,
            &render_pipeline_layout,
            &vs_module,
            &fs_module,
            (RENDER_TEXTURE_FORMAT, PAINTING_TEXTURE_FORMAT),
        );
        // Swap chain pipeline will never change and is separate from others.
        let swap_chain_pipeline =
            crate::utils::create_swap_chain_pipeline(&device, &vs_module, sc_desc.format);

        let postprocess_ops: Vec<PostProcess> = vec![PostProcess::new(
            &device,
            Vec::from(POST_PROCESS_SRGB_SHADER_BYTES),
            custom_uniforms_buffer.is_some(),
        )];
        let mut custom_size = None;
        if custom_uniforms_buffer_size > 0 {
            custom_size = Some(custom_uniforms_buffer_size);
        }

        // Inform dashboard of our window size so that it has a sensible default for painting res.
        transmitter
            .send(CanvasMessage::UpdatePaintingResolutioninGUI(
                IntVector2::new(size.width as i32, size.height as i32),
            ))
            .unwrap();
        Self {
            window,
            instance,
            surface,
            adapter,
            device,
            queue,
            sc_desc,
            swap_chain,
            render_pipeline,
            painting_pipeline,
            swap_chain_pipeline,
            clear_color: wgpu::Color {
                r: 0.1,
                g: 0.2,
                b: 0.3,
                a: 1.0,
            },
            size,
            uniforms,
            user_uniforms_buffer: custom_uniforms_buffer,
            user_uniforms_buffer_size: custom_size,
            user_uniforms: match user_uniforms {
                Some(uni) => uni,
                None => vec![],
            },
            push_constants,
            uniforms_device_buffer: u_buffer,
            bind_groups: [primary_bind_group, secondary_bind_group],
            bind_group_layouts: [primary_bind_group_layout, secondary_bind_group_layout],
            textures: asset_textures,
            postprocess_ops,
            stop_watch: Stopwatch::start_new(),
            paused: false,
            last_update: std::time::Instant::now(),
            transmitter,
            receiver,
            show_titlebar: true,
            shader_file_watcher: None,
            shader_file_watcher_receiver: None,
            json_file_watcher: None,
            json_file_watcher_receiver: None,
        }
    }

    /// Expected to be called from main thread when user resizes canvas window.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.size = new_size;
        self.sc_desc.width = new_size.width;
        self.sc_desc.height = new_size.height;
        self.swap_chain = self.device.create_swap_chain(&self.surface, &self.sc_desc);
        self.uniforms.resolution.x = new_size.width as f32;
        self.uniforms.resolution.y = new_size.height as f32;
        self.transmitter
            .send(CanvasMessage::WindowResized(IntVector2::new(
                new_size.width as i32,
                new_size.height as i32,
            )))
            .unwrap();
    }

    /// Expected to be called from main thread to handle IO events.
    /// This fn assumes the incoming events are from the Canvas' window.
    pub fn input(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::KeyboardInput { input, .. } => match input {
                KeyboardInput {
                    state: ElementState::Pressed,
                    virtual_keycode: Some(VirtualKeyCode::Space),
                    ..
                } => {
                    self.paused = !self.paused;
                    if self.paused {
                        self.stop_watch.stop();
                    } else {
                        self.stop_watch.start();
                    }
                    self.transmitter
                        .send(CanvasMessage::PausePlayChanged)
                        .unwrap();
                    true
                }
                KeyboardInput {
                    state: ElementState::Pressed,
                    virtual_keycode: Some(VirtualKeyCode::Escape),
                    ..
                } => false,
                _ => true,
            },
            WindowEvent::CursorMoved { position, .. } => {
                self.uniforms.mouse_info.z = self.uniforms.mouse_info.x;
                self.uniforms.mouse_info.w = self.uniforms.mouse_info.y;
                self.uniforms.mouse_info.x = position.x as f32;
                self.uniforms.mouse_info.y = position.y as f32;
                // Send message.
                self.transmitter
                    .send(CanvasMessage::MouseMoved(Vector2::new(
                        self.uniforms.mouse_info.x,
                        self.uniforms.mouse_info.y,
                    )))
                    .unwrap();
                true
            }
            WindowEvent::Resized(physical_size) => {
                self.resize(*physical_size);
                true
            }
            WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                // new_inner_size is &mut so w have to dereference it twice
                self.resize(**new_inner_size);
                true
            }
            _ => false,
        }
    }

    /// Used to parse messages received from Dashboard and act accordingly.
    fn dashboard_signal_received(&mut self, message: DashboardMessage) {
        match message {
            DashboardMessage::PausePlayChanged => {
                self.paused = !self.paused;
                if self.paused {
                    self.stop_watch.stop();
                } else {
                    self.stop_watch.start();
                }
            }
            DashboardMessage::Pause => {
                self.paused = true;
                self.stop_watch.stop();
            }
            DashboardMessage::Play => {
                self.paused = false;
                self.stop_watch.start();
            }
            DashboardMessage::TitlebarStatusChanged => {
                self.show_titlebar = !self.show_titlebar;
                self.window.set_decorations(self.show_titlebar);
            }
            DashboardMessage::PaintingRenderRequested(resolution) => {
                self.create_painting(resolution)
            }
            DashboardMessage::UniformUpdatedViaGUI(modified_uniform) => {
                let user_uniforms = &mut self.user_uniforms;
                if let Some(index) = user_uniforms
                    .iter()
                    .position(|uniform| uniform.name() == modified_uniform.name())
                {
                    user_uniforms[index] = modified_uniform;
                }
            }
            DashboardMessage::MovieRenderRequested(resolution) => {
                self.create_movie_frame(resolution);
            }
        }
    }

    /// Called every frame prior to render.
    /// Updates uniforms, checks watched files (if any), examines messages from Dashboard.
    pub fn update(&mut self) {
        // Receive messages from Dashboard and act accordingly
        loop {
            let msg_result = self.receiver.try_recv();
            match msg_result {
                Ok(msg) => self.dashboard_signal_received(msg),
                Err(_) => break,
            }
        }

        {
            // Check if shader file watcher reports file updated.
            let mut file_events = Vec::new();
            match &self.shader_file_watcher_receiver {
                Some(rx) => loop {
                    let msg_result = rx.try_recv();
                    match msg_result {
                        Ok(event) => file_events.push(event),
                        Err(_) => break,
                    }
                },
                None => {}
            }
            for an_event in file_events {
                self.update_shader_pipeline(an_event);
            }
        }
        {
            let mut file_events = Vec::new();
            // Check if json file watcher reports file updated.
            match &self.json_file_watcher_receiver {
                Some(rx) => loop {
                    let msg_result = rx.try_recv();
                    match msg_result {
                        Ok(event) => file_events.push(event),
                        Err(_) => break,
                    }
                },
                None => {}
            }
            for an_event in file_events {
                self.update_custom_uniforms_from_file(an_event);
            }
        }
        // Referesh user uniforms buffer
        if let Some(buffer) = &self.user_uniforms_buffer {
            let mut total_size = 0;
            for a_uniform in &self.user_uniforms {
                total_size += a_uniform.size();
            }
            let mut bytes = Vec::with_capacity(total_size);
            for a_uniform in &self.user_uniforms {
                bytes.extend_from_slice(&a_uniform.bytes());
            }
            self.queue.write_buffer(&buffer, 0, &bytes);
        }

        // Only actually update uniforms if not paused, but we always update buffer.
        if !self.paused {
            self.uniforms.frame_num += 1;
            self.uniforms.time = self.stop_watch.elapsed().as_secs_f32();
            let now = std::time::Instant::now();
            let delta_duration = now.duration_since(self.last_update);
            self.uniforms.time_delta = delta_duration.as_secs_f32();
            let today = chrono::Local::now();
            self.uniforms.date =
                IntVector4::new(today.year(), today.month() as i32, today.day() as i32, 0);
            self.last_update = now;
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Update Uniforms Encoder"),
            });
        // Copy uniforms from CPU to staging buffer, then copy from staging buffer to main buf.
        let descriptor = BufferInitDescriptor {
            label: Some("Uniforms Buffer"),
            contents: bytemuck::bytes_of(&self.uniforms),
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
        let command_buffer = encoder.finish();
        self.queue.submit(Some(command_buffer));
    }

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
            usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT | wgpu::TextureUsage::SAMPLED,
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
        // The last op is always the sRGB conversion, which we don't need when rendering to screen.
        for i in 0..self.postprocess_ops.len() - 1 {
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
                false,
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
                        ty: wgpu::BindingType::Sampler { comparison: false },
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        count: None,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::SampledTexture {
                            component_type: wgpu::TextureComponentType::Float,
                            dimension: wgpu::TextureViewDimension::D2,
                            multisampled: true,
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
            usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT
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
                label: Some("Painting Encoder"),
            });

        let painting_start_time = std::time::Instant::now();
        // First run the pipeline.
        {
            let painting_view = painting.create_view(&wgpu::TextureViewDescriptor::default());
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
        for postprocess_op in &mut self.postprocess_ops {
            // If user has provided custom uniforms, pass them to the post-processing stage as well.
            let mut custom_data = None;
            if let Some(custom_buffer) = self.user_uniforms_buffer.as_ref() {
                custom_data = Some((custom_buffer, self.user_uniforms_buffer_size.unwrap()));
            }
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
                true,
            );
            // Swap input and output textures handles
            std::mem::swap(&mut stage_in, &mut stage_out);
        }
        // Swap one more time to get final output tex (undo last swap).
        std::mem::swap(&mut stage_in, &mut stage_out);

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

    /// Use to trigger automatic reload when shader is changed on disk.
    /// Works for both text source and SPIR-V binaries
    pub fn watch_shader_file(&mut self, file: &str, interval_ms: u64) {
        let (tx, rx) = channel();
        let mut file_watcher =
            notify::watcher(tx, std::time::Duration::from_millis(interval_ms)).unwrap();
        file_watcher
            .watch(file, notify::RecursiveMode::NonRecursive)
            .expect("Invalid file provided.");

        self.shader_file_watcher = Some(file_watcher);
        self.shader_file_watcher_receiver = Some(rx);
    }

    /// Use to trigger automatic reload when uniforms file is changed on disk.
    pub fn watch_uniforms_file(&mut self, file: &str, interval_ms: u64) {
        let (tx, rx) = channel();
        let mut file_watcher =
            notify::watcher(tx, std::time::Duration::from_millis(interval_ms)).unwrap();
        file_watcher
            .watch(file, notify::RecursiveMode::NonRecursive)
            .expect("Invalid file provided.");

        self.json_file_watcher = Some(file_watcher);
        self.json_file_watcher_receiver = Some(rx);
    }

    /// Reload uniforms file from disk and update render pipelines.
    fn update_custom_uniforms_from_file(&mut self, event: DebouncedEvent) {
        let mut disable = false;
        match event {
            DebouncedEvent::Create(path_buf) | DebouncedEvent::Write(path_buf) => {
                let file = path_buf.to_str().unwrap();
                info!("Detected uniforms JSON file changed, reloading {}", file);
                let text =
                    std::fs::read_to_string(file).expect("Error reading uniforms from file.");
                let json_data = json::parse(&text).expect("Error parsing JSON");
                self.user_uniforms = load_uniforms_from_json(&json_data);
                self.push_constants = Some(load_push_constants_from_json(&json_data));
            }
            DebouncedEvent::Remove(path_buf) => {
                info!(
                    "Uniforms JSON file {} removed, disabling file watcher.",
                    path_buf.to_str().unwrap()
                );
                disable = true;
            }
            DebouncedEvent::Rename(src, _) => {
                info!(
                    "Uniforms JSON file {} renamed, disabling file watcher.",
                    src.to_str().unwrap()
                );
                disable = true;
            }
            DebouncedEvent::Error(err, buf) => {
                warn!("Encountered error {:?}", err);
                match buf {
                    Some(path) => warn!("File: {}", path.to_str().unwrap()),
                    None => {}
                }
                warn!("Disabling file watcher.");
                disable = true;
            }
            _ => {}
        }
        if disable {
            self.json_file_watcher_receiver = None;
            self.json_file_watcher = None
        }
    }

    /// Reload shader from disk and update render pipelines
    fn update_shader_pipeline(&mut self, event: DebouncedEvent) {
        let mut disable = false;
        match event {
            DebouncedEvent::Create(path_buf) | DebouncedEvent::Write(path_buf) => {
                let file = path_buf.to_str().unwrap();
                let fs_spirv_data = match crate::utils::load_shader(file) {
                    Ok(data) => data,
                    Err(e) => {
                        error!("Error compiling shader: {}", e);
                        self.transmitter
                            .send(CanvasMessage::ShaderCompilationFailed(e.to_string()))
                            .unwrap();
                        return;
                    }
                };
                let fs_module = self
                    .device
                    .create_shader_module(wgpu::util::make_spirv(&fs_spirv_data));
                let vs_module = self
                    .device
                    .create_shader_module(wgpu::util::make_spirv(VS_MODULE_BYTES));

                let layouts = [&self.bind_group_layouts[0], &self.bind_group_layouts[1]];
                let mut constants_for_pipeline = vec![];
                if let Some(constants) = self.push_constants.as_ref() {
                    let mut size = 0;
                    for a_constant in constants {
                        size += a_constant.size();
                    }
                    constants_for_pipeline.push(wgpu::PushConstantRange {
                        stages: wgpu::ShaderStage::FRAGMENT,
                        range: 0..(size as u32),
                    });
                }
                let render_pipeline_layout =
                    self.device
                        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                            label: Some("Canvas Pipeline Layout"),
                            bind_group_layouts: &layouts,
                            push_constant_ranges: &constants_for_pipeline,
                        });
                let (render_pipeline, painting_pipeline) = crate::utils::create_pipelines(
                    &self.device,
                    &render_pipeline_layout,
                    &vs_module,
                    &fs_module,
                    (RENDER_TEXTURE_FORMAT, PAINTING_TEXTURE_FORMAT),
                );

                self.render_pipeline = render_pipeline;
                self.painting_pipeline = painting_pipeline;

                self.transmitter
                    .send(CanvasMessage::ShaderCompilationSucceeded)
                    .unwrap();
                info!("Detected shader file changed, reloading {}", file);
            }
            DebouncedEvent::Remove(path_buf) => {
                info!(
                    "Shader file {} removed, disabling file watcher.",
                    path_buf.to_str().unwrap()
                );
                disable = true;
            }
            DebouncedEvent::Rename(src, _) => {
                info!(
                    "Shader file {} renamed, disabling file watcher.",
                    src.to_str().unwrap()
                );
                disable = true;
            }
            DebouncedEvent::Error(err, buf) => {
                warn!("Encountered error {:?}", err);
                match buf {
                    Some(path) => warn!("File: {}", path.to_str().unwrap()),
                    None => {}
                }
                warn!("Disabling file watcher.");
                disable = true;
            }
            _ => {}
        }
        if disable {
            self.shader_file_watcher_receiver = None;
            self.shader_file_watcher = None
        }
    }

    pub fn add_post_processing_shader(&mut self, shader_data: Vec<u8>) {
        let postprocess = PostProcess::new(
            &self.device,
            shader_data,
            self.user_uniforms_buffer.is_some(),
        );
        // We have a default included post-processing stage that is run in the painting pipeline
        // for doing sRGB conversion. That must always run last.
        self.postprocess_ops
            .insert(self.postprocess_ops.len() - 1, postprocess);
    }

    /// Time to exit, cleanup resources.
    pub fn exit_requested(&mut self) {
        self.shader_file_watcher = None;
        self.shader_file_watcher_receiver = None;
        self.json_file_watcher = None;
        self.json_file_watcher_receiver = None;
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
    }

    /// Called when Dashboard requests a movie render frame.
    pub fn create_movie_frame(&mut self, resolution: UIntVector2) {
        let painting_tex_desc = wgpu::TextureDescriptor {
            size: Extent3d {
                width: resolution.x as u32,
                height: resolution.y as u32,
                depth: 1,
            },
            format: PAINTING_TEXTURE_FORMAT,
            usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT
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
                label: Some("Painting Encoder"),
            });

        let painting_start_time = std::time::Instant::now();
        // First run the pipeline.
        {
            let painting_view = painting.create_view(&wgpu::TextureViewDescriptor::default());
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
        for postprocess_op in &mut self.postprocess_ops {
            // If user has provided custom uniforms, pass them to the post-processing stage as well.
            let mut custom_data = None;
            if let Some(custom_buffer) = self.user_uniforms_buffer.as_ref() {
                custom_data = Some((custom_buffer, self.user_uniforms_buffer_size.unwrap()));
            }
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
                true,
            );
            // Swap input and output textures handles
            std::mem::swap(&mut stage_in, &mut stage_out);
        }
        // Swap one more time to get final output tex (undo last swap).
        std::mem::swap(&mut stage_in, &mut stage_out);

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
