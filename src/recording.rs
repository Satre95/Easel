use ffmpeg::{encoder, rational::Rational, util::color::Space, util::format::Pixel};
use ffmpeg_next as ffmpeg;
use std::time::Instant;

pub fn init_recording() {
    ffmpeg::init().unwrap();
}

pub struct Recorder {
    encoder: encoder::video::Video,
    frame_count: usize,
    start_time: std::time::Instant,
    width: usize,
    height: usize,
}

impl Recorder {
    pub fn new(width: usize, height: usize, format: Pixel) -> Recorder {
        let mut encoder = ffmpeg::codec::encoder::new();
        let mut video_encoder = encoder.video().unwrap();
        video_encoder.set_format(format);
        video_encoder.set_width(width as u32);
        video_encoder.set_height(height as u32);
        // We assume some features of the encoder for now.
        video_encoder.set_colorspace(Space::RGB);
        video_encoder.set_frame_rate(Some(Rational::new(1, 60)));
        video_encoder.set_time_base(Rational::new(1, 60));

        Recorder {
            encoder: video_encoder,
            frame_count: 0,
            start_time: std::time::Instant::now(),
            width,
            height,
        }
    }
}
