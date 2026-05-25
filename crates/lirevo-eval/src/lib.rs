#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

//! Refiner-stage model evaluation harness.

pub mod backend;
pub mod cli;
pub mod corpus;
pub mod probes;
pub mod profiles;
pub mod report;
pub mod scoring;
pub mod util;
