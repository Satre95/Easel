use crate::uniforms::UserUniform;
use crate::vector::{IntVector2, Vector2};

/// Message Enums used by [crate::canvas::Canvas] to send messages to interested parties.
pub enum CanvasMessage {
    /// Signifies a new frame has been dispatch for rendering (not a painting draw call)
    RenderPassSubmitted,
    /// Mouse has moved to a new location in the window.
    MouseMoved(Vector2),
    /// Frame has been rendered
    FrameStep,
    /// Error with swapchain.
    SwapChainFrameError(wgpu::SwapChainError),
    /// Contains new window size.
    WindowResized(IntVector2),
    /// A painting render operation has been dispatched.
    /// The buffer will contain the painting data once rendering finishes.
    /// The IntVector2 is the resolution of the painting.
    /// The Instant is the time point at which this render operation started.
    PaintingStarted(wgpu::Buffer, IntVector2, std::time::Instant),
    /// A movie frame render operation has been dispatched.
    /// The buffer will contain the frame data once rendering finishes.
    /// The IntVector2 is the resolution of the frame.
    /// The Instant is the time point at which this render operation started.
    MovieFrameStarted(wgpu::Buffer, IntVector2, std::time::Instant),
    /// Signifies shader reloaded from disk, recompiled, and render pipeline has been updated.
    ShaderCompilationSucceeded,
    /// Error reloading shader, contains error message.
    ShaderCompilationFailed(String),
    /// Indication pause play state changed from canvas window.
    PausePlayChanged,
    /// Used by Canvas to tell Dashboard how to build the editor GUI for a given custom uniform.
    UniformForGUI(Box<dyn UserUniform>),
    /// Change the resolution of the painting in the GUI.
    UpdatePaintingResolutioninGUI(IntVector2),
}
