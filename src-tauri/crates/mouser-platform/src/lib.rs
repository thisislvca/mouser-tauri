use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(target_os = "macos")]
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use mouser_core::build_connected_device_info;
use mouser_core::{
    clamp_dpi, default_config, default_device_settings, default_known_apps, default_layouts,
    known_device_specs, AppConfig, DebugEventKind, DeviceFingerprint, DeviceInfo, DeviceLayout,
    DeviceSettings, KnownApp, LogicalControl, Profile, Settings,
};
use serde_json::{json, Value};
use thiserror::Error;

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
pub struct HookEvent {
    pub control: LogicalControl,
    pub pressed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookBackendEvent {
    pub kind: DebugEventKind,
    pub message: String,
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

fn supports_macos_thumb_wheel_trackpad_model(model_key: Option<&str>) -> bool {
    model_key
        .is_some_and(|model_key| model_key == "mx_master" || model_key.starts_with("mx_master_"))
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
    fn current_frontmost_app(&self) -> Result<Option<String>, PlatformError>;
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

#[derive(Clone, Default)]
pub struct StaticDeviceCatalog {
    layouts: Vec<DeviceLayout>,
    devices: Vec<DeviceInfo>,
    apps: Vec<KnownApp>,
}

impl StaticDeviceCatalog {
    pub fn new() -> Self {
        Self {
            layouts: default_layouts(),
            devices: known_device_specs()
                .into_iter()
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
            apps: default_known_apps(),
        }
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
            Ok(raw) => match decode_app_config(&raw) {
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
        decode_app_config(&raw).map_err(|error| {
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

fn decode_app_config(raw: &str) -> Result<AppConfig, serde_json::Error> {
    let mut value: Value = serde_json::from_str(raw)?;
    migrate_app_config_value(&mut value);
    serde_json::from_value(value)
}

fn migrate_app_config_value(value: &mut Value) {
    let Some(root) = value.as_object_mut() else {
        return;
    };

    let legacy_settings = root.get("settings").and_then(Value::as_object);
    let legacy_layout_overrides = legacy_settings
        .and_then(|settings| settings.get("deviceLayoutOverrides"))
        .and_then(Value::as_object)
        .cloned();

    let had_device_defaults = root.get("deviceDefaults").is_some();
    let mut device_defaults = root
        .get("deviceDefaults")
        .cloned()
        .unwrap_or_else(|| serde_json::to_value(default_device_settings()).unwrap_or(json!({})));

    if let Some(defaults) = device_defaults.as_object_mut() {
        for field in [
            "dpi",
            "invertHorizontalScroll",
            "invertVerticalScroll",
            "gestureThreshold",
            "gestureDeadzone",
            "gestureTimeoutMs",
            "gestureCooldownMs",
        ] {
            if !had_device_defaults || defaults.get(field).is_none() {
                if let Some(legacy_value) = legacy_settings.and_then(|settings| settings.get(field))
                {
                    defaults.insert(field.to_string(), legacy_value.clone());
                }
            }
        }
    }
    root.insert("deviceDefaults".to_string(), device_defaults.clone());

    if let Some(managed_devices) = root.get_mut("managedDevices").and_then(Value::as_array_mut) {
        for device in managed_devices {
            let Some(device_obj) = device.as_object_mut() else {
                continue;
            };

            if device_obj.get("settings").is_none() {
                device_obj.insert("settings".to_string(), device_defaults.clone());
            }

            if let Some(layout_override) = legacy_layout_overrides
                .as_ref()
                .and_then(|overrides| {
                    let device_id = device_obj.get("id").and_then(Value::as_str);
                    let model_key = device_obj.get("modelKey").and_then(Value::as_str);
                    device_id
                        .and_then(|id| overrides.get(id))
                        .or_else(|| model_key.and_then(|key| overrides.get(key)))
                })
                .cloned()
            {
                let settings = device_obj
                    .entry("settings".to_string())
                    .or_insert_with(|| device_defaults.clone());
                if let Some(settings_obj) = settings.as_object_mut() {
                    let entry = settings_obj
                        .entry("manualLayoutOverride".to_string())
                        .or_insert(Value::Null);
                    if entry.is_null() {
                        *entry = layout_override;
                    }
                }
            }
        }
    }

    if root
        .get("version")
        .and_then(Value::as_u64)
        .is_some_and(|version| version < 3)
    {
        root.insert("version".to_string(), Value::from(3));
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code, unused_imports))]
pub mod macos {
    use super::*;
    pub use crate::macos_hook::MacOsHookBackend;
    #[cfg(target_os = "macos")]
    use crate::macos_iokit::{enumerate_iokit_infos, MacOsIoKitInfo, MacOsNativeHidDevice};

    #[cfg(target_os = "macos")]
    use hidapi::{BusType, DeviceInfo as HidDeviceInfo, HidApi, HidDevice};
    #[cfg(target_os = "macos")]
    use mouser_core::hydrate_identity_key;
    #[cfg(target_os = "macos")]
    use objc2_app_kit::NSWorkspace;

    const LOGI_VID: u16 = 0x046D;
    const LONG_ID: u8 = 0x11;
    const LONG_LEN: usize = 20;
    const BT_DEV_IDX: u8 = 0xFF;
    const FEAT_ADJ_DPI: u16 = 0x2201;
    const FEAT_UNIFIED_BATT: u16 = 0x1004;
    const FEAT_BATTERY_STATUS: u16 = 0x1000;
    const MY_SW: u8 = 0x0A;
    const HIDPP_GET_SENSOR_DPI_FN: u8 = 0x02;
    const HIDPP_SET_SENSOR_DPI_FN: u8 = 0x03;

    pub struct MacOsHidBackend;
    pub struct MacOsAppFocusBackend;

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
                    Err(error) => issues.push(format!("hidapi: {}", map_hid_error(error))),
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

        fn current_frontmost_app(&self) -> Result<Option<String>, PlatformError> {
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

                if let Some(url) = app.executableURL() {
                    if let Some(path) = url.path() {
                        let executable = Path::new(&path.to_string())
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string());
                        if executable.is_some() {
                            return Ok(executable);
                        }
                    }
                }

                if let Some(bundle_id) = app.bundleIdentifier() {
                    return Ok(Some(bundle_id.to_string()));
                }

                Ok(app.localizedName().map(|name| name.to_string()))
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn vendor_hid_infos(api: &HidApi) -> Vec<&HidDeviceInfo> {
        api.device_list()
            .filter(|info| info.vendor_id() == LOGI_VID && info.usage_page() >= 0xFF00)
            .collect()
    }

    #[cfg(target_os = "macos")]
    trait HidppDeviceIo {
        fn write_packet(&self, packet: &[u8]) -> Result<(), PlatformError>;
        fn read_packet(&self, timeout_ms: i32) -> Result<Vec<u8>, PlatformError>;
    }

    #[cfg(target_os = "macos")]
    impl HidppDeviceIo for HidDevice {
        fn write_packet(&self, packet: &[u8]) -> Result<(), PlatformError> {
            self.write(packet).map_err(map_hid_error)?;
            Ok(())
        }

        fn read_packet(&self, timeout_ms: i32) -> Result<Vec<u8>, PlatformError> {
            let mut buffer = [0u8; 64];
            let size = self
                .read_timeout(&mut buffer, timeout_ms)
                .map_err(map_hid_error)?;
            Ok(buffer[..size].to_vec())
        }
    }

    #[cfg(target_os = "macos")]
    impl HidppDeviceIo for MacOsNativeHidDevice {
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
    fn probe_transport_device<T: HidppDeviceIo + ?Sized>(
        device: &T,
        product_id: Option<u16>,
        product_name: Option<&str>,
        transport: Option<&str>,
        source: &'static str,
        fingerprint: DeviceFingerprint,
    ) -> Option<DeviceInfo> {
        let current_dpi = read_hidpp_current_dpi(device)
            .ok()
            .flatten()
            .unwrap_or(1000);
        let battery_level = read_hidpp_battery(device).ok().flatten();
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
    fn set_hidpp_dpi<T: HidppDeviceIo + ?Sized>(
        device: &T,
        dpi: u16,
    ) -> Result<bool, PlatformError> {
        let Some(feature_index) = find_feature(device, FEAT_ADJ_DPI)? else {
            return Ok(false);
        };

        let hi = ((dpi >> 8) & 0xFF) as u8;
        let lo = (dpi & 0xFF) as u8;
        Ok(request(device, feature_index, HIDPP_SET_SENSOR_DPI_FN, &[0, hi, lo])?.is_some())
    }

    #[cfg(target_os = "macos")]
    fn read_hidpp_current_dpi<T: HidppDeviceIo + ?Sized>(
        device: &T,
    ) -> Result<Option<u16>, PlatformError> {
        let Some(feature_index) = find_feature(device, FEAT_ADJ_DPI)? else {
            return Ok(None);
        };

        let Some(response) = request(device, feature_index, HIDPP_GET_SENSOR_DPI_FN, &[0])? else {
            return Ok(None);
        };
        Ok(parse_sensor_dpi_response(&response))
    }

    #[cfg(target_os = "macos")]
    fn parse_sensor_dpi_response(response: &[u8]) -> Option<u16> {
        if response.len() < 3 {
            return None;
        }

        Some(u16::from(response[1]) << 8 | u16::from(response[2]))
    }

    #[cfg(target_os = "macos")]
    fn read_hidpp_battery<T: HidppDeviceIo + ?Sized>(
        device: &T,
    ) -> Result<Option<u8>, PlatformError> {
        if let Some(feature_index) = find_feature(device, FEAT_UNIFIED_BATT)? {
            if let Some(response) = request(device, feature_index, 1, &[])? {
                return Ok(response.first().copied());
            }
        }

        if let Some(feature_index) = find_feature(device, FEAT_BATTERY_STATUS)? {
            if let Some(response) = request(device, feature_index, 0, &[])? {
                return Ok(response.first().copied());
            }
        }

        Ok(None)
    }

    #[cfg(target_os = "macos")]
    fn find_feature<T: HidppDeviceIo + ?Sized>(
        device: &T,
        feature_id: u16,
    ) -> Result<Option<u8>, PlatformError> {
        let feature_hi = ((feature_id >> 8) & 0xFF) as u8;
        let feature_lo = (feature_id & 0xFF) as u8;
        let Some(response) = request(device, 0x00, 0, &[feature_hi, feature_lo, 0x00])? else {
            return Ok(None);
        };
        Ok(response
            .first()
            .copied()
            .filter(|feature_index| *feature_index != 0))
    }

    #[cfg(target_os = "macos")]
    fn request<T: HidppDeviceIo + ?Sized>(
        device: &T,
        feature_index: u8,
        function: u8,
        params: &[u8],
    ) -> Result<Option<Vec<u8>>, PlatformError> {
        write_request(device, feature_index, function, params)?;
        let deadline = Instant::now() + Duration::from_millis(1_500);
        let expected_reply_functions = [function, (function + 1) & 0x0F];

        while Instant::now() < deadline {
            let packet = device.read_packet(200)?;
            if packet.is_empty() {
                continue;
            }

            let Some((response_feature, response_function, response_sw, response_params)) =
                parse_message(&packet)
            else {
                continue;
            };

            if response_feature == 0xFF {
                return Ok(None);
            }

            if response_feature == feature_index
                && response_sw == MY_SW
                && expected_reply_functions.contains(&response_function)
            {
                return Ok(Some(response_params));
            }
        }

        Ok(None)
    }

    #[cfg(target_os = "macos")]
    fn write_request<T: HidppDeviceIo + ?Sized>(
        device: &T,
        feature_index: u8,
        function: u8,
        params: &[u8],
    ) -> Result<(), PlatformError> {
        let mut packet = [0u8; LONG_LEN];
        packet[0] = LONG_ID;
        packet[1] = BT_DEV_IDX;
        packet[2] = feature_index;
        packet[3] = ((function & 0x0F) << 4) | (MY_SW & 0x0F);
        for (offset, byte) in params.iter().copied().enumerate() {
            if 4 + offset < LONG_LEN {
                packet[4 + offset] = byte;
            }
        }
        device.write_packet(&packet)?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn parse_message(raw: &[u8]) -> Option<(u8, u8, u8, Vec<u8>)> {
        if raw.len() < 4 {
            return None;
        }

        let offset = usize::from(matches!(raw.first(), Some(0x10) | Some(0x11)));
        if raw.len() < offset + 4 {
            return None;
        }

        let feature = raw[offset + 1];
        let function_and_sw = raw[offset + 2];
        let function = (function_and_sw >> 4) & 0x0F;
        let sw = function_and_sw & 0x0F;
        let params = raw[offset + 3..].to_vec();

        Some((feature, function, sw, params))
    }

    #[cfg(target_os = "macos")]
    fn map_hid_error(error: hidapi::HidError) -> PlatformError {
        PlatformError::Message(error.to_string())
    }
}

pub mod windows {
    pub use crate::windows_backend::{
        WindowsAppFocusBackend, WindowsHidBackend, WindowsHookBackend,
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
    fn load_or_recover_migrates_legacy_device_settings() {
        let path = unique_test_path("legacy-device-settings");
        let legacy = json!({
            "version": 2,
            "activeProfileId": "default",
            "profiles": [{
                "id": "default",
                "label": "Default",
                "appMatchers": [],
                "bindings": [],
            }],
            "managedDevices": [{
                "id": "mx_master_3s",
                "modelKey": "mx_master_3s",
                "displayName": "MX Master 3S",
                "nickname": null,
                "createdAtMs": 1,
                "lastSeenAtMs": 1,
                "lastSeenTransport": "Bluetooth Low Energy",
            }],
            "settings": {
                "startMinimized": true,
                "startAtLogin": false,
                "appearanceMode": "system",
                "debugMode": true,
                "dpi": 1600,
                "invertHorizontalScroll": true,
                "invertVerticalScroll": true,
                "gestureThreshold": 75,
                "gestureDeadzone": 55,
                "gestureTimeoutMs": 2500,
                "gestureCooldownMs": 250,
                "deviceLayoutOverrides": {
                    "mx_master_3s": "mx_master"
                }
            }
        });
        fs::write(&path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
        let store = JsonConfigStore::new(path.clone());

        let (config, warning) = store.load_or_recover();

        assert!(warning.is_none());
        assert_eq!(config.version, 3);
        assert_eq!(config.device_defaults.dpi, 1600);
        assert!(config.device_defaults.invert_horizontal_scroll);
        assert!(config.device_defaults.invert_vertical_scroll);
        assert!(!config.device_defaults.macos_thumb_wheel_simulate_trackpad);
        assert_eq!(
            config
                .device_defaults
                .macos_thumb_wheel_trackpad_hold_timeout_ms,
            500
        );
        assert_eq!(config.device_defaults.gesture_threshold, 75);
        assert_eq!(config.managed_devices.len(), 1);
        let managed = &config.managed_devices[0];
        assert_eq!(managed.settings.dpi, 1600);
        assert!(managed.settings.invert_horizontal_scroll);
        assert!(!managed.settings.macos_thumb_wheel_simulate_trackpad);
        assert_eq!(
            managed.settings.macos_thumb_wheel_trackpad_hold_timeout_ms,
            500
        );
        assert_eq!(
            managed.settings.manual_layout_override.as_deref(),
            Some("mx_master")
        );

        let _ = fs::remove_file(path);
    }
}
