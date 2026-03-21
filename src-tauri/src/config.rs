use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use mouser_core::{
    default_config, default_device_settings, default_profile_bindings,
    legacy_default_profile_bindings_v3, normalize_bindings, AppConfig, Binding,
};
use mouser_platform::{ConfigStore, PlatformError};

pub struct JsonConfigStore {
    path: PathBuf,
}

type JsonMap = serde_json::Map<String, serde_json::Value>;

impl JsonConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> PathBuf {
        let base = if cfg!(target_os = "macos") {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Library")
                .join("Application Support")
        } else if cfg!(target_os = "linux") {
            linux_config_base_dir()
        } else if cfg!(target_os = "windows") {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        };

        base.join("Mouser Tauri").join("config.json")
    }

    pub fn load_or_recover(&self) -> (AppConfig, Option<String>) {
        if !self.path.exists() {
            return (default_config(), None);
        }

        match fs::read_to_string(&self.path) {
            Ok(raw) => match deserialize_app_config(&raw) {
                Ok(config) => (config, None),
                Err(error) => {
                    let warning = self
                        .preserve_unreadable_config()
                        .map(|backup_path| {
                            format!(
                                "Failed to decode config at {}: {error}. Preserved unreadable file at {} and loaded defaults.",
                                self.path.display(),
                                backup_path.display()
                            )
                        })
                        .unwrap_or_else(|rename_error| {
                            format!(
                                "Failed to decode config at {}: {error}. Could not preserve the unreadable file: {rename_error}. Loaded defaults.",
                                self.path.display()
                            )
                        });
                    (default_config(), Some(warning))
                }
            },
            Err(error) => (
                default_config(),
                Some(format!(
                    "Failed to read config at {}: {error}. Loaded defaults.",
                    self.path.display()
                )),
            ),
        }
    }

    fn ensure_parent(&self) -> Result<(), PlatformError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| PlatformError::Io {
                path: parent.display().to_string(),
                message: error.to_string(),
            })?;
        }
        Ok(())
    }

    fn preserve_unreadable_config(&self) -> Result<PathBuf, PlatformError> {
        let backup_path = self.recovery_path("corrupt");
        fs::rename(&self.path, &backup_path).map_err(|error| PlatformError::Io {
            path: self.path.display().to_string(),
            message: error.to_string(),
        })?;
        Ok(backup_path)
    }

    fn temporary_write_path(&self) -> PathBuf {
        self.recovery_path("tmp")
    }

    fn recovery_path(&self, suffix: &str) -> PathBuf {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let mut path = self.path.clone();
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config.json");
        path.set_file_name(format!("{file_name}.{suffix}-{timestamp_ms}"));
        path
    }
}

impl ConfigStore for JsonConfigStore {
    fn load(&self) -> Result<AppConfig, PlatformError> {
        if !self.path.exists() {
            return Ok(default_config());
        }

        let raw = fs::read_to_string(&self.path).map_err(|error| PlatformError::Io {
            path: self.path.display().to_string(),
            message: error.to_string(),
        })?;
        deserialize_app_config(&raw).map_err(|error| {
            PlatformError::Message(format!("failed to decode {}: {error}", self.path.display()))
        })
    }

