#[cfg(target_os = "macos")]
use std::path::Path;
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "macos")]
use mouser_core::build_connected_device_info;
use mouser_core::{
    clamp_dpi, default_config, default_device_settings, default_known_apps_ref,
    default_layouts_ref, known_device_specs_ref, AppConfig, AppDiscoverySource, AppIdentity,
    DebugEventKind, DeviceFingerprint, DeviceInfo, DeviceLayout, DeviceSettings, InstalledApp,
    KnownApp, LogicalControl, Profile, Settings,
};
use thiserror::Error;

mod gesture;
mod hidpp;
mod macos_hook;
mod macos_iokit;
mod windows_backend;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCapabilities {
    pub can_intercept_buttons: bool,
    pub can_intercept_scroll: bool,
    pub supports_gesture_diversion: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidCapabilities {
    pub can_enumerate_devices: bool,
    pub can_read_battery: bool,
    pub can_read_dpi: bool,
    pub can_write_dpi: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookBackendSettings {
    pub invert_horizontal_scroll: bool,
    pub invert_vertical_scroll: bool,
    pub macos_thumb_wheel_simulate_trackpad: bool,
    pub macos_thumb_wheel_trackpad_hold_timeout_ms: u32,
    pub gesture_threshold: u16,
    pub gesture_deadzone: u16,
    pub gesture_timeout_ms: u32,
    pub gesture_cooldown_ms: u32,
    pub debug_mode: bool,
}

impl HookBackendSettings {
    pub fn from_app_and_device(
        settings: &Settings,
        device_settings: &DeviceSettings,
        device_model_key: Option<&str>,
    ) -> Self {
        Self {
            invert_horizontal_scroll: device_settings.invert_horizontal_scroll,
            invert_vertical_scroll: device_settings.invert_vertical_scroll,
            macos_thumb_wheel_simulate_trackpad: cfg!(target_os = "macos")
                && device_settings.macos_thumb_wheel_simulate_trackpad
                && supports_macos_thumb_wheel_trackpad_model(device_model_key),
            macos_thumb_wheel_trackpad_hold_timeout_ms: device_settings
                .macos_thumb_wheel_trackpad_hold_timeout_ms,
            gesture_threshold: device_settings.gesture_threshold,
            gesture_deadzone: device_settings.gesture_deadzone,
            gesture_timeout_ms: device_settings.gesture_timeout_ms,
            gesture_cooldown_ms: device_settings.gesture_cooldown_ms,
            debug_mode: settings.debug_mode,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookEvent {
    pub control: LogicalControl,
    pub pressed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookBackendEvent {
    pub kind: DebugEventKind,
    pub message: String,
}

fn supports_macos_thumb_wheel_trackpad_model(model_key: Option<&str>) -> bool {
    model_key.is_some_and(|model_key| {
        model_key == "mx_master" || model_key.starts_with("mx_master_")
    })
}

pub(crate) const HOOK_EVENT_BUFFER_LIMIT: usize = 128;

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("{0}")]
    Unsupported(&'static str),
    #[error("{0}")]
    Message(String),
    #[error("io error at {path}: {message}")]
    Io { path: String, message: String },
}

pub trait HookBackend: Send + Sync {
    fn backend_id(&self) -> &'static str;
    fn capabilities(&self) -> HookCapabilities;
    fn configure(
        &self,
        settings: &HookBackendSettings,
        profile: &Profile,
        enabled: bool,
    ) -> Result<(), PlatformError>;
    fn drain_events(&self) -> Vec<HookBackendEvent>;
}

pub trait HidBackend: Send + Sync {
    fn backend_id(&self) -> &'static str;
    fn capabilities(&self) -> HidCapabilities;
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, PlatformError>;
    fn set_device_dpi(&self, device_key: &str, dpi: u16) -> Result<(), PlatformError>;
}

pub trait AppFocusBackend: Send + Sync {
    fn backend_id(&self) -> &'static str;
    fn current_frontmost_app(&self) -> Result<Option<AppIdentity>, PlatformError>;
}

pub trait AppDiscoveryBackend: Send + Sync {
    fn backend_id(&self) -> &'static str;
    fn discover_apps(&self) -> Result<Vec<InstalledApp>, PlatformError>;
}

pub trait DeviceCatalog: Send + Sync {
    fn all_devices(&self) -> Vec<DeviceInfo>;
    fn all_layouts(&self) -> Vec<DeviceLayout>;
    fn known_apps(&self) -> Vec<KnownApp>;
    fn clamp_dpi(&self, device_key: Option<&str>, value: u16) -> u16;
}

pub trait ConfigStore: Send + Sync {
    fn load(&self) -> Result<AppConfig, PlatformError>;
    fn save(&self, config: &AppConfig) -> Result<(), PlatformError>;
}

pub(crate) fn horizontal_scroll_control(delta: i32) -> Option<LogicalControl> {
    match delta.cmp(&0) {
        std::cmp::Ordering::Greater => Some(LogicalControl::HscrollRight),
        std::cmp::Ordering::Less => Some(LogicalControl::HscrollLeft),
        std::cmp::Ordering::Equal => None,
    }
}

pub(crate) fn push_bounded_hook_event(
    events: &mut Vec<HookBackendEvent>,
    kind: DebugEventKind,
    message: impl Into<String>,
) {
    events.push(HookBackendEvent {
        kind,
        message: message.into(),
    });
    if events.len() > HOOK_EVENT_BUFFER_LIMIT {
        let excess = events.len() - HOOK_EVENT_BUFFER_LIMIT;
        events.drain(0..excess);
    }
}

pub(crate) fn dedupe_installed_apps(apps: Vec<InstalledApp>) -> Result<Vec<InstalledApp>, PlatformError> {
    let mut deduped = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for mut app in apps {
        let stable_id = app.identity.stable_id();
        if !seen.insert(stable_id) {
            continue;
        }

        if app.identity.preferred_matchers().is_empty() {
            continue;
        }

        app.source_kinds.sort();
        app.source_kinds.dedup();
        deduped.push(app);
    }

    deduped.sort_by(|left, right| {
        left.identity
            .label_or_fallback()
            .unwrap_or_default()
            .cmp(&right.identity.label_or_fallback().unwrap_or_default())
    });

    Ok(deduped)
}

#[derive(Clone, Default)]
pub struct StaticDeviceCatalog {
    layouts: Vec<DeviceLayout>,
    devices: Vec<DeviceInfo>,
    apps: Vec<KnownApp>,
}

impl StaticDeviceCatalog {
    pub fn new() -> Self {
        Self {
            layouts: default_layouts_ref().to_vec(),
            devices: known_device_specs_ref()
                .iter()
                .cloned()
                .map(|spec| DeviceInfo {
                    key: spec.key.clone(),
                    model_key: spec.key,
                    display_name: spec.display_name.clone(),
                    nickname: None,
                    product_id: spec.product_ids.first().copied(),
                    product_name: Some(spec.display_name),
                    transport: None,
                    source: Some("catalog".to_string()),
                    ui_layout: spec.ui_layout,
                    image_asset: spec.image_asset,
                    supported_controls: spec.supported_controls,
                    gesture_cids: spec.gesture_cids,
                    dpi_min: spec.dpi_min,
                    dpi_max: spec.dpi_max,
                    connected: false,
                    battery_level: None,
                    current_dpi: 1000,
                    fingerprint: DeviceFingerprint::default(),
                })
                .collect(),
            apps: default_known_apps_ref().to_vec(),
        }
    }

    pub fn layouts(&self) -> &[DeviceLayout] {
        &self.layouts
    }

    pub fn known_apps_ref(&self) -> &[KnownApp] {
        &self.apps
    }

    pub fn layout_by_key(&self, layout_key: &str) -> Option<&DeviceLayout> {
        self.layouts.iter().find(|layout| layout.key == layout_key)
    }
}

impl DeviceCatalog for StaticDeviceCatalog {
    fn all_devices(&self) -> Vec<DeviceInfo> {
        self.devices.clone()
    }

    fn all_layouts(&self) -> Vec<DeviceLayout> {
        self.layouts.clone()
    }

    fn known_apps(&self) -> Vec<KnownApp> {
        self.apps.clone()
    }

    fn clamp_dpi(&self, device_key: Option<&str>, value: u16) -> u16 {
        let device = device_key.and_then(|device_key| {
            self.devices
                .iter()
                .find(|candidate| candidate.key == device_key)
        });
        clamp_dpi(device, value)
    }
}

pub struct JsonConfigStore {
    path: PathBuf,
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

    let (mut device_defaults, layout_overrides) = {
        let settings_value = config
            .entry("settings".to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !settings_value.is_object() {
            *settings_value = serde_json::Value::Object(serde_json::Map::new());
        }
        let settings = settings_value.as_object_mut().unwrap();

        let mut device_defaults = settings
            .remove("deviceDefaults")
            .filter(|value| value.is_object())
            .unwrap_or_else(|| {
                serde_json::to_value(default_device_settings())
                    .expect("default device settings should serialize")
            });
        if !device_defaults.is_object() {
            device_defaults = serde_json::to_value(default_device_settings())
                .expect("default device settings should serialize");
        }
        {
            let device_defaults_map = device_defaults.as_object_mut().unwrap();
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
        }

        let layout_overrides = settings
            .remove("deviceLayoutOverrides")
            .and_then(|value| value.as_object().cloned());
        (device_defaults, layout_overrides)
    };
    let device_defaults_template = device_defaults.clone();

    let mut applied_layout_override = false;
    if let Some(managed_devices) = config
        .get_mut("managedDevices")
        .and_then(|value| value.as_array_mut())
    {
        for device in managed_devices {
            let Some(device_object) = device.as_object_mut() else {
                continue;
            };
            let device_id = device_object
                .get("id")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let model_key = device_object
                .get("modelKey")
                .and_then(|value| value.as_str())
                .map(str::to_string);

            let device_settings = device_object
                .entry("settings".to_string())
                .or_insert_with(|| device_defaults_template.clone());
            if !device_settings.is_object() {
                *device_settings = device_defaults_template.clone();
            }
            let device_settings_map = device_settings.as_object_mut().unwrap();

            if let Some(layout_overrides) = layout_overrides.as_ref() {
                let override_value = device_id
                    .as_deref()
                    .and_then(|device_id| layout_overrides.get(device_id))
                    .or_else(|| {
                        model_key
                            .as_deref()
                            .and_then(|model_key| layout_overrides.get(model_key))
                    })
                    .cloned();
                if let Some(override_value) = override_value {
                    device_settings_map
                        .insert("manualLayoutOverride".to_string(), override_value);
                    applied_layout_override = true;
                }
            }
        }
    }

    if !applied_layout_override {
        if let Some(layout_overrides) = layout_overrides.as_ref() {
            if layout_overrides.len() == 1 {
                if let Some(layout) = layout_overrides.values().next() {
                    device_defaults
                        .as_object_mut()
                        .unwrap()
                        .insert("manualLayoutOverride".to_string(), layout.clone());
                }
            }
        }
    }

    config.insert("deviceDefaults".to_string(), device_defaults);

    if config.get("version").and_then(|value| value.as_u64()).unwrap_or(0) < 3 {
        config.insert("version".to_string(), serde_json::Value::from(3u64));
    }
}

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
        } else if cfg!(target_os = "windows") {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        };

        base.join("Mouser Tauri").join("config.json")
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

#[cfg_attr(not(target_os = "macos"), allow(dead_code, unused_imports))]
pub mod macos {
    use super::*;
    use crate::hidpp::{self, HidppIo, BT_DEV_IDX};
    pub use crate::macos_hook::MacOsHookBackend;
    #[cfg(target_os = "macos")]
    use crate::macos_iokit::{enumerate_iokit_infos, MacOsIoKitInfo, MacOsNativeHidDevice};

    #[cfg(target_os = "macos")]
    use hidapi::{BusType, DeviceInfo as HidDeviceInfo, HidApi, HidDevice};
    #[cfg(target_os = "macos")]
    use mouser_core::hydrate_identity_key;
    #[cfg(target_os = "macos")]
    use objc2_app_kit::NSWorkspace;
    #[cfg(target_os = "macos")]
    use plist::Value as PlistValue;

    const LOGI_VID: u16 = 0x046D;

    pub struct MacOsHidBackend;
    pub struct MacOsAppFocusBackend;
    pub struct MacOsAppDiscoveryBackend;

    impl HidBackend for MacOsHidBackend {
        fn backend_id(&self) -> &'static str {
            if cfg!(target_os = "macos") {
                "macos-iokit+hidapi"
            } else {
                "macos-unsupported"
            }
        }

        fn capabilities(&self) -> HidCapabilities {
            if cfg!(target_os = "macos") {
                HidCapabilities {
                    can_enumerate_devices: true,
                    can_read_battery: true,
                    can_read_dpi: true,
                    can_write_dpi: true,
                }
            } else {
                HidCapabilities {
                    can_enumerate_devices: false,
                    can_read_battery: false,
                    can_read_dpi: false,
                    can_write_dpi: false,
                }
            }
        }

        fn list_devices(&self) -> Result<Vec<DeviceInfo>, PlatformError> {
            #[cfg(not(target_os = "macos"))]
            {
                Err(PlatformError::Unsupported(
                    "live macOS HID integration is only available on macOS",
                ))
            }

            #[cfg(target_os = "macos")]
            {
                let mut devices = Vec::new();
                let mut issues = Vec::new();

                match enumerate_iokit_infos() {
                    Ok(infos) => {
                        for info in infos {
                            if let Some(device_info) = probe_iokit_device(&info) {
                                push_unique_device(&mut devices, device_info);
                            }
                        }
                    }
                    Err(error) => issues.push(format!("iokit: {error}")),
                }

                match HidApi::new() {
                    Ok(api) => {
                        for info in vendor_hid_infos(&api) {
                            if let Ok(device) = info.open_device(&api) {
                                if let Some(device_info) = probe_hidapi_device(&device, info) {
                                    push_unique_device(&mut devices, device_info);
                                }
                            }
                        }
                    }
                    Err(error) => issues.push(format!("hidapi: {error}")),
                }

                if devices.is_empty() && !issues.is_empty() {
                    return Err(PlatformError::Message(format!(
                        "failed to enumerate macOS HID devices: {}",
                        issues.join("; ")
                    )));
                }

                Ok(devices)
            }
        }

        fn set_device_dpi(&self, device_key: &str, dpi: u16) -> Result<(), PlatformError> {
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (device_key, dpi);
                Err(PlatformError::Unsupported(
                    "live macOS HID integration is only available on macOS",
                ))
            }

            #[cfg(target_os = "macos")]
            {
                if let Ok(infos) = enumerate_iokit_infos() {
                    for info in infos {
                        let transport = iokit_transport_label(info.transport.as_deref());
                        if !device_key_matches(
                            device_key,
                            Some(info.product_id),
                            info.product_string.as_deref(),
                            transport.as_deref(),
                            "iokit",
                            fingerprint_from_iokit_info(&info),
                            dpi,
                        ) {
                            continue;
                        }

                        let Ok(device) = open_iokit_device(&info) else {
                            continue;
                        };
                        if set_hidpp_dpi(&device, dpi)? {
                            return Ok(());
                        }
                    }
                }

                if let Ok(api) = HidApi::new() {
                    for info in vendor_hid_infos(&api) {
                        let Ok(device) = info.open_device(&api) else {
                            continue;
                        };
                        if device_key_matches(
                            device_key,
                            Some(info.product_id()),
                            info.product_string(),
                            Some(transport_label(info.bus_type())),
                            "hidapi",
                            fingerprint_from_hid_info(info),
                            dpi,
                        ) && set_hidpp_dpi(&device, dpi)?
                        {
                            return Ok(());
                        }
                    }
                }

                Err(PlatformError::Message(format!(
                    "could not find a live Logitech device matching `{device_key}`"
                )))
            }
        }
    }

    impl AppFocusBackend for MacOsAppFocusBackend {
        fn backend_id(&self) -> &'static str {
            if cfg!(target_os = "macos") {
                "macos-nsworkspace"
            } else {
                "macos-unsupported"
            }
        }

        fn current_frontmost_app(&self) -> Result<Option<AppIdentity>, PlatformError> {
            #[cfg(not(target_os = "macos"))]
            {
                Err(PlatformError::Unsupported(
                    "live macOS app focus detection is only available on macOS",
                ))
            }

            #[cfg(target_os = "macos")]
            {
                let workspace = NSWorkspace::sharedWorkspace();
                let Some(app) = workspace.frontmostApplication() else {
                    return Ok(None);
                };

                let executable_path = app
                    .executableURL()
                    .and_then(|url| url.path())
                    .map(|path| path.to_string());
                let executable = executable_path.as_deref().and_then(|path| {
                    Path::new(path)
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                });

                Ok(Some(AppIdentity {
                    label: app.localizedName().map(|name| name.to_string()),
                    executable,
                    executable_path,
                    bundle_id: app.bundleIdentifier().map(|bundle_id| bundle_id.to_string()),
                    package_family_name: None,
                }))
            }
        }
    }

    impl AppDiscoveryBackend for MacOsAppDiscoveryBackend {
        fn backend_id(&self) -> &'static str {
            if cfg!(target_os = "macos") {
                "macos-applications"
            } else {
                "macos-unsupported"
            }
        }

        fn discover_apps(&self) -> Result<Vec<InstalledApp>, PlatformError> {
            #[cfg(not(target_os = "macos"))]
            {
                Err(PlatformError::Unsupported(
                    "macOS app discovery is only available on macOS",
                ))
            }

            #[cfg(target_os = "macos")]
            {
                let mut apps = Vec::new();
                for root in macos_application_roots() {
                    collect_macos_apps_from_root(&root, &mut apps)?;
                }
                dedupe_installed_apps(apps)
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn macos_application_roots() -> Vec<PathBuf> {
        let mut roots = vec![
            PathBuf::from("/Applications"),
            PathBuf::from("/System/Applications"),
            PathBuf::from("/System/Applications/Utilities"),
        ];
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("Applications"));
        }
        roots
    }

    #[cfg(target_os = "macos")]
    fn collect_macos_apps_from_root(
        root: &Path,
        apps: &mut Vec<InstalledApp>,
    ) -> Result<(), PlatformError> {
        if !root.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(root).map_err(|error| PlatformError::Io {
            path: root.display().to_string(),
            message: error.to_string(),
        })? {
            let entry = entry.map_err(|error| PlatformError::Io {
                path: root.display().to_string(),
                message: error.to_string(),
            })?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("app") {
                continue;
            }

            if let Some(app) = read_macos_bundle(&path) {
                apps.push(app);
            }
        }

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn read_macos_bundle(path: &Path) -> Option<InstalledApp> {
        let plist_path = path.join("Contents").join("Info.plist");
        let plist = PlistValue::from_file(&plist_path).ok()?;
        let dict = plist.as_dictionary()?;
        if dict
            .get("LSBackgroundOnly")
            .and_then(PlistValue::as_boolean)
            .unwrap_or(false)
        {
            return None;
        }

        let label = dict
            .get("CFBundleDisplayName")
            .and_then(PlistValue::as_string)
            .or_else(|| dict.get("CFBundleName").and_then(PlistValue::as_string))
            .map(str::trim)
            .filter(|value: &&str| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                path.file_stem()
                    .map(|value| value.to_string_lossy().to_string())
            })?;
        if label.to_ascii_lowercase().contains("helper") {
            return None;
        }

        let executable = dict
            .get("CFBundleExecutable")
            .and_then(PlistValue::as_string)
            .map(str::trim)
            .filter(|value: &&str| !value.is_empty())
            .map(str::to_string);
        let executable_path = executable
            .as_deref()
            .map(|name| path.join("Contents").join("MacOS").join(name))
            .filter(|path: &PathBuf| path.exists())
            .map(|path: PathBuf| path.to_string_lossy().to_string());

        Some(InstalledApp {
            identity: AppIdentity {
                label: Some(label),
                executable,
                executable_path,
                bundle_id: dict
                    .get("CFBundleIdentifier")
                    .and_then(PlistValue::as_string)
                    .map(str::trim)
                    .filter(|value: &&str| !value.is_empty())
                    .map(str::to_string),
                package_family_name: None,
            },
            source_kinds: vec![AppDiscoverySource::ApplicationBundle],
            source_path: Some(path.to_string_lossy().to_string()),
        })
    }

    #[cfg(target_os = "macos")]
    fn vendor_hid_infos(api: &HidApi) -> Vec<&HidDeviceInfo> {
        api.device_list()
            .filter(|info| info.vendor_id() == LOGI_VID && info.usage_page() >= 0xFF00)
            .collect()
    }

    #[cfg(target_os = "macos")]
    impl HidppIo for MacOsNativeHidDevice {
        fn write_packet(&self, packet: &[u8]) -> Result<(), PlatformError> {
            self.write_report(packet)
        }

        fn read_packet(&self, timeout_ms: i32) -> Result<Vec<u8>, PlatformError> {
            self.read_timeout(timeout_ms)
        }
    }

    #[cfg(target_os = "macos")]
    fn probe_hidapi_device(device: &HidDevice, info: &HidDeviceInfo) -> Option<DeviceInfo> {
        probe_transport_device(
            device,
            Some(info.product_id()),
            info.product_string(),
            Some(transport_label(info.bus_type())),
            "hidapi",
            fingerprint_from_hid_info(info),
        )
    }

    #[cfg(target_os = "macos")]
    fn probe_iokit_device(info: &MacOsIoKitInfo) -> Option<DeviceInfo> {
        let transport = iokit_transport_label(info.transport.as_deref());
        let device = open_iokit_device(info).ok()?;
        probe_transport_device(
            &device,
            Some(info.product_id),
            info.product_string.as_deref(),
            transport.as_deref(),
            "iokit",
            fingerprint_from_iokit_info(info),
        )
    }

    #[cfg(target_os = "macos")]
    fn probe_transport_device<T: HidppIo + ?Sized>(
        device: &T,
        product_id: Option<u16>,
        product_name: Option<&str>,
        transport: Option<&str>,
        source: &'static str,
        fingerprint: DeviceFingerprint,
    ) -> Option<DeviceInfo> {
        let current_dpi = hidpp::read_sensor_dpi(device, BT_DEV_IDX, 1_500)
            .ok()
            .flatten()
            .unwrap_or(1000);
        let battery_level = hidpp::read_battery_level(device, BT_DEV_IDX, 1_500)
            .ok()
            .flatten();
        Some(build_connected_device_info(
            product_id,
            product_name,
            transport,
            Some(source),
            battery_level,
            current_dpi,
            fingerprint,
        ))
    }

    #[cfg(target_os = "macos")]
    fn fingerprint_from_hid_info(info: &HidDeviceInfo) -> DeviceFingerprint {
        let mut fingerprint = DeviceFingerprint {
            identity_key: None,
            serial_number: info.serial_number().map(str::to_string),
            hid_path: Some(info.path().to_string_lossy().into_owned()),
            interface_number: Some(info.interface_number()),
            usage_page: Some(info.usage_page()),
            usage: Some(info.usage()),
            location_id: None,
        };
        hydrate_identity_key(Some(info.product_id()), &mut fingerprint);
        fingerprint
    }

    #[cfg(target_os = "macos")]
    fn fingerprint_from_iokit_info(info: &MacOsIoKitInfo) -> DeviceFingerprint {
        let mut fingerprint = DeviceFingerprint {
            identity_key: None,
            serial_number: info.serial_number.clone(),
            hid_path: None,
            interface_number: None,
            usage_page: Some(info.usage_page as u16),
            usage: Some(info.usage as u16),
            location_id: info.location_id,
        };
        hydrate_identity_key(Some(info.product_id), &mut fingerprint);
        fingerprint
    }

    #[cfg(target_os = "macos")]
    fn transport_label(bus_type: BusType) -> &'static str {
        match bus_type {
            BusType::Bluetooth => "Bluetooth Low Energy",
            BusType::Usb => "USB",
            BusType::I2c => "I2C",
            BusType::Spi => "SPI",
            BusType::Unknown => "Unknown transport",
        }
    }

    #[cfg(target_os = "macos")]
    fn iokit_transport_label(transport: Option<&str>) -> Option<String> {
        transport.map(|value| match value {
            "Bluetooth" => "Bluetooth Low Energy".to_string(),
            other => other.to_string(),
        })
    }

    #[cfg(target_os = "macos")]
    fn push_unique_device(devices: &mut Vec<DeviceInfo>, device: DeviceInfo) {
        if devices
            .iter()
            .all(|existing: &DeviceInfo| existing.key != device.key)
        {
            devices.push(device);
        }
    }

    #[cfg(target_os = "macos")]
    fn device_key_matches(
        device_key: &str,
        product_id: Option<u16>,
        product_name: Option<&str>,
        transport: Option<&str>,
        source: &'static str,
        fingerprint: DeviceFingerprint,
        dpi: u16,
    ) -> bool {
        build_connected_device_info(
            product_id,
            product_name,
            transport,
            Some(source),
            None,
            dpi,
            fingerprint,
        )
        .key == device_key
    }

    #[cfg(target_os = "macos")]
    fn iokit_open_candidates(info: &MacOsIoKitInfo) -> Vec<MacOsIoKitInfo> {
        let mut candidates = vec![info.clone()];
        if info.transport.as_deref() != Some("USB") {
            candidates.push(MacOsIoKitInfo {
                product_id: info.product_id,
                usage_page: 0,
                usage: 0,
                transport: Some("Bluetooth Low Energy".to_string()),
                product_string: info.product_string.clone(),
                serial_number: info.serial_number.clone(),
                location_id: info.location_id,
            });
        }
        candidates
    }

    #[cfg(target_os = "macos")]
    fn open_iokit_device(info: &MacOsIoKitInfo) -> Result<MacOsNativeHidDevice, PlatformError> {
        let mut last_error = None;

        for candidate in iokit_open_candidates(info) {
            match MacOsNativeHidDevice::open(&candidate) {
                Ok(device) => return Ok(device),
                Err(error) => last_error = Some(error),
            }
        }

        Err(last_error.unwrap_or_else(|| {
            PlatformError::Message(format!(
                "could not open IOHIDDevice for pid 0x{:04X}",
                info.product_id
            ))
        }))
    }

    #[cfg(target_os = "macos")]
    fn set_hidpp_dpi<T: HidppIo + ?Sized>(device: &T, dpi: u16) -> Result<bool, PlatformError> {
        hidpp::set_sensor_dpi(device, BT_DEV_IDX, dpi, 1_500)
    }
}

pub mod windows {
    pub use crate::windows_backend::{
        WindowsAppDiscoveryBackend, WindowsAppFocusBackend, WindowsHidBackend, WindowsHookBackend,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use mouser_core::default_settings;

    fn unique_test_path(name: &str) -> PathBuf {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        std::env::temp_dir().join(format!("mouser-platform-{name}-{timestamp_ms}.json"))
    }

    #[test]
    fn horizontal_scroll_mapping_is_consistent() {
        assert_eq!(
            horizontal_scroll_control(120),
            Some(LogicalControl::HscrollRight)
        );
        assert_eq!(
            horizontal_scroll_control(-120),
            Some(LogicalControl::HscrollLeft)
        );
        assert_eq!(horizontal_scroll_control(0), None);
    }

    #[test]
    fn bounded_hook_event_queue_keeps_latest_entries() {
        let mut events = Vec::new();
        for index in 0..(HOOK_EVENT_BUFFER_LIMIT + 5) {
            push_bounded_hook_event(&mut events, DebugEventKind::Info, format!("event-{index}"));
        }

        assert_eq!(events.len(), HOOK_EVENT_BUFFER_LIMIT);
        assert_eq!(
            events.first().map(|event| event.message.as_str()),
            Some("event-5")
        );
        assert_eq!(
            events.last().map(|event| event.message.as_str()),
            Some("event-132")
        );
    }

    #[test]
    fn thumb_wheel_trackpad_setting_only_enables_for_mx_master_models() {
        let settings = default_settings();
        let mut device_settings = default_device_settings();
        device_settings.macos_thumb_wheel_simulate_trackpad = true;
        device_settings.macos_thumb_wheel_trackpad_hold_timeout_ms = 900;

        let mx_master = HookBackendSettings::from_app_and_device(
            &settings,
            &device_settings,
            Some("mx_master_3s"),
        );
        let generic_mouse =
            HookBackendSettings::from_app_and_device(&settings, &device_settings, Some("mouse"));
        let unknown_device =
            HookBackendSettings::from_app_and_device(&settings, &device_settings, None);

        assert_eq!(
            mx_master.macos_thumb_wheel_simulate_trackpad,
            cfg!(target_os = "macos")
        );
        assert_eq!(mx_master.macos_thumb_wheel_trackpad_hold_timeout_ms, 900);
        assert!(!generic_mouse.macos_thumb_wheel_simulate_trackpad);
        assert_eq!(
            generic_mouse.macos_thumb_wheel_trackpad_hold_timeout_ms,
            900
        );
        assert!(!unknown_device.macos_thumb_wheel_simulate_trackpad);
        assert_eq!(
            unknown_device.macos_thumb_wheel_trackpad_hold_timeout_ms,
            900
        );
    }

    #[test]
    fn load_or_recover_preserves_invalid_json() {
        let path = unique_test_path("recover");
        fs::write(&path, "{not valid json").unwrap();
        let store = JsonConfigStore::new(path.clone());

        let (config, warning) = store.load_or_recover();

        assert_eq!(config, default_config());
        let warning = warning.expect("expected recovery warning");
        assert!(warning.contains("Preserved unreadable file"));
        assert!(!path.exists());

        let parent = path.parent().unwrap();
        let file_name = path.file_name().unwrap().to_string_lossy().to_string();
        let recovered = fs::read_dir(parent)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().starts_with(&file_name))
            .expect("expected preserved config backup");

        assert!(recovered.path().exists());
        let _ = fs::remove_file(recovered.path());
    }

    #[test]
    fn load_migrates_global_device_settings_into_per_device_settings() {
        let path = unique_test_path("migrate");
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

        assert_eq!(config.version, 3);
        assert_eq!(config.settings.debug_mode, true);
        assert_eq!(config.device_defaults.dpi, 1600);
        let managed = config
            .managed_devices
            .iter()
            .find(|device| device.id == "mx_master_3s-1")
            .expect("expected managed device");
        assert_eq!(managed.settings.dpi, 1600);
        assert_eq!(managed.settings.invert_horizontal_scroll, true);
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
}
