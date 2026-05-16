//! cpal default input device discovery + named-device lookup.

use cpal::traits::{DeviceTrait, HostTrait};

use crate::AudioError;

pub(crate) struct InputDevice {
    pub device: cpal::Device,
    pub label: String,
    pub config: cpal::SupportedStreamConfig,
}

fn device_name(d: &cpal::Device) -> Result<String, AudioError> {
    d.description()
        .map(|desc| desc.name().to_string())
        .map_err(|e| AudioError::Cpal(format!("device name: {e}")))
}

/// Resolve an input device by name (None = default). Returns the device + a
/// supported config (we pick the default config, which cpal picks for us).
pub(crate) fn resolve(name: Option<&str>) -> Result<InputDevice, AudioError> {
    let host = cpal::default_host();

    let device = if let Some(want) = name {
        host.input_devices()
            .map_err(|e| AudioError::Cpal(format!("enumerate devices: {e}")))?
            .find(|d| device_name(d).map(|n| n == want).unwrap_or(false))
            .ok_or_else(|| AudioError::DeviceNotFound(want.to_string()))?
    } else {
        host.default_input_device().ok_or(AudioError::NoDevice)?
    };

    let label = device_name(&device)?;

    let config = device
        .default_input_config()
        .map_err(|e| AudioError::UnsupportedConfig(e.to_string()))?;

    Ok(InputDevice { device, label, config })
}
