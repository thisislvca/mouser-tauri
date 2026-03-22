mod discovery;
mod error;
mod events;
mod platform;
mod routing;
mod service;

pub use error::{RuntimeError, RuntimeResult};
pub use events::{RuntimeBackgroundUpdate, RuntimeMutationResult, RuntimeNotification};
pub(crate) use routing::build_device_routing_event;
pub use service::{RuntimeNotifier, RuntimeService};

use std::{
    collections::{BTreeSet, VecDeque},
    path::PathBuf,
    sync::Arc,
};

use mouser_core::{
    build_engine_snapshot, clamp_dpi, default_action_catalog, default_app_discovery_snapshot,
    default_device_settings, known_device_spec_by_key, known_device_specs, manual_layout_choices,
    normalize_device_settings, AppConfig, AppDiscoverySnapshot, AppIdentity, BackendHealth,
    BackendHealthState, BootstrapPayload, DebugEvent, DebugEventKind, DebugLogGroup, DeviceInfo,
    DeviceRoutingSnapshot, EngineSnapshot, EngineSnapshotState, ManagedDevice,
    PlatformCapabilities, RuntimeHealth,
};
#[cfg(test)]
use mouser_core::{DeviceSettings, Profile};
use mouser_platform::{
    current_platform_name, emit_backend_console_log, host_hidapi_available, host_iokit_available,
    AppDiscoveryBackend, AppFocusBackend, ConfigStore, HidBackend, HookBackend, HookBackendEvent,
    HookBackendSettings, PlatformError, StaticDeviceCatalog,
};

use crate::config::JsonConfigStore;
use events::RuntimeUpdateEffect;
use platform::{
    current_app_discovery_backend, current_app_focus_backend, current_hid_backend,
    current_hook_backend, load_config_with_recovery, model_default_profile,
    model_default_profile_id, now_ms,
};
use routing::normalized_identity_key;

pub struct AppRuntime {
    catalog: StaticDeviceCatalog,
    config_store: Box<dyn ConfigStore>,
    hid_backend: Arc<dyn HidBackend>,
    hook_backend: Arc<dyn HookBackend>,
    app_focus_backend: Arc<dyn AppFocusBackend>,
    app_discovery_backend: Arc<dyn AppDiscoveryBackend>,
    config: AppConfig,
    resolved_profile_id: String,
    detected_devices: Vec<DeviceInfo>,
    selected_device_key: Option<String>,
    frontmost_app: Option<AppIdentity>,
    app_discovery: AppDiscoverySnapshot,
    enabled: bool,
    runtime_health: RuntimeHealth,
    debug_log: VecDeque<DebugLogEntry>,
    next_debug_seq: u64,
}

#[derive(Clone, Copy)]
enum BackendSlot {
    Persistence,
    Hid,
    Hook,
    Focus,
    Discovery,
}

#[derive(Clone)]
struct DebugLogEntry {
    seq: u64,
    event: DebugEvent,
}

impl AppRuntime {
    pub fn new(config_path: Option<PathBuf>) -> Self {
        let config_store = Box::new(JsonConfigStore::new(
            config_path.unwrap_or_else(JsonConfigStore::default_path),
        ));
        Self::from_parts(config_store)
    }

    fn from_parts(config_store: Box<dyn ConfigStore>) -> Self {
        let catalog = StaticDeviceCatalog::new();
        let (mut config, load_warning) = load_config_with_recovery(config_store.as_ref());
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
            runtime_health: RuntimeHealth::default(),
            debug_log: VecDeque::new(),
            next_debug_seq: 0,
        };

        if let Some(load_warning) = load_warning {
            runtime.push_debug(DebugEventKind::Warning, load_warning);
        }
        runtime.ensure_selected_device();
        runtime.sync_active_profile();
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

