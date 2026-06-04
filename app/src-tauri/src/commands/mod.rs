pub mod settings;
pub mod models;
pub mod inference;
pub mod dictation;
pub mod permissions;
pub mod windows;
pub mod dialog;
pub mod updater;
pub mod history;

#[derive(Clone, serde::Serialize)]
pub struct Toast {
    pub kind: &'static str,
    pub message: String,
}

pub fn toast(kind: &'static str, message: impl Into<String>) -> Toast {
    Toast { kind, message: message.into() }
}
