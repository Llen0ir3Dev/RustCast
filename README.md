# RustCast

A lightweight, native streaming and screen-recording application for Linux, written in Rust.

RustCast aims to be a small, dependable alternative to OBS Studio for low-end hardware. It uses
[GStreamer](https://gstreamer.freedesktop.org/) for capture, mixing, x264 encoding, muxing and
RTMP/RTMPS output, and [egui](https://github.com/emilk/egui) for a compact dark interface.

> **Status:** active early development. The media pipeline, scene/source model, GUI shell,
> recording and RTMP output paths are implemented in code and compiled by GitHub Actions.
> Hardware capture and network streaming still need real-device validation — see
> [docs/PERFORMANCE.md](docs/PERFORMANCE.md) for the honest benchmarking status.

## Goals

- Live streaming over RTMP/RTMPS (x264 + AAC, FLV muxer, `rtmp2sink`/`rtmpsink`).
- Local recording to MKV or MP4 with separate bitrate/format settings.
- Screen capture (X11, PipeWire path for Wayland when available), webcam, image, video file,
  text overlay and color background sources.
- Microphone and desktop audio capture through PulseAudio, per-source volume/mute, level meters,
  optional rnnoise suppression and audio monitoring.
- Multiple scenes with instant switching and save/load of scene collections.
- Low memory use and CPU encoding tuned for 480p30 on modest machines.

## Why these choices

| Concern | Choice | Reason |
| --- | --- | --- |
| GUI | egui / eframe | Small native binary, no web runtime, responsive on weak CPUs |
| Media | GStreamer Rust bindings | Mature capture, mixing, x264, AAC, muxers and RTMP sinks; no codec reinvention |
| Audio | PulseAudio (`pulsesrc`) + GStreamer `adder`/`volume`/`level` | Direct integration with desktop and monitor sources |
| Encoding | `x264enc` | CPU-only, predictable quality/CPU trade-off, no GPU dependency |
| Persistence | JSON in the user config dir | Simple, auditable, no database |

## Supported platform

- **Primary:** Ubuntu 22.04 LTS and Bodhi Linux 7.x (Ubuntu 22.04 base), X11, x86_64 or ARM64.
- Also builds on Ubuntu 24.04.
- Wayland screen capture uses PipeWire when `WAYLAND_DISPLAY` is set and `pipewiresrc` is installed.
- No AMD GPU encoder is required or used.

## Install runtime dependencies (Ubuntu 22.04 / Bodhi)

```bash
sudo apt update
sudo apt install -y \
  gstreamer1.0-tools \
  gstreamer1.0-plugins-base \
  gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-ugly \
  gstreamer1.0-libav \
  gstreamer1.0-rtmp \
  gstreamer1.0-pulseaudio \
  libgtk-3-0 \
  libx11-6
```

`gstreamer1.0-plugins-ugly` provides `x264enc`; `gstreamer1.0-rtmp` provides `rtmp2sink`;
`gstreamer1.0-libav` provides the AAC encoder. Noise suppression additionally needs a plugin that
exposes the `rnnoise` element where available.

## Build from source

```bash
sudo apt install -y \
  cargo rustc pkg-config build-essential \
  libgtk-3-dev libx11-dev libxkbcommon-dev libwayland-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libpulse-dev libasound2-dev

git clone https://github.com/Llen0ir3Dev/RustCast.git
cd RustCast
cargo build --release
./target/release/rustcast
```

Debug build:

```bash
RUST_LOG=debug cargo run
```

GitHub Actions compiles and tests every push on both Ubuntu 22.04 and 24.04. Release archives are
built on Ubuntu 22.04 for wider compatibility.

## Quick start

1. Start RustCast and add a **Screen Capture** or **Color Background** source.
2. Check the Preview panel.
3. For local recording, open **Settings → Recording**, choose MKV or MP4, then click
   **Start Recording**.
4. For streaming, open **Settings → Streaming**, enter an `rtmp://` or `rtmps://` server URL and
   your stream key, then click **Start Streaming**.
5. Add a **Microphone / Audio Input** or **Desktop Audio** source for audio.

Stream keys are saved only in `~/.config/rustcast/config.json`. Never commit that file or paste a
key into an issue.

## Architecture

```
src/
  app/         egui application state and command glue
  ui/          panels, mixer, preview and settings dialogs
  media/       GStreamer pipeline, source bins, output branches (UI-independent)
  scenes/      scene/source data model and scene collection persistence
  audio/       meter smoothing and volume helpers
  video/       preview frame packing
  streaming/   stream status and endpoint validation
  recording/   recording status
  settings/    validated, persisted configuration
  platform/    Linux capability detection
```

The composited output is:

```
sources -> compositor + adder -> x264/AAC -> tee -> [preview | file mux | RTMP mux]
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for details and
[docs/BUILDING.md](docs/BUILDING.md) for dependency notes.

## Performance

- Default output: 854x480, 30 FPS, 1200 kbps video, 160 kbps AAC, 2 x264 threads.
- CPU encoding only; no hardware encoder dependency.
- Media work runs on GStreamer streaming threads, not the UI thread.
- Preview uses a drop-oldest queue and a single RGBA frame buffer.
- **No performance numbers are claimed without measurement.** Run the procedure in
  [docs/PERFORMANCE.md](docs/PERFORMANCE.md) on your hardware and report results.

## Roadmap

- [x] Repository, CI, release workflow, docs and issue templates
- [x] Modular scene/source/settings model with persistence
- [x] GStreamer pipeline with compositor, audio mixer, preview and x264/AAC encoding
- [x] Screen, webcam, image, video file, text and color sources
- [x] MKV/MP4 recording and RTMP/RTMPS output with basic auto-reconnect
- [x] egui dark UI with scenes, sources, mixer, preview and settings
- [ ] Real-hardware validation on Bodhi/Ubuntu 22.04 with screen, webcam, mic and RTMP server
- [ ] Benchmarks and low-end tuning based on measurements
- [ ] Wildcard/transitions and more advanced microphone filters
- [ ] Full Wayland/PipeWire source configurator

## Contributing

See [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md). Please run `cargo fmt`, `cargo test` and
`cargo clippy` before opening a pull request.

## License

Apache-2.0. See [LICENSE](LICENSE).

RustCast is an independent project and does not include or copy OBS Studio source code.
