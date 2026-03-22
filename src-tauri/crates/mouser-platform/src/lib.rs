use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
#[cfg(target_os = "macos")]
use mouser_core::build_connected_device_info;
#[cfg(any(target_os = "linux", target_os = "macos", test))]
use mouser_core::AppDiscoverySource;
use mouser_core::{
    clamp_dpi, default_device_settings, default_known_apps_ref, default_layouts_ref,
    known_device_specs_ref, normalize_app_match_value, AppConfig, AppIdentity, AppMatcherKind,
    Binding, DebugEventKind, DebugLogGroup, DebugLogGroups, DeviceBatteryInfo, DeviceControlSpec,
    DeviceFingerprint, DeviceInfo, DeviceLayout, DeviceSettings, InstalledApp, KnownApp,
    LogicalControl, Settings,
};
#[cfg(target_os = "macos")]
use std::process::Command;
use thiserror::Error;

mod common;
mod gesture;
mod hidpp;
mod linux_backend;
mod macos_hook;
mod macos_iokit;
mod windows_backend;

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
pub struct HookDeviceRoute {
    pub managed_device_key: String,
    pub resolved_profile_id: String,
    pub live_device: DeviceInfo,
    pub bindings: Vec<Binding>,
    pub device_settings: DeviceSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookBackendSettings {
    pub debug_mode: bool,
    pub debug_log_groups: DebugLogGroups,
    pub invert_horizontal_scroll: bool,
    pub invert_vertical_scroll: bool,
    pub macos_thumb_wheel_simulate_trackpad: bool,
    pub macos_thumb_wheel_trackpad_hold_timeout_ms: u32,
    pub gesture_threshold: u16,
    pub gesture_deadzone: u16,
    pub gesture_timeout_ms: u32,
    pub gesture_cooldown_ms: u32,
    pub device_model_key: Option<String>,
    pub device_identity_key: Option<String>,
    pub device_controls: Vec<DeviceControlSpec>,
    pub routes: Vec<HookDeviceRoute>,
}

impl HookBackendSettings {
    pub fn from_app_and_device(
        settings: &Settings,
        device_settings: &DeviceSettings,
        active_device: Option<&DeviceInfo>,
    ) -> Self {
        let device_model_key = active_device.map(|device| device.model_key.clone());
        let device_identity_key = active_device.and_then(|device| {
            device
                .fingerprint
                .identity_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
        Self {
            debug_mode: settings.debug_mode,
            debug_log_groups: settings.debug_log_groups.clone(),
            invert_horizontal_scroll: device_settings.invert_horizontal_scroll,
            invert_vertical_scroll: device_settings.invert_vertical_scroll,
            macos_thumb_wheel_simulate_trackpad: cfg!(target_os = "macos")
                && device_settings.macos_thumb_wheel_simulate_trackpad
                && supports_macos_thumb_wheel_trackpad_model(device_model_key.as_deref()),
            macos_thumb_wheel_trackpad_hold_timeout_ms: device_settings
                .macos_thumb_wheel_trackpad_hold_timeout_ms,
            gesture_threshold: device_settings.gesture_threshold,
            gesture_deadzone: device_settings.gesture_deadzone,
            gesture_timeout_ms: device_settings.gesture_timeout_ms,
            gesture_cooldown_ms: device_settings.gesture_cooldown_ms,
            device_model_key,
            device_identity_key,
            device_controls: active_device
                .map(|device| device.controls.clone())
                .unwrap_or_default(),
            routes: Vec::new(),
        }
    }

    pub fn from_routes(settings: &Settings, routes: Vec<HookDeviceRoute>) -> Self {
        let primary = routes.first();
        Self {
            debug_mode: settings.debug_mode,
            debug_log_groups: settings.debug_log_groups.clone(),
            invert_horizontal_scroll: primary
                .map(|route| route.device_settings.invert_horizontal_scroll)
                .unwrap_or(false),
            invert_vertical_scroll: primary
                .map(|route| route.device_settings.invert_vertical_scroll)
                .unwrap_or(false),
            macos_thumb_wheel_simulate_trackpad: primary
                .map(|route| {
                    cfg!(target_os = "macos")
                        && route.device_settings.macos_thumb_wheel_simulate_trackpad
                        && supports_macos_thumb_wheel_trackpad_model(Some(
                            route.live_device.model_key.as_str(),
                        ))
                })
                .unwrap_or(false),
            macos_thumb_wheel_trackpad_hold_timeout_ms: primary
                .map(|route| {
                    route
                        .device_settings
                        .macos_thumb_wheel_trackpad_hold_timeout_ms
                })
                .unwrap_or_else(|| {
                    default_device_settings().macos_thumb_wheel_trackpad_hold_timeout_ms
                }),
            gesture_threshold: primary
                .map(|route| route.device_settings.gesture_threshold)
                .unwrap_or_else(|| default_device_settings().gesture_threshold),
            gesture_deadzone: primary
                .map(|route| route.device_settings.gesture_deadzone)
                .unwrap_or_else(|| default_device_settings().gesture_deadzone),
            gesture_timeout_ms: primary
                .map(|route| route.device_settings.gesture_timeout_ms)
                .unwrap_or_else(|| default_device_settings().gesture_timeout_ms),
            gesture_cooldown_ms: primary
                .map(|route| route.device_settings.gesture_cooldown_ms)
                .unwrap_or_else(|| default_device_settings().gesture_cooldown_ms),
            device_model_key: primary.map(|route| route.live_device.model_key.clone()),
            device_identity_key: primary.and_then(|route| {
                route
                    .live_device
                    .fingerprint
                    .identity_key
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            }),
            device_controls: primary
                .map(|route| route.live_device.controls.clone())
                .unwrap_or_default(),
            routes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookEvent {
    pub control: LogicalControl,
    pub pressed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookBackendEvent {
    pub kind: DebugEventKind,
    pub message: String,
}

pub(crate) fn backend_debug_logging_enabled(
    debug_mode: bool,
    debug_log_groups: &DebugLogGroups,
    group: DebugLogGroup,
) -> bool {
    debug_mode && debug_log_groups.enabled(group)
}

pub fn emit_backend_console_log(
    backend: &str,
    kind: DebugEventKind,
    group: DebugLogGroup,
    message: &str,
) {
    let kind = match kind {
        DebugEventKind::Info => "info",
        DebugEventKind::Warning => "warning",
        DebugEventKind::Gesture => "gesture",
    };
    let group = match group {
        DebugLogGroup::Runtime => "runtime",
        DebugLogGroup::HookRouting => "hook-routing",
        DebugLogGroup::Gestures => "gestures",
        DebugLogGroup::ThumbWheel => "thumb-wheel",
        DebugLogGroup::Hid => "hid",
    };
    eprintln!("[mouser][{backend}][{group}][{kind}] {message}");
}

pub(crate) fn supports_macos_thumb_wheel_trackpad_model(model_key: Option<&str>) -> bool {
    model_key
        .is_some_and(|model_key| model_key == "mx_master" || model_key.starts_with("mx_master_"))
}

pub(crate) const HOOK_EVENT_BUFFER_LIMIT: usize = 128;

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("{0}")]
    Unsupported(&'static str),
    #[error("{0}")]
    Message(String),
    #[error("io error at {path}: {message}")]
    Io { path: String, message: String },
}

pub fn current_platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "other"
    }
}

pub fn host_hidapi_available() -> bool {
    cfg!(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "windows"
    ))
}

pub fn host_iokit_available() -> bool {
    cfg!(target_os = "macos")
}

pub trait HookBackend: Send + Sync {
    fn backend_id(&self) -> &'static str;
    fn capabilities(&self) -> HookCapabilities;
    fn configure(&self, settings: &HookBackendSettings, enabled: bool)
        -> Result<(), PlatformError>;
    fn execute_action(&self, action_id: &str) -> Result<(), PlatformError>;
    fn drain_events(&self) -> Vec<HookBackendEvent>;
}

pub trait HidBackend: Send + Sync {
    fn backend_id(&self) -> &'static str;
    fn capabilities(&self) -> HidCapabilities;
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, PlatformError>;
    fn set_device_dpi(&self, device_key: &str, dpi: u16) -> Result<(), PlatformError>;
}

pub trait AppFocusBackend: Send + Sync {
    fn backend_id(&self) -> &'static str;
    fn current_frontmost_app(&self) -> Result<Option<AppIdentity>, PlatformError>;
}

pub trait AppDiscoveryBackend: Send + Sync {
    fn backend_id(&self) -> &'static str;
    fn discover_apps(&self) -> Result<Vec<InstalledApp>, PlatformError>;
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

    fn load_or_recover(&self) -> (AppConfig, Option<String>) {
        match self.load() {
            Ok(config) => (config, None),
            Err(error) => (
                mouser_core::default_config(),
                Some(format!("Failed to load config: {error}. Loaded defaults.")),
            ),
        }
    }
}

pub fn load_native_app_icon(source_path: &str) -> Result<Option<String>, PlatformError> {
    #[cfg(target_os = "macos")]
    {
        if let Some(icon) = macos::load_native_app_icon(source_path)? {
            return Ok(Some(icon));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(icon) = windows_backend::load_native_app_icon(source_path)? {
            return Ok(Some(icon));
        }
    }

    local_image_data_url(source_path)
}

fn local_image_data_url(source_path: &str) -> Result<Option<String>, PlatformError> {
    let path = Path::new(source_path.trim());
    let Some(extension) = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
    else {
        return Ok(None);
    };
    let Some(mime_type) = image_mime_type(&extension) else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }

    let bytes = fs::read(path).map_err(|error| PlatformError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;
    if bytes.is_empty() {
        return Ok(None);
    }

    Ok(Some(format!(
        "data:{mime_type};base64,{}",
        BASE64_STANDARD.encode(bytes)
    )))
}

fn image_mime_type(extension: &str) -> Option<&'static str> {
    match extension {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "svg" => Some("image/svg+xml"),
        "ico" => Some("image/x-icon"),
        "icns" => Some("image/icns"),
        _ => None,
    }
}

pub(crate) fn horizontal_scroll_control(delta: i32) -> Option<LogicalControl> {
    match delta.cmp(&0) {
        std::cmp::Ordering::Greater => Some(LogicalControl::HscrollRight),
        std::cmp::Ordering::Less => Some(LogicalControl::HscrollLeft),
        std::cmp::Ordering::Equal => None,
    }
}

pub(crate) fn push_bounded_hook_event(
    events: &mut Vec<HookBackendEvent>,
    kind: DebugEventKind,
    message: impl Into<String>,
) {
    events.push(HookBackendEvent {
        kind,
        message: message.into(),
    });
    if events.len() > HOOK_EVENT_BUFFER_LIMIT {
        let excess = events.len() - HOOK_EVENT_BUFFER_LIMIT;
        events.drain(0..excess);
    }
}

pub(crate) fn dedupe_installed_apps(apps: Vec<InstalledApp>) -> Vec<InstalledApp> {
    let mut deduped = Vec::<DedupedInstalledApp>::new();

    for app in apps {
        let identity = NormalizedAppIdentity::new(&app.identity);
        if !identity.has_matchers {
            continue;
        }

        if let Some(existing) = deduped
            .iter_mut()
            .find(|existing| existing.identity.overlaps(&identity))
        {
            merge_installed_app(&mut existing.app, app);
            existing.identity = NormalizedAppIdentity::new(&existing.app.identity);
        } else {
            deduped.push(DedupedInstalledApp { app, identity });
        }
    }

    let mut deduped = deduped
        .into_iter()
        .map(|mut entry| {
            let app = &mut entry.app;
            app.source_kinds.sort();
            app.source_kinds.dedup();
            entry.app
        })
        .collect::<Vec<_>>();

    deduped.sort_by(|left, right| {
        left.identity
            .label_or_fallback()
            .unwrap_or_default()
            .cmp(&right.identity.label_or_fallback().unwrap_or_default())
    });

    deduped
}

struct DedupedInstalledApp {
    app: InstalledApp,
    identity: NormalizedAppIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct NormalizedMatcher {
    kind: AppMatcherKind,
    value: String,
}

#[derive(Clone)]
struct NormalizedAppIdentity {
    stable_id: String,
    has_matchers: bool,
    non_executable_matchers: std::collections::BTreeSet<NormalizedMatcher>,
}

impl NormalizedAppIdentity {
    fn new(identity: &AppIdentity) -> Self {
        let preferred_matchers = identity.preferred_matchers();
        let non_executable_matchers = preferred_matchers
            .iter()
            .filter(|matcher| matcher.kind != AppMatcherKind::Executable)
            .map(|matcher| NormalizedMatcher {
                kind: matcher.kind,
                value: normalize_app_match_value(matcher.kind, &matcher.value),
            })
            .collect();

        Self {
            stable_id: identity.stable_id(),
            has_matchers: !preferred_matchers.is_empty(),
            non_executable_matchers,
        }
    }

    fn overlaps(&self, other: &Self) -> bool {
        if self.stable_id == other.stable_id {
            return true;
        }

        self.non_executable_matchers
            .iter()
            .any(|matcher| other.non_executable_matchers.contains(matcher))
    }
}

fn merge_installed_app(existing: &mut InstalledApp, incoming: InstalledApp) {
    merge_identity(&mut existing.identity, incoming.identity);
    existing.source_kinds.extend(incoming.source_kinds);

    if should_replace_source_path(
        existing.source_path.as_deref(),
        incoming.source_path.as_deref(),
    ) {
        existing.source_path = incoming.source_path;
    }
}

fn merge_identity(existing: &mut AppIdentity, incoming: AppIdentity) {
    if preferred_text(existing.label.as_deref(), incoming.label.as_deref()) {
        existing.label = incoming.label;
    }

    fill_missing(&mut existing.executable, incoming.executable);
    fill_missing(&mut existing.executable_path, incoming.executable_path);
    fill_missing(&mut existing.bundle_id, incoming.bundle_id);
    fill_missing(
        &mut existing.package_family_name,
        incoming.package_family_name,
    );
}

fn preferred_text(current: Option<&str>, candidate: Option<&str>) -> bool {
    candidate_beats_current(current, candidate, label_quality)
}

fn label_quality(value: &str) -> usize {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return 0;
    }

    let mut score = trimmed.len();
    if trimmed.contains(' ') {
        score += 8;
    }
    if !trimmed.to_ascii_lowercase().ends_with(".exe") {
        score += 8;
    }
    score
}

fn should_replace_source_path(current: Option<&str>, candidate: Option<&str>) -> bool {
    candidate_beats_current(current, candidate, source_path_priority)
}

fn source_path_priority(path: &str) -> usize {
    let lower = path.trim().to_ascii_lowercase();
    if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp")
        || lower.ends_with(".bmp")
        || lower.ends_with(".svg")
        || lower.ends_with(".ico")
    {
        return 5;
    }
    if lower.ends_with(".exe") {
        return 4;
    }
    if lower.ends_with(".lnk") {
        return 3;
    }
    if lower.starts_with("shell:appsfolder\\") {
        return 2;
    }
    1
}

fn fill_missing(slot: &mut Option<String>, candidate: Option<String>) {
    if slot.is_none() {
        *slot = candidate;
    }
}

fn candidate_beats_current(
    current: Option<&str>,
    candidate: Option<&str>,
    score: impl Fn(&str) -> usize,
) -> bool {
    let Some(candidate) = candidate.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let Some(current) = current.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };

    score(candidate) > score(current)
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
            layouts: default_layouts_ref().to_vec(),
            devices: known_device_specs_ref()
                .iter()
                .cloned()
                .map(|spec| DeviceInfo {
                    key: spec.key.clone(),
                    model_key: spec.key,
                    display_name: spec.display_name.clone(),
                    nickname: None,
                    product_id: spec.product_ids.first().copied(),
                    product_name: Some(spec.display_name),
                    transport: None,
                    source: Some("catalog".to_string()),
                    ui_layout: spec.ui_layout,
                    image_asset: spec.image_asset,
                    supported_controls: spec.supported_controls,
                    controls: spec.controls,
                    support: spec.support,
                    gesture_cids: spec.gesture_cids,
                    dpi_min: spec.dpi_min,
                    dpi_max: spec.dpi_max,
                    dpi_inferred: spec.dpi_inferred,
                    dpi_source_kind: spec.dpi_source_kind,
                    connected: false,
                    battery: None,
                    battery_level: None,
                    current_dpi: 1000,
                    fingerprint: DeviceFingerprint::default(),
                })
                .collect(),
            apps: default_known_apps_ref().to_vec(),
        }
    }

