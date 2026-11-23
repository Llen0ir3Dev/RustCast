//! Construction of per-source GStreamer bins.
//!
//! Every source bin exposes up to two ghost pads named `video_src` and
//! `audio_src`. The engine links those pads into the scene compositor and the
//! audio adder. Source bins are deliberately independent of each other so a
//! failing device can be removed without tearing down the whole pipeline.

use anyhow::{anyhow, Context, Result};
use gstreamer as gst;
use gst::prelude::*;
use gstreamer_video as gst_video;

use crate::scenes::{SourceConfig, SourceId, SourceKind};
use crate::settings::AppSettings;

pub struct SourceBins {
    pub video: Option<gst::Bin>,
    pub audio: Option<gst::Bin>,
    pub volume: Option<gst::Element>,
}

pub fn element(factory: &str, name: &str) -> Result<gst::Element> {
    gst::ElementFactory::make(factory)
        .name(name)
        .build()
        .with_context(|| {
            format!(
                "GStreamer element '{factory}' is unavailable; install the matching gstreamer plugin package"
            )
        })
}

fn raw_video_caps(width: u32, height: u32, fps: u32) -> gst::Caps {
    gst::Caps::builder("video/x-raw")
        .field("format", gst_video::VideoFormat::I420.to_str())
        .field("width", width as i32)
        .field("height", height as i32)
        .field("framerate", gst::Fraction::new(fps as i32, 1))
        .build()
}

fn raw_audio_caps(rate: u32, channels: i32) -> gst::Caps {
    gst::Caps::builder("audio/x-raw")
        .field("format", "S16LE")
        .field("rate", rate as i32)
        .field("channels", channels)
        .build()
}

/// Builds the common video tail: convert, scale, rate-limit, capsfilter.
fn video_tail(
    bin: &gst::Bin,
    cfg: &SourceConfig,
    settings: &AppSettings,
) -> Result<(gst::Pad, gst::GhostPad)> {
    let convert = element("videoconvert", &format!("vconv_{}", cfg.id))?;
    let scale = element("videoscale", &format!("vscale_{}", cfg.id))?;
    let rate = element("videorate", &format!("vrate_{}", cfg.id))?;
    let capsfilter = element("capsfilter", &format!("vcaps_{}", cfg.id))?;
    capsfilter.set_property(
        "caps",
        raw_video_caps(cfg.width, cfg.height, settings.video.fps),
    );
    let queue = element("queue", &format!("vqueue_{}", cfg.id))?;
    queue.set_property("max-buffers", 3u32);
    queue.set_property("drop", true);

    bin.add_many(&[&convert, &scale, &rate, &capsfilter, &queue])?;
    convert.link(&scale)?;
    scale.link(&rate)?;
    rate.link(&capsfilter)?;
    capsfilter.link(&queue)?;

    let src_pad = queue
        .static_pad("src")
        .ok_or_else(|| anyhow!("video queue has no src pad"))?;
    let ghost = gst::GhostPad::builder_with_target(&src_pad)?
        .name("video_src")
        .build();
    Ok((convert.static_pad("sink").context("video convert sink pad")?, ghost))
}

