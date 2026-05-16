//! Node addon wrapping audio-capture + os-integration for M3 Electron consumption.

#![deny(clippy::all)]

#[macro_use]
extern crate napi_derive;

use os_integration::PermissionStatus;

fn map_perm(s: PermissionStatus) -> String {
    match s {
        PermissionStatus::Granted => "granted",
        PermissionStatus::Denied => "denied",
        PermissionStatus::NotDetermined => "not_determined",
    }
    .to_string()
}

#[napi]
pub fn check_accessibility() -> String {
    map_perm(os_integration::check_accessibility())
}

#[napi]
pub fn prompt_accessibility() -> String {
    map_perm(os_integration::prompt_accessibility())
}

#[napi]
pub fn check_microphone() -> String {
    map_perm(os_integration::check_microphone())
}

use napi::bindgen_prelude::Buffer;

#[napi]
pub struct Recorder {
    inner: audio_capture::Recorder,
}

#[napi]
impl Recorder {
    #[napi(constructor)]
    pub fn new() -> napi::Result<Self> {
        let inner = audio_capture::Recorder::new(audio_capture::RecorderConfig::default())
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(Self { inner })
    }

    #[napi]
    pub fn start(&mut self) -> napi::Result<()> {
        self.inner.start().map_err(|e| napi::Error::from_reason(e.to_string()))
    }

    #[napi]
    pub fn stop(&mut self) -> napi::Result<Buffer> {
        let rec = self
            .inner
            .stop()
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let wav = audio_capture::samples_to_wav(&rec.samples);
        Ok(wav.into())
    }
}

#[napi]
pub struct Injector {
    inner: os_integration::Injector,
}

impl Default for Injector {
    fn default() -> Self {
        Self::new()
    }
}

#[napi]
impl Injector {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self { inner: os_integration::Injector::new() }
    }

    #[napi(factory)]
    pub fn with_force_pasteboard(force_pasteboard: bool) -> Self {
        Self { inner: os_integration::Injector::with_force_pasteboard(force_pasteboard) }
    }

    #[napi]
    pub fn inject(&self, text: String) -> napi::Result<String> {
        let method = self
            .inner
            .inject(&text)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(match method {
            os_integration::InjectionMethod::Accessibility => "accessibility".into(),
            os_integration::InjectionMethod::Pasteboard => "pasteboard".into(),
        })
    }
}

use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};

fn parse_hotkey(s: &str) -> os_integration::Hotkey {
    match s {
        "right-option" | "RightOption" => os_integration::Hotkey::RightOption,
        "left-option" | "LeftOption" => os_integration::Hotkey::LeftOption,
        "right-command" | "RightCommand" => os_integration::Hotkey::RightCommand,
        "fn" | "Fn" => os_integration::Hotkey::Fn,
        "f5" | "F5" => os_integration::Hotkey::F5,
        _ => os_integration::Hotkey::RightOption,
    }
}

#[napi]
pub struct HotkeyListener {
    handle: Option<os_integration::HotkeyListener>,
    drain_task: Option<std::thread::JoinHandle<()>>,
}

#[napi]
impl HotkeyListener {
    #[napi(factory)]
    pub fn install(
        hotkey: String,
        on_event: ThreadsafeFunction<String>,
    ) -> napi::Result<Self> {
        let parsed = parse_hotkey(&hotkey);
        let (listener, mut rx) = os_integration::HotkeyListener::install(parsed)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        let tsfn = on_event.clone();
        let drain_task = std::thread::Builder::new()
            .name("hotkey-tsfn-bridge".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio rt for tsfn bridge");
                rt.block_on(async move {
                    while let Some(event) = rx.recv().await {
                        let payload = match event {
                            os_integration::HotkeyEvent::Down => "down".to_string(),
                            os_integration::HotkeyEvent::Up => "up".to_string(),
                        };
                        let _ = tsfn.call(Ok(payload), ThreadsafeFunctionCallMode::NonBlocking);
                    }
                });
            })
            .map_err(|e| napi::Error::from_reason(format!("spawn bridge thread: {e}")))?;

        Ok(Self {
            handle: Some(listener),
            drain_task: Some(drain_task),
        })
    }

    #[napi]
    pub fn shutdown(&mut self) -> napi::Result<()> {
        if let Some(h) = self.handle.take() {
            h.shutdown();
        }
        if let Some(t) = self.drain_task.take() {
            let _ = t.join();
        }
        Ok(())
    }
}
