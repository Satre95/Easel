use crate::{
    uniforms, utils,
    vector::{IntVector2, UIntVector2},
};
use futures::{executor::block_on, future::join};
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
    join_handle: JoinHandle<()>,
    sender: std::sync::mpsc::SyncSender<RecorderThreadSignal>,
    receiver: std::sync::mpsc::Receiver<bool>,
    pub done: bool,
    pub stop_signal_sent: bool,
}

impl Recorder {
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
        let buf_size: usize = 60 * 16 * width as usize * height as usize;
        let (our_sender, thread_receiver) = std::sync::mpsc::sync_channel(buf_size);
        let (thread_sender, our_receiver) = std::sync::mpsc::channel();
        let join_handle = std::thread::spawn(move || {
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
                    // "-hwaccel",
                    // "cuda",
                    "-i",
                    "-",
                    // "-c:v",
                    // "libx264",
                    "-c:v",
                    "h264_nvenc",
                    "-pix_fmt",
                    "yuv420p",
                    // "-profile",
                    // "high444p",
                    // "-crf",
                    // "20",
                    &filename,
                ])
                .stdin(Stdio::piped())
                // .stdout(Stdio::piped())
                // .stderr(Stdio::piped())
                .spawn()
                .unwrap();

            let mut pixel_data = Vec::<u8>::new();
            loop {
                let msg = thread_receiver.recv().unwrap();
                match msg {
                    RecorderThreadSignal::Stop => {
                        info!("Stop signal received.");
                        break;
                    }
                    RecorderThreadSignal::Frame(buffer, resolution) => {
                        let pipe_in = ffmpeg_process.stdin.as_mut().unwrap();
                        block_on(utils::transcode_painting_data(
                            buffer,
                            resolution,
                            true,
                            &mut pixel_data,
                        ));
                        pipe_in.write_all(&pixel_data).unwrap();
                    }
                }
            }

            ffmpeg_process.stdin.as_mut().unwrap().flush().unwrap();
            let output = ffmpeg_process
                .wait_with_output()
                .expect("Failed to wait on FFmpeg process");

            info!("FFMpeg finished with status: {}", output.status);
            thread_sender.send(true).unwrap();
            // std::io::stdout().write_all(&output.stdout).unwrap();
            // std::io::stderr().write_all(&output.stderr).unwrap();
        });

        Recorder {
            join_handle,
            sender: our_sender,
            receiver: our_receiver,
            done: false,
            stop_signal_sent: false,
        }
    }

    pub fn poll(&mut self) -> bool {
        let msg_result = self.receiver.try_recv();
        match msg_result {
            Ok(done) => self.done = done,
            Err(_) => {}
        }
        self.done
    }

    pub fn add_frame(
        &self,
        buffer: wgpu::Buffer,
        resolution: UIntVector2,
        _timestamp: std::time::Instant,
    ) {
        self.sender
            .send(RecorderThreadSignal::Frame(buffer, resolution))
            .unwrap();
    }

    pub fn stop(&mut self) {
        if self.stop_signal_sent {
            panic!("Attempting to request stop on recorder that has already stopped!");
        }
        info!("Sending stop signal to FFMpeg.");
        self.sender.send(RecorderThreadSignal::Stop).unwrap();
        self.stop_signal_sent = true;
    }

    pub fn finish(self) {
        self.join_handle.join().unwrap();
    }
}
