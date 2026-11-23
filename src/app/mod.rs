//! Application state and glue between UI actions and the media engine.

use anyhow::Result;
use egui::TextureHandle;
use std::path::PathBuf;
use std::time::Instant;

use crate::audio::Level;
use crate::media::{EngineEvent, MediaEngine};
use crate::recording::RecordStatus;
use crate::scenes::{SceneCollection, SourceConfig, SourceId, SourceKind};
use crate::settings::{AppSettings, ConfigStore};
use crate::streaming::StreamStatus;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub time: chrono::DateTime<chrono::Local>,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Drafts {
    pub new_scene_name: String,
    pub source_path: String,
    pub source_device: String,
    pub source_text: String,
    pub color_hex: String,
    pub font_size: u32,
    pub scene_collection_path: String,
    pub record_dir: String,
}

impl Default for Drafts {
    fn default() -> Self {
        Self {
            new_scene_name: String::new(),
            source_path: String::new(),
            source_device: String::new(),
            source_text: String::new(),
            color_hex: "3355ff".to_string(),
            font_size: 36,
            scene_collection_path: String::new(),
            record_dir: String::new(),
        }
    }
}

pub struct RustCastApp {
    pub drafts: Drafts,
    pub config: ConfigStore,
    pub settings: AppSettings,
    pub collection: SceneCollection,
    pub engine: Option<MediaEngine>,
    pub engine_error: Option<String>,
    pub logs: Vec<LogEntry>,
    pub settings_open: bool,
    pub about_open: bool,
    pub preview_texture: Option<TextureHandle>,
    pub preview_size: (usize, usize),
    pub selected_source: Option<SourceId>,
    pub scene_path: PathBuf,
    pub pending_restart: bool,
    pub last_frame: Instant,
}

