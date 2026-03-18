use std::{collections::BTreeMap, fs, path::Path};

use mouser_core::{
    default_config, normalize_bindings, AppMatcher, AppMatcherKind, AppearanceMode, Binding,
    DeviceSettings, LegacyImportReport, LogicalControl, Profile, Settings,
};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct ImportSource {
    pub source_path: Option<String>,
    pub raw_json: Option<String>,
}

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("import requires either raw_json or source_path")]
    MissingSource,
    #[error("failed to read legacy config from {path}: {message}")]
    Io { path: String, message: String },
    #[error("legacy config is not valid JSON: {0}")]
    InvalidJson(String),
}

pub fn import_legacy_config(source: ImportSource) -> Result<LegacyImportReport, ImportError> {
    let raw_json = if let Some(raw_json) = source.raw_json {
        raw_json
    } else if let Some(path) = source.source_path.clone() {
        fs::read_to_string(Path::new(&path)).map_err(|error| ImportError::Io {
            path,
            message: error.to_string(),
        })?
    } else {
        return Err(ImportError::MissingSource);
    };

    let value: Value = serde_json::from_str(&raw_json)
        .map_err(|error| ImportError::InvalidJson(error.to_string()))?;
    Ok(import_legacy_value(value, source.source_path))
}

pub fn import_legacy_value(value: Value, source_path: Option<String>) -> LegacyImportReport {
    let mut warnings = Vec::new();
    let mut config = default_config();

    if let Some(active_profile) = value.get("active_profile").and_then(Value::as_str) {
        config.active_profile_id = active_profile.to_string();
    }

    if let Some(settings) = value.get("settings").and_then(Value::as_object) {
        let (app_settings, device_defaults) = import_settings(settings, &mut warnings);
        config.settings = app_settings;
        config.device_defaults = device_defaults;
    }

    config.profiles.clear();
    if let Some(profiles) = value.get("profiles").and_then(Value::as_object) {
        for (profile_id, raw_profile) in profiles {
            config
                .profiles
                .push(import_profile(profile_id, raw_profile, &mut warnings));
        }
    }

    config.ensure_invariants();

    LegacyImportReport {
        imported_profiles: config.profiles.len(),
        warnings,
        source_path,
        config,
    }
}

fn import_settings(
    settings: &serde_json::Map<String, Value>,
    warnings: &mut Vec<String>,
) -> (Settings, DeviceSettings) {
    let defaults = default_config();
    let mut imported = defaults.settings;
    let mut device_defaults = defaults.device_defaults;

    for (key, value) in settings {
        match key.as_str() {
            "start_minimized" => imported.start_minimized = value.as_bool().unwrap_or(true),
            "start_with_windows" => imported.start_at_login = value.as_bool().unwrap_or(false),
            "invert_hscroll" => {
                device_defaults.invert_horizontal_scroll = value.as_bool().unwrap_or(false)
            }
            "invert_vscroll" => {
                device_defaults.invert_vertical_scroll = value.as_bool().unwrap_or(false)
            }
            "dpi" => device_defaults.dpi = value.as_u64().unwrap_or(1000) as u16,
            "gesture_threshold" => {
                device_defaults.gesture_threshold = value.as_u64().unwrap_or(50) as u16
            }
            "gesture_deadzone" => {
                device_defaults.gesture_deadzone = value.as_u64().unwrap_or(40) as u16
            }
            "gesture_timeout_ms" => {
                device_defaults.gesture_timeout_ms = value.as_u64().unwrap_or(3000) as u32
            }
            "gesture_cooldown_ms" => {
                device_defaults.gesture_cooldown_ms = value.as_u64().unwrap_or(500) as u32
            }
            "appearance_mode" => {
                imported.appearance_mode = match value.as_str() {
                    Some("light") => AppearanceMode::Light,
                    Some("dark") => AppearanceMode::Dark,
                    _ => AppearanceMode::System,
                }
            }
            "debug_mode" => imported.debug_mode = value.as_bool().unwrap_or(false),
            "device_layout_overrides" => {
                let overrides = value
                    .as_object()
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|(device, layout)| {
                                layout
                                    .as_str()
                                    .map(|layout| (device.clone(), layout.to_string()))
                            })
                            .collect::<BTreeMap<_, _>>()
                    })
                    .unwrap_or_default();
                if overrides.len() == 1 {
                    device_defaults.manual_layout_override =
                        overrides.values().next().cloned();
                } else if !overrides.is_empty() {
                    warnings.push(
                        "Imported multiple legacy layout overrides; add the device first, then re-apply any per-device layout overrides in Mouser Tauri.".to_string(),
                    );
                }
            }
            unsupported => warnings.push(format!(
                "Ignored unsupported legacy setting key `{unsupported}`"
            )),
        }
    }

    (imported, device_defaults)
}