    pub fn layouts(&self) -> &[DeviceLayout] {
        &self.layouts
    }

    pub fn known_apps_ref(&self) -> &[KnownApp] {
        &self.apps
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

#[cfg_attr(not(target_os = "macos"), allow(dead_code, unused_imports))]
pub mod macos {
    use super::*;
    use crate::hidpp::{self, HidppIo, BT_DEV_IDX};
    pub use crate::macos_hook::MacOsHookBackend;
    #[cfg(target_os = "macos")]
    pub use crate::macos_iokit::MacOsDeviceMonitor;
    #[cfg(target_os = "macos")]
    use crate::macos_iokit::{enumerate_iokit_infos, MacOsIoKitInfo, MacOsNativeHidDevice};

    #[cfg(target_os = "macos")]
    use block2::RcBlock;
    #[cfg(target_os = "macos")]
    use hidapi::{BusType, DeviceInfo as HidDeviceInfo, HidApi};
    #[cfg(target_os = "macos")]
    use mouser_core::{hydrate_identity_key, DeviceFingerprint};
    #[cfg(target_os = "macos")]
    use objc2::runtime::ProtocolObject;
    #[cfg(target_os = "macos")]
    use objc2_app_kit::NSWorkspace;
    #[cfg(target_os = "macos")]
    use objc2_app_kit::NSWorkspaceDidActivateApplicationNotification;
    #[cfg(target_os = "macos")]
    use objc2_foundation::{NSNotification, NSObjectProtocol};
    #[cfg(target_os = "macos")]
    use plist::Value as PlistValue;
    #[cfg(target_os = "macos")]
    use std::{
        collections::{BTreeMap, BTreeSet},
        ptr::NonNull,
        sync::Mutex,
        time::{Duration, Instant},
    };

    const LOGI_VID: u16 = 0x046D;
    #[cfg(target_os = "macos")]
    const BATTERY_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
    #[cfg(target_os = "macos")]
    const DPI_VERIFY_DELAY: Duration = Duration::from_secs(1);

    pub struct MacOsHidBackend {
        #[cfg(target_os = "macos")]
        telemetry_cache: Mutex<BTreeMap<String, DeviceTelemetryCacheEntry>>,
    }
    pub struct MacOsAppFocusBackend;
    pub struct MacOsAppDiscoveryBackend;

    #[cfg(target_os = "macos")]
    #[allow(dead_code)]
    pub struct MacOsAppFocusMonitor {
        _notification_center: objc2::rc::Retained<objc2_foundation::NSNotificationCenter>,
        _observer: objc2::rc::Retained<ProtocolObject<dyn NSObjectProtocol>>,
        _observer_block: RcBlock<dyn Fn(NonNull<NSNotification>)>,
    }

    #[cfg(target_os = "macos")]
    #[derive(Debug, Clone)]
    struct DeviceTelemetryCacheEntry {
        current_dpi: Option<u16>,
        battery: Option<DeviceBatteryInfo>,
        last_battery_probe_at: Instant,
        verify_after: Option<Instant>,
        connected: bool,
    }

    #[cfg(target_os = "macos")]
    #[derive(Debug, Clone)]
    struct DeviceTelemetrySnapshot {
        current_dpi: Option<u16>,
        battery: Option<DeviceBatteryInfo>,
    }

    #[cfg(target_os = "macos")]
    #[derive(Debug, Clone)]
    struct TelemetryProbePlan {
        should_probe: bool,
        cached: DeviceTelemetrySnapshot,
    }

    impl MacOsHidBackend {
        pub fn new() -> Self {
            Self {
                #[cfg(target_os = "macos")]
                telemetry_cache: Mutex::new(BTreeMap::new()),
            }
        }
    }

    impl Default for MacOsHidBackend {
        fn default() -> Self {
            Self::new()
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn load_native_app_icon(source_path: &str) -> Result<Option<String>, PlatformError> {
        let bundle_path = Path::new(source_path);
        if !bundle_path.exists() {
            return Ok(None);
        }

        let Some(icon_path) = resolve_bundle_icon_path(bundle_path)? else {
            return Ok(None);
        };

        encode_icon_as_data_url(&icon_path)
    }

    #[cfg(target_os = "macos")]
    impl MacOsAppFocusMonitor {
        pub fn new<F>(notify: F) -> Result<Self, PlatformError>
        where
            F: Fn(Option<AppIdentity>) + Send + Sync + 'static,
        {
            let notification_center = NSWorkspace::sharedWorkspace().notificationCenter();
            let observer_block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
                if let Ok(frontmost_app) = current_frontmost_app_identity() {
                    notify(frontmost_app);
                }
            });
            let observer = unsafe {
                notification_center.addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceDidActivateApplicationNotification),
                    None,
                    None,
                    &observer_block,
                )
            };

            Ok(Self {
                _notification_center: notification_center,
                _observer: observer,
                _observer_block: observer_block,
            })
        }
    }

