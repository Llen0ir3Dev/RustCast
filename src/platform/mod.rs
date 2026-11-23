//! Linux platform detection. The application stays X11-first and can use
//! PipeWire capture when Wayland is active and the plugin is installed.

use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct SystemInfo {
    pub display: String,
    pub wayland: bool,
    pub video_devices: Vec<String>,
}

pub fn detect() -> SystemInfo {
    let video_devices = std::fs::read_dir("/dev")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("video"))
                .unwrap_or(false)
        })
        .filter(|path| path.exists())
        .map(|path| path.to_string_lossy().into_owned())
        .collect();

    SystemInfo {
        display: std::env::var("DISPLAY").unwrap_or_default(),
        wayland: std::env::var("WAYLAND_DISPLAY").is_ok(),
        video_devices,
    }
}

impl SystemInfo {
    /// Returns human-readable warnings for missing capture capabilities.
    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.display.is_empty() && !self.wayland {
            warnings.push(
                "No DISPLAY or WAYLAND_DISPLAY detected; screen capture will not start."
                    .to_string(),
            );
        }
        if self.video_devices.is_empty() {
            warnings.push("No /dev/video* device found; webcam sources will not start.".to_string());
        }
        warnings
    }
}

/// True when a GStreamer element factory is available at runtime.
pub fn has_gst_element(name: &str) -> bool {
    gstreamer::ElementFactory::find(name).is_some()
}

/// Checks whether a media file exists without following directory paths.
pub fn is_file(path: &str) -> bool {
    Path::new(path).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warnings_are_present_without_display() {
        let info = SystemInfo::default();
        assert!(!info.warnings().is_empty());
    }
}
