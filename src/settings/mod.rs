//! Persistent application configuration.
//!
//! Settings are stored as JSON under the user's config directory
//! (`~/.config/rustcast/config.json` on Linux). Stream keys are local-only
//! and are never written to the repository.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Output video settings for the composited scene.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VideoSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub encoder_threads: u32,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            // 854x480 is 16:9 "480p"; a good fit for low-end CPUs.
            width: 854,
            height: 480,
            fps: 30,
            bitrate_kbps: 1200,
            encoder_threads: 2,
        }
    }
}

impl VideoSettings {
    /// Caps values to sane ranges so a malformed config can never stall the
    /// media pipeline or allocate an unreasonable amount of RAM.
    pub fn sanitize(&mut self) {
        self.width = self.width.clamp(320, 3840);
        self.height = self.height.clamp(240, 2160);
        self.fps = self.fps.clamp(10, 60);
        self.bitrate_kbps = self.bitrate_kbps.clamp(200, 20_000);
        self.encoder_threads = self.encoder_threads.clamp(1, 8);
    }
}

/// RTMP/RTMPS destination.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamSettings {
    pub server_url: String,
    pub stream_key: String,
    pub reconnect: bool,
    pub bitrate_kbps: u32,
}

impl StreamSettings {
    /// Returns the complete RTMP destination without ever logging the key.
    pub fn endpoint(&self) -> Option<String> {
        let server = self.server_url.trim().trim_end_matches('/');
        let key = self.stream_key.trim();
        if server.is_empty() || key.is_empty() {
            return None;
        }
        if !(server.starts_with("rtmp://") || server.starts_with("rtmps://")) {
            return None;
        }
        Some(format!("{server}/{key}"))
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RecordFormat {
    Mp4,
    Mkv,
}

impl RecordFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
        }
    }

    pub fn muxer(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4mux",
            Self::Mkv => "matroskamux",
        }
    }
}

impl std::fmt::Display for RecordFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mp4 => write!(f, "MP4"),
            Self::Mkv => write!(f, "MKV (recommended)"),
        }
    }
}

/// Recording settings are intentionally separate from stream settings.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RecordSettings {
    pub format: RecordFormat,
    pub directory: PathBuf,
    pub basename: String,
    pub bitrate_kbps: u32,
    /// When true, recording uses the video output size instead of its own.
    pub use_output_resolution: bool,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Default for RecordSettings {
    fn default() -> Self {
        Self {
            format: RecordFormat::Mkv,
            directory: dirs::video_dir().unwrap_or_else(|| PathBuf::from(".")),
            basename: "RustCast".to_string(),
            bitrate_kbps: 2500,
            use_output_resolution: true,
            width: 1280,
            height: 720,
            fps: 30,
        }
    }
}

impl RecordSettings {
    pub fn sanitize(&mut self) {
        self.bitrate_kbps = self.bitrate_kbps.clamp(200, 40_000);
        self.width = self.width.clamp(320, 3840);
        self.height = self.height.clamp(240, 2160);
        self.fps = self.fps.clamp(10, 60);
        if self.basename.trim().is_empty() {
            self.basename = "RustCast".to_string();
        }
    }

    /// Builds a unique output path using the local time.
    pub fn output_path(&self) -> PathBuf {
        let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
        let name = format!("{}_{}.{}", self.basename, stamp, self.format.extension());
        self.directory.join(name)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AudioSettings {
    pub sample_rate: u32,
    pub aac_bitrate_kbps: u32,
    pub noise_suppression: bool,
    pub monitor: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            aac_bitrate_kbps: 160,
            noise_suppression: false,
            monitor: false,
        }
    }
}

impl AudioSettings {
    pub fn sanitize(&mut self) {
        self.sample_rate = if self.sample_rate == 44_100 { 44_100 } else { 48_000 };
        self.aac_bitrate_kbps = self.aac_bitrate_kbps.clamp(64, 320);
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AppSettings {
    pub video: VideoSettings,
    pub stream: StreamSettings,
    pub record: RecordSettings,
    pub audio: AudioSettings,
}

impl AppSettings {
    pub fn sanitize(&mut self) {
        self.video.sanitize();
        self.record.sanitize();
        self.audio.sanitize();
    }
}

/// Loads, validates and saves settings in one place.
#[derive(Clone, Debug)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new() -> Self {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rustcast");
        Self {
            path: dir.join("config.json"),
        }
    }

    pub fn with_path(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> AppSettings {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return AppSettings::default();
        };
        match serde_json::from_str::<AppSettings>(&text) {
            Ok(mut settings) => {
                settings.sanitize();
                settings
            }
            Err(err) => {
                log::warn!("Ignoring invalid config {}: {err}", self.path.display());
                AppSettings::default()
            }
        }
    }

    pub fn save(&self, settings: &AppSettings) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(settings)?;
        std::fs::write(&self.path, text)
            .with_context(|| format!("writing {}", self.path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_endpoint_requires_scheme_and_key() {
        let mut s = StreamSettings::default();
        assert_eq!(s.endpoint(), None);
        s.server_url = "rtmp://live.example.com/live".into();
        assert_eq!(s.endpoint(), None);
        s.stream_key = "secret".into();
        assert_eq!(
            s.endpoint(),
            Some("rtmp://live.example.com/live/secret".into())
        );
        s.server_url = "https://example.com".into();
        assert_eq!(s.endpoint(), None);
    }

    #[test]
    fn sanitize_clamps_values() {
        let mut s = VideoSettings {
            width: 100,
            height: 99999,
            fps: 0,
            bitrate_kbps: 1,
            encoder_threads: 99,
        };
        s.sanitize();
        assert_eq!(s.width, 320);
        assert_eq!(s.height, 2160);
        assert_eq!(s.fps, 10);
        assert_eq!(s.bitrate_kbps, 200);
        assert_eq!(s.encoder_threads, 8);
    }

    #[test]
    fn config_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::with_path(dir.path().join("cfg.json"));
        let original = AppSettings::default();
        store.save(&original).unwrap();
        let loaded = store.load();
        assert_eq!(original, loaded);
    }
}
