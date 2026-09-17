# GIF Replacer Tool

> **Full documentation:** [`docs/user-guide.md`](../docs/user-guide.md) for using the tool,
> and [`docs/developer-manual.md`](../docs/developer-manual.md) for its internals.
> Both live in the parent project folder and are the authoritative source.

A desktop app that lets a non-technical user drop in an LVGL-converted `.c` GIF file, pick an emotion slot, and have it renamed, wired into the active EmotionDisplay profile, built, and flashed to the ESP32 board — no terminal required.

## Tech stack

- **Backend**: Rust + Tauri 1.5 (`src-tauri/`)
- **Frontend**: Vanilla HTML/CSS/JS, no framework and no build step (`src/`)
- Config persisted at `~/.gif-tool-config.json`

## Quick start

```bash
cd gif-replacer-tool
npm install
npm run dev
```

First launch: paste or browse to your EmotionDisplay project root — the folder containing `sdkconfig.defaults` and `main/`. Everything else loads automatically.

**Prerequisite:** ESP-IDF **v5.5.2** must be installed at `~/esp/esp-idf-v5.5.2`. The app does not install it. To set it up:

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Axonex-Intelligence-Limited/gif-replacer-tool/main/scripts/setup-macos.sh)"
```

## Source layout

```
src-tauri/src/
  profile.rs   — profile detection, list/switch profiles, get/set default emotion, clone, reset
  parser.rs    — validates + renames LVGL .c symbols
  builder.rs   — writes gif file, runs idf.py build/flash, captures output
  config.rs    — load/save ~/.gif-tool-config.json
  main.rs      — Tauri commands + event emission (build-output stream)
src/
  index.html, app.js, style.css  — single-page UI, no build step
```

## Tests

23 unit tests across `profile`, `parser`, `builder` and `config`.

```bash
cd src-tauri && cargo test
```

Note: `test_profile_detection` hardcodes an absolute path to the original developer's checkout and will fail elsewhere until that path is changed.

## Known limitations

- `tauri.conf.json` has `bundle.active` set to `false`, so `npm run build` produces a release binary but **not** a `.app` bundle. Set `bundle.active` to `true` and supply icons to package a distributable.
- The `idf_path` key in the config file is not read by anything; ESP-IDF is resolved from `~/esp/esp-idf-v5.5.2` or `$IDF_PATH`.

See the developer manual for the full list.

## Repository

https://github.com/Axonex-Intelligence-Limited/gif-replacer-tool (private)
