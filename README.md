# Mouser Tauri

Clean-room Tauri v2 + React/TypeScript rewrite of Mouser.

This repo is the first milestone shell, not the live HID or hook engine. It already includes:

- A separate Tauri desktop app repo with a standard React/TypeScript frontend
- An internal Rust workspace under [`src-tauri`](/Users/luca/Documents/dev/mouser-tauri/src-tauri)
- A new Rust-native config schema and typed domain model
- A legacy Mouser JSON importer
- Mock devices, mock app focus, and mock engine status
- A React shell for `Devices`, `Buttons`, `Profiles`, `Settings`, and `Debug`
- Tauri commands and runtime events for the mock-backed flow

## Workspace

- [`crates/mouser-core`](/Users/luca/Documents/dev/mouser-tauri/src-tauri/crates/mouser-core): domain types, config schema, layouts, actions, and engine snapshot models
- [`crates/mouser-import`](/Users/luca/Documents/dev/mouser-tauri/src-tauri/crates/mouser-import): importer from the current Python Mouser `config.json`
- [`crates/mouser-mock`](/Users/luca/Documents/dev/mouser-tauri/src-tauri/crates/mouser-mock): mock device catalog and mock runtime
- [`crates/mouser-platform`](/Users/luca/Documents/dev/mouser-tauri/src-tauri/crates/mouser-platform): platform port traits and Windows/macOS stubs

## Commands

The Tauri backend currently exposes:

- `bootstrap_load`
- `config_get`
- `config_save`
- `profiles_create`
- `profiles_update`
- `profiles_delete`
- `devices_list`
- `devices_select_mock`
- `import_legacy_config`

It also emits:

- `device_changed`
- `profile_changed`
- `engine_status_changed`
- `debug_event`

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

- This repo's frontend scripts call `bun`, so Bun needs to be installed even if you also use `npm`.
- `rustup` installs both `rustc` and `cargo`.
- Tauri needs extra native dependencies that vary by OS, so follow the official prerequisites page before running the desktop app.

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

## Current scope

This milestone deliberately does not ship:

- live HID++ communication
- live global input hooks
- real frontmost-app integration

Those remain behind clean interfaces so the next milestone can replace the mock runtime incrementally.
