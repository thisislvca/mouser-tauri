# Runtime Architecture

This document explains how the current backend is organized, what owns state, and how the frontend talks to it.

## High-Level Structure

The repo is split into a React/Tauri shell and an internal Rust workspace:

- `src/`
  - React UI
  - TanStack Query bootstrap/load model
  - typed command/event wrapper in `src/lib/api.ts`
  - generated Specta bindings in `src/lib/bindings.ts`
- `src-tauri/src/`
  - Tauri setup and tray wiring
  - command definitions
  - runtime service and config store
- `src-tauri/crates/mouser-core/`
  - shared types and snapshots
  - default actions and settings
  - generated Logitech catalog and layouts
- `src-tauri/crates/mouser-import/`
  - legacy Mouser config importer
- `src-tauri/crates/mouser-platform/`
  - platform backends for HID, hooks, app focus, and app discovery

## Design Principle

Mutable backend state is centralized in one place.

- `AppRuntime` owns config, detected devices, app discovery, selected device, resolved profile, runtime health, and debug log.
- `RuntimeService` is the concurrency boundary around that runtime.
- Tauri commands do not mutate global state directly. They send requests into the runtime service and get typed responses back.

This keeps state transitions predictable and makes it possible to emit coherent bootstrap snapshots after each mutation.

## Startup Flow

At app startup:

1. `run()` creates `RuntimeService`.
2. The Tauri app is built and Specta commands/events are mounted.
3. A tray menu is created from the current runtime state.
4. The runtime listener is attached so background updates can emit Tauri events.
5. Background workers start.
6. OS-specific monitors start where available.

In debug builds, bindings are exported before the app launches so the frontend and backend stay in sync.

## Runtime Service Model

`RuntimeService` uses a message-driven design:

- request channel for command-style operations
- notification channel for background signals
- background update channel for pushing state changes back to Tauri
- I/O worker for device polling and app discovery

Important background jobs:

- startup sync
- periodic hook event draining
- periodic safety resync
- app discovery refresh
- Windows device polling
- Linux background polling
- macOS/Windows focus monitors

The result is a backend that can react to device changes, app-focus changes, hook events, and explicit frontend mutations without giving up single-owner state.

## Bootstrap Payload

The frontend is driven primarily by `BootstrapPayload`, which includes:

- `config`
- `availableActions`
- `knownApps`
- `appDiscovery`
- `supportedDevices`
- `layouts`
- `engineSnapshot`
- `platformCapabilities`
- `manualLayoutChoices`

The frontend fetches this once through `bootstrap_load` and invalidates the query when key runtime events arrive.

## Device Model

The backend distinguishes between:

- `detected_devices`: live devices currently seen by the HID backend
- `managed_devices`: persisted user-owned records in config

That distinction matters because a device can remain configured even while disconnected.

`mouser-core` provides:

- generated known-device specs
- support metadata
- layout assets and hotspots
- default control bindings
- helpers for building managed/live snapshots

## Routing And Profile Resolution

Routing is not just “pick a selected device”.

The runtime resolves devices in two passes:

1. identity match
   - uses a normalized device fingerprint identity key when available
2. model fallback
   - falls back to model matching if no stable identity is available

Each live device produces a routing entry describing:

- matched managed device
- match kind
- resolved profile
- attribution status
- whether the route is authoritative enough for hook delivery

This is emitted as `device_routing_changed` when routing actually changes.

Profile resolution works like this:

- a device can have an explicitly assigned profile
- otherwise the runtime tries to match the frontmost app against profile matchers
- if nothing matches, it falls back to the `default` profile

Bindings are filtered to the controls actually supported by the active live device before hook routes are built.

## Config Persistence And Recovery

Persistence is handled by `JsonConfigStore`.

Notable behavior:

- config is saved atomically through a temp file + rename flow
- parent directories are created as needed
- unreadable configs are preserved as timestamped recovery files
- config JSON is migrated on load before deserialization
- invariants are enforced so the default profile and normalized bindings always exist

The persisted schema includes:

- app-wide settings
- global device defaults
- profiles and app matchers
- managed devices with per-device settings, nickname, identity, and assigned profile

## Legacy Import

The importer is intentionally a migration bridge, not a dependency on the legacy backend.

It accepts either:

- `source_path`
- `raw_json`

It translates legacy data into the new config model, including:

- active profile
- basic app settings
- device defaults
- profile labels
- executable-based app matchers
- control/action mappings where equivalents exist

Unsupported controls or settings generate warnings and are returned in `LegacyImportReport`.

## Tauri Command Surface

The backend currently exposes these commands:

- `bootstrap_load`
- `config_get`
- `config_save`
- `app_settings_update`
- `device_defaults_update`
- `app_discovery_refresh`
- `app_icon_load`
- `profiles_create`
- `profiles_update`
- `profiles_delete`
- `devices_list`
- `devices_add`
- `devices_update_settings`
- `devices_update_profile`
- `devices_update_nickname`
- `devices_reset_to_factory`
- `devices_remove`
- `devices_select`
- `devices_select_mock`
- `import_legacy_config`
- `debug_clear_log`

All commands are exported through Specta, which is why the frontend can use a generated typed API instead of hand-written stringly-typed calls.

## Runtime Events

The backend emits these Tauri events:

- `device_changed`
- `device_routing_changed`
- `profile_changed`
- `engine_status_changed`
- `app_discovery_changed`
- `debug_event`

The current frontend listens to the state-changing events and invalidates the bootstrap query so the UI can re-render from fresh backend state.

## Platform Backends

The runtime selects one backend set at startup:

- HID backend
- hook backend
- app-focus backend
- app-discovery backend

Current backend IDs exposed in the app are intentionally descriptive, for example:

- macOS: `macos-iokit+hidapi`, `macos-eventtap`, `macos-nsworkspace`, `macos-applications`
- Windows: `windows-hidapi`, `windows-hook`, `windows-foreground`, `windows-hybrid`
- Linux: `linux-hidapi`, `linux-evdev`, `linux-x11`, `linux-desktop`

These IDs are shown in the Debug view and are useful when diagnosing missing permissions or unavailable runtime integrations.

## Health And Debugging

`RuntimeHealth` tracks five backend slots:

- persistence
- hid
- hook
- focus
- discovery

Each slot records:

- state: `ready`, `stale`, or `error`
- message
- last update timestamp

Debug logging is grouped so high-volume traces can be enabled selectively:

- runtime
- hook routing
- gestures
- thumb wheel
- hid

The frontend no longer streams high-volume backend logs into the UI. When debug mode is enabled, logs are emitted to the Rust console.
