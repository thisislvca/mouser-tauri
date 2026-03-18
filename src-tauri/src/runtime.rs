use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use mouser_core::{
    build_managed_device_info, clamp_dpi, default_action_catalog, effective_layout_key,
    known_device_spec_by_key, known_device_specs, layout_by_key, manual_layout_choices, AppConfig,
    BootstrapPayload, DebugEvent, DebugEventKind, DeviceInfo, EngineSnapshot, EngineStatus,
    ManagedDevice, PlatformCapabilities, Profile,
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
    detected_devices: Vec<DeviceInfo>,
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
            detected_devices: Vec::new(),
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
            known_apps: self.catalog.known_apps(),
            supported_devices: known_device_specs(),
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
        self.log_dpi_state("DPI snapshot");

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

    pub fn add_managed_device(&mut self, model_key: &str) -> Option<String> {
        let Some(device_id) = self.add_managed_device_record(model_key) else {
            self.push_debug(
                DebugEventKind::Warning,
                format!("Unsupported managed device `{model_key}`"),
            );
            return None;
        };

        self.selected_device_key = Some(device_id.clone());
        self.config.settings.dpi = self
            .catalog
            .clamp_dpi(Some(model_key), self.config.settings.dpi);
        self.persist_config();
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

        self.config.settings.device_layout_overrides.remove(device_key);
        if self.selected_device_key.as_deref() == Some(device_key) {
            self.selected_device_key = None;
        }
        self.ensure_selected_device();
        self.persist_config();
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
            if let Some(device) = self.active_device_info() {
                self.config.settings.dpi = clamp_dpi(Some(&device), self.config.settings.dpi);
                self.persist_config();
            }
            self.push_debug(
                DebugEventKind::Info,
                format!("Selected device `{device_key}`"),
            );
            self.log_device_inventory("Device library");
            self.log_dpi_state("DPI snapshot");
        }
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
        self.managed_device_infos()
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
        let devices = self.managed_device_infos();
        let active_device = self
            .selected_device_key
            .as_ref()
            .and_then(|device_key| devices.iter().find(|device| &device.key == device_key))
            .cloned()
            .map(|mut device| {
                let layout_key =
                    effective_layout_key(&self.config.settings, Some(&device.key), &device.ui_layout);
                device.ui_layout = layout_key.clone();
                if let Some(layout) = layout_by_key(&self.catalog.all_layouts(), &layout_key) {
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
                connected: active_device.as_ref().is_some_and(|device| device.connected),
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
        let previous_dpi = self.config.settings.dpi;
        config.ensure_invariants();
        let selected_model_key = self.selected_managed_device().map(|device| device.model_key.as_str());
        let requested_dpi = config.settings.dpi;
        let clamped_dpi = self.catalog.clamp_dpi(selected_model_key, requested_dpi);
        if requested_dpi != clamped_dpi {
            self.push_debug_if_enabled(
                DebugEventKind::Warning,
                format!(
                    "Clamped requested DPI from {requested_dpi} to {clamped_dpi} for {}",
                    selected_model_key.unwrap_or("generic pointer")
                ),
            );
        }
        config.settings.dpi = clamped_dpi;
        self.config = config;
        self.ensure_selected_device();

        let active_device = self.active_device_info();
        if previous_dpi != self.config.settings.dpi {
            self.push_debug_if_enabled(
                DebugEventKind::Info,
                format!(
                    "Applying DPI request {} -> {} for {}",
                    previous_dpi,
                    self.config.settings.dpi,
                    active_device
                        .as_ref()
                        .map(|device| device.display_name.as_str())
                        .unwrap_or("generic pointer"),
                ),
            );
        }

        if let Some(device) = active_device.as_ref() {
            if let Some(backend_key) = self.selected_backend_device_key() {
                if let Err(error) = self
                    .hid_backend
                    .set_device_dpi(backend_key, self.config.settings.dpi)
                {
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
        if previous_dpi != self.config.settings.dpi {
            self.log_dpi_state("DPI apply request");
        }
        debug_mode_was_enabled
    }

    fn refresh_live_state(&mut self) {
        let previous_frontmost_app = self.frontmost_app.clone();

        match self.hid_backend.list_devices() {
            Ok(devices) => self.replace_detected_devices(devices),
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

    fn replace_detected_devices(&mut self, devices: Vec<DeviceInfo>) {
        let previous_summary = self.describe_devices();
        let previously_connected = self.connected_managed_device_ids();
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
                if !was_connected || device.last_seen_transport != live.transport {
                    device.last_seen_at_ms = Some(now);
                    device.last_seen_transport = live.transport.clone();
                    changed_config = true;
                }
            }
        }

        self.ensure_selected_device();
        if let Some(device) = self.active_device_info() {
            let device_dpi = clamp_dpi(Some(&device), self.config.settings.dpi);
            if self.config.settings.dpi != device_dpi {
                self.push_debug_if_enabled(
                    DebugEventKind::Warning,
                    format!(
                        "Clamped configured DPI from {} to {} for {} (range {}-{})",
                        self.config.settings.dpi,
                        device_dpi,
                        device.display_name,
                        device.dpi_min,
                        device.dpi_max,
                    ),
                );
                self.config.settings.dpi = device_dpi;
                changed_config = true;
            }
        }

        if changed_config {
            self.persist_config();
        }

        if self.describe_devices() != previous_summary {
            self.log_device_inventory("Device probe");
            self.log_dpi_state("DPI snapshot");
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
        let configured_dpi = self.config.settings.dpi;
        let selected_device_key = self
            .selected_device_key
            .clone()
            .unwrap_or_else(|| "none".to_string());
        let managed_label = self
            .selected_managed_device()
            .map(|device| format!("{} ({})", device.display_name, device.model_key))
            .unwrap_or_else(|| "none".to_string());
        let live_summary = self
            .selected_live_device_raw()
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
            .active_device_info()
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
        let sync_summary = self
            .selected_live_device_raw()
            .map(|device| {
                if configured_dpi == device.current_dpi {
                    "configured_vs_live=in_sync".to_string()
                } else {
                    format!(
                        "configured_vs_live=drift(configured={}, live={})",
                        configured_dpi,
                        device.current_dpi,
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
        let managed = self.managed_device_infos();
        let managed_summary = if managed.is_empty() {
            "no managed devices".to_string()
        } else {
            managed
                .iter()
                .map(|device| {
                    format!(
                        "{} [{}; transport={}; dpi={}]",
                        device.display_name,
                        if device.connected { "connected" } else { "added" },
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
            created_at_ms: now_ms(),
            last_seen_at_ms: None,
            last_seen_transport: None,
        };
        self.config.managed_devices.push(device);

        if let Some(existing_override) = self
            .config
            .settings
            .device_layout_overrides
            .get(model_key)
            .cloned()
        {
            self.config
                .settings
                .device_layout_overrides
                .entry(id.clone())
                .or_insert(existing_override);
        }

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

        let connected_ids = self.connected_managed_device_ids();
        self.selected_device_key = self
            .config
            .managed_devices
            .iter()
            .find(|device| connected_ids.contains(&device.id))
            .map(|device| device.id.clone())
            .or_else(|| self.config.managed_devices.first().map(|device| device.id.clone()));
    }

    fn managed_device_infos(&self) -> Vec<DeviceInfo> {
        let assignments = self.matched_live_device_indexes();
        self.config
            .managed_devices
            .iter()
            .map(|device| {
                let live = assignments
                    .get(&device.id)
                    .and_then(|index| self.detected_devices.get(*index));
                build_managed_device_info(device, live, self.config.settings.dpi)
            })
            .collect()
    }

    fn matched_live_device_indexes(&self) -> BTreeMap<String, usize> {
        let mut assignments = BTreeMap::new();
        let mut remaining_indexes = (0..self.detected_devices.len()).collect::<Vec<_>>();

        for device in &self.config.managed_devices {
            if let Some(position) = remaining_indexes.iter().position(|index| {
                self.detected_devices[*index].model_key == device.model_key
            }) {
                let index = remaining_indexes.remove(position);
                assignments.insert(device.id.clone(), index);
            }
        }

        assignments
    }

    fn connected_managed_device_ids(&self) -> BTreeSet<String> {
        self.matched_live_device_indexes()
            .into_keys()
            .collect::<BTreeSet<_>>()
    }

    fn selected_managed_device(&self) -> Option<&ManagedDevice> {
        self.selected_device_key.as_ref().and_then(|device_key| {
            self.config
                .managed_devices
                .iter()
                .find(|device| &device.id == device_key)
        })
    }

    fn selected_live_device_raw(&self) -> Option<&DeviceInfo> {
        let selected_device_key = self.selected_device_key.as_ref()?;
        let assignments = self.matched_live_device_indexes();
        let index = assignments.get(selected_device_key)?;
        self.detected_devices.get(*index)
    }

    fn selected_backend_device_key(&self) -> Option<&str> {
        self.selected_live_device_raw().map(|device| device.key.as_str())
    }

    fn active_device_info(&self) -> Option<DeviceInfo> {
        let selected_device_key = self.selected_device_key.as_ref()?;
        self.managed_device_infos()
            .into_iter()
            .find(|device| &device.key == selected_device_key)
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
