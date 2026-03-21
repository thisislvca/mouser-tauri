#![cfg_attr(not(target_os = "windows"), allow(dead_code, unused_imports))]

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Condvar, Mutex, OnceLock, RwLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use mouser_core::{
    build_connected_device_info, default_config, hydrate_identity_key, resolve_known_device,
    AppDiscoverySource, AppIdentity, DebugEventKind, DebugLogGroup, DebugLogGroups,
    DeviceBatteryInfo,
    DeviceControlCaptureKind, DeviceControlSpec, DeviceFingerprint, DeviceInfo, DeviceSettings,
    InstalledApp, LogicalControl,
};
use serde_json::Value as JsonValue;

use crate::{
    backend_debug_logging_enabled, emit_backend_console_log,
    dedupe_installed_apps, gesture,
    hidpp::{self, HidppIo, BT_DEV_IDX},
    horizontal_scroll_control, push_bounded_hook_event, AppDiscoveryBackend, AppFocusBackend,
    HidBackend, HidCapabilities, HookBackend, HookBackendEvent, HookBackendSettings,
    HookCapabilities, HookDeviceRoute, PlatformError,
};

#[cfg(target_os = "windows")]
use hidapi::{BusType, DeviceInfo as HidDeviceInfo, HidApi, HidDevice};
#[cfg(target_os = "windows")]
use lnk::encoding::WINDOWS_1252;
#[cfg(target_os = "windows")]
use lnk::ShellLink;
#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, APPMODEL_ERROR_NO_PACKAGE, BOOL, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT,
        WPARAM,
    },
    Storage::Packaging::Appx::{GetPackageFamilyName, PACKAGE_FAMILY_NAME_MAX_LENGTH},
    System::{LibraryLoader::GetModuleHandleW, Threading::*},
    UI::{Accessibility::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
#[cfg(target_os = "windows")]
use winreg::{enums::*, RegKey};

const LOGI_VID: u16 = 0x046D;
const FEAT_REPROG_V4: u16 = 0x1B04;
const DEVICE_INDICES: [u8; 3] = [0xFF, 0x00, 0x01];
const GESTURE_DIVERT_FLAGS: u8 = 0x01;
const GESTURE_RAWXY_FLAGS: u8 = 0x05;
const GESTURE_UNDIVERT_FLAGS: u8 = 0x00;
const GESTURE_UNDIVERT_RAWXY_FLAGS: u8 = 0x04;
const BATTERY_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const DPI_VERIFY_DELAY: Duration = Duration::from_secs(1);
#[cfg(target_os = "windows")]
const EXPLORER_CLASSES: &[&str] = &[
    "CabinetWClass",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Progman",
    "WorkerW",
];

pub struct WindowsHookBackend {
    #[cfg(target_os = "windows")]
    shared: Arc<WindowsHookShared>,
    #[cfg(target_os = "windows")]
    stop: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    thread_id: Arc<AtomicU32>,
    #[cfg(target_os = "windows")]
    worker: Mutex<Option<JoinHandle<()>>>,
    #[cfg(target_os = "windows")]
    gesture_worker: Mutex<Option<JoinHandle<()>>>,
}

pub struct WindowsHidBackend {
    #[cfg(target_os = "windows")]
    telemetry_cache: Mutex<BTreeMap<String, DeviceTelemetryCacheEntry>>,
}
pub struct WindowsAppFocusBackend;
pub struct WindowsAppFocusMonitor {
    #[cfg(target_os = "windows")]
    stop: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    thread_id: Arc<AtomicU32>,
    #[cfg(target_os = "windows")]
    worker: Mutex<Option<JoinHandle<()>>>,
}
pub struct WindowsAppDiscoveryBackend;

#[derive(Debug, Clone)]
struct DeviceTelemetryCacheEntry {
    current_dpi: Option<u16>,
    battery: Option<DeviceBatteryInfo>,
    last_battery_probe_at: Instant,
    verify_after: Option<Instant>,
    connected: bool,
}

#[derive(Debug, Clone)]
struct DeviceTelemetrySnapshot {
    current_dpi: Option<u16>,
    battery: Option<DeviceBatteryInfo>,
}

#[derive(Debug, Clone)]
struct TelemetryProbePlan {
    probe_dpi: bool,
    probe_battery: bool,
    cached: DeviceTelemetrySnapshot,
}

#[derive(Clone, PartialEq, Eq)]
struct WindowsHookConfig {
    enabled: bool,
    debug_mode: bool,
    debug_log_groups: DebugLogGroups,
    routes: Vec<WindowsDeviceRoute>,
}

#[derive(Clone, PartialEq, Eq)]
struct WindowsDeviceRoute {
    managed_device_key: String,
    resolved_profile_id: String,
    live_device: DeviceInfo,
    device_settings: DeviceSettings,
    bindings: HashMap<LogicalControl, String>,
    device_controls: Vec<DeviceControlSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReprogRoute {
    control: LogicalControl,
    cids: Vec<u16>,
    rawxy_enabled: bool,
}

impl WindowsHookConfig {
    fn from_runtime(settings: &HookBackendSettings, enabled: bool) -> Self {
        Self {
            enabled,
            debug_mode: settings.debug_mode,
            debug_log_groups: settings.debug_log_groups.clone(),
            routes: settings
                .routes
                .iter()
                .cloned()
                .map(WindowsDeviceRoute::from_runtime)
                .collect(),
        }
    }

    fn debug_logging_enabled(&self, group: DebugLogGroup) -> bool {
        backend_debug_logging_enabled(self.debug_mode, &self.debug_log_groups, group)
    }

    fn summary(&self) -> String {
        self.routes
            .iter()
            .map(WindowsDeviceRoute::summary)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn gesture_capture_requested(&self) -> bool {
        self.enabled && self.routes.iter().any(WindowsDeviceRoute::gesture_capture_requested)
    }

    fn global_route(&self) -> Option<&WindowsDeviceRoute> {
        (self.enabled && self.routes.len() == 1).then(|| self.routes.first()).flatten()
    }
}

impl WindowsDeviceRoute {
    fn from_runtime(route: HookDeviceRoute) -> Self {
        Self {
            managed_device_key: route.managed_device_key,
            resolved_profile_id: route.resolved_profile_id,
            device_controls: route.live_device.controls.clone(),
            bindings: route
                .bindings
                .iter()
                .map(|binding| (binding.control, binding.action_id.clone()))
                .collect(),
            live_device: route.live_device,
            device_settings: route.device_settings,
        }
    }

    fn action_for(&self, control: LogicalControl) -> Option<&str> {
        self.bindings
            .get(&control)
            .map(String::as_str)
            .filter(|action_id| *action_id != "none")
    }

    fn handles_control(&self, control: LogicalControl) -> bool {
        self.action_for(control).is_some()
    }

    fn gesture_direction_enabled(&self) -> bool {
        [
            LogicalControl::GestureLeft,
            LogicalControl::GestureRight,
            LogicalControl::GestureUp,
            LogicalControl::GestureDown,
        ]
        .into_iter()
        .any(|control| self.handles_control(control))
    }

    fn gesture_capture_requested(&self) -> bool {
        !self.reprog_routes().is_empty()
    }

    fn gesture_route(&self) -> Option<ReprogRoute> {
        let gesture_requested = [
            LogicalControl::GesturePress,
            LogicalControl::GestureLeft,
            LogicalControl::GestureRight,
            LogicalControl::GestureUp,
            LogicalControl::GestureDown,
        ]
        .into_iter()
        .any(|control| self.handles_control(control));
        if !gesture_requested {
            return None;
        }

        self.device_controls.iter().find_map(|control| {
            (control.control == LogicalControl::GesturePress && !control.reprog_cids.is_empty())
                .then(|| ReprogRoute {
                    control: LogicalControl::GesturePress,
                    cids: control.reprog_cids.clone(),
                    rawxy_enabled: self.gesture_direction_enabled(),
                })
        })
    }

    fn reprog_routes(&self) -> Vec<ReprogRoute> {
        let mut routes = Vec::new();

        if let Some(route) = self.gesture_route() {
            routes.push(route);
        }

        for control in &self.device_controls {
            if control.capture_kind != DeviceControlCaptureKind::ReprogButton
                || !self.handles_control(control.control)
                || control.reprog_cids.is_empty()
            {
                continue;
            }

            routes.push(ReprogRoute {
                control: control.control,
                cids: control.reprog_cids.clone(),
                rawxy_enabled: false,
            });
        }

        routes
    }

    fn summary(&self) -> String {
        let bindings = [
            LogicalControl::Back,
            LogicalControl::Forward,
            LogicalControl::Middle,
            LogicalControl::HscrollLeft,
            LogicalControl::HscrollRight,
            LogicalControl::GesturePress,
            LogicalControl::GestureLeft,
            LogicalControl::GestureRight,
            LogicalControl::GestureUp,
            LogicalControl::GestureDown,
        ]
        .into_iter()
        .map(|control| {
            format!(
                "{}={}",
                control.label(),
                self.bindings
                    .get(&control)
                    .map(String::as_str)
                    .unwrap_or("none")
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

        format!(
            "{}:{} [{}]",
            self.managed_device_key, self.resolved_profile_id, bindings
        )
    }
}

#[cfg(target_os = "windows")]
fn map_hid_error(error: hidapi::HidError) -> PlatformError {
    PlatformError::Message(error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GestureInputSource {
    HidRawxy,
}

impl GestureInputSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::HidRawxy => "hid_rawxy",
        }
    }
}

#[derive(Debug, Clone, Default)]
struct GestureTrackingState {
    active: bool,
    tracking: bool,
    triggered: bool,
    started_at: Option<Instant>,
    last_move_at: Option<Instant>,
    delta_x: f64,
    delta_y: f64,
    cooldown_until: Option<Instant>,
    input_source: Option<GestureInputSource>,
}

struct WindowsHookShared {
    config: RwLock<Arc<WindowsHookConfig>>,
    events: Mutex<Vec<HookBackendEvent>>,
    gesture_wait: Mutex<()>,
    gesture_cv: Condvar,
    hook_running: AtomicBool,
    gesture_connected: AtomicBool,
}

impl WindowsHookShared {
    fn new() -> Self {
        let config = default_config();
        let hook_settings = HookBackendSettings::from_app_and_device(
            &config.settings,
            &config.device_defaults,
            None,
        );

        Self {
            config: RwLock::new(Arc::new(WindowsHookConfig::from_runtime(&hook_settings, true))),
            events: Mutex::new(Vec::new()),
            gesture_wait: Mutex::new(()),
            gesture_cv: Condvar::new(),
            hook_running: AtomicBool::new(false),
            gesture_connected: AtomicBool::new(false),
        }
    }

    fn current_config(&self) -> Arc<WindowsHookConfig> {
        Arc::clone(&self.config.read().unwrap())
    }

    fn reconfigure(&self, settings: &HookBackendSettings, enabled: bool) {
        let next = Arc::new(WindowsHookConfig::from_runtime(settings, enabled));
        let changed = {
            let mut config = self.config.write().unwrap();
            if config.as_ref() == next.as_ref() {
                false
            } else {
                *config = Arc::clone(&next);
                true
            }
        };
        self.gesture_cv.notify_all();

        if changed && next.debug_logging_enabled(DebugLogGroup::HookRouting) {
            emit_backend_console_log(
                "windows",
                DebugEventKind::Info,
                DebugLogGroup::HookRouting,
                &format!("Windows hook routes -> {}", next.summary()),
            );
        }
    }

    fn push_event(&self, kind: DebugEventKind, message: impl Into<String>) {
        let mut events = self.events.lock().unwrap();
        push_bounded_hook_event(&mut events, kind, message);
    }

    fn log_console(&self, group: DebugLogGroup, kind: DebugEventKind, message: impl Into<String>) {
        let message = message.into();
        let config = self.current_config();
        if config.debug_logging_enabled(group) {
            emit_backend_console_log("windows", kind, group, &message);
        }
    }

    fn push_debug(&self, message: impl Into<String>) {
        self.log_console(DebugLogGroup::HookRouting, DebugEventKind::Info, message);
    }

    fn push_gesture_debug(&self, message: impl Into<String>) {
        self.log_console(DebugLogGroup::Gestures, DebugEventKind::Gesture, message);
    }

    fn drain_events(&self) -> Vec<HookBackendEvent> {
        let mut events = self.events.lock().unwrap();
        std::mem::take(&mut *events)
    }

    fn gesture_capture_requested(&self) -> bool {
        self.config.read().unwrap().gesture_capture_requested()
    }

    fn mark_hook_running(&self, running: bool, message: Option<String>) {
        let previous = self.hook_running.swap(running, Ordering::SeqCst);
        if let Some(message) = message {
            if previous != running || self.config.read().unwrap().debug_mode {
                self.push_event(DebugEventKind::Info, message);
            }
        }
    }

    fn mark_gesture_connected(&self, connected: bool, message: Option<String>) {
        let previous = self.gesture_connected.swap(connected, Ordering::SeqCst);
        if let Some(message) = message {
            if previous != connected || self.config.read().unwrap().debug_mode {
                self.push_event(DebugEventKind::Info, message);
            }
        }
    }

    fn dispatch_route_control_action(&self, route: &WindowsDeviceRoute, control: LogicalControl) {
        let Some(action_id) = route.action_for(control).map(str::to_string) else {
            return;
        };

        self.push_debug(format!(
            "Mapped {} on {} -> {}",
            control.label(),
            route.managed_device_key,
            action_id
        ));
        if let Err(error) = execute_action(&action_id) {
            self.push_event(
                DebugEventKind::Warning,
                format!("Action `{action_id}` failed: {error}"),
            );
        }
    }
}

impl WindowsHidBackend {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "windows")]
            telemetry_cache: Mutex::new(BTreeMap::new()),
        }
    }
}

impl Default for WindowsHidBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
type AppFocusCallback = dyn Fn(Option<AppIdentity>) + Send + Sync + 'static;

#[cfg(target_os = "windows")]
impl WindowsAppFocusMonitor {
    pub fn new<F>(notify: F) -> Result<Self, PlatformError>
    where
        F: Fn(Option<AppIdentity>) + Send + Sync + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_id = Arc::new(AtomicU32::new(0));
        let callback: Arc<AppFocusCallback> = Arc::new(notify);
        let (startup_tx, startup_rx) = mpsc::channel::<Result<(), String>>();
        let worker_stop = Arc::clone(&stop);
        let worker_thread_id = Arc::clone(&thread_id);

        let worker = thread::spawn(move || {
            run_app_focus_monitor_worker(callback, worker_stop, worker_thread_id, startup_tx);
        });

        match startup_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                thread_id,
                worker: Mutex::new(Some(worker)),
            }),
            Ok(Err(error)) => {
                stop.store(true, Ordering::SeqCst);
                let _ = worker.join();
                Err(PlatformError::Message(error))
            }
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                let _ = worker.join();
                Err(PlatformError::Message(
                    "Windows app-focus monitor startup timed out".to_string(),
                ))
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
impl WindowsAppFocusMonitor {
    pub fn new<F>(_notify: F) -> Result<Self, PlatformError>
    where
        F: Fn(Option<AppIdentity>) + Send + Sync + 'static,
    {
        Err(PlatformError::Unsupported(
            "live Windows app focus monitoring is only available on Windows",
        ))
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsAppFocusMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let thread_id = self.thread_id.swap(0, Ordering::SeqCst);
        if thread_id != 0 {
            unsafe {
                PostThreadMessageW(thread_id, WM_QUIT, 0, 0);
            }
        }

        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
    }
}