    #[cfg(target_os = "macos")]
    impl MacOsHidBackend {
        fn telemetry_plan(&self, cache_key: &str, now: Instant) -> TelemetryProbePlan {
            let cache = self.telemetry_cache.lock().unwrap();
            let entry = cache.get(cache_key);
            TelemetryProbePlan {
                should_probe: should_probe_cached_telemetry(entry, now),
                cached: DeviceTelemetrySnapshot {
                    current_dpi: entry.and_then(|entry| entry.current_dpi),
                    battery: entry.and_then(|entry| entry.battery.clone()),
                },
            }
        }

        fn remember_device_telemetry(
            &self,
            cache_key: String,
            current_dpi: u16,
            battery: Option<DeviceBatteryInfo>,
            now: Instant,
        ) {
            self.telemetry_cache.lock().unwrap().insert(
                cache_key,
                DeviceTelemetryCacheEntry {
                    current_dpi: Some(current_dpi),
                    battery,
                    last_battery_probe_at: now,
                    verify_after: None,
                    connected: true,
                },
            );
        }

        fn note_connected_devices(&self, connected_cache_keys: &BTreeSet<String>) {
            let mut cache = self.telemetry_cache.lock().unwrap();
            for (cache_key, entry) in cache.iter_mut() {
                entry.connected = connected_cache_keys.contains(cache_key);
            }
        }

