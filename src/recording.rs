use crate::{
    utils,
    vector::{IntVector2, UIntVector2},
};
use futures::executor::block_on;
use log::info;
use std::io::{self, Write};
use std::process::Stdio;
use std::process::{Child, Command};
use std::thread::JoinHandle;
use wgpu::TextureFormat;

enum RecorderThreadSignal {
    Stop,
    Frame(wgpu::Buffer, UIntVector2),
}

pub struct Recorder {
    width: u32,
    height: u32,
    framerate: u32,
    texture_format: TextureFormat,
    filename: String,
    join_handle: JoinHandle<()>,
    message_transmitter: std::sync::mpsc::Sender<RecorderThreadSignal>,
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
                "-s:v",
                &resolution_string,
                "-framerate",
                &framerate.to_string(),
                "-pix_fmt",
                pix_fmt,
                "-i",
                "-",
                "-f",
                "h264",
                "-c:v",
                "h264_videotoolbox",
                &filename,
            ])
            .stdin(Stdio::piped())
            // .stdout(Stdio::piped())
            // .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let (transmitter, receiver) = std::sync::mpsc::channel();

        let join_handle = std::thread::spawn(move || {
            let pipe_in = ffmpeg_process.stdin.as_mut().unwrap();

            loop {
                let msg = receiver.recv().unwrap();
                match msg {
                    RecorderThreadSignal::Stop => {
                        info!("Stop Signal received, broke out of loop.");
                        break;
                    }
                    RecorderThreadSignal::Frame(buf, res) => {
                        // let pixel_size = Self::bytes_per_pixel_for_texture_format(self.texture_format);
                        let pixel_data = block_on(utils::transcode_painting_data(buf, res));
                        // info!("Writing frame data to FFmpeg pipe.");
                        pipe_in.write_all(&pixel_data).unwrap();
                        // info!("Finished writing frame data to FFmpeg pipe.");
                    }
                }
            }

            let output = ffmpeg_process
                .wait_with_output()
                .expect("Failed to wait on FFmpeg process");

            println!("FFMpeg status: {}", output.status);
            // std::io::stdout().write_all(&output.stdout).unwrap();
            // std::io::stderr().write_all(&output.stderr).unwrap();
        });

        Recorder {
            width,
            height,
            texture_format,
            framerate,
            filename,
            join_handle,
            message_transmitter: transmitter,
        }
    }

    pub fn add_frame(
        &self,
        buffer: wgpu::Buffer,
        resolution: UIntVector2,
        _timestamp: std::time::Instant,
    ) {
        self.message_transmitter
            .send(RecorderThreadSignal::Frame(buffer, resolution))
            .unwrap();
    }

    pub fn stop(&self) {
        self.message_transmitter
            .send(RecorderThreadSignal::Stop)
            .unwrap();
    }

    pub fn finish(self) {
        self.join_handle.join().unwrap();
    }
}
