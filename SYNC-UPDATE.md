# Starling Extra — upstream sync report

## Source baseline

- Starling input: the uploaded `starling-extras-starling (6).zip` source tree.
- Upstream input: the uploaded `horizon-source.tar.gz`, source snapshot `9f42064` (Nightly 2026-11-13).
- Target game version stated by both project trees: Minecraft Java Edition 26.3.

## Merge policy

- New upstream files and common, non-custom files were brought forward from the uploaded Horizon snapshot.
- All Starling-only files were retained (1446 source-only paths found in the Starling tree).
- Overlay-manifest source paths were retained: 384 added entries and 50 modified entries listed (the manifest header itself declares 409 added files; only 384 source paths are enumerated in its ADDED section).
- Additional shared files with Starling/mobile feature markers were retained (224 paths), including Android, Bedrock-style inventory/control, gyroscope/haptics, native-mod and custom HUD code.
- Starling project identity, Android build inputs, winit Android input patch, project docs and user build profiles were kept.
- Root Cargo workspace was merged so it includes the upstream crates plus `starling-catalog`, `horizon-android`, and `starling-account`, and keeps Starling's patched `winit` and `quick` APK profile.

## Important limits

This is a **conservative source sync**, not a verified release build. The environment used to prepare this archive does not have `cargo` or `rustc`, so Rust compilation, Android APK packaging, runtime touch testing, world loading, shader-pack testing and native-mod execution could not be run. The existing `Cargo.lock` was preserved; Cargo may need to refresh it on the first local build.

The uploaded Horizon snapshot is dated 2026-11-13. Horizon's public `main` advanced after that snapshot (including the native/WASM mod API merge on 2026-11-16); those later commits are not included because the supplied archive does not contain them. Do not treat this archive as a build signed or released by upstream.

## Static checks completed

- Cargo manifest parses as TOML.
- JSON and newline-delimited JSON fixtures were parsed where applicable.
- Starling overlay additions/modifications were checked for path presence; historical manifest paths absent from the uploaded source are listed below.

## Manifest paths absent from the uploaded Starling tree

- `.github/upstream.json`
- `.github/workflows/patch-runner.yml`