        fn note_dpi_write(&self, cache_key: String, dpi: u16, now: Instant) {
            let mut cache = self.telemetry_cache.lock().unwrap();
            let entry = cache.entry(cache_key).or_insert(DeviceTelemetryCacheEntry {
                current_dpi: None,
                battery: None,
                last_battery_probe_at: now,
                verify_after: None,
                connected: true,
            });
            entry.current_dpi = Some(dpi);
            entry.verify_after = Some(now + DPI_VERIFY_DELAY);
            entry.connected = true;
        }
    }

    impl HidBackend for MacOsHidBackend {
        fn backend_id(&self) -> &'static str {
            if cfg!(target_os = "macos") {
                "macos-iokit+hidapi"
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
                let mut devices = Vec::new();
                let mut issues = Vec::new();
                let mut connected_cache_keys = BTreeSet::new();
                let now = Instant::now();

                match enumerate_iokit_infos() {
                    Ok(infos) => {
                        for info in infos {
                            if let Some(device_info) =
                                self.build_iokit_device_info(&info, now, &mut connected_cache_keys)
                            {
                                push_unique_device(&mut devices, device_info);
                            }
                        }
                    }
                    Err(error) => issues.push(format!("iokit: {error}")),
                }

                match HidApi::new() {
                    Ok(api) => {
                        for info in vendor_hid_infos(&api) {
                            if let Some(device_info) = self.build_hidapi_device_info(
                                &api,
                                info,
                                now,
                                &mut connected_cache_keys,
                            ) {
                                push_unique_device(&mut devices, device_info);
                            }
                        }
                    }
                    Err(error) => issues.push(format!("hidapi: {error}")),
                }

                self.note_connected_devices(&connected_cache_keys);

                if devices.is_empty() && !issues.is_empty() {
                    return Err(PlatformError::Message(format!(
                        "failed to enumerate macOS HID devices: {}",
                        issues.join("; ")
                    )));
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
                let now = Instant::now();
                if let Ok(infos) = enumerate_iokit_infos() {
                    for info in infos {
                        let transport = iokit_transport_label(info.transport.as_deref());
                        let fingerprint = fingerprint_from_iokit_info(&info);
                        if !device_key_matches(
                            device_key,
                            Some(info.product_id),
                            info.product_string.as_deref(),
                            transport.as_deref(),
                            "iokit",
                            fingerprint.clone(),
                            dpi,
                        ) {
                            continue;
                        }

                        let Ok(device) = open_iokit_device(&info) else {
                            continue;
                        };
                        if set_hidpp_dpi(&device, dpi)? {
                            self.note_dpi_write(
                                telemetry_cache_key(
                                    Some(info.product_id),
                                    transport.as_deref(),
                                    &fingerprint,
                                ),
                                dpi,
                                now,
                            );
                            return Ok(());
                        }
                    }
                }

                if let Ok(api) = HidApi::new() {
                    for info in vendor_hid_infos(&api) {
                        let Ok(device) = info.open_device(&api) else {
                            continue;
                        };
                        let fingerprint = fingerprint_from_hid_info(info);
                        if device_key_matches(
                            device_key,
                            Some(info.product_id()),
                            info.product_string(),
                            Some(transport_label(info.bus_type())),
                            "hidapi",
                            fingerprint.clone(),
                            dpi,
                        ) && set_hidpp_dpi(&device, dpi)?
                        {
                            self.note_dpi_write(
                                telemetry_cache_key(
                                    Some(info.product_id()),
                                    Some(transport_label(info.bus_type())),
                                    &fingerprint,
                                ),
                                dpi,
                                now,
                            );
                            return Ok(());
                        }
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

        fn current_frontmost_app(&self) -> Result<Option<AppIdentity>, PlatformError> {
            #[cfg(not(target_os = "macos"))]
            {
                Err(PlatformError::Unsupported(
                    "live macOS app focus detection is only available on macOS",
                ))
            }

            #[cfg(target_os = "macos")]
            {
                current_frontmost_app_identity()
            }
        }
    }

    impl AppDiscoveryBackend for MacOsAppDiscoveryBackend {
        fn backend_id(&self) -> &'static str {
            if cfg!(target_os = "macos") {
                "macos-applications"
            } else {
                "macos-unsupported"
            }
        }

        fn discover_apps(&self) -> Result<Vec<InstalledApp>, PlatformError> {
            #[cfg(not(target_os = "macos"))]
            {
                Err(PlatformError::Unsupported(
                    "macOS app discovery is only available on macOS",
                ))
            }

            #[cfg(target_os = "macos")]
            {
                let mut apps = Vec::new();
                for root in macos_application_roots() {
                    collect_macos_apps_from_root(&root, &mut apps)?;
                }
                Ok(dedupe_installed_apps(apps))
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn macos_application_roots() -> Vec<PathBuf> {
        let mut roots = vec![
            PathBuf::from("/Applications"),
            PathBuf::from("/System/Applications"),
            PathBuf::from("/System/Applications/Utilities"),
        ];
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("Applications"));
        }
        roots
    }

    #[cfg(target_os = "macos")]
    fn collect_macos_apps_from_root(
        root: &Path,
        apps: &mut Vec<InstalledApp>,
    ) -> Result<(), PlatformError> {
        if !root.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(root).map_err(|error| PlatformError::Io {
            path: root.display().to_string(),
            message: error.to_string(),
        })? {
            let entry = entry.map_err(|error| PlatformError::Io {
                path: root.display().to_string(),
                message: error.to_string(),
            })?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("app") {
                continue;
            }

            if let Some(app) = read_macos_bundle(&path) {
                apps.push(app);
            }
        }

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn read_macos_bundle(path: &Path) -> Option<InstalledApp> {
        let plist_path = path.join("Contents").join("Info.plist");
        let plist = PlistValue::from_file(&plist_path).ok()?;
        let dict = plist.as_dictionary()?;
        if dict
            .get("LSBackgroundOnly")
            .and_then(PlistValue::as_boolean)
            .unwrap_or(false)
        {
            return None;
        }

        let label = dict
            .get("CFBundleDisplayName")
            .and_then(PlistValue::as_string)
            .or_else(|| dict.get("CFBundleName").and_then(PlistValue::as_string))
            .map(str::trim)
            .filter(|value: &&str| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                path.file_stem()
                    .map(|value| value.to_string_lossy().to_string())
            })?;
        if label.to_ascii_lowercase().contains("helper") {
            return None;
        }

        let executable = dict
            .get("CFBundleExecutable")
            .and_then(PlistValue::as_string)
            .map(str::trim)
            .filter(|value: &&str| !value.is_empty())
            .map(str::to_string);
        let executable_path = executable
            .as_deref()
            .map(|name| path.join("Contents").join("MacOS").join(name))
            .filter(|path: &PathBuf| path.exists())
            .map(|path: PathBuf| path.to_string_lossy().to_string());

        Some(InstalledApp {
            identity: AppIdentity {
                label: Some(label),
                executable,
                executable_path,
                bundle_id: dict
                    .get("CFBundleIdentifier")
                    .and_then(PlistValue::as_string)
                    .map(str::trim)
                    .filter(|value: &&str| !value.is_empty())
                    .map(str::to_string),
                package_family_name: None,
            },
            source_kinds: vec![AppDiscoverySource::ApplicationBundle],
            source_path: Some(path.to_string_lossy().to_string()),
        })
    }

    #[cfg(target_os = "macos")]
    fn resolve_bundle_icon_path(bundle_path: &Path) -> Result<Option<PathBuf>, PlatformError> {
        let plist_path = bundle_path.join("Contents").join("Info.plist");
        let plist = PlistValue::from_file(&plist_path).map_err(|error| PlatformError::Io {
            path: plist_path.display().to_string(),
            message: error.to_string(),
        })?;
        let Some(dict) = plist.as_dictionary() else {
            return Ok(None);
        };

        let resources_dir = bundle_path.join("Contents").join("Resources");
        if !resources_dir.exists() {
            return Ok(None);
        }

        for candidate in bundle_icon_name_candidates(dict) {
            if let Some(icon_path) = resolve_icon_candidate(&resources_dir, &candidate) {
                return Ok(Some(icon_path));
            }
        }

        let mut fallback_icons = fs::read_dir(&resources_dir)
            .map_err(|error| PlatformError::Io {
                path: resources_dir.display().to_string(),
                message: error.to_string(),
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("icns"))
            })
            .collect::<Vec<_>>();
        fallback_icons.sort();

        Ok(fallback_icons.into_iter().next())
    }

    #[cfg(target_os = "macos")]
    fn bundle_icon_name_candidates(dict: &plist::Dictionary) -> Vec<String> {
        let mut candidates = Vec::new();

        if let Some(icon_file) = dict
            .get("CFBundleIconFile")
            .and_then(PlistValue::as_string)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            candidates.push(icon_file.to_string());
        }

        if let Some(icon_name) = dict
            .get("CFBundleIconName")
            .and_then(PlistValue::as_string)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            candidates.push(icon_name.to_string());
        }

        if let Some(icon_files) = dict
            .get("CFBundleIcons")
            .and_then(PlistValue::as_dictionary)
            .and_then(|icons| icons.get("CFBundlePrimaryIcon"))
            .and_then(PlistValue::as_dictionary)
            .and_then(|primary| primary.get("CFBundleIconFiles"))
            .and_then(PlistValue::as_array)
        {
            for icon_file in icon_files
                .iter()
                .filter_map(PlistValue::as_string)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                candidates.push(icon_file.to_string());
            }
        }

