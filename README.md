# Mouser Tauri

Clean-room Tauri v2 + React/TypeScript rewrite of Mouser.

This repo now includes a first Linux-native backend pass alongside the typed config/import/runtime work. It includes:

- A separate Tauri desktop app repo with a standard React/TypeScript frontend
- An internal Rust workspace under [`src-tauri`](/Users/luca/Documents/dev/mouser-tauri/src-tauri)
- A new Rust-native config schema and typed domain model
- A legacy Mouser JSON importer
- Mock devices, mock app focus, and mock engine status
- A live Linux backend for Logitech HID++ enumeration, DPI changes, battery/DPI reads, input remapping, gestures, and app discovery
- A React shell for `Devices`, `Buttons`, `Profiles`, `Settings`, and `Debug`
- Tauri commands and runtime events for the desktop runtime, with live Linux backends and mock fallbacks

## Workspace

- [`crates/mouser-core`](/Users/luca/Documents/dev/mouser-tauri/src-tauri/crates/mouser-core): domain types, config schema, layouts, actions, and engine snapshot models
- [`crates/mouser-import`](/Users/luca/Documents/dev/mouser-tauri/src-tauri/crates/mouser-import): importer from the current Python Mouser `config.json`
- [`crates/mouser-mock`](/Users/luca/Documents/dev/mouser-tauri/src-tauri/crates/mouser-mock): mock device catalog and mock runtime
- [`crates/mouser-platform`](/Users/luca/Documents/dev/mouser-tauri/src-tauri/crates/mouser-platform): platform backends, including the Linux HID/hook/app-focus/app-discovery implementation

## Commands

The Tauri backend exposes config, profile, device, app-discovery, icon-loading, and import commands to the frontend.

It also emits:

- `device_changed`
- `app_discovery_changed` when the discovery snapshot actually changes
- `profile_changed`
- `engine_status_changed`
- `debug_event` for incremental runtime log entries

## First-Time Setup

If this is your first time running the repo, install these first:

- Rust via `rustup`: [https://rustup.rs](https://rustup.rs)
- Bun: [https://bun.sh/docs/installation](https://bun.sh/docs/installation)
- Node.js LTS: [https://nodejs.org](https://nodejs.org)
- Tauri system prerequisites for your OS: [https://v2.tauri.app/start/prerequisites/](https://v2.tauri.app/start/prerequisites/)

Quick setup:

```bash
cd /Users/luca/Documents/dev/mouser-tauri
bun install
cargo test --manifest-path src-tauri/Cargo.toml
bun run test:run
bun run tauri dev
```

Notes:

- Bun is the default package manager in this repo because [`bun.lock`](/Users/luca/Documents/dev/mouser-tauri/bun.lock) is checked in, but the package scripts are standard and also work through `npm`.
- `rustup` installs both `rustc` and `cargo`.
- Tauri needs extra native dependencies that vary by OS, so follow the official prerequisites page before running the desktop app.
- For Linux-specific runtime permissions, build packages, and current Wayland/X11 limitations, see [docs/linux-support.md](/Users/luca/Documents/dev/mouser-tauri/docs/linux-support.md).

## Development

```bash
cd /Users/luca/Documents/dev/mouser-tauri
bun install
bun run test:run
bun run build
cargo test --manifest-path src-tauri/Cargo.toml
```

For the desktop app:

```bash
cd /Users/luca/Documents/dev/mouser-tauri
bun run tauri dev
```

Bindings workflow:

- `bun run tauri dev` regenerates [bindings.ts](/Users/luca/Documents/dev/mouser-tauri/src/lib/bindings.ts) automatically in debug builds.
- Plain frontend commands such as `bun run dev`, `bun run build`, and `bun run test:run` use the checked-in bindings file.
- If you change Rust commands, events, or Specta types without running `tauri dev`, refresh the file manually with `bun run generate:bindings`.

## Current scope

This repo now ships a Linux-first live backend with:

- Logitech HID++ enumeration, DPI writes, and telemetry reads
- `evdev`-based global remapping for middle/back/forward/horizontal-scroll controls
- HID++ gesture diversion with virtual keyboard/mouse injection
- X11 frontmost-app detection and `.desktop` plus running-process app discovery
- XDG config storage and native icon loading for file-backed app icons

Still incomplete:

- Wayland frontmost-app detection and profile auto-switching
- runtime validation across Linux distros and desktop environments
- equivalent hardening and parity work on the macOS and Windows backends

## Logitech Actions Not Yet Supported

These actions are imported from Logitech device data and now surface in the UI as disabled `Unsupported` options, but Mouser does not execute them yet:

- Horizontal Scroll: `card_global_presets_osx_horizontal_scroll`
- Keyboard Shortcut: `card_global_presets_keyboard_shortcut`
- Middle Button: `card_global_presets_middle_button`
- Mode Shift: `card_global_presets_mode_shift`
- One Of Gesture Button: `card_global_presets_one_of_gesture_button`
- Scroll Left: `card_global_presets_scroll_left`
- Scroll Right: `card_global_presets_scroll_right`
- Show Radial Menu: `card_global_presets_show_radial_menu`
- Smart Zoom: `card_global_presets_osx_smart_zoom`
- Spotlight Effects: `card_global_presets_spotlight_effects`
- Swipe Pages: `card_global_presets_osx_swipe_pages`
- Volume Control: `card_global_presets_osx_volume_control`
- Zoom: `card_global_presets_osx_zoom`
