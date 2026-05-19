#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

//! Refiner-stage model evaluation harness.
//!
//! Dev-only binary that runs candidate LLMs against a multilingual
//! style-transfer corpus and produces comparable score reports.
//! Never linked into the shipped Tauri app.

pub mod cli;