        candidates.dedup();
        candidates
    }

    #[cfg(target_os = "macos")]
    fn resolve_icon_candidate(resources_dir: &Path, candidate: &str) -> Option<PathBuf> {
        let base_name = candidate.trim_end_matches(".icns");
        let paths = [
            resources_dir.join(candidate),
            resources_dir.join(format!("{candidate}.icns")),
            resources_dir.join(format!("{base_name}.icns")),
        ];

        paths.into_iter().find(|path| path.exists())
    }

    #[cfg(target_os = "macos")]
    fn encode_icon_as_data_url(icon_path: &Path) -> Result<Option<String>, PlatformError> {
        let output_path = std::env::temp_dir().join(format!(
            "mouser-app-icon-{}-{}.png",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));

        let output = Command::new("/usr/bin/sips")
            .arg("-s")
            .arg("format")
            .arg("png")
            .arg(icon_path)
            .arg("--out")
            .arg(&output_path)
            .output()
            .map_err(|error| PlatformError::Io {
                path: icon_path.display().to_string(),
                message: error.to_string(),
            })?;

        if !output.status.success() || !output_path.exists() {
            return Ok(None);
        }

        let bytes = fs::read(&output_path).map_err(|error| PlatformError::Io {
            path: output_path.display().to_string(),
            message: error.to_string(),
        })?;
        let _ = fs::remove_file(&output_path);

        if bytes.is_empty() {
            return Ok(None);
        }

        Ok(Some(format!(
            "data:image/png;base64,{}",
            BASE64_STANDARD.encode(bytes)
        )))
    }

    #[cfg(target_os = "macos")]
    fn vendor_hid_infos(api: &HidApi) -> Vec<&HidDeviceInfo> {
        api.device_list()
            .filter(|info| info.vendor_id() == LOGI_VID && info.usage_page() >= 0xFF00)
            .collect()
    }