impl WindowsHidBackend {
    fn telemetry_plan(&self, cache_key: &str, now: Instant) -> TelemetryProbePlan {
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (cache_key, now);
            TelemetryProbePlan {
                probe_dpi: true,
                probe_battery: true,
                cached: DeviceTelemetrySnapshot {
                    current_dpi: None,
                    battery: None,
                },
            }
        }

        #[cfg(target_os = "windows")]
        {
            let cache = self.telemetry_cache.lock().unwrap();
            let entry = cache.get(cache_key);
            TelemetryProbePlan {
                probe_dpi: should_probe_dpi(entry, now),
                probe_battery: should_probe_battery(entry, now),
                cached: DeviceTelemetrySnapshot {
                    current_dpi: entry.and_then(|entry| entry.current_dpi),
                    battery: entry.and_then(|entry| entry.battery.clone()),
                },
            }
        }
    }

    fn remember_device_telemetry(
        &self,
        cache_key: String,
        telemetry: DeviceTelemetrySnapshot,
        plan: &TelemetryProbePlan,
        now: Instant,
    ) {
        #[cfg(target_os = "windows")]
        {
            let mut cache = self.telemetry_cache.lock().unwrap();
            let entry = cache.entry(cache_key).or_insert(DeviceTelemetryCacheEntry {
                current_dpi: None,
                battery: None,
                last_battery_probe_at: now,
                verify_after: None,
                connected: true,
            });

            entry.connected = true;

            if telemetry.current_dpi.is_some() {
                entry.current_dpi = telemetry.current_dpi;
            }

            if plan.probe_dpi {
                entry.verify_after = None;
            }

            if plan.probe_battery {
                entry.battery = telemetry.battery.clone();
                entry.last_battery_probe_at = now;
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (cache_key, telemetry, plan, now);
        }
    }

    fn note_connected_devices(&self, connected_cache_keys: &BTreeSet<String>) {
        #[cfg(target_os = "windows")]
        {
            let mut cache = self.telemetry_cache.lock().unwrap();
            for (cache_key, entry) in cache.iter_mut() {
                entry.connected = connected_cache_keys.contains(cache_key);
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = connected_cache_keys;
        }
    }

    fn note_dpi_write(&self, cache_key: String, dpi: u16, now: Instant) {
        #[cfg(target_os = "windows")]
        {
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

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (cache_key, dpi, now);
        }
    }
}

impl WindowsHookBackend {
    pub fn new() -> Self {
        #[cfg(not(target_os = "windows"))]
        {
            Self {}
        }

        #[cfg(target_os = "windows")]
        {
            let shared = Arc::new(WindowsHookShared::new());
            let stop = Arc::new(AtomicBool::new(false));
            let thread_id = Arc::new(AtomicU32::new(0));
            let (startup_tx, startup_rx) = mpsc::channel::<()>();

            {
                let mut slot = global_hook_shared().lock().unwrap();
                *slot = Some(Arc::clone(&shared));
            }

            let worker_shared = Arc::clone(&shared);
            let worker_stop = Arc::clone(&stop);
            let worker_thread_id = Arc::clone(&thread_id);
            let worker = thread::spawn(move || {
                run_mouse_hook_worker(worker_shared, worker_stop, worker_thread_id, startup_tx);
            });

            let gesture_shared = Arc::clone(&shared);
            let gesture_stop = Arc::clone(&stop);
            let gesture_worker = thread::spawn(move || {
                run_gesture_worker(gesture_shared, gesture_stop);
            });

            let _ = startup_rx.recv_timeout(Duration::from_secs(2));

            Self {
                shared,
                stop,
                thread_id,
                worker: Mutex::new(Some(worker)),
                gesture_worker: Mutex::new(Some(gesture_worker)),
            }
        }
    }
}

impl Default for WindowsHookBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl HookBackend for WindowsHookBackend {
    fn backend_id(&self) -> &'static str {
        #[cfg(not(target_os = "windows"))]
        {
            "windows-stub"
        }

        #[cfg(target_os = "windows")]
        {
            let hook_running = self.shared.hook_running.load(Ordering::SeqCst);
            let gesture_connected = self.shared.gesture_connected.load(Ordering::SeqCst);

            match (hook_running, gesture_connected) {
                (true, true) => "windows-hook+hidapi-gesture",
                (true, false) => "windows-hook",
                (false, true) => "windows-hidapi-gesture",
                (false, false) => "windows-hook-unavailable",
            }
        }
    }

    fn capabilities(&self) -> HookCapabilities {
        #[cfg(not(target_os = "windows"))]
        {
            HookCapabilities {
                can_intercept_buttons: false,
                can_intercept_scroll: false,
                supports_gesture_diversion: false,
            }
        }

        #[cfg(target_os = "windows")]
        {
            HookCapabilities {
                can_intercept_buttons: self.shared.hook_running.load(Ordering::SeqCst),
                can_intercept_scroll: self.shared.hook_running.load(Ordering::SeqCst),
                supports_gesture_diversion: self.shared.gesture_connected.load(Ordering::SeqCst),
            }
        }
    }

    fn configure(
        &self,
        settings: &HookBackendSettings,
        enabled: bool,
    ) -> Result<(), PlatformError> {
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (settings, enabled);
            Ok(())
        }

        #[cfg(target_os = "windows")]
        {
            self.shared.reconfigure(settings, enabled);
            Ok(())
        }
    }

    fn execute_action(&self, action_id: &str) -> Result<(), PlatformError> {
        execute_action(action_id)
    }

    fn drain_events(&self) -> Vec<HookBackendEvent> {
        #[cfg(not(target_os = "windows"))]
        {
            Vec::new()
        }

        #[cfg(target_os = "windows")]
        {
            self.shared.drain_events()
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsHookBackend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.shared.gesture_cv.notify_all();
        let thread_id = self.thread_id.swap(0, Ordering::SeqCst);
        if thread_id != 0 {
            unsafe {
                PostThreadMessageW(thread_id, WM_QUIT, 0, 0);
            }
        }

        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }

        if let Some(gesture_worker) = self.gesture_worker.lock().unwrap().take() {
            let _ = gesture_worker.join();
        }

        let mut slot = global_hook_shared().lock().unwrap();
        *slot = None;
    }
}

