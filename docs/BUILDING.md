# Building

## Toolchain

Use stable Rust (1.75 or newer). GitHub Actions builds with the current stable toolchain.

## Ubuntu 22.04 / Bodhi Linux

Development packages:

```bash
sudo apt update
sudo apt install -y \
  cargo rustc pkg-config build-essential \
  libgtk-3-dev libx11-dev libxkbcommon-dev libwayland-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libpulse-dev libasound2-dev
```

Runtime plugins:

```bash
sudo apt install -y \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
  gstreamer1.0-libav gstreamer1.0-rtmp gstreamer1.0-pulseaudio
```

Build and run:

```bash
cargo build --release
RUST_LOG=info ./target/release/rustcast
```

## Troubleshooting build errors

- `No package 'gstreamer-1.0' found` — install `libgstreamer1.0-dev`.
- `No package 'gstreamer-app-1.0' found` — install `libgstreamer-plugins-base1.0-dev`.
- GTK/X11 errors from `eframe` — install `libgtk-3-dev` and `libx11-dev`.
- Runtime error about a missing element — install the corresponding plugin package listed above.

## Checks

```bash
cargo fmt --all -- --check
cargo build --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

CI runs the same checks on Ubuntu 22.04 and 24.04.