impl RustCastApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        crate::ui::apply_dark_theme(&_cc.egui_ctx);
        let config = ConfigStore::new();
        let settings = config.load();
        let scene_path = config
            .path()
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("scenes.json");
        let collection = SceneCollection::load_from_file(&scene_path).unwrap_or_default();

        let engine_result = MediaEngine::new(settings.clone());
        let engine_error = engine_result.as_ref().err().map(|e| e.to_string());
        let mut drafts = Drafts::default();
        drafts.record_dir = settings.record.directory.to_string_lossy().into_owned();
        let mut app = Self {
            drafts,
            config,
            settings,
            collection,
            engine: engine_result.ok(),
            engine_error,
            logs: Vec::new(),
            settings_open: false,
            about_open: false,
            preview_texture: None,
            preview_size: (0, 0),
            selected_source: None,
            scene_path,
            pending_restart: false,
            last_frame: Instant::now(),
        };
        if app.engine.is_some() {
            app.reload_scene();
        }
        let warnings = crate::platform::detect().warnings();
        for warning in warnings {
            app.log(LogLevel::Warning, warning);
        }
        if let Some(err) = &app.engine_error {
            app.log(LogLevel::Error, format!("Media engine failed to start: {err}"));
        }
        app
    }

    pub fn log(&mut self, level: LogLevel, message: impl Into<String>) {
        self.logs.push(LogEntry {
            time: chrono::Local::now(),
            level,
            message: message.into(),
        });
        if self.logs.len() > 200 {
            self.logs.remove(0);
        }
    }

    pub fn save(&mut self) {
        if let Err(err) = self.config.save(&self.settings) {
            self.log(LogLevel::Error, format!("Failed to save settings: {err}"));
        }
        if let Err(err) = self.collection.save_to_file(&self.scene_path) {
            self.log(LogLevel::Error, format!("Failed to save scenes: {err}"));
        }
    }

    pub fn stream_status(&self) -> StreamStatus {
        self.engine
            .as_ref()
            .map(MediaEngine::stream_status)
            .unwrap_or(StreamStatus::Idle)
    }

    pub fn record_status(&self) -> RecordStatus {
        self.engine
            .as_ref()
            .map(MediaEngine::record_status)
            .unwrap_or(RecordStatus::Idle)
    }

    pub fn level_for(&self, id: SourceId) -> Level {
        self.engine
            .as_ref()
            .map(|e| e.levels().get(&format!("level_{id}")))
            .unwrap_or_default()
    }

    pub fn master_level(&self) -> Level {
        self.engine
            .as_ref()
            .map(|e| e.levels().get("master_level"))
            .unwrap_or_default()
    }

    pub fn reload_scene(&mut self) {
        if let Some(engine) = &mut self.engine {
            engine.rebuild_scene(&self.collection);
        }
    }

    pub fn add_source(&mut self, kind: SourceKind) {
        let width = self.settings.video.width;
        let height = self.settings.video.height;
        let Some(id) = self.collection.add_source(kind, width, height) else {
            return;
        };
        if let Some(cfg) = self.collection.find_source(id).cloned() {
            if let Some(engine) = &mut self.engine {
                if let Err(err) = engine.add_source(&cfg) {
                    self.log(LogLevel::Error, format!("Cannot add source: {err}"));
                }
            }
        }
        self.selected_source = Some(id);
        self.save();
    }

    pub fn remove_selected_source(&mut self) {
        let Some(id) = self.selected_source else { return };
        if let Some(scene) = self.collection.active_mut() {
            scene.sources.retain(|source| source.id != id);
        }
        if let Some(engine) = &mut self.engine {
            engine.remove_source(id);
        }
        self.selected_source = None;
        self.save();
    }

    pub fn update_source(&mut self, cfg: SourceConfig, rebuild: bool) {
        let id = cfg.id;
        if let Some(scene) = self.collection.active_mut() {
            if let Some(existing) = scene.sources.iter_mut().find(|s| s.id == id) {
                *existing = cfg.clone();
            }
        }
        if let Some(engine) = &mut self.engine {
            if rebuild {
                engine.remove_source(id);
                if let Err(err) = engine.add_source(&cfg) {
                    self.log(LogLevel::Error, format!("Cannot update source: {err}"));
                }
            } else {
                engine.apply_source_config(&cfg);
            }
        }
        self.save();
    }

    pub fn select_scene(&mut self, index: usize) {
        if self.collection.select_scene(index) {
            self.selected_source = None;
            self.reload_scene();
            self.save();
        }
    }

    pub fn add_scene(&mut self, name: String) {
        self.collection.add_scene(name);
        self.selected_source = None;
        self.reload_scene();
        self.save();
    }

    pub fn remove_scene(&mut self, index: usize) {
        if self.collection.remove_scene(index) {
            self.selected_source = None;
            self.reload_scene();
            self.save();
        }
    }

    pub fn toggle_stream(&mut self) {
        let Some(engine) = &mut self.engine else { return };
        if engine.stream_status() == StreamStatus::Idle || engine.stream_status() == StreamStatus::Error {
            match engine.start_stream() {
                Ok(()) => self.log(LogLevel::Info, "Starting stream output"),
                Err(err) => self.log(LogLevel::Error, err.to_string()),
            }
        } else {
            engine.stop_stream();
        }
    }

    pub fn toggle_record(&mut self) {
        let Some(engine) = &mut self.engine else { return };
        if engine.record_status() == RecordStatus::Idle {
            match engine.start_record() {
                Ok(()) => self.log(LogLevel::Info, "Starting recording"),
                Err(err) => self.log(LogLevel::Error, err.to_string()),
            }
        } else {
            engine.stop_record();
        }
    }

    /// Applies settings changes. Resolution/FPS/AAC format changes require a
    /// new pipeline, so those set `pending_restart` and are applied on exit of
    /// the settings window.
    pub fn apply_settings(&mut self, settings: AppSettings, needs_restart: bool) {
        self.settings = settings;
        if needs_restart {
            self.pending_restart = true;
        } else if let Some(engine) = &mut self.engine {
            engine.update_settings(self.settings.clone());
        }
        self.save();
    }

    pub fn restart_engine(&mut self) {
        self.pending_restart = false;
        let result = MediaEngine::new(self.settings.clone());
        match result {
            Ok(mut engine) => {
                engine.rebuild_scene(&self.collection);
                self.engine = Some(engine);
                self.engine_error = None;
                self.log(LogLevel::Info, "Media pipeline restarted with new settings");
            }
            Err(err) => {
                self.engine_error = Some(err.to_string());
                self.log(LogLevel::Error, format!("Failed to restart media engine: {err}"));
            }
        }
    }

    pub fn handle_engine_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Info(message) => self.log(LogLevel::Info, message),
            EngineEvent::Warning(message) => self.log(LogLevel::Warning, message),
            EngineEvent::Error(message) => self.log(LogLevel::Error, message),
            EngineEvent::RecordingFinished(path) => {
                self.log(LogLevel::Info, format!("Recording saved to {}", path.display()));
            }
            EngineEvent::SourceFailed(id, message) => {
                self.log(LogLevel::Error, format!("Source {id} failed: {message}"));
            }
            EngineEvent::StreamDisconnected(message) => {
                self.log(LogLevel::Error, format!("Stream disconnected: {message}"));
            }
        }
    }

    pub fn frame(&mut self, ctx: &egui::Context) {
        if let Some(engine) = &mut self.engine {
            for event in engine.poll() {
                self.handle_engine_event(event);
            }
            engine.tick();
            if let Some(frame) = engine.take_preview() {
                self.preview_size = (frame.width, frame.height);
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [frame.width, frame.height],
                    &frame.rgba,
                );
                let options = egui::TextureOptions::LINEAR;
                match &mut self.preview_texture {
                    Some(texture) => texture.set(image, options),
                    None => {
                        self.preview_texture = Some(ctx.load_texture(
                            "rustcast-preview",
                            image,
                            options,
                        ));
                    }
                }
            }
        }
        self.last_frame = Instant::now();
        ctx.request_repaint();

        crate::ui::top_bar(self, ctx);
        crate::ui::scenes_panel(self, ctx);
        crate::ui::sources_panel(self, ctx);
        crate::ui::central_area(self, ctx);
        crate::ui::settings_window(self, ctx);
        crate::ui::about_window(self, ctx);

        if self.pending_restart && !self.settings_open {
            self.restart_engine();
        }
    }
}
