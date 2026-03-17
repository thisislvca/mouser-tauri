use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use mouser_core::{
    clamp_dpi, default_action_catalog, default_config, default_device_catalog, default_layouts,
    effective_layout_key, manual_layout_choices, AppConfig, BootstrapPayload, DebugEvent,
    DebugEventKind, DeviceInfo, DeviceLayout, EngineSnapshot, EngineStatus, PlatformCapabilities,
    Profile,
};
use mouser_platform::{ConfigStore, DeviceCatalog, PlatformError};

#[derive(Clone, Default)]
pub struct MockCatalog {
    layouts: Vec<DeviceLayout>,
    devices: Vec<DeviceInfo>,
}

impl MockCatalog {
    pub fn new() -> Self {
        Self {
            layouts: default_layouts(),
            devices: default_device_catalog(),
        }
    }
}

impl DeviceCatalog for MockCatalog {
    fn all_devices(&self) -> Vec<DeviceInfo> {
        self.devices.clone()
    }

    fn all_layouts(&self) -> Vec<DeviceLayout> {
        self.layouts.clone()
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

#[derive(Clone)]
pub struct MemoryConfigStore {
    inner: Arc<Mutex<AppConfig>>,
}

impl MemoryConfigStore {
    pub fn new(config: AppConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(config)),
        }
    }
}

impl ConfigStore for MemoryConfigStore {
    fn load(&self) -> Result<AppConfig, PlatformError> {
        Ok(self.inner.lock().unwrap().clone())
    }

    fn save(&self, config: &AppConfig) -> Result<(), PlatformError> {
        *self.inner.lock().unwrap() = config.clone();
        Ok(())
    }
}

pub struct MockRuntime {
    catalog: MockCatalog,
    config_store: MemoryConfigStore,
    devices: Vec<DeviceInfo>,
    selected_device_key: Option<String>,
    frontmost_app: Option<String>,
    enabled: bool,
    debug_log: Vec<DebugEvent>,
}

impl Default for MockRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl MockRuntime {
    pub fn new() -> Self {
        let catalog = MockCatalog::new();
        let devices = catalog.all_devices();
        let selected_device_key = devices.first().map(|device| device.key.clone());
        let config_store = MemoryConfigStore::new(default_config());
        let mut runtime = Self {
            catalog,
            config_store,
            devices,
            selected_device_key,
            frontmost_app: Some("Finder".to_string()),
            enabled: true,
            debug_log: Vec::new(),
        };
        runtime.apply_device_selection();
        runtime.push_debug(DebugEventKind::Info, "Mock runtime ready");
        runtime
    }

    pub fn bootstrap_payload(&self) -> BootstrapPayload {
        BootstrapPayload {
            config: self.config(),
            available_actions: default_action_catalog(),
            layouts: self.catalog.all_layouts(),
            engine_snapshot: self.engine_snapshot(),
            platform_capabilities: current_platform_capabilities(),
            manual_layout_choices: manual_layout_choices(&self.catalog.all_layouts()),
        }
    }

    pub fn config(&self) -> AppConfig {
        self.config_store.load().unwrap()
    }

    pub fn save_config(&mut self, mut config: AppConfig) {
        config.ensure_invariants();
        let device_key = self.selected_device_key.as_deref();
        config.settings.dpi = self.catalog.clamp_dpi(device_key, config.settings.dpi);
        self.config_store.save(&config).unwrap();
        self.apply_device_selection();
        let profile_changed = self.sync_active_profile();
        self.push_debug(
            DebugEventKind::Info,
            format!(
                "Saved config for profile `{}` at {} DPI",
                self.config().active_profile_id,
                self.config().settings.dpi
            ),
        );
        if profile_changed {
            self.push_debug(
                DebugEventKind::Info,
                format!(
                    "Resolved profile for app {:?} -> {}",
                    self.frontmost_app,
                    self.config().active_profile_id
                ),
            );
        }
    }

    pub fn create_profile(&mut self, profile: Profile) {
        let mut config = self.config();
        config.upsert_profile(profile);
        self.save_config(config);
    }

    pub fn update_profile(&mut self, profile: Profile) {
        let mut config = self.config();
        config.upsert_profile(profile);
        self.save_config(config);
    }

    pub fn delete_profile(&mut self, profile_id: &str) {
        let mut config = self.config();
        config.delete_profile(profile_id);
        self.save_config(config);
    }

