//! # Easel
//! Easel is a shader playground for creating high-quality digital paintings for printing.
//! To this end, Easel intentionally uses high bitrate textures during the render process even though they are less memory and compute efficient.
//! Easel is designed to be part of a workflow where you may want to further edit your digital paintings in an image editing program for later printing and display.
//! Paintings are rendered using 16-bits-per-component textures and written to disk as uncompressed high-res 16-bit TIFF files.
//!
//! While rendering to screen, lower bitrate textures are used for efficiency.
//! However, when the `Create Painting` button is pressed, a separte render pipeline utilising 16-bit textures is run to create the digital painting.
//! Please note that using high bitrate texture such as these consumes large amounts of memory.
//! The maximum painting resolution is determined by the amount of memory in your GPU.
//! Attempting to use more than this will cause the program to crash.
//!
//! Easel is designed to be cross-platform and run on Windows, macOS, and Linux.
//! It uses [wgpu] as the render backend and [imgui] for the GUI.
//!
//! # Getting Started
//! Easel expects the shaders and bindings to follow a certain format. To get started, use the `--generate` option to create a basic shader.
//!
//! # Usage
//! Easel supports rendering either text source fragment shaders or compiled SPIR-V modules. If providing a text shader, the extension must be ".frag".
//! If providing a compiled shader, the file extension must be ".spv".
//!
//! Shaders must be written in Vulkan GLSL. However, thanks to the [shaderc] and [wgpu] crates, Easel can run these shaders even on platforms without Vulkan (eg macOS).
//! Easel automatically performs the shader translation for you.
//!
//! ## Uniforms & Push Constants
//! Easel automatically provides the following uniform data to all shaders:
//!
//!   - Viewport resolution in pixels.
//!   - Time in seconds since program start.
//!   - Time in seconds since last frame.
//!   - Current render frame count (starts at 0)
//!   - Current mouse position + mouse position in the previous frame.
//!
//! Use the skeleton shader as a reference for the order and bindings for these uniforms.
//!
//! Additionally, Easel supports providing extra uniform and/or push constant data via a JSON file.
//! This can be useful in the case where a shader is provided as a compiled binary, but you want to control its behaviour with certain uniforms.
//! Push constants can also be specified in this file.
//!
//! The JSON file must follow a specific format where each uniform indicates its type explicitly. Below is an example.
//! ```text
//! {
//!     "push constants": {
//!         "samples per pixel": [ "u32", 2 ]
//!     },
//!     "uniforms": {
//!         "antialiasing": ["bool", true]   
//!     }
//! }
//! ````
//!
//! At this time, the only the following data types are supported for push constants and uniforms: `i32`, `i64`, `f32`, `f64`, `u32`, `u64`, and `bool`.
//! **Note:** `bool` uniforms are bound as `u32` in shaders to respect alignment constraints.
//!
//! ### Binding Order
//! Easel-provided uniforms are always bound to set 0, binding 0. If you also provide uniforms, they are bound to set 0 binding 1.
//! The order of bindings within the set is the same as the order in the JSON file. For example:
//! ```text
//! layout(set = 0, binding = 0) uniform EaselUniforms {
//!     vec4 u_resolution;
//!     float u_time;
//!     float u_time_delta;
//!     uint u_frame_num;
//!     vec4 u_mouse_info;
//! };
//! layout(set = 0, binding = 1) uniform MyUniforms { bool antialiasing; };
//!```
//!
//! ## Texture Loading
//! Up to [wgpu::Limits::max_sampled_textures_per_shader_stage] images can be loaded and bound as input textures to the fragment shader using the `-t` option.
//!
//! ### Binding Order
//! In the shader, all textures are bound in set 1. At binding location 0 in the set is the sampler, followed by each texture.
//! For example:
//! ```text
//! layout(set = 1, binding = 0) uniform sampler sampler_0;
//! layout(set = 1, binding = 1) uniform texture2D texture_0;
//! layout(set = 1, binding = 2) uniform texture2D texture_1;
//! ```
//!
//! At this time, only PNG images are supported. Support for other formats is planned to be implemented in the next release.
//!
//! ## Postprocessing Effects
//! If you would like to run postprocessing effects and/or chain multiple shaders together, use the `-p` option.
//! Multiple shaders can be provided and shaders are run in order. Post-processing effects are applied to both on and off screen renders.
//! These shaders can also be provided as source text, compiled SPIR-V, or both.
//!
//! ## Live Coding
//! If you would like to live-code your shaders, Easel also supports auto-loading of both the shader file and the JSON file.
//! This works for both text shaders and SPIR-V blobs. Auto-reloading of postprocessing shaders is not supported at this time.
//!
//! # Help
//! Run `easel --help` to see all options and instructions.
//!
//! # Log
//! By default Easel will log errors and warnings to the console that launched it. If you would like to see more detailed logs, set the environment variable
//! `RUST_LOG=easel=<log_level>` before launching the program. Logging functionality is implemented using the [env_logger] crate.
//!
//! # Platform Specific Features
//! When built for macOS, Easel also has the option to automatically open rendered paintings in the default system image viewer.
//! This option can be toggled in the GUI.

