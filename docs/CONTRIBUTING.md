# Contributing

## Ground rules

- Keep the media engine independent of the GUI.
- Never copy OBS Studio source code. Inspiration from public architecture discussions is fine;
  implementation must be our own.
- Never commit stream keys, passwords, API keys, `.env` files or local config.
- Prefer small, working changes over large speculative frameworks.
- Every user-visible button must perform a real action.

## Workflow

1. Fork and create a topic branch.
2. Make changes with `cargo fmt`.
3. Run locally if native dependencies are available, otherwise let GitHub Actions compile:

   ```bash
   cargo test --all-targets
   cargo clippy --all-targets -- -D warnings
   ```

4. Open a pull request describing what changed and how it was verified.

## Adding a source type

1. Extend `SourceKind` and its helpers in `src/scenes/mod.rs`.
2. Add construction logic in `src/media/sources.rs`, exposing ghost pads and reusing the shared
   video/audio tails.
3. Add a UI entry in `src/ui/mod.rs`.
4. Document required GStreamer plugins.
5. Test with a real device or file before marking it complete.

## Reporting performance

Include hardware, distro, resolution/FPS/bitrate, command used, average CPU, peak RSS and any
dropped frames. Follow [PERFORMANCE.md](PERFORMANCE.md).
