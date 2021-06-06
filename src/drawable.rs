pub trait Drawable {
    fn input(&mut self, incoming_event: &winit::event::WindowEvent);
    fn window_id(&self) -> winit::window::WindowId;
}
