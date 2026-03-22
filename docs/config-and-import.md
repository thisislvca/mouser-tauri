# Config And Import

This document covers where Mouser Tauri stores state, how config recovery works, and what the legacy importer translates into the new schema.

## Config File Location

`JsonConfigStore::default_path()` resolves to:

- macOS: `~/Library/Application Support/Mouser Tauri/config.json`
- Linux: `$XDG_CONFIG_HOME/Mouser Tauri/config.json`
- Linux fallback: `~/.config/Mouser Tauri/config.json`
- Windows: `%APPDATA%\\Mouser Tauri\\config.json`

## What Lives In Config

The persisted `AppConfig` contains:

- `version`
- `activeProfileId`
- `profiles`
- `managedDevices`
- `settings`
- `deviceDefaults`

### `settings`

Current app-wide settings include:

- `startMinimized`
- `startAtLogin`
- `appearanceMode`
- `debugMode`
- `debugLogGroups`

Important caveat:

- `startMinimized` and `startAtLogin` are stored in the schema and editable in the UI, but the OS integration behind them is not implemented yet.

### `deviceDefaults`

Global defaults seed newly added devices before a device gets its own saved settings:

- `dpi`
- `invertHorizontalScroll`
- `invertVerticalScroll`
- `macosThumbWheelSimulateTrackpad`
- `macosThumbWheelTrackpadHoldTimeoutMs`
- `gestureThreshold`
- `gestureDeadzone`
- `gestureTimeoutMs`
- `gestureCooldownMs`
- `manualLayoutOverride`

### `managedDevices`

Each managed device stores user-owned state separately from live detection:

- stable internal id
- model key and display name
- optional nickname
- optional assigned profile id
- optional identity key
- per-device settings
- creation and last-seen metadata

That separation is why devices can remain configured even while disconnected.

## Save Behavior

Config writes are deliberately conservative:

- the parent directory is created if needed
- JSON is written to a temporary file first
- the temp file is flushed with `sync_all`
- the temp file is renamed into place

Windows uses an extra backup step when replacing an existing config file.

## Recovery Behavior

If the config exists but cannot be decoded:

- Mouser preserves the unreadable file using a timestamped recovery name such as `config.json.corrupt-<timestamp>`
- defaults are loaded instead of crashing
- the runtime records a warning so the app can surface that recovery happened

Temporary files also use timestamped suffixes such as `config.json.tmp-<timestamp>`.

## Load-Time Migration

Config JSON is migrated before deserialization.

Current migration behavior includes:

- moving legacy device-default fields out of the old `settings` shape and into `deviceDefaults`
- carrying forward legacy layout override data
- normalizing profile bindings
- ensuring the default profile exists
- upgrading older default bindings to the v4 defaults when appropriate
- normalizing managed-device records and clearing invalid profile references

The runtime then enforces invariants again after load.

## Legacy Import

Legacy import lives in `mouser-import` and is intentionally isolated from the new backend runtime.

It accepts:

- a filesystem path to a legacy config file
- raw JSON pasted into the app

The importer returns `LegacyImportReport`, which includes:

- the translated `AppConfig`
- warning strings
- source path, when one was used
- imported profile count

## What The Importer Translates

### App settings

Supported legacy settings that map into the new schema:

- `start_minimized`
- `start_with_windows`
- `invert_hscroll`
- `invert_vscroll`
- `dpi`
- `gesture_threshold`
- `gesture_deadzone`
- `gesture_timeout_ms`
- `gesture_cooldown_ms`
- `appearance_mode`
- `debug_mode`

### Profiles

The importer reads legacy profiles and translates:

- profile id
- label
- executable-based app list into `AppMatcherKind::Executable`
- supported control mappings

### Action mapping

There is currently one explicit action-id remap:

- legacy `win_d` becomes `show_desktop`

## Known Import Limits

The importer is intentionally strict and noisy.

- Unsupported setting keys are ignored with warnings.
- Unsupported controls are ignored with warnings.
- Non-string mapping values are ignored with warnings.
- Legacy app matching is imported as executable matchers only; richer matcher types are part of the new schema, not the old one.
- If multiple legacy layout overrides are present, Mouser keeps only the simple single-default case. Otherwise it warns and asks you to reapply per-device layout overrides after adding the device.

## Manual Editing Guidance

If you edit `config.json` by hand:

- keep profile ids stable
- do not remove the `default` profile
- expect bindings to be normalized on next load
- expect invalid managed-device profile references to be cleared
- prefer using the app UI when changing per-device settings so routing metadata stays coherent
