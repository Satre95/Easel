use super::{Dashboard, DashboardMessage};
use crate::{recording::Recorder, recording::MOVIE_TEXTURE_FORMAT, uniforms, vector::UIntVector2};
use imgui::Condition;
use imgui::{im_str, ImString, StyleColor};
use log::{info, warn};
use winit::event::*;

impl Dashboard {
    /// Receives events from the winit event queue and responds appropriately.
    pub fn input(&mut self, event: &winit::event::Event<()>) {
        match event {
            Event::WindowEvent {
                ref event,
                window_id,
            } if *window_id == self.window.id() => match event {
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    self.hidpi_factor = *scale_factor as f32;
                }
                WindowEvent::Resized(physical_size) => {
                    self.size = *physical_size;
                    self.sc_desc = wgpu::SwapChainDescriptor {
                        usage: wgpu::TextureUsage::RENDER_ATTACHMENT,
                        format: wgpu::TextureFormat::Bgra8UnormSrgb,
                        width: physical_size.width as u32,
                        height: physical_size.height as u32,
                        present_mode: wgpu::PresentMode::Mailbox,
                    };
                    self.swap_chain = self.device.create_swap_chain(&self.surface, &self.sc_desc);
                }
                WindowEvent::KeyboardInput { input, .. } => match input {
                    KeyboardInput {
                        state: ElementState::Pressed,
                        virtual_keycode: Some(VirtualKeyCode::Space),
                        ..
                    } => {
                        self.state.paused = !self.state.paused;
                        self.transmitter
                            .send(DashboardMessage::PausePlayChanged)
                            .unwrap();
                    }
                    _ => (),
                },
                _ => {}
            },
            _ => (),
        }
        self.imgui_platform
            .handle_event(self.imgui_context.io_mut(), &self.window, event);
    }

    /// Renders the UI and responds to UI events.
    pub fn render_dashboard(&mut self) {
        let now = std::time::Instant::now();
        self.imgui_context
            .io_mut()
            .update_delta_time(now - self.last_frame);
        self.last_frame = now;

        let frame = match self.swap_chain.get_current_frame() {
            Ok(frame) => frame,
            Err(e) => {
                warn!("GUI Dropped frame: {:?}", e);
                return;
            }
        };
        self.imgui_platform
            .prepare_frame(self.imgui_context.io_mut(), &self.window)
            .expect("Failed to prepare frame");

        let ui = self.imgui_context.frame();
        let mut color_tokens = vec![];
        color_tokens.push(ui.push_style_color(StyleColor::Text, [0.0, 0.0, 0.0, 1.0]));
        color_tokens.push(ui.push_style_color(StyleColor::Header, [0.949, 0.949, 0.953, 1.0]));
        color_tokens.push(ui.push_style_color(StyleColor::HeaderHovered, [1.0, 1.0, 1.0, 1.0]));
        color_tokens.push(ui.push_style_color(StyleColor::Button, [0.741, 0.933, 0.984, 1.0]));
        color_tokens
            .push(ui.push_style_color(StyleColor::ButtonActive, [0.741, 0.933, 0.984, 1.0]));
        color_tokens
            .push(ui.push_style_color(StyleColor::ButtonHovered, [0.533, 0.851, 0.816, 1.0]));
        color_tokens.push(ui.push_style_color(StyleColor::FrameBg, [0.741, 0.933, 0.984, 1.0]));
        color_tokens.push(ui.push_style_color(StyleColor::WindowBg, [0.906, 0.784, 0.573, 1.0]));

        {
            let render_time = self.state.last_render_time;
            let frame_num = self.state.frame_num;
            let frame_timeouts = self.state.frame_timeout_count;
            let mouse_pos = self.state.mouse_pos;
            let render_canvas_size = self.state.render_window_size;
            let paused_state = self.state.paused;
            let mut pause_button_pressed = false;
            let titlebars_state = self.state.show_titlebar;
            let mut titlebar_button_pressed = false;
            let gui_width = self.size.width as f32 / self.hidpi_factor - 10.0;
            let mut create_painting_button_pressed = false;
            let painting_width = &mut self.state.painting_resolution.x;
            let painting_height = &mut self.state.painting_resolution.y;
            let _recording_width = &mut self.state.recording_resolution.x;
            let _recording_height = &mut self.state.recording_resolution.y;
            let movie_framerate = &mut self.state.movie_framerate;
            let mut painting_filename = ImString::with_capacity(256);
            let mut _recording_filename = ImString::with_capacity(256);
            let open_painting_externally = &mut self.state.open_painting_externally;
            let pause_while_painting = &mut self.state.pause_while_painting;
            let shader_compilation_error_msg = self.state.shader_compilation_error_msg.as_ref();
            let user_uniforms = &mut self.state.gui_uniforms;
            let mut _record_button_pressed = false;
            let _recorder = self.recorder.as_ref();

            painting_filename.push_str(&self.state.painting_filename);
            _recording_filename.push_str(&self.state.recording_filename);
            let mut painting_filename_changed = false;
            let mut _recording_filename_changed = false;
            let painting_in_progress = match &mut self.state.painting_progress_receiver {
                None => false,
                Some(rx) => {
                    let msg_result = rx.try_recv();
                    match msg_result {
                        Ok(_) => {
                            self.state.painting_progress_receiver = None;

                            // Log the amount of time render + write took.
                            if let Some(start) = self.state.painting_start_time {
                                let now = std::time::Instant::now();
                                let elapsed = now.duration_since(start).as_secs_f64();
                                info!("Painting render + write took {} seconds", elapsed);
                                self.state.painting_start_time = None;
                            }

                            // Send message to unpause the rendering.
                            if *pause_while_painting {
                                self.transmitter.send(DashboardMessage::Play).unwrap();
                            }
                            false
                        } // Finished.
                        Err(_) => true, // Still writing, hasn't reported status yet.
                    }
                }
            };
            let controls = imgui::Window::new(im_str!("Controls"));

            controls
                .size(
                    [
                        self.window.inner_size().width as f32 / self.hidpi_factor,
                        self.window.inner_size().height as f32 / self.hidpi_factor,
                    ],
                    Condition::Always,
                )
                .position([0.0, 0.0], Condition::Always)
                .collapsible(false)
                .no_decoration()
                .movable(false)
                .build(&ui, || {
                    if imgui::CollapsingHeader::new(im_str!("Stats & Controls"))
                        .default_open(true)
                        .open_on_arrow(true)
                        .open_on_double_click(true)
                        .build(&ui)
                    {
                        ui.text(format!("Render Time: {:.3} ms", render_time));
                        ui.text(format!("Frames Rendered: {}", frame_num));
                        ui.text(format!("Frame Timeouts: {}", frame_timeouts));
                        ui.text(im_str!(
                            "Mouse Position: ({:.1}, {:.1})",
                            mouse_pos.x,
                            mouse_pos.y
                        ));
                        ui.text(im_str!(
                            "Canvas Size: {} x {}",
                            render_canvas_size.x,
                            render_canvas_size.y
                        ));
                        ui.separator();
                        if paused_state {
                            pause_button_pressed = ui.button(im_str!("Play"), [gui_width, 25.0]);
                        } else {
                            pause_button_pressed = ui.button(im_str!("Pause"), [gui_width, 25.0]);
                        }
                        if titlebars_state {
                            titlebar_button_pressed =
                                ui.button(im_str!("Hide Titlebar"), [gui_width, 25.0]);
                        } else {
                            titlebar_button_pressed =
                                ui.button(im_str!("Show Titlebar"), [gui_width, 25.0]);
                        }
                    }

                    if imgui::CollapsingHeader::new(im_str!("Painting Options"))
                        .default_open(true)
                        .open_on_arrow(true)
                        .open_on_double_click(true)
                        .build(&ui)
                    {
                        ui.input_int(im_str!("Width##Painting"), painting_width)
                            .build();
                        ui.input_int(im_str!("Height##Painting"), painting_height)
                            .build();

                        let file_input =
                            ui.input_text(im_str!("Filename##Painting"), &mut painting_filename);
                        painting_filename_changed = file_input.build();
                        if cfg!(target_os = "macos") {
                            ui.checkbox(im_str!("Open in External App"), open_painting_externally);
                        }
                        ui.checkbox(im_str!("Pause While Painting"), pause_while_painting);
                        if !painting_in_progress {
                            create_painting_button_pressed =
                                ui.button(im_str!("Create"), [gui_width, 50.0]);
                        }
                    }

                    #[cfg(feature = "movie-recording")]
                    if imgui::CollapsingHeader::new(im_str!("Recording Options"))
                        .default_open(true)
                        .open_on_arrow(true)
                        .open_on_double_click(true)
                        .build(&ui)
                    {
                        ui.input_int(im_str!("Width##Movie"), _recording_width)
                            .build();
                        ui.input_int(im_str!("Height##Movie"), _recording_height)
                            .build();
                        ui.input_int(im_str!("Framerate##Movie"), movie_framerate)
                            .build();

                        let file_input =
                            ui.input_text(im_str!("Filename##Movie"), &mut _recording_filename);
                        _recording_filename_changed = file_input.build();
                        if let Some(rec) = _recorder {
                            if !rec.stop_signal_sent {
                                _record_button_pressed =
                                    ui.button(im_str!("Stop##Recording"), [gui_width, 25.0]);
                            }
                        } else {
                            _record_button_pressed =
                                ui.button(im_str!("Start##Recording"), [gui_width, 25.0]);
                        }
                    }
                    //---------------------------------
                    if !user_uniforms.is_empty() {
                        if imgui::CollapsingHeader::new(im_str!("Uniforms"))
                            .default_open(true)
                            .open_on_arrow(true)
                            .open_on_double_click(true)
                            .build(&ui)
                        {
                            for uniform in user_uniforms {
                                uniforms::update_user_uniform_ui(&ui, uniform);
                            }
                        }
                    }
                    //---------------------------------
                    ui.popup_modal(im_str!("Shader Recompilation")).build(|| {
                        if shader_compilation_error_msg.is_none() {
                            ui.close_current_popup();
                        }
                        ui.text_colored(
                            [1.0, 0.325, 0.286, 1.0],
                            im_str!("Error compiling shader."),
                        );
                        ui.text_wrapped(im_str!("See log for details."));
                    });
                    if shader_compilation_error_msg.is_some() {
                        ui.open_popup(im_str!("Shader Recompilation"));
                    }
                });
            if pause_button_pressed {
                self.state.paused = !self.state.paused;
                self.transmitter
                    .send(DashboardMessage::PausePlayChanged)
                    .unwrap();
            }
            if titlebar_button_pressed {
                self.state.show_titlebar = !self.state.show_titlebar;
                self.transmitter
                    .send(DashboardMessage::TitlebarStatusChanged)
                    .unwrap();
            }
            if painting_filename_changed {
                self.state.painting_filename = String::from(painting_filename.to_str());
            }
            if create_painting_button_pressed {
                if *pause_while_painting {
                    self.transmitter.send(DashboardMessage::Pause).unwrap();
                }
                self.transmitter
                    .send(DashboardMessage::PaintingRenderRequested(UIntVector2::new(
                        self.state.painting_resolution.x as u32,
                        self.state.painting_resolution.y as u32,
                    )))
                    .unwrap();
            }
            if _recording_filename_changed {
                self.state.recording_filename = String::from(_recording_filename.to_str());
            }
            if _record_button_pressed {
                if self.recorder.is_none() {
                    self.recorder = Some(Recorder::new(
                        self.state.recording_resolution.x as u32,
                        self.state.recording_resolution.y as u32,
                        MOVIE_TEXTURE_FORMAT,
                        *movie_framerate as u32,
                        format!("{}.mp4", self.state.recording_filename),
                    ));
                } else {
                    let recorder = self.recorder.as_mut().unwrap();
                    recorder.stop();
                }
            }
        }

        while !color_tokens.is_empty() {
            let token = color_tokens.pop().unwrap();
            token.pop(&ui);
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("GUI Command Encoder"),
            });
        self.imgui_platform.prepare_render(&ui, &self.window);

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                    attachment: &frame.output.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.clear_color),
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
            });

            self.imgui_renderer
                .render(ui.render(), &self.queue, &self.device, &mut rpass)
                .expect("GUI Rendering Failed");
        }

        self.queue.submit(Some(encoder.finish()));
    }
}