/// Builds the common audio tail with optional noise suppression and exposes
/// the volume element so the mixer can control gain at the source.
fn audio_tail(
    bin: &gst::Bin,
    cfg: &SourceConfig,
    settings: &AppSettings,
) -> Result<(gst::Pad, gst::GhostPad, gst::Element)> {
    let denoise = if settings.audio.noise_suppression
        && gst::ElementFactory::find("rnnoise").is_some()
    {
        Some(element("rnnoise", &format!("denoise_{}", cfg.id))?)
    } else {
        if settings.audio.noise_suppression {
            log::warn!("Noise suppression requested but the 'rnnoise' element is not installed");
        }
        None
    };

    let channels = if denoise.is_some() { 1 } else { 2 };
    let convert = element("audioconvert", &format!("aconv_{}", cfg.id))?;
    let resample = element("audioresample", &format!("aresample_{}", cfg.id))?;
    let capsfilter = element("capsfilter", &format!("acaps_{}", cfg.id))?;
    capsfilter.set_property("caps", raw_audio_caps(settings.audio.sample_rate, channels));

    bin.add_many(&[&convert, &resample, &capsfilter])?;
    convert.link(&resample)?;
    resample.link(&capsfilter)?;

    let mut tail = capsfilter.clone();
    if let Some(denoise) = denoise {
        bin.add(&denoise)?;
        tail.link(&denoise)?;
        let back = element("audioconvert", &format!("denoise_back_{}", cfg.id))?;
        let stereo = element("capsfilter", &format!("denoise_caps_{}", cfg.id))?;
        stereo.set_property("caps", raw_audio_caps(settings.audio.sample_rate, 2));
        bin.add_many(&[&back, &stereo])?;
        denoise.link(&back)?;
        back.link(&stereo)?;
        tail = stereo;
    }

    let volume = element("volume", &format!("volume_{}", cfg.id))?;
    volume.set_property("volume", cfg.volume as f64);
    volume.set_property("mute", cfg.muted);
    let level = element("level", &format!("level_{}", cfg.id))?;
    level.set_property("interval", 100_000_000u64);
    bin.add_many(&[&volume, &level])?;
    tail.link(&volume)?;
    volume.link(&level)?;

    let src_pad = level
        .static_pad("src")
        .ok_or_else(|| anyhow!("level element has no src pad"))?;
    let ghost = gst::GhostPad::builder_with_target(&src_pad)?
        .name("audio_src")
        .build();
    Ok((convert.static_pad("sink").context("audio convert sink pad")?, ghost, volume))
}

fn capture_element(cfg: &SourceConfig) -> Result<gst::Element> {
    match cfg.kind {
        SourceKind::ScreenCapture => {
            if std::env::var("WAYLAND_DISPLAY").is_ok() && gst::ElementFactory::find("pipewiresrc").is_some() {
                element("pipewiresrc", &format!("capture_{}", cfg.id))
            } else {
                let src = element("ximagesrc", &format!("capture_{}", cfg.id))?;
                src.set_property("use-damage", false);
                src.set_property("remote", false);
                if !cfg.device.is_empty() {
                    src.set_property("display-name", &cfg.device);
                }
                Ok(src)
            }
        }
        SourceKind::Webcam => {
            let src = element("v4l2src", &format!("webcam_{}", cfg.id))?;
            let device = if cfg.device.is_empty() {
                "/dev/video0".to_string()
            } else {
                cfg.device.clone()
            };
            src.set_property("device", &device);
            Ok(src)
        }
        SourceKind::AudioInput => {
            let device = if cfg.device.is_empty() {
                "default".to_string()
            } else {
                cfg.device.clone()
            };
            if gst::ElementFactory::find("pulsesrc").is_some() {
                let src = element("pulsesrc", &format!("mic_{}", cfg.id))?;
                src.set_property("device", &device);
                Ok(src)
            } else {
                element("autoaudiosrc", &format!("mic_{}", cfg.id))
            }
        }
        SourceKind::DesktopAudio => {
            let device = if cfg.device.is_empty() {
                "default.monitor".to_string()
            } else {
                cfg.device.clone()
            };
            let src = element("pulsesrc", &format!("desktop_audio_{}", cfg.id))?;
            src.set_property("device", &device);
            Ok(src)
        }
        _ => Err(anyhow!("{:?} is not a capture source", cfg.kind)),
    }
}

fn add_video_source(
    bin: &gst::Bin,
    cfg: &SourceConfig,
    settings: &AppSettings,
    source: gst::Element,
) -> Result<()> {
    let (video_input, ghost) = video_tail(bin, cfg, settings)?;
    bin.add(&source)?;
    source.link(&video_input)?;
    bin.add_pad(&ghost)?;
    Ok(())
}