fn import_profile(profile_id: &str, raw_profile: &Value, warnings: &mut Vec<String>) -> Profile {
    let label = raw_profile
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or(profile_id)
        .to_string();
    let app_matchers = raw_profile
        .get("apps")
        .and_then(Value::as_array)
        .map(|apps| {
            apps.iter()
                .filter_map(Value::as_str)
                .map(|executable| AppMatcher {
                    kind: AppMatcherKind::Executable,
                    value: executable.to_string(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut bindings = Vec::new();
    if let Some(mappings) = raw_profile.get("mappings").and_then(Value::as_object) {
        for (legacy_key, action_value) in mappings {
            let Some(action_id) = action_value.as_str() else {
                warnings.push(format!(
                    "Ignored non-string mapping for `{legacy_key}` in profile `{profile_id}`"
                ));
                continue;
            };
            if let Some(control) = map_legacy_control(legacy_key) {
                bindings.push(Binding {
                    control,
                    action_id: map_legacy_action(action_id).to_string(),
                });
            } else {
                warnings.push(format!(
                    "Ignored unsupported legacy control `{legacy_key}` in profile `{profile_id}`"
                ));
            }
        }
    }

    Profile {
        id: profile_id.to_string(),
        label,
        app_matchers,
        bindings: normalize_bindings(bindings),
    }
}

fn map_legacy_control(legacy_key: &str) -> Option<LogicalControl> {
    match legacy_key {
        "middle" => Some(LogicalControl::Middle),
        "gesture" => Some(LogicalControl::GesturePress),
        "gesture_left" => Some(LogicalControl::GestureLeft),
        "gesture_right" => Some(LogicalControl::GestureRight),
        "gesture_up" => Some(LogicalControl::GestureUp),
        "gesture_down" => Some(LogicalControl::GestureDown),
        "xbutton1" => Some(LogicalControl::Back),
        "xbutton2" => Some(LogicalControl::Forward),
        "hscroll_left" => Some(LogicalControl::HscrollLeft),
        "hscroll_right" => Some(LogicalControl::HscrollRight),
        _ => None,
    }
}

fn map_legacy_action(action_id: &str) -> &str {
    match action_id {
        "win_d" => "show_desktop",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_current_default_shape() {
        let report = import_legacy_value(
            serde_json::json!({
                "version": 4,
                "active_profile": "default",
                "profiles": {
                    "default": {
                        "label": "Default (All Apps)",
                        "apps": [],
                        "mappings": {
                            "middle": "none",
                            "gesture": "none",
                            "gesture_left": "none",
                            "gesture_right": "none",
                            "gesture_up": "none",
                            "gesture_down": "none",
                            "xbutton1": "alt_tab",
                            "xbutton2": "alt_tab",
                            "hscroll_left": "browser_back",
                            "hscroll_right": "browser_forward"
                        }
                    }
                },
                "settings": {
                    "dpi": 1000
                }
            }),
            None,
        );

        assert_eq!(report.config.active_profile_id, "default");
        assert_eq!(report.config.profiles.len(), 1);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn preserves_profile_matchers_and_maps_old_actions() {
        let report = import_legacy_value(
            serde_json::json!({
                "profiles": {
                    "code": {
                        "label": "VS Code",
                        "apps": ["Code.exe"],
                        "mappings": {
                            "gesture_up": "win_d",
                            "xbutton1": "browser_back"
                        }
                    }
                }
            }),
            None,
        );

        let profile = report.config.profile_by_id("code").unwrap();
        assert_eq!(profile.app_matchers[0].value, "Code.exe");
        assert_eq!(
            profile
                .binding_for(LogicalControl::GestureUp)
                .unwrap()
                .action_id,
            "show_desktop"
        );
    }

    #[test]
    fn warns_on_unknown_partial_fields_without_crashing() {
        let report = import_legacy_value(
            serde_json::json!({
                "profiles": {
                    "strange": {
                        "mappings": {
                            "mystery_button": "copy"
                        }
                    }
                },
                "settings": {
                    "custom_future_flag": true
                }
            }),
            None,
        );

        assert!(!report.warnings.is_empty());
        assert!(report.config.profile_by_id("strange").is_some());
    }
}
