use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use mouser_core::{
    build_managed_device_info, clamp_dpi, default_action_catalog, default_config,
    default_app_discovery_snapshot, default_device_catalog, default_known_apps, default_layouts,
    effective_layout_key, manual_layout_choices, AppConfig, AppIdentity, BootstrapPayload,
    DebugEvent, DebugEventKind, DeviceFingerprint, DeviceInfo, DeviceLayout, DeviceSettings,
    EngineSnapshot, EngineStatus, KnownApp, ManagedDevice, PlatformCapabilities, Profile,
};
use mouser_platform::{ConfigStore, DeviceCatalog, PlatformError};

#[derive(Clone, Default)]
pub struct MockCatalog {
    layouts: Vec<DeviceLayout>,
    devices: Vec<DeviceInfo>,
}

impl MockCatalog {
    pub fn new() -> Self {
        let devices = default_device_catalog()
            .into_iter()
            .enumerate()
            .map(|(index, mut device)| {
                let identity_key = format!("mock:{}:{}", device.model_key, index + 1);
                device.key = identity_key.clone();
                device.fingerprint = DeviceFingerprint {
                    identity_key: Some(identity_key),
                    ..DeviceFingerprint::default()
                };
                device
            })
            .collect();
        Self {
            layouts: default_layouts(),
            devices,
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

    fn known_apps(&self) -> Vec<KnownApp> {
        default_known_apps()
    }

    fn clamp_dpi(&self, device_key: Option<&str>, value: u16) -> u16 {
        let device = device_key.and_then(|device_key| {
            self.devices
                .iter()
                .find(|candidate| candidate.model_key == device_key || candidate.key == device_key)
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
    resolved_profile_id: String,
    detected_devices: Vec<DeviceInfo>,
    selected_device_key: Option<String>,
    frontmost_app: Option<AppIdentity>,
    enabled: bool,
    debug_log: VecDeque<DebugEvent>,
}

impl Default for MockRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl MockRuntime {
    pub fn new() -> Self {
        let catalog = MockCatalog::new();
        let mut config = default_config();
        config.managed_devices = catalog
            .all_devices()
            .into_iter()
            .map(|device| ManagedDevice {
                id: device.model_key.clone(),
                model_key: device.model_key,
                display_name: device.display_name,
                nickname: None,
                profile_id: None,
                identity_key: device.fingerprint.identity_key.clone(),
                settings: config.device_defaults.clone(),
                created_at_ms: now_ms(),
                last_seen_at_ms: None,
                last_seen_transport: device.transport,
            })
            .collect();
        let selected_device_key = config
            .managed_devices
            .first()
            .map(|device| device.id.clone());
        let config_store = MemoryConfigStore::new(config);
        let mut runtime = Self {
            catalog,
            config_store,
            resolved_profile_id: "default".to_string(),
            detected_devices: Vec::new(),
            selected_device_key,
            frontmost_app: Some(AppIdentity {
                label: Some("Finder".to_string()),
                executable: Some("Finder".to_string()),
                ..AppIdentity::default()
            }),
            enabled: true,
            debug_log: VecDeque::new(),
        };
        runtime.apply_device_selection();
        runtime.sync_active_profile();
        runtime.push_debug(DebugEventKind::Info, "Mock runtime ready");
        runtime
    }

    pub fn bootstrap_payload(&self) -> BootstrapPayload {
        BootstrapPayload {
            config: self.config(),
            available_actions: default_action_catalog(),
            known_apps: default_known_apps(),
            app_discovery: default_app_discovery_snapshot(),
            supported_devices: mouser_core::known_device_specs(),
            layouts: self.catalog.all_layouts(),
            engine_snapshot: self.engine_snapshot(),
            platform_capabilities: current_platform_capabilities(),
            manual_layout_choices: manual_layout_choices(&self.catalog.all_layouts()),
        }
    }

    pub fn config(&self) -> AppConfig {
        self.config_store.load().unwrap()
    }

    pub fn save_config(&mut self, config: AppConfig) {
        let profile_changed = self.apply_config(config);
        self.push_debug(
            DebugEventKind::Info,
            format!(
                "Saved config for profile `{}` at {} DPI",
                self.resolved_profile_id,
                self.selected_device_settings().dpi
            ),
        );
        if profile_changed {
            self.push_debug(
                DebugEventKind::Info,
                format!(
                    "Resolved profile for app {:?} -> {}",
                    self.frontmost_app, self.resolved_profile_id
                ),
            );
        }
    }

    pub fn create_profile(&mut self, profile: Profile) {
        let mut config = self.config();
        config.upsert_profile(profile);
        let selected_profile_id = self
            .selected_managed_device()
            .and_then(|device| device.profile_id);
        config.sync_active_profile(
            selected_profile_id.as_deref(),
            self.frontmost_app.as_ref(),
        );
        self.save_config(config);
    }

    pub fn update_profile(&mut self, profile: Profile) {
        let mut config = self.config();
        config.upsert_profile(profile);
        let selected_profile_id = self
            .selected_managed_device()
            .and_then(|device| device.profile_id);
        config.sync_active_profile(
            selected_profile_id.as_deref(),
            self.frontmost_app.as_ref(),
        );
        self.save_config(config);
    }

    pub fn delete_profile(&mut self, profile_id: &str) {
        let mut config = self.config();
        config.delete_profile(profile_id);
        let selected_profile_id = self
            .selected_managed_device()
            .and_then(|device| device.profile_id);
        config.sync_active_profile(
            selected_profile_id.as_deref(),
            self.frontmost_app.as_ref(),
        );
        self.save_config(config);
    }

    pub fn select_device(&mut self, device_key: &str) {
        self.selected_device_key = Some(device_key.to_string());
        self.apply_device_selection();
        self.sync_active_profile();
        self.push_debug(
            DebugEventKind::Info,
            format!("Switched mock device to `{device_key}`"),
        );
    }

    pub fn set_frontmost_app(&mut self, frontmost_app: Option<String>) {
        self.frontmost_app = frontmost_app.as_deref().map(|value| AppIdentity {
            label: Some(value.to_string()),
            executable: Some(value.to_string()),
            ..AppIdentity::default()
        });
        if self.sync_active_profile() {
            self.push_debug(
                DebugEventKind::Info,
                format!(
                    "Mock app focus changed to {:?}; active profile is `{}`",
                    frontmost_app, self.resolved_profile_id
                ),
            );
        }
    }

    pub fn apply_imported_config(&mut self, config: AppConfig) {
        self.apply_config(config);
        self.push_debug(DebugEventKind::Info, "Imported legacy Mouser config");
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.managed_device_infos()
    }

    pub fn engine_snapshot(&self) -> EngineSnapshot {
        let devices = self.managed_device_infos();
        let active_device = self
            .selected_device_key
            .as_ref()
            .and_then(|device_key| devices.iter().find(|device| &device.key == device_key))
            .cloned()
            .map(|mut device| {
                let manual_layout_override = self
                    .selected_managed_device()
                    .and_then(|managed| managed.settings.manual_layout_override);
                let layout_key = effective_layout_key(
                    manual_layout_override.as_deref(),
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
            devices,
            detected_devices: self.detected_devices.clone(),
            active_device_key: self.selected_device_key.clone(),
            active_device: active_device.clone(),
            engine_status: EngineStatus {
                enabled: self.enabled,
                connected: active_device
                    .as_ref()
                    .is_some_and(|device| device.connected),
                active_profile_id: self.resolved_profile_id.clone(),
                frontmost_app: self
                    .frontmost_app
                    .as_ref()
                    .and_then(AppIdentity::label_or_fallback),
                selected_device_key: self.selected_device_key.clone(),
                debug_mode: self.config().settings.debug_mode,
                debug_log: self.debug_log.iter().cloned().collect(),
            },
        }
    }

    pub fn last_debug_event(&self) -> Option<DebugEvent> {
        self.debug_log.front().cloned()
    }

    fn apply_device_selection(&mut self) {
        let Some(selected_key) = self.selected_device_key.clone() else {
            self.detected_devices.clear();
            return;
        };

        self.detected_devices = self
            .catalog
            .all_devices()
            .into_iter()
            .filter(|device| device.model_key == selected_key || device.key == selected_key)
            .map(|mut device| {
                device.connected = true;
                device.current_dpi = clamp_dpi(Some(&device), self.selected_device_settings().dpi);
                device
            })
            .collect();
    }

    fn managed_device_infos(&self) -> Vec<DeviceInfo> {
        let selected_live = self.detected_devices.first();
        self.config()
            .managed_devices
            .into_iter()
            .map(|device| {
                let live = if selected_live
                    .as_ref()
                    .is_some_and(|live| live.model_key == device.model_key)
                {
                    selected_live
                } else {
                    None
                };
                build_managed_device_info(&device, live)
            })
            .collect()
    }

    fn sync_active_profile(&mut self) -> bool {
        let selected_profile_id = self
            .selected_managed_device()
            .and_then(|device| device.profile_id);
        let config = self.config();
        let next_profile_id = config.resolved_profile_id(
            selected_profile_id.as_deref(),
            self.frontmost_app.as_ref(),
        );
        if self.resolved_profile_id != next_profile_id {
            self.resolved_profile_id = next_profile_id;
            return true;
        }
        false
    }

    fn apply_config(&mut self, mut config: AppConfig) -> bool {
        config.ensure_invariants();
        self.config_store.save(&config).unwrap();
        self.apply_device_selection();
        self.sync_active_profile()
    }

    fn selected_managed_device(&self) -> Option<ManagedDevice> {
        let selected_key = self.selected_device_key.as_deref()?;
        self.config()
            .managed_devices
            .into_iter()
            .find(|device| device.id == selected_key)
    }

    fn selected_device_settings(&self) -> DeviceSettings {
        self.selected_managed_device()
            .map(|device| device.settings)
            .unwrap_or_else(|| self.config().device_defaults)
    }

    fn push_debug(&mut self, kind: DebugEventKind, message: impl Into<String>) {
        self.debug_log.push_front(DebugEvent {
            kind,
            message: message.into(),
            timestamp_ms: now_ms(),
        });
        while self.debug_log.len() > 24 {
            let _ = self.debug_log.pop_back();
        }
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
        mapping_engine_ready: false,
        gesture_diversion_available: false,
        active_hid_backend: "mock-hid".to_string(),
        active_hook_backend: "mock-hook".to_string(),
        active_focus_backend: "mock-focus".to_string(),
        hidapi_available: cfg!(any(target_os = "macos", target_os = "windows")),
        iokit_available: cfg!(target_os = "macos"),
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
        assert_eq!(
            runtime.engine_snapshot().engine_status.active_profile_id,
            "code"
        );
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

    #[test]
    fn imported_config_clamps_selected_device_dpi() {
        let mut runtime = MockRuntime::new();
        let mut config = runtime.config();
        config
            .managed_devices
            .iter_mut()
            .find(|device| device.id == "mx_master_3s")
            .unwrap()
            .settings
            .dpi = 20_000;

        runtime.apply_imported_config(config);

        assert_eq!(
            runtime
                .config()
                .managed_devices
                .into_iter()
                .find(|device| device.id == "mx_master_3s")
                .unwrap()
                .settings
                .dpi,
            8000
        );
        assert_eq!(
            runtime.engine_snapshot().active_device.unwrap().current_dpi,
            8000
        );
    }
}