fn add_decoded_source(
    bin: &gst::Bin,
    cfg: &SourceConfig,
    settings: &AppSettings,
    source: gst::Element,
    freeze: bool,
    with_audio: bool,
) -> Result<Option<gst::Element>> {
    let decodebin = element("decodebin", &format!("decode_{}", cfg.id))?;
    bin.add_many(&[&source, &decodebin])?;
    source.link(&decodebin)?;

    let (video_input, video_ghost) = video_tail(bin, cfg, settings)?;
    bin.add_pad(&video_ghost)?;

    let mut video_target = video_input;
    if freeze {
        let imagefreeze = element("imagefreeze", &format!("freeze_{}", cfg.id))?;
        bin.add(&imagefreeze)?;
        imagefreeze.link(&video_target)?;
        video_target = imagefreeze
            .static_pad("sink")
            .context("imagefreeze sink pad")?;
    }

    let (audio_input, volume) = if with_audio {
        let (input, ghost, volume) = audio_tail(bin, cfg, settings)?;
        bin.add_pad(&ghost)?;
        (Some(input), Some(volume))
    } else {
        (None, None)
    };

    decodebin.connect_pad_added(move |_, pad| {
        let caps = pad.current_caps().unwrap_or_else(|| pad.query_caps(None));
        let Some(structure) = caps.structure(0) else {
            return;
        };
        let name = structure.name();
        if name.starts_with("video/") {
            let _ = pad.link(&video_target);
        } else if name.starts_with("audio/") {
            if let Some(input) = &audio_input {
                let _ = pad.link(input);
            }
        }
    });
    Ok(volume)
}

/// Creates the source bins for one source configuration.
pub fn build_source(cfg: &SourceConfig, settings: &AppSettings) -> Result<SourceBins> {
    let id: SourceId = cfg.id;
    match cfg.kind {
        SourceKind::ScreenCapture | SourceKind::Webcam => {
            let bin = gst::Bin::new(Some(&format!("bin_{id}_{:?}", cfg.kind)));
            let source = capture_element(cfg)?;
            add_video_source(&bin, cfg, settings, source)?;
            Ok(SourceBins {
                video: Some(bin),
                audio: None,
                volume: None,
            })
        }
        SourceKind::AudioInput | SourceKind::DesktopAudio => {
            let bin = gst::Bin::new(Some(&format!("bin_{id}_{:?}", cfg.kind)));
            let source = capture_element(cfg)?;
            let (input, ghost, volume) = audio_tail(&bin, cfg, settings)?;
            bin.add(&source)?;
            source.link(&input)?;
            bin.add_pad(&ghost)?;
            Ok(SourceBins {
                video: None,
                audio: Some(bin),
                volume: Some(volume),
            })
        }
        SourceKind::Image | SourceKind::VideoFile => {
            let bin = gst::Bin::new(Some(&format!("bin_{id}_{:?}", cfg.kind)));
            if cfg.path.is_empty() || !std::path::Path::new(&cfg.path).is_file() {
                return Err(anyhow!(
                    "File '{}' does not exist; choose a valid media file",
                    cfg.path
                ));
            }
            let filesrc = element("filesrc", &format!("filesrc_{id}"))?;
            filesrc.set_property("location", &cfg.path);
            let volume = add_decoded_source(
                &bin,
                cfg,
                settings,
                filesrc,
                cfg.kind == SourceKind::Image,
                cfg.kind == SourceKind::VideoFile,
            )?;
            let audio = if cfg.kind == SourceKind::VideoFile {
                Some(bin.clone())
            } else {
                None
            };
            Ok(SourceBins {
                video: Some(bin),
                audio,
                volume,
            })
        }
        SourceKind::Text | SourceKind::Color => {
            let bin = gst::Bin::new(Some(&format!("bin_{id}_{:?}", cfg.kind)));
            let base = element("color", &format!("color_{id}"))?;
            let rgba = if cfg.kind == SourceKind::Text {
                0x0000_0000u32 // fully transparent; textoverlay draws the visible pixels
            } else {
                0xFF00_0000u32 | cfg.color
            };
            base.set_property("color", rgba);
            let (video_input, ghost) = video_tail(&bin, cfg, settings)?;

            if cfg.kind == SourceKind::Text {
                let alpha = element("alpha", &format!("alpha_{id}"))?;
                alpha.set_property("alpha", 0.0f64);
                let overlay = element("textoverlay", &format!("text_{id}"))?;
                overlay.set_property("text", &cfg.text);
                overlay.set_property("font-desc", format!("Sans {}", cfg.font_size));
                overlay.set_property("shaded-background", false);
                bin.add_many(&[&base, &alpha, &overlay])?;
                base.link(&alpha)?;
                alpha.link(&overlay)?;
                overlay.link(&video_input)?;
            } else {
                bin.add(&base)?;
                base.link(&video_input)?;
            }
            bin.add_pad(&ghost)?;
            Ok(SourceBins {
                video: Some(bin),
                audio: None,
                volume: None,
            })
        }
    }
}
