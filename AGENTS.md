# Repository Guidelines

## Project Structure & Module Organization

This is a Windows-only Rust 2024 desktop application. Keep `src/main.rs` as the thin executable entry point and place reusable behavior in `src/lib.rs` and its modules. Pure image operations belong in `src/core/`; SCRFD face detection lives in `src/detect/`; application state and clipboard integration are under `src/app/`; egui screens, editor components, themes, and toasts are in `src/ui/`. Integration tests and image fixtures live in `tests/`, while `examples/make_sample.rs` generates a sample image for visual checks. Design and contributor notes are maintained in `docs/`.

## Build, Test, and Development Commands

- `cargo build` builds a debug executable for local development.
- `cargo run -- path/to/image.png` launches the GUI with an optional input image.
- `cargo build --release` produces the distributable `target/release/qp-meme-gen.exe`.
- `cargo test` runs unit tests and `tests/e2e.rs`.
- `cargo fmt --all -- --check` verifies standard Rust formatting; run `cargo fmt` to apply it.
- `cargo clippy --all-targets -- -D warnings` rejects lint warnings.
- `cargo run --example make_sample` creates a manual visual-check sample.

The first build downloads `assets/det_10g.onnx` and verifies its SHA-256 in `build.rs`. Later builds can run offline.

## Coding Style & Naming Conventions

Use rustfmt defaults (four-space indentation) and keep Clippy clean. Follow Rust naming: `snake_case` for modules, functions, and tests; `PascalCase` for types and traits; `SCREAMING_SNAKE_CASE` for constants. Prefer small, testable functions in `core` over embedding image logic in UI code. Preserve the existing Chinese user-facing copy and concise module-level documentation.

## Testing Guidelines

Place focused unit tests beside the implementation and cross-module or model-backed scenarios in `tests/e2e.rs`. Name tests after observable behavior, such as `mirror_full_image_is_symmetric`. Store deterministic fixtures in `tests/fixtures/`. There is no numeric coverage gate; changes should cover success paths, boundaries, and regressions. Run formatting, Clippy, and the full test suite before submitting.

## Commit & Pull Request Guidelines

History favors short imperative summaries, sometimes with a Conventional Commit prefix (`feat: add overlay text`). Keep each commit focused. Pull requests should explain user-visible behavior and architecture impact, link relevant issues, list validation commands, and include before/after screenshots for UI changes. Do not commit `target/`, downloaded ONNX weights, partial downloads, or local `qp-meme-gen.toml` settings.