impl HidBackend for WindowsHidBackend {
    fn backend_id(&self) -> &'static str {
        #[cfg(target_os = "windows")]
        {
            "windows-hidapi"
        }

        #[cfg(not(target_os = "windows"))]
        {
            "windows-stub"
        }
    }

    fn capabilities(&self) -> HidCapabilities {
        #[cfg(target_os = "windows")]
        {
            HidCapabilities {
                can_enumerate_devices: true,
                can_read_battery: true,
                can_read_dpi: true,
                can_write_dpi: true,
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            HidCapabilities {
                can_enumerate_devices: false,
                can_read_battery: false,
                can_read_dpi: false,
                can_write_dpi: false,
            }
        }
    }

    fn list_devices(&self) -> Result<Vec<DeviceInfo>, PlatformError> {
        #[cfg(not(target_os = "windows"))]
        {
            Err(PlatformError::Unsupported(
                "live Windows HID integration is only available on Windows",
            ))
        }

        #[cfg(target_os = "windows")]
        {
            let api = HidApi::new().map_err(map_hid_error)?;
            let mut devices = Vec::new();
            let mut connected_cache_keys = BTreeSet::new();
            let now = Instant::now();

            for info in vendor_hid_infos(&api) {
                let fingerprint = fingerprint_from_hid_info(info);
                let transport = transport_label(info.bus_type());
                let cache_key =
                    telemetry_cache_key(Some(info.product_id()), Some(transport), &fingerprint);
                connected_cache_keys.insert(cache_key.clone());
                let plan = self.telemetry_plan(&cache_key, now);

                let telemetry = if plan.probe_dpi || plan.probe_battery {
                    match info.open_device(&api) {
                        Ok(device) => {
                            let telemetry = probe_device_telemetry(&device, &plan);
                            self.remember_device_telemetry(
                                cache_key.clone(),
                                telemetry.clone(),
                                &plan,
                                now,
                            );
                            telemetry
                        }
                        Err(_) => plan.cached.clone(),
                    }
                } else {
                    plan.cached.clone()
                };

                push_unique_device(
                    &mut devices,
                    build_connected_device_info(
                        Some(info.product_id()),
                        info.product_string(),
                        Some(transport),
                        Some("hidapi"),
                        telemetry.battery,
                        telemetry.current_dpi.unwrap_or(1000),
                        fingerprint,
                    ),
                );
            }

            self.note_connected_devices(&connected_cache_keys);
            Ok(devices)
        }
    }

    fn set_device_dpi(&self, device_key: &str, dpi: u16) -> Result<(), PlatformError> {
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (device_key, dpi);
            Err(PlatformError::Unsupported(
                "live Windows HID integration is only available on Windows",
            ))
        }

        #[cfg(target_os = "windows")]
        {
            let api = HidApi::new().map_err(map_hid_error)?;
            let now = Instant::now();
            for info in vendor_hid_infos(&api) {
                let Ok(device) = info.open_device(&api) else {
                    continue;
                };
                let fingerprint = fingerprint_from_hid_info(info);
                let transport = transport_label(info.bus_type());

                if device_key_matches(
                    device_key,
                    Some(info.product_id()),
                    info.product_string(),
                    Some(transport),
                    "hidapi",
                    fingerprint.clone(),
                    dpi,
                ) && set_hidpp_dpi(&device, dpi)?
                {
                    self.note_dpi_write(
                        telemetry_cache_key(Some(info.product_id()), Some(transport), &fingerprint),
                        dpi,
                        now,
                    );
                    return Ok(());
                }
            }

            Err(PlatformError::Message(format!(
                "could not find a live Logitech device matching `{device_key}`"
            )))
        }
    }
}

impl AppFocusBackend for WindowsAppFocusBackend {
    fn backend_id(&self) -> &'static str {
        #[cfg(target_os = "windows")]
        {
            "windows-foreground"
        }

        #[cfg(not(target_os = "windows"))]
        {
            "windows-stub"
        }
    }

    fn current_frontmost_app(&self) -> Result<Option<AppIdentity>, PlatformError> {
        #[cfg(not(target_os = "windows"))]
        {
            Err(PlatformError::Unsupported(
                "live Windows frontmost app detection is only available on Windows",
            ))
        }

        #[cfg(target_os = "windows")]
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return Ok(None);
            }

            foreground_app_identity(hwnd)
        }
    }
}

impl AppDiscoveryBackend for WindowsAppDiscoveryBackend {
    fn backend_id(&self) -> &'static str {
        #[cfg(target_os = "windows")]
        {
            "windows-hybrid"
        }

        #[cfg(not(target_os = "windows"))]
        {
            "windows-stub"
        }
    }

    fn discover_apps(&self) -> Result<Vec<InstalledApp>, PlatformError> {
        #[cfg(not(target_os = "windows"))]
        {
            Err(PlatformError::Unsupported(
                "live Windows app discovery is only available on Windows",
            ))
        }

        #[cfg(target_os = "windows")]
        {
            let mut apps = Vec::new();
            for root in windows_start_menu_roots() {
                collect_shortcut_apps(&root, &mut apps)?;
            }
            collect_app_path_apps(&mut apps)?;
            collect_uninstall_registry_apps(&mut apps)?;
            collect_package_apps(&mut apps)?;
            collect_running_process_apps(&mut apps)?;
            Ok(dedupe_installed_apps(apps))
        }
    }
}

#[cfg(target_os = "windows")]
fn global_hook_shared() -> &'static Mutex<Option<Arc<WindowsHookShared>>> {
    static SHARED: OnceLock<Mutex<Option<Arc<WindowsHookShared>>>> = OnceLock::new();
    SHARED.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "windows")]
fn global_app_focus_callbacks() -> &'static Mutex<HashMap<isize, Arc<AppFocusCallback>>> {
    static CALLBACKS: OnceLock<Mutex<HashMap<isize, Arc<AppFocusCallback>>>> = OnceLock::new();
    CALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(target_os = "windows")]
