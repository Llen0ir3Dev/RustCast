//! Media engine bindings to GStreamer. Nothing in this module depends on egui.

mod engine;
mod sources;

pub use engine::{EngineEvent, MediaEngine};