    pub fn select_device(&mut self, device_key: &str) {
        self.selected_device_key = Some(device_key.to_string());
        self.apply_device_selection();
        self.push_debug(
            DebugEventKind::Info,
            format!("Switched mock device to `{device_key}`"),
        );
    }

    pub fn set_frontmost_app(&mut self, frontmost_app: Option<String>) {
        self.frontmost_app = frontmost_app.clone();
        if self.sync_active_profile() {
            self.push_debug(
                DebugEventKind::Info,
                format!(
                    "Mock app focus changed to {:?}; active profile is `{}`",
                    frontmost_app,
                    self.config().active_profile_id
                ),
            );
        }
    }

    pub fn apply_imported_config(&mut self, mut config: AppConfig) {
        config.ensure_invariants();
        self.config_store.save(&config).unwrap();
        self.apply_device_selection();
        self.sync_active_profile();
        self.push_debug(DebugEventKind::Info, "Imported legacy Mouser config");
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.devices.clone()
    }

    pub fn engine_snapshot(&self) -> EngineSnapshot {
        let active_device = self
            .selected_device_key
            .as_ref()
            .and_then(|device_key| self.devices.iter().find(|device| &device.key == device_key))
            .cloned()
            .map(|mut device| {
                let layout_key = effective_layout_key(
                    &self.config().settings,
                    Some(&device.key),
                    &device.ui_layout,
                );
                device.ui_layout = layout_key.clone();
                if let Some(layout) = self
                    .catalog
                    .all_layouts()
                    .into_iter()
                    .find(|layout| layout.key == layout_key)
                {
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
                connected: self
                    .selected_device_key
                    .as_ref()
                    .and_then(|device_key| {
                        self.devices
                            .iter()
                            .find(|device| &device.key == device_key)
                            .map(|device| device.connected)
                    })
                    .unwrap_or(false),
                active_profile_id: self.config().active_profile_id,
                frontmost_app: self.frontmost_app.clone(),
                selected_device_key: self.selected_device_key.clone(),
                debug_mode: self.config().settings.debug_mode,
                debug_log: self.debug_log.clone(),
            },
        }
    }

    pub fn last_debug_event(&self) -> Option<DebugEvent> {
        self.debug_log.first().cloned()
    }

    fn apply_device_selection(&mut self) {
        let config = self.config();
        for device in &mut self.devices {
            let selected = self
                .selected_device_key
                .as_ref()
                .map(|selected_key| selected_key == &device.key)
                .unwrap_or(false);
            device.connected = selected;
            device.current_dpi = clamp_dpi(Some(device), config.settings.dpi);
        }
    }

    fn sync_active_profile(&mut self) -> bool {
        let mut config = self.config();
        let changed = config.sync_active_profile_for_app(self.frontmost_app.as_deref());
        if changed {
            self.config_store.save(&config).unwrap();
        }
        changed
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
        self.debug_log.truncate(24);
    }
}

fn current_platform_capabilities() -> PlatformCapabilities {
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
        live_hooks_available: false,
        live_hid_available: false,
        tray_ready: true,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use mouser_core::{default_profile_bindings, AppMatcher, AppMatcherKind, Profile};

    use super::*;

    #[test]
    fn selecting_mock_device_updates_snapshot() {
        let mut runtime = MockRuntime::new();
        runtime.select_device("mx_anywhere_3s");
        let snapshot = runtime.engine_snapshot();
        assert_eq!(
            snapshot.active_device_key.as_deref(),
            Some("mx_anywhere_3s")
        );
        assert!(snapshot.active_device.unwrap().connected);
    }

    #[test]
    fn mock_app_focus_changes_trigger_profile_switch() {
        let mut runtime = MockRuntime::new();
        runtime.create_profile(Profile {
            id: "code".to_string(),
            label: "VS Code".to_string(),
            app_matchers: vec![AppMatcher {
                kind: AppMatcherKind::Executable,
                value: "Code.exe".to_string(),
            }],
            bindings: default_profile_bindings(),
        });
        runtime.set_frontmost_app(Some("Code.exe".to_string()));
        assert_eq!(runtime.config().active_profile_id, "code");
    }

    #[test]
    fn debug_events_reach_engine_status_log() {
        let mut runtime = MockRuntime::new();
        let before = runtime.engine_snapshot().engine_status.debug_log.len();
        runtime.select_device("mystery_logitech_mouse");
        let after = runtime.engine_snapshot().engine_status.debug_log.len();
        assert!(after >= before);
        assert!(runtime.last_debug_event().is_some());
    }
}
