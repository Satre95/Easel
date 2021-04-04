use crate::push_constants::load_push_constants_from_json;
use crate::uniforms::load_uniforms_from_json;
use std::sync::mpsc::channel;

use super::message::CanvasMessage;
use super::{Canvas, PAINTING_TEXTURE_FORMAT, RENDER_TEXTURE_FORMAT, VS_MODULE_BYTES};
use crate::postprocessing::PostProcess;
use crate::recording::MOVIE_TEXTURE_FORMAT;
use log::{error, info, warn};
use notify::{DebouncedEvent, Watcher};

impl Canvas {
    /// Reload shader from disk and update render pipelines
    pub fn update_shader_pipeline(&mut self, event: DebouncedEvent) {
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
                    .create_shader_module(&wgpu::ShaderModuleDescriptor {
                        label: Some("Vertex Shader"),
                        source: wgpu::util::make_spirv(&fs_spirv_data),
                        flags: wgpu::ShaderFlags::VALIDATION,
                    });
                let vs_module = self
                    .device
                    .create_shader_module(&wgpu::ShaderModuleDescriptor {
                        label: Some("Vertex Shader"),
                        source: wgpu::util::make_spirv(VS_MODULE_BYTES),
                        flags: wgpu::ShaderFlags::VALIDATION,
                    });

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
                let (render_pipeline, painting_pipeline, movie_pipeline) =
                    crate::utils::create_pipelines(
                        &self.device,
                        &render_pipeline_layout,
                        &vs_module,
                        &fs_module,
                        (
                            RENDER_TEXTURE_FORMAT,
                            PAINTING_TEXTURE_FORMAT,
                            MOVIE_TEXTURE_FORMAT,
                        ),
                    );

                self.render_pipeline = render_pipeline;
                self.painting_pipeline = painting_pipeline;
                self.movie_pipeline = movie_pipeline;

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
    pub fn update_custom_uniforms_from_file(&mut self, event: DebouncedEvent) {
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
}
