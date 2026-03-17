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

## Development

```bash
cd /Users/luca/Documents/dev/mouser-tauri
npm install
npm run test:run
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
```

For the desktop app:

```bash
cd /Users/luca/Documents/dev/mouser-tauri
npm run tauri dev
```

## Current scope

This milestone deliberately does not ship:

- live HID++ communication
- live global input hooks
- real frontmost-app integration

Those remain behind clean interfaces so the next milestone can replace the mock runtime incrementally.
