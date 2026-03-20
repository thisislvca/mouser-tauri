use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use mouser_core::{
    active_device_with_layout, build_engine_snapshot, build_managed_device_info, clamp_dpi,
    default_action_catalog, default_app_catalog, default_app_discovery_snapshot,
    known_device_spec_by_key, known_device_specs, manual_layout_choices, normalize_app_match_value,
    AppConfig, AppDiscoverySnapshot, AppIdentity, AppMatcher, AppMatcherKind, BootstrapPayload,
    CatalogApp, DebugEvent, DebugEventKind, DeviceInfo, DeviceSettings, DiscoveredApp,
    EngineSnapshot, EngineSnapshotState, InstalledApp, ManagedDevice, PlatformCapabilities,
    Profile,
};
use mouser_platform::{
    linux::{LinuxAppDiscoveryBackend, LinuxAppFocusBackend, LinuxHidBackend, LinuxHookBackend},
    macos::{MacOsAppDiscoveryBackend, MacOsAppFocusBackend, MacOsHidBackend, MacOsHookBackend},
    windows::{
        WindowsAppDiscoveryBackend, WindowsAppFocusBackend, WindowsHidBackend, WindowsHookBackend,
    },
    AppDiscoveryBackend, AppFocusBackend, ConfigStore, HidBackend, HookBackend, HookBackendEvent,
    HookBackendSettings, JsonConfigStore, PlatformError, StaticDeviceCatalog,
};

pub struct AppRuntime {
    catalog: StaticDeviceCatalog,
    config_store: JsonConfigStore,
    hid_backend: Arc<dyn HidBackend>,
    hook_backend: Arc<dyn HookBackend>,
    app_focus_backend: Arc<dyn AppFocusBackend>,
    app_discovery_backend: Box<dyn AppDiscoveryBackend>,
    config: AppConfig,
    resolved_profile_id: String,
    detected_devices: Vec<DeviceInfo>,
    selected_device_key: Option<String>,
    frontmost_app: Option<AppIdentity>,
    app_discovery: AppDiscoverySnapshot,
    enabled: bool,
    debug_log: VecDeque<DebugEvent>,
}

struct DeviceResolution {
    assignments: BTreeMap<String, usize>,
    managed_devices: Vec<DeviceInfo>,
}

impl DeviceResolution {
    fn connected_ids(&self) -> BTreeSet<String> {
        self.assignments.keys().cloned().collect()
    }

    fn selected_live_device_index(&self, selected_device_key: Option<&str>) -> Option<usize> {
        selected_device_key.and_then(|device_key| self.assignments.get(device_key).copied())
    }
}

