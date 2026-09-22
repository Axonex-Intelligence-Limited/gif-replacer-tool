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

**Prerequisite:** ESP-IDF **v5.5.2**. The app does not install it. One script per platform does.

macOS — installs to `~/esp/esp-idf-v5.5.2`:

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Axonex-Intelligence-Limited/gif-replacer-tool/main/scripts/setup-macos.sh)"
```

Windows — downloads Espressif's offline 5.5.2 installer, runs it silently, and points the app at
the result. Needs no administrator rights:

```powershell
Invoke-WebRequest -Uri https://raw.githubusercontent.com/Axonex-Intelligence-Limited/gif-replacer-tool/main/scripts/setup-windows.ps1 -OutFile setup-windows.ps1
Unblock-File .\setup-windows.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\setup-windows.ps1
```

The Windows script has not been run on a Windows machine yet. See
`scripts/setup-windows-laptop-checklist.md` for what still needs verifying.

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

59 Rust unit tests — `builder` 25, `profile` 14, `gifconv` 12, `parser` 6, `config` 2 — plus 39
Pester tests for the Windows setup script. The suite is machine-independent; it uses synthetic
fixtures in the temp directory rather than any hardcoded checkout.

```bash
cd src-tauri && cargo test
pwsh -NoProfile -Command 'Invoke-Pester -Path ./scripts/tests'
```

The Pester tests skip the parts that need Windows. CI runs both suites on `windows-latest`.

## Known limitations

- `tauri.conf.json` bundles **NSIS only**. On Windows, `npm run tauri build` produces both the bare
  `target/release/GIF Replacer Tool.exe` and an installer under `target/release/bundle/nsis/`. A
  macOS `.app` bundle is **not** configured.
- `scripts/setup-windows.ps1` has never been executed on Windows. Its pure logic is unit-tested;
  the platform interaction is not. See `scripts/setup-windows-laptop-checklist.md`.

See the developer manual for the full list.

## Repository

https://github.com/Axonex-Intelligence-Limited/gif-replacer-tool (public)