    fn save(&self, config: &AppConfig) -> Result<(), PlatformError> {
        self.ensure_parent()?;
        let json = serde_json::to_vec_pretty(config)
            .map_err(|error| PlatformError::Message(error.to_string()))?;
        let temp_path = self.temporary_write_path();
        let mut temp_file = File::create(&temp_path).map_err(|error| PlatformError::Io {
            path: temp_path.display().to_string(),
            message: error.to_string(),
        })?;
        temp_file
            .write_all(&json)
            .map_err(|error| PlatformError::Io {
                path: temp_path.display().to_string(),
                message: error.to_string(),
            })?;
        temp_file.sync_all().map_err(|error| PlatformError::Io {
            path: temp_path.display().to_string(),
            message: error.to_string(),
        })?;

        #[cfg(target_os = "windows")]
        {
            let backup_path = if self.path.exists() {
                let backup_path = self.recovery_path("bak");
                fs::rename(&self.path, &backup_path).map_err(|error| PlatformError::Io {
                    path: self.path.display().to_string(),
                    message: error.to_string(),
                })?;
                Some(backup_path)
            } else {
                None
            };

            return match fs::rename(&temp_path, &self.path) {
                Ok(()) => {
                    if let Some(backup_path) = backup_path {
                        let _ = fs::remove_file(backup_path);
                    }
                    Ok(())
                }
                Err(error) => {
                    if let Some(backup_path) = backup_path.as_ref() {
                        let _ = fs::rename(backup_path, &self.path);
                    }
                    Err(PlatformError::Io {
                        path: self.path.display().to_string(),
                        message: error.to_string(),
                    })
                }
            };
        }

        #[cfg(not(target_os = "windows"))]
        {
            fs::rename(&temp_path, &self.path).map_err(|error| PlatformError::Io {
                path: self.path.display().to_string(),
                message: error.to_string(),
            })
        }
    }
}

fn linux_config_base_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .map(|home| home.join(".config"))
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn deserialize_app_config(raw: &str) -> Result<AppConfig, serde_json::Error> {
    let mut value: serde_json::Value = serde_json::from_str(raw)?;
    migrate_app_config_value(&mut value);
    serde_json::from_value(value)
}

fn migrate_app_config_value(value: &mut serde_json::Value) {
    let Some(config) = value.as_object_mut() else {
        return;
    };

    let settings = ensure_json_object(
        config
            .entry("settings".to_string())
            .or_insert_with(empty_json_object),
    );
    let (mut device_defaults, layout_overrides) = extract_legacy_device_defaults(settings);
    let device_defaults_template = device_defaults.clone();

    let applied_layout_override = migrate_managed_device_settings(
        config,
        &device_defaults_template,
        layout_overrides.as_ref(),
    );
    apply_default_layout_override(
        &mut device_defaults,
        layout_overrides.as_ref(),
        applied_layout_override,
    );

    config.insert("deviceDefaults".to_string(), device_defaults);
    apply_config_version_migrations(config);
}

fn empty_json_object() -> serde_json::Value {
    serde_json::Value::Object(JsonMap::new())
}

fn ensure_json_object(value: &mut serde_json::Value) -> &mut JsonMap {
    if !value.is_object() {
        *value = empty_json_object();
    }

    value
        .as_object_mut()
        .expect("json object should remain an object")
}

fn default_device_settings_value() -> serde_json::Value {
    serde_json::to_value(default_device_settings())
        .expect("default device settings should serialize")
}

fn extract_legacy_device_defaults(settings: &mut JsonMap) -> (serde_json::Value, Option<JsonMap>) {
    let mut device_defaults = settings
        .remove("deviceDefaults")
        .unwrap_or_else(default_device_settings_value);
    if !device_defaults.is_object() {
        device_defaults = default_device_settings_value();
    }

    let device_defaults_map = ensure_json_object(&mut device_defaults);
    for (legacy_key, next_key) in [
        ("dpi", "dpi"),
        ("invertHorizontalScroll", "invertHorizontalScroll"),
        ("invertVerticalScroll", "invertVerticalScroll"),
        ("gestureThreshold", "gestureThreshold"),
        ("gestureDeadzone", "gestureDeadzone"),
        ("gestureTimeoutMs", "gestureTimeoutMs"),
        ("gestureCooldownMs", "gestureCooldownMs"),
    ] {
        if let Some(legacy_value) = settings.remove(legacy_key) {
            device_defaults_map.insert(next_key.to_string(), legacy_value);
        }
    }

    let layout_overrides = settings
        .remove("deviceLayoutOverrides")
        .and_then(|value| value.as_object().cloned());

    (device_defaults, layout_overrides)
}

