# Linux Support

Linux support is native, but it depends on low-level device access and currently uses X11 for frontmost-app detection.

The Linux backend lives in `src-tauri/crates/mouser-platform/src/linux_backend.rs`.

## Backend Pieces

The current Linux stack includes:

- `linux-hidapi`
  - Logitech HID++ device enumeration
  - battery telemetry reads
  - current DPI reads
  - DPI writes
- `linux-evdev`
  - global mouse interception through `evdev`
  - middle/back/forward/thumb-wheel remapping
  - virtual keyboard and mouse injection through `/dev/uinput`
- `linux-hidapi-gesture`
  - HID++ gesture diversion when the device/channel supports it
- `linux-x11`
  - frontmost-app detection on X11
- `linux-desktop`
  - app discovery from `.desktop` files and running processes

## What Works Today

- Logitech device enumeration through `hidapi`
- battery and current-DPI telemetry
- DPI writes
- button remapping for supported controls
- horizontal scroll remapping
- gesture-triggered remapping when the gesture channel is available
- app discovery from desktop entries plus running processes
- config storage in XDG config locations

## Build Requirements

Install the normal Tauri prerequisites first:

- [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/)

You also need the Linux device-stack headers and tools:

- `pkg-config`
- `libudev` development headers

Common package names:

- Debian/Ubuntu: `pkg-config` and `libudev-dev`
- Fedora: `pkgconf-pkg-config` and `systemd-devel`
- Arch: `pkgconf` and `systemd`

## Runtime Permissions

The live backend needs access to:

- `/dev/input/event*` for interception
- `/dev/uinput` for virtual input injection
- Logitech `hidraw` interfaces for HID++ telemetry and gesture diversion

In practice that usually means udev rules, group membership, or both.

If remapping does not start, open the app’s Debug view and inspect:

- active HID backend
- active hook backend
- active focus backend

Those backend IDs are the fastest way to see which layer is missing permissions.

## Expected Backend IDs

Depending on what is available, you should see combinations like:

- hook backend
  - `linux-evdev+hidapi-gesture`
  - `linux-evdev`
  - `linux-hidapi-gesture`
  - `linux-evdev-unavailable`
- HID backend
  - `linux-hidapi`
- focus backend
  - `linux-x11`
  - `linux-x11-unavailable`
- discovery backend
  - `linux-desktop`

## X11 And Wayland

Frontmost-app detection is currently X11-only.

- On X11, app-based profile switching should work.
- On Wayland, the runtime currently reports no frontmost app, so auto-switching does not engage.
- Actual remapping is still handled through `evdev` and `/dev/uinput`, so button remaps do not depend on X11.

## Config Path

Linux config is stored at:

- `$XDG_CONFIG_HOME/Mouser Tauri/config.json`
- or `~/.config/Mouser Tauri/config.json`

If the file becomes unreadable, Mouser preserves it as a timestamped recovery file and loads defaults.

## Troubleshooting

If the hook backend shows `linux-evdev-unavailable`:

- confirm the process can read the relevant `/dev/input/event*` nodes
- confirm `/dev/uinput` exists and is writable
- confirm the target mouse is visible through `evdev`

If the HID backend cannot read telemetry or write DPI:

- confirm the Logitech device exposes `hidraw` interfaces
- confirm the process can open them

If the focus backend shows `linux-x11-unavailable`:

- confirm you are running under X11
- confirm `DISPLAY` is set for the process

If gesture remapping does not appear:

- confirm the device family actually exposes a supported Logitech gesture channel
- check whether the hook backend downgraded from `linux-evdev+hidapi-gesture` to `linux-evdev`

## Validation Notes

The Linux backend is implemented and integrated into the runtime service, but it still needs broader real-hardware validation across:

- X11 sessions
- Wayland sessions
- USB devices
- Bluetooth Low Energy devices
- different desktop environments and distro permission setups
