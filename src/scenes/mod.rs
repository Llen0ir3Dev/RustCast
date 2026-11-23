//! Scene and source data model. This module knows nothing about GStreamer or
//! the UI; the media engine consumes [`SourceConfig`] values directly.

use serde::{Deserialize, Serialize};

pub type SceneId = u64;
pub type SourceId = u64;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SourceKind {
    ScreenCapture,
    Webcam,
    Image,
    VideoFile,
    AudioInput,
    DesktopAudio,
    Text,
    Color,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ScreenCapture => "Screen Capture",
            Self::Webcam => "Webcam",
            Self::Image => "Image",
            Self::VideoFile => "Video File",
            Self::AudioInput => "Microphone / Audio Input",
            Self::DesktopAudio => "Desktop Audio",
            Self::Text => "Text Overlay",
            Self::Color => "Color Background",
        }
    }

    pub fn has_video(self) -> bool {
        !matches!(self, Self::AudioInput | Self::DesktopAudio)
    }

    pub fn has_audio(self) -> bool {
        matches!(
            self,
            Self::AudioInput | Self::DesktopAudio | Self::VideoFile
        )
    }

    pub fn all() -> [Self; 8] {
        use SourceKind::*;
        [
            ScreenCapture,
            Webcam,
            Image,
            VideoFile,
            AudioInput,
            DesktopAudio,
            Text,
            Color,
        ]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SourceConfig {
    pub id: SourceId,
    pub name: String,
    pub kind: SourceKind,
    pub visible: bool,
    pub muted: bool,
    /// Linear gain, 0.0 .. 1.0 (1.0 = unchanged).
    pub volume: f32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Device path/name for webcam and audio sources.
    pub device: String,
    /// File path for image and video sources.
    pub path: String,
    pub text: String,
    /// RGB hex value used by the color and text sources.
    pub color: u32,
    pub font_size: u32,
}

impl SourceConfig {
    pub fn new(id: SourceId, kind: SourceKind, output_width: u32, output_height: u32) -> Self {
        let name = kind.label().to_string();
        let full = (output_width, output_height);
        let (width, height) = match kind {
            SourceKind::Webcam => (
                (output_width / 3).max(160),
                (output_height / 3).max(120),
            ),
            SourceKind::Text => (output_width.min(640), 80),
            SourceKind::Color => full,
            _ => full,
        };
        let (x, y) = match kind {
            SourceKind::Webcam => (12, (output_height as i32 - height as i32 - 12).max(0)),
            SourceKind::Text => ((output_width as i32 - width as i32) / 2, 12),
            _ => (0, 0),
        };
        Self {
            id,
            name,
            kind,
            visible: true,
            muted: false,
            volume: 1.0,
            x,
            y,
            width: width.max(16),
            height: height.max(16),
            device: String::new(),
            path: String::new(),
            text: match kind {
                SourceKind::Text => "RustCast".to_string(),
                _ => String::new(),
            },
            color: 0x3355ff,
            font_size: 36,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Scene {
    pub id: SceneId,
    pub name: String,
    pub sources: Vec<SourceConfig>,
}

impl Scene {
    pub fn new(id: SceneId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            sources: Vec::new(),
        }
    }
}

/// A persistable collection of scenes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneCollection {
    pub scenes: Vec<Scene>,
    pub active_scene: usize,
    next_scene_id: SceneId,
    next_source_id: SourceId,
}

impl Default for SceneCollection {
    fn default() -> Self {
        Self {
            scenes: vec![Scene::new(1, "Scene 1")],
            active_scene: 0,
            next_scene_id: 2,
            next_source_id: 1,
        }
    }
}

impl SceneCollection {
    pub fn active(&self) -> Option<&Scene> {
        self.scenes.get(self.active_scene)
    }

    pub fn active_mut(&mut self) -> Option<&mut Scene> {
        self.scenes.get_mut(self.active_scene)
    }

    pub fn add_scene(&mut self, name: impl Into<String>) -> SceneId {
        let id = self.next_scene_id;
        self.next_scene_id += 1;
        self.scenes.push(Scene::new(id, name));
        self.active_scene = self.scenes.len() - 1;
        id
    }

    pub fn remove_scene(&mut self, index: usize) -> bool {
        if self.scenes.len() <= 1 || index >= self.scenes.len() {
            return false;
        }
        self.scenes.remove(index);
        if self.active_scene >= self.scenes.len() {
            self.active_scene = self.scenes.len() - 1;
        } else if index < self.active_scene {
            self.active_scene -= 1;
        }
        true
    }

    pub fn select_scene(&mut self, index: usize) -> bool {
        if index < self.scenes.len() {
            self.active_scene = index;
            true
        } else {
            false
        }
    }

    pub fn add_source(
        &mut self,
        kind: SourceKind,
        output_width: u32,
        output_height: u32,
    ) -> Option<SourceId> {
        let id = self.next_source_id;
        let source = SourceConfig::new(id, kind, output_width, output_height);
        self.next_source_id += 1;
        self.active_mut()?.sources.push(source);
        Some(id)
    }

    pub fn find_source(&self, id: SourceId) -> Option<&SourceConfig> {
        self.active()?.sources.iter().find(|s| s.id == id)
    }

    pub fn find_source_mut(&mut self, id: SourceId) -> Option<&mut SourceConfig> {
        self.active_mut()?.sources.iter_mut().find(|s| s.id == id)
    }

    pub fn save_to_file(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn load_from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_lifecycle() {
        let mut collection = SceneCollection::default();
        let a = collection.add_scene("Gameplay");
        let b = collection.add_scene("Just Chatting");
        assert_eq!(collection.active().unwrap().id, b);
        let source = collection.add_source(SourceKind::Webcam, 854, 480).unwrap();
        assert!(collection.find_source(source).is_some());
        assert!(collection.select_scene(0));
        assert_eq!(collection.active().unwrap().id, a);
        assert!(collection.find_source(source).is_none());
        assert!(collection.remove_scene(0));
        assert_eq!(collection.scenes.len(), 1);
        assert!(!collection.remove_scene(0));
    }

    #[test]
    fn source_sizes_are_sane() {
        let source = SourceConfig::new(7, SourceKind::ScreenCapture, 854, 480);
        assert_eq!((source.width, source.height), (854, 480));
        assert!(source.has_video_placeholder_check());
    }
}

#[cfg(test)]
impl SourceConfig {
    fn has_video_placeholder_check(&self) -> bool {
        self.width > 0 && self.height > 0
    }
}
