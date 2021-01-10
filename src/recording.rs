use crate::{utils, vector::IntVector2};
use ffmpeg::{encoder, rational::Rational, util::color::Space, util::format::Pixel};
use ffmpeg_next as ffmpeg;
use std::time::Instant;

pub fn init_recording() {
    ffmpeg::init().unwrap();
}

pub struct Recorder {
    encoder: encoder::video::Video,
    frame_count: usize,
    start_time: Instant,
    width: u32,
    height: u32,
    pixel_format: Pixel,
}

impl Recorder {
    pub fn new(width: u32, height: u32, format: Pixel) -> Recorder {
        let mut encoder = ffmpeg::codec::encoder::new();
        let mut video_encoder = encoder.video().unwrap();
        video_encoder.set_format(format);
        video_encoder.set_width(width);
        video_encoder.set_height(height);
        // We assume some features of the encoder for now.
        video_encoder.set_colorspace(Space::RGB);
        video_encoder.set_frame_rate(Some(Rational::new(1, 60)));
        video_encoder.set_time_base(Rational::new(1, 60));

        Recorder {
            encoder: video_encoder,
            frame_count: 0,
            start_time: Instant::now(),
            width,
            height,
            pixel_format: format,
        }
    }

    async fn encode_frame(&mut self, buffer: wgpu::Buffer, resolution: IntVector2) {
        // Create an FFmpeg frame.
        let mut frame = ffmpeg::util::frame::Video::new(self.pixel_format, self.width, self.height);
        let pixel_data = utils::transcode_painting_data(buffer, resolution).await;
        let frame_data = frame.data_mut(0);
    }
}
