//! The media engine owns one GStreamer pipeline and is completely independent
//! from the GUI. It exposes a small command API plus a pollable event queue.

use anyhow::{anyhow, Context, Result};
use gstreamer as gst;
use gst::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::sources::{build_source, element};
use crate::audio::LevelStore;
use crate::recording::RecordStatus;
use crate::scenes::{SceneCollection, SourceId};
use crate::settings::{AppSettings, RecordFormat};
use crate::streaming::StreamStatus;
use crate::video::{pack_rgba, PreviewFrame};

#[derive(Clone, Debug)]
pub enum EngineEvent {
    Info(String),
    Warning(String),
    Error(String),
    SourceFailed(SourceId, String),
    StreamDisconnected(String),
    RecordingFinished(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BranchKind {
    Stream,
    Record,
}

struct OutputBranch {
    kind: BranchKind,
    video_tee_pad: gst::Pad,
    audio_tee_pad: Option<gst::Pad>,
    elements: Vec<gst::Element>,
    path: Option<PathBuf>,
}

struct ActiveSource {
    id: SourceId,
    video_bin: Option<gst::Bin>,
    audio_bin: Option<gst::Bin>,
    compositor_pad: Option<gst::Pad>,
    mixer_pad: Option<gst::Pad>,
    volume: Option<gst::Element>,
}

impl Drop for ActiveSource {
    fn drop(&mut self) {
        if let Some(pad) = &self.compositor_pad {
            if let Some(peer) = pad.peer() {
                let _ = pad.unlink(&peer);
            }
        }
        if let Some(pad) = &self.mixer_pad {
            if let Some(peer) = pad.peer() {
                let _ = pad.unlink(&peer);
            }
        }
    }
}

pub struct MediaEngine {
    pipeline: gst::Pipeline,
    bus: gst::Bus,
    compositor: gst::Element,
    mixer: gst::Element,
    encoded_video_tee: gst::Element,
    encoded_audio_tee: gst::Element,
    x264: gst::Element,
    aac: gst::Element,
    preview_sink: gst_app::AppSink,
    source_bins: HashMap<SourceId, ActiveSource>,
    settings: AppSettings,
    stream_branch: Option<OutputBranch>,
    record_branch: Option<OutputBranch>,
    stream_wanted: bool,
    stream_status: StreamStatus,
    record_status: RecordStatus,
    levels: LevelStore,
    events: Vec<EngineEvent>,
    retry_at: Option<Instant>,
    retries: u32,
}

fn caps_video(width: u32, height: u32, fps: u32) -> gst::Caps {
    gst::Caps::builder("video/x-raw")
        .field("format", gst_video::VideoFormat::I420.to_str())
        .field("width", width as i32)
        .field("height", height as i32)
        .field("framerate", gst::Fraction::new(fps as i32, 1))
        .build()
}

fn caps_audio(rate: u32) -> gst::Caps {
    gst::Caps::builder("audio/x-raw")
        .field("format", "S16LE")
        .field("rate", rate as i32)
        .field("channels", 2)
        .build()
}

fn queue(name: &str) -> Result<gst::Element> {
    let q = element("queue", name)?;
    q.set_property("max-buffers", 4u32);
    q.set_property("drop", true);
    Ok(q)
}

impl MediaEngine {
    pub fn new(mut settings: AppSettings) -> Result<Self> {
        gst::init().context("initialising GStreamer")?;
        settings.sanitize();

        let pipeline = gst::Pipeline::new();
        let bus = pipeline
            .bus()
            .ok_or_else(|| anyhow!("pipeline has no message bus"))?;

        let compositor = element("compositor", "scene_compositor")?;
        let output_convert = element("videoconvert", "output_convert")?;
        let output_caps = element("capsfilter", "output_caps")?;
        output_caps.set_property(
            "caps",
            caps_video(settings.video.width, settings.video.height, settings.video.fps),
        );
        let video_tee = element("tee", "composed_video_tee")?;

        let preview_queue = queue("preview_queue")?;
        let preview_element = element("appsink", "preview_sink")?;
        let preview_sink = preview_element
            .downcast::<gst_app::AppSink>()
            .map_err(|_| anyhow!("appsink element has the wrong type"))?;
        preview_sink.set_caps(Some(&gst::Caps::builder("video/x-raw")
            .field("format", gst_video::VideoFormat::Rgba.to_str())
            .field("width", settings.video.width as i32)
            .field("height", settings.video.height as i32)
            .field("framerate", gst::Fraction::new(settings.video.fps as i32, 1))
            .build()));
        preview_sink.set_async(false);
        preview_sink.set_sync(false);
        preview_sink.set_max_buffers(1);
        preview_sink.set_drop(true);

        let encode_video_queue = queue("encode_video_queue")?;
        let encode_convert = element("videoconvert", "encode_convert")?;
        let x264 = element("x264enc", "x264_cpu_encoder")?;
        x264.set_property("bitrate", settings.video.bitrate_kbps);
        x264.set_property("key-int-max", settings.video.fps * 2);
        x264.set_property("threads", settings.video.encoder_threads);
        let h264parse = element("h264parse", "h264parse")?;
        let encoded_video_tee = element("tee", "encoded_video_tee")?;

        let mixer = element("adder", "audio_adder")?;
        let master_convert = element("audioconvert", "master_convert")?;
        let master_resample = element("audioresample", "master_resample")?;
        let master_caps = element("capsfilter", "master_caps")?;
        master_caps.set_property("caps", caps_audio(settings.audio.sample_rate));
        let master_level = element("level", "master_level")?;
        master_level.set_property("interval", 100_000_000u64);
        let audio_tee = element("tee", "composed_audio_tee")?;

        let encode_audio_queue = queue("encode_audio_queue")?;
        let aac = ["avenc_aac", "faac", "voaacenc"]
            .iter()
            .find_map(|name| gst::ElementFactory::make(name).name("audio_encoder").build().ok())
            .ok_or_else(|| anyhow!("No AAC encoder found (install gstreamer1.0-libav)"))?;
        aac.set_property("bitrate", (settings.audio.aac_bitrate_kbps * 1000) as i32);
        let encoded_audio_tee = element("tee", "encoded_audio_tee")?;

        for e in [
            &compositor,
            &output_convert,
            &output_caps,
            &video_tee,
            &preview_queue,
            preview_sink.upcast_ref::<gst::Element>(),
            &encode_video_queue,
            &encode_convert,
            &x264,
            &h264parse,
            &encoded_video_tee,
            &mixer,
            &master_convert,
            &master_resample,
            &master_caps,
            &master_level,
            &audio_tee,
            &encode_audio_queue,
            &aac,
            &encoded_audio_tee,
        ] {
            pipeline.add(e)?;
        }

        compositor.link(&output_convert)?;
        output_convert.link(&output_caps)?;
        output_caps.link(&video_tee)?;
        video_tee.link(&preview_queue)?;
        preview_queue.link(preview_sink.upcast_ref::<gst::Element>())?;
        video_tee.link(&encode_video_queue)?;
        encode_video_queue.link(&encode_convert)?;
        encode_convert.link(&x264)?;
        x264.link(&h264parse)?;
        h264parse.link(&encoded_video_tee)?;

        mixer.link(&master_convert)?;
        master_convert.link(&master_resample)?;
        master_resample.link(&master_caps)?;
        master_caps.link(&master_level)?;
        master_level.link(&audio_tee)?;
        audio_tee.link(&encode_audio_queue)?;
        encode_audio_queue.link(&aac)?;
        aac.link(&encoded_audio_tee)?;

        pipeline.set_state(gst::State::Playing)?;

        Ok(Self {
            pipeline,
            bus,
            compositor,
            mixer,
            encoded_video_tee,
            encoded_audio_tee,
            x264,
            aac,
            preview_sink,
            source_bins: HashMap::new(),
            settings,
            stream_branch: None,
            record_branch: None,
            stream_wanted: false,
            stream_status: StreamStatus::Idle,
            record_status: RecordStatus::Idle,
            levels: LevelStore::default(),
            events: Vec::new(),
            retry_at: None,
            retries: 0,
        })
    }

    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    /// Applies settings that can change without rebuilding the whole pipeline.
    pub fn update_settings(&mut self, settings: AppSettings) {
        self.x264
            .set_property("bitrate", settings.video.bitrate_kbps);
        self.x264
            .set_property("key-int-max", settings.video.fps * 2);
        self.aac
            .set_property("bitrate", (settings.audio.aac_bitrate_kbps * 1000) as i32);
        self.settings = settings;
    }

    pub fn stream_status(&self) -> StreamStatus {
        self.stream_status
    }

    pub fn record_status(&self) -> RecordStatus {
        self.record_status
    }

    pub fn levels(&self) -> &LevelStore {
        &self.levels
    }

    pub fn has_audio_sources(&self) -> bool {
        self.source_bins.values().any(|s| s.audio_bin.is_some())
    }

    pub fn add_source(&mut self, cfg: &crate::scenes::SourceConfig) -> Result<()> {
        if self.source_bins.contains_key(&cfg.id) {
            return Ok(());
        }
        let bins = build_source(cfg, &self.settings)?;
        let mut active = ActiveSource {
            id: cfg.id,
            video_bin: bins.video,
            audio_bin: bins.audio,
            compositor_pad: None,
            mixer_pad: None,
            volume: bins.volume,
        };

        if let Some(bin) = &active.video_bin {
            self.pipeline.add(bin)?;
            let pad = self
                .compositor
                .request_pad_simple("sink_%u")
                .context("requesting compositor sink pad")?;
            let ghost = bin.static_pad("video_src").context("video_src ghost pad")?;
            ghost.link(&pad)?;
            pad.set_property("xpos", cfg.x);
            pad.set_property("ypos", cfg.y);
            pad.set_property("width", cfg.width as i32);
            pad.set_property("height", cfg.height as i32);
            pad.set_property("alpha", if cfg.visible { 1.0f64 } else { 0.0f64 });
            bin.sync_state_with_parent()?;
            active.compositor_pad = Some(pad);
        }

        if let Some(bin) = &active.audio_bin {
            self.pipeline.add(bin)?;
            let pad = self
                .mixer
                .request_pad_simple("sink_%u")
                .context("requesting adder sink pad")?;
            let ghost = bin.static_pad("audio_src").context("audio_src ghost pad")?;
            ghost.link(&pad)?;
            bin.sync_state_with_parent()?;
            active.mixer_pad = Some(pad);
        }

        if let Some(volume) = &active.volume {
            volume.set_property("volume", cfg.volume as f64);
            volume.set_property("mute", cfg.muted);
        }

        self.source_bins.insert(cfg.id, active);
        self.events.push(EngineEvent::Info(format!(
            "Added source: {}",
            cfg.name
        )));
        Ok(())
    }

    pub fn remove_source(&mut self, id: SourceId) {
        if let Some(active) = self.source_bins.remove(&id) {
            if let Some(bin) = &active.video_bin {
                let _ = bin.set_state(gst::State::Null);
                let _ = self.pipeline.remove(bin);
            }
            if let Some(bin) = &active.audio_bin {
                // Video files share one bin between video and audio.
                if active.video_bin.as_ref() != Some(bin) {
                    let _ = bin.set_state(gst::State::Null);
                    let _ = self.pipeline.remove(bin);
                }
            }
            if let Some(pad) = &active.compositor_pad {
                let _ = self.compositor.release_request_pad(pad);
            }
            if let Some(pad) = &active.mixer_pad {
                let _ = self.mixer.release_request_pad(pad);
            }
        }
    }

    pub fn clear_sources(&mut self) {
        let ids: Vec<SourceId> = self.source_bins.keys().copied().collect();
        for id in ids {
            self.remove_source(id);
        }
    }

    pub fn rebuild_scene(&mut self, collection: &SceneCollection) {
        self.clear_sources();
        if let Some(scene) = collection.active() {
            for source in &scene.sources {
                if let Err(err) = self.add_source(source) {
                    self.events.push(EngineEvent::SourceFailed(source.id, err.to_string()));
                }
            }
        }
    }

    pub fn apply_source_config(&mut self, cfg: &crate::scenes::SourceConfig) {
        let Some(active) = self.source_bins.get_mut(&cfg.id) else {
            return;
        };
        if let Some(pad) = &active.compositor_pad {
            pad.set_property("xpos", cfg.x);
            pad.set_property("ypos", cfg.y);
            pad.set_property("width", cfg.width as i32);
            pad.set_property("height", cfg.height as i32);
            pad.set_property("alpha", if cfg.visible { 1.0f64 } else { 0.0f64 });
        }
        if let Some(volume) = &active.volume {
            volume.set_property("volume", cfg.volume as f64);
            volume.set_property("mute", cfg.muted);
        }
    }

    fn attach_tee(&self, tee: &gst::Element, name: &str) -> Result<(gst::Element, gst::Pad)> {
        let q = queue(name)?;
        self.pipeline.add(&q)?;
        let tee_pad = tee
            .request_pad_simple("src_%u")
            .context("requesting tee source pad")?;
        let sink_pad = q.static_pad("sink").context("queue sink pad")?;
        tee_pad.link(&sink_pad)?;
        q.sync_state_with_parent()?;
        Ok((q, tee_pad))
    }

    fn build_branch(
        &mut self,
        kind: BranchKind,
        bitrate: u32,
        format: Option<RecordFormat>,
        endpoint_or_path: String,
    ) -> Result<OutputBranch> {
        let (mux_factory, mux_name, sink_factory, sink_name) = match kind {
            BranchKind::Stream => (
                "flvmux",
                "stream_mux",
                if gst::ElementFactory::find("rtmp2sink").is_some() {
                    "rtmp2sink"
                } else {
                    "rtmpsink"
                },
                "rtmp_output",
            ),
            BranchKind::Record => (
                format.unwrap_or(RecordFormat::Mkv).muxer(),
                "record_mux",
                "filesink",
                "file_output",
            ),
        };

        let mux = element(mux_factory, mux_name)?;
        if kind == BranchKind::Stream {
            mux.set_property("streamable", true);
        }
        let sink = element(sink_factory, sink_name)?;
        sink.set_property("location", &endpoint_or_path);
        if kind == BranchKind::Record {
            sink.set_property("sync", false);
            sink.set_property("async", false);
        }
        self.pipeline.add_many(&[&mux, &sink])?;
        mux.link(&sink)?;

        let (video_queue, video_tee_pad) =
            self.attach_tee(&self.encoded_video_tee.clone(), "branch_video_queue")?;
        let mux_video_pad = mux
            .request_pad_simple("video_%u")
            .context("requesting mux video pad")?;
        video_queue.link(&mux_video_pad)?;

        let mut elements = vec![video_queue, mux.clone(), sink];
        let audio_tee_pad = if self.has_audio_sources() {
            let (audio_queue, pad) =
                self.attach_tee(&self.encoded_audio_tee.clone(), "branch_audio_queue")?;
            let mux_audio_pad = mux
                .request_pad_simple("audio_%u")
                .context("requesting mux audio pad")?;
            audio_queue.link(&mux_audio_pad)?;
            elements.insert(0, audio_queue);
            Some(pad)
        } else {
            None
        };

        for e in &elements {
            e.sync_state_with_parent()?;
        }

        let path = if kind == BranchKind::Record {
            Some(PathBuf::from(&endpoint_or_path))
        } else {
            None
        };
        Ok(OutputBranch {
            kind,
            video_tee_pad,
            audio_tee_pad,
            elements,
            path,
        })
    }

    fn stop_branch(&mut self, branch: OutputBranch) {
        for pad in [&branch.video_tee_pad, branch.audio_tee_pad.as_ref()]
            .into_iter()
            .flatten()
        {
            if let Some(peer) = pad.peer() {
                let _ = pad.unlink(&peer);
            }
        }
        if let Some(mux) = branch
            .elements
            .iter()
            .find(|e| e.name().contains("mux"))
        {
            let _ = mux.send_event(gst::event::Eos::new());
        }
        let _ = self
            .bus
            .timed_pop_filtered(gst::ClockTime::from_mseconds(500), &[gst::MessageType::Eos]);
        for e in &branch.elements {
            let _ = e.set_state(gst::State::Null);
            let _ = self.pipeline.remove(e);
        }
        let _ = self
            .encoded_video_tee
            .release_request_pad(&branch.video_tee_pad);
        if let Some(pad) = &branch.audio_tee_pad {
            let _ = self.encoded_audio_tee.release_request_pad(pad);
        }
        if branch.kind == BranchKind::Record {
            self.record_status = RecordStatus::Idle;
            if let Some(path) = branch.path {
                self.events.push(EngineEvent::RecordingFinished(path));
            }
        }
    }

    pub fn start_stream(&mut self) -> Result<()> {
        let endpoint = self
            .settings
            .stream
            .endpoint()
            .ok_or_else(|| anyhow!("Configure a valid RTMP server URL and stream key first"))?;
        crate::streaming::validate(&self.settings.stream)
            .map_err(|err| anyhow!(err.to_string()))?;
        if self.stream_branch.is_some() {
            return Ok(());
        }
        let branch = self.build_branch(
            BranchKind::Stream,
            self.settings.stream.bitrate_kbps,
            None,
            endpoint,
        )?;
        self.stream_branch = Some(branch);
        self.stream_wanted = true;
        self.stream_status = StreamStatus::Connecting;
        self.retry_at = None;
        self.retries = 0;
        self.events
            .push(EngineEvent::Info("Connecting to stream server…".into()));
        Ok(())
    }

    pub fn stop_stream(&mut self) {
        self.stream_wanted = false;
        self.retry_at = None;
        if let Some(branch) = self.stream_branch.take() {
            self.stop_branch(branch);
        }
        self.stream_status = StreamStatus::Idle;
        self.events
            .push(EngineEvent::Info("Stream stopped".into()));
    }

    pub fn start_record(&mut self) -> Result<()> {
        if self.record_branch.is_some() {
            return Ok(());
        }
        let path = self.settings.record.output_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating recording directory {}", parent.display()))?;
        }
        let format = self.settings.record.format;
        let branch =
            self.build_branch(BranchKind::Record, self.settings.record.bitrate_kbps, Some(format), path.to_string_lossy().into_owned())?;
        self.record_branch = Some(branch);
        self.record_status = RecordStatus::Recording;
        self.events.push(EngineEvent::Info(format!(
            "Recording started: {}",
            path.display()
        )));
        Ok(())
    }

    pub fn stop_record(&mut self) {
        if let Some(branch) = self.record_branch.take() {
            self.stop_branch(branch);
        }
    }

    /// Copies the newest preview frame without blocking the UI thread.
    pub fn take_preview(&self) -> Option<PreviewFrame> {
        let sample = self.preview_sink.try_pull_sample(gst::ClockTime::ZERO)?;
        let buffer = sample.buffer()?;
        let caps = sample.caps()?;
        let info = gst_video::VideoInfo::from_caps(caps).ok()?;
        let frame = gst_video::VideoFrame::from_buffer_readable(buffer.clone(), &info).ok()?;
        let plane = frame.plane_data(0)?;
        let stride = frame.plane_stride()[0] as usize;
        let width = info.width() as usize;
        let height = info.height() as usize;
        Some(PreviewFrame::new(
            width,
            height,
            pack_rgba(plane, stride, width, height),
        ))
    }

    fn parse_level_message(&mut self, msg: &gst::Message) {
        let gst::MessageView::Element(element) = msg.view() else {
            return;
        };
        let structure = element.structure();
        if structure.name() != "level" {
            return;
        }
        let name = msg
            .src()
            .map(|src| src.name().to_string())
            .unwrap_or_default();
        let linear = |values: Option<gst::glib::Array<f64>>| -> f32 {
            let Some(values) = values else { return 0.0 };
            let db = values
                .as_slice()
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            if db.is_finite() {
                10.0f64.powf(db / 20.0).min(1.0) as f32
            } else {
                0.0
            }
        };
        let rms = linear(structure.get::<gst::glib::Array<f64>>("rms").ok());
        let peak = linear(structure.get::<gst::glib::Array<f64>>("peak").ok());
        self.levels.update(&name, rms, peak);
    }

    /// Drains GStreamer bus messages into the UI-facing event queue.
    pub fn poll(&mut self) -> Vec<EngineEvent> {
        let types = [
            gst::MessageType::Error,
            gst::MessageType::Warning,
            gst::MessageType::Element,
        ];
        while let Some(msg) = self.bus.timed_pop_filtered(gst::ClockTime::ZERO, &types) {
            match msg.view() {
                gst::MessageView::Error(err) => {
                    let source = msg
                        .src()
                        .map(|src| src.name().to_string())
                        .unwrap_or_else(|| "gstreamer".into());
                    let text = err.error().to_string();
                    if source.contains("rtmp") || source.contains("stream_mux") {
                        self.stream_status = StreamStatus::Error;
                        self.events
                            .push(EngineEvent::StreamDisconnected(text.clone()));
                        if self.stream_wanted && self.settings.stream.reconnect {
                            self.retry_at = Some(Instant::now() + Duration::from_secs(2));
                        }
                    } else if let Some(id) = source
                        .strip_prefix("bin_")
                        .and_then(|rest| rest.split('_').next())
                        .and_then(|value| value.parse::<SourceId>().ok())
                    {
                        self.events.push(EngineEvent::SourceFailed(id, text.clone()));
                    }
                    self.events.push(EngineEvent::Error(format!("{source}: {text}")));
                }
                gst::MessageView::Warning(warn) => {
                    let source = msg
                        .src()
                        .map(|src| src.name().to_string())
                        .unwrap_or_else(|| "gstreamer".into());
                    let text = warn.error().to_string();
                    if source.contains("rtmp") && self.stream_wanted {
                        self.stream_status = StreamStatus::Reconnecting;
                        self.retry_at = Some(Instant::now() + Duration::from_secs(2));
                    }
                    self.events
                        .push(EngineEvent::Warning(format!("{source}: {text}")));
                }
                _ => self.parse_level_message(&msg),
            }
        }
        self.levels.decay();
        std::mem::take(&mut self.events)
    }

    /// Performs time-based work such as automatic stream reconnection.
    pub fn tick(&mut self) {
        let due = self
            .retry_at
            .map(|at| Instant::now() >= at)
            .unwrap_or(false);
        if !due || !self.stream_wanted {
            return;
        }
        self.retry_at = None;
        if let Some(branch) = self.stream_branch.take() {
            self.stop_branch(branch);
        }
        self.retries += 1;
        self.stream_status = StreamStatus::Reconnecting;
        match self.start_stream() {
            Ok(()) => {
                self.events.push(EngineEvent::Info(format!(
                    "Reconnect attempt {}",
                    self.retries
                )));
            }
            Err(err) => {
                self.stream_status = StreamStatus::Error;
                self.retry_at = Some(Instant::now() + Duration::from_secs(3));
                self.events.push(EngineEvent::Error(err.to_string()));
            }
        }
    }

    /// Marks a stream as live. Called by the UI once the sink reports output.
    pub fn set_stream_live(&mut self) {
        if self.stream_wanted {
            self.stream_status = StreamStatus::Live;
            self.retry_at = None;
            self.retries = 0;
        }
    }
}

impl Drop for MediaEngine {
    fn drop(&mut self) {
        self.stream_wanted = false;
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
