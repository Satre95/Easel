use crate::{utils, vector::UIntVector2};
use futures::executor::block_on;
use log::info;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use wgpu::TextureFormat;

pub static MOVIE_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

enum RecorderToThreadSignal {
    Stop,
    Frame(wgpu::Buffer, UIntVector2),
}

enum ThreadToRecorderSignal {
    Ready,
    Finished,
}

pub struct Recorder {
    join_handle: JoinHandle<()>,
    sender: std::sync::mpsc::Sender<RecorderToThreadSignal>,
    receiver: std::sync::mpsc::Receiver<ThreadToRecorderSignal>,
    pub done: bool,
    pub ready: bool,
    stop_signal_received: bool,
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
            TextureFormat::Rgba8UnormSrgb => "rgba",
            _ => panic!("Unsupported texture format. Only the following texture formats are supported: Rgba8UnormSrgb")
        };
        let resolution_string = format!("{}x{}", width.to_string(), height.to_string());
        let (our_sender, thread_receiver) = std::sync::mpsc::channel();
        let (thread_sender, our_receiver) = std::sync::mpsc::channel();
        let framerate_str = framerate.to_string();
        let join_handle = std::thread::spawn(move || {
            let mut args = vec![
                "-hide_banner",
                "-y",
                "-f",
                "rawvideo",
                "-framerate",
                &framerate_str,
                "-video_size",
                &resolution_string,
                "-pixel_format",
                pix_fmt,
            ];
            if cfg!(target_os = "windows") {
                args.extend_from_slice(&[
                    "-hwaccel",
                    "cuda",
                    "-i",
                    "-",
                    "-c:v",
                    "hevc_nvenc",
                    "-preset",
                    "2", // medium
                    "-pix_fmt",
                    "yuv420p",
                    "-r",
                    &framerate_str,
                    &filename,
                ]);
            } else {
                args.extend_from_slice(&[
                    "-i",
                    "-",
                    "-c:v",
                    "libx265",
                    "-pix_fmt",
                    "yuv420p",
                    "-x265-params",
                    "lossless=1",
                    "-r",
                    &framerate_str,
                    &filename,
                ]);
            }
            let mut ffmpeg_process = Command::new("ffmpeg")
                .args(&args)
                .stdin(Stdio::piped())
                .spawn()
                .unwrap();

            // Notify Recorder struct that we are ready to start receiving frames.
            thread_sender.send(ThreadToRecorderSignal::Ready).unwrap();

            let mut pixel_data = Vec::<u8>::new();
            let mut frame_count: usize = 0;
            loop {
                let msg = thread_receiver.recv().unwrap();
                match msg {
                    RecorderToThreadSignal::Stop => {
                        info!("Stop signal received.");
                        break;
                    }
                    RecorderToThreadSignal::Frame(buffer, resolution) => {
                        let pipe_in = ffmpeg_process.stdin.as_mut().unwrap();
                        block_on(utils::transcode_frame_data_for_movie(
                            buffer,
                            resolution,
                            &mut pixel_data,
                        ));
                        pipe_in.write_all(&pixel_data).unwrap();
                        frame_count += 1;
                        pixel_data.clear();
                    }
                }
            }

            ffmpeg_process.stdin.as_mut().unwrap().flush().unwrap();
            let output = ffmpeg_process
                .wait_with_output()
                .expect("Failed to wait on FFmpeg process");

            info!(
                "FFMpeg processed {} frames and finished with status: {}",
                frame_count, output.status
            );
            thread_sender
                .send(ThreadToRecorderSignal::Finished)
                .unwrap();
            // std::io::stdout().write_all(&output.stdout).unwrap();
            // std::io::stderr().write_all(&output.stderr).unwrap();
        });

        Recorder {
            join_handle,
            sender: our_sender,
            receiver: our_receiver,
            done: false,
            ready: false,
            stop_signal_received: false,
        }
    }

    /// Whether this recorder has finished processing all frames.
    pub fn poll(&mut self) -> bool {
        let msg_result = self.receiver.try_recv();
        match msg_result {
            Ok(signal) => match signal {
                ThreadToRecorderSignal::Finished => self.done = true,
                ThreadToRecorderSignal::Ready => self.ready = true,
            },
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
            .send(RecorderToThreadSignal::Frame(buffer, resolution))
            .unwrap();
    }

    pub fn stop(&mut self) {
        if self.stop_signal_received {
            panic!("Attempting to request stop on recorder that has already stopped!");
        }
        info!("Sending stop signal to FFMpeg.");
        self.sender.send(RecorderToThreadSignal::Stop).unwrap();
        self.stop_signal_received = true;
    }

    pub fn finish(self) {
        self.join_handle.join().unwrap();
    }
}

// impl Future for Recorder {
//     type Output = Recorder;

//     fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {}
// }
