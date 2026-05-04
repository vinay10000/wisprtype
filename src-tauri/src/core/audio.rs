use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;
use std::sync::{Arc, Mutex};

pub struct AudioCapturer {
    stream: Option<Stream>,
    pub buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
}

impl AudioCapturer {
    pub fn new() -> Self {
        Self {
            stream: None,
            buffer: Arc::new(Mutex::new(Vec::new())),
            sample_rate: 16000,
            channels: 1,
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        self.clear();
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or("No input device available")?;
        let config = device.default_input_config().map_err(|e| e.to_string())?;

        self.sample_rate = config.sample_rate().0;
        self.channels = config.channels();

        let sample_format = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        
        let buffer_arc = self.buffer.clone();
        
        let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

        let channels = self.channels;
        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config,
                move |data: &[f32], _: &_| {
                    if let Ok(mut buf) = buffer_arc.lock() {
                        if channels == 1 {
                            buf.extend_from_slice(data);
                        } else {
                            for chunk in data.chunks(channels as usize) {
                                let sum: f32 = chunk.iter().sum();
                                buf.push(sum / chunk.len() as f32);
                            }
                        }
                    }
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config,
                move |data: &[i16], _: &_| {
                    if let Ok(mut buf) = buffer_arc.lock() {
                        if channels == 1 {
                            buf.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
                        } else {
                            for chunk in data.chunks(channels as usize) {
                                let sum: f32 = chunk
                                    .iter()
                                    .map(|&s| s as f32 / i16::MAX as f32)
                                    .sum();
                                buf.push(sum / chunk.len() as f32);
                            }
                        }
                    }
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config,
                move |data: &[u16], _: &_| {
                    if let Ok(mut buf) = buffer_arc.lock() {
                        let normalize = |s: u16| (s as f32 - 32768.0) / 32768.0;
                        if channels == 1 {
                            buf.extend(data.iter().map(|&s| normalize(s)));
                        } else {
                            for chunk in data.chunks(channels as usize) {
                                let sum: f32 = chunk.iter().map(|&s| normalize(s)).sum();
                                buf.push(sum / chunk.len() as f32);
                            }
                        }
                    }
                },
                err_fn,
                None,
            ),
            _ => return Err("Unsupported sample format".to_string()),
        }.map_err(|e| e.to_string())?;

        stream.play().map_err(|e| e.to_string())?;
        self.stream = Some(stream);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.stream = None; 
    }
    
    pub fn clear(&mut self) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
    }

    pub fn get_resampled_audio(&self) -> Vec<f32> {
        let Ok(buf) = self.buffer.lock() else {
            return Vec::new();
        };
        if self.sample_rate == 16000 {
            return buf.clone();
        }
        
        // Naive nearest-neighbor resampling to 16000Hz
        let mut resampled = Vec::new();
        let ratio = self.sample_rate as f32 / 16000.0;
        let mut i = 0.0;
        while (i as usize) < buf.len() {
            resampled.push(buf[i as usize]);
            i += ratio;
        }
        resampled
    }
}