fn run_app_focus_monitor_worker(
    callback: Arc<AppFocusCallback>,
    stop: Arc<AtomicBool>,
    thread_id: Arc<AtomicU32>,
    startup_tx: mpsc::Sender<Result<(), String>>,
) {
    unsafe {
        thread_id.store(GetCurrentThreadId(), Ordering::SeqCst);
        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            Some(app_focus_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );

        if hook.is_null() {
            let _ = startup_tx.send(Err("failed to install Windows foreground hook".to_string()));
            return;
        }

        global_app_focus_callbacks()
            .lock()
            .unwrap()
            .insert(hook as isize, callback);
        let _ = startup_tx.send(Ok(()));

        let mut message = std::mem::zeroed::<MSG>();
        while !stop.load(Ordering::SeqCst) {
            let result = GetMessageW(&mut message, std::ptr::null_mut(), 0, 0);
            if result == -1 || result == 0 {
                break;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        global_app_focus_callbacks()
            .lock()
            .unwrap()
            .remove(&(hook as isize));
        UnhookWinEvent(hook);
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn app_focus_event_proc(
    hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _event_thread: u32,
    _event_time: u32,
) {
    if event != EVENT_SYSTEM_FOREGROUND || hwnd.is_null() {
        return;
    }

    let callback = {
        let callbacks = global_app_focus_callbacks().lock().unwrap();
        callbacks.get(&(hook as isize)).cloned()
    };
    let Some(callback) = callback else {
        return;
    };

    let frontmost_app = foreground_app_identity(hwnd).ok().flatten();
    callback(frontmost_app);
}

#[cfg(target_os = "windows")]
fn run_mouse_hook_worker(
    shared: Arc<WindowsHookShared>,
    stop: Arc<AtomicBool>,
    thread_id: Arc<AtomicU32>,
    startup_tx: mpsc::Sender<()>,
) {
    unsafe {
        thread_id.store(GetCurrentThreadId(), Ordering::SeqCst);

        let module = GetModuleHandleW(std::ptr::null()) as HINSTANCE;
        let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), module, 0);
        if hook.is_null() {
            shared.push_event(
                DebugEventKind::Warning,
                "Failed to install Windows mouse hook".to_string(),
            );
            let _ = startup_tx.send(());
            return;
        }

        shared.mark_hook_running(true, Some("Installed Windows mouse hook".to_string()));
        let _ = startup_tx.send(());

        let mut message = std::mem::zeroed::<MSG>();
        while !stop.load(Ordering::SeqCst) {
            let result = GetMessageW(&mut message, 0 as HWND, 0, 0);
            if result == -1 {
                shared.push_event(
                    DebugEventKind::Warning,
                    "Windows hook message loop failed".to_string(),
                );
                break;
            }
            if result == 0 {
                break;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        UnhookWindowsHookEx(hook);
        shared.mark_hook_running(false, Some("Stopped Windows mouse hook".to_string()));
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn low_level_mouse_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code != HC_ACTION as i32 {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }

    let shared = {
        let slot = global_hook_shared().lock().unwrap();
        slot.clone()
    };
    let Some(shared) = shared else {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    };

    let data = &*(lparam as *const MSLLHOOKSTRUCT);
    if data.flags & LLMHF_INJECTED != 0 {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }

    let config = shared.current_config();
    let Some(route) = config.global_route() else {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    };
    let message = wparam as u32;

    match message {
        WM_XBUTTONDOWN | WM_XBUTTONUP => {
            let xbutton = hiword(data.mouseData);
            let control = match xbutton as u16 {
                XBUTTON1 => Some(LogicalControl::Back),
                XBUTTON2 => Some(LogicalControl::Forward),
                _ => None,
            };
            let Some(control) = control else {
                return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
            };

            if !route.handles_control(control) {
                return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
            }

            if message == WM_XBUTTONDOWN {
                shared.dispatch_route_control_action(route, control);
            }
            return 1;
        }
        WM_MBUTTONDOWN | WM_MBUTTONUP => {
            if !route.handles_control(LogicalControl::Middle) {
                return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
            }

            if message == WM_MBUTTONDOWN {
                shared.dispatch_route_control_action(route, LogicalControl::Middle);
            }
            return 1;
        }
        WM_MOUSEHWHEEL => {
            let delta = hiword(data.mouseData);
            if delta != 0 {
                let Some(control) = horizontal_scroll_control(delta) else {
                    return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
                };

                if route.handles_control(control) {
                    shared.dispatch_route_control_action(route, control);
                    return 1;
                }

                if route.device_settings.invert_horizontal_scroll {
                    inject_scroll(MOUSEEVENTF_HWHEEL, -delta);
                    return 1;
                }
            }
        }
        WM_MOUSEWHEEL => {
            let delta = hiword(data.mouseData);
            if delta != 0 && route.device_settings.invert_vertical_scroll {
                inject_scroll(MOUSEEVENTF_WHEEL, -delta);
                return 1;
            }
        }
        _ => {}
    }

    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

#[cfg(target_os = "windows")]
fn run_gesture_worker(shared: Arc<WindowsHookShared>, stop: Arc<AtomicBool>) {
    let mut sessions = BTreeMap::<String, GestureSession>::new();

    while !stop.load(Ordering::SeqCst) {
        let config = shared.current_config();
        let desired_routes = if config.enabled {
            config
                .routes
                .iter()
                .filter(|route| route.gesture_capture_requested())
                .cloned()
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        if desired_routes.is_empty() {
            for (_, mut session) in std::mem::take(&mut sessions) {
                session.shutdown();
            }
            shared.mark_gesture_connected(false, Some("Gesture listener parked".to_string()));
            let mut guard = shared.gesture_wait.lock().unwrap();
            while !stop.load(Ordering::SeqCst) && !shared.gesture_capture_requested() {
                guard = shared.gesture_cv.wait(guard).unwrap();
            }
            continue;
        }

        let desired_by_key = desired_routes
            .iter()
            .map(|route| (route.managed_device_key.as_str(), route))
            .collect::<BTreeMap<_, _>>();
        let stale_keys = sessions
            .iter()
            .filter_map(|(key, session)| {
                let desired = desired_by_key.get(key.as_str())?;
                (!session.matches_route(desired)).then(|| key.clone())
            })
            .collect::<Vec<_>>();
        for key in stale_keys {
            if let Some(mut session) = sessions.remove(&key) {
                session.shutdown();
            }
        }
        let removed_keys = sessions
            .keys()
            .filter(|key| !desired_by_key.contains_key(key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        for key in removed_keys {
            if let Some(mut session) = sessions.remove(&key) {
                session.shutdown();
            }
        }

        let api = match HidApi::new().map_err(map_hid_error) {
            Ok(api) => api,
            Err(error) => {
                shared.mark_gesture_connected(
                    false,
                    Some(format!("Gesture listener unavailable: {error}")),
                );
                let guard = shared.gesture_wait.lock().unwrap();
                let _ = shared
                    .gesture_cv
                    .wait_timeout(guard, Duration::from_millis(900))
                    .unwrap();
                continue;
            }
        };

        let mut last_error = None;
        for route in desired_routes {
            if sessions.contains_key(&route.managed_device_key) {
                continue;
            }
            match try_build_gesture_session_for_route(&shared, &route, &api) {
                Ok(session) => {
                    let source = session
                        .product_name
                        .clone()
                        .unwrap_or_else(|| format!("PID 0x{:04X}", session.product_id));
                    shared.push_event(
                        DebugEventKind::Info,
                        format!(
                            "Gesture listener attached to {} for {}",
                            source, route.managed_device_key
                        ),
                    );
                    sessions.insert(route.managed_device_key.clone(), session);
                }
                Err(error) => {
                    last_error = Some(error);
                }
            }
        }

        if sessions.is_empty() {
            shared.mark_gesture_connected(
                false,
                Some(format!(
                    "Gesture listener unavailable: {}",
                    last_error
                        .map(|error| error.to_string())
                        .unwrap_or_else(|| "no matching HID routes".to_string())
                )),
            );
            let guard = shared.gesture_wait.lock().unwrap();
            let _ = shared
                .gesture_cv
                .wait_timeout(guard, Duration::from_millis(500))
                .unwrap();
            continue;
        }

        shared.mark_gesture_connected(true, None);

        let mut disconnected = Vec::new();
        for (route_key, active_session) in sessions.iter_mut() {
            match active_session.device.read_packet(12) {
                Ok(packet) => {
                    if !packet.is_empty() {
                        active_session.handle_report(&shared, &packet);
                    }
                }
                Err(error) => {
                    shared.push_event(
                        DebugEventKind::Warning,
                        format!(
                            "Gesture listener lost HID stream for {}: {error}",
                            route_key
                        ),
                    );
                    disconnected.push(route_key.clone());
                }
            }
        }
        for route_key in disconnected {
            if let Some(mut session) = sessions.remove(&route_key) {
                session.shutdown();
            }
        }
    }

    for (_, mut session) in sessions {
        session.shutdown();
    }
    shared.mark_gesture_connected(false, None);
}

#[cfg(target_os = "windows")]
struct GestureSession {
    route: WindowsDeviceRoute,
    product_id: u16,
    product_name: Option<String>,
    device: HidDevice,
    dev_idx: u8,
    feature_idx: u8,
    routes: Vec<ReprogRoute>,
    active_cids: BTreeSet<u16>,
    gesture_active: bool,
    tracking_state: GestureTrackingState,
}

#[cfg(target_os = "windows")]
impl GestureSession {
    fn matches_route(&self, route: &WindowsDeviceRoute) -> bool {
        &self.route == route
    }

    fn route_key(&self) -> &str {
        &self.route.managed_device_key
    }

    fn handle_report(&mut self, shared: &WindowsHookShared, raw: &[u8]) {
        let Some((dev_idx, feature_idx, function, _sw, params)) = hidpp::parse_message(raw) else {
            return;
        };

        if dev_idx != self.dev_idx || feature_idx != self.feature_idx {
            return;
        }

        if function == 1 {
            if !self.rawxy_enabled() || !self.gesture_active || params.len() < 4 {
                return;
            }

            let delta_x = decode_s16(params[0], params[1]);
            let delta_y = decode_s16(params[2], params[3]);
            if delta_x != 0 || delta_y != 0 {
                self.handle_hid_rawxy_move(shared, delta_x, delta_y);
            }
            return;
        }

        if function != 0 {
            return;
        }

        let active_cids = collect_active_cids(&params);

        let changed_cids = active_cids
            .difference(&self.active_cids)
            .copied()
            .collect::<Vec<_>>();
        for cid in changed_cids {
            if self.gesture_cid_active(cid) {
                if !self.gesture_active {
                    self.gesture_active = true;
                    self.handle_hid_gesture_down(shared);
                }
                continue;
            }

            if let Some(control) = self.control_for_cid(cid) {
                shared.dispatch_route_control_action(&self.route, control);
            }
        }

        let gesture_now = active_cids.iter().any(|cid| self.gesture_cid_active(*cid));
        if !gesture_now && self.gesture_active {
            self.gesture_active = false;
            self.handle_hid_gesture_up(shared);
        }

        self.active_cids = active_cids;
    }

    fn handle_hid_gesture_down(&mut self, shared: &WindowsHookShared) {
        let route_key = self.route_key().to_string();
        let state = &mut self.tracking_state;
        if state.active {
            return;
        }

        state.active = true;
        state.triggered = false;
        shared.push_gesture_debug(format!("Gesture button down [{}]", route_key));

        if self.route.gesture_direction_enabled() && !cooldown_active(state) {
            start_gesture_tracking(state);
        } else {
            state.tracking = false;
            state.triggered = false;
        }
    }

    fn handle_hid_gesture_up(&mut self, shared: &WindowsHookShared) {
        let should_click = {
            let state = &mut self.tracking_state;
            if !state.active {
                return;
            }

            let should_click = !state.triggered;
            state.active = false;
            finish_gesture_tracking(state);
            state.triggered = false;
            should_click
        };

        shared.push_gesture_debug(format!(
            "Gesture button up [{}] click_candidate={should_click}",
            self.route_key(),
        ));

        if should_click {
            shared.dispatch_route_control_action(&self.route, LogicalControl::GesturePress);
        }
    }

    fn handle_hid_rawxy_move(&mut self, shared: &WindowsHookShared, delta_x: i16, delta_y: i16) {
        if !self.route.gesture_direction_enabled() || !self.tracking_state.active {
            return;
        }

        self.accumulate_gesture_delta(
            shared,
            f64::from(delta_x),
            f64::from(delta_y),
            GestureInputSource::HidRawxy,
        );
    }

    fn accumulate_gesture_delta(
        &mut self,
        shared: &WindowsHookShared,
        delta_x: f64,
        delta_y: f64,
        source: GestureInputSource,
    ) {
        let route_key = self.route_key().to_string();
        let state = &mut self.tracking_state;
        if !(self.route.gesture_direction_enabled() && state.active) {
            return;
        }

        if cooldown_active(state) {
            return;
        }

        if !state.tracking {
            shared.push_gesture_debug(format!(
                "Gesture tracking started via {} [{}]",
                source.as_str(),
                route_key,
            ));
            start_gesture_tracking(state);
        }

        let now = Instant::now();
        let idle_timed_out = state.last_move_at.is_some_and(|last_move_at| {
            now.duration_since(last_move_at).as_millis()
                > u128::from(self.route.device_settings.gesture_timeout_ms.max(250))
        });
        if idle_timed_out {
            shared.push_gesture_debug(format!(
                "Gesture segment reset after {} ms [{}]",
                self.route.device_settings.gesture_timeout_ms.max(250),
                route_key,
            ));
            start_gesture_tracking(state);
        }

        if state.input_source.is_some() && state.input_source != Some(source) {
            return;
        }
        state.input_source = Some(source);

        state.delta_x += delta_x;
        state.delta_y += delta_y;
        state.last_move_at = Some(now);

        if let Some(control) = detect_gesture_control(
            state.delta_x,
            state.delta_y,
            f64::from(self.route.device_settings.gesture_threshold.max(5)),
            f64::from(self.route.device_settings.gesture_deadzone),
        ) {
            state.triggered = true;
            shared.push_gesture_debug(format!(
                "Gesture detected {} source={} dx={} dy={} [{}]",
                control.label(),
                source.as_str(),
                state.delta_x as i32,
                state.delta_y as i32,
                route_key,
            ));
            shared.dispatch_route_control_action(&self.route, control);
            state.cooldown_until = Some(
                Instant::now()
                    + Duration::from_millis(u64::from(
                        self.route.device_settings.gesture_cooldown_ms,
                    )),
            );
            finish_gesture_tracking(state);
        }
    }

    fn gesture_cid_active(&self, cid: u16) -> bool {
        self.routes.iter().any(|route| {
            route.control == LogicalControl::GesturePress && route.cids.contains(&cid)
        })
    }

    fn control_for_cid(&self, cid: u16) -> Option<LogicalControl> {
        self.routes.iter().find_map(|route| {
            (route.control != LogicalControl::GesturePress && route.cids.contains(&cid))
                .then_some(route.control)
        })
    }

    fn rawxy_enabled(&self) -> bool {
        self.routes
            .iter()
            .any(|route| route.control == LogicalControl::GesturePress && route.rawxy_enabled)
    }

    fn shutdown(&mut self) {
        for route in &self.routes {
            let flags = if route.rawxy_enabled {
                GESTURE_UNDIVERT_RAWXY_FLAGS
            } else {
                GESTURE_UNDIVERT_FLAGS
            };
            for cid in &route.cids {
                let _ = hidpp::write_request(
                    &self.device,
                    self.dev_idx,
                    self.feature_idx,
                    3,
                    &[
                        ((cid >> 8) & 0xFF) as u8,
                        (cid & 0xFF) as u8,
                        flags,
                        0x00,
                        0x00,
                    ],
                );
            }
        }
        self.gesture_active = false;
        self.active_cids.clear();
        self.tracking_state = GestureTrackingState::default();
    }
}

#[cfg(target_os = "windows")]
fn try_build_gesture_session_for_route(
    shared: &WindowsHookShared,
    route: &WindowsDeviceRoute,
    api: &HidApi,
) -> Result<GestureSession, PlatformError> {
    let mut last_error = None;

    for info in vendor_hid_infos(api)
        .into_iter()
        .filter(|info| hid_info_matches_route(info, route))
    {
        let Ok(device) = info.open_device(&api) else {
            continue;
        };

        match initialize_gesture_session(shared, route.clone(), info, device) {
            Ok(session) => return Ok(session),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        PlatformError::Message("no Logitech gesture-capable HID interface found".to_string())
    }))
}

#[cfg(target_os = "windows")]
fn initialize_gesture_session(
    shared: &WindowsHookShared,
    route: WindowsDeviceRoute,
    info: &HidDeviceInfo,
    device: HidDevice,
) -> Result<GestureSession, PlatformError> {
    let routes = route.reprog_routes();
    if routes.is_empty() {
        return Err(PlatformError::Message(
            "no active Logitech REPROG routes configured".to_string(),
        ));
    }

    for dev_idx in DEVICE_INDICES {
        let Some(feature_idx) = find_hidpp_feature(&device, dev_idx, FEAT_REPROG_V4, 250)? else {
            continue;
        };

        if try_initialize_reprog_session(shared, &device, dev_idx, feature_idx, &routes)? {
            return Ok(GestureSession {
                route,
                product_id: info.product_id(),
                product_name: info.product_string().map(str::to_string),
                device,
                dev_idx,
                feature_idx,
                routes: routes.to_vec(),
                active_cids: BTreeSet::new(),
                gesture_active: false,
                tracking_state: GestureTrackingState::default(),
            });
        }
    }

    Err(PlatformError::Message(format!(
        "logitech reprog diversion failed for pid 0x{:04X}",
        info.product_id()
    )))
}

#[cfg(target_os = "windows")]
fn try_initialize_reprog_session(
    shared: &WindowsHookShared,
    device: &HidDevice,
    dev_idx: u8,
    feature_idx: u8,
    routes: &[ReprogRoute],
) -> Result<bool, PlatformError> {
    let mut diverted = Vec::new();

    for route in routes {
        for cid in &route.cids {
            let flags = if route.rawxy_enabled {
                GESTURE_RAWXY_FLAGS
            } else {
                GESTURE_DIVERT_FLAGS
            };
            if set_gesture_reporting(&device, dev_idx, feature_idx, *cid, flags, 250)?.is_some() {
                diverted.push((route.control, *cid, route.rawxy_enabled));
                shared.push_gesture_debug(format!(
                    "Diverted cid 0x{:04X} for {} via devIdx=0x{:02X} rawxy={}",
                    cid,
                    route.control.label(),
                    dev_idx,
                    route.rawxy_enabled
                ));
            } else {
                for (_, diverted_cid, diverted_rawxy) in &diverted {
                    let reset_flags = if *diverted_rawxy {
                        GESTURE_UNDIVERT_RAWXY_FLAGS
                    } else {
                        GESTURE_UNDIVERT_FLAGS
                    };
                    let _ = hidpp::write_request(
                        device,
                        dev_idx,
                        feature_idx,
                        3,
                        &[
                            ((diverted_cid >> 8) & 0xFF) as u8,
                            (diverted_cid & 0xFF) as u8,
                            reset_flags,
                            0x00,
                            0x00,
                        ],
                    );
                }
                return Ok(false);
            }
        }
    }

    Ok(true)
}

#[cfg(target_os = "windows")]
fn collect_active_cids(params: &[u8]) -> BTreeSet<u16> {
    params
        .chunks_exact(2)
        .take_while(|pair| pair[0] != 0 || pair[1] != 0)
        .map(|pair| u16::from(pair[0]) << 8 | u16::from(pair[1]))
        .collect()
}

#[cfg(target_os = "windows")]
fn vendor_hid_infos(api: &HidApi) -> Vec<&HidDeviceInfo> {
    api.device_list()
        .filter(|info| info.vendor_id() == LOGI_VID && info.usage_page() >= 0xFF00)
        .collect()
}

#[cfg(target_os = "windows")]
fn probe_device_telemetry(
    device: &HidDevice,
    plan: &TelemetryProbePlan,
) -> DeviceTelemetrySnapshot {
    DeviceTelemetrySnapshot {
        current_dpi: if plan.probe_dpi {
            read_hidpp_current_dpi(device)
                .ok()
                .flatten()
                .or(plan.cached.current_dpi)
        } else {
            plan.cached.current_dpi
        },
        battery: if plan.probe_battery {
            read_hidpp_battery(device)
                .ok()
                .flatten()
                .or(plan.cached.battery)
        } else {
            plan.cached.battery
        },
    }
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
fn hid_info_matches_route(info: &HidDeviceInfo, route: &WindowsDeviceRoute) -> bool {
    if let Some(identity_key) =
        normalized_identity_key(route.live_device.fingerprint.identity_key.as_deref())
    {
        return normalized_identity_key(fingerprint_from_hid_info(info).identity_key.as_deref())
            == Some(identity_key);
    }

    Some(route.live_device.model_key.as_str()).is_some_and(|model_key| {
        resolve_known_device(Some(info.product_id()), info.product_string())
            .map(|spec| spec.key == model_key)
            .unwrap_or(false)
    })
}

#[cfg(target_os = "windows")]
fn normalized_identity_key(value: Option<&str>) -> Option<&str> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn should_probe_dpi(entry: Option<&DeviceTelemetryCacheEntry>, now: Instant) -> bool {
    let Some(entry) = entry else {
        return true;
    };

    !entry.connected
        || entry.current_dpi.is_none()
        || entry
            .verify_after
            .is_some_and(|verify_after| now >= verify_after)
}

fn should_probe_battery(entry: Option<&DeviceTelemetryCacheEntry>, now: Instant) -> bool {
    let Some(entry) = entry else {
        return true;
    };

    !entry.connected || now.duration_since(entry.last_battery_probe_at) >= BATTERY_CACHE_TTL
}

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

#[cfg(target_os = "windows")]
fn push_unique_device(devices: &mut Vec<DeviceInfo>, device: DeviceInfo) {
    if devices.iter().all(|existing| existing.key != device.key) {
        devices.push(device);
    }
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
fn transport_label(bus_type: BusType) -> &'static str {
    match bus_type {
        BusType::Bluetooth => "Bluetooth Low Energy",
        BusType::Usb => "USB",
        BusType::I2c => "I2C",
        BusType::Spi => "SPI",
        BusType::Unknown => "Unknown transport",
    }
}

#[cfg(target_os = "windows")]
fn set_hidpp_dpi(device: &HidDevice, dpi: u16) -> Result<bool, PlatformError> {
    hidpp::set_sensor_dpi(device, BT_DEV_IDX, dpi, 1_500)
}

#[cfg(target_os = "windows")]
fn read_hidpp_current_dpi(device: &HidDevice) -> Result<Option<u16>, PlatformError> {
    hidpp::read_sensor_dpi(device, BT_DEV_IDX, 1_500)
}

#[cfg(target_os = "windows")]
fn read_hidpp_battery(device: &HidDevice) -> Result<Option<DeviceBatteryInfo>, PlatformError> {
    hidpp::read_battery_info(device, BT_DEV_IDX, 1_500)
}

#[cfg(target_os = "windows")]
fn find_hidpp_feature(
    device: &HidDevice,
    dev_idx: u8,
    feature_id: u16,
    timeout_ms: i32,
) -> Result<Option<u8>, PlatformError> {
    hidpp::find_feature(device, dev_idx, feature_id, timeout_ms)
}

#[cfg(target_os = "windows")]
fn set_gesture_reporting(
    device: &HidDevice,
    dev_idx: u8,
    feature_idx: u8,
    gesture_cid: u16,
    flags: u8,
    timeout_ms: i32,
) -> Result<Option<(u8, u8, u8, u8, Vec<u8>)>, PlatformError> {
    hidpp::request(
        device,
        dev_idx,
        feature_idx,
        3,
        &[
            ((gesture_cid >> 8) & 0xFF) as u8,
            (gesture_cid & 0xFF) as u8,
            flags,
            0x00,
            0x00,
        ],
        timeout_ms,
    )
}

fn cooldown_active(state: &GestureTrackingState) -> bool {
    state
        .cooldown_until
        .is_some_and(|cooldown_until| Instant::now() < cooldown_until)
}

fn start_gesture_tracking(state: &mut GestureTrackingState) {
    let now = Instant::now();
    state.tracking = true;
    state.started_at = Some(now);
    state.last_move_at = Some(now);
    state.delta_x = 0.0;
    state.delta_y = 0.0;
    state.input_source = None;
}

fn finish_gesture_tracking(state: &mut GestureTrackingState) {
    state.tracking = false;
    state.started_at = None;
    state.last_move_at = None;
    state.delta_x = 0.0;
    state.delta_y = 0.0;
    state.input_source = None;
}

fn detect_gesture_control(
    delta_x: f64,
    delta_y: f64,
    threshold: f64,
    deadzone: f64,
) -> Option<LogicalControl> {
    gesture::detect_gesture_control(delta_x, delta_y, threshold, deadzone)
}

#[cfg(target_os = "windows")]
fn decode_s16(hi: u8, lo: u8) -> i16 {
    let value = u16::from(hi) << 8 | u16::from(lo);
    value as i16
}

#[cfg(target_os = "windows")]
fn hiword(value: u32) -> i32 {
    let mut hi = ((value >> 16) & 0xFFFF) as i32;
    if hi >= 0x8000 {
        hi -= 0x1_0000;
    }
    hi
}

#[cfg(target_os = "windows")]
fn inject_scroll(flags: u32, delta: i32) {
    unsafe {
        mouse_event(flags, 0, 0, delta, 0);
    }
}

#[cfg(target_os = "windows")]
fn windows_start_menu_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(app_data) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(app_data).join("Microsoft/Windows/Start Menu/Programs"));
    }
    if let Some(program_data) = std::env::var_os("ProgramData") {
        roots.push(PathBuf::from(program_data).join("Microsoft/Windows/Start Menu/Programs"));
    }
    roots
}

#[cfg(target_os = "windows")]
fn collect_shortcut_apps(root: &Path, apps: &mut Vec<InstalledApp>) -> Result<(), PlatformError> {
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
        if path.is_dir() {
            collect_shortcut_apps(&path, apps)?;
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("lnk") {
            continue;
        }
        if let Some(app) = read_shortcut_app(&path) {
            apps.push(app);
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn read_shortcut_app(path: &Path) -> Option<InstalledApp> {
    let shortcut = ShellLink::open(path, WINDOWS_1252).ok()?;
    let target_path = shortcut.link_target()?;
    let label = shortcut_display_label(path, &shortcut)?;
    let icon_source = shortcut
        .string_data()
        .icon_location()
        .as_deref()
        .and_then(parse_windows_icon_location);

    if let Some(package_family_name) = shortcut_package_family_name(
        &target_path,
        shortcut.string_data().command_line_arguments().as_deref(),
    ) {
        return Some(InstalledApp {
            identity: AppIdentity {
                label: Some(label),
                executable: None,
                executable_path: None,
                bundle_id: None,
                package_family_name: Some(package_family_name),
            },
            source_kinds: vec![
                AppDiscoverySource::Package,
                AppDiscoverySource::StartMenuShortcut,
            ],
            source_path: icon_source.or_else(|| Some(path.to_string_lossy().to_string())),
        });
    }

    let target = PathBuf::from(&target_path);
    let executable = target.file_name()?.to_string_lossy().to_string();
    if !executable.to_ascii_lowercase().ends_with(".exe") {
        return None;
    }

    Some(InstalledApp {
        identity: AppIdentity {
            label: Some(label),
            executable: Some(executable),
            executable_path: Some(target_path.clone()),
            bundle_id: None,
            package_family_name: None,
        },
        source_kinds: vec![AppDiscoverySource::StartMenuShortcut],
        source_path: icon_source.or(Some(target_path)),
    })
}

#[cfg(target_os = "windows")]
fn shortcut_display_label(path: &Path, shortcut: &ShellLink) -> Option<String> {
    shortcut
        .string_data()
        .name_string()
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            path.file_stem()
                .map(|value| value.to_string_lossy().trim().to_string())
        })
        .filter(|value| {
            let lower = value.to_ascii_lowercase();
            !value.is_empty()
                && !lower.contains("uninstall")
                && !lower.contains("help")
                && !lower.contains("readme")
        })
}

#[cfg(target_os = "windows")]
fn parse_windows_icon_location(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }

    let candidate = trimmed.split(',').next()?.trim().trim_matches('"');
    if candidate.is_empty() {
        return None;
    }

    if candidate.starts_with("shell:") {
        return Some(candidate.to_string());
    }

    Some(candidate.to_string())
}

#[cfg(target_os = "windows")]
fn shortcut_package_family_name(target_path: &str, arguments: Option<&str>) -> Option<String> {
    let target_name = Path::new(target_path)
        .file_name()
        .map(|value| value.to_string_lossy().to_ascii_lowercase());
    if target_name.as_deref() != Some("explorer.exe") {
        return None;
    }

    arguments.and_then(shell_apps_package_family_name)
}

#[cfg(target_os = "windows")]
fn shell_apps_package_family_name(value: &str) -> Option<String> {
    let normalized = value.trim().trim_matches('"').replace('/', "\\");
    let marker = "shell:AppsFolder\\";
    let suffix = normalized.get(normalized.find(marker)? + marker.len()..)?;
    let (family, _) = suffix.split_once('!')?;
    let trimmed = family.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(target_os = "windows")]
fn collect_app_path_apps(apps: &mut Vec<InstalledApp>) -> Result<(), PlatformError> {
    let roots = [
        (HKEY_CURRENT_USER, KEY_READ | KEY_WOW64_64KEY),
        (HKEY_CURRENT_USER, KEY_READ | KEY_WOW64_32KEY),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_32KEY),
    ];

    for (hive, flags) in roots {
        let root = RegKey::predef(hive);
        let Ok(app_paths) = root.open_subkey_with_flags(
            "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths",
            flags,
        ) else {
            continue;
        };

        for subkey_name in app_paths.enum_keys().flatten() {
            let Ok(subkey) = app_paths.open_subkey_with_flags(&subkey_name, flags) else {
                continue;
            };
            let raw_path = read_registry_string(&subkey, "").or_else(|| {
                read_registry_string(&subkey, "Path").map(|dir| format!("{dir}\\{subkey_name}"))
            });
            let Some(executable_path) = raw_path
                .as_deref()
                .and_then(normalize_registry_executable_path)
            else {
                continue;
            };
            let executable = Path::new(&executable_path)
                .file_name()
                .map(|value| value.to_string_lossy().to_string());
            let label = Path::new(&executable_path)
                .file_stem()
                .map(|value| value.to_string_lossy().to_string());
            apps.push(InstalledApp {
                identity: AppIdentity {
                    label,
                    executable,
                    executable_path: Some(executable_path.clone()),
                    bundle_id: None,
                    package_family_name: None,
                },
                source_kinds: vec![AppDiscoverySource::Registry],
                source_path: Some(executable_path),
            });
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn collect_uninstall_registry_apps(apps: &mut Vec<InstalledApp>) -> Result<(), PlatformError> {
    let roots = [
        (HKEY_CURRENT_USER, KEY_READ | KEY_WOW64_64KEY),
        (HKEY_CURRENT_USER, KEY_READ | KEY_WOW64_32KEY),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_32KEY),
    ];

    for (hive, flags) in roots {
        let root = RegKey::predef(hive);
        let Ok(uninstall) = root.open_subkey_with_flags(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            flags,
        ) else {
            continue;
        };

        for subkey_name in uninstall.enum_keys().flatten() {
            let Ok(subkey) = uninstall.open_subkey_with_flags(&subkey_name, flags) else {
                continue;
            };
            if read_registry_u32(&subkey, "SystemComponent").unwrap_or_default() == 1 {
                continue;
            }

            let Some(label) = read_registry_string(&subkey, "DisplayName")
                .map(|value| value.trim().to_string())
                .filter(|value| is_user_facing_app_label(value))
            else {
                continue;
            };

            let executable_path = read_registry_string(&subkey, "DisplayIcon")
                .as_deref()
                .and_then(normalize_registry_executable_path)
                .or_else(|| {
                    read_registry_string(&subkey, "InstallLocation")
                        .as_deref()
                        .and_then(first_executable_in_directory)
                });
            let Some(executable_path) = executable_path else {
                continue;
            };

            let executable = Path::new(&executable_path)
                .file_name()
                .map(|value| value.to_string_lossy().to_string());
            apps.push(InstalledApp {
                identity: AppIdentity {
                    label: Some(label),
                    executable,
                    executable_path: Some(executable_path.clone()),
                    bundle_id: None,
                    package_family_name: None,
                },
                source_kinds: vec![AppDiscoverySource::Registry],
                source_path: Some(executable_path),
            });
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn collect_package_apps(apps: &mut Vec<InstalledApp>) -> Result<(), PlatformError> {
    let Some(json) = powershell_json(include_str!("windows_appx_discovery.ps1"))? else {
        return Ok(());
    };

    for entry in json_items(&json) {
        let Some(package_family_name) = json_string(entry, "packageFamilyName") else {
            continue;
        };

        let label = json_string(entry, "label");
        let executable = json_string(entry, "executable");
        let executable_path = json_string(entry, "executablePath");
        let source_path = json_string(entry, "sourcePath")
            .or_else(|| executable_path.clone())
            .or_else(|| {
                json_string(entry, "appId").map(|app_id| format!("shell:AppsFolder\\{app_id}"))
            });

        apps.push(InstalledApp {
            identity: AppIdentity {
                label,
                executable,
                executable_path,
                bundle_id: None,
                package_family_name: Some(package_family_name),
            },
            source_kinds: vec![AppDiscoverySource::Package],
            source_path,
        });
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn collect_running_process_apps(apps: &mut Vec<InstalledApp>) -> Result<(), PlatformError> {
    let Some(json) = powershell_json(include_str!("windows_running_apps.ps1"))? else {
        return Ok(());
    };

    for entry in json_items(&json) {
        let Some(executable_path) = json_string(entry, "executablePath") else {
            continue;
        };
        let executable = json_string(entry, "executable").or_else(|| {
            Path::new(&executable_path)
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
        });
        let label = json_string(entry, "label");
        apps.push(InstalledApp {
            identity: AppIdentity {
                label,
                executable,
                executable_path: Some(executable_path.clone()),
                bundle_id: None,
                package_family_name: json_string(entry, "packageFamilyName"),
            },
            source_kinds: vec![AppDiscoverySource::RunningProcess],
            source_path: Some(executable_path),
        });
    }

    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn load_native_app_icon(source_path: &str) -> Result<Option<String>, PlatformError> {
    let trimmed = source_path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if let Some(data_url) = image_file_data_url(trimmed)? {
        return Ok(Some(data_url));
    }

    if trimmed.starts_with("shell:AppsFolder\\") {
        return Ok(None);
    }

    extract_windows_icon_as_data_url(trimmed)
}

#[cfg(target_os = "windows")]
fn image_file_data_url(source_path: &str) -> Result<Option<String>, PlatformError> {
    let path = Path::new(source_path);
    let Some(extension) = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
    else {
        return Ok(None);
    };

    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        _ => return Ok(None),
    };

    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };
    if bytes.is_empty() {
        return Ok(None);
    }

    Ok(Some(format!(
        "data:{mime};base64,{}",
        BASE64_STANDARD.encode(bytes)
    )))
}

#[cfg(target_os = "windows")]
fn extract_windows_icon_as_data_url(source_path: &str) -> Result<Option<String>, PlatformError> {
    let stdout = powershell_stdout(include_str!("windows_icon_extract.ps1"), &[source_path])?;
    let encoded = stdout.trim();
    if encoded.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!("data:image/png;base64,{encoded}")))
}

#[cfg(target_os = "windows")]
fn powershell_json(script: &str) -> Result<Option<JsonValue>, PlatformError> {
    let stdout = powershell_stdout(script, &[])?;
    if stdout.trim().is_empty() {
        return Ok(None);
    }

    serde_json::from_str(stdout.trim())
        .map(Some)
        .map_err(|error| {
            PlatformError::Message(format!("failed to parse PowerShell JSON: {error}"))
        })
}

#[cfg(target_os = "windows")]
fn powershell_stdout(script: &str, args: &[&str]) -> Result<String, PlatformError> {
    let mut command = Command::new("powershell.exe");
    command
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script);
    for arg in args {
        command.arg(arg);
    }

    let output = match command.output() {
        Ok(output) => output,
        Err(_) => return Ok(String::new()),
    };

    if !output.status.success() {
        return Ok(String::new());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "windows")]
fn json_items(value: &JsonValue) -> Vec<&JsonValue> {
    match value {
        JsonValue::Array(items) => items.iter().collect(),
        JsonValue::Object(_) => vec![value],
        _ => Vec::new(),
    }
}

#[cfg(target_os = "windows")]
fn json_string(value: &JsonValue, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(target_os = "windows")]
fn read_registry_string(key: &RegKey, name: &str) -> Option<String> {
    key.get_value::<String, _>(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "windows")]
fn read_registry_u32(key: &RegKey, name: &str) -> Option<u32> {
    key.get_value::<u32, _>(name).ok()
}

#[cfg(target_os = "windows")]
fn normalize_registry_executable_path(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }

    let comma_trimmed = trimmed.split(',').next()?.trim().trim_matches('"');
    let lower = comma_trimmed.to_ascii_lowercase();
    for extension in [".exe", ".ico", ".png"] {
        if let Some(index) = lower.find(extension) {
            let end = index + extension.len();
            return Some(comma_trimmed[..end].to_string());
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn first_executable_in_directory(value: &str) -> Option<String> {
    let path = Path::new(value.trim().trim_matches('"'));
    if !path.is_dir() {
        return None;
    }

    let mut candidates = fs::read_dir(path)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|entry| {
            entry
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .map(|candidate| candidate.to_string_lossy().to_string())
}

#[cfg(target_os = "windows")]
fn is_user_facing_app_label(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    !lower.is_empty()
        && !lower.contains("uninstall")
        && !lower.contains("helper")
        && !lower.contains("setup")
}

#[cfg(target_os = "windows")]
unsafe fn foreground_app_identity(hwnd: HWND) -> Result<Option<AppIdentity>, PlatformError> {
    let Some(identity) = process_identity_for_window(hwnd)? else {
        return Ok(None);
    };

    let executable = identity
        .executable
        .as_deref()
        .map(|value| value.to_ascii_lowercase());
    if executable.as_deref() == Some("applicationframehost.exe") {
        return resolve_uwp_child_identity(hwnd);
    }

    if executable.as_deref() == Some("explorer.exe") {
        let class_name = window_class(hwnd).unwrap_or_default();
        if !EXPLORER_CLASSES
            .iter()
            .any(|explorer_class| *explorer_class == class_name)
        {
            if let Some(resolved) = resolve_uwp_child_identity(hwnd)? {
                return Ok(Some(resolved));
            }
            return find_uwp_app_global_identity();
        }
    }

    Ok(Some(identity))
}

#[cfg(target_os = "windows")]
unsafe fn process_identity_for_window(hwnd: HWND) -> Result<Option<AppIdentity>, PlatformError> {
    let mut process_id = 0;
    GetWindowThreadProcessId(hwnd, &mut process_id);
    process_identity_for_pid(process_id, Some(hwnd))
}

#[cfg(target_os = "windows")]
unsafe fn process_identity_for_pid(
    process_id: u32,
    label_hwnd: Option<HWND>,
) -> Result<Option<AppIdentity>, PlatformError> {
    if process_id == 0 {
        return Ok(None);
    }

    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
    if process.is_null() {
        return Ok(None);
    }

    let result = (|| {
        let executable_path = process_image_path(process)?;
        let executable = Path::new(&executable_path)
            .file_name()
            .map(|value| value.to_string_lossy().to_string());
        let label = label_hwnd
            .and_then(|hwnd| unsafe { window_title(hwnd) })
            .or_else(|| {
                Path::new(&executable_path)
                    .file_stem()
                    .map(|value| value.to_string_lossy().to_string())
            })
            .or_else(|| executable.clone());

        Ok(Some(AppIdentity {
            label,
            executable,
            executable_path: Some(executable_path),
            bundle_id: None,
            package_family_name: package_family_name_for_process(process),
        }))
    })();

    CloseHandle(process as HANDLE);
    result
}

#[cfg(target_os = "windows")]
unsafe fn package_family_name_for_process(process: HANDLE) -> Option<String> {
    let mut length = PACKAGE_FAMILY_NAME_MAX_LENGTH + 1;
    loop {
        let mut buffer = vec![0u16; length as usize];
        let status = GetPackageFamilyName(process, &mut length, buffer.as_mut_ptr());
        if status == APPMODEL_ERROR_NO_PACKAGE {
            return None;
        }
        if status == 0 {
            let end = buffer
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(buffer.len());
            return Some(String::from_utf16_lossy(&buffer[..end]));
        }
        if status != 122 || length == 0 {
            return None;
        }
    }
}

#[cfg(target_os = "windows")]
struct UwpChildSearch {
    host_pid: u32,
    resolved: Option<AppIdentity>,
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn enum_uwp_child_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam as *mut UwpChildSearch);
    let mut child_pid = 0;
    GetWindowThreadProcessId(hwnd, &mut child_pid);
    if child_pid == 0 || child_pid == search.host_pid {
        return 1;
    }

    if let Ok(Some(identity)) = process_identity_for_pid(child_pid, Some(hwnd)) {
        if !identity
            .executable
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("applicationframehost.exe"))
        {
            search.resolved = Some(identity);
            return 0;
        }
    }

    1
}

#[cfg(target_os = "windows")]
unsafe fn resolve_uwp_child_identity(hwnd: HWND) -> Result<Option<AppIdentity>, PlatformError> {
    let mut host_pid = 0;
    GetWindowThreadProcessId(hwnd, &mut host_pid);
    if host_pid == 0 {
        return Ok(None);
    }

    let mut search = UwpChildSearch {
        host_pid,
        resolved: None,
    };
    EnumChildWindows(
        hwnd,
        Some(enum_uwp_child_windows),
        &mut search as *mut UwpChildSearch as LPARAM,
    );
    Ok(search.resolved)
}

#[cfg(target_os = "windows")]
struct GlobalUwpSearch {
    resolved: Option<AppIdentity>,
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn enum_visible_top_level_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }

    let search = &mut *(lparam as *mut GlobalUwpSearch);
    let Ok(Some(identity)) = process_identity_for_window(hwnd) else {
        return 1;
    };

    if identity
        .executable
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("applicationframehost.exe"))
    {
        if let Ok(Some(resolved)) = resolve_uwp_child_identity(hwnd) {
            search.resolved = Some(resolved);
            return 0;
        }
    }

    1
}

#[cfg(target_os = "windows")]
unsafe fn find_uwp_app_global_identity() -> Result<Option<AppIdentity>, PlatformError> {
    let mut search = GlobalUwpSearch { resolved: None };
    EnumWindows(
        Some(enum_visible_top_level_windows),
        &mut search as *mut GlobalUwpSearch as LPARAM,
    );
    Ok(search.resolved)
}

#[cfg(target_os = "windows")]
unsafe fn window_title(hwnd: HWND) -> Option<String> {
    let length = GetWindowTextLengthW(hwnd);
    if length <= 0 {
        return None;
    }

    let mut buffer = vec![0u16; length as usize + 1];
    let written = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    if written <= 0 {
        return None;
    }

    let title = String::from_utf16_lossy(&buffer[..written as usize]);
    let trimmed = title.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(target_os = "windows")]
unsafe fn window_class(hwnd: HWND) -> Option<String> {
    let mut buffer = vec![0u16; 256];
    let written = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    if written <= 0 {
        return None;
    }

    Some(String::from_utf16_lossy(&buffer[..written as usize]))
}

#[cfg(target_os = "windows")]
unsafe fn process_image_path(process: HANDLE) -> Result<String, PlatformError> {
    let mut buffer = vec![0u16; 32_768];
    let mut size = buffer.len() as u32;
    if QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut size) == 0 {
        return Err(PlatformError::Message(
            "QueryFullProcessImageNameW failed".to_string(),
        ));
    }

    Ok(String::from_utf16_lossy(&buffer[..size as usize]))
}

#[cfg(target_os = "windows")]
fn execute_action(action_id: &str) -> Result<(), PlatformError> {
    let keys = match action_id {
        "alt_tab" => vec![VK_MENU as u16, VK_TAB as u16],
        "alt_shift_tab" => vec![VK_MENU as u16, VK_SHIFT as u16, VK_TAB as u16],
        "show_desktop" => vec![VK_LWIN as u16, b'D' as u16],
        "task_view" | "mission_control" | "app_expose" => {
            vec![VK_LWIN as u16, VK_TAB as u16]
        }
        "launchpad" => vec![VK_LWIN as u16],
        "space_left" => vec![VK_LWIN as u16, VK_CONTROL as u16, VK_LEFT as u16],
        "space_right" => vec![VK_LWIN as u16, VK_CONTROL as u16, VK_RIGHT as u16],
        "browser_back" => vec![VK_BROWSER_BACK as u16],
        "browser_forward" => vec![VK_BROWSER_FORWARD as u16],
        "close_tab" => vec![VK_CONTROL as u16, b'W' as u16],
        "new_tab" => vec![VK_CONTROL as u16, b'T' as u16],
        "copy" => vec![VK_CONTROL as u16, b'C' as u16],
        "paste" => vec![VK_CONTROL as u16, b'V' as u16],
        "cut" => vec![VK_CONTROL as u16, b'X' as u16],
        "undo" => vec![VK_CONTROL as u16, b'Z' as u16],
        "redo" => vec![VK_CONTROL as u16, b'Y' as u16],
        "select_all" => vec![VK_CONTROL as u16, b'A' as u16],
        "save" => vec![VK_CONTROL as u16, b'S' as u16],
        "find" => vec![VK_CONTROL as u16, b'F' as u16],
        "screen_capture" => vec![VK_LWIN as u16, VK_SHIFT as u16, b'S' as u16],
        "emoji_picker" => vec![VK_LWIN as u16, VK_OEM_PERIOD as u16],
        "volume_up" => vec![VK_VOLUME_UP as u16],
        "volume_down" => vec![VK_VOLUME_DOWN as u16],
        "volume_mute" => vec![VK_VOLUME_MUTE as u16],
        "play_pause" => vec![VK_MEDIA_PLAY_PAUSE as u16],
        "next_track" => vec![VK_MEDIA_NEXT_TRACK as u16],
        "prev_track" => vec![VK_MEDIA_PREV_TRACK as u16],
        "none" => Vec::new(),
        other => {
            return Err(PlatformError::Unsupported(match other {
                _ => "action is not implemented on Windows",
            }));
        }
    };

    send_key_combo(&keys);
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn execute_action(_action_id: &str) -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported(
        "Windows actions are only available on Windows",
    ))
}

#[cfg(target_os = "windows")]
fn send_key_combo(keys: &[u16]) {
    if keys.is_empty() {
        return;
    }

    let mut inputs = Vec::with_capacity(keys.len() * 2);
    for &vk in keys {
        inputs.push(key_input(vk, false));
    }
    for &vk in keys.iter().rev() {
        inputs.push(key_input(vk, true));
    }

    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
    }
}

#[cfg(target_os = "windows")]
fn key_input(vk: u16, key_up: bool) -> INPUT {
    let mut flags = if key_up { KEYEVENTF_KEYUP } else { 0 };
    if is_extended_key(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(target_os = "windows")]
fn is_extended_key(vk: u16) -> bool {
    [
        VK_BROWSER_BACK as u16,
        VK_BROWSER_FORWARD as u16,
        VK_VOLUME_MUTE as u16,
        VK_VOLUME_DOWN as u16,
        VK_VOLUME_UP as u16,
        VK_MEDIA_NEXT_TRACK as u16,
        VK_MEDIA_PREV_TRACK as u16,
        VK_MEDIA_PLAY_PAUSE as u16,
        VK_LEFT as u16,
        VK_RIGHT as u16,
        VK_UP as u16,
        VK_DOWN as u16,
        VK_TAB as u16,
    ]
    .contains(&vk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_cache_key_prefers_identity_then_serial() {
        let fingerprint = DeviceFingerprint {
            identity_key: Some("mx-master-3s".to_string()),
            serial_number: Some("abc123".to_string()),
            hid_path: Some("hid#path".to_string()),
            interface_number: Some(1),
            usage_page: Some(0xFF00),
            usage: Some(1),
            location_id: None,
        };

        assert_eq!(
            telemetry_cache_key(Some(0xB034), Some("USB"), &fingerprint),
            "identity:mx-master-3s"
        );

        let mut serial_only = fingerprint.clone();
        serial_only.identity_key = None;
        assert_eq!(
            telemetry_cache_key(Some(0xB034), Some("USB"), &serial_only),
            "serial:b034:abc123"
        );
    }

    #[test]
    fn telemetry_probe_policy_handles_ttl_reconnect_and_verify() {
        let now = Instant::now();
        let fresh = DeviceTelemetryCacheEntry {
            current_dpi: Some(1200),
            battery: Some(DeviceBatteryInfo {
                kind: mouser_core::DeviceBatteryKind::Percentage,
                percentage: Some(70),
                label: "70%".to_string(),
                source_feature: None,
                raw_capabilities: Vec::new(),
                raw_status: Vec::new(),
            }),
            last_battery_probe_at: now,
            verify_after: None,
            connected: true,
        };

        assert!(!should_probe_dpi(Some(&fresh), now));
        assert!(!should_probe_battery(Some(&fresh), now));

        let disconnected = DeviceTelemetryCacheEntry {
            connected: false,
            ..fresh.clone()
        };
        assert!(should_probe_dpi(Some(&disconnected), now));
        assert!(should_probe_battery(Some(&disconnected), now));

        let stale_battery = DeviceTelemetryCacheEntry {
            last_battery_probe_at: now - BATTERY_CACHE_TTL,
            ..fresh.clone()
        };
        assert!(!should_probe_dpi(Some(&stale_battery), now));
        assert!(should_probe_battery(Some(&stale_battery), now));

        let verify_due = DeviceTelemetryCacheEntry {
            verify_after: Some(now),
            ..fresh
        };
        assert!(should_probe_dpi(Some(&verify_due), now));
        assert!(!should_probe_battery(Some(&verify_due), now));
    }
}
