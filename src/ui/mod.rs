//! egui user interface. Every control here maps to a real engine command.

use egui::{Color32, RichText, Rounding, Vec2};

use crate::app::{LogLevel, RustCastApp};
use crate::audio::volume_to_db;
use crate::recording::RecordStatus;
use crate::scenes::{SourceKind, SourceConfig};
use crate::settings::{AppSettings, RecordFormat};
use crate::streaming::StreamStatus;

pub fn apply_dark_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_rounding = Rounding::same(6.0);
    visuals.menu_rounding = Rounding::same(4.0);
    visuals.widgets.noninteractive.bg_fill = Color32::from_gray(28);
    visuals.widgets.inactive.bg_fill = Color32::from_gray(42);
    visuals.widgets.hovered.bg_fill = Color32::from_gray(62);
    visuals.widgets.active.bg_fill = Color32::from_rgb(35, 86, 140);
    ctx.set_visuals(visuals);
}

fn status_color(app: &RustCastApp) -> Color32 {
    match app.stream_status() {
        StreamStatus::Live => Color32::from_rgb(70, 210, 110),
        StreamStatus::Connecting | StreamStatus::Reconnecting => Color32::from_rgb(235, 190, 70),
        StreamStatus::Error => Color32::from_rgb(235, 80, 80),
        StreamStatus::Idle => Color32::from_gray(130),
    }
}

pub fn top_bar(app: &mut RustCastApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("RustCast").color(Color32::from_rgb(120, 180, 255)));
            ui.separator();
            ui.colored_label(status_color(app), format!("● {}", app.stream_status().label()));
            ui.label(format!("| {}", app.record_status().label()));
            ui.separator();
            let enabled = app.engine.is_some();
            let stream_label = if matches!(
                app.stream_status(),
                StreamStatus::Idle | StreamStatus::Error
            ) {
                "▶ Start Streaming"
            } else {
                "■ Stop Streaming"
            };
            if ui.add_enabled(enabled, egui::Button::new(stream_label)).clicked() {
                app.toggle_stream();
            }
            let record_label = if app.record_status() == RecordStatus::Idle {
                "● Start Recording"
            } else {
                "■ Stop Recording"
            };
            if ui.add_enabled(enabled, egui::Button::new(record_label)).clicked() {
                app.toggle_record();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("About").clicked() {
                    app.about_open = true;
                }
                if ui.button("⚙ Settings").clicked() {
                    app.settings_open = true;
                }
            });
        });
    });
}

pub fn scenes_panel(app: &mut RustCastApp, ctx: &egui::Context) {
    egui::SidePanel::left("scenes")
        .default_width(200.0)
        .min_width(160.0)
        .show(ctx, |ui| {
            ui.heading("Scenes");
            let mut selected = app.collection.active_scene;
            for (index, scene) in app.collection.scenes.iter().enumerate() {
                let response = ui.selectable_label(index == selected, scene.name.as_str());
                if response.clicked() {
                    selected = index;
                }
            }
            app.select_scene(selected);

            ui.separator();
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut app.drafts.new_scene_name)
                        .hint_text("New scene name")
                        .desired_width(110.0),
                );
                if ui.button("Add").clicked() {
                    let name = if app.drafts.new_scene_name.trim().is_empty() {
                        format!("Scene {}", app.collection.scenes.len() + 1)
                    } else {
                        app.drafts.new_scene_name.clone()
                    };
                    app.add_scene(name);
                    app.drafts.new_scene_name.clear();
                }
            });
            if ui.button("Delete selected scene").clicked() {
                let index = app.collection.active_scene;
                app.remove_scene(index);
            }

            ui.separator();
            ui.label("Scene collections");
            ui.add(
                egui::TextEdit::singleline(&mut app.drafts.scene_collection_path)
                    .hint_text("Path to scenes.json"),
            );
            ui.horizontal(|ui| {
                if ui.button("Save as…").clicked() {
                    let path = std::path::PathBuf::from(&app.drafts.scene_collection_path);
                    if path.as_os_str().is_empty() {
                        app.log(LogLevel::Warning, "Enter a path first");
                    } else if let Err(err) = app.collection.save_to_file(&path) {
                        app.log(LogLevel::Error, format!("Cannot save scenes: {err}"));
                    } else {
                        app.log(LogLevel::Info, format!("Scenes saved to {}", path.display()));
                    }
                }
                if ui.button("Load").clicked() {
                    let path = std::path::PathBuf::from(&app.drafts.scene_collection_path);
                    match crate::scenes::SceneCollection::load_from_file(&path) {
                        Ok(collection) => {
                            app.collection = collection;
                            app.reload_scene();
                            app.log(LogLevel::Info, format!("Scenes loaded from {}", path.display()));
                        }
                        Err(err) => app.log(LogLevel::Error, format!("Cannot load scenes: {err}")),
                    }
                }
            });
        });
}