fn migrate_managed_device_settings(
    config: &mut JsonMap,
    device_defaults_template: &serde_json::Value,
    layout_overrides: Option<&JsonMap>,
) -> bool {
    let Some(managed_devices) = config
        .get_mut("managedDevices")
        .and_then(|value| value.as_array_mut())
    else {
        return false;
    };

    let mut applied_layout_override = false;
    for device in managed_devices {
        let Some(device_object) = device.as_object_mut() else {
            continue;
        };
        let override_value = layout_override_for_device(device_object, layout_overrides);

        let device_settings = device_object
            .entry("settings".to_string())
            .or_insert_with(|| device_defaults_template.clone());
        let device_settings_map = ensure_json_object(device_settings);

        if let Some(override_value) = override_value {
            device_settings_map.insert("manualLayoutOverride".to_string(), override_value);
            applied_layout_override = true;
        }
    }

    applied_layout_override
}

fn layout_override_for_device(
    device: &JsonMap,
    layout_overrides: Option<&JsonMap>,
) -> Option<serde_json::Value> {
    let layout_overrides = layout_overrides?;
    let device_id = device.get("id").and_then(|value| value.as_str());
    let model_key = device.get("modelKey").and_then(|value| value.as_str());

    device_id
        .and_then(|device_id| layout_overrides.get(device_id))
        .or_else(|| model_key.and_then(|model_key| layout_overrides.get(model_key)))
        .cloned()
}

fn apply_default_layout_override(
    device_defaults: &mut serde_json::Value,
    layout_overrides: Option<&JsonMap>,
    applied_layout_override: bool,
) {
    if applied_layout_override {
        return;
    }

    let Some(layout_overrides) = layout_overrides else {
        return;
    };
    if layout_overrides.len() != 1 {
        return;
    }

    let Some(layout) = layout_overrides.values().next() else {
        return;
    };

    ensure_json_object(device_defaults).insert("manualLayoutOverride".to_string(), layout.clone());
}

fn apply_config_version_migrations(config: &mut JsonMap) {
    let version = config
        .get("version")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    if version < 4 {
        migrate_legacy_default_profile_bindings(config);
        config.insert("version".to_string(), serde_json::Value::from(4u64));
    }
}

