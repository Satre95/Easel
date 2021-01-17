use crate::{utils, vector::UIntVector2};
use std::io::{self, Write};
use std::process::Stdio;
use std::process::{Child, Command};
use wgpu::TextureFormat;

pub struct Recorder {
    width: u32,
    height: u32,
    framerate: u32,
    texture_format: TextureFormat,
    ffmpeg_process: Child,
    filename: String,
}

impl Recorder {
    pub fn bytes_per_pixel_for_texture_format(format: TextureFormat) -> usize {
        match format {
            TextureFormat::Rgba16Uint => 8,
            TextureFormat::Rgba8Unorm => 4,
            _ => panic!("Unsupported texture format. Only the following texture formats are supported: Rgba16Uint, Rgba8Unorm")
        }
    }

    pub fn new(
        width: u32,
        height: u32,
        texture_format: TextureFormat,
        framerate: u32,
        filename: String,
    ) -> Recorder {
        let pix_fmt = match texture_format{
            TextureFormat::Rgba16Float => "rgb48le",
            _ => panic!("Unsupported texture format. Only the following texture formats are supported: Rgba16Float")
        };
        let resolution_string = format!("{}x{}", width.to_string(), height.to_string());
        let mut ffmpeg_process = Command::new("ffmpeg")
            .args(&[
                "-y",
                "-f",
                "rawvideo",
                "-r",
                &framerate.to_string(),
                "-s",
                &resolution_string,
                "-pix_fmt",
                pix_fmt,
                "-i",
                "pipe:0",
                &filename,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Recorder {
            width,
            height,
            texture_format,
            framerate,
            ffmpeg_process,
            filename,
        }
    }

    pub async fn add_frame(&mut self, buffer: wgpu::Buffer, resolution: UIntVector2) {
        // let pixel_size = Self::bytes_per_pixel_for_texture_format(self.texture_format);
        let pixel_data = utils::transcode_painting_data(buffer, resolution).await;
        let pipe_in = self.ffmpeg_process.stdin.as_mut().unwrap();
        pipe_in.write_all(&pixel_data).unwrap();
    }

    pub fn finish(mut self) {
        // Wait for child process to finish execution then collect output.
        // This also closes the stdin channel before waiting.
        let output = self
            .ffmpeg_process
            .wait_with_output()
            .expect("Failed to wait on FFmpeg process");

        println!("FFMpeg status: {}", output.status);
        std::io::stdout().write_all(&output.stdout).unwrap();
        std::io::stderr().write_all(&output.stderr).unwrap();
    }
}
