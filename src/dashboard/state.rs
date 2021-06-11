use crate::{
    uniforms::UserUniform,
    utils::WriteFinished,
    vector::{IntVector2, Vector2},
};
use std::{sync::mpsc::Receiver, usize};

/// Struct containing information the GUI is displaying and interacting with.
pub struct DashboardState {
    pub last_render_time: f64,
    pub frame_num: usize,
    pub frame_timeout_count: usize,
    pub mouse_pos: Vector2,
    pub render_window_size: IntVector2,
    pub paused: bool,
    pub show_titlebar: bool,
    pub painting_resolution: IntVector2,
    pub recording_resolution: IntVector2,
    pub painting_filename: String,
    pub recording_filename: String,
    pub recording_in_progress: bool,
    /// Unit: seconds
    pub movie_framerate: i32,
    /// Only available on macOS.
    pub open_painting_externally: bool,
    pub pause_while_painting: bool,
    pub painting_progress_receiver: Option<Receiver<WriteFinished>>,
    pub shader_compilation_error_msg: Option<String>,
    pub painting_start_time: Option<std::time::Instant>,
    pub gui_uniforms: Vec<UserUniform>,
}

impl DashboardState {
    pub fn new() -> DashboardState {
        DashboardState {
            last_render_time: 0.0,
            frame_num: 0,
            frame_timeout_count: 0,
            mouse_pos: Vector2::zero(),
            render_window_size: IntVector2::zero(),
            paused: false,
            show_titlebar: true,
            painting_resolution: IntVector2::zero(),
            recording_resolution: IntVector2::new(1024, 1024),
            painting_filename: String::from("Painting"),
            recording_filename: String::from("Muybridge"),
            recording_in_progress: false,
            movie_framerate: 60,
            open_painting_externally: true,
            pause_while_painting: true,
            painting_progress_receiver: None,
            shader_compilation_error_msg: None,
            painting_start_time: None,
            gui_uniforms: Vec::new(),
        }
    }
}