    pub fn save_config(&mut self, config: AppConfig) -> RuntimeResult<()> {
        let debug_mode_was_enabled = self.replace_config(config)?;
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
        Ok(())
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

    pub fn set_debug_mode(&mut self, enabled: bool) -> RuntimeResult<()> {
        if self.config.settings.debug_mode == enabled {
            return Ok(());
        }

        self.config.settings.debug_mode = enabled;
        self.persist_config()?;
        self.sync_hook_backend();

        if enabled {
            self.log_debug_session_state();
        } else {
            self.push_debug(DebugEventKind::Info, "Debug mode disabled");
        }
        Ok(())
    }

    pub fn add_managed_device(&mut self, model_key: &str) -> RuntimeResult<Option<String>> {
        let Some(device_id) = self.add_managed_device_record(model_key) else {
            self.push_debug(
                DebugEventKind::Warning,
                format!("Unsupported managed device `{model_key}`"),
            );
            return Ok(None);
        };

        self.selected_device_key = Some(device_id.clone());
        self.sync_active_profile();
        self.persist_config()?;
        self.sync_hook_backend();
        self.push_debug(
            DebugEventKind::Info,
            format!("Added managed device `{model_key}`"),
        );
        self.log_device_inventory("Device library");
        self.log_dpi_state("DPI snapshot");
        Ok(Some(device_id))
    }

    pub fn remove_managed_device(&mut self, device_key: &str) -> RuntimeResult<()> {
        let before = self.config.managed_devices.len();
        self.config
            .managed_devices
            .retain(|device| device.id != device_key);
        if before == self.config.managed_devices.len() {
            return Ok(());
        }

        if self.selected_device_key.as_deref() == Some(device_key) {
            self.selected_device_key = None;
        }
        self.ensure_selected_device();
        self.sync_active_profile();
        self.persist_config()?;
        self.sync_hook_backend();
        self.push_debug(
            DebugEventKind::Info,
            format!("Removed managed device `{device_key}`"),
        );
        self.log_device_inventory("Device library");
        Ok(())
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

    pub fn reset_managed_device_to_factory_defaults(
        &mut self,
        device_key: &str,
    ) -> RuntimeResult<()> {
        let Some(device) = self
            .config
            .managed_devices
            .iter()
            .find(|device| device.id == device_key)
            .cloned()
        else {
            return Ok(());
        };
        let Some(spec) = known_device_spec_by_key(&device.model_key) else {
            return Ok(());
        };

        let profile = model_default_profile(&spec);
        let profile_id = profile.id.clone();
        let display_name = device.display_name.clone();
        let mut settings = default_device_settings();
        normalize_device_settings(Some(&spec.key), &mut settings);

        let mut updated = false;
        self.apply_config_edit(|config| {
            config.upsert_profile(profile);
            let Some(device) = config
                .managed_devices
                .iter_mut()
                .find(|device| device.id == device_key)
            else {
                return;
            };
            device.profile_id = Some(profile_id.clone());
            device.settings = settings;
            updated = true;
        })?;
        if !updated {
            return Ok(());
        }

        self.push_debug(
            DebugEventKind::Info,
            format!("Reset `{display_name}` to factory defaults"),
        );
        self.log_active_profile_snapshot("Factory reset bindings");
        self.log_dpi_state("Factory reset DPI");
        Ok(())
    }

    pub fn apply_imported_config(&mut self, config: AppConfig) -> RuntimeResult<()> {
        let debug_mode_was_enabled = self.replace_config(config)?;
        self.push_debug(DebugEventKind::Info, "Imported legacy Mouser config");
        self.log_active_profile_snapshot("Imported bindings");
        self.log_debug_session_if_newly_enabled(debug_mode_was_enabled);
        Ok(())
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.managed_device_infos()
    }

    pub(crate) fn debug_event_cursor(&self) -> u64 {
        self.next_debug_seq
    }

    pub(crate) fn app_discovery_snapshot(&self) -> AppDiscoverySnapshot {
        self.app_discovery.clone()
    }

    pub fn clear_debug_log(&mut self) {
        self.debug_log.clear();
    }

    pub fn record_debug_event(&mut self, kind: DebugEventKind, message: impl Into<String>) {
        self.push_debug(kind, message);
    }

    pub(crate) fn hid_backend_handle(&self) -> Arc<dyn HidBackend> {
        Arc::clone(&self.hid_backend)
    }

    pub(crate) fn hook_backend_handle(&self) -> Arc<dyn HookBackend> {
        Arc::clone(&self.hook_backend)
    }

    pub(crate) fn app_focus_backend_handle(&self) -> Arc<dyn AppFocusBackend> {
        Arc::clone(&self.app_focus_backend)
    }

    pub(crate) fn app_discovery_backend_handle(&self) -> Arc<dyn AppDiscoveryBackend> {
        Arc::clone(&self.app_discovery_backend)
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
        let previous_debug_cursor = self.debug_event_cursor();
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
            app_discovery_changed: false,
            debug_events: self.debug_events_since(previous_debug_cursor),
        }
    }

    pub fn engine_snapshot(&self) -> EngineSnapshot {
        let resolution = self.device_resolution();
        let device_routing = self.device_routing_snapshot_from_resolution(&resolution);
        let active_device = self.active_device_from_resolution(&resolution);
        build_engine_snapshot(
            resolution.managed_devices,
            self.detected_devices.clone(),
            device_routing,
            self.selected_device_key.clone(),
            active_device,
            EngineSnapshotState {
                enabled: self.enabled,
                active_profile_id: self.resolved_profile_id.clone(),
                frontmost_app: self.frontmost_app.as_ref(),
                debug_mode: self.config.settings.debug_mode,
                debug_log: self
                    .debug_log
                    .iter()
                    .map(|entry| entry.event.clone())
                    .collect(),
                runtime_health: self.runtime_health.clone(),
            },
        )
    }

    pub fn device_routing_snapshot(&self) -> DeviceRoutingSnapshot {
        let resolution = self.device_resolution();
        self.device_routing_snapshot_from_resolution(&resolution)
    }

    fn persist_config(&mut self) -> RuntimeResult<()> {
        self.config_store.save(&self.config).map_err(|error| {
            self.mark_backend_health(
                BackendSlot::Persistence,
                BackendHealthState::Error,
                Some(format!("Failed to save config: {error}")),
            );
            RuntimeError::platform("persist_config", error)
        })?;
        self.mark_backend_health(BackendSlot::Persistence, BackendHealthState::Ready, None);
        Ok(())
    }

    fn replace_config(&mut self, config: AppConfig) -> RuntimeResult<bool> {
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
        self.apply_config_post_edit(previous_dpi)?;
        Ok(debug_mode_was_enabled)
    }

    fn apply_config_edit(&mut self, edit: impl FnOnce(&mut AppConfig)) -> RuntimeResult<bool> {
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
        self.apply_config_post_edit(previous_dpi)?;
        Ok(debug_mode_was_enabled)
    }

    fn apply_config_post_edit(&mut self, previous_dpi: u16) -> RuntimeResult<()> {
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

        self.persist_config()?;
        self.sync_active_profile();
        self.sync_hook_backend();
        if previous_dpi != configured_dpi {
            self.log_dpi_state("DPI apply request");
        }
        Ok(())
    }

    fn apply_device_results(&mut self, devices: Result<Vec<DeviceInfo>, PlatformError>) -> bool {
        match devices {
            Ok(devices) => {
                let health_changed =
                    self.mark_backend_health(BackendSlot::Hid, BackendHealthState::Ready, None);
                self.replace_detected_devices(devices) || health_changed
            }
            Err(error) => {
                let message = format!("Live HID refresh failed: {error}");
                self.push_debug(DebugEventKind::Warning, message.clone());
                self.mark_backend_health(BackendSlot::Hid, BackendHealthState::Stale, Some(message))
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
                payload_changed |=
                    self.mark_backend_health(BackendSlot::Focus, BackendHealthState::Ready, None);
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
                let message = format!("Frontmost-app refresh failed: {error}");
                self.push_debug(DebugEventKind::Warning, message.clone());
                payload_changed |= self.mark_backend_health(
                    BackendSlot::Focus,
                    BackendHealthState::Stale,
                    Some(message),
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
            if let Some(assignment) = assignments.get(&device.id) {
                let live = &self.detected_devices[assignment.live_index];
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
            if let Err(error) = self.persist_config() {
                self.push_debug(DebugEventKind::Warning, error.to_string());
            }
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
        let resolution = self.device_resolution();
        let hook_settings = HookBackendSettings::from_routes(
            &self.config.settings,
            self.hook_device_routes(&resolution),
        );

        if let Err(error) = self.hook_backend.configure(&hook_settings, self.enabled) {
            let message = format!("Failed to configure hook backend: {error}");
            self.push_debug(DebugEventKind::Warning, message.clone());
            self.mark_backend_health(BackendSlot::Hook, BackendHealthState::Error, Some(message));
        } else {
            self.mark_backend_health(BackendSlot::Hook, BackendHealthState::Ready, None);
        }

        self.collect_hook_events(self.hook_backend.drain_events());
    }

    fn collect_hook_events(&mut self, events: Vec<HookBackendEvent>) {
        for event in events {
            self.store_debug(event.kind, event.message);
        }
    }

    fn platform_capabilities(&self) -> PlatformCapabilities {
        let hid_capabilities = self.hid_backend.capabilities();
        let hook_capabilities = self.hook_backend.capabilities();
        PlatformCapabilities {
            platform: current_platform_name().to_string(),
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
            hidapi_available: host_hidapi_available(),
            iokit_available: host_iokit_available(),
        }
    }

    fn mark_backend_health(
        &mut self,
        slot: BackendSlot,
        state: BackendHealthState,
        message: Option<String>,
    ) -> bool {
        let next = BackendHealth {
            state,
            message,
            updated_at_ms: Some(now_ms()),
        };
        let current = match slot {
            BackendSlot::Persistence => &mut self.runtime_health.persistence,
            BackendSlot::Hid => &mut self.runtime_health.hid,
            BackendSlot::Hook => &mut self.runtime_health.hook,
            BackendSlot::Focus => &mut self.runtime_health.focus,
            BackendSlot::Discovery => &mut self.runtime_health.discovery,
        };

        if current.state == next.state && current.message == next.message {
            return false;
        }

        *current = next;
        true
    }

    fn push_debug(&mut self, kind: DebugEventKind, message: impl Into<String>) {
        let message = message.into();
        self.maybe_emit_runtime_console_log(kind, &message);
        self.store_debug(kind, message);
    }

    fn store_debug(&mut self, kind: DebugEventKind, message: impl Into<String>) {
        let entry = DebugLogEntry {
            seq: self.next_debug_seq,
            event: DebugEvent {
                kind,
                message: message.into(),
                timestamp_ms: now_ms(),
            },
        };
        self.next_debug_seq += 1;
        self.debug_log.push_front(entry);
        while self.debug_log.len() > 48 {
            let _ = self.debug_log.pop_back();
        }
    }

    fn maybe_emit_runtime_console_log(&self, kind: DebugEventKind, message: &str) {
        if !(self.config.settings.debug_mode
            && self
                .config
                .settings
                .debug_log_groups
                .enabled(DebugLogGroup::Runtime))
        {
            return;
        }
        emit_backend_console_log("runtime", kind, DebugLogGroup::Runtime, message);
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

    pub(crate) fn debug_events_since(&self, previous_cursor: u64) -> Vec<DebugEvent> {
        let mut events = self
            .debug_log
            .iter()
            .filter(|entry| entry.seq >= previous_cursor)
            .map(|entry| entry.event.clone())
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
                if host_hidapi_available() {
                    "ready"
                } else {
                    "unavailable"
                },
                if host_iokit_available() {
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
        let profile_id = self.ensure_model_default_profile(&spec);
        let id = self.next_managed_device_id(model_key);
        let device = ManagedDevice {
            id: id.clone(),
            model_key: spec.key,
            display_name: spec.display_name,
            nickname: None,
            profile_id: Some(profile_id),
            identity_key: None,
            settings: self.config.device_defaults.clone(),
            created_at_ms: now_ms(),
            last_seen_at_ms: None,
            last_seen_transport: None,
        };
        self.config.managed_devices.push(device);

        Some(id)
    }

    fn ensure_model_default_profile(&mut self, spec: &mouser_core::KnownDeviceSpec) -> String {
        let profile_id = model_default_profile_id(&spec.key);
        if self.config.profile_by_id(&profile_id).is_some() {
            return profile_id;
        }

        self.config.upsert_profile(model_default_profile(spec));
        profile_id
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
}

#[cfg(test)]
mod tests {
    use super::routing::live_matches_managed_device;
    use super::*;
    use mouser_core::{
        default_app_discovery_snapshot, default_config, default_device_settings, Binding,
        DeviceAttributionStatus, DeviceFingerprint, DeviceMatchKind, InstalledApp,
    };
    use mouser_platform::{ConfigStore, HidCapabilities, HookCapabilities};

    struct TestHidBackend;
    struct TestHookBackend;
    struct TestAppFocusBackend;
    struct TestAppDiscoveryBackend;
    struct FailingHookBackend;
    struct FailingDiscoveryBackend;
    struct FailingConfigStore;

    impl ConfigStore for FailingConfigStore {
        fn load(&self) -> Result<AppConfig, PlatformError> {
            Ok(default_config())
        }

        fn save(&self, _config: &AppConfig) -> Result<(), PlatformError> {
            Err(PlatformError::Message("disk full".to_string()))
        }
    }

    impl HidBackend for TestHidBackend {
        fn backend_id(&self) -> &'static str {
            "test-hid"
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
            Ok(Vec::new())
        }

        fn set_device_dpi(&self, _device_key: &str, _dpi: u16) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    impl HookBackend for TestHookBackend {
        fn backend_id(&self) -> &'static str {
            "test-hook"
        }

        fn capabilities(&self) -> HookCapabilities {
            HookCapabilities {
                can_intercept_buttons: false,
                can_intercept_scroll: false,
                supports_gesture_diversion: false,
            }
        }

        fn configure(
            &self,
            _settings: &HookBackendSettings,
            _enabled: bool,
        ) -> Result<(), PlatformError> {
            Ok(())
        }

        fn execute_action(&self, _action_id: &str) -> Result<(), PlatformError> {
            Ok(())
        }

        fn drain_events(&self) -> Vec<HookBackendEvent> {
            Vec::new()
        }
    }

    impl HookBackend for FailingHookBackend {
        fn backend_id(&self) -> &'static str {
            "failing-hook"
        }

        fn capabilities(&self) -> HookCapabilities {
            HookCapabilities {
                can_intercept_buttons: false,
                can_intercept_scroll: false,
                supports_gesture_diversion: false,
            }
        }

        fn configure(
            &self,
            _settings: &HookBackendSettings,
            _enabled: bool,
        ) -> Result<(), PlatformError> {
            Err(PlatformError::Message("hook offline".to_string()))
        }

        fn execute_action(&self, _action_id: &str) -> Result<(), PlatformError> {
            Ok(())
        }

        fn drain_events(&self) -> Vec<HookBackendEvent> {
            Vec::new()
        }
    }

    impl AppFocusBackend for TestAppFocusBackend {
        fn backend_id(&self) -> &'static str {
            "test-focus"
        }

        fn current_frontmost_app(&self) -> Result<Option<AppIdentity>, PlatformError> {
            Ok(None)
        }
    }

    impl AppDiscoveryBackend for TestAppDiscoveryBackend {
        fn backend_id(&self) -> &'static str {
            "test-discovery"
        }

        fn discover_apps(&self) -> Result<Vec<InstalledApp>, PlatformError> {
            Ok(Vec::new())
        }
    }

    impl AppDiscoveryBackend for FailingDiscoveryBackend {
        fn backend_id(&self) -> &'static str {
            "failing-discovery"
        }

        fn discover_apps(&self) -> Result<Vec<InstalledApp>, PlatformError> {
            Err(PlatformError::Message("scan failed".to_string()))
        }
    }

    fn test_runtime() -> AppRuntime {
        test_runtime_with_store(Box::new(JsonConfigStore::new(
            std::env::temp_dir().join(format!("mouser-runtime-test-{}.json", now_ms())),
        )))
    }

    fn test_runtime_with_store(config_store: Box<dyn ConfigStore>) -> AppRuntime {
        let config = default_config();
        AppRuntime {
            catalog: StaticDeviceCatalog::new(),
            config_store,
            hid_backend: Arc::new(TestHidBackend),
            hook_backend: Arc::new(TestHookBackend),
            app_focus_backend: Arc::new(TestAppFocusBackend),
            app_discovery_backend: Arc::new(TestAppDiscoveryBackend),
            resolved_profile_id: config.active_profile_id.clone(),
            config,
            detected_devices: Vec::new(),
            selected_device_key: None,
            frontmost_app: None,
            app_discovery: default_app_discovery_snapshot(),
            enabled: true,
            runtime_health: RuntimeHealth::default(),
            debug_log: VecDeque::new(),
            next_debug_seq: 0,
        }
    }

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
            controls: Vec::new(),
            support: mouser_core::DeviceSupportMatrix {
                level: mouser_core::DeviceSupportLevel::Experimental,
                supports_battery_status: false,
                supports_dpi_configuration: false,
                has_interactive_layout: false,
                notes: Vec::new(),
            },
            gesture_cids: vec![0x00C3, 0x00D7],
            dpi_min: 200,
            dpi_max: 8000,
            dpi_inferred: false,
            dpi_source_kind: None,
            connected: true,
            battery: Some(mouser_core::DeviceBatteryInfo {
                kind: mouser_core::DeviceBatteryKind::Percentage,
                percentage: Some(80),
                label: "80%".to_string(),
                source_feature: None,
                raw_capabilities: Vec::new(),
                raw_status: Vec::new(),
            }),
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

    #[test]
    fn device_routing_snapshot_reports_identity_match_for_live_device() {
        let mut runtime = test_runtime();
        runtime.config.managed_devices = vec![managed_device(Some("serial:123"))];
        runtime.selected_device_key = Some("mx-master".to_string());
        runtime.detected_devices = vec![live_device(Some("serial:123"))];

        let snapshot = runtime.device_routing_snapshot();
        assert_eq!(snapshot.entries.len(), 1);
        let entry = &snapshot.entries[0];
        assert_eq!(entry.live_device_key, "live-device");
        assert_eq!(entry.managed_device_key.as_deref(), Some("mx-master"));
        assert_eq!(entry.match_kind, DeviceMatchKind::Identity);
        assert!(entry.is_active_target);
    }

    #[test]
    fn device_routing_snapshot_reports_model_fallback_when_identity_missing() {
        let mut runtime = test_runtime();
        runtime.config.managed_devices = vec![managed_device(None)];
        runtime.selected_device_key = Some("mx-master".to_string());
        runtime.detected_devices = vec![live_device(Some("serial:456"))];

        let snapshot = runtime.device_routing_snapshot();
        assert_eq!(snapshot.entries.len(), 1);
        let entry = &snapshot.entries[0];
        assert_eq!(entry.managed_device_key.as_deref(), Some("mx-master"));
        assert_eq!(entry.match_kind, DeviceMatchKind::ModelFallback);
    }

    #[test]
    fn hook_device_routes_keep_single_model_fallback_assignment() {
        let mut runtime = test_runtime();
        runtime.config.managed_devices = vec![managed_device(None)];
        runtime.detected_devices = vec![live_device(Some("serial:456"))];

        let resolution = runtime.device_resolution();
        let routes = runtime.hook_device_routes(&resolution);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].managed_device_key, "mx-master");
    }

    #[test]
    fn hook_device_routes_drop_ambiguous_same_model_fallback_assignments() {
        let mut runtime = test_runtime();
        let mut managed_a = managed_device(None);
        managed_a.id = "mx-master-a".to_string();
        let mut managed_b = managed_device(None);
        managed_b.id = "mx-master-b".to_string();
        runtime.config.managed_devices = vec![managed_a, managed_b];

        let mut live_a = live_device(Some("serial:111"));
        live_a.key = "live-a".to_string();
        let mut live_b = live_device(Some("serial:222"));
        live_b.key = "live-b".to_string();
        runtime.detected_devices = vec![live_a, live_b];

        let resolution = runtime.device_resolution();
        assert!(runtime.hook_device_routes(&resolution).is_empty());

        let snapshot = runtime.device_routing_snapshot_from_resolution(&resolution);
        assert_eq!(snapshot.entries.len(), 2);
        assert!(snapshot.entries.iter().all(|entry| {
            entry.match_kind == DeviceMatchKind::ModelFallback
                && entry.attribution_status == DeviceAttributionStatus::Ambiguous
        }));
    }

    #[test]
    fn debug_events_since_still_returns_new_events_after_ring_buffer_rollover() {
        let mut runtime = test_runtime();

        for index in 0..48 {
            runtime.push_debug(DebugEventKind::Info, format!("seed-{index}"));
        }

        let cursor = runtime.debug_event_cursor();
        runtime.push_debug(DebugEventKind::Info, "latest");

        let events = runtime.debug_events_since(cursor);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "latest");
    }

