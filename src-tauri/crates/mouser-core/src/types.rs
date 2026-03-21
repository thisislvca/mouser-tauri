use std::{collections::BTreeSet, path::Path};

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
    SmartshiftToggle,
    MissionControlButton,
    SmartZoomButton,
    PrecisionModeButton,
    DpiButton,
    EmojiButton,
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
            Self::SmartshiftToggle,
            Self::MissionControlButton,
            Self::SmartZoomButton,
            Self::PrecisionModeButton,
            Self::DpiButton,
            Self::EmojiButton,
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
            Self::SmartshiftToggle => "SmartShift toggle",
            Self::MissionControlButton => "Mission Control button",
            Self::SmartZoomButton => "Smart Zoom button",
            Self::PrecisionModeButton => "Precision mode button",
            Self::DpiButton => "DPI button",
            Self::EmojiButton => "Emoji button",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DebugLogGroup {
    Runtime,
    HookRouting,
    Gestures,
    ThumbWheel,
    Hid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DebugLogGroups {
    pub runtime: bool,
    pub hook_routing: bool,
    pub gestures: bool,
    pub thumb_wheel: bool,
    pub hid: bool,
}

impl DebugLogGroups {
    pub fn enabled(&self, group: DebugLogGroup) -> bool {
        match group {
            DebugLogGroup::Runtime => self.runtime,
            DebugLogGroup::HookRouting => self.hook_routing,
            DebugLogGroup::Gestures => self.gestures,
            DebugLogGroup::ThumbWheel => self.thumb_wheel,
            DebugLogGroup::Hid => self.hid,
        }
    }
}

impl Default for DebugLogGroups {
    fn default() -> Self {
        crate::defaults::default_debug_log_groups()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub start_minimized: bool,
    pub start_at_login: bool,
    pub appearance_mode: AppearanceMode,
    pub debug_mode: bool,
    #[serde(default = "crate::defaults::default_debug_log_groups")]
    pub debug_log_groups: DebugLogGroups,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSettings {
    pub dpi: u16,
    pub invert_horizontal_scroll: bool,
    pub invert_vertical_scroll: bool,
    #[serde(default)]
    pub macos_thumb_wheel_simulate_trackpad: bool,
    #[serde(default = "crate::defaults::default_macos_thumb_wheel_trackpad_hold_timeout_ms")]
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
        self.bindings = crate::defaults::normalize_bindings(std::mem::take(&mut self.bindings));
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
    #[serde(default = "crate::defaults::default_device_settings")]
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
            self.profiles.push(crate::defaults::default_profile());
        }

        if !self.profiles.iter().any(|profile| profile.id == "default") {
            self.profiles.insert(0, crate::defaults::default_profile());
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
                if default_profile.bindings == crate::defaults::legacy_default_profile_bindings_v3()
                {
                    default_profile.bindings = crate::defaults::default_profile_bindings();
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

        crate::catalog::normalize_device_settings(None, &mut self.device_defaults);
        let valid_profile_ids = self
            .profiles
            .iter()
            .map(|profile| profile.id.clone())
            .collect::<BTreeSet<_>>();

        let mut seen_managed_ids = BTreeSet::new();
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
    #[serde(default = "crate::defaults::default_action_supported")]
    pub supported: bool,
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
    DesktopEntry,
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
    #[serde(default = "crate::defaults::default_device_settings")]
    pub settings: DeviceSettings,
    pub created_at_ms: u64,
    pub last_seen_at_ms: Option<u64>,
    pub last_seen_transport: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DeviceSupportLevel {
    Full,
    Partial,
    Experimental,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSupportMatrix {
    pub level: DeviceSupportLevel,
    pub supports_battery_status: bool,
    pub supports_dpi_configuration: bool,
    pub has_interactive_layout: bool,
    pub notes: Vec<String>,
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
    #[serde(default)]
    pub controls: Vec<DeviceControlSpec>,
    pub support: DeviceSupportMatrix,
    pub dpi_min: u16,
    pub dpi_max: u16,
    #[serde(default)]
    pub dpi_inferred: bool,
    #[serde(default)]
    pub dpi_source_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DeviceBatteryKind {
    Percentage,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceBatteryInfo {
    pub kind: DeviceBatteryKind,
    #[serde(default)]
    pub percentage: Option<u8>,
    pub label: String,
    #[serde(default)]
    pub source_feature: Option<String>,
    #[serde(default)]
    pub raw_capabilities: Vec<u8>,
    #[serde(default)]
    pub raw_status: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DeviceControlCaptureKind {
    OsButton,
    OsHscroll,
    Gesture,
    ReprogButton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DeviceControlDefaultSource {
    Explicit,
    Recommendation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceControlSpec {
    pub control: LogicalControl,
    pub label: String,
    pub capture_kind: DeviceControlCaptureKind,
    pub default_action_id: String,
    #[serde(default)]
    pub factory_default_action_id: Option<String>,
    #[serde(default)]
    pub factory_default_label: Option<String>,
    #[serde(default)]
    pub factory_default_source: Option<DeviceControlDefaultSource>,
    pub recommended_action_ids: Vec<String>,
    pub source_slot_ids: Vec<String>,
    pub source_slot_names: Vec<String>,
    pub reprog_cids: Vec<u16>,
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
    #[serde(default)]
    pub controls: Vec<DeviceControlSpec>,
    pub support: DeviceSupportMatrix,
    pub gesture_cids: Vec<u16>,
    pub dpi_min: u16,
    pub dpi_max: u16,
    #[serde(default)]
    pub dpi_inferred: bool,
    #[serde(default)]
    pub dpi_source_kind: Option<String>,
    pub connected: bool,
    #[serde(default)]
    pub battery: Option<DeviceBatteryInfo>,
    pub battery_level: Option<u8>,
    pub current_dpi: u16,
    #[serde(default)]
    pub fingerprint: DeviceFingerprint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DeviceMatchKind {
    Identity,
    ModelFallback,
    Unmanaged,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DeviceAttributionStatus {
    Ready,
    ModelFallback,
    Ambiguous,
    #[default]
    Unmanaged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRoutingEntry {
    pub live_device_key: String,
    pub live_model_key: String,
    pub live_display_name: String,
    pub live_identity_key: Option<String>,
    pub managed_device_key: Option<String>,
    pub managed_display_name: Option<String>,
    pub device_profile_id: Option<String>,
    pub resolved_profile_id: Option<String>,
    pub match_kind: DeviceMatchKind,
    pub is_active_target: bool,
    #[serde(default)]
    pub hook_eligible: bool,
    #[serde(default)]
    pub attribution_status: DeviceAttributionStatus,
    #[serde(default)]
    pub source_hints: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRoutingSnapshot {
    pub entries: Vec<DeviceRoutingEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRoutingChangeKind {
    Connected,
    Disconnected,
    Reassigned,
    ActiveTargetChanged,
    ResolvedProfileChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRoutingChange {
    pub kind: DeviceRoutingChangeKind,
    pub live_device_key: String,
    pub managed_device_key: Option<String>,
    pub resolved_profile_id: Option<String>,
    pub match_kind: Option<DeviceMatchKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRoutingEvent {
    pub snapshot: DeviceRoutingSnapshot,
    pub changes: Vec<DeviceRoutingChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
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
    #[serde(default)]
    pub device_routing: DeviceRoutingSnapshot,
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

pub(crate) fn push_unique_matcher(
    matchers: &mut Vec<AppMatcher>,
    kind: AppMatcherKind,
    value: &str,
) {
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

pub(crate) fn non_empty_name(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

pub(crate) fn normalize_profile_in_place(profile: &mut Profile) {
    profile.bindings = crate::defaults::normalize_bindings(std::mem::take(&mut profile.bindings));
    if profile.label.trim().is_empty() {
        profile.label = profile.id.clone();
    }
}

pub(crate) fn normalize_optional_text(value: &mut Option<String>) {
    if value
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        *value = None;
    }
}

pub(crate) fn normalize_managed_device(
    device: &mut ManagedDevice,
    valid_profile_ids: &BTreeSet<String>,
) {
    if device.display_name.trim().is_empty() {
        device.display_name = crate::catalog::known_device_spec_by_key(&device.model_key)
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
    crate::catalog::normalize_device_settings(Some(&device.model_key), &mut device.settings);
}

pub(crate) fn managed_device_display_name(
    managed: &ManagedDevice,
    live_product_name: Option<&str>,
) -> String {
    managed
        .nickname
        .as_deref()
        .and_then(|nickname| non_empty_name(Some(nickname)))
        .or_else(|| non_empty_name(live_product_name))
        .unwrap_or_else(|| managed.display_name.clone())
}

pub(crate) fn managed_device_transport(
    managed: &ManagedDevice,
    live: Option<&DeviceInfo>,
) -> Option<String> {
    live.and_then(|device| device.transport.clone())
        .or_else(|| managed.last_seen_transport.clone())
}

pub(crate) fn managed_device_source(live: Option<&DeviceInfo>) -> Option<String> {
    live.and_then(|device| device.source.clone())
        .or_else(|| Some("managed".to_string()))
}

pub(crate) fn managed_device_fingerprint(
    managed: &ManagedDevice,
    live: Option<&DeviceInfo>,
) -> DeviceFingerprint {
    live.map(|device| device.fingerprint.clone())
        .unwrap_or_else(|| DeviceFingerprint {
            identity_key: managed.identity_key.clone(),
            ..DeviceFingerprint::default()
        })
}

pub(crate) fn normalize_name(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