pub fn sources_panel(app: &mut RustCastApp, ctx: &egui::Context) {
    egui::SidePanel::left("sources")
        .default_width(230.0)
        .min_width(190.0)
        .show(ctx, |ui| {
            ui.heading("Sources");
            let mut add_kind = None;
            ui.menu_button("+ Add Source", |ui| {
                for kind in SourceKind::all() {
                    if ui.button(kind.label()).clicked() {
                        add_kind = Some(kind);
                        ui.close_menu();
                    }
                }
            });
            if let Some(kind) = add_kind {
                app.add_source(kind);
            }

            let Some(scene) = app.collection.active() else { return };
            let sources: Vec<SourceConfig> = scene.sources.clone();
            for source in &sources {
                ui.horizontal(|ui| {
                    let label = format!("{} {}", if source.visible { "👁" } else { "○" }, source.name);
                    if ui
                        .selectable_label(app.selected_source == Some(source.id), label)
                        .clicked()
                    {
                        app.selected_source = Some(source.id);
                    }
                    ui.colored_label(
                        if source.muted { Color32::GRAY } else { Color32::from_rgb(120, 210, 130) },
                        "♪",
                    );
                });
            }

            ui.separator();
            let Some(selected) = app.selected_source else {
                ui.label("Select a source to edit its properties.");
                return;
            };
            let Some(mut cfg) = app.collection.find_source(selected).cloned() else {
                return;
            };
            ui.collapsing("Properties", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut cfg.name).desired_width(120.0));
                });
                ui.label(format!("Type: {}", cfg.kind.label()));

                match cfg.kind {
                    SourceKind::Webcam | SourceKind::AudioInput | SourceKind::DesktopAudio => {
                        ui.horizontal(|ui| {
                            ui.label("Device");
                            ui.add(
                                egui::TextEdit::singleline(&mut app.drafts.source_device)
                                    .hint_text(match cfg.kind {
                                        SourceKind::Webcam => "/dev/video0",
                                        _ => "default",
                                    })
                                    .desired_width(120.0),
                            );
                        });
                        if ui.button("Use entered device").clicked() {
                            cfg.device = app.drafts.source_device.clone();
                            app.update_source(cfg.clone(), true);
                        }
                        if cfg.kind == SourceKind::AudioInput || cfg.kind == SourceKind::DesktopAudio {
                            ui.horizontal(|ui| {
                                ui.label("Volume");
                                let mut percent = cfg.volume * 100.0;
                                if ui
                                    .add(egui::Slider::new(&mut percent, 0.0..=100.0).suffix("%"))
                                    .changed()
                                {
                                    cfg.volume = percent / 100.0;
                                    app.update_source(cfg.clone(), false);
                                }
                            });
                            if ui.checkbox(&mut cfg.muted, "Mute").clicked() {
                                app.update_source(cfg.clone(), false);
                            }
                        }
                    }
                    SourceKind::Image | SourceKind::VideoFile => {
                        ui.add(
                            egui::TextEdit::singleline(&mut app.drafts.source_path)
                                .hint_text("/path/to/file"),
                        );
                        if ui.button("Use entered path").clicked() {
                            cfg.path = app.drafts.source_path.clone();
                            app.update_source(cfg.clone(), true);
                        }
                        ui.label(format!("Current: {}", if cfg.path.is_empty() { "(none)" } else { cfg.path.as_str() }));
                    }
                    SourceKind::Text => {
                        ui.add(egui::TextEdit::singleline(&mut cfg.text).hint_text("Overlay text"));
                        ui.horizontal(|ui| {
                            ui.label("Font size");
                            ui.add(egui::Slider::new(&mut cfg.font_size, 12..=96));
                        });
                    }
                    SourceKind::Color => {
                        ui.horizontal(|ui| {
                            ui.label("RGB hex");
                            ui.add(
                                egui::TextEdit::singleline(&mut app.drafts.color_hex)
                                    .desired_width(80.0),
                            );
                            if ui.button("Apply").clicked() {
                                if let Ok(value) =
                                    u32::from_str_radix(app.drafts.color_hex.trim_start_matches('#'), 16)
                                {
                                    cfg.color = value & 0x00FF_FFFF;
                                    app.update_source(cfg.clone(), true);
                                } else {
                                    app.log(LogLevel::Warning, "Invalid hex color");
                                }
                            }
                        });
                        ui.label(format!(
                            "#{:06X}",
                            cfg.color & 0x00FF_FFFF
                        ));
                    }
                    SourceKind::ScreenCapture => {}
                }

                if cfg.kind.has_video() {
                    ui.horizontal(|ui| {
                        ui.label("Position");
                        ui.add(egui::DragValue::new(&mut cfg.x).speed(1.0));
                        ui.add(egui::DragValue::new(&mut cfg.y).speed(1.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Size");
                        ui.add(egui::DragValue::new(&mut cfg.width).speed(1.0));
                        ui.add(egui::DragValue::new(&mut cfg.height).speed(1.0));
                    });
                }
                if ui.checkbox(&mut cfg.visible, "Visible").clicked() {
                    app.update_source(cfg.clone(), false);
                }
                if ui.button("Apply properties").clicked() {
                    app.update_source(cfg, true);
                }
            });
            if ui.button("Remove source").clicked() {
                app.remove_selected_source();
            }
        });
}

