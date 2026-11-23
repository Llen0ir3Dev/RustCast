# Performance policy and measurement

RustCast is designed for low-end Linux systems (for example, a Bodhi Linux 7 machine with an AMD
Radeon R3 APU and about 6.7 GB RAM), but **design intent is not evidence**. No FPS, CPU or RAM
numbers are claimed until they are measured on real hardware.

## Design choices that reduce load

- CPU-only x264 with a default of two threads, 854x480 at 30 FPS and 1200 kbps.
- Preview frames are dropped when the UI is late (`appsink max-buffers=1`, `drop=true`).
- One reusable RGBA texture; no per-frame texture allocation after startup.
- No background helper processes; all capture/encode/mux work happens inside one GStreamer pipeline.
- Output branches are created only while recording or streaming.
- `panic = "abort"`, `lto = "thin"`, `strip = true` in the release profile.

## How to measure

1. Install runtime dependencies and build with `cargo build --release`.
2. Start the app with `RUST_LOG=info ./target/release/rustcast`.
3. Create a scene with a screen capture source and (optionally) a microphone source.
4. Warm up for 60 seconds.
5. Sample CPU and memory:

   ```bash
   pid=$(pgrep -f target/release/rustcast | head -1)
   ps -o pid,rss,vsz,pcpu,etime,cmd -p "$pid"
   top -b -n 5 -p "$pid" | tail -20
   ```

6. Record for five minutes to MKV, then compare dropped frames and file integrity:

   ```bash
   ffprobe -v error -show_entries format=duration,size -of default=noprint_wrappers=1 recording.mkv
   gst-discoverer-1.0 recording.mkv
   ```

7. For streaming, use a local RTMP server (`nginx-rtmp` or `MediaMTX`) and verify with `ffprobe`:

   ```bash
   ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1 rtmp://localhost/live/test
   ```

8. Record results in an issue with hardware, distro, resolution, FPS, bitrate, average CPU, peak RSS
   and observed drops.

## Baseline target (to verify, not a claim)

| Setting | Target |
| --- | --- |
| Output | 854x480 @ 30 FPS |
| Video bitrate | 1200 kbps |
| x264 threads | 2 |
| Idle RSS | measure on device |
| Recording RSS | measure on device |
| Sustained FPS | measure on device |
