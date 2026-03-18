use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use mouser_core::{
    clamp_dpi, default_action_catalog, default_known_apps, effective_layout_key, layout_by_key,
    manual_layout_choices, AppConfig, BootstrapPayload, DebugEvent, DebugEventKind, DeviceInfo,
    EngineSnapshot, EngineStatus, PlatformCapabilities, Profile,
};
use mouser_platform::{
    macos::{MacOsAppFocusBackend, MacOsHidBackend, MacOsHookBackend},
    windows::{WindowsAppFocusBackend, WindowsHidBackend, WindowsHookBackend},
    AppFocusBackend, ConfigStore, DeviceCatalog, HidBackend, HookBackend, JsonConfigStore,
    StaticDeviceCatalog,
};

pub struct AppRuntime {
    catalog: StaticDeviceCatalog,
    config_store: JsonConfigStore,
    hid_backend: Box<dyn HidBackend>,
    hook_backend: Box<dyn HookBackend>,
    app_focus_backend: Box<dyn AppFocusBackend>,
    config: AppConfig,
    devices: Vec<DeviceInfo>,
    selected_device_key: Option<String>,
    frontmost_app: Option<String>,
    enabled: bool,
    debug_log: Vec<DebugEvent>,
}

impl AppRuntime {
    pub fn new(config_path: Option<PathBuf>) -> Self {
        let catalog = StaticDeviceCatalog::new();
        let config_store =
            JsonConfigStore::new(config_path.unwrap_or_else(JsonConfigStore::default_path));
        let (mut config, load_warning) = config_store.load_or_recover();
        config.ensure_invariants();

        let mut runtime = Self {
            catalog,
            config_store,
            hid_backend: current_hid_backend(),
            hook_backend: current_hook_backend(),
            app_focus_backend: current_app_focus_backend(),
            config,
            devices: Vec::new(),
            selected_device_key: None,
            frontmost_app: None,
            enabled: true,
            debug_log: Vec::new(),
        };

        if let Some(load_warning) = load_warning {
            runtime.push_debug(DebugEventKind::Warning, load_warning);
        }
        runtime.refresh_live_state();
        runtime.sync_hook_backend();
        runtime.push_debug(
            DebugEventKind::Info,
            format!(
                "Runtime ready (hid={}, hook={}, focus={})",
                runtime.hid_backend.backend_id(),
                runtime.hook_backend.backend_id(),
                runtime.app_focus_backend.backend_id()
            ),
        );
        if runtime.config.settings.debug_mode {
            runtime.log_debug_session_state();
        }
        runtime
    }

    pub fn bootstrap_payload(&self) -> BootstrapPayload {
        BootstrapPayload {
            config: self.config.clone(),
            available_actions: default_action_catalog(),
            known_apps: default_known_apps(),
            layouts: self.catalog.all_layouts(),
            engine_snapshot: self.engine_snapshot(),
            platform_capabilities: self.platform_capabilities(),
            manual_layout_choices: manual_layout_choices(&self.catalog.all_layouts()),
        }
    }

    pub fn config(&self) -> AppConfig {
        self.config.clone()
    }

    pub fn save_config(&mut self, config: AppConfig) {
        let debug_mode_was_enabled = self.apply_config(config);
        self.push_debug(
            DebugEventKind::Info,
            format!(
                "Saved config for `{}` at {} DPI",
                self.config.active_profile_id, self.config.settings.dpi
            ),
        );

        if !debug_mode_was_enabled && self.config.settings.debug_mode {
            self.log_debug_session_state();
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }

        self.enabled = enabled;
        self.sync_hook_backend();
        self.push_debug(
            DebugEventKind::Info,
            if enabled {
                "Remapping enabled"
            } else {
                "Remapping disabled"
            },
        );
    }

