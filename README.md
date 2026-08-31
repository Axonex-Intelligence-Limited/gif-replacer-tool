# GIF Replacer Tool

**Status**: Phase 1 (CLI logic) + Phase 2 (Tauri desktop GUI) complete and working.

A desktop app that lets a non-technical user drag in an LVGL-converted `.c` GIF file, pick an emotion slot, and have it renamed, wired into the active EmotionDisplay profile, built, and flashed to the ESP32 board — no terminal required.

## What it does

1. **Convert** — link to the [LVGL Image Converter](https://lvgl.io/tools/imageconverter) (True color + Alpha, C array output) is built into the app as Step 1.
2. **Project path** — set once in-app (Browse or type); auto-saved to `~/.gif-tool-config.json` and reloaded on next launch.
3. **Profile** — the app auto-discovers all profiles under `main/*/gif_profile.h` and shows the currently active one (from `sdkconfig.defaults`). A dropdown lets the user switch the active profile directly from the app (writes `CONFIG_GIF_PROFILE_<NAME>=y` back to `sdkconfig.defaults`).
4. **Default emotion** — the profile's `#define GIF_PROFILE_DEFAULT <emotion>` in `gif_profile.h` can be viewed and changed from the app.
5. **Drop file** — drag-and-drop or click-to-browse a `.c` file; validated as a single `lv_img_dsc_t` declaration with a matching `_map` array.
6. **Pick emotion slot** — radio grid built from the active profile's `gif_table[]` entries.
7. **Serial port** — dropdown auto-populated via `serialport` crate on app launch, with a manual 🔄 Scan button and a manual text-entry fallback. Deduplicated on macOS (see Known Issues below).
8. **Replace & Build & Flash** — renames the array/descriptor symbols to match the target emotion, overwrites `main/<profile>/gif/<emotion>.c`, runs `idf.py build` then `idf.py -p <port> flash`, streaming full output live to an in-app log.

## Tech stack

- **Backend**: Rust + Tauri 1.5 (`src-tauri/`)
- **Frontend**: Vanilla HTML/CSS/JS, no framework (`src/`)
- Config persisted at `~/.gif-tool-config.json` (project path, last serial port)

## Quick start

```bash
cd gif-replacer-tool
npm install
npm run dev      # launches the Tauri app in dev mode
```

First launch: paste or browse to your EmotionDisplay project root (the folder containing `sdkconfig.defaults` and `main/`). Everything else loads automatically.

To package as a standalone `.app`:
```bash
npm run build
# output: src-tauri/target/release/bundle/macos/
```

## Source layout

```
src-tauri/src/
  profile.rs   — profile detection, list/switch profiles, get/set default emotion
  parser.rs    — validates + renames LVGL .c symbols
  builder.rs   — writes gif file, runs idf.py build/flash, captures output
  config.rs    — load/save ~/.gif-tool-config.json
  main.rs      — Tauri commands + event emission (build-output stream)
src/
  index.html, app.js, style.css  — single-page UI, no build step
```

All Rust modules have unit tests. Run with:
```bash
cd src-tauri && cargo test
```
11 tests currently pass (profile, parser, builder, config).

## Known issues

- **Stray home-directory git repo (historical)**: `/Users/anthony/.git` still exists (unrelated to this project). This project now has its own git repo (scoped to `gif-replacer-tool/`), so `git status`/`git log` no longer walk up into it.

## Not yet implemented (planned)

- **Profile clone + reset workflow**: instead of editing base profiles (`floki`/`bduck`/`pengu`) directly, users would create numbered clones (`floki_1`, etc.) to experiment on, with a "Reset to Default" button that deletes the clone and re-copies from the base profile. Design is drafted (see conversation history) but not implemented — needs `create_profile_clone`, `reset_profile_to_default` in `profile.rs`, two new Tauri commands, and UI buttons next to the profile selector.