fn level_bar(ui: &mut egui::Ui, level: crate::audio::Level) {
    let color = if level.peak > 0.85 {
        Color32::from_rgb(235, 80, 80)
    } else if level.peak > 0.6 {
        Color32::from_rgb(235, 190, 70)
    } else {
        Color32::from_rgb(80, 200, 120)
    };
    ui.add(
        egui::ProgressBar::new(level.rms)
            .desired_width(90.0)
            .fill(color),
    );
}

pub fn central_area(app: &mut RustCastApp, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("Preview");
        let available = ui.available_size();
        let preview_height = (available.y * 0.62).max(200.0);
        egui::Frame::canvas(ui.style()).show(ui, |ui| {
            if let Some(texture) = &app.preview_texture {
                let (w, h) = (app.preview_size.0 as f32, app.preview_size.1 as f32);
                let sized = egui::load::SizedTexture::new(texture.id(), Vec2::new(w, h));
                ui.add(
                    egui::Image::from_texture(sized)
                        .max_size(Vec2::new(available.x - 16.0, preview_height - 16.0))
                        .maintain_aspect_ratio(true),
                );
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        app.engine_error
                            .as_deref()
                            .unwrap_or("No preview yet — add a screen, webcam or color source"),
                    );
                });
            }
        });

        ui.separator();
        ui.heading("Audio Mixer");
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                let sources: Vec<SourceConfig> = app
                    .collection
                    .active()
                    .map(|scene| scene.sources.clone())
                    .unwrap_or_default();
                if !sources.is_empty() {
                    for source in &sources {
                        if !source.kind.has_audio() {
                            continue;
                        }
                        ui.vertical(|ui| {
                            ui.set_min_width(140.0);
                            ui.label(&source.name);
                            let mut percent = source.volume * 100.0;
                            if ui
                                .add(egui::Slider::new(&mut percent, 0.0..=100.0).suffix("%"))
                                .changed()
                            {
                                let mut cfg = source.clone();
                                cfg.volume = percent / 100.0;
                                app.update_source(cfg, false);
                            }
                            let mut muted = source.muted;
                            if ui.checkbox(&mut muted, "Mute").clicked() {
                                let mut cfg = source.clone();
                                cfg.muted = muted;
                                app.update_source(cfg, false);
                            }
                            level_bar(ui, app.level_for(source.id));
                        });
                        ui.separator();
                    }
                }
                ui.vertical(|ui| {
                    ui.set_min_width(140.0);
                    ui.label(RichText::new("Master").strong());
                    ui.label(volume_to_db(1.0));
                    level_bar(ui, app.master_level());
                });
            });
        });

        ui.separator();
        egui::CollapsingHeader::new("Event log")
            .default_open(false)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                    for entry in app.logs.iter().rev() {
                        let color = match entry.level {
                            LogLevel::Info => Color32::LIGHT_GRAY,
                            LogLevel::Warning => Color32::from_rgb(235, 190, 70),
                            LogLevel::Error => Color32::from_rgb(235, 90, 90),
                        };
                        ui.colored_label(
                            color,
                            format!("{} {}", entry.time.format("%H:%M:%S"), entry.message),
                        );
                    }
                });
            });
    });
}