    pub fn set_debug_mode(&mut self, enabled: bool) {
        if self.config.settings.debug_mode == enabled {
            return;
        }

        self.config.settings.debug_mode = enabled;
        self.persist_config();
        self.sync_hook_backend();

        if enabled {
            self.log_debug_session_state();
        } else {
            self.push_debug(DebugEventKind::Info, "Debug mode disabled");
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn debug_mode(&self) -> bool {
        self.config.settings.debug_mode
    }

    pub fn create_profile(&mut self, profile: Profile) {
        self.config.upsert_profile(profile);
        self.persist_config();
        self.sync_hook_backend();
        self.push_debug(DebugEventKind::Info, "Created profile");
        self.log_active_profile_snapshot("Bindings snapshot");
    }

    pub fn update_profile(&mut self, profile: Profile) {
        self.config.upsert_profile(profile);
        self.persist_config();
        self.sync_hook_backend();
        self.push_debug(DebugEventKind::Info, "Updated profile");
        self.log_active_profile_snapshot("Bindings snapshot");
    }

    pub fn delete_profile(&mut self, profile_id: &str) {
        if self.config.delete_profile(profile_id) {
            self.persist_config();
            self.sync_hook_backend();
            self.push_debug(
                DebugEventKind::Info,
                format!("Deleted profile `{profile_id}`"),
            );
        }
    }

    pub fn select_device(&mut self, device_key: &str) {
        self.selected_device_key = Some(device_key.to_string());
        if let Some(device) = self.active_device_raw() {
            self.config.settings.dpi = clamp_dpi(Some(device), device.current_dpi);
            self.persist_config();
        }
        self.push_debug(
            DebugEventKind::Info,
            format!("Selected device `{device_key}`"),
        );
        self.log_device_inventory("Device probe");
    }

    pub fn apply_imported_config(&mut self, config: AppConfig) {
        let debug_mode_was_enabled = self.apply_config(config);
        self.push_debug(DebugEventKind::Info, "Imported legacy Mouser config");
        self.log_active_profile_snapshot("Imported bindings");
        if !debug_mode_was_enabled && self.config.settings.debug_mode {
            self.log_debug_session_state();
        }
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.devices.clone()
    }

    pub fn last_debug_event(&self) -> Option<DebugEvent> {
        self.debug_log.first().cloned()
    }

    pub fn clear_debug_log(&mut self) {
        self.debug_log.clear();
    }

    pub fn poll(&mut self) -> bool {
        let before = self.engine_snapshot();
        let config_before = self.config.clone();
        self.refresh_live_state();
        self.collect_hook_events();
        before != self.engine_snapshot() || config_before != self.config
    }

    pub fn engine_snapshot(&self) -> EngineSnapshot {
        let active_device = self.active_device_raw().cloned().map(|mut device| {
            let layout_key =
                effective_layout_key(&self.config.settings, Some(&device.key), &device.ui_layout);
            device.ui_layout = layout_key.clone();
            if let Some(layout) = layout_by_key(&self.catalog.all_layouts(), &layout_key) {
                device.image_asset = layout.image_asset;
            }
            device
        });

        EngineSnapshot {
            devices: self.devices.clone(),
            active_device_key: self.selected_device_key.clone(),
            active_device,
            engine_status: EngineStatus {
                enabled: self.enabled,
                connected: self.active_device_raw().is_some(),
                active_profile_id: self.config.active_profile_id.clone(),
                frontmost_app: self.frontmost_app.clone(),
                selected_device_key: self.selected_device_key.clone(),
                debug_mode: self.config.settings.debug_mode,
                debug_log: self.debug_log.clone(),
            },
        }
    }

    fn persist_config(&mut self) {
        if let Err(error) = self.config_store.save(&self.config) {
            self.push_debug(
                DebugEventKind::Warning,
                format!("Failed to save config: {error}"),
            );
        }
    }

    fn apply_config(&mut self, mut config: AppConfig) -> bool {
        let debug_mode_was_enabled = self.config.settings.debug_mode;
        config.ensure_invariants();
        let selected_device_key = self.selected_device_key.as_deref();
        config.settings.dpi = self
            .catalog
            .clamp_dpi(selected_device_key, config.settings.dpi);
        self.config = config;

        if let Some(device_key) = selected_device_key {
            if let Err(error) = self
                .hid_backend
                .set_device_dpi(device_key, self.config.settings.dpi)
            {
                self.push_debug(
                    DebugEventKind::Warning,
                    format!("Failed to apply DPI to {device_key}: {error}"),
                );
            } else if let Some(device) = self
                .devices
                .iter_mut()
                .find(|device| device.key == device_key)
            {
                device.current_dpi = self.config.settings.dpi;
            }
        }

        self.persist_config();
        self.sync_active_profile();
        self.sync_hook_backend();
        debug_mode_was_enabled
    }

    fn refresh_live_state(&mut self) {
        let previous_frontmost_app = self.frontmost_app.clone();

        match self.hid_backend.list_devices() {
            Ok(devices) => self.replace_devices(devices),
            Err(error) => self.push_debug(
                DebugEventKind::Warning,
                format!("Live HID refresh failed: {error}"),
            ),
        }

        match self.app_focus_backend.current_frontmost_app() {
            Ok(frontmost_app) => {
                self.frontmost_app = frontmost_app;
                if self.frontmost_app != previous_frontmost_app {
                    self.push_debug_if_enabled(
                        DebugEventKind::Info,
                        format!(
                            "Frontmost app -> {}",
                            self.frontmost_app.as_deref().unwrap_or("unknown")
                        ),
                    );
                }
                self.sync_active_profile();
            }
            Err(error) => self.push_debug(
                DebugEventKind::Warning,
                format!("Frontmost-app refresh failed: {error}"),
            ),
        }
    }

    fn replace_devices(&mut self, devices: Vec<DeviceInfo>) {
        let previous_summary = self.describe_devices();
        self.devices = devices;

        if self.devices.is_empty() {
            self.selected_device_key = None;
            self.push_debug_if_enabled(DebugEventKind::Info, "No supported Logitech HID devices");
            return;
        }

        let keep_current = self
            .selected_device_key
            .as_ref()
            .is_some_and(|selected_device_key| {
                self.devices
                    .iter()
                    .any(|device| device.key == *selected_device_key)
            });

        if !keep_current {
            self.selected_device_key = self.devices.first().map(|device| device.key.clone());
        }

        if let Some(device) = self.active_device_raw() {
            let device_dpi = clamp_dpi(Some(device), device.current_dpi);
            if self.config.settings.dpi != device_dpi {
                self.config.settings.dpi = device_dpi;
                self.persist_config();
            }
        }

        if self.describe_devices() != previous_summary {
            self.log_device_inventory("Device probe");
        }
    }

    fn sync_active_profile(&mut self) {
        if self
            .config
            .sync_active_profile_for_app(self.frontmost_app.as_deref())
        {
            self.persist_config();
            self.push_debug(
                DebugEventKind::Info,
                format!(
                    "Resolved profile for app {:?} -> {}",
                    self.frontmost_app, self.config.active_profile_id
                ),
            );
            self.log_active_profile_snapshot("Active bindings");
            self.sync_hook_backend();
        }
    }

    fn sync_hook_backend(&mut self) {
        let Some(profile) = self.config.active_profile().cloned() else {
            return;
        };

        if let Err(error) =
            self.hook_backend
                .configure(&self.config.settings, &profile, self.enabled)
        {
            self.push_debug(
                DebugEventKind::Warning,
                format!("Failed to configure hook backend: {error}"),
            );
        }

        self.collect_hook_events();
    }

    fn collect_hook_events(&mut self) {
        for event in self.hook_backend.drain_events() {
            self.push_debug(event.kind, event.message);
        }
    }

    fn platform_capabilities(&self) -> PlatformCapabilities {
        let hid_capabilities = self.hid_backend.capabilities();
        let hook_capabilities = self.hook_backend.capabilities();
        PlatformCapabilities {
            platform: if cfg!(target_os = "macos") {
                "macos".to_string()
            } else if cfg!(target_os = "windows") {
                "windows".to_string()
            } else {
                "other".to_string()
            },
            windows_supported: true,
            macos_supported: true,
            live_hooks_available: hook_capabilities.can_intercept_buttons,
            live_hid_available: hid_capabilities.can_enumerate_devices,
            tray_ready: true,
            mapping_engine_ready: hook_capabilities.can_intercept_buttons,
            gesture_diversion_available: hook_capabilities.supports_gesture_diversion,
            active_hid_backend: self.hid_backend.backend_id().to_string(),
            active_hook_backend: self.hook_backend.backend_id().to_string(),
            active_focus_backend: self.app_focus_backend.backend_id().to_string(),
            hidapi_available: cfg!(target_os = "macos"),
            iokit_available: cfg!(target_os = "macos"),
        }
    }

    fn push_debug(&mut self, kind: DebugEventKind, message: impl Into<String>) {
        self.debug_log.insert(
            0,
            DebugEvent {
                kind,
                message: message.into(),
                timestamp_ms: now_ms(),
            },
        );
        self.debug_log.truncate(48);
    }

    fn push_debug_if_enabled(&mut self, kind: DebugEventKind, message: impl Into<String>) {
        if self.config.settings.debug_mode {
            self.push_debug(kind, message);
        }
    }

    fn log_debug_session_state(&mut self) {
        self.push_debug(DebugEventKind::Info, "Debug mode enabled");
        self.push_debug(
            DebugEventKind::Info,
            format!(
                "Backends: hid={} hook={} focus={}",
                self.hid_backend.backend_id(),
                self.hook_backend.backend_id(),
                self.app_focus_backend.backend_id(),
            ),
        );
        self.push_debug(
            DebugEventKind::Info,
            format!(
                "Transport support: hidapi={} iokit={}",
                if cfg!(target_os = "macos") {
                    "ready"
                } else {
                    "unavailable"
                },
                if cfg!(target_os = "macos") {
                    "ready"
                } else {
                    "unavailable"
                },
            ),
        );
        if !self.hook_backend.capabilities().can_intercept_buttons {
            self.push_debug(
                DebugEventKind::Warning,
                "Live remapping is not implemented in Rust yet; mapping edits currently update config only.",
            );
        }
        self.log_device_inventory("Device probe");
        self.log_active_profile_snapshot("Active bindings");
    }

    fn log_device_inventory(&mut self, prefix: &str) {
        self.push_debug_if_enabled(
            DebugEventKind::Info,
            format!("{prefix}: {}", self.describe_devices()),
        );
    }

    fn log_active_profile_snapshot(&mut self, prefix: &str) {
        self.push_debug_if_enabled(
            DebugEventKind::Info,
            format!("{prefix}: {}", self.describe_active_profile()),
        );
    }

    fn describe_active_profile(&self) -> String {
        let Some(profile) = self.config.active_profile() else {
            return "no active profile".to_string();
        };

        let controls = [
            mouser_core::LogicalControl::Back,
            mouser_core::LogicalControl::Forward,
            mouser_core::LogicalControl::GesturePress,
            mouser_core::LogicalControl::GestureLeft,
            mouser_core::LogicalControl::GestureRight,
            mouser_core::LogicalControl::GestureUp,
            mouser_core::LogicalControl::GestureDown,
            mouser_core::LogicalControl::HscrollLeft,
            mouser_core::LogicalControl::HscrollRight,
        ];

        let bindings = controls
            .into_iter()
            .map(|control| {
                let action_id = profile
                    .binding_for(control)
                    .map(|binding| binding.action_id.as_str())
                    .unwrap_or("none");
                format!("{}={action_id}", control.label())
            })
            .collect::<Vec<_>>()
            .join(", ");

        format!("{} [{}]", profile.id, bindings)
    }

    fn describe_devices(&self) -> String {
        if self.devices.is_empty() {
            return "no devices".to_string();
        }

        self.devices
            .iter()
            .map(|device| {
                format!(
                    "{} (pid={}, transport={}, source={}, dpi={}, battery={})",
                    device.display_name,
                    device
                        .product_id
                        .map(|product_id| format!("0x{product_id:04x}"))
                        .unwrap_or_else(|| "n/a".to_string()),
                    device.transport.as_deref().unwrap_or("unknown"),
                    device.source.as_deref().unwrap_or("unknown"),
                    device.current_dpi,
                    device
                        .battery_level
                        .map(|level| format!("{level}%"))
                        .unwrap_or_else(|| "n/a".to_string()),
                )
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn active_device_raw(&self) -> Option<&DeviceInfo> {
        self.selected_device_key
            .as_ref()
            .and_then(|device_key| self.devices.iter().find(|device| &device.key == device_key))
    }
}

fn current_hid_backend() -> Box<dyn HidBackend> {
    if cfg!(target_os = "macos") {
        Box::new(MacOsHidBackend)
    } else {
        Box::new(WindowsHidBackend)
    }
}

fn current_hook_backend() -> Box<dyn HookBackend> {
    if cfg!(target_os = "macos") {
        Box::new(MacOsHookBackend::new())
    } else {
        Box::new(WindowsHookBackend::new())
    }
}

fn current_app_focus_backend() -> Box<dyn AppFocusBackend> {
    if cfg!(target_os = "macos") {
        Box::new(MacOsAppFocusBackend)
    } else {
        Box::new(WindowsAppFocusBackend)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
