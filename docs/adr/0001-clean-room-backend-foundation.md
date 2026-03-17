# ADR 0001: Clean-room backend foundation

## Status

Accepted.

## Decision

The Mouser Tauri rewrite will not ship `logiops-core`, `hidpp-transport`, Solaar-derived code, or other GPL-oriented Logitech backends in milestone 1.

Instead, the new repo uses:

- a clean-room Rust domain model in `mouser-core`
- platform port traits in `mouser-platform`
- a mock runtime in `mouser-mock`
- an importer that treats the current Python Mouser config as a migration source, not as executable logic

## Rationale

- `logiops-core` and `hidpp-transport` are GPL and Linux-oriented. They are useful references but not the right foundation for a Windows + macOS-first Tauri rewrite.
- Solaar is mature and valuable as a behavioral reference, but it is also not the right shipping dependency for this architecture.
- `hidpp` remains interesting as a future protocol reference or optional adapter experiment.
- `hidapi` remains a viable future transport dependency once live-device work starts.

## Consequences

- Milestone 1 optimizes for typed contracts, UI iteration, config migration, and app structure instead of premature low-level device integration.
- Replacing the mock runtime with live Windows and macOS backends later should not require a schema or UI rewrite.