    #[cfg(target_os = "macos")]
    impl HidppIo for MacOsNativeHidDevice {
        fn write_packet(&self, packet: &[u8]) -> Result<(), PlatformError> {
            self.write_report(packet)
        }

        fn read_packet(&self, timeout_ms: i32) -> Result<Vec<u8>, PlatformError> {
            self.read_timeout(timeout_ms)
        }
    }

    #[cfg(target_os = "macos")]
    impl MacOsHidBackend {
        fn build_hidapi_device_info(
            &self,
            api: &HidApi,
            info: &HidDeviceInfo,
            now: Instant,
            connected_cache_keys: &mut BTreeSet<String>,
        ) -> Option<DeviceInfo> {
            let transport = Some(transport_label(info.bus_type()));
            let fingerprint = fingerprint_from_hid_info(info);
            let cache_key = telemetry_cache_key(Some(info.product_id()), transport, &fingerprint);

            let device_info = if connected_cache_keys.contains(&cache_key) {
                let plan = self.telemetry_plan(&cache_key, now);
                build_connected_device_info(
                    Some(info.product_id()),
                    info.product_string(),
                    transport,
                    Some("hidapi"),
                    plan.cached.battery,
                    plan.cached.current_dpi.unwrap_or(1000),
                    fingerprint,
                )
            } else {
                let plan = self.telemetry_plan(&cache_key, now);
                if plan.should_probe {
                    let device = info.open_device(api).ok()?;
                    let (current_dpi, battery) = probe_device_telemetry(
                        &device,
                        plan.cached.current_dpi,
                        plan.cached.battery,
                    );
                    self.remember_device_telemetry(
                        cache_key.clone(),
                        current_dpi,
                        battery.clone(),
                        now,
                    );
                    build_connected_device_info(
                        Some(info.product_id()),
                        info.product_string(),
                        transport,
                        Some("hidapi"),
                        battery,
                        current_dpi,
                        fingerprint,
                    )
                } else {
                    build_connected_device_info(
                        Some(info.product_id()),
                        info.product_string(),
                        transport,
                        Some("hidapi"),
                        plan.cached.battery,
                        plan.cached.current_dpi.unwrap_or(1000),
                        fingerprint,
                    )
                }
            };

            connected_cache_keys.insert(cache_key);
            Some(device_info)
        }

        fn build_iokit_device_info(
            &self,
            info: &MacOsIoKitInfo,
            now: Instant,
            connected_cache_keys: &mut BTreeSet<String>,
        ) -> Option<DeviceInfo> {
            let transport = iokit_transport_label(info.transport.as_deref());
            let fingerprint = fingerprint_from_iokit_info(info);
            let cache_key =
                telemetry_cache_key(Some(info.product_id), transport.as_deref(), &fingerprint);

            let device_info = if connected_cache_keys.contains(&cache_key) {
                let plan = self.telemetry_plan(&cache_key, now);
                build_connected_device_info(
                    Some(info.product_id),
                    info.product_string.as_deref(),
                    transport.as_deref(),
                    Some("iokit"),
                    plan.cached.battery,
                    plan.cached.current_dpi.unwrap_or(1000),
                    fingerprint,
                )
            } else {
                let plan = self.telemetry_plan(&cache_key, now);
                if plan.should_probe {
                    let device = open_iokit_device(info).ok()?;
                    let (current_dpi, battery) = probe_device_telemetry(
                        &device,
                        plan.cached.current_dpi,
                        plan.cached.battery,
                    );
                    self.remember_device_telemetry(
                        cache_key.clone(),
                        current_dpi,
                        battery.clone(),
                        now,
                    );
                    build_connected_device_info(
                        Some(info.product_id),
                        info.product_string.as_deref(),
                        transport.as_deref(),
                        Some("iokit"),
                        battery,
                        current_dpi,
                        fingerprint,
                    )
                } else {
                    build_connected_device_info(
                        Some(info.product_id),
                        info.product_string.as_deref(),
                        transport.as_deref(),
                        Some("iokit"),
                        plan.cached.battery,
                        plan.cached.current_dpi.unwrap_or(1000),
                        fingerprint,
                    )
                }
            };

            connected_cache_keys.insert(cache_key);
            Some(device_info)
        }
    }

    #[cfg(target_os = "macos")]
    fn probe_device_telemetry<T: HidppIo + ?Sized>(
        device: &T,
        cached_dpi: Option<u16>,
        cached_battery: Option<DeviceBatteryInfo>,
    ) -> (u16, Option<DeviceBatteryInfo>) {
        let current_dpi = hidpp::read_sensor_dpi(device, BT_DEV_IDX, 1_500)
            .ok()
            .flatten()
            .or(cached_dpi)
            .unwrap_or(1000);
        let battery = hidpp::read_battery_info(device, BT_DEV_IDX, 1_500)
            .ok()
            .flatten()
            .or(cached_battery);
        (current_dpi, battery)
    }

    #[cfg(target_os = "macos")]
    fn current_frontmost_app_identity() -> Result<Option<AppIdentity>, PlatformError> {
        let workspace = NSWorkspace::sharedWorkspace();
        let Some(app) = workspace.frontmostApplication() else {
            return Ok(None);
        };

        let executable_path = app
            .executableURL()
            .and_then(|url| url.path())
            .map(|path| path.to_string());
        let executable = executable_path.as_deref().and_then(|path| {
            Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        });

        Ok(Some(AppIdentity {
            label: app.localizedName().map(|name| name.to_string()),
            executable,
            executable_path,
            bundle_id: app
                .bundleIdentifier()
                .map(|bundle_id| bundle_id.to_string()),
            package_family_name: None,
        }))
    }

    #[cfg(target_os = "macos")]
    fn fingerprint_from_hid_info(info: &HidDeviceInfo) -> DeviceFingerprint {
        let mut fingerprint = DeviceFingerprint {
            identity_key: None,
            serial_number: info.serial_number().map(str::to_string),
            hid_path: Some(info.path().to_string_lossy().into_owned()),
            interface_number: Some(info.interface_number()),
            usage_page: Some(info.usage_page()),
            usage: Some(info.usage()),
            location_id: None,
        };
        hydrate_identity_key(Some(info.product_id()), &mut fingerprint);
        fingerprint
    }

    #[cfg(target_os = "macos")]
    fn fingerprint_from_iokit_info(info: &MacOsIoKitInfo) -> DeviceFingerprint {
        let mut fingerprint = DeviceFingerprint {
            identity_key: None,
            serial_number: info.serial_number.clone(),
            hid_path: None,
            interface_number: None,
            usage_page: Some(info.usage_page as u16),
            usage: Some(info.usage as u16),
            location_id: info.location_id,
        };
        hydrate_identity_key(Some(info.product_id), &mut fingerprint);
        fingerprint
    }