mod canvas;
mod dashboard;
// mod drawable;
mod postprocessing;
mod push_constants;
mod recording;
mod skeletons;
mod texture;
mod uniforms;
mod utils;
mod vector;

use clap::{App, Arg};
use futures::executor::block_on;
use log::error;
use winit::{
    event::*,
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

use crate::{
    canvas::CanvasMessage,
    dashboard::{Dashboard, DashboardMessage},
};
use canvas::Canvas;
use std::{cmp::max, time::Instant};
use std::{collections::HashMap, fs, path::Path};
use std::{sync::mpsc::sync_channel, thread};
use winit::dpi::PhysicalSize;

static UPDATE_INTERVAL_MS: u128 = 16;

enum EventThreadMessage {
    Tick,
    SystemEvent(winit::event::Event<'static, ()>),
    // Exit,
}

fn main() {
    env_logger::init();
    // Load command line args.
    let matches = setup_program_args();

    if let Some(shader_file) = matches.value_of("shader") {
        if matches.is_present("generate") {
            let path = std::path::Path::new(shader_file);
            if path.exists() {
                error!(
                    "There is already a file present at {}, canceling write.",
                    shader_file
                );
                return;
            }
            std::fs::write(&path, skeletons::SHADER_SKELETON).unwrap();
        }

        let fs_spv_data = match utils::load_shader(shader_file) {
            Ok(data) => data,
            Err(e) => {
                error!("Error compiling/loading shader: {}", e);
                return;
            }
        };

        // Get textures to load, if any
        let mut images_to_load: Vec<String> = Vec::new();
        if let Some(files) = matches.values_of("textures") {
            for a_file in files {
                images_to_load.push(String::from(a_file));
            }
        }
        // Set width & height, if specified.
        let mut canvas_width = 1920;
        let mut canvas_height = 1280;
        if let Some(width) = matches.value_of("width") {
            canvas_width = width.parse::<i32>().unwrap()
        }
        if let Some(height) = matches.value_of("height") {
            canvas_height = height.parse::<i32>().unwrap()
        }

        // Load custom uniforms from JSON file if specified.
        let mut custom_uniforms = None;
        let mut push_constants = None;
        if let Some(uniforms_file) = matches.value_of("uniforms") {
            let text =
                fs::read_to_string(uniforms_file).expect("Error reading uniforms from file.");
            let json_data = json::parse(&text).expect("Error parsing JSON.");
            let cu = uniforms::load_uniforms_from_json(&json_data);
            if !cu.is_empty() {
                custom_uniforms = Some(cu);
            }
            let pc = push_constants::load_push_constants_from_json(&json_data);
            if !pc.is_empty() {
                push_constants = Some(pc);
            }
        }

        // Setup the render window.
        let event_loop = EventLoop::new();
        let render_window = WindowBuilder::new().build(&event_loop).unwrap();
        render_window.set_title("Canvas");
        render_window.set_inner_size(PhysicalSize::new(canvas_width, canvas_height));
        render_window.set_decorations(true);
        render_window.set_resizable(true);
        let mut images: Vec<image::DynamicImage> = Vec::new();
        for a_file in &images_to_load {
            let an_image = image::open(Path::new(a_file));
            match an_image {
                Ok(img) => images.push(img),
                Err(error) => {
                    error!("Error loading image: {}", error);
                    return;
                }
            }
        }

        // Setup channels for Dashboard <--> Canvas communication
        let (dashboard_tx, state_rx) = sync_channel::<DashboardMessage>(1024);
        let (state_tx, dashboard_rx) = sync_channel::<CanvasMessage>(1024);

        let mut drawables = HashMap::new();
        let mut window_ids = vec![render_window.id()];
        // Setup render state.
        let mut canvas = Box::new(block_on(Canvas::new(
            render_window,
            fs_spv_data,
            Some(images),
            custom_uniforms,
            push_constants,
            state_tx,
            state_rx,
        )));
        // Make channels for sending events to Canvas
        let (canvas_event_tx, canvas_event_rx) = sync_channel::<EventThreadMessage>(24);
        drawables.insert(canvas.window.id(), canvas_event_tx);

        // Setup post-processing shaders if specified
        if let Some(postprocess_shaders) = matches.values_of("postprocess") {
            let mut postprocess_shader_modules = Vec::with_capacity(postprocess_shaders.len());
            for shader in postprocess_shaders {
                postprocess_shader_modules.push(utils::load_shader(shader).unwrap());
            }
            for module in postprocess_shader_modules {
                canvas.add_post_processing_shader(module);
            }
        }

        // Setup auto-updating, if specified.
        if let Some(interval_str) = matches.value_of("auto-update") {
            let interval = max(
                interval_str
                    .parse::<u64>()
                    .expect("Invalid update interval provided. Must be integer"),
                80,
            );
            canvas.watch_shader_file(shader_file, interval);
            // If also given custom uniforms, start watching that file.
            if let Some(uniforms_file) = matches.value_of("uniforms") {
                canvas.watch_uniforms_file(uniforms_file, interval);
            }
        }

        // Setup another window for Dashboard
        let dashboard_window_builder = WindowBuilder::new().with_resizable(true);
        let dashboard_window = dashboard_window_builder.build(&event_loop).unwrap();
        dashboard_window.set_title("Dashboard");
        dashboard_window.set_inner_size(PhysicalSize::new(500, 1250));
        window_ids.push(dashboard_window.id());
        // Setup Dashboard
        let mut dashboard = block_on(Dashboard::new(dashboard_window, dashboard_tx, dashboard_rx));
        // Make channels for sending events to Dashboard
        let (dashboard_event_tx, dashboard_event_rx) = sync_channel::<EventThreadMessage>(24);
        drawables.insert(dashboard.window.id(), dashboard_event_tx);

        thread::spawn(move || {
            while let Ok(thread_event) = canvas_event_rx.recv() {
                match thread_event {
                    EventThreadMessage::Tick => {
                        canvas.update();
                        canvas.render_canvas();
                        canvas.post_render();
                    }
                    EventThreadMessage::SystemEvent(event) => canvas.input(&event),
                }
            }
        });

        let mut last_render_time = Instant::now();
        event_loop.run(move |event, _, control_flow| {
            dashboard.input(&event);
            match event {
                Event::RedrawRequested(_) => {}
                Event::MainEventsCleared => {
                    let now = Instant::now();
                    let delta = (now - last_render_time).as_millis();
                    if delta >= UPDATE_INTERVAL_MS {
                        dashboard.update();
                        dashboard.render_dashboard();
                        dashboard.post_render();

                        last_render_time = now;
                    }
                }
                Event::WindowEvent { ref event, .. } => match event {
                    WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                    WindowEvent::KeyboardInput { input, .. } => match input {
                        KeyboardInput {
                            state: ElementState::Pressed,
                            virtual_keycode: Some(VirtualKeyCode::Escape),
                            ..
                        } => {
                            canvas.exit_requested();
                            *control_flow = ControlFlow::Exit
                        }
                        _ => {}
                    },
                    _ => {}
                },
                _ => {}
            }
        });
    } else {
        error!("Please provide a fragment shader.")
    }
}

/// Sets up all arguments to be parsed by Easel
fn setup_program_args() -> clap::ArgMatches {
    App::new("Easel")
        .version("1.0.1")
        .author("Siddharth A. <sid.atre@me.com>")
        .arg(
            Arg::new("shader")
                .about("The fragment shader to use.")
                .index(1)
                .required(true),
        )
        .arg(
            Arg::new("textures")
                .long_about("List of images to load. Textures are bound to the shader Set 1 in the order specified here.")
                .required(false)
                .takes_value(true)
                .short('t')
                .long("textures")
                .multiple(true)
        )
        .arg(
            Arg::new("width")
                .about("Width of canvas")
                .required(false)
                .takes_value(true)
                .short('w')
                .long("width")
                .default_value("1920")
        )
        .arg(
            Arg::new("height")
                .about("Height of canvas")
                .required(false)
                .takes_value(true)
                .short('h')
                .long("height")
                .default_value("1280")
        )
        .arg(
            Arg::new("auto-update")
                .long_about("Check the shader and/or uniforms files on this interval (ms). If changed, updates render pipelines. Default is the minimum.")
                .required(false)
                .takes_value(true)
                .short('a')
                .default_value("80")
                .long("auto-update")
        )
        .arg(
            Arg::new("uniforms")
                .long_about("Provide a JSON file with custom uniforms. Uniforms are bound in Set 2 in the order provided.")
                .required(false)
                .takes_value(true)
                .short('u')
                .long("uniforms")
        )
        .arg(Arg::new("postprocess")
            .long_about("Provided a shader to run after main fragment shader. Multiple can be provided. Postprocessing operations are applied in the order given here.")
            .required(false)
            .takes_value(true)
            .multiple(true)
            .short('p')
            .long("postprocess"))
        .arg(Arg::new("generate")
            .long_about("Generate a basic skeleton for an Easel shader. The shader is written to disk and then loaded.")
            .required(false)
            .short('g')
            .long("generate")
        )
        .get_matches()
}