    #[test]
    fn reset_managed_device_to_factory_defaults_restores_model_profile_and_settings() {
        let mut runtime = test_runtime();
        runtime.config.managed_devices.push(ManagedDevice {
            id: "mx_master_3s-1".to_string(),
            model_key: "mx_master_3s".to_string(),
            display_name: "MX Master 3S".to_string(),
            nickname: Some("Desk Mouse".to_string()),
            profile_id: Some("custom".to_string()),
            identity_key: None,
            settings: DeviceSettings {
                dpi: 2400,
                invert_horizontal_scroll: true,
                invert_vertical_scroll: true,
                macos_thumb_wheel_simulate_trackpad: true,
                macos_thumb_wheel_trackpad_hold_timeout_ms: 900,
                gesture_threshold: 90,
                gesture_deadzone: 70,
                gesture_timeout_ms: 5000,
                gesture_cooldown_ms: 900,
                manual_layout_override: Some("generic_mouse".to_string()),
            },
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_transport: None,
        });
        runtime.config.upsert_profile(Profile {
            id: "custom".to_string(),
            label: "Custom".to_string(),
            app_matchers: Vec::new(),
            bindings: vec![Binding {
                control: mouser_core::LogicalControl::Back,
                action_id: "mission_control".to_string(),
            }],
        });

        let _ = runtime.reset_managed_device_to_factory_defaults("mx_master_3s-1");

        let device = runtime
            .config
            .managed_devices
            .iter()
            .find(|device| device.id == "mx_master_3s-1")
            .unwrap();
        assert_eq!(device.profile_id.as_deref(), Some("device_mx_master_3s"));
        assert_eq!(device.settings, default_device_settings());
        assert_eq!(device.nickname.as_deref(), Some("Desk Mouse"));

        let profile = runtime
            .config
            .profile_by_id("device_mx_master_3s")
            .expect("expected seeded profile");
        assert_eq!(
            profile
                .binding_for(mouser_core::LogicalControl::GestureLeft)
                .map(|binding| binding.action_id.as_str()),
            Some("space_left")
        );
        assert_eq!(
            profile
                .binding_for(mouser_core::LogicalControl::GesturePress)
                .map(|binding| binding.action_id.as_str()),
            Some("mission_control")
        );
    }