    #[cfg(target_os = "macos")]
    fn should_probe_cached_telemetry(
        entry: Option<&DeviceTelemetryCacheEntry>,
        now: Instant,
    ) -> bool {
        let Some(entry) = entry else {
            return true;
        };

        !entry.connected
            || entry.current_dpi.is_none()
            || now.duration_since(entry.last_battery_probe_at) >= BATTERY_CACHE_TTL
            || entry
                .verify_after
                .is_some_and(|verify_after| now >= verify_after)
    }

    #[cfg(target_os = "macos")]
    fn telemetry_cache_key(
        product_id: Option<u16>,
        transport: Option<&str>,
        fingerprint: &DeviceFingerprint,
    ) -> String {
        if let Some(identity_key) = fingerprint
            .identity_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return format!("identity:{identity_key}");
        }

        if let Some(serial_number) = fingerprint
            .serial_number
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return format!(
                "serial:{:04x}:{serial_number}",
                product_id.unwrap_or_default()
            );
        }

        format!(
            "tuple:{:04x}:{}:{}:{}:{}:{}:{}",
            product_id.unwrap_or_default(),
            transport.unwrap_or_default(),
            fingerprint.location_id.unwrap_or_default(),
            fingerprint.interface_number.unwrap_or_default(),
            fingerprint.usage_page.unwrap_or_default(),
            fingerprint.usage.unwrap_or_default(),
            fingerprint.hid_path.as_deref().unwrap_or_default(),
        )
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
    fn iokit_transport_label(transport: Option<&str>) -> Option<String> {
        transport.map(|value| match value {
            "Bluetooth" => "Bluetooth Low Energy".to_string(),
            other => other.to_string(),
        })
    }

    #[cfg(target_os = "macos")]
    fn push_unique_device(devices: &mut Vec<DeviceInfo>, device: DeviceInfo) {
        if devices
            .iter()
            .all(|existing: &DeviceInfo| existing.key != device.key)
        {
            devices.push(device);
        }
    }

    #[cfg(target_os = "macos")]
    fn device_key_matches(
        device_key: &str,
        product_id: Option<u16>,
        product_name: Option<&str>,
        transport: Option<&str>,
        source: &'static str,
        fingerprint: DeviceFingerprint,
        dpi: u16,
    ) -> bool {
        build_connected_device_info(
            product_id,
            product_name,
            transport,
            Some(source),
            None,
            dpi,
            fingerprint,
        )
        .key == device_key
    }

    #[cfg(target_os = "macos")]
    fn iokit_open_candidates(info: &MacOsIoKitInfo) -> Vec<MacOsIoKitInfo> {
        let mut candidates = vec![info.clone()];
        if info.transport.as_deref() != Some("USB") {
            candidates.push(MacOsIoKitInfo {
                product_id: info.product_id,
                usage_page: 0,
                usage: 0,
                transport: Some("Bluetooth Low Energy".to_string()),
                product_string: info.product_string.clone(),
                serial_number: info.serial_number.clone(),
                location_id: info.location_id,
            });
        }
        candidates
    }

    #[cfg(target_os = "macos")]
    fn open_iokit_device(info: &MacOsIoKitInfo) -> Result<MacOsNativeHidDevice, PlatformError> {
        let mut last_error = None;

        for candidate in iokit_open_candidates(info) {
            match MacOsNativeHidDevice::open(&candidate) {
                Ok(device) => return Ok(device),
                Err(error) => last_error = Some(error),
            }
        }

        Err(last_error.unwrap_or_else(|| {
            PlatformError::Message(format!(
                "could not open IOHIDDevice for pid 0x{:04X}",
                info.product_id
            ))
        }))
    }

    #[cfg(target_os = "macos")]
    fn set_hidpp_dpi<T: HidppIo + ?Sized>(device: &T, dpi: u16) -> Result<bool, PlatformError> {
        hidpp::set_sensor_dpi(device, BT_DEV_IDX, dpi, 1_500)
    }

    #[cfg(all(test, target_os = "macos"))]
    mod telemetry_tests {
        use super::*;

        fn sample_fingerprint() -> DeviceFingerprint {
            DeviceFingerprint {
                identity_key: None,
                serial_number: Some("ABC123".to_string()),
                hid_path: Some("/dev/hid0".to_string()),
                interface_number: Some(2),
                usage_page: Some(0xFF00),
                usage: Some(0x0001),
                location_id: Some(0xCAFEBABE),
            }
        }

        #[test]
        fn telemetry_cache_key_is_stable_for_same_physical_device() {
            let first = sample_fingerprint();
            let mut second = sample_fingerprint();
            second.hid_path = Some("/dev/hid1".to_string());

            assert_eq!(
                telemetry_cache_key(Some(0xB034), Some("Bluetooth Low Energy"), &first),
                telemetry_cache_key(Some(0xB034), Some("Bluetooth Low Energy"), &second),
            );
        }

        #[test]
        fn telemetry_probe_policy_handles_ttl_reconnect_and_verify() {
            let now = Instant::now();
            let fresh = DeviceTelemetryCacheEntry {
                current_dpi: Some(1000),
                battery: Some(DeviceBatteryInfo {
                    kind: mouser_core::DeviceBatteryKind::Percentage,
                    percentage: Some(80),
                    label: "80%".to_string(),
                    source_feature: None,
                    raw_capabilities: Vec::new(),
                    raw_status: Vec::new(),
                }),
                last_battery_probe_at: now,
                verify_after: None,
                connected: true,
            };
            assert!(!should_probe_cached_telemetry(Some(&fresh), now));

            let stale_battery = DeviceTelemetryCacheEntry {
                last_battery_probe_at: now - BATTERY_CACHE_TTL,
                ..fresh.clone()
            };
            assert!(should_probe_cached_telemetry(Some(&stale_battery), now));

            let disconnected = DeviceTelemetryCacheEntry {
                connected: false,
                ..fresh.clone()
            };
            assert!(should_probe_cached_telemetry(Some(&disconnected), now));

            let verify_due = DeviceTelemetryCacheEntry {
                verify_after: Some(now),
                ..fresh
            };
            assert!(should_probe_cached_telemetry(Some(&verify_due), now));
            assert!(should_probe_cached_telemetry(None, now));
        }
    }
}

pub mod linux {
    pub use crate::linux_backend::{
        LinuxAppDiscoveryBackend, LinuxAppFocusBackend, LinuxHidBackend, LinuxHookBackend,
    };
}

