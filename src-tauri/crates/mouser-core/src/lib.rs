use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::OnceLock,
};

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum LogicalControl {
    Middle,
    GesturePress,
    GestureLeft,
    GestureRight,
    GestureUp,
    GestureDown,
    Back,
    Forward,
    HscrollLeft,
    HscrollRight,
}

impl LogicalControl {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Middle,
            Self::GesturePress,
            Self::GestureLeft,
            Self::GestureRight,
            Self::GestureUp,
            Self::GestureDown,
            Self::Back,
            Self::Forward,
            Self::HscrollLeft,
            Self::HscrollRight,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Middle => "Middle button",
            Self::GesturePress => "Gesture button",
            Self::GestureLeft => "Gesture swipe left",
            Self::GestureRight => "Gesture swipe right",
            Self::GestureUp => "Gesture swipe up",
            Self::GestureDown => "Gesture swipe down",
            Self::Back => "Back button",
            Self::Forward => "Forward button",
            Self::HscrollLeft => "Horizontal scroll left",
            Self::HscrollRight => "Horizontal scroll right",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub control: LogicalControl,
    pub action_id: String,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum AppMatcherKind {
    Executable,
    ExecutablePath,
    BundleId,
    PackageFamilyName,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppMatcher {
    pub kind: AppMatcherKind,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppIdentity {
    pub label: Option<String>,
    pub executable: Option<String>,
    pub executable_path: Option<String>,
    pub bundle_id: Option<String>,
    pub package_family_name: Option<String>,
}

impl AppIdentity {
    pub fn label_or_fallback(&self) -> Option<String> {
        self.label
            .clone()
            .or_else(|| self.executable.clone())
            .or_else(|| {
                self.executable_path.as_deref().and_then(|path| {
                    Path::new(path)
                        .file_name()
                        .map(|value| value.to_string_lossy().to_string())
                })
            })
            .or_else(|| self.bundle_id.clone())
            .or_else(|| self.package_family_name.clone())
    }

    pub fn preferred_matchers(&self) -> Vec<AppMatcher> {
        let mut matchers = Vec::new();

        for (kind, value) in [
            (AppMatcherKind::BundleId, self.bundle_id.as_deref()),
            (
                AppMatcherKind::PackageFamilyName,
                self.package_family_name.as_deref(),
            ),
            (
                AppMatcherKind::ExecutablePath,
                self.executable_path.as_deref(),
            ),
            (AppMatcherKind::Executable, self.executable.as_deref()),
        ] {
            if let Some(value) = value {
                push_unique_matcher(&mut matchers, kind, value);
            }
        }

        matchers
    }

    pub fn matches(&self, matcher: &AppMatcher) -> bool {
        let candidate = match matcher.kind {
            AppMatcherKind::Executable => self.executable.as_deref(),
            AppMatcherKind::ExecutablePath => self.executable_path.as_deref(),
            AppMatcherKind::BundleId => self.bundle_id.as_deref(),
            AppMatcherKind::PackageFamilyName => self.package_family_name.as_deref(),
        };

        candidate.is_some_and(|candidate| {
            normalize_app_match_value(matcher.kind, candidate)
                == normalize_app_match_value(matcher.kind, &matcher.value)
        })
    }

    pub fn stable_id(&self) -> String {
        if let Some(bundle_id) = self.bundle_id.as_deref() {
            return format!(
                "bundle:{}",
                normalize_app_match_value(AppMatcherKind::BundleId, bundle_id)
            );
        }

        if let Some(package_family_name) = self.package_family_name.as_deref() {
            return format!(
                "package:{}",
                normalize_app_match_value(AppMatcherKind::PackageFamilyName, package_family_name,)
            );
        }

        if let Some(executable_path) = self.executable_path.as_deref() {
            return format!(
                "path:{}",
                normalize_app_match_value(AppMatcherKind::ExecutablePath, executable_path)
            );
        }

        if let Some(executable) = self.executable.as_deref() {
            return format!(
                "exe:{}",
                normalize_app_match_value(AppMatcherKind::Executable, executable)
            );
        }

        format!(
            "label:{}",
            normalize_name(self.label.as_deref().unwrap_or("application"))
        )
    }

    pub fn detail_label(&self) -> Option<String> {
        self.bundle_id
            .clone()
            .or_else(|| self.package_family_name.clone())
            .or_else(|| self.executable.clone())
            .or_else(|| {
                self.executable_path.as_deref().map(|path| {
                    Path::new(path)
                        .file_name()
                        .map(|value| value.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.to_string())
                })
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceMode {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub start_minimized: bool,
    pub start_at_login: bool,
    pub appearance_mode: AppearanceMode,
    pub debug_mode: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSettings {
    pub dpi: u16,
    pub invert_horizontal_scroll: bool,
    pub invert_vertical_scroll: bool,
    #[serde(default)]
    pub macos_thumb_wheel_simulate_trackpad: bool,
    #[serde(default = "default_macos_thumb_wheel_trackpad_hold_timeout_ms")]
    pub macos_thumb_wheel_trackpad_hold_timeout_ms: u32,
    pub gesture_threshold: u16,
    pub gesture_deadzone: u16,
    pub gesture_timeout_ms: u32,
    pub gesture_cooldown_ms: u32,
    pub manual_layout_override: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub label: String,
    pub app_matchers: Vec<AppMatcher>,
    pub bindings: Vec<Binding>,
}

impl Profile {
    pub fn binding_for(&self, control: LogicalControl) -> Option<&Binding> {
        self.bindings
            .iter()
            .find(|binding| binding.control == control)
    }

    pub fn set_binding(&mut self, control: LogicalControl, action_id: impl Into<String>) {
        let action_id = action_id.into();
        if let Some(binding) = self
            .bindings
            .iter_mut()
            .find(|binding| binding.control == control)
        {
            binding.action_id = action_id;
        } else {
            self.bindings.push(Binding { control, action_id });
        }
        self.bindings = normalize_bindings(std::mem::take(&mut self.bindings));
    }

    pub fn normalized(mut self) -> Self {
        normalize_profile_in_place(&mut self);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub version: u32,
    pub active_profile_id: String,
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub managed_devices: Vec<ManagedDevice>,
    pub settings: Settings,
    #[serde(default = "default_device_settings")]
    pub device_defaults: DeviceSettings,
}

impl AppConfig {
    pub fn active_profile(&self) -> Option<&Profile> {
        self.profiles
            .iter()
            .find(|profile| profile.id == self.active_profile_id)
    }

    pub fn active_profile_mut(&mut self) -> Option<&mut Profile> {
        self.profiles
            .iter_mut()
            .find(|profile| profile.id == self.active_profile_id)
    }

    pub fn profile_by_id(&self, profile_id: &str) -> Option<&Profile> {
        self.profiles
            .iter()
            .find(|profile| profile.id == profile_id)
    }

    pub fn upsert_profile(&mut self, profile: Profile) {
        if let Some(existing) = self.profiles.iter_mut().find(|item| item.id == profile.id) {
            *existing = profile.normalized();
        } else {
            self.profiles.push(profile.normalized());
        }
        self.ensure_invariants();
    }

    pub fn delete_profile(&mut self, profile_id: &str) -> bool {
        if profile_id == "default" {
            return false;
        }

        let before = self.profiles.len();
        self.profiles.retain(|profile| profile.id != profile_id);
        for device in &mut self.managed_devices {
            if device.profile_id.as_deref() == Some(profile_id) {
                device.profile_id = None;
            }
        }
        if self.active_profile_id == profile_id {
            self.active_profile_id = "default".to_string();
        }
        self.ensure_invariants();
        before != self.profiles.len()
    }

    pub fn matched_profile_id_for_app(&self, app: Option<&AppIdentity>) -> String {
        app.and_then(|app| {
            self.profiles.iter().find(|profile| {
                profile
                    .app_matchers
                    .iter()
                    .any(|matcher| app.matches(matcher))
            })
        })
        .map(|profile| profile.id.clone())
        .unwrap_or_else(|| "default".to_string())
    }

    pub fn resolved_profile_id(
        &self,
        preferred_profile_id: Option<&str>,
        app: Option<&AppIdentity>,
    ) -> String {
        preferred_profile_id
            .and_then(|profile_id| {
                self.profile_by_id(profile_id)
                    .map(|_| profile_id.to_string())
            })
            .unwrap_or_else(|| self.matched_profile_id_for_app(app))
    }

    pub fn sync_active_profile(
        &mut self,
        preferred_profile_id: Option<&str>,
        app: Option<&AppIdentity>,
    ) -> bool {
        let target = self.resolved_profile_id(preferred_profile_id, app);

        if self.active_profile_id == target {
            return false;
        }

        self.active_profile_id = target;
        true
    }

    pub fn sync_active_profile_for_app(&mut self, app: Option<&AppIdentity>) -> bool {
        self.sync_active_profile(None, app)
    }

    pub fn ensure_invariants(&mut self) {
        if self.profiles.is_empty() {
            self.profiles.push(default_profile());
        }

        if !self.profiles.iter().any(|profile| profile.id == "default") {
            self.profiles.insert(0, default_profile());
        }

        for profile in &mut self.profiles {
            normalize_profile_in_place(profile);
        }

        if self.version < 4 {
            if let Some(default_profile) = self
                .profiles
                .iter_mut()
                .find(|profile| profile.id == "default")
            {
                if default_profile.bindings == legacy_default_profile_bindings_v3() {
                    default_profile.bindings = default_profile_bindings();
                }
            }
        }

        if !self
            .profiles
            .iter()
            .any(|profile| profile.id == self.active_profile_id)
        {
            self.active_profile_id = "default".to_string();
        }

        normalize_device_settings(None, &mut self.device_defaults);
        let valid_profile_ids = self
            .profiles
            .iter()
            .map(|profile| profile.id.clone())
            .collect::<std::collections::BTreeSet<_>>();

        let mut seen_managed_ids = std::collections::BTreeSet::new();
        self.managed_devices.retain(|device| {
            !device.id.trim().is_empty() && seen_managed_ids.insert(device.id.clone())
        });
        for device in &mut self.managed_devices {
            normalize_managed_device(device, &valid_profile_ids);
        }

        self.version = self.version.max(4);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActionDefinition {
    pub id: String,
    pub label: String,
    pub category: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KnownApp {
    pub executable: String,
    pub label: String,
    pub icon_asset: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogApp {
    pub id: String,
    pub label: String,
    pub icon_asset: Option<String>,
    pub matchers: Vec<AppMatcher>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppDiscoverySource {
    Catalog,
    ApplicationBundle,
    StartMenuShortcut,
    Registry,
    Package,
    RunningProcess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    pub identity: AppIdentity,
    pub source_kinds: Vec<AppDiscoverySource>,
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredApp {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub matchers: Vec<AppMatcher>,
    pub icon_asset: Option<String>,
    pub source_kinds: Vec<AppDiscoverySource>,
    pub source_path: Option<String>,
    pub suggested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppDiscoverySnapshot {
    pub suggested_apps: Vec<DiscoveredApp>,
    pub browse_apps: Vec<DiscoveredApp>,
    pub last_scan_at_ms: Option<u64>,
    pub scanning: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ManagedDevice {
    pub id: String,
    pub model_key: String,
    pub display_name: String,
    pub nickname: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub identity_key: Option<String>,
    #[serde(default = "default_device_settings")]
    pub settings: DeviceSettings,
    pub created_at_ms: u64,
    pub last_seen_at_ms: Option<u64>,
    pub last_seen_transport: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KnownDeviceSpec {
    pub key: String,
    pub display_name: String,
    pub product_ids: Vec<u16>,
    pub aliases: Vec<String>,
    pub gesture_cids: Vec<u16>,
    pub ui_layout: String,
    pub image_asset: String,
    pub supported_controls: Vec<LogicalControl>,
    pub dpi_min: u16,
    pub dpi_max: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum HotspotSummaryType {
    Mapping,
    Gesture,
    Hscroll,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LabelSide {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceHotspot {
    pub control: LogicalControl,
    pub label: String,
    pub summary_type: HotspotSummaryType,
    pub norm_x: f32,
    pub norm_y: f32,
    pub label_side: LabelSide,
    pub label_off_x: i32,
    pub label_off_y: i32,
    pub is_hscroll: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLayout {
    pub key: String,
    pub label: String,
    pub image_asset: String,
    pub image_width: u32,
    pub image_height: u32,
    pub interactive: bool,
    pub manual_selectable: bool,
    pub note: String,
    pub hotspots: Vec<DeviceHotspot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFingerprint {
    pub identity_key: Option<String>,
    pub serial_number: Option<String>,
    pub hid_path: Option<String>,
    pub interface_number: Option<i32>,
    pub usage_page: Option<u16>,
    pub usage: Option<u16>,
    pub location_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub key: String,
    pub model_key: String,
    pub display_name: String,
    pub nickname: Option<String>,
    pub product_id: Option<u16>,
    pub product_name: Option<String>,
    pub transport: Option<String>,
    pub source: Option<String>,
    pub ui_layout: String,
    pub image_asset: String,
    pub supported_controls: Vec<LogicalControl>,
    pub gesture_cids: Vec<u16>,
    pub dpi_min: u16,
    pub dpi_max: u16,
    pub connected: bool,
    pub battery_level: Option<u8>,
    pub current_dpi: u16,
    #[serde(default)]
    pub fingerprint: DeviceFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DebugEventKind {
    Info,
    Warning,
    Gesture,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DebugEvent {
    pub kind: DebugEventKind,
    pub message: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub enabled: bool,
    pub connected: bool,
    pub active_profile_id: String,
    pub frontmost_app: Option<String>,
    pub selected_device_key: Option<String>,
    pub debug_mode: bool,
    pub debug_log: Vec<DebugEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EngineSnapshot {
    pub devices: Vec<DeviceInfo>,
    pub detected_devices: Vec<DeviceInfo>,
    pub active_device_key: Option<String>,
    pub active_device: Option<DeviceInfo>,
    pub engine_status: EngineStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub platform: String,
    pub windows_supported: bool,
    pub macos_supported: bool,
    pub live_hooks_available: bool,
    pub live_hid_available: bool,
    pub tray_ready: bool,
    pub mapping_engine_ready: bool,
    pub gesture_diversion_available: bool,
    pub active_hid_backend: String,
    pub active_hook_backend: String,
    pub active_focus_backend: String,
    pub hidapi_available: bool,
    pub iokit_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LayoutChoice {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapPayload {
    pub config: AppConfig,
    pub available_actions: Vec<ActionDefinition>,
    pub known_apps: Vec<KnownApp>,
    pub app_discovery: AppDiscoverySnapshot,
    pub supported_devices: Vec<KnownDeviceSpec>,
    pub layouts: Vec<DeviceLayout>,
    pub engine_snapshot: EngineSnapshot,
    pub platform_capabilities: PlatformCapabilities,
    pub manual_layout_choices: Vec<LayoutChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LegacyImportReport {
    pub config: AppConfig,
    pub warnings: Vec<String>,
    pub source_path: Option<String>,
    pub imported_profiles: usize,
}

pub fn default_settings() -> Settings {
    Settings {
        start_minimized: true,
        start_at_login: false,
        appearance_mode: AppearanceMode::System,
        debug_mode: false,
    }
}

pub fn default_macos_thumb_wheel_trackpad_hold_timeout_ms() -> u32 {
    500
}

pub fn default_device_settings() -> DeviceSettings {
    DeviceSettings {
        dpi: 1000,
        invert_horizontal_scroll: false,
        invert_vertical_scroll: false,
        macos_thumb_wheel_simulate_trackpad: false,
        macos_thumb_wheel_trackpad_hold_timeout_ms:
            default_macos_thumb_wheel_trackpad_hold_timeout_ms(),
        gesture_threshold: 50,
        gesture_deadzone: 40,
        gesture_timeout_ms: 3000,
        gesture_cooldown_ms: 500,
        manual_layout_override: None,
    }
}

pub fn default_app_discovery_snapshot() -> AppDiscoverySnapshot {
    AppDiscoverySnapshot {
        suggested_apps: Vec::new(),
        browse_apps: Vec::new(),
        last_scan_at_ms: None,
        scanning: false,
    }
}

pub fn default_profile_bindings() -> Vec<Binding> {
    normalize_bindings(vec![
        Binding {
            control: LogicalControl::Back,
            action_id: "browser_back".to_string(),
        },
        Binding {
            control: LogicalControl::Forward,
            action_id: "browser_forward".to_string(),
        },
        Binding {
            control: LogicalControl::HscrollLeft,
            action_id: "browser_back".to_string(),
        },
        Binding {
            control: LogicalControl::HscrollRight,
            action_id: "browser_forward".to_string(),
        },
    ])
}

pub fn legacy_default_profile_bindings_v3() -> Vec<Binding> {
    normalize_bindings(vec![
        Binding {
            control: LogicalControl::Back,
            action_id: "alt_tab".to_string(),
        },
        Binding {
            control: LogicalControl::Forward,
            action_id: "alt_tab".to_string(),
        },
        Binding {
            control: LogicalControl::HscrollLeft,
            action_id: "browser_back".to_string(),
        },
        Binding {
            control: LogicalControl::HscrollRight,
            action_id: "browser_forward".to_string(),
        },
    ])
}

pub fn default_profile() -> Profile {
    Profile {
        id: "default".to_string(),
        label: "Default (All Apps)".to_string(),
        app_matchers: Vec::new(),
        bindings: default_profile_bindings(),
    }
}

pub fn default_config() -> AppConfig {
    AppConfig {
        version: 4,
        active_profile_id: "default".to_string(),
        profiles: vec![default_profile()],
        managed_devices: Vec::new(),
        settings: default_settings(),
        device_defaults: default_device_settings(),
    }
}

pub fn normalize_bindings(bindings: Vec<Binding>) -> Vec<Binding> {
    let mut map = BTreeMap::new();
    for binding in bindings {
        map.insert(binding.control, binding.action_id);
    }

    LogicalControl::all()
        .into_iter()
        .map(|control| Binding {
            control,
            action_id: map.remove(&control).unwrap_or_else(|| "none".to_string()),
        })
        .collect()
}

pub fn default_action_catalog() -> Vec<ActionDefinition> {
    default_action_catalog_ref().to_vec()
}

pub fn default_action_catalog_ref() -> &'static [ActionDefinition] {
    static ACTIONS: OnceLock<Vec<ActionDefinition>> = OnceLock::new();
    ACTIONS.get_or_init(|| {
        vec![
            action("alt_tab", "Alt + Tab (Switch Windows)", "Navigation"),
            action(
                "alt_shift_tab",
                "Alt + Shift + Tab (Switch Windows Reverse)",
                "Navigation",
            ),
            action(
                "show_desktop",
                "Show Desktop (Win + D / Mission Control)",
                "Navigation",
            ),
            action("task_view", "Task View / App Expose", "Navigation"),
            action("mission_control", "Mission Control", "Navigation"),
            action("app_expose", "App Expose", "Navigation"),
            action("launchpad", "Launchpad", "Navigation"),
            action("space_left", "Previous Space", "Navigation"),
            action("space_right", "Next Space", "Navigation"),
            action("browser_back", "Browser Back", "Browser"),
            action("browser_forward", "Browser Forward", "Browser"),
            action("close_tab", "Close Tab (Ctrl/Cmd + W)", "Browser"),
            action("new_tab", "New Tab (Ctrl/Cmd + T)", "Browser"),
            action("copy", "Copy (Ctrl/Cmd + C)", "Editing"),
            action("paste", "Paste (Ctrl/Cmd + V)", "Editing"),
            action("cut", "Cut (Ctrl/Cmd + X)", "Editing"),
            action("undo", "Undo (Ctrl/Cmd + Z)", "Editing"),
            action("select_all", "Select All (Ctrl/Cmd + A)", "Editing"),
            action("save", "Save (Ctrl/Cmd + S)", "Editing"),
            action("find", "Find (Ctrl/Cmd + F)", "Editing"),
            action("volume_up", "Volume Up", "Media"),
            action("volume_down", "Volume Down", "Media"),
            action("volume_mute", "Volume Mute", "Media"),
            action("play_pause", "Play / Pause", "Media"),
            action("next_track", "Next Track", "Media"),
            action("prev_track", "Previous Track", "Media"),
            action("none", "Do Nothing (Pass-through)", "Other"),
        ]
    })
}

pub fn default_app_catalog() -> Vec<CatalogApp> {
    vec![
        catalog_app(
            "microsoft_edge",
            "Microsoft Edge",
            None,
            &[
                (AppMatcherKind::Executable, "msedge.exe"),
                (AppMatcherKind::Executable, "Microsoft Edge"),
                (AppMatcherKind::BundleId, "com.microsoft.edgemac"),
            ],
        ),
        catalog_app(
            "google_chrome",
            "Google Chrome",
            None,
            &[
                (AppMatcherKind::Executable, "chrome.exe"),
                (AppMatcherKind::Executable, "Google Chrome"),
                (AppMatcherKind::BundleId, "com.google.Chrome"),
            ],
        ),
        catalog_app(
            "safari",
            "Safari",
            None,
            &[
                (AppMatcherKind::Executable, "Safari"),
                (AppMatcherKind::BundleId, "com.apple.Safari"),
            ],
        ),
        catalog_app(
            "vscode",
            "Visual Studio Code",
            None,
            &[
                (AppMatcherKind::Executable, "Code.exe"),
                (AppMatcherKind::Executable, "Code"),
                (AppMatcherKind::BundleId, "com.microsoft.VSCode"),
            ],
        ),
        catalog_app(
            "vlc",
            "VLC Media Player",
            None,
            &[
                (AppMatcherKind::Executable, "vlc.exe"),
                (AppMatcherKind::Executable, "VLC"),
                (AppMatcherKind::BundleId, "org.videolan.vlc"),
            ],
        ),
        catalog_app(
            "windows_media_player",
            "Windows Media Player",
            None,
            &[
                (AppMatcherKind::Executable, "Microsoft.Media.Player.exe"),
                (AppMatcherKind::Executable, "wmplayer.exe"),
            ],
        ),
        catalog_app(
            "finder",
            "Finder",
            None,
            &[
                (AppMatcherKind::Executable, "Finder"),
                (AppMatcherKind::BundleId, "com.apple.finder"),
            ],
        ),
    ]
}

pub fn default_known_apps() -> Vec<KnownApp> {
    default_known_apps_ref().to_vec()
}

pub fn default_known_apps_ref() -> &'static [KnownApp] {
    static KNOWN_APPS: OnceLock<Vec<KnownApp>> = OnceLock::new();
    KNOWN_APPS.get_or_init(build_default_known_apps)
}

fn build_default_known_apps() -> Vec<KnownApp> {
    let mut apps = Vec::new();
    for catalog in default_app_catalog() {
        for matcher in catalog.matchers {
            if matcher.kind != AppMatcherKind::Executable {
                continue;
            }

            if apps
                .iter()
                .any(|app: &KnownApp| app.executable.eq_ignore_ascii_case(&matcher.value))
            {
                continue;
            }

            apps.push(known_app(
                &matcher.value,
                &catalog.label,
                catalog.icon_asset.as_deref(),
            ));
        }
    }
    apps
}

pub fn known_device_specs() -> Vec<KnownDeviceSpec> {
    known_device_specs_ref().to_vec()
}

pub fn known_device_specs_ref() -> &'static [KnownDeviceSpec] {
    static KNOWN_DEVICE_SPECS: OnceLock<Vec<KnownDeviceSpec>> = OnceLock::new();
    KNOWN_DEVICE_SPECS.get_or_init(|| {
        vec![
            known_device(KnownDeviceSeed {
                key: "mx_master_3s",
                display_name: "MX Master 3S",
                product_ids: &[0xB034],
                aliases: &["Logitech MX Master 3S", "MX Master 3S for Mac"],
                gesture_cids: &[0x00C3, 0x00D7],
                ui_layout: "mx_master",
                image_asset: "/assets/mouse.png",
                dpi_range: (200, 8000),
            }),
            known_device(KnownDeviceSeed {
                key: "mx_master_3",
                display_name: "MX Master 3",
                product_ids: &[0xB023],
                aliases: &[
                    "Wireless Mouse MX Master 3",
                    "MX Master 3 for Mac",
                    "MX Master 3 Mac",
                ],
                gesture_cids: &[0x00C3, 0x00D7],
                ui_layout: "mx_master",
                image_asset: "/assets/mouse.png",
                dpi_range: (200, 8000),
            }),
            known_device(KnownDeviceSeed {
                key: "mx_master_2s",
                display_name: "MX Master 2S",
                product_ids: &[0xB019],
                aliases: &["Wireless Mouse MX Master 2S"],
                gesture_cids: &[0x00C3, 0x00D7],
                ui_layout: "mx_master",
                image_asset: "/assets/mouse.png",
                dpi_range: (200, 4000),
            }),
            known_device(KnownDeviceSeed {
                key: "mx_master",
                display_name: "MX Master",
                product_ids: &[0xB012],
                aliases: &["Wireless Mouse MX Master"],
                gesture_cids: &[0x00C3, 0x00D7],
                ui_layout: "mx_master",
                image_asset: "/assets/mouse.png",
                dpi_range: (200, 4000),
            }),
            known_device(KnownDeviceSeed {
                key: "mx_vertical",
                display_name: "MX Vertical",
                product_ids: &[0xB020],
                aliases: &[
                    "MX Vertical Wireless Mouse",
                    "MX Vertical Advanced Ergonomic Mouse",
                ],
                gesture_cids: &[0x00C3, 0x00D7],
                ui_layout: "mx_vertical",
                image_asset: "/assets/icons/mouse-simple.svg",
                dpi_range: (200, 4000),
            }),
            known_device(KnownDeviceSeed {
                key: "mx_anywhere_3s",
                display_name: "MX Anywhere 3S",
                product_ids: &[0xB037],
                aliases: &["MX Anywhere 3S for Mac"],
                gesture_cids: &[0x00C3],
                ui_layout: "mx_anywhere",
                image_asset: "/assets/icons/mouse-simple.svg",
                dpi_range: (200, 8000),
            }),
            known_device(KnownDeviceSeed {
                key: "mx_anywhere_3",
                display_name: "MX Anywhere 3",
                product_ids: &[0xB025],
                aliases: &["MX Anywhere 3 for Mac"],
                gesture_cids: &[0x00C3],
                ui_layout: "mx_anywhere",
                image_asset: "/assets/icons/mouse-simple.svg",
                dpi_range: (200, 4000),
            }),
            known_device(KnownDeviceSeed {
                key: "mx_anywhere_2s",
                display_name: "MX Anywhere 2S",
                product_ids: &[0xB01A],
                aliases: &["Wireless Mobile Mouse MX Anywhere 2S"],
                gesture_cids: &[0x00C3],
                ui_layout: "mx_anywhere",
                image_asset: "/assets/icons/mouse-simple.svg",
                dpi_range: (200, 4000),
            }),
        ]
    })
}

pub fn known_device_spec_by_key(model_key: &str) -> Option<KnownDeviceSpec> {
    known_device_specs_ref()
        .iter()
        .find(|spec| spec.key == model_key)
        .cloned()
}

pub fn default_layouts() -> Vec<DeviceLayout> {
    default_layouts_ref().to_vec()
}

pub fn default_layouts_ref() -> &'static [DeviceLayout] {
    static LAYOUTS: OnceLock<Vec<DeviceLayout>> = OnceLock::new();
    LAYOUTS.get_or_init(|| {
    vec![
        DeviceLayout {
            key: "mx_master".to_string(),
            label: "MX Master family".to_string(),
            image_asset: "/assets/mouse.png".to_string(),
            image_width: 460,
            image_height: 360,
            interactive: true,
            manual_selectable: true,
            note: String::new(),
            hotspots: vec![
                hotspot(HotspotSpec {
                    control: LogicalControl::Middle,
                    label: "Middle button",
                    summary_type: HotspotSummaryType::Mapping,
                    norm: (0.35, 0.40),
                    label_side: LabelSide::Right,
                    label_offset: (100, -160),
                    is_hscroll: false,
                }),
                hotspot(HotspotSpec {
                    control: LogicalControl::GesturePress,
                    label: "Gesture button",
                    summary_type: HotspotSummaryType::Gesture,
                    norm: (0.70, 0.63),
                    label_side: LabelSide::Left,
                    label_offset: (-200, 60),
                    is_hscroll: false,
                }),
                hotspot(HotspotSpec {
                    control: LogicalControl::Forward,
                    label: "Forward button",
                    summary_type: HotspotSummaryType::Mapping,
                    norm: (0.60, 0.48),
                    label_side: LabelSide::Left,
                    label_offset: (-300, 0),
                    is_hscroll: false,
                }),
                hotspot(HotspotSpec {
                    control: LogicalControl::Back,
                    label: "Back button",
                    summary_type: HotspotSummaryType::Mapping,
                    norm: (0.65, 0.40),
                    label_side: LabelSide::Right,
                    label_offset: (200, 50),
                    is_hscroll: false,
                }),
                hotspot(HotspotSpec {
                    control: LogicalControl::HscrollLeft,
                    label: "Horizontal scroll",
                    summary_type: HotspotSummaryType::Hscroll,
                    norm: (0.60, 0.375),
                    label_side: LabelSide::Right,
                    label_offset: (200, -50),
                    is_hscroll: true,
                }),
            ],
        },
        DeviceLayout {
            key: "mx_anywhere".to_string(),
            label: "MX Anywhere family".to_string(),
            image_asset: "/assets/icons/mouse-simple.svg".to_string(),
            image_width: 220,
            image_height: 220,
            interactive: false,
            manual_selectable: false,
            note: "MX Anywhere support is wired for device detection and HID++ probing. A dedicated overlay still needs to be added."
                .to_string(),
            hotspots: Vec::new(),
        },
        DeviceLayout {
            key: "mx_vertical".to_string(),
            label: "MX Vertical family".to_string(),
            image_asset: "/assets/icons/mouse-simple.svg".to_string(),
            image_width: 220,
            image_height: 220,
            interactive: false,
            manual_selectable: false,
            note: "MX Vertical falls back to a generic device card until a dedicated overlay is added."
                .to_string(),
            hotspots: Vec::new(),
        },
        DeviceLayout {
            key: "generic_mouse".to_string(),
            label: "Generic mouse".to_string(),
            image_asset: "/assets/icons/mouse-simple.svg".to_string(),
            image_width: 220,
            image_height: 220,
            interactive: false,
            manual_selectable: false,
            note: "This device is detected and the backend can still probe HID++ features, but Mouser does not have a dedicated visual overlay for it yet."
                .to_string(),
            hotspots: Vec::new(),
        },
    ]
    })
}

pub fn manual_layout_choices(layouts: &[DeviceLayout]) -> Vec<LayoutChoice> {
    let mut choices = vec![LayoutChoice {
        key: String::new(),
        label: "Auto-detect".to_string(),
    }];
    choices.extend(
        layouts
            .iter()
            .filter(|layout| layout.manual_selectable)
            .map(|layout| LayoutChoice {
                key: layout.key.clone(),
                label: layout.label.clone(),
            }),
    );
    choices
}

pub fn default_device_catalog() -> Vec<DeviceInfo> {
    vec![
        DeviceInfo {
            key: "mx_master_3s".to_string(),
            model_key: "mx_master_3s".to_string(),
            display_name: "MX Master 3S".to_string(),
            nickname: None,
            product_id: Some(0xB034),
            product_name: Some("MX Master 3S".to_string()),
            transport: Some("Bluetooth Low Energy".to_string()),
            source: Some("mock-catalog".to_string()),
            ui_layout: "mx_master".to_string(),
            image_asset: "/assets/mouse.png".to_string(),
            supported_controls: LogicalControl::all(),
            gesture_cids: vec![0x00C3, 0x00D7],
            dpi_min: 200,
            dpi_max: 8000,
            connected: true,
            battery_level: Some(84),
            current_dpi: 1200,
            fingerprint: DeviceFingerprint::default(),
        },
        DeviceInfo {
            key: "mx_anywhere_3s".to_string(),
            model_key: "mx_anywhere_3s".to_string(),
            display_name: "MX Anywhere 3S".to_string(),
            nickname: None,
            product_id: Some(0xB037),
            product_name: Some("MX Anywhere 3S".to_string()),
            transport: Some("Bolt Receiver".to_string()),
            source: Some("mock-catalog".to_string()),
            ui_layout: "mx_anywhere".to_string(),
            image_asset: "/assets/icons/mouse-simple.svg".to_string(),
            supported_controls: LogicalControl::all(),
            gesture_cids: vec![0x00C3],
            dpi_min: 200,
            dpi_max: 8000,
            connected: false,
            battery_level: Some(62),
            current_dpi: 1000,
            fingerprint: DeviceFingerprint::default(),
        },
        DeviceInfo {
            key: "mystery_logitech_mouse".to_string(),
            model_key: "mystery_logitech_mouse".to_string(),
            display_name: "Mystery Logitech Mouse".to_string(),
            nickname: None,
            product_id: Some(0xB999),
            product_name: Some("Mystery Logitech Mouse".to_string()),
            transport: Some("USB".to_string()),
            source: Some("mock-catalog".to_string()),
            ui_layout: "generic_mouse".to_string(),
            image_asset: "/assets/icons/mouse-simple.svg".to_string(),
            supported_controls: LogicalControl::all(),
            gesture_cids: vec![0x00C3, 0x00D7],
            dpi_min: 200,
            dpi_max: 8000,
            connected: false,
            battery_level: None,
            current_dpi: 900,
            fingerprint: DeviceFingerprint::default(),
        },
    ]
}

pub fn resolve_known_device(
    product_id: Option<u16>,
    product_name: Option<&str>,
) -> Option<KnownDeviceSpec> {
    let normalized_name = normalize_name(product_name.unwrap_or_default());
    known_device_specs_ref()
        .iter()
        .find(|spec| {
            product_id
                .map(|product_id| spec.product_ids.contains(&product_id))
                .unwrap_or(false)
                || (!normalized_name.is_empty()
                    && std::iter::once(spec.display_name.as_str())
                        .chain(spec.aliases.iter().map(String::as_str))
                        .any(|candidate| normalize_name(candidate) == normalized_name))
        })
        .cloned()
}

pub fn build_connected_device_info(
    product_id: Option<u16>,
    product_name: Option<&str>,
    transport: Option<&str>,
    source: Option<&str>,
    battery_level: Option<u8>,
    current_dpi: u16,
    mut fingerprint: DeviceFingerprint,
) -> DeviceInfo {
    hydrate_identity_key(product_id, &mut fingerprint);
    let product_name = non_empty_name(product_name);
    if let Some(spec) = resolve_known_device(product_id, product_name.as_deref()) {
        let model_key = spec.key.clone();
        let display_name = product_name
            .clone()
            .unwrap_or_else(|| spec.display_name.clone());
        let key = live_device_key(&model_key, &fingerprint);
        return DeviceInfo {
            key,
            model_key,
            display_name,
            nickname: None,
            product_id,
            product_name,
            transport: transport.map(str::to_string),
            source: source.map(str::to_string),
            ui_layout: spec.ui_layout,
            image_asset: spec.image_asset,
            supported_controls: spec.supported_controls,
            gesture_cids: spec.gesture_cids,
            dpi_min: spec.dpi_min,
            dpi_max: spec.dpi_max,
            connected: true,
            battery_level,
            current_dpi,
            fingerprint,
        };
    }

    let display_name = product_name
        .or_else(|| product_id.map(|product_id| format!("Logitech PID 0x{product_id:04X}")))
        .unwrap_or_else(|| "Logitech mouse".to_string());
    let key = normalize_name(&display_name).replace(' ', "_");
    let fallback_key = if key.is_empty() {
        "logitech_mouse".to_string()
    } else {
        key
    };
    let key = live_device_key(&fallback_key, &fingerprint);

    DeviceInfo {
        key,
        model_key: fallback_key,
        display_name: display_name.clone(),
        nickname: None,
        product_id,
        product_name: Some(display_name),
        transport: transport.map(str::to_string),
        source: source.map(str::to_string),
        ui_layout: "generic_mouse".to_string(),
        image_asset: "/assets/icons/mouse-simple.svg".to_string(),
        supported_controls: LogicalControl::all(),
        gesture_cids: vec![0x00C3, 0x00D7],
        dpi_min: 200,
        dpi_max: 8000,
        connected: true,
        battery_level,
        current_dpi,
        fingerprint,
    }
}

pub fn build_managed_device_info(managed: &ManagedDevice, live: Option<&DeviceInfo>) -> DeviceInfo {
    let effective_current_dpi = live
        .map(|device| device.current_dpi)
        .unwrap_or(managed.settings.dpi);
    let live_product_name = live.and_then(|device| non_empty_name(device.product_name.as_deref()));
    let display_name = managed_device_display_name(managed, live_product_name.as_deref());
    let connected = live.is_some();
    let battery_level = live.and_then(|device| device.battery_level);
    let transport = managed_device_transport(managed, live);
    let source = managed_device_source(live);
    let fingerprint = managed_device_fingerprint(managed, live);

    if let Some(spec) = known_device_spec_by_key(&managed.model_key) {
        return DeviceInfo {
            key: managed.id.clone(),
            model_key: managed.model_key.clone(),
            display_name,
            nickname: managed.nickname.clone(),
            product_id: live
                .and_then(|device| device.product_id)
                .or_else(|| spec.product_ids.first().copied()),
            product_name: live_product_name.or_else(|| Some(spec.display_name.clone())),
            transport,
            source,
            ui_layout: spec.ui_layout,
            image_asset: spec.image_asset,
            supported_controls: spec.supported_controls,
            gesture_cids: spec.gesture_cids,
            dpi_min: spec.dpi_min,
            dpi_max: spec.dpi_max,
            connected,
            battery_level,
            current_dpi: effective_current_dpi.max(spec.dpi_min).min(spec.dpi_max),
            fingerprint,
        };
    }

    DeviceInfo {
        key: managed.id.clone(),
        model_key: managed.model_key.clone(),
        display_name,
        nickname: managed.nickname.clone(),
        product_id: live.and_then(|device| device.product_id),
        product_name: live_product_name.or_else(|| Some(managed.display_name.clone())),
        transport,
        source,
        ui_layout: "generic_mouse".to_string(),
        image_asset: "/assets/icons/mouse-simple.svg".to_string(),
        supported_controls: LogicalControl::all(),
        gesture_cids: vec![0x00C3, 0x00D7],
        dpi_min: 200,
        dpi_max: 8000,
        connected,
        battery_level,
        current_dpi: effective_current_dpi.clamp(200, 8000),
        fingerprint,
    }
}

pub fn clamp_dpi(device: Option<&DeviceInfo>, value: u16) -> u16 {
    let min = device.map(|info| info.dpi_min).unwrap_or(200);
    let max = device.map(|info| info.dpi_max).unwrap_or(8000);
    value.max(min).min(max)
}

pub fn normalize_device_settings(model_key: Option<&str>, settings: &mut DeviceSettings) {
    let (dpi_min, dpi_max) = known_device_spec_by_key(model_key.unwrap_or_default())
        .map(|spec| (spec.dpi_min, spec.dpi_max))
        .unwrap_or((200, 8000));
    settings.dpi = settings.dpi.max(dpi_min).min(dpi_max);

    if settings
        .manual_layout_override
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        settings.manual_layout_override = None;
    }
}

pub fn layout_by_key<'a>(
    layouts: &'a [DeviceLayout],
    layout_key: &str,
) -> Option<&'a DeviceLayout> {
    layouts.iter().find(|layout| layout.key == layout_key)
}

pub fn effective_layout_key(manual_override: Option<&str>, default_layout_key: &str) -> String {
    manual_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_layout_key)
        .to_string()
}

pub fn active_device_with_layout(
    mut device: DeviceInfo,
    manual_override: Option<&str>,
    layouts: &[DeviceLayout],
) -> DeviceInfo {
    let layout_key = effective_layout_key(manual_override, &device.ui_layout);
    device.ui_layout = layout_key.clone();
    if let Some(layout) = layout_by_key(layouts, &layout_key) {
        device.image_asset = layout.image_asset.clone();
    }
    device
}

pub struct EngineSnapshotState<'a> {
    pub enabled: bool,
    pub active_profile_id: String,
    pub frontmost_app: Option<&'a AppIdentity>,
    pub debug_mode: bool,
    pub debug_log: Vec<DebugEvent>,
}

pub fn build_engine_snapshot(
    devices: Vec<DeviceInfo>,
    detected_devices: Vec<DeviceInfo>,
    active_device_key: Option<String>,
    active_device: Option<DeviceInfo>,
    state: EngineSnapshotState<'_>,
) -> EngineSnapshot {
    EngineSnapshot {
        devices,
        detected_devices,
        active_device_key: active_device_key.clone(),
        active_device: active_device.clone(),
        engine_status: EngineStatus {
            enabled: state.enabled,
            connected: active_device
                .as_ref()
                .is_some_and(|device| device.connected),
            active_profile_id: state.active_profile_id,
            frontmost_app: state.frontmost_app.and_then(AppIdentity::label_or_fallback),
            selected_device_key: active_device_key,
            debug_mode: state.debug_mode,
            debug_log: state.debug_log,
        },
    }
}

struct KnownDeviceSeed<'a> {
    key: &'a str,
    display_name: &'a str,
    product_ids: &'a [u16],
    aliases: &'a [&'a str],
    gesture_cids: &'a [u16],
    ui_layout: &'a str,
    image_asset: &'a str,
    dpi_range: (u16, u16),
}

struct HotspotSpec<'a> {
    control: LogicalControl,
    label: &'a str,
    summary_type: HotspotSummaryType,
    norm: (f32, f32),
    label_side: LabelSide,
    label_offset: (i32, i32),
    is_hscroll: bool,
}

fn action(id: &str, label: &str, category: &str) -> ActionDefinition {
    ActionDefinition {
        id: id.to_string(),
        label: label.to_string(),
        category: category.to_string(),
    }
}

fn catalog_app(
    id: &str,
    label: &str,
    icon_asset: Option<&str>,
    matchers: &[(AppMatcherKind, &str)],
) -> CatalogApp {
    let mut app_matchers = Vec::new();
    for (kind, value) in matchers {
        push_unique_matcher(&mut app_matchers, *kind, value);
    }

    CatalogApp {
        id: id.to_string(),
        label: label.to_string(),
        icon_asset: icon_asset.map(str::to_string),
        matchers: app_matchers,
    }
}

fn known_app(executable: &str, label: &str, icon_asset: Option<&str>) -> KnownApp {
    KnownApp {
        executable: executable.to_string(),
        label: label.to_string(),
        icon_asset: icon_asset.map(str::to_string),
    }
}

fn known_device(spec: KnownDeviceSeed<'_>) -> KnownDeviceSpec {
    KnownDeviceSpec {
        key: spec.key.to_string(),
        display_name: spec.display_name.to_string(),
        product_ids: spec.product_ids.to_vec(),
        aliases: spec
            .aliases
            .iter()
            .map(|alias| (*alias).to_string())
            .collect(),
        gesture_cids: spec.gesture_cids.to_vec(),
        ui_layout: spec.ui_layout.to_string(),
        image_asset: spec.image_asset.to_string(),
        supported_controls: LogicalControl::all(),
        dpi_min: spec.dpi_range.0,
        dpi_max: spec.dpi_range.1,
    }
}

fn non_empty_name(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn normalize_profile_in_place(profile: &mut Profile) {
    profile.bindings = normalize_bindings(std::mem::take(&mut profile.bindings));
    if profile.label.trim().is_empty() {
        profile.label = profile.id.clone();
    }
}

fn normalize_optional_text(value: &mut Option<String>) {
    if value
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        *value = None;
    }
}

fn normalize_managed_device(device: &mut ManagedDevice, valid_profile_ids: &BTreeSet<String>) {
    if device.display_name.trim().is_empty() {
        device.display_name = known_device_spec_by_key(&device.model_key)
            .map(|spec| spec.display_name)
            .unwrap_or_else(|| device.model_key.clone());
    }

    normalize_optional_text(&mut device.nickname);
    normalize_optional_text(&mut device.profile_id);
    if device
        .profile_id
        .as_deref()
        .is_some_and(|profile_id| !valid_profile_ids.contains(profile_id))
    {
        device.profile_id = None;
    }

    normalize_optional_text(&mut device.identity_key);
    normalize_device_settings(Some(&device.model_key), &mut device.settings);
}

fn managed_device_display_name(managed: &ManagedDevice, live_product_name: Option<&str>) -> String {
    managed
        .nickname
        .as_deref()
        .and_then(|nickname| non_empty_name(Some(nickname)))
        .or_else(|| non_empty_name(live_product_name))
        .unwrap_or_else(|| managed.display_name.clone())
}

fn managed_device_transport(managed: &ManagedDevice, live: Option<&DeviceInfo>) -> Option<String> {
    live.and_then(|device| device.transport.clone())
        .or_else(|| managed.last_seen_transport.clone())
}

fn managed_device_source(live: Option<&DeviceInfo>) -> Option<String> {
    live.and_then(|device| device.source.clone())
        .or_else(|| Some("managed".to_string()))
}

fn managed_device_fingerprint(
    managed: &ManagedDevice,
    live: Option<&DeviceInfo>,
) -> DeviceFingerprint {
    live.map(|device| device.fingerprint.clone())
        .unwrap_or_else(|| DeviceFingerprint {
            identity_key: managed.identity_key.clone(),
            ..DeviceFingerprint::default()
        })
}

pub fn normalize_app_match_value(kind: AppMatcherKind, value: &str) -> String {
    let trimmed = value.trim();
    match kind {
        AppMatcherKind::Executable => Path::new(trimmed.replace('\\', "/").as_str())
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_else(|| trimmed.to_ascii_lowercase()),
        AppMatcherKind::ExecutablePath => trimmed.replace('\\', "/").to_ascii_lowercase(),
        AppMatcherKind::BundleId | AppMatcherKind::PackageFamilyName => {
            trimmed.to_ascii_lowercase()
        }
    }
}

fn push_unique_matcher(matchers: &mut Vec<AppMatcher>, kind: AppMatcherKind, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }

    let normalized = normalize_app_match_value(kind, trimmed);
    if matchers.iter().any(|matcher| {
        matcher.kind == kind && normalize_app_match_value(kind, &matcher.value) == normalized
    }) {
        return;
    }

    matchers.push(AppMatcher {
        kind,
        value: trimmed.to_string(),
    });
}

pub fn hydrate_identity_key(product_id: Option<u16>, fingerprint: &mut DeviceFingerprint) {
    if fingerprint.identity_key.is_some() {
        return;
    }

    fingerprint.identity_key = fingerprint
        .serial_number
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|serial| format!("serial:{:04x}:{}", product_id.unwrap_or_default(), serial,))
        .or_else(|| {
            fingerprint.location_id.map(|location_id| {
                format!(
                    "location:{:04x}:{location_id:08x}:{:04x}:{:04x}",
                    product_id.unwrap_or_default(),
                    fingerprint.usage_page.unwrap_or_default(),
                    fingerprint.usage.unwrap_or_default(),
                )
            })
        })
        .or_else(|| {
            fingerprint
                .hid_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|path| format!("path:{path}"))
        })
        .or_else(|| {
            match (
                fingerprint.interface_number,
                fingerprint.usage_page,
                fingerprint.usage,
            ) {
                (Some(interface_number), Some(usage_page), Some(usage)) => Some(format!(
                    "interface:{:04x}:{interface_number}:{usage_page:04x}:{usage:04x}",
                    product_id.unwrap_or_default(),
                )),
                _ => None,
            }
        });
}

fn live_device_key(model_key: &str, fingerprint: &DeviceFingerprint) -> String {
    fingerprint
        .identity_key
        .clone()
        .unwrap_or_else(|| model_key.to_string())
}

fn normalize_name(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn hotspot(spec: HotspotSpec<'_>) -> DeviceHotspot {
    DeviceHotspot {
        control: spec.control,
        label: spec.label.to_string(),
        summary_type: spec.summary_type,
        norm_x: spec.norm.0,
        norm_y: spec.norm.1,
        label_side: spec.label_side,
        label_off_x: spec.label_offset.0,
        label_off_y: spec.label_offset.1,
        is_hscroll: spec.is_hscroll,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app_identity(executable: &str) -> AppIdentity {
        AppIdentity {
            label: Some(executable.to_string()),
            executable: Some(executable.to_string()),
            ..AppIdentity::default()
        }
    }

    #[test]
    fn config_round_trip_preserves_defaults() {
        let config = default_config();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let decoded: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, config);
    }

    #[test]
    fn profile_resolution_prefers_matching_app() {
        let mut config = default_config();
        config.upsert_profile(Profile {
            id: "vscode".to_string(),
            label: "VS Code".to_string(),
            app_matchers: vec![AppMatcher {
                kind: AppMatcherKind::Executable,
                value: "Code.exe".to_string(),
            }],
            bindings: default_profile_bindings(),
        });

        assert!(config.sync_active_profile_for_app(Some(&test_app_identity("code.exe"))));
        assert_eq!(config.active_profile_id, "vscode");
        assert!(config.sync_active_profile_for_app(Some(&test_app_identity("Finder"))));
        assert_eq!(config.active_profile_id, "default");
    }

    #[test]
    fn binding_lookup_uses_control_identity() {
        let profile = default_profile();
        let binding = profile.binding_for(LogicalControl::Back).unwrap();
        assert_eq!(binding.action_id, "browser_back");
    }

    #[test]
    fn ensure_invariants_migrates_legacy_default_profile_bindings_once() {
        let mut config = default_config();
        config.version = 3;
        config.profiles[0].bindings = legacy_default_profile_bindings_v3();

        config.ensure_invariants();

        assert_eq!(config.version, 4);
        assert_eq!(
            config.profiles[0]
                .binding_for(LogicalControl::Back)
                .map(|binding| binding.action_id.as_str()),
            Some("browser_back")
        );
        assert_eq!(
            config.profiles[0]
                .binding_for(LogicalControl::Forward)
                .map(|binding| binding.action_id.as_str()),
            Some("browser_forward")
        );
    }

    #[test]
    fn device_metadata_and_dpi_clamp_match_catalog() {
        let catalog = default_device_catalog();
        let device = catalog
            .iter()
            .find(|device| device.key == "mx_master_3s")
            .unwrap();
        assert_eq!(device.ui_layout, "mx_master");
        assert_eq!(clamp_dpi(Some(device), 9000), 8000);
        assert_eq!(clamp_dpi(Some(device), 100), 200);
    }

    #[test]
    fn connected_device_info_prefers_live_product_name_for_display() {
        let device = build_connected_device_info(
            Some(0xB023),
            Some("MX Master 3 Mac"),
            Some("Bluetooth Low Energy"),
            Some("iokit"),
            Some(100),
            1000,
            DeviceFingerprint::default(),
        );

        assert_eq!(device.model_key, "mx_master_3");
        assert_eq!(device.display_name, "MX Master 3 Mac");
        assert_eq!(device.product_name.as_deref(), Some("MX Master 3 Mac"));
    }

    #[test]
    fn managed_device_info_prefers_live_product_name_without_nickname() {
        let managed = ManagedDevice {
            id: "mx_master_3-1".to_string(),
            model_key: "mx_master_3".to_string(),
            display_name: "MX Master 3".to_string(),
            nickname: None,
            profile_id: None,
            identity_key: None,
            settings: default_device_settings(),
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_transport: Some("Bluetooth Low Energy".to_string()),
        };
        let live = build_connected_device_info(
            Some(0xB023),
            Some("MX Master 3 Mac"),
            Some("Bluetooth Low Energy"),
            Some("iokit"),
            Some(100),
            1000,
            DeviceFingerprint::default(),
        );

        let merged = build_managed_device_info(&managed, Some(&live));

        assert_eq!(merged.display_name, "MX Master 3 Mac");
        assert_eq!(merged.product_name.as_deref(), Some("MX Master 3 Mac"));
    }

    #[test]
    fn profile_resolution_prefers_device_assignment() {
        let mut config = default_config();
        config.upsert_profile(Profile {
            id: "vscode".to_string(),
            label: "VS Code".to_string(),
            app_matchers: vec![AppMatcher {
                kind: AppMatcherKind::Executable,
                value: "Code.exe".to_string(),
            }],
            bindings: default_profile_bindings(),
        });
        config.managed_devices.push(ManagedDevice {
            id: "mx_master_3-1".to_string(),
            model_key: "mx_master_3".to_string(),
            display_name: "MX Master 3 Mac".to_string(),
            nickname: None,
            profile_id: Some("default".to_string()),
            identity_key: None,
            settings: default_device_settings(),
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_transport: None,
        });

        assert!(!config.sync_active_profile(Some("default"), Some(&test_app_identity("Code.exe")),));
        assert_eq!(config.active_profile_id, "default");
    }

    #[test]
    fn deleting_profile_clears_managed_device_assignment() {
        let mut config = default_config();
        config.upsert_profile(Profile {
            id: "vscode".to_string(),
            label: "VS Code".to_string(),
            app_matchers: vec![AppMatcher {
                kind: AppMatcherKind::Executable,
                value: "Code.exe".to_string(),
            }],
            bindings: default_profile_bindings(),
        });
        config.managed_devices.push(ManagedDevice {
            id: "mx_master_3-1".to_string(),
            model_key: "mx_master_3".to_string(),
            display_name: "MX Master 3 Mac".to_string(),
            nickname: None,
            profile_id: Some("vscode".to_string()),
            identity_key: None,
            settings: default_device_settings(),
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_transport: None,
        });

        assert!(config.delete_profile("vscode"));
        assert_eq!(config.managed_devices[0].profile_id, None);
    }
}
