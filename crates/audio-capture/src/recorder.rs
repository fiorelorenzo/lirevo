//! Recorder is implemented in T5. This stub keeps the crate compilable.

use crate::AudioError;

#[derive(Debug, Clone)]
pub struct RecorderConfig {
    pub device_name: Option<String>,
    pub max_duration_secs: u32,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self { device_name: None, max_duration_secs: 60 }
    }
}

pub struct Recording {
    pub samples: Vec<f32>,
    pub duration_ms: u32,
    pub device_label: String,
}

pub struct Recorder {
    _cfg: RecorderConfig,
}

impl Recorder {
    pub fn new(cfg: RecorderConfig) -> Result<Self, AudioError> {
        Ok(Self { _cfg: cfg })
    }

    pub fn start(&mut self) -> Result<(), AudioError> {
        Err(AudioError::Internal("Recorder::start lands in T5".into()))
    }

    pub fn stop(&mut self) -> Result<Recording, AudioError> {
        Err(AudioError::Internal("Recorder::stop lands in T5".into()))
    }
}
