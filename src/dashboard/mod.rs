use crate::vector::{IntVector2, UIntVector2};
use crate::{canvas::CanvasMessage, uniforms::UserUniform};
use crate::{recording::Recorder, utils::AsyncTiffWriter};
use core::panic;

use imgui::FontSource;
use imgui_wgpu::RendererConfig;
use imgui_winit_support;
use std::{
    sync::mpsc::{Receiver, SyncSender},
    time::Instant,
};
use wgpu::{PowerPreference, RequestAdapterOptions};
use winit::window::Window;

mod ui;
pub use self::ui::*;

mod state;
pub use self::state::*;

/// Message Enums used by [Dashboard] to send messages to interested parties.
pub enum DashboardMessage {
    PausePlayChanged,
    Play,
    Pause,
    TitlebarStatusChanged,
    PaintingRenderRequested(UIntVector2),
    PaintingResolutionUpdated(UIntVector2),
    MovieRenderRequested(UIntVector2),
    UniformUpdatedViaGUI(Box<dyn UserUniform>),
}

/// Centralized controller and GUI class.
/// Renders to its own window and provides controls for render [crate::canvas::Canvas]
/// Provides runtime stats and other useful information.
pub struct Dashboard {
    pub window: winit::window::Window,
    pub instance: wgpu::Instance,
    pub surface: wgpu::Surface,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    sc_desc: wgpu::SwapChainDescriptor,
    swap_chain: wgpu::SwapChain,

    clear_color: wgpu::Color,
    size: winit::dpi::PhysicalSize<u32>,
    imgui_context: imgui::Context,
    imgui_platform: imgui_winit_support::WinitPlatform,
    imgui_renderer: imgui_wgpu::Renderer,
    last_frame: std::time::Instant,
    hidpi_factor: f32,

    state: DashboardState,

    transmitter: SyncSender<DashboardMessage>,
    receiver: Receiver<CanvasMessage>,
    recorder: Option<Recorder>,
    last_movie_frame_time: Option<Instant>,
}

