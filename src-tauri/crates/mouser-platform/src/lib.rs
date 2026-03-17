use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use mouser_core::{
    build_connected_device_info, clamp_dpi, default_config, default_known_apps, default_layouts,
    known_device_specs, AppConfig, DeviceInfo, DeviceLayout, KnownApp, LogicalControl,
};
use thiserror::Error;

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
                    key: spec.key,
                    display_name: spec.display_name.clone(),
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
        serde_json::from_str::<AppConfig>(&raw).map_err(|error| {
            PlatformError::Message(format!("failed to decode {}: {error}", self.path.display()))
        })
    }

    fn save(&self, config: &AppConfig) -> Result<(), PlatformError> {
        self.ensure_parent()?;
        let json = serde_json::to_string_pretty(config)
            .map_err(|error| PlatformError::Message(error.to_string()))?;
        fs::write(&self.path, json).map_err(|error| PlatformError::Io {
            path: self.path.display().to_string(),
            message: error.to_string(),
        })
    }
}

pub mod macos {
    use super::*;

    #[cfg(target_os = "macos")]
    use hidapi::{BusType, DeviceInfo as HidDeviceInfo, HidApi, HidDevice};
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

    pub struct MacOsHookBackend;
    pub struct MacOsHidBackend;
    pub struct MacOsAppFocusBackend;

    impl HookBackend for MacOsHookBackend {
        fn backend_id(&self) -> &'static str {
            "macos-eventtap-stub"
        }

        fn capabilities(&self) -> HookCapabilities {
            HookCapabilities {
                can_intercept_buttons: false,
                can_intercept_scroll: false,
                supports_gesture_diversion: false,
            }
        }
    }

    impl HidBackend for MacOsHidBackend {
        fn backend_id(&self) -> &'static str {
            if cfg!(target_os = "macos") {
                "macos-hidapi"
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
                let api = HidApi::new().map_err(map_hid_error)?;
                let mut devices = Vec::new();
                for info in vendor_hid_infos(&api) {
                    if let Ok(device) = info.open_device(&api) {
                        if let Some(device_info) = probe_device(&device, info) {
                            if devices
                                .iter()
                                .all(|existing: &DeviceInfo| existing.key != device_info.key)
                            {
                                devices.push(device_info);
                            }
                        }
                    }
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
                let api = HidApi::new().map_err(map_hid_error)?;
                for info in vendor_hid_infos(&api) {
                    let Ok(device) = info.open_device(&api) else {
                        continue;
                    };
                    let candidate = build_connected_device_info(
                        Some(info.product_id()),
                        info.product_string(),
                        Some(transport_label(info.bus_type())),
                        Some("hidapi"),
                        None,
                        dpi,
                    );
                    if candidate.key == device_key && set_device_dpi(&device, dpi)? {
                        return Ok(());
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
    fn probe_device(device: &HidDevice, info: &HidDeviceInfo) -> Option<DeviceInfo> {
        let current_dpi = read_current_dpi(device).ok().flatten().unwrap_or(1000);
        let battery_level = read_battery(device).ok().flatten();
        Some(build_connected_device_info(
            Some(info.product_id()),
            info.product_string(),
            Some(transport_label(info.bus_type())),
            Some("hidapi"),
            battery_level,
            current_dpi,
        ))
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
    fn set_device_dpi(device: &HidDevice, dpi: u16) -> Result<bool, PlatformError> {
        let Some(feature_index) = find_feature(device, FEAT_ADJ_DPI)? else {
            return Ok(false);
        };

        let hi = ((dpi >> 8) & 0xFF) as u8;
        let lo = (dpi & 0xFF) as u8;
        Ok(request(device, feature_index, 1, &[0, hi, lo])?.is_some())
    }

    #[cfg(target_os = "macos")]
    fn read_current_dpi(device: &HidDevice) -> Result<Option<u16>, PlatformError> {
        let Some(feature_index) = find_feature(device, FEAT_ADJ_DPI)? else {
            return Ok(None);
        };

        let Some(response) = request(device, feature_index, 0, &[0])? else {
            return Ok(None);
        };
        if response.len() < 2 {
            return Ok(None);
        }
        Ok(Some(u16::from(response[0]) << 8 | u16::from(response[1])))
    }

    #[cfg(target_os = "macos")]
    fn read_battery(device: &HidDevice) -> Result<Option<u8>, PlatformError> {
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
    fn find_feature(device: &HidDevice, feature_id: u16) -> Result<Option<u8>, PlatformError> {
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
    fn request(
        device: &HidDevice,
        feature_index: u8,
        function: u8,
        params: &[u8],
    ) -> Result<Option<Vec<u8>>, PlatformError> {
        write_request(device, feature_index, function, params)?;
        let deadline = Instant::now() + Duration::from_millis(1_500);
        let expected_reply_functions = [function, (function + 1) & 0x0F];
        let mut buffer = [0u8; 64];

        while Instant::now() < deadline {
            let size = device
                .read_timeout(&mut buffer, 200)
                .map_err(map_hid_error)?;
            if size == 0 {
                continue;
            }

            let Some((response_feature, response_function, response_sw, response_params)) =
                parse_message(&buffer[..size])
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
    fn write_request(
        device: &HidDevice,
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
        device.write(&packet).map_err(map_hid_error)?;
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
    use super::*;

    pub struct WindowsHookBackend;
    pub struct WindowsHidBackend;
    pub struct WindowsAppFocusBackend;

    impl HookBackend for WindowsHookBackend {
        fn backend_id(&self) -> &'static str {
            "windows-stub"
        }

        fn capabilities(&self) -> HookCapabilities {
            HookCapabilities {
                can_intercept_buttons: false,
                can_intercept_scroll: false,
                supports_gesture_diversion: false,
            }
        }
    }

    impl HidBackend for WindowsHidBackend {
        fn backend_id(&self) -> &'static str {
            "windows-stub"
        }

        fn capabilities(&self) -> HidCapabilities {
            HidCapabilities {
                can_enumerate_devices: false,
                can_read_battery: false,
                can_read_dpi: false,
                can_write_dpi: false,
            }
        }

        fn list_devices(&self) -> Result<Vec<DeviceInfo>, PlatformError> {
            Err(PlatformError::Unsupported(
                "live Windows HID integration is not implemented yet",
            ))
        }

        fn set_device_dpi(&self, _device_key: &str, _dpi: u16) -> Result<(), PlatformError> {
            Err(PlatformError::Unsupported(
                "live Windows HID integration is not implemented yet",
            ))
        }
    }

    impl AppFocusBackend for WindowsAppFocusBackend {
        fn backend_id(&self) -> &'static str {
            "windows-stub"
        }

        fn current_frontmost_app(&self) -> Result<Option<String>, PlatformError> {
            Err(PlatformError::Unsupported(
                "live Windows frontmost app detection is not implemented yet",
            ))
        }
    }
}
