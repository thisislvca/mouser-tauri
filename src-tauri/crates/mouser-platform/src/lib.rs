use mouser_core::{AppConfig, DeviceInfo, DeviceLayout, LogicalControl};
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
    fn clamp_dpi(&self, device_key: Option<&str>, value: u16) -> u16;
}

pub trait ConfigStore: Send + Sync {
    fn load(&self) -> Result<AppConfig, PlatformError>;
    fn save(&self, config: &AppConfig) -> Result<(), PlatformError>;
}

pub mod macos {
    use super::*;

    pub struct MacOsHookBackend;
    pub struct MacOsHidBackend;
    pub struct MacOsAppFocusBackend;

    impl HookBackend for MacOsHookBackend {
        fn backend_id(&self) -> &'static str {
            "macos-stub"
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
            "macos-stub"
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
                "live macOS HID integration is not implemented yet",
            ))
        }

        fn set_device_dpi(&self, _device_key: &str, _dpi: u16) -> Result<(), PlatformError> {
            Err(PlatformError::Unsupported(
                "live macOS HID integration is not implemented yet",
            ))
        }
    }

    impl AppFocusBackend for MacOsAppFocusBackend {
        fn backend_id(&self) -> &'static str {
            "macos-stub"
        }

        fn current_frontmost_app(&self) -> Result<Option<String>, PlatformError> {
            Err(PlatformError::Unsupported(
                "live macOS frontmost app detection is not implemented yet",
            ))
        }
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
