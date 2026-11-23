//! RustCast: a lightweight Linux streaming and recording application.
//!
//! The crate is split into a UI-independent media engine (`media`) and thin
//! presentation layers (`app`, `ui`). Data models live in `scenes`,
//! `settings`, `audio`, `video`, `streaming` and `recording`.

pub mod app;
pub mod audio;
pub mod media;
pub mod platform;
pub mod recording;
pub mod scenes;
pub mod settings;
pub mod streaming;
pub mod ui;
pub mod video;