pub mod windows {
    pub use crate::windows_backend::{
        WindowsAppDiscoveryBackend, WindowsAppFocusBackend, WindowsAppFocusMonitor,
        WindowsHidBackend, WindowsHookBackend,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use mouser_core::default_settings;

    #[test]
    fn horizontal_scroll_mapping_is_consistent() {
        assert_eq!(
            horizontal_scroll_control(120),
            Some(LogicalControl::HscrollRight)
        );
        assert_eq!(
            horizontal_scroll_control(-120),
            Some(LogicalControl::HscrollLeft)
        );
        assert_eq!(horizontal_scroll_control(0), None);
    }

    #[test]
    fn bounded_hook_event_queue_keeps_latest_entries() {
        let mut events = Vec::new();
        for index in 0..(HOOK_EVENT_BUFFER_LIMIT + 5) {
            push_bounded_hook_event(&mut events, DebugEventKind::Info, format!("event-{index}"));
        }

        assert_eq!(events.len(), HOOK_EVENT_BUFFER_LIMIT);
        assert_eq!(
            events.first().map(|event| event.message.as_str()),
            Some("event-5")
        );
        assert_eq!(
            events.last().map(|event| event.message.as_str()),
            Some("event-132")
        );
    }

    #[test]
    fn thumb_wheel_trackpad_setting_only_enables_for_mx_master_models() {
        let settings = default_settings();
        let mut device_settings = default_device_settings();
        device_settings.macos_thumb_wheel_simulate_trackpad = true;
        device_settings.macos_thumb_wheel_trackpad_hold_timeout_ms = 900;
        let mx_master = mouser_core::default_device_catalog()
            .into_iter()
            .find(|device| device.model_key == "mx_master_3s")
            .expect("mx master fixture");
        let mut generic_mouse = mx_master.clone();
        generic_mouse.model_key = "mouse".to_string();

        let mx_master =
            HookBackendSettings::from_app_and_device(&settings, &device_settings, Some(&mx_master));
        let generic_mouse = HookBackendSettings::from_app_and_device(
            &settings,
            &device_settings,
            Some(&generic_mouse),
        );
        let unknown_device =
            HookBackendSettings::from_app_and_device(&settings, &device_settings, None);

        assert_eq!(
            mx_master.macos_thumb_wheel_simulate_trackpad,
            cfg!(target_os = "macos")
        );
        assert_eq!(mx_master.macos_thumb_wheel_trackpad_hold_timeout_ms, 900);
        assert!(!generic_mouse.macos_thumb_wheel_simulate_trackpad);
        assert_eq!(
            generic_mouse.macos_thumb_wheel_trackpad_hold_timeout_ms,
            900
        );
        assert!(!unknown_device.macos_thumb_wheel_simulate_trackpad);
        assert_eq!(
            unknown_device.macos_thumb_wheel_trackpad_hold_timeout_ms,
            900
        );
    }

    #[test]
    fn dedupe_installed_apps_merges_sources_and_prefers_richer_metadata() {
        let apps = vec![
            InstalledApp {
                identity: AppIdentity {
                    label: Some("Code".to_string()),
                    executable: Some("Code.exe".to_string()),
                    executable_path: Some("C:\\Apps\\Code.exe".to_string()),
                    bundle_id: None,
                    package_family_name: None,
                },
                source_kinds: vec![AppDiscoverySource::StartMenuShortcut],
                source_path: Some("C:\\Users\\luca\\Start Menu\\Code.lnk".to_string()),
            },
            InstalledApp {
                identity: AppIdentity {
                    label: Some("Visual Studio Code".to_string()),
                    executable: Some("Code.exe".to_string()),
                    executable_path: Some("C:\\Apps\\Code.exe".to_string()),
                    bundle_id: None,
                    package_family_name: None,
                },
                source_kinds: vec![AppDiscoverySource::Registry],
                source_path: Some("C:\\Apps\\Code.exe".to_string()),
            },
        ];

        let deduped = dedupe_installed_apps(apps);

        assert_eq!(deduped.len(), 1);
        let app = &deduped[0];
        assert_eq!(app.identity.label.as_deref(), Some("Visual Studio Code"));
        assert_eq!(
            app.source_kinds,
            vec![
                AppDiscoverySource::StartMenuShortcut,
                AppDiscoverySource::Registry,
            ]
        );
        assert_eq!(app.source_path.as_deref(), Some("C:\\Apps\\Code.exe"));
    }

    #[test]
    fn dedupe_installed_apps_merges_package_and_running_entries_by_path_overlap() {
        let apps = vec![
            InstalledApp {
                identity: AppIdentity {
                    label: Some("Contoso Player".to_string()),
                    executable: Some("ContosoPlayer.exe".to_string()),
                    executable_path: Some(
                        "C:\\Program Files\\WindowsApps\\Contoso.Player_1.0.0.0_x64__abc\\ContosoPlayer.exe"
                            .to_string(),
                    ),
                    bundle_id: None,
                    package_family_name: Some("Contoso.Player_abc".to_string()),
                },
                source_kinds: vec![AppDiscoverySource::Package],
                source_path: Some(
                    "C:\\Program Files\\WindowsApps\\Contoso.Player_1.0.0.0_x64__abc\\Assets\\Logo.png"
                        .to_string(),
                ),
            },
            InstalledApp {
                identity: AppIdentity {
                    label: Some("Contoso Player".to_string()),
                    executable: Some("ContosoPlayer.exe".to_string()),
                    executable_path: Some(
                        "C:\\Program Files\\WindowsApps\\Contoso.Player_1.0.0.0_x64__abc\\ContosoPlayer.exe"
                            .to_string(),
                    ),
                    bundle_id: None,
                    package_family_name: None,
                },
                source_kinds: vec![AppDiscoverySource::RunningProcess],
                source_path: Some(
                    "C:\\Program Files\\WindowsApps\\Contoso.Player_1.0.0.0_x64__abc\\ContosoPlayer.exe"
                        .to_string(),
                ),
            },
        ];

        let deduped = dedupe_installed_apps(apps);

        assert_eq!(deduped.len(), 1);
        let app = &deduped[0];
        assert_eq!(
            app.source_kinds,
            vec![
                AppDiscoverySource::Package,
                AppDiscoverySource::RunningProcess,
            ]
        );
        assert_eq!(
            app.identity.package_family_name.as_deref(),
            Some("Contoso.Player_abc")
        );
    }

    #[test]
    fn dedupe_installed_apps_does_not_merge_executable_only_name_collisions() {
        let apps = vec![
            InstalledApp {
                identity: AppIdentity {
                    label: Some("Portable Tool".to_string()),
                    executable: Some("tool.exe".to_string()),
                    executable_path: Some("D:\\Portable\\tool.exe".to_string()),
                    bundle_id: None,
                    package_family_name: None,
                },
                source_kinds: vec![AppDiscoverySource::RunningProcess],
                source_path: Some("D:\\Portable\\tool.exe".to_string()),
            },
            InstalledApp {
                identity: AppIdentity {
                    label: Some("Installed Tool".to_string()),
                    executable: Some("tool.exe".to_string()),
                    executable_path: Some("C:\\Program Files\\Tool\\tool.exe".to_string()),
                    bundle_id: None,
                    package_family_name: None,
                },
                source_kinds: vec![AppDiscoverySource::Registry],
                source_path: Some("C:\\Program Files\\Tool\\tool.exe".to_string()),
            },
        ];

        let deduped = dedupe_installed_apps(apps);

        assert_eq!(deduped.len(), 2);
    }
}