pub struct RuntimeUpdateEffect {
    pub payload_changed: bool,
    pub debug_events: Vec<DebugEvent>,
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
            app_discovery_backend: current_app_discovery_backend(),
            resolved_profile_id: config.active_profile_id.clone(),
            config,
            detected_devices: Vec::new(),
            selected_device_key: None,
            frontmost_app: None,
            app_discovery: default_app_discovery_snapshot(),
            enabled: true,
            debug_log: VecDeque::new(),
        };

        if let Some(load_warning) = load_warning {
            runtime.push_debug(DebugEventKind::Warning, load_warning);
        }
        runtime.refresh_live_state();
        runtime.refresh_app_discovery();
        runtime.sync_hook_backend();
        runtime.push_debug(
            DebugEventKind::Info,
            format!(
                "Runtime ready (hid={}, hook={}, focus={}, discovery={})",
                runtime.hid_backend.backend_id(),
                runtime.hook_backend.backend_id(),
                runtime.app_focus_backend.backend_id(),
                runtime.app_discovery_backend.backend_id()
            ),
        );
        if runtime.config.settings.debug_mode {
            runtime.log_debug_session_state();
        }
        runtime
    }

    pub fn bootstrap_payload(&self) -> BootstrapPayload {
        let layouts = self.catalog.layouts().to_vec();
        BootstrapPayload {
            config: self.config.clone(),
            available_actions: default_action_catalog(),
            known_apps: self.catalog.known_apps_ref().to_vec(),
            app_discovery: self.app_discovery.clone(),
            supported_devices: known_device_specs(),
            layouts: layouts.clone(),
            engine_snapshot: self.engine_snapshot(),
            platform_capabilities: self.platform_capabilities(),
            manual_layout_choices: manual_layout_choices(&layouts),
        }
    }

    pub fn config(&self) -> AppConfig {
        self.config.clone()
    }

    pub fn save_config(&mut self, config: AppConfig) {
        let debug_mode_was_enabled = self.replace_config(config);
        self.push_debug(
            DebugEventKind::Info,
            format!(
                "Saved config for `{}` at {} DPI",
                self.resolved_profile_id,
                self.selected_device_settings().dpi
            ),
        );
        self.log_dpi_state("DPI snapshot");
        self.log_debug_session_if_newly_enabled(debug_mode_was_enabled);
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
        self.save_profile(profile, "Created profile");
    }

    pub fn update_profile(&mut self, profile: Profile) {
        self.save_profile(profile, "Updated profile");
    }

    fn save_profile(&mut self, profile: Profile, message: &'static str) {
        self.config.upsert_profile(profile);
        self.sync_active_profile();
        self.persist_config();
        self.sync_hook_backend();
        self.push_debug(DebugEventKind::Info, message);
        self.log_active_profile_snapshot("Bindings snapshot");
    }

    fn edit_managed_device(
        &mut self,
        device_key: &str,
        edit: impl FnOnce(&mut ManagedDevice),
    ) -> bool {
        let mut edit = Some(edit);
        let mut updated = false;
        self.apply_config_edit(|config| {
            let Some(device) = config
                .managed_devices
                .iter_mut()
                .find(|device| device.id == device_key)
            else {
                return;
            };
            if let Some(edit) = edit.take() {
                edit(device);
                updated = true;
            }
        });
        updated
    }

    pub fn delete_profile(&mut self, profile_id: &str) {
        if self.config.delete_profile(profile_id) {
            self.sync_active_profile();
            self.persist_config();
            self.sync_hook_backend();
            self.push_debug(
                DebugEventKind::Info,
                format!("Deleted profile `{profile_id}`"),
            );
        }
    }

    pub fn add_managed_device(&mut self, model_key: &str) -> Option<String> {
        let Some(device_id) = self.add_managed_device_record(model_key) else {
            self.push_debug(
                DebugEventKind::Warning,
                format!("Unsupported managed device `{model_key}`"),
            );
            return None;
        };

        self.selected_device_key = Some(device_id.clone());
        self.sync_active_profile();
        self.persist_config();
        self.sync_hook_backend();
        self.push_debug(
            DebugEventKind::Info,
            format!("Added managed device `{model_key}`"),
        );
        self.log_device_inventory("Device library");
        self.log_dpi_state("DPI snapshot");
        Some(device_id)
    }

    pub fn remove_managed_device(&mut self, device_key: &str) {
        let before = self.config.managed_devices.len();
        self.config
            .managed_devices
            .retain(|device| device.id != device_key);
        if before == self.config.managed_devices.len() {
            return;
        }

        if self.selected_device_key.as_deref() == Some(device_key) {
            self.selected_device_key = None;
        }
        self.ensure_selected_device();
        self.sync_active_profile();
        self.persist_config();
        self.sync_hook_backend();
        self.push_debug(
            DebugEventKind::Info,
            format!("Removed managed device `{device_key}`"),
        );
        self.log_device_inventory("Device library");
    }

    pub fn select_device(&mut self, device_key: &str) {
        if self
            .config
            .managed_devices
            .iter()
            .any(|device| device.id == device_key)
        {
            self.selected_device_key = Some(device_key.to_string());
            self.sync_active_profile();
            self.sync_hook_backend();
            self.push_debug(
                DebugEventKind::Info,
                format!("Selected device `{device_key}`"),
            );
            self.log_device_inventory("Device library");
            self.log_active_profile_snapshot("Active bindings");
            self.log_dpi_state("DPI snapshot");
        }
    }

    pub fn update_app_settings(&mut self, settings: mouser_core::Settings) {
        let debug_mode_was_enabled = self.apply_config_edit(|config| {
            config.settings = settings;
        });
        self.push_debug(DebugEventKind::Info, "Updated app settings");
        self.log_debug_session_if_newly_enabled(debug_mode_was_enabled);
    }

    pub fn update_device_defaults(&mut self, settings: DeviceSettings) {
        self.apply_config_edit(|config| {
            config.device_defaults = settings;
        });
        self.push_debug(
            DebugEventKind::Info,
            "Updated default settings for new devices",
        );
    }

    pub fn update_managed_device_settings(&mut self, device_key: &str, settings: DeviceSettings) {
        let updated = self.edit_managed_device(device_key, move |device| {
            device.settings = settings;
        });
        if !updated {
            return;
        }
        self.push_debug(
            DebugEventKind::Info,
            format!("Updated settings for device `{device_key}`"),
        );
        self.log_dpi_state("DPI snapshot");
    }

    pub fn update_managed_device_profile(&mut self, device_key: &str, profile_id: Option<String>) {
        let updated = self.edit_managed_device(device_key, move |device| {
            device.profile_id = profile_id;
        });
        if !updated {
            return;
        }
        self.push_debug(
            DebugEventKind::Info,
            format!("Updated profile assignment for device `{device_key}`"),
        );
        self.log_active_profile_snapshot("Active bindings");
    }

    pub fn update_managed_device_nickname(&mut self, device_key: &str, nickname: Option<String>) {
        let updated = self.edit_managed_device(device_key, move |device| {
            device.nickname = nickname;
        });
        if !updated {
            return;
        }
        self.push_debug(
            DebugEventKind::Info,
            format!("Updated nickname for device `{device_key}`"),
        );
        self.log_device_inventory("Device library");
    }

    pub fn apply_imported_config(&mut self, config: AppConfig) {
        let debug_mode_was_enabled = self.replace_config(config);
        self.push_debug(DebugEventKind::Info, "Imported legacy Mouser config");
        self.log_active_profile_snapshot("Imported bindings");
        self.log_debug_session_if_newly_enabled(debug_mode_was_enabled);
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.managed_device_infos()
    }

    pub(crate) fn debug_log_len(&self) -> usize {
        self.debug_log.len()
    }

    pub fn clear_debug_log(&mut self) {
        self.debug_log.clear();
    }

    pub fn record_debug_event(&mut self, kind: DebugEventKind, message: impl Into<String>) {
        self.push_debug(kind, message);
    }

    pub fn refresh_app_discovery(&mut self) -> bool {
        let previous = self.app_discovery.clone();
        match self.app_discovery_backend.discover_apps() {
            Ok(installed_apps) => {
                self.app_discovery =
                    build_app_discovery_snapshot(&default_app_catalog(), &installed_apps);
                let changed = self.app_discovery != previous;
                if changed {
                    self.push_debug_if_enabled(
                        DebugEventKind::Info,
                        format!(
                            "Discovered {} suggested apps and {} installed apps",
                            self.app_discovery.suggested_apps.len(),
                            self.app_discovery.browse_apps.len()
                        ),
                    );
                }
                changed
            }
            Err(error) => {
                self.push_debug(
                    DebugEventKind::Warning,
                    format!("App discovery refresh failed: {error}"),
                );
                false
            }
        }
    }

    pub fn poll_backends(
        &self,
    ) -> (
        Arc<dyn HidBackend>,
        Arc<dyn AppFocusBackend>,
        Arc<dyn HookBackend>,
    ) {
        (
            Arc::clone(&self.hid_backend),
            Arc::clone(&self.app_focus_backend),
            Arc::clone(&self.hook_backend),
        )
    }

    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn apply_poll_results(
        &mut self,
        devices: Result<Vec<DeviceInfo>, PlatformError>,
        frontmost_app: Result<Option<AppIdentity>, PlatformError>,
        hook_events: Vec<HookBackendEvent>,
    ) -> RuntimeUpdateEffect {
        self.apply_runtime_updates(Some(devices), Some(frontmost_app), hook_events)
    }

    pub fn apply_runtime_updates(
        &mut self,
        devices: Option<Result<Vec<DeviceInfo>, PlatformError>>,
        frontmost_app: Option<Result<Option<AppIdentity>, PlatformError>>,
        hook_events: Vec<HookBackendEvent>,
    ) -> RuntimeUpdateEffect {
        let previous_debug_len = self.debug_log.len();
        let mut payload_changed = false;
        if let Some(devices) = devices {
            payload_changed |= self.apply_device_results(devices);
        }
        if let Some(frontmost_app) = frontmost_app {
            payload_changed |= self.apply_frontmost_app_result(frontmost_app);
        }
        self.collect_hook_events(hook_events);
        RuntimeUpdateEffect {
            payload_changed,
            debug_events: self.debug_events_since(previous_debug_len),
        }
    }

    pub fn engine_snapshot(&self) -> EngineSnapshot {
        let resolution = self.device_resolution();
        let active_device = self.active_device_from_resolution(&resolution);
        build_engine_snapshot(
            resolution.managed_devices,
            self.detected_devices.clone(),
            self.selected_device_key.clone(),
            active_device,
            EngineSnapshotState {
                enabled: self.enabled,
                active_profile_id: self.resolved_profile_id.clone(),
                frontmost_app: self.frontmost_app.as_ref(),
                debug_mode: self.config.settings.debug_mode,
                debug_log: self.debug_log.iter().cloned().collect(),
            },
        )
    }

    fn persist_config(&mut self) {
        if let Err(error) = self.config_store.save(&self.config) {
            self.push_debug(
                DebugEventKind::Warning,
                format!("Failed to save config: {error}"),
            );
        }
    }

    fn replace_config(&mut self, config: AppConfig) -> bool {
        let debug_mode_was_enabled = self.config.settings.debug_mode;
        let previous_dpi = self.selected_device_settings().dpi;
        self.config = config;
        self.config.ensure_invariants();
        if self
            .config
            .profile_by_id(&self.resolved_profile_id)
            .is_none()
        {
            self.resolved_profile_id = self.config.active_profile_id.clone();
        }
        self.apply_config_post_edit(previous_dpi);
        debug_mode_was_enabled
    }

    fn apply_config_edit(&mut self, edit: impl FnOnce(&mut AppConfig)) -> bool {
        let debug_mode_was_enabled = self.config.settings.debug_mode;
        let previous_dpi = self.selected_device_settings().dpi;
        edit(&mut self.config);
        self.config.ensure_invariants();
        if self
            .config
            .profile_by_id(&self.resolved_profile_id)
            .is_none()
        {
            self.resolved_profile_id = self.config.active_profile_id.clone();
        }
        self.apply_config_post_edit(previous_dpi);
        debug_mode_was_enabled
    }

    fn apply_config_post_edit(&mut self, previous_dpi: u16) {
        self.ensure_selected_device();

        let resolution = self.device_resolution();
        let active_device = self.active_device_from_resolution(&resolution);
        let configured_dpi = self.selected_device_settings().dpi;
        if previous_dpi != configured_dpi {
            self.push_debug_if_enabled(
                DebugEventKind::Info,
                format!(
                    "Applying DPI request {} -> {} for {}",
                    previous_dpi,
                    configured_dpi,
                    active_device
                        .as_ref()
                        .map(|device| device.display_name.as_str())
                        .unwrap_or("generic pointer"),
                ),
            );
        }

        if let Some(device) = active_device.as_ref() {
            if let Some(backend_key) = resolution
                .selected_live_device_index(self.selected_device_key.as_deref())
                .and_then(|index| self.detected_devices.get(index))
                .map(|device| device.key.as_str())
            {
                if let Err(error) = self.hid_backend.set_device_dpi(backend_key, configured_dpi) {
                    self.push_debug(
                        DebugEventKind::Warning,
                        format!("Failed to apply DPI to {}: {error}", device.display_name),
                    );
                }
            }
        }

        self.persist_config();
        self.sync_active_profile();
        self.sync_hook_backend();
        if previous_dpi != configured_dpi {
            self.log_dpi_state("DPI apply request");
        }
    }

    fn refresh_live_state(&mut self) {
        self.refresh_live_state_with_results(
            self.hid_backend.list_devices(),
            self.app_focus_backend.current_frontmost_app(),
        );
    }

    fn refresh_live_state_with_results(
        &mut self,
        devices: Result<Vec<DeviceInfo>, PlatformError>,
        frontmost_app: Result<Option<AppIdentity>, PlatformError>,
    ) {
        self.apply_device_results(devices);
        self.apply_frontmost_app_result(frontmost_app);
    }

    fn apply_device_results(&mut self, devices: Result<Vec<DeviceInfo>, PlatformError>) -> bool {
        match devices {
            Ok(devices) => self.replace_detected_devices(devices),
            Err(error) => {
                self.push_debug(
                    DebugEventKind::Warning,
                    format!("Live HID refresh failed: {error}"),
                );
                false
            }
        }
    }

    fn apply_frontmost_app_result(
        &mut self,
        frontmost_app: Result<Option<AppIdentity>, PlatformError>,
    ) -> bool {
        let previous_frontmost_app = self.frontmost_app.clone();
        let mut payload_changed = false;

        match frontmost_app {
            Ok(frontmost_app) => {
                self.frontmost_app = frontmost_app;
                if self.frontmost_app != previous_frontmost_app {
                    payload_changed = true;
                    self.push_debug_if_enabled(
                        DebugEventKind::Info,
                        format!(
                            "Frontmost app -> {}",
                            self.frontmost_app
                                .as_ref()
                                .and_then(AppIdentity::label_or_fallback)
                                .unwrap_or_else(|| "unknown".to_string())
                        ),
                    );
                }
                if self.sync_active_profile() {
                    payload_changed = true;
                    self.sync_hook_backend();
                }
            }
            Err(error) => {
                self.push_debug(
                    DebugEventKind::Warning,
                    format!("Frontmost-app refresh failed: {error}"),
                );
            }
        }
        payload_changed
    }

    fn replace_detected_devices(&mut self, devices: Vec<DeviceInfo>) -> bool {
        let previously_connected = self.device_resolution().connected_ids();
        let previous_selected_device_key = self.selected_device_key.clone();
        let mut changed = self.detected_devices != devices;
        self.detected_devices = devices;

        let mut changed_config = false;
        if self.config.managed_devices.is_empty() && !self.detected_devices.is_empty() {
            let detected_model_keys = self
                .detected_devices
                .iter()
                .map(|device| device.model_key.clone())
                .collect::<Vec<_>>();
            for model_key in detected_model_keys {
                if self.add_managed_device_record(&model_key).is_some() {
                    changed_config = true;
                }
            }
        }

        let assignments = self.matched_live_device_indexes();
        let now = now_ms();
        for device in &mut self.config.managed_devices {
            if let Some(index) = assignments.get(&device.id) {
                let live = &self.detected_devices[*index];
                let was_connected = previously_connected.contains(&device.id);
                if device.nickname.is_none() {
                    if let Some(live_product_name) = live
                        .product_name
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty() && *value != device.display_name)
                    {
                        device.display_name = live_product_name.to_string();
                        changed_config = true;
                    }
                }
                if device.identity_key.is_none() {
                    if let Some(identity_key) =
                        normalized_identity_key(live.fingerprint.identity_key.as_deref())
                    {
                        device.identity_key = Some(identity_key.to_string());
                        changed_config = true;
                    }
                }
                if !was_connected || device.last_seen_transport != live.transport {
                    device.last_seen_at_ms = Some(now);
                    device.last_seen_transport = live.transport.clone();
                    changed_config = true;
                }
            }
        }

        self.ensure_selected_device();
        changed |= self.selected_device_key != previous_selected_device_key;

        let resolution = self.device_resolution_with_assignments(assignments);
        if let Some(device) = self.active_device_from_resolution(&resolution) {
            let device_dpi = clamp_dpi(Some(&device), self.selected_device_settings().dpi);
            if self.selected_device_settings().dpi != device_dpi {
                self.push_debug_if_enabled(
                    DebugEventKind::Warning,
                    format!(
                        "Clamped configured DPI from {} to {} for {} (range {}-{})",
                        self.selected_device_settings().dpi,
                        device_dpi,
                        device.display_name,
                        device.dpi_min,
                        device.dpi_max,
                    ),
                );
                if let Some(selected_device) = self.selected_managed_device_mut() {
                    selected_device.settings.dpi = device_dpi;
                    changed_config = true;
                }
            }
        }

        if changed_config {
            self.persist_config();
            changed = true;
        }

        if changed {
            self.log_device_inventory("Device probe");
            self.log_dpi_state("DPI snapshot");
        }
        changed
    }

    fn sync_active_profile(&mut self) -> bool {
        let selected_profile_id = self
            .selected_managed_device()
            .and_then(|device| device.profile_id.clone());
        let next_profile_id = self
            .config
            .resolved_profile_id(selected_profile_id.as_deref(), self.frontmost_app.as_ref());
        if self.resolved_profile_id != next_profile_id {
            self.resolved_profile_id = next_profile_id;
            self.push_debug(
                DebugEventKind::Info,
                format!(
                    "Resolved profile for app {:?} -> {}",
                    self.frontmost_app
                        .as_ref()
                        .and_then(AppIdentity::label_or_fallback),
                    self.resolved_profile_id
                ),
            );
            self.log_active_profile_snapshot("Active bindings");
            return true;
        }
        false
    }

    fn sync_hook_backend(&mut self) {
        let Some(profile) = self.active_profile().cloned() else {
            return;
        };
        let hook_settings = HookBackendSettings::from_app_and_device(
            &self.config.settings,
            self.selected_device_settings(),
            self.selected_managed_device()
                .map(|device| device.model_key.as_str()),
        );

        if let Err(error) = self
            .hook_backend
            .configure(&hook_settings, &profile, self.enabled)
        {
            self.push_debug(
                DebugEventKind::Warning,
                format!("Failed to configure hook backend: {error}"),
            );
        }

        self.collect_hook_events(self.hook_backend.drain_events());
    }

    fn collect_hook_events(&mut self, events: Vec<HookBackendEvent>) {
        for event in events {
            self.push_debug(event.kind, event.message);
        }
    }

    fn platform_capabilities(&self) -> PlatformCapabilities {
        let hid_capabilities = self.hid_backend.capabilities();
        let hook_capabilities = self.hook_backend.capabilities();
        PlatformCapabilities {
            platform: if cfg!(target_os = "macos") {
                "macos".to_string()
            } else if cfg!(target_os = "linux") {
                "linux".to_string()
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
            hidapi_available: cfg!(any(
                target_os = "linux",
                target_os = "macos",
                target_os = "windows"
            )),
            iokit_available: cfg!(target_os = "macos"),
        }
    }

    fn push_debug(&mut self, kind: DebugEventKind, message: impl Into<String>) {
        self.debug_log.push_front(DebugEvent {
            kind,
            message: message.into(),
            timestamp_ms: now_ms(),
        });
        while self.debug_log.len() > 48 {
            let _ = self.debug_log.pop_back();
        }
    }

    fn push_debug_if_enabled(&mut self, kind: DebugEventKind, message: impl Into<String>) {
        if self.config.settings.debug_mode {
            self.push_debug(kind, message);
        }
    }

    fn log_debug_session_if_newly_enabled(&mut self, debug_mode_was_enabled: bool) {
        if !debug_mode_was_enabled && self.config.settings.debug_mode {
            self.log_debug_session_state();
        }
    }

    pub(crate) fn debug_events_since(&self, previous_len: usize) -> Vec<DebugEvent> {
        let new_count = self.debug_log.len().saturating_sub(previous_len);
        let mut events = self
            .debug_log
            .iter()
            .take(new_count)
            .cloned()
            .collect::<Vec<_>>();
        events.reverse();
        events
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
                if cfg!(any(target_os = "macos", target_os = "windows")) {
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
        self.log_dpi_state("DPI snapshot");
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

    fn log_dpi_state(&mut self, prefix: &str) {
        let resolution = self.device_resolution();
        let configured_dpi = self.selected_device_settings().dpi;
        let selected_device_key = self
            .selected_device_key
            .clone()
            .unwrap_or_else(|| "none".to_string());
        let managed_label = self
            .selected_managed_device()
            .map(|device| format!("{} ({})", device.display_name, device.model_key))
            .unwrap_or_else(|| "none".to_string());
        let selected_live = resolution
            .selected_live_device_index(self.selected_device_key.as_deref())
            .and_then(|index| self.detected_devices.get(index));
        let live_summary = selected_live
            .map(|device| {
                format!(
                    "{} model={} transport={} live_dpi={} range={}-{}",
                    device.display_name,
                    device.model_key,
                    device.transport.as_deref().unwrap_or("unknown"),
                    device.current_dpi,
                    device.dpi_min,
                    device.dpi_max,
                )
            })
            .unwrap_or_else(|| "none".to_string());
        let displayed_summary = self
            .active_device_from_resolution(&resolution)
            .map(|device| {
                format!(
                    "{} displayed_dpi={} range={}-{} connected={}",
                    device.display_name,
                    device.current_dpi,
                    device.dpi_min,
                    device.dpi_max,
                    device.connected,
                )
            })
            .unwrap_or_else(|| "none".to_string());
        let sync_summary = selected_live
            .map(|device| {
                if configured_dpi == device.current_dpi {
                    "configured_vs_live=in_sync".to_string()
                } else {
                    format!(
                        "configured_vs_live=drift(configured={}, live={})",
                        configured_dpi, device.current_dpi,
                    )
                }
            })
            .unwrap_or_else(|| "configured_vs_live=no-live-device".to_string());

        self.push_debug_if_enabled(
            DebugEventKind::Info,
            format!(
                "{prefix}: selected_key={selected_device_key}; managed={managed_label}; configured_dpi={configured_dpi}; displayed={displayed_summary}; live={live_summary}; {sync_summary}",
            ),
        );
    }

    fn describe_active_profile(&self) -> String {
        let Some(profile) = self.active_profile() else {
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
        let managed = self.device_resolution().managed_devices;
        let managed_summary = if managed.is_empty() {
            "no managed devices".to_string()
        } else {
            managed
                .iter()
                .map(|device| {
                    format!(
                        "{} [{}; transport={}; dpi={}]",
                        device.display_name,
                        if device.connected {
                            "connected"
                        } else {
                            "added"
                        },
                        device.transport.as_deref().unwrap_or("unknown"),
                        device.current_dpi,
                    )
                })
                .collect::<Vec<_>>()
                .join(" | ")
        };

        let detected_summary = if self.detected_devices.is_empty() {
            "no live detections".to_string()
        } else {
            self.detected_devices
                .iter()
                .map(|device| {
                    format!(
                        "{} (model={}, source={}, transport={})",
                        device.display_name,
                        device.model_key,
                        device.source.as_deref().unwrap_or("unknown"),
                        device.transport.as_deref().unwrap_or("unknown"),
                    )
                })
                .collect::<Vec<_>>()
                .join(" | ")
        };

        format!("managed: {managed_summary}; detected: {detected_summary}")
    }

    fn add_managed_device_record(&mut self, model_key: &str) -> Option<String> {
        let spec = known_device_spec_by_key(model_key)?;
        let id = self.next_managed_device_id(model_key);
        let device = ManagedDevice {
            id: id.clone(),
            model_key: spec.key,
            display_name: spec.display_name,
            nickname: None,
            profile_id: None,
            identity_key: None,
            settings: self.config.device_defaults.clone(),
            created_at_ms: now_ms(),
            last_seen_at_ms: None,
            last_seen_transport: None,
        };
        self.config.managed_devices.push(device);

        Some(id)
    }

    fn next_managed_device_id(&self, model_key: &str) -> String {
        let base = model_key.replace(' ', "_");
        let mut suffix = self
            .config
            .managed_devices
            .iter()
            .filter(|device| device.model_key == model_key)
            .count()
            + 1;
        let mut candidate = format!("{base}-{suffix}");
        let existing = self
            .config
            .managed_devices
            .iter()
            .map(|device| device.id.as_str())
            .collect::<BTreeSet<_>>();
        while existing.contains(candidate.as_str()) {
            suffix += 1;
            candidate = format!("{base}-{suffix}");
        }
        candidate
    }

    fn ensure_selected_device(&mut self) {
        let keep_current = self
            .selected_device_key
            .as_ref()
            .is_some_and(|selected_device_key| {
                self.config
                    .managed_devices
                    .iter()
                    .any(|device| device.id == *selected_device_key)
            });

        if keep_current {
            return;
        }

        let connected_ids = self.device_resolution().connected_ids();
        self.selected_device_key = self
            .config
            .managed_devices
            .iter()
            .find(|device| connected_ids.contains(&device.id))
            .map(|device| device.id.clone())
            .or_else(|| {
                self.config
                    .managed_devices
                    .first()
                    .map(|device| device.id.clone())
            });
    }

    fn managed_device_infos(&self) -> Vec<DeviceInfo> {
        self.device_resolution().managed_devices
    }

    fn device_resolution(&self) -> DeviceResolution {
        let assignments = self.matched_live_device_indexes();
        self.device_resolution_with_assignments(assignments)
    }

    fn device_resolution_with_assignments(
        &self,
        assignments: BTreeMap<String, usize>,
    ) -> DeviceResolution {
        let managed_devices = self
            .config
            .managed_devices
            .iter()
            .map(|device| {
                let live = assignments
                    .get(&device.id)
                    .and_then(|index| self.detected_devices.get(*index));
                build_managed_device_info(device, live)
            })
            .collect();

        DeviceResolution {
            assignments,
            managed_devices,
        }
    }

    fn active_device_from_resolution(&self, resolution: &DeviceResolution) -> Option<DeviceInfo> {
        self.config
            .managed_devices
            .iter()
            .find(|device| Some(device.id.as_str()) == self.selected_device_key.as_deref())
            .and_then(|managed| {
                resolution
                    .managed_devices
                    .iter()
                    .find(|device| device.key == managed.id)
                    .cloned()
                    .map(|device| {
                        active_device_with_layout(
                            device,
                            managed.settings.manual_layout_override.as_deref(),
                            self.catalog.layouts(),
                        )
                    })
            })
    }

    fn matched_live_device_indexes(&self) -> BTreeMap<String, usize> {
        let mut assignments = BTreeMap::new();
        let mut remaining_indexes = (0..self.detected_devices.len()).collect::<Vec<_>>();

        for device in &self.config.managed_devices {
            let Some(identity_key) = normalized_identity_key(device.identity_key.as_deref()) else {
                continue;
            };
            if let Some(position) = remaining_indexes.iter().position(|index| {
                let live = &self.detected_devices[*index];
                live.model_key == device.model_key
                    && normalized_identity_key(live.fingerprint.identity_key.as_deref())
                        == Some(identity_key)
            }) {
                let index = remaining_indexes.remove(position);
                assignments.insert(device.id.clone(), index);
            }
        }

        for device in &self.config.managed_devices {
            if assignments.contains_key(&device.id) {
                continue;
            }
            if let Some(position) = remaining_indexes.iter().position(|index| {
                live_matches_managed_device(device, &self.detected_devices[*index])
            }) {
                let index = remaining_indexes.remove(position);
                assignments.insert(device.id.clone(), index);
            }
        }

        assignments
    }

    fn selected_managed_device(&self) -> Option<&ManagedDevice> {
        self.selected_device_key.as_ref().and_then(|device_key| {
            self.config
                .managed_devices
                .iter()
                .find(|device| &device.id == device_key)
        })
    }

    fn selected_managed_device_mut(&mut self) -> Option<&mut ManagedDevice> {
        let selected_device_key = self.selected_device_key.clone()?;
        self.config
            .managed_devices
            .iter_mut()
            .find(|device| device.id == selected_device_key)
    }

    fn selected_device_settings(&self) -> &DeviceSettings {
        self.selected_managed_device()
            .map(|device| &device.settings)
            .unwrap_or(&self.config.device_defaults)
    }

    fn active_profile(&self) -> Option<&Profile> {
        self.config.profile_by_id(&self.resolved_profile_id)
    }
}

fn build_app_discovery_snapshot(
    catalog_apps: &[CatalogApp],
    installed_apps: &[InstalledApp],
) -> AppDiscoverySnapshot {
    let mut suggested_apps = Vec::new();
    let mut browse_apps = Vec::new();

    for installed_app in installed_apps {
        let catalog_match = catalog_apps.iter().find(|catalog_app| {
            catalog_app
                .matchers
                .iter()
                .any(|matcher| installed_app.identity.matches(matcher))
        });
        let discovered_app = discovered_app_from_sources(installed_app, catalog_match);
        if discovered_app.suggested {
            suggested_apps.push(discovered_app.clone());
        }
        browse_apps.push(discovered_app);
    }

    suggested_apps.sort_by(|left, right| left.label.cmp(&right.label));
    browse_apps.sort_by(|left, right| left.label.cmp(&right.label));

    AppDiscoverySnapshot {
        suggested_apps,
        browse_apps,
        last_scan_at_ms: Some(now_ms()),
        scanning: false,
    }
}

fn discovered_app_from_sources(
    installed_app: &InstalledApp,
    catalog_match: Option<&CatalogApp>,
) -> DiscoveredApp {
    let mut matchers = installed_app.identity.preferred_matchers();
    if let Some(catalog_match) = catalog_match {
        merge_matchers(&mut matchers, &catalog_match.matchers);
    }

    DiscoveredApp {
        id: catalog_match
            .map(|catalog_app| catalog_app.id.clone())
            .unwrap_or_else(|| installed_app.identity.stable_id()),
        label: catalog_match
            .map(|catalog_app| catalog_app.label.clone())
            .or_else(|| installed_app.identity.label_or_fallback())
            .unwrap_or_else(|| "Application".to_string()),
        description: installed_app.identity.detail_label(),
        matchers,
        icon_asset: catalog_match.and_then(|catalog_app| catalog_app.icon_asset.clone()),
        source_kinds: installed_app.source_kinds.clone(),
        source_path: installed_app.source_path.clone(),
        suggested: catalog_match.is_some(),
    }
}

fn merge_matchers(target: &mut Vec<AppMatcher>, source: &[AppMatcher]) {
    for matcher in source {
        let normalized = normalize_app_match_value(matcher.kind, &matcher.value);
        if target.iter().any(|existing| {
            existing.kind == matcher.kind
                && normalize_app_match_value(existing.kind, &existing.value) == normalized
        }) {
            continue;
        }
        target.push(matcher.clone());
    }
    target.sort_by(|left, right| matcher_priority(left.kind).cmp(&matcher_priority(right.kind)));
}

fn matcher_priority(kind: AppMatcherKind) -> u8 {
    match kind {
        AppMatcherKind::BundleId => 0,
        AppMatcherKind::PackageFamilyName => 1,
        AppMatcherKind::ExecutablePath => 2,
        AppMatcherKind::Executable => 3,
    }
}

fn current_hid_backend() -> Arc<dyn HidBackend> {
    if cfg!(target_os = "macos") {
        Arc::new(MacOsHidBackend::new())
    } else if cfg!(target_os = "linux") {
        Arc::new(LinuxHidBackend::new())
    } else {
        Arc::new(WindowsHidBackend::new())
    }
}

fn current_hook_backend() -> Arc<dyn HookBackend> {
    if cfg!(target_os = "macos") {
        Arc::new(MacOsHookBackend::new())
    } else if cfg!(target_os = "linux") {
        Arc::new(LinuxHookBackend::new())
    } else {
        Arc::new(WindowsHookBackend::new())
    }
}

fn current_app_focus_backend() -> Arc<dyn AppFocusBackend> {
    if cfg!(target_os = "macos") {
        Arc::new(MacOsAppFocusBackend)
    } else if cfg!(target_os = "linux") {
        Arc::new(LinuxAppFocusBackend)
    } else {
        Arc::new(WindowsAppFocusBackend)
    }
}

fn current_app_discovery_backend() -> Box<dyn AppDiscoveryBackend> {
    if cfg!(target_os = "macos") {
        Box::new(MacOsAppDiscoveryBackend)
    } else if cfg!(target_os = "linux") {
        Box::new(LinuxAppDiscoveryBackend)
    } else {
        Box::new(WindowsAppDiscoveryBackend)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn normalized_identity_key(value: Option<&str>) -> Option<&str> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn live_matches_managed_device(managed: &ManagedDevice, live: &DeviceInfo) -> bool {
    if live.model_key != managed.model_key {
        return false;
    }

    match (
        normalized_identity_key(managed.identity_key.as_deref()),
        normalized_identity_key(live.fingerprint.identity_key.as_deref()),
    ) {
        (Some(managed_identity), Some(live_identity)) => managed_identity == live_identity,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mouser_core::{default_device_settings, DeviceFingerprint};

    fn managed_device(identity_key: Option<&str>) -> ManagedDevice {
        ManagedDevice {
            id: "mx-master".to_string(),
            model_key: "mx_master_3s".to_string(),
            display_name: "MX Master 3S".to_string(),
            nickname: None,
            profile_id: None,
            identity_key: identity_key.map(str::to_string),
            settings: default_device_settings(),
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_transport: Some("Bluetooth Low Energy".to_string()),
        }
    }

    fn live_device(identity_key: Option<&str>) -> DeviceInfo {
        DeviceInfo {
            key: "live-device".to_string(),
            model_key: "mx_master_3s".to_string(),
            display_name: "MX Master 3S".to_string(),
            nickname: None,
            product_id: Some(0xB034),
            product_name: Some("MX Master 3S".to_string()),
            transport: Some("Bluetooth Low Energy".to_string()),
            source: Some("hidapi".to_string()),
            ui_layout: "mx_master".to_string(),
            image_asset: "/assets/mouse.png".to_string(),
            supported_controls: Vec::new(),
            gesture_cids: vec![0x00C3, 0x00D7],
            dpi_min: 200,
            dpi_max: 8000,
            connected: true,
            battery_level: Some(80),
            current_dpi: 1000,
            fingerprint: DeviceFingerprint {
                identity_key: identity_key.map(str::to_string),
                ..DeviceFingerprint::default()
            },
        }
    }

    #[test]
    fn identityless_live_device_does_not_steal_managed_identity_match() {
        let managed = managed_device(Some("serial:123"));
        let live = live_device(None);
        assert!(!live_matches_managed_device(&managed, &live));
    }

    #[test]
    fn unmanaged_identity_can_still_match_same_model_live_device() {
        let managed = managed_device(None);
        let live = live_device(Some("serial:123"));
        assert!(live_matches_managed_device(&managed, &live));
    }
}
