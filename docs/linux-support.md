# Linux Support

This repo now includes a first Linux backend pass in [`mouser-platform`](/Users/luca/Documents/dev/mouser-tauri/src-tauri/crates/mouser-platform).

## What Works

- Logitech HID++ device enumeration through `hidapi`
- DPI reads and writes
- Battery and current DPI telemetry reads
- Global mouse interception through `evdev`
- Middle, back, forward, horizontal scroll, and gesture-triggered remapping
- Virtual keyboard and virtual mouse injection through `/dev/uinput`
- X11 frontmost-app detection
- App discovery from `.desktop` files plus running processes
- XDG config storage at `XDG_CONFIG_HOME/Mouser Tauri/config.json` or `~/.config/Mouser Tauri/config.json`

## Linux Build Requirements

On a real Linux machine, install the normal Tauri prerequisites first:

- [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/)

You also need the packages required by the Linux HID/input stack:

- `pkg-config`
- `libudev` development headers

Package names vary by distro:

- Debian/Ubuntu: `pkg-config` and `libudev-dev`
- Fedora: `pkgconf-pkg-config` and `systemd-devel`
- Arch: `pkgconf` and `systemd`

## Runtime Permissions

The live Linux backend needs access to:

- `/dev/input/event*` for mouse interception
- `/dev/uinput` for virtual mouse and keyboard injection
- Logitech HID interfaces exposed through `hidraw` for HID++ telemetry and gesture diversion

In practice that usually means running with the right udev rules or group membership for your distro. If remapping does not start, check the active hook/backend names in the app debug view first.

## X11 And Wayland

The current Linux focus backend is X11-only.

- On X11, frontmost-app detection and profile auto-switching should work.
- On Wayland, the app currently returns no active app, so profile auto-switching will not engage.
- Input remapping still uses `evdev` and `/dev/uinput`, so it is independent of X11 for the actual button remap path.

## Troubleshooting

If the debug panel shows `linux-evdev-unavailable`:

- confirm the process can read `/dev/input/event*`
- confirm `/dev/uinput` exists and is writable
- confirm the target mouse is visible through `evdev`

If the debug panel shows `linux-hidapi` issues:

- confirm the machine exposes Logitech HID interfaces through `hidraw`
- confirm the process can open those interfaces

If the focus backend shows `linux-x11-unavailable`:

- confirm you are running under X11
- confirm `DISPLAY` is set

## Validation Status

The backend is implemented and host-side builds/tests pass from this repo, but it still needs runtime validation on real Linux hardware across at least:

- one X11 desktop session
- one Wayland session
- one USB connection path
- one Bluetooth Low Energy connection path