fn migrate_legacy_default_profile_bindings(config: &mut JsonMap) {
    let Some(profiles) = config
        .get_mut("profiles")
        .and_then(|value| value.as_array_mut())
    else {
        return;
    };

    let Some(default_profile) = profiles.iter_mut().find_map(|profile| {
        let profile_object = profile.as_object_mut()?;
        let profile_id = profile_object.get("id")?.as_str()?;
        (profile_id == "default").then_some(profile_object)
    }) else {
        return;
    };

    let Some(bindings_value) = default_profile.get_mut("bindings") else {
        return;
    };

    let Ok(bindings) = serde_json::from_value::<Vec<Binding>>(bindings_value.clone()) else {
        return;
    };

    if normalize_bindings(bindings) == legacy_default_profile_bindings_v3() {
        *bindings_value = serde_json::to_value(default_profile_bindings())
            .expect("default profile bindings should serialize");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use mouser_core::LogicalControl;
    use mouser_platform::ConfigStore;

    fn temp_config_path(test_name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mouser-config-store-{test_name}-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn load_or_recover_preserves_invalid_json() {
        let path = temp_config_path("recover");
        fs::write(&path, "{not valid json").unwrap();

        let store = JsonConfigStore::new(path.clone());
        let (config, warning) = store.load_or_recover();
        assert_eq!(config, default_config());

        let warning = warning.expect("expected recovery warning");
        assert!(warning.contains("Preserved unreadable file"));

        let parent = path.parent().unwrap();
        let file_name = path.file_name().unwrap().to_string_lossy().to_string();
        let mut preserved_files = fs::read_dir(parent)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|entry_path| {
                entry_path != &path
                    && entry_path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with(&format!("{file_name}.corrupt-")))
            })
            .collect::<Vec<_>>();
        preserved_files.sort();
        assert_eq!(preserved_files.len(), 1);
    }

    #[test]
    fn save_and_load_round_trip() {
        let path = temp_config_path("round-trip");
        let store = JsonConfigStore::new(path.clone());
        let mut config = default_config();
        config.settings.debug_mode = true;

        store.save(&config).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded, config);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_migrates_global_device_settings_into_per_device_settings() {
        let path = temp_config_path("migrate");
        fs::write(
            &path,
            serde_json::json!({
                "version": 2,
                "activeProfileId": "default",
                "profiles": [{
                    "id": "default",
                    "label": "Default (All Apps)",
                    "appMatchers": [],
                    "bindings": [],
                }],
                "managedDevices": [{
                    "id": "mx_master_3s-1",
                    "modelKey": "mx_master_3s",
                    "displayName": "MX Master 3S",
                    "nickname": null,
                    "identityKey": null,
                    "createdAtMs": 1,
                    "lastSeenAtMs": null,
                    "lastSeenTransport": "Bluetooth Low Energy"
                }],
                "settings": {
                    "startMinimized": true,
                    "startAtLogin": false,
                    "appearanceMode": "system",
                    "debugMode": true,
                    "dpi": 1600,
                    "invertHorizontalScroll": true,
                    "gestureThreshold": 65,
                    "deviceLayoutOverrides": {
                        "mx_master_3s": "mx_master"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let store = JsonConfigStore::new(path.clone());
        let config = store.load().unwrap();

        assert_eq!(config.version, 4);
        assert!(config.settings.debug_mode);
        assert_eq!(config.device_defaults.dpi, 1600);
        let managed = config
            .managed_devices
            .iter()
            .find(|device| device.id == "mx_master_3s-1")
            .expect("expected managed device");
        assert_eq!(managed.settings.dpi, 1600);
        assert!(managed.settings.invert_horizontal_scroll);
        assert!(config.device_defaults.invert_horizontal_scroll);
        assert!(!config.device_defaults.invert_vertical_scroll);
        assert!(!config.device_defaults.macos_thumb_wheel_simulate_trackpad);
        assert_eq!(
            config
                .device_defaults
                .macos_thumb_wheel_trackpad_hold_timeout_ms,
            500
        );
        assert_eq!(config.device_defaults.gesture_threshold, 65);
        assert_eq!(config.managed_devices.len(), 1);
        assert_eq!(managed.settings.dpi, 1600);
        assert!(managed.settings.invert_horizontal_scroll);
        assert!(!managed.settings.macos_thumb_wheel_simulate_trackpad);
        assert_eq!(
            managed.settings.macos_thumb_wheel_trackpad_hold_timeout_ms,
            500
        );
        assert_eq!(managed.settings.gesture_threshold, 65);
        assert_eq!(
            managed.settings.manual_layout_override.as_deref(),
            Some("mx_master")
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn deserialize_migrates_legacy_default_profile_button_bindings() {
        let config = deserialize_app_config(
            &serde_json::json!({
                "version": 3,
                "activeProfileId": "default",
                "profiles": [{
                    "id": "default",
                    "label": "Default (All Apps)",
                    "appMatchers": [],
                    "bindings": [
                        { "control": "back", "actionId": "alt_tab" },
                        { "control": "forward", "actionId": "alt_tab" },
                        { "control": "hscroll_left", "actionId": "browser_back" },
                        { "control": "hscroll_right", "actionId": "browser_forward" }
                    ],
                }],
                "managedDevices": [],
                "settings": {
                    "startMinimized": true,
                    "startAtLogin": false,
                    "appearanceMode": "system",
                    "debugMode": false
                },
                "deviceDefaults": default_device_settings()
            })
            .to_string(),
        )
        .expect("expected config to deserialize");

        let default_profile = config
            .profiles
            .iter()
            .find(|profile| profile.id == "default")
            .expect("expected default profile");

        assert_eq!(config.version, 4);
        assert_eq!(
            default_profile
                .binding_for(LogicalControl::Back)
                .map(|binding| binding.action_id.as_str()),
            Some("browser_back")
        );
        assert_eq!(
            default_profile
                .binding_for(LogicalControl::Forward)
                .map(|binding| binding.action_id.as_str()),
            Some("browser_forward")
        );
    }
}
