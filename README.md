# Mouser Tauri

Clean-room Tauri v2 + React/TypeScript rewrite of Mouser, with a native Rust backend, typed frontend bindings, and platform-specific device/app integrations.

This repo is no longer just a UI shell. The checked-in code now includes:

- A Rust runtime service that owns config, device state, profile resolution, routing, health, and background work
- Native platform backends for macOS, Windows, and Linux
- A generated Logitech mouse catalog with 43 known device entries and matching layout assets
- Per-device tuning, per-app profiles, legacy Mouser config import, app discovery, runtime events, and debug tooling

## What The App Does

Mouser Tauri lets you:

- add supported Logitech mice as managed devices
- assign a profile per device or let profiles auto-switch by frontmost app
- remap buttons, thumb-wheel actions, and gesture controls when the active backend supports them
- read live battery/DPI telemetry and write DPI values through native HID backends
- inspect device routing, backend health, active platform adapters, and raw battery telemetry
- import an existing Mouser `config.json` as a migration source

The current frontend is organized around four working areas:

- `Buttons`: profile bindings and layout-aware control editing
- `Point & Scroll`: device-level tuning such as DPI, scroll inversion, layout override, and gesture thresholds
- `Profiles`: profile CRUD plus app matcher editing
- `Debug`: backend capability inspection, battery telemetry, and legacy import

## Honest Status

The codebase is much further along than the old README suggested, but it is still a work in progress.

- The backend surface is real and native, not mocked-only.
- Device and OS validation is still uneven across hardware combinations.
- The `startMinimized` and `startAtLogin` settings are persisted in config and exposed in the UI, but OS launch integration is not wired up yet.
- Debug logging now goes to the Rust console only. Run the app from a terminal when you need backend logs.
- Some Logitech preset actions are imported into the UI as unsupported placeholders and are not executed yet.

## Platform Status

| Platform | Backend status in tree | Notes |
| --- | --- | --- |
| macOS | Native HID, event tap, app focus, app discovery | Requires Accessibility permission for live interception. Uses `IOKit` + `hidapi` for device work and `NSWorkspace` for app focus. |
| Windows | Native HID, low-level hook, app focus, app discovery | Uses Win32 hooks/focus APIs plus `hidapi`. App discovery pulls from shortcuts, registry, packages, and running processes. |
| Linux | Native HID, `evdev` hook, X11 focus, desktop discovery | Requires low-level device access (`/dev/input`, `/dev/uinput`, `hidraw`). Frontmost-app detection is currently X11-only. |

Linux still has the most environment-specific setup, so it has a dedicated guide: [`docs/linux-support.md`](docs/linux-support.md).

## Getting Started

### Prerequisites

Install these first:

- Rust via [rustup](https://rustup.rs)
- Node.js LTS with `npm`
- Bun
- Tauri v2 system prerequisites for your OS: [https://v2.tauri.app/start/prerequisites/](https://v2.tauri.app/start/prerequisites/)

Why both Node and Bun?

- The repo uses Bun for dependency install and most package scripts.
- `src-tauri/tauri.conf.json` currently runs `npm run dev` / `npm run build` as Tauri pre-commands, so a working Node/npm install still needs to be present.

### First Run

```bash
cd /Users/luca/Documents/dev/mouser-tauri
bun install
cargo test --manifest-path src-tauri/Cargo.toml
bun run test:run
bun run tauri dev
```

## Common Commands

```bash
# Frontend dev server only
bun run dev

# Full desktop app
bun run tauri dev

# Frontend production build
bun run build

# Frontend tests
bun run test:run

# Rust tests
cargo test --manifest-path src-tauri/Cargo.toml

# Refresh generated TS bindings after changing Rust commands/types/events
bun run generate:bindings
```

Bindings note:

- `bun run tauri dev` regenerates `src/lib/bindings.ts` automatically in debug builds.
- Plain frontend commands such as `bun run dev`, `bun run build`, and `bun run test:run` use the checked-in bindings file.

## Config, Data, And Import

Default config path by platform:

- macOS: `~/Library/Application Support/Mouser Tauri/config.json`
- Linux: `$XDG_CONFIG_HOME/Mouser Tauri/config.json` or `~/.config/Mouser Tauri/config.json`
- Windows: `%APPDATA%\\Mouser Tauri\\config.json`

Important behavior:

- Config saves are written through a temp file and then renamed into place.
- If Mouser cannot decode the existing config, it preserves the unreadable file as a timestamped recovery file and loads defaults.
- Legacy Mouser JSON can be imported from a file path or pasted raw into the Debug view.
- Legacy settings are translated into the new typed schema where possible, and skipped keys produce warnings instead of silently disappearing.

Full details live in [`docs/config-and-import.md`](docs/config-and-import.md).

## Repository Layout

- `src/`: React app, sections, hooks, UI primitives, API wrapper, and generated bindings
- `src-tauri/src/`: Tauri entrypoint, command surface, runtime orchestration, config store
- `src-tauri/crates/mouser-core/`: shared types, defaults, catalog, layout data, snapshot builders
- `src-tauri/crates/mouser-import/`: legacy Mouser importer
- `src-tauri/crates/mouser-platform/`: macOS/Windows/Linux backends
- `docs/`: focused project docs and ADRs

## Backend Architecture

The backend is structured around a single runtime owner instead of scattering state across commands.

- `RuntimeService` runs the request/notification loop and background workers.
- `AppRuntime` owns mutable state: config, detected devices, app discovery snapshot, resolved profile, device routing, debug log, and backend health.
- Platform adapters implement `HidBackend`, `HookBackend`, `AppFocusBackend`, and `AppDiscoveryBackend`.
- The frontend calls typed Tauri commands and refreshes its bootstrap query when runtime events arrive.

See [`docs/runtime-architecture.md`](docs/runtime-architecture.md) for the detailed flow, command list, and emitted events.

## Additional Docs

- [`docs/runtime-architecture.md`](docs/runtime-architecture.md)
- [`docs/config-and-import.md`](docs/config-and-import.md)
- [`docs/linux-support.md`](docs/linux-support.md)
- [`docs/adr/0001-clean-room-backend-foundation.md`](docs/adr/0001-clean-room-backend-foundation.md)

## Unsupported Logitech Preset Actions

The generated Logitech catalog currently contributes 13 action identifiers that Mouser Tauri surfaces in the UI as unsupported options but does not execute yet:

- `card_global_presets_osx_horizontal_scroll`
- `card_global_presets_keyboard_shortcut`
- `card_global_presets_middle_button`
- `card_global_presets_mode_shift`
- `card_global_presets_one_of_gesture_button`
- `card_global_presets_scroll_left`
- `card_global_presets_scroll_right`
- `card_global_presets_show_radial_menu`
- `card_global_presets_osx_smart_zoom`
- `card_global_presets_spotlight_effects`
- `card_global_presets_osx_swipe_pages`
- `card_global_presets_osx_volume_control`
- `card_global_presets_osx_zoom`
