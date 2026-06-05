//! Smart input-device routing for Bluetooth playback.
//!
//! Bluetooth headsets (`AirPods`) can't carry hi-fi A2DP stereo output and an
//! HFP mic input at the same time: opening their mic forces macOS to switch
//! the whole device to mono HFP, wrecking whatever the user is listening to
//! and degrading the dictation mic. When audio is actively playing through a
//! Bluetooth output and the dictation mic would also be a Bluetooth device,
//! we prefer the built-in mic so the headset stays in stereo.
//!
//! All `CoreAudio` probing here is best-effort: any query failure resolves to
//! a "don't reroute" answer (`false` / `None`) rather than erroring, because
//! routing is a nicety and must never block a dictation.

/// Decision for which input device to actually open for a dictation.
pub struct InputChoice {
    /// Device name to open, or `None` for the system default.
    pub device: Option<String>,
    /// Whether smart routing actually changed the configured device.
    pub rerouted: bool,
}

/// If `enabled` and audio is playing through a Bluetooth output path and the
/// configured/default mic is Bluetooth, prefer the built-in mic so the
/// Bluetooth output stays in stereo. Otherwise keep `configured`.
#[must_use]
pub fn choose_input_device(configured: Option<String>, enabled: bool) -> InputChoice {
    if enabled && output_is_active() && input_is_bluetooth(configured.as_deref()) {
        if let Some(builtin) = builtin_input_name() {
            return InputChoice { device: Some(builtin), rerouted: true };
        }
    }
    InputChoice { device: configured, rerouted: false }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::c_void;
    use std::ptr::NonNull;

    use objc2_core_audio::{
        kAudioDevicePropertyDeviceIsRunningSomewhere, kAudioDevicePropertyStreamConfiguration,
        kAudioDevicePropertyTransportType, kAudioDeviceTransportTypeBluetooth,
        kAudioDeviceTransportTypeBluetoothLE, kAudioDeviceTransportTypeBuiltIn,
        kAudioHardwarePropertyDefaultInputDevice, kAudioHardwarePropertyDefaultOutputDevice,
        kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain, kAudioObjectPropertyName,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeInput, kAudioObjectSystemObject,
        AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
        AudioObjectPropertyAddress, AudioObjectPropertyScope, AudioObjectPropertySelector,
    };
    use objc2_core_audio_types::AudioBufferList;
    use objc2_core_foundation::{CFRetained, CFString};

    fn address(
        selector: AudioObjectPropertySelector,
        scope: AudioObjectPropertyScope,
    ) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain,
        }
    }

    /// Read a fixed-size POD property into a `T`. Returns `Some` only on a clean
    /// status 0 read. Best-effort: any failure yields `None`.
    fn get_fixed<T: Copy + Default>(
        obj: AudioObjectID,
        selector: AudioObjectPropertySelector,
        scope: AudioObjectPropertyScope,
    ) -> Option<T> {
        let addr = address(selector, scope);
        let mut value = T::default();
        #[allow(clippy::cast_possible_truncation)]
        let mut size = std::mem::size_of::<T>() as u32;
        // SAFETY: `addr` and `size` are valid local pointers; the qualifier is
        // null with size 0 (these properties take no qualifier); `value` is a
        // `T`-sized buffer matching `size`. The fn writes at most `size` bytes
        // into it. We only trust the result on status 0.
        let status = unsafe {
            AudioObjectGetPropertyData(
                obj,
                NonNull::from(&addr),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::from(&mut value).cast::<c_void>(),
            )
        };
        if status == 0 {
            Some(value)
        } else {
            None
        }
    }

    /// The system object id (`kAudioObjectSystemObject` is typed `c_int`).
    #[allow(clippy::cast_sign_loss)]
    fn system_object() -> AudioObjectID {
        kAudioObjectSystemObject as AudioObjectID
    }

    /// Enumerate every audio device id known to the HAL.
    fn all_devices() -> Vec<AudioObjectID> {
        let addr = address(kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal);
        let mut byte_size: u32 = 0;
        // SAFETY: `addr` and `byte_size` are valid local pointers; null
        // qualifier with size 0. On success `byte_size` holds the property's
        // byte length.
        let status = unsafe {
            AudioObjectGetPropertyDataSize(
                system_object(),
                NonNull::from(&addr),
                0,
                std::ptr::null(),
                NonNull::from(&mut byte_size),
            )
        };
        if status != 0 || byte_size == 0 {
            return Vec::new();
        }
        let count = byte_size as usize / std::mem::size_of::<AudioObjectID>();
        let mut ids = vec![0u32; count];
        #[allow(clippy::cast_possible_truncation)]
        let mut io_size = byte_size;
        // SAFETY: `ids` is a `count`-element buffer sized to exactly
        // `byte_size` bytes (`io_size`); the fn writes at most that many bytes.
        let status = unsafe {
            AudioObjectGetPropertyData(
                system_object(),
                NonNull::from(&addr),
                0,
                std::ptr::null(),
                NonNull::from(&mut io_size),
                NonNull::new(ids.as_mut_ptr()).unwrap().cast::<c_void>(),
            )
        };
        if status != 0 {
            return Vec::new();
        }
        let actual = io_size as usize / std::mem::size_of::<AudioObjectID>();
        ids.truncate(actual);
        ids
    }

    /// The OS-visible name of a device, or `None` on failure.
    fn device_name(dev: AudioObjectID) -> Option<String> {
        let addr = address(kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal);
        let mut cf_ptr: *const CFString = std::ptr::null();
        #[allow(clippy::cast_possible_truncation)]
        let mut size = std::mem::size_of::<*const CFString>() as u32;
        // SAFETY: `addr` and `size` are valid local pointers; `cf_ptr` is a
        // pointer-sized output buffer matching `size`. On status 0 the HAL
        // writes a retained (+1) `CFStringRef` into `cf_ptr` that we own.
        let status = unsafe {
            AudioObjectGetPropertyData(
                dev,
                NonNull::from(&addr),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::from(&mut cf_ptr).cast::<c_void>(),
            )
        };
        if status != 0 {
            return None;
        }
        let ptr = NonNull::new(cf_ptr.cast_mut())?;
        // SAFETY: `ptr` is a valid `CFStringRef` with a +1 retain count owned
        // by us (the Get returned it retained). `CFRetained::from_raw` takes
        // ownership of that retain and releases it on drop, so we don't leak.
        let cf = unsafe { CFRetained::from_raw(ptr) };
        Some(cf.to_string())
    }

    /// Number of input channels the device exposes, summed across its input
    /// stream buffers. `0` means the device has no input capability.
    fn input_channel_count(dev: AudioObjectID) -> u32 {
        let addr = address(
            kAudioDevicePropertyStreamConfiguration,
            kAudioObjectPropertyScopeInput,
        );
        let mut byte_size: u32 = 0;
        // SAFETY: valid local pointers; null qualifier. `byte_size` receives
        // the size of the variable-length `AudioBufferList` on success.
        let status = unsafe {
            AudioObjectGetPropertyDataSize(
                dev,
                NonNull::from(&addr),
                0,
                std::ptr::null(),
                NonNull::from(&mut byte_size),
            )
        };
        if status != 0 || (byte_size as usize) < std::mem::size_of::<u32>() {
            return 0;
        }
        // `AudioBufferList` is a variable-length struct (a u32 count followed
        // by N `AudioBuffer`s). Back it with a `u64` buffer so the storage is
        // 8-byte aligned (matching `AudioBufferList`/`AudioBuffer`, which
        // contain pointers), sized to cover the reported byte length.
        let words = (byte_size as usize).div_ceil(std::mem::size_of::<u64>());
        let mut backing = vec![0u64; words];
        #[allow(clippy::cast_possible_truncation)]
        let mut io_size = byte_size;
        // SAFETY: `backing` is `words * 8 >= byte_size` bytes; the fn writes at
        // most `io_size` (== `byte_size`) bytes into it.
        let status = unsafe {
            AudioObjectGetPropertyData(
                dev,
                NonNull::from(&addr),
                0,
                std::ptr::null(),
                NonNull::from(&mut io_size),
                NonNull::new(backing.as_mut_ptr()).unwrap().cast::<c_void>(),
            )
        };
        if status != 0 {
            return 0;
        }
        let list_ptr = backing.as_ptr().cast::<AudioBufferList>();
        // SAFETY: `backing` holds a populated `AudioBufferList`; we read its
        // header unaligned, then index the trailing `AudioBuffer` array by
        // offset (the struct declares `[AudioBuffer; 1]` but the HAL may write
        // `mNumberBuffers` of them contiguously).
        unsafe {
            let n = std::ptr::addr_of!((*list_ptr).mNumberBuffers).read_unaligned();
            let first = std::ptr::addr_of!((*list_ptr).mBuffers).cast::<objc2_core_audio_types::AudioBuffer>();
            let mut total = 0u32;
            for i in 0..n as usize {
                let buf = first.add(i);
                total = total.saturating_add(
                    std::ptr::addr_of!((*buf).mNumberChannels).read_unaligned(),
                );
            }
            total
        }
    }

    fn is_bluetooth_transport(t: u32) -> bool {
        t == kAudioDeviceTransportTypeBluetooth || t == kAudioDeviceTransportTypeBluetoothLE
    }

    #[must_use]
    pub fn output_is_active() -> bool {
        let Some(dev) = get_fixed::<AudioObjectID>(
            system_object(),
            kAudioHardwarePropertyDefaultOutputDevice,
            kAudioObjectPropertyScopeGlobal,
        ) else {
            return false;
        };
        get_fixed::<u32>(
            dev,
            kAudioDevicePropertyDeviceIsRunningSomewhere,
            kAudioObjectPropertyScopeGlobal,
        )
        .is_some_and(|running| running != 0)
    }

    #[must_use]
    pub fn input_is_bluetooth(name: Option<&str>) -> bool {
        let dev = match name {
            None => get_fixed::<AudioObjectID>(
                system_object(),
                kAudioHardwarePropertyDefaultInputDevice,
                kAudioObjectPropertyScopeGlobal,
            ),
            Some(want) => all_devices().into_iter().find(|&d| {
                input_channel_count(d) > 0 && device_name(d).as_deref() == Some(want)
            }),
        };
        let Some(dev) = dev else { return false };
        get_fixed::<u32>(
            dev,
            kAudioDevicePropertyTransportType,
            kAudioObjectPropertyScopeGlobal,
        )
        .is_some_and(is_bluetooth_transport)
    }

    #[must_use]
    pub fn builtin_input_name() -> Option<String> {
        for dev in all_devices() {
            if input_channel_count(dev) == 0 {
                continue;
            }
            let transport = get_fixed::<u32>(
                dev,
                kAudioDevicePropertyTransportType,
                kAudioObjectPropertyScopeGlobal,
            );
            if transport == Some(kAudioDeviceTransportTypeBuiltIn) {
                if let Some(name) = device_name(dev) {
                    return Some(name);
                }
            }
        }
        None
    }
}

#[cfg(target_os = "macos")]
pub use imp::{builtin_input_name, input_is_bluetooth, output_is_active};

/// `true` if the default OUTPUT device is currently running IO. Best-effort:
/// any query failure resolves to `false`.
#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn output_is_active() -> bool {
    false
}

/// `true` if the resolved input device (named, or the default when `None`)
/// uses a Bluetooth transport. Best-effort: failure resolves to `false`.
#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn input_is_bluetooth(_name: Option<&str>) -> bool {
    false
}

/// The name of the first built-in input-capable device, if any. Best-effort:
/// failure resolves to `None`.
#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn builtin_input_name() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_input_device_disabled_is_passthrough() {
        let choice = choose_input_device(Some("AirPods Pro".into()), false);
        assert_eq!(choice.device.as_deref(), Some("AirPods Pro"));
        assert!(!choice.rerouted);

        let default = choose_input_device(None, false);
        assert!(default.device.is_none());
        assert!(!default.rerouted);
    }
}