fn settings_body(app: &mut RustCastApp, ui: &mut egui::Ui) {
    let original = app.settings.clone();
    let mut settings: AppSettings = original.clone();

    ui.heading("Video & Encoder");
    ui.horizontal(|ui| {
        ui.label("Width");
        ui.add(egui::Slider::new(&mut settings.video.width, 320..=3840));
        ui.label("Height");
        ui.add(egui::Slider::new(&mut settings.video.height, 240..=2160));
    });
    ui.horizontal(|ui| {
        ui.label("FPS");
        ui.add(egui::Slider::new(&mut settings.video.fps, 10..=60));
        ui.label("Bitrate (kbps)");
        ui.add(egui::Slider::new(&mut settings.video.bitrate_kbps, 200..=20_000));
    });
    ui.horizontal(|ui| {
        ui.label("x264 threads");
        ui.add(egui::Slider::new(&mut settings.video.encoder_threads, 1..=8));
    });
    ui.label("480p / 30 FPS / 1200 kbps is recommended for low-end CPUs.");

    ui.separator();
    ui.heading("Streaming");
    ui.horizontal(|ui| {
        ui.label("Server");
        ui.add(
            egui::TextEdit::singleline(&mut settings.stream.server_url)
                .hint_text("rtmp://live.example.com/live")
                .desired_width(280.0),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Stream key");
        ui.add(
            egui::TextEdit::singleline(&mut settings.stream.stream_key)
                .password(true)
                .desired_width(280.0),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Bitrate (kbps)");
        ui.add(egui::Slider::new(&mut settings.stream.bitrate_kbps, 200..=20_000));
        ui.checkbox(&mut settings.stream.reconnect, "Auto-reconnect");
    });
    ui.label("The stream key is stored locally in your user config directory, never in the repository.");

    ui.separator();
    ui.heading("Recording");
    ui.horizontal(|ui| {
        ui.label("Format");
        egui::ComboBox::from_label("")
            .selected_text(settings.record.format.to_string())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut settings.record.format, RecordFormat::Mkv, "MKV (recommended)");
                ui.selectable_value(&mut settings.record.format, RecordFormat::Mp4, "MP4");
            });
        ui.label("Bitrate (kbps)");
        ui.add(egui::Slider::new(&mut settings.record.bitrate_kbps, 200..=40_000));
    });
    ui.horizontal(|ui| {
        ui.label("Directory");
        ui.add(
            egui::TextEdit::singleline(&mut app.drafts.record_dir)
                .desired_width(280.0),
        );
    });
    settings.record.directory = std::path::PathBuf::from(app.drafts.record_dir.clone());
    ui.horizontal(|ui| {
        ui.label("File prefix");
        ui.add(
            egui::TextEdit::singleline(&mut settings.record.basename)
                .desired_width(120.0),
        );
        ui.checkbox(
            &mut settings.record.use_output_resolution,
            "Use stream resolution",
        );
    });
    if !settings.record.use_output_resolution {
        ui.horizontal(|ui| {
            ui.label("Width");
            ui.add(egui::Slider::new(&mut settings.record.width, 320..=3840));
            ui.label("Height");
            ui.add(egui::Slider::new(&mut settings.record.height, 240..=2160));
            ui.label("FPS");
            ui.add(egui::Slider::new(&mut settings.record.fps, 10..=60));
        });
    }

    ui.separator();
    ui.heading("Audio");
    ui.horizontal(|ui| {
        ui.label("Sample rate");
        egui::ComboBox::from_label("")
            .selected_text(format!("{} Hz", settings.audio.sample_rate))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut settings.audio.sample_rate, 48_000, "48000 Hz");
                ui.selectable_value(&mut settings.audio.sample_rate, 44_100, "44100 Hz");
            });
        ui.label("AAC kbps");
        ui.add(egui::Slider::new(&mut settings.audio.aac_bitrate_kbps, 64..=320));
    });
    ui.checkbox(
        &mut settings.audio.noise_suppression,
        "Noise suppression (requires rnnoise plugin)",
    );
    ui.checkbox(&mut settings.audio.monitor, "Monitor audio (requires output device)");

    ui.separator();
    let needs_restart = original.video != settings.video
        || original.audio.sample_rate != settings.audio.sample_rate
        || original.audio.noise_suppression != settings.audio.noise_suppression
        || original.audio.monitor != settings.audio.monitor
        || original.record != settings.record;
    if ui.button("Save settings").clicked() {
        app.apply_settings(settings, needs_restart);
        app.log(
            LogLevel::Info,
            if needs_restart {
                "Settings saved; media pipeline will restart"
            } else {
                "Settings saved"
            },
        );
    }
}

pub fn settings_window(app: &mut RustCastApp, ctx: &egui::Context) {
    egui::Window::new("Settings")
        .open(&mut app.settings_open)
        .default_size(Vec2::new(620.0, 640.0))
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| settings_body(app, ui));
        });
}

pub fn about_window(app: &mut RustCastApp, ctx: &egui::Context) {
    egui::Window::new("About RustCast")
        .open(&mut app.about_open)
        .default_size(Vec2::new(420.0, 260.0))
        .resizable(false)
        .show(ctx, |ui| {
            ui.heading("RustCast 0.1.0");
            ui.label("A lightweight, native streaming and recording app for Linux.");
            ui.label("Built with Rust, egui and GStreamer. Licensed under Apache-2.0.");
            ui.separator();
            ui.label("No OBS source code is used. Media processing is delegated to mature GStreamer plugins.");
            ui.hyperlink("https://github.com/Llen0ir3Dev/RustCast");
        });
}