    #[test]
    fn config_mutation_surfaces_persistence_failures_and_marks_health() {
        let mut runtime = test_runtime_with_store(Box::new(FailingConfigStore));
        let mut next_config = runtime.config.clone();
        next_config.settings.start_minimized = false;
        let result = runtime.save_config(next_config);

        assert!(matches!(result, Err(RuntimeError::Platform { .. })));
        assert_eq!(
            runtime
                .engine_snapshot()
                .engine_status
                .runtime_health
                .persistence
                .state,
            BackendHealthState::Error
        );
    }

    #[test]
    fn hid_refresh_failure_marks_backend_as_stale() {
        let mut runtime = test_runtime();

        let effect = runtime.apply_poll_results(
            Err(PlatformError::Message("hid unavailable".to_string())),
            Ok(None),
            Vec::new(),
        );

        assert!(effect.payload_changed);
        assert_eq!(
            runtime
                .engine_snapshot()
                .engine_status
                .runtime_health
                .hid
                .state,
            BackendHealthState::Stale
        );
    }

    #[test]
    fn focus_refresh_failure_marks_backend_as_stale() {
        let mut runtime = test_runtime();

        let effect = runtime.apply_poll_results(
            Ok(Vec::new()),
            Err(PlatformError::Message("focus unavailable".to_string())),
            Vec::new(),
        );

        assert!(effect.payload_changed);
        assert_eq!(
            runtime
                .engine_snapshot()
                .engine_status
                .runtime_health
                .focus
                .state,
            BackendHealthState::Stale
        );
    }

    #[test]
    fn app_discovery_failure_marks_backend_as_stale() {
        let mut runtime = test_runtime();
        runtime.app_discovery_backend = Arc::new(FailingDiscoveryBackend);

        runtime.start_app_discovery_scan();
        let changed = runtime
            .finish_app_discovery_scan(Err(PlatformError::Message("scan failed".to_string())));

        assert!(changed);
        assert_eq!(
            runtime
                .engine_snapshot()
                .engine_status
                .runtime_health
                .discovery
                .state,
            BackendHealthState::Stale
        );
    }

    #[test]
    fn hook_configure_failure_marks_backend_as_error() {
        let mut runtime = test_runtime();
        runtime.hook_backend = Arc::new(FailingHookBackend);

        runtime.sync_hook_backend();

        assert_eq!(
            runtime
                .engine_snapshot()
                .engine_status
                .runtime_health
                .hook
                .state,
            BackendHealthState::Error
        );
    }
}
