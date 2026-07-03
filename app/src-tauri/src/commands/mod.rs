pub mod dialog;
pub mod dictation;
pub mod history;
pub mod hotkey;
pub mod inference;
pub mod models;
pub mod permissions;
pub mod profile;
pub mod settings;
pub mod updater;
pub mod windows;

#[derive(Clone, serde::Serialize)]
pub struct Toast {
    pub kind: &'static str,
    pub message: String,
}

pub fn toast(kind: &'static str, message: impl Into<String>) -> Toast {
    Toast {
        kind,
        message: message.into(),
    }
}