impl Dashboard {
    /// Construct a new [Dashboard].
    /// * `window` - The [winit::window::Window] this object will render to. Takes ownership.
    /// * `transmitter` - [std::sync::mpsc::Sender] object used to send [DashboardMessage]s to intererested parties.
    /// * `receiver` - [std::sync::mpsc::Receiver] object used to receive messages from [crate::canvas::Canvas]
    pub async fn new(
        window: Window,
        transmitter: SyncSender<DashboardMessage>,
        receiver: Receiver<CanvasMessage>,
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
                power_preference: PowerPreference::LowPower,
            })
            .await
            .unwrap();
        let device_desc = wgpu::DeviceDescriptor {
            label: None,
            features: adapter.features(),
            limits: Default::default(),
        };

        let (device, mut queue) = adapter.request_device(&device_desc, None).await.unwrap();

        //------------------------------------------------------------------------------------------
        // Setup swap chain
        let sc_desc = wgpu::SwapChainDescriptor {
            usage: wgpu::TextureUsage::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
        };
        let swap_chain = device.create_swap_chain(&surface, &sc_desc);

        //------------------------------------------------------------------------------------------
        // ImGUI setup
        let hidpi_factor = window.scale_factor() as f32;
        let mut imgui = imgui::Context::create();
        let mut platform = imgui_winit_support::WinitPlatform::init(&mut imgui);
        platform.attach_window(
            imgui.io_mut(),
            &window,
            imgui_winit_support::HiDpiMode::Default,
        );
        let font_size = (18.0 * hidpi_factor) as f32;
        imgui.io_mut().font_global_scale = (1.0 / hidpi_factor) as f32;
        imgui.set_ini_filename(None);
        imgui.fonts().add_font(&[FontSource::TtfData {
            size_pixels: font_size,
            data: include_bytes!("../../assets/Quicksand/static/Quicksand-Medium.ttf"),
            config: Some(imgui::FontConfig {
                oversample_v: hidpi_factor as i32,
                oversample_h: hidpi_factor as i32,
                size_pixels: font_size,
                ..Default::default()
            }),
        }]);

        //------------------------------------------------------------------------------------------
        // Setup ImGUI WGPU Renderer
        let clear_color = wgpu::Color {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 1.0,
        };
        let mut renderer_config = RendererConfig::new_srgb();
        renderer_config.texture_format = sc_desc.format;
        let renderer = imgui_wgpu::Renderer::new(&mut imgui, &device, &mut queue, renderer_config);
        let mut state = DashboardState::new();
        state.render_window_size = IntVector2::new(size.width as i32, size.height as i32);

        Self {
            window,
            instance,
            surface,
            adapter,
            device,
            queue,
            sc_desc,
            swap_chain,
            clear_color,
            size,
            imgui_context: imgui,
            imgui_platform: platform,
            imgui_renderer: renderer,
            last_frame: std::time::Instant::now(),
            hidpi_factor,
            state,
            transmitter,
            receiver,
            recorder: None,
            last_movie_frame_time: None,
        }
    }

    /// Used to parse and respond to messages received from [crate::canvas::Canvas]
    fn handle_message(&mut self, message: CanvasMessage) {
        match message {
            CanvasMessage::FrameStep => self.state.frame_num += 1,
            CanvasMessage::MouseMoved(pos) => self.state.mouse_pos = pos,
            CanvasMessage::RenderPassSubmitted => {}
            CanvasMessage::WindowResized(new_size) => self.state.render_window_size = new_size,
            CanvasMessage::SwapChainFrameError(frame_error) => match frame_error {
                wgpu::SwapChainError::Timeout => self.state.frame_timeout_count += 1,
                _ => {}
            },
            CanvasMessage::PaintingStarted(buf, resolution, start_time) => {
                let filename = self.state.painting_filename.clone() + ".tiff";
                self.state.painting_start_time = Some(start_time);
                let open_externally = match cfg!(target_os = "macos") {
                    true => self.state.open_painting_externally,
                    false => false,
                };
                self.state.painting_progress_receiver = Some(AsyncTiffWriter::write(
                    buf,
                    UIntVector2::new(resolution.x as u32, resolution.y as u32),
                    filename,
                    open_externally,
                ));
            }
            CanvasMessage::ShaderCompilationFailed(err_msg) => {
                self.state.shader_compilation_error_msg = Some(err_msg);
                // Pause rendering
                self.transmitter.send(DashboardMessage::Pause).unwrap();
            }
            CanvasMessage::ShaderCompilationSucceeded => {
                self.state.shader_compilation_error_msg = None;
                self.transmitter.send(DashboardMessage::Play).unwrap();
                self.state.paused = false;
            }
            CanvasMessage::PausePlayChanged => {
                self.state.paused = !self.state.paused;
            }
            CanvasMessage::UniformForGUI(uniform) => {
                self.state.gui_uniforms.push(uniform);
            }
            CanvasMessage::UpdatePaintingResolutioninGUI(res) => {
                self.state.painting_resolution = res;
            }
            CanvasMessage::MovieFrameStarted(buf, resolution, start_time) => {
                if let Some(ref mut recorder) = self.recorder {
                    recorder.add_frame(buf, resolution, start_time);
                } else {
                    panic!("Frame received for movie at timestamp {:?}, but no recorder is instantiated.", start_time);
                }
            }
        }
    }

    /// Expected to be called every frame tick **before** [Self::render_dashboard()]
    /// Checks the receiver queue for any incoming messages, among other things.
    pub fn update(&mut self) {
        self.device.poll(wgpu::Maintain::Poll);
        let update_time = std::time::Instant::now();
        // First, check if we have received any messages and act accordingly
        loop {
            let msg_result = self.receiver.try_recv();
            match msg_result {
                Ok(msg) => self.handle_message(msg),
                Err(_) => break,
            }
        }

        if let Some(ref mut recorder) = self.recorder {
            if self.state.movie_framerate < 1 {
                panic!("Invalid framerate {} provided!", self.state.movie_framerate);
            }
            // If we have not stopped, keep requesting frames on the selected FPS interval
            let mut frame_needed = self.state.recording_in_progress;
            if let Some(last_frame_time) = self.last_movie_frame_time.as_mut() {
                let seconds_per_frame = 1.0 / (self.state.movie_framerate as f64);
                let delta = (update_time - *last_frame_time).as_secs_f64();
                frame_needed = frame_needed && delta >= seconds_per_frame;
            }
            if frame_needed && recorder.ready {
                self.transmitter
                    .send(DashboardMessage::MovieRenderRequested(UIntVector2::new(
                        self.state.recording_resolution.x as u32,
                        self.state.recording_resolution.y as u32,
                    )))
                    .unwrap();
                self.last_movie_frame_time = Some(update_time);
            }
            // If finished, cleanup.
            if recorder.poll() {
                self.recorder.take().unwrap().finish();
            }
        }

        // Ping Canvas with the currently set painting res
        self.transmitter
            .send(DashboardMessage::PaintingResolutionUpdated(
                UIntVector2::new(
                    self.state.painting_resolution.x as u32,
                    self.state.painting_resolution.y as u32,
                ),
            ))
            .unwrap();
    }

    pub fn post_render(&mut self) {
        for uniform in &self.state.gui_uniforms {
            self.transmitter
                .send(DashboardMessage::UniformUpdatedViaGUI(uniform.copy()))
                .unwrap();
        }
        self.state.gui_uniforms.clear();
        let now = std::time::Instant::now();
        self.state.last_render_time = (now - self.last_frame).as_secs_f64() * 1000.0;
        self.window.request_redraw();
        self.last_frame = now;
    }
}
