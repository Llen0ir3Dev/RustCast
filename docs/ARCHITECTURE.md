# Architecture

## Separation of concerns

The media engine is a library module with no egui dependency. It owns exactly one GStreamer
pipeline. The UI sends commands (`add_source`, `start_stream`, ...) and polls events. This keeps the
UI thread free of capture and encoding work and allows the engine to be tested or driven by another
frontend later.

## Pipeline graph

```
                                   +-- preview queue -> appsink (RGBA) -> egui texture
scene sources -> compositor -> tee +
                                   +-- encode queue -> x264enc -> tee -> FLV/RTMP or MKV/MP4 mux

audio sources -> adder -> level -> tee -> AAC enc -> tee -> same muxers
```

* **Compositor sink pads** are requested per video source. `xpos`, `ypos`, `width`, `height` and
  `alpha` are set from `SourceConfig`, which is how visibility and layout are implemented without
  restarting the pipeline.
* **Adder sink pads** are requested per audio source. Each source has a `volume` element and a
  `level` element named `level_<source id>`; the engine parses `level` bus messages to feed meters.
* **Output branches** are attached to encoder tees on demand. Starting a recording or a stream
  creates its own queue + muxer + sink; stopping sends EOS, removes the branch and releases the tee
  pads. Streaming and recording can run together.
* **Preview** is a single appsink with `max-buffers=1` and `drop=true`. The UI pulls the newest RGBA
  frame each repaint and uploads it to one reusable `egui::TextureHandle`.

## Source bins

Each source is built as a `gst::Bin` exposing `video_src` and/or `audio_src` ghost pads:

| Source | GStreamer elements |
| --- | --- |
| Screen (X11) | `ximagesrc` + videoconvert/scale/rate + caps |
| Screen (Wayland) | `pipewiresrc` when available |
| Webcam | `v4l2src device=...` |
| Image | `filesrc -> decodebin -> imagefreeze` |
| Video file | `filesrc -> decodebin` with dynamic video/audio pad linking |
| Text | transparent `color` + `alpha` + `textoverlay` |
| Color | `color` with packed ARGB value |
| Mic / desktop audio | `pulsesrc` (+ optional `rnnoise`) + volume + level |

## Failure handling

* Bus errors and warnings are translated into UI log events.
* Errors from an RTMP sink mark the stream for reconnect; `MediaEngine::tick` retries with a
  bounded backoff while streaming remains enabled.
* Errors from a source bin are reported with the source id. The UI keeps the scene configuration and
  the source can be edited or removed.
* Device disconnection is handled at the GStreamer element level; no separate polling process is
  spawned.

## Settings and secrets

`ConfigStore` writes `~/.config/rustcast/config.json`. Stream keys are stored there only. They are
never logged, never included in diagnostics, and never committed. Scene collections use the same
config directory (`scenes.json`) and can also be saved elsewhere from the Scenes panel.

## Deliberate scope decisions

* One output resolution/fps for the composited program feed. Recording has its own bitrate, format
  and (optionally) output size through a separate mux branch rather than a second scaler pipeline.
* Scene switching rebuilds source bins. This is simple and stable; a seamless crossfade/transition
  system is future work.
* Wayland capture requires PipeWire. No unsafe X11/Wayland compositor protocol code is implemented.
