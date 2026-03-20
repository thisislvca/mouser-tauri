#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, OnceLock, RwLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use mouser_core::{
    build_connected_device_info, hydrate_identity_key, AppDiscoverySource, AppIdentity,
    DebugEventKind, DeviceFingerprint, DeviceInfo, InstalledApp, LogicalControl, Profile,
};

use crate::{
    dedupe_installed_apps, gesture,
    hidpp::{self, HidppIo, BT_DEV_IDX},
    horizontal_scroll_control, push_bounded_hook_event, AppDiscoveryBackend, AppFocusBackend,
    HidBackend, HidCapabilities, HookBackend, HookBackendEvent, HookBackendSettings,
    HookCapabilities, PlatformError,
};

#[cfg(target_os = "linux")]
use evdev::{
    uinput::VirtualDevice, AttributeSet, BusType as EvdevBusType, Device, EventSummary, InputEvent,
    InputId, KeyCode, RelativeAxisCode,
};
#[cfg(target_os = "linux")]
use hidapi::{BusType as HidBusType, DeviceInfo as HidDeviceInfo, HidApi, HidDevice};
#[cfg(target_os = "linux")]
use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, ConnectionExt as _, Window},
};

const LOGI_VID: u16 = 0x046D;
const FEAT_REPROG_V4: u16 = 0x1B04;
const DEVICE_INDICES: [u8; 3] = [0xFF, 0x00, 0x01];
const DEFAULT_GESTURE_CIDS: [u16; 3] = [0x00C3, 0x00D7, 0x0056];
const GESTURE_DIVERT_FLAGS: u8 = 0x01;
const GESTURE_RAWXY_FLAGS: u8 = 0x05;
const GESTURE_UNDIVERT_FLAGS: u8 = 0x00;
const GESTURE_UNDIVERT_RAWXY_FLAGS: u8 = 0x04;
const BATTERY_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const DPI_VERIFY_DELAY: Duration = Duration::from_secs(1);
const HOOK_RETRY_DELAY: Duration = Duration::from_secs(2);

pub struct LinuxHookBackend {
    #[cfg(target_os = "linux")]
    shared: Arc<LinuxHookShared>,
    #[cfg(target_os = "linux")]
    stop: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    worker: Mutex<Option<JoinHandle<()>>>,
    #[cfg(target_os = "linux")]
    gesture_worker: Mutex<Option<JoinHandle<()>>>,
}

pub struct LinuxHidBackend {
    #[cfg(target_os = "linux")]
    telemetry_cache: Mutex<BTreeMap<String, DeviceTelemetryCacheEntry>>,
}

pub struct LinuxAppFocusBackend;
pub struct LinuxAppDiscoveryBackend;

#[derive(Debug, Clone)]
struct DeviceTelemetryCacheEntry {
    current_dpi: Option<u16>,
    battery_level: Option<u8>,
    last_battery_probe_at: Instant,
    verify_after: Option<Instant>,
    connected: bool,
}

#[derive(Debug, Clone)]
struct DeviceTelemetrySnapshot {
    current_dpi: Option<u16>,
    battery_level: Option<u8>,
}

#[derive(Debug, Clone)]
struct TelemetryProbePlan {
    probe_dpi: bool,
    probe_battery: bool,
    cached: DeviceTelemetrySnapshot,
}

#[derive(Clone, PartialEq, Eq)]
struct LinuxHookConfig {
    enabled: bool,
    invert_horizontal_scroll: bool,
    invert_vertical_scroll: bool,
    debug_mode: bool,
    bindings: HashMap<LogicalControl, String>,
    gesture_threshold: u16,
    gesture_deadzone: u16,
    gesture_timeout_ms: u32,
    gesture_cooldown_ms: u32,
}

impl LinuxHookConfig {
    fn from_runtime(settings: &HookBackendSettings, profile: &Profile, enabled: bool) -> Self {
        Self {
            enabled,
            invert_horizontal_scroll: settings.invert_horizontal_scroll,
            invert_vertical_scroll: settings.invert_vertical_scroll,
            debug_mode: settings.debug_mode,
            bindings: profile
                .bindings
                .iter()
                .map(|binding| (binding.control, binding.action_id.clone()))
                .collect(),
            gesture_threshold: settings.gesture_threshold.max(5),
            gesture_deadzone: settings.gesture_deadzone,
            gesture_timeout_ms: settings.gesture_timeout_ms.max(250),
            gesture_cooldown_ms: settings.gesture_cooldown_ms,
        }
    }

    fn action_for(&self, control: LogicalControl) -> Option<&str> {
        self.bindings.get(&control).map(String::as_str)
    }

    fn handles_control(&self, control: LogicalControl) -> bool {
        self.enabled
            && self
                .action_for(control)
                .is_some_and(|action_id| action_id != "none")
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
        self.enabled
            && [
                LogicalControl::GesturePress,
                LogicalControl::GestureLeft,
                LogicalControl::GestureRight,
                LogicalControl::GestureUp,
                LogicalControl::GestureDown,
            ]
            .into_iter()
            .any(|control| {
                self.action_for(control)
                    .is_some_and(|action_id| action_id != "none")
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GestureInputSource {
    HidRawxy,
    Evdev,
}

impl GestureInputSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::HidRawxy => "hid_rawxy",
            Self::Evdev => "evdev",
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

struct LinuxHookShared {
    config: RwLock<Arc<LinuxHookConfig>>,
    events: Mutex<Vec<HookBackendEvent>>,
    gesture_state: Mutex<GestureTrackingState>,
    gesture_cv: Condvar,
    hook_running: AtomicBool,
    gesture_connected: AtomicBool,
}

impl LinuxHookShared {
    fn new() -> Self {
        let config = mouser_core::default_config();
        let profile = config
            .active_profile()
            .cloned()
            .unwrap_or_else(|| config.profiles[0].clone());
        let hook_settings = HookBackendSettings::from_app_and_device(
            &config.settings,
            &config.device_defaults,
            None,
        );

        Self {
            config: RwLock::new(Arc::new(LinuxHookConfig::from_runtime(
                &hook_settings,
                &profile,
                true,
            ))),
            events: Mutex::new(Vec::new()),
            gesture_state: Mutex::new(GestureTrackingState::default()),
            gesture_cv: Condvar::new(),
            hook_running: AtomicBool::new(false),
            gesture_connected: AtomicBool::new(false),
        }
    }

    fn current_config(&self) -> Arc<LinuxHookConfig> {
        Arc::clone(&self.config.read().unwrap())
    }

    fn reconfigure(&self, settings: &HookBackendSettings, profile: &Profile, enabled: bool) {
        let next = Arc::new(LinuxHookConfig::from_runtime(settings, profile, enabled));
        let changed = {
            let mut config = self.config.write().unwrap();
            if config.as_ref() == next.as_ref() {
                false
            } else {
                *config = Arc::clone(&next);
                true
            }
        };

        if !next.gesture_capture_requested() {
            self.reset_gesture_state();
        }
        self.gesture_cv.notify_all();

        if changed && next.debug_mode {
            self.push_event(
                DebugEventKind::Info,
                format!(
                    "Linux hook reconfigured: enabled={} debug={} gesture_capture={}",
                    next.enabled,
                    next.debug_mode,
                    next.gesture_capture_requested()
                ),
            );
        }
    }

    fn push_event(&self, kind: DebugEventKind, message: impl Into<String>) {
        let mut events = self.events.lock().unwrap();
        push_bounded_hook_event(&mut events, kind, message);
    }

    fn push_debug(&self, message: impl Into<String>) {
        if self.config.read().unwrap().debug_mode {
            self.push_event(DebugEventKind::Info, message);
        }
    }

    fn push_gesture_debug(&self, message: impl Into<String>) {
        if self.config.read().unwrap().debug_mode {
            self.push_event(DebugEventKind::Gesture, message);
        }
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

    fn dispatch_control_action(&self, config: &LinuxHookConfig, control: LogicalControl) {
        let Some(action_id) = config.action_for(control).map(str::to_string) else {
            return;
        };

        self.push_debug(format!("Mapped {} -> {}", control.label(), action_id));
        if let Err(error) = execute_action(&action_id) {
            self.push_event(
                DebugEventKind::Warning,
                format!("Action `{action_id}` failed: {error}"),
            );
        }
    }

    fn hid_gesture_down(&self) {
        let config = self.current_config();
        if !config.gesture_capture_requested() {
            return;
        }

        let mut state = self.gesture_state.lock().unwrap();
        if state.active {
            return;
        }

        state.active = true;
        state.triggered = false;
        if config.gesture_direction_enabled() && !cooldown_active(&state) {
            self.push_gesture_debug("Gesture button down");
            start_gesture_tracking(&mut state);
        } else {
            finish_gesture_tracking(&mut state);
        }

        self.gesture_cv.notify_all();
    }

    fn hid_gesture_up(&self) {
        let config = self.current_config();
        let should_click = {
            let mut state = self.gesture_state.lock().unwrap();
            if !state.active {
                return;
            }

            let should_click =
                !state.triggered && config.handles_control(LogicalControl::GesturePress);
            state.active = false;
            finish_gesture_tracking(&mut state);
            state.triggered = false;
            should_click
        };

        self.gesture_cv.notify_all();

        self.push_gesture_debug(format!(
            "Gesture button up click_candidate={}",
            should_click
        ));

        if should_click {
            self.dispatch_control_action(&config, LogicalControl::GesturePress);
        }
    }

    fn hid_rawxy_move(&self, delta_x: i16, delta_y: i16) {
        let config = self.current_config();
        let mut state = self.gesture_state.lock().unwrap();
        self.accumulate_gesture_delta(
            &config,
            &mut state,
            f64::from(delta_x),
            f64::from(delta_y),
            GestureInputSource::HidRawxy,
        );
        self.gesture_cv.notify_all();
    }

    fn evdev_move(&self, delta_x: i32, delta_y: i32) {
        let config = self.current_config();
        let mut state = self.gesture_state.lock().unwrap();
        self.accumulate_gesture_delta(
            &config,
            &mut state,
            f64::from(delta_x),
            f64::from(delta_y),
            GestureInputSource::Evdev,
        );
        self.gesture_cv.notify_all();
    }

    fn reset_gesture_state(&self) {
        let mut state = self.gesture_state.lock().unwrap();
        *state = GestureTrackingState::default();
        self.gesture_cv.notify_all();
    }

    fn gesture_active_without_hid_rawxy(&self) -> bool {
        let state = self.gesture_state.lock().unwrap();
        state.active && state.input_source != Some(GestureInputSource::HidRawxy)
    }

    fn accumulate_gesture_delta(
        &self,
        config: &LinuxHookConfig,
        state: &mut GestureTrackingState,
        delta_x: f64,
        delta_y: f64,
        source: GestureInputSource,
    ) {
        if !(config.gesture_direction_enabled() && state.active) {
            return;
        }
        if cooldown_active(state) {
            self.push_gesture_debug(format!(
                "Gesture cooldown active source={} dx={} dy={}",
                source.as_str(),
                delta_x,
                delta_y
            ));
            return;
        }
        if !state.tracking {
            self.push_gesture_debug(format!(
                "Gesture tracking started source={}",
                source.as_str()
            ));
            start_gesture_tracking(state);
        }

        let now = Instant::now();
        let idle_ms = state
            .last_move_at
            .map(|last_move_at| now.duration_since(last_move_at).as_millis() as u32)
            .unwrap_or_default();
        if idle_ms > config.gesture_timeout_ms {
            self.push_gesture_debug(format!(
                "Gesture segment reset timeout source={} accum_x={} accum_y={}",
                source.as_str(),
                state.delta_x,
                state.delta_y
            ));
            start_gesture_tracking(state);
        }

        if source == GestureInputSource::HidRawxy && state.input_source == Some(GestureInputSource::Evdev)
        {
            self.push_gesture_debug(format!(
                "Gesture source promoted from evdev to hid_rawxy prev_accum_x={} prev_accum_y={}",
                state.delta_x, state.delta_y
            ));
            start_gesture_tracking(state);
        }

        if state.input_source.is_some_and(|current| current != source) {
            self.push_gesture_debug(format!(
                "Gesture source locked to {}; ignoring {} dx={} dy={}",
                state.input_source.unwrap().as_str(),
                source.as_str(),
                delta_x,
                delta_y
            ));
            return;
        }

        state.input_source = Some(source);
        state.delta_x += delta_x;
        state.delta_y += delta_y;
        state.last_move_at = Some(now);

        self.push_gesture_debug(format!(
            "Gesture segment source={} accum_x={} accum_y={}",
            source.as_str(),
            state.delta_x,
            state.delta_y
        ));

        let Some(control) = gesture::detect_gesture_control(
            state.delta_x,
            state.delta_y,
            f64::from(config.gesture_threshold),
            f64::from(config.gesture_deadzone),
        ) else {
            return;
        };

        state.triggered = true;
        self.push_gesture_debug(format!(
            "Gesture detected {} source={} delta_x={} delta_y={}",
            control.label(),
            source.as_str(),
            state.delta_x,
            state.delta_y
        ));
        self.dispatch_control_action(config, control);
        state.cooldown_until =
            Some(Instant::now() + Duration::from_millis(u64::from(config.gesture_cooldown_ms)));
        finish_gesture_tracking(state);
    }
}

impl LinuxHookBackend {
    pub fn new() -> Self {
        #[cfg(not(target_os = "linux"))]
        {
            Self {}
        }

        #[cfg(target_os = "linux")]
        {
            let shared = Arc::new(LinuxHookShared::new());
            let stop = Arc::new(AtomicBool::new(false));

            let worker_shared = Arc::clone(&shared);
            let worker_stop = Arc::clone(&stop);
            let worker = thread::spawn(move || run_hook_worker(worker_shared, worker_stop));

            let gesture_shared = Arc::clone(&shared);
            let gesture_stop = Arc::clone(&stop);
            let gesture_worker =
                thread::spawn(move || run_gesture_worker(gesture_shared, gesture_stop));

            Self {
                shared,
                stop,
                worker: Mutex::new(Some(worker)),
                gesture_worker: Mutex::new(Some(gesture_worker)),
            }
        }
    }
}

impl Default for LinuxHookBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl LinuxHidBackend {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            telemetry_cache: Mutex::new(BTreeMap::new()),
        }
    }
}

impl Default for LinuxHidBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl HookBackend for LinuxHookBackend {
    fn backend_id(&self) -> &'static str {
        #[cfg(not(target_os = "linux"))]
        {
            "linux-stub"
        }

        #[cfg(target_os = "linux")]
        {
            let hook_running = self.shared.hook_running.load(Ordering::SeqCst);
            let gesture_connected = self.shared.gesture_connected.load(Ordering::SeqCst);
            match (hook_running, gesture_connected) {
                (true, true) => "linux-evdev+hidapi-gesture",
                (true, false) => "linux-evdev",
                (false, true) => "linux-hidapi-gesture",
                (false, false) => "linux-evdev-unavailable",
            }
        }
    }

    fn capabilities(&self) -> HookCapabilities {
        #[cfg(not(target_os = "linux"))]
        {
            HookCapabilities {
                can_intercept_buttons: false,
                can_intercept_scroll: false,
                supports_gesture_diversion: false,
            }
        }

        #[cfg(target_os = "linux")]
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
        profile: &Profile,
        enabled: bool,
    ) -> Result<(), PlatformError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (settings, profile, enabled);
            Ok(())
        }

        #[cfg(target_os = "linux")]
        {
            self.shared.reconfigure(settings, profile, enabled);
            Ok(())
        }
    }

    fn drain_events(&self) -> Vec<HookBackendEvent> {
        #[cfg(not(target_os = "linux"))]
        {
            Vec::new()
        }

        #[cfg(target_os = "linux")]
        {
            self.shared.drain_events()
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for LinuxHookBackend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.shared.gesture_cv.notify_all();

        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
        if let Some(gesture_worker) = self.gesture_worker.lock().unwrap().take() {
            let _ = gesture_worker.join();
        }
    }
}

impl HidBackend for LinuxHidBackend {
    fn backend_id(&self) -> &'static str {
        #[cfg(target_os = "linux")]
        {
            "linux-hidapi"
        }

        #[cfg(not(target_os = "linux"))]
        {
            "linux-stub"
        }
    }

    fn capabilities(&self) -> HidCapabilities {
        #[cfg(target_os = "linux")]
        {
            HidCapabilities {
                can_enumerate_devices: true,
                can_read_battery: true,
                can_read_dpi: true,
                can_write_dpi: true,
            }
        }

        #[cfg(not(target_os = "linux"))]
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
        #[cfg(not(target_os = "linux"))]
        {
            Err(PlatformError::Unsupported(
                "live Linux HID integration is only available on Linux",
            ))
        }

        #[cfg(target_os = "linux")]
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
                        telemetry.battery_level,
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
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (device_key, dpi);
            Err(PlatformError::Unsupported(
                "live Linux HID integration is only available on Linux",
            ))
        }

        #[cfg(target_os = "linux")]
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

impl AppFocusBackend for LinuxAppFocusBackend {
    fn backend_id(&self) -> &'static str {
        #[cfg(not(target_os = "linux"))]
        {
            "linux-stub"
        }

        #[cfg(target_os = "linux")]
        {
            if std::env::var_os("DISPLAY").is_some() {
                "linux-x11"
            } else {
                "linux-x11-unavailable"
            }
        }
    }

    fn current_frontmost_app(&self) -> Result<Option<AppIdentity>, PlatformError> {
        #[cfg(not(target_os = "linux"))]
        {
            Err(PlatformError::Unsupported(
                "live Linux frontmost app detection is only available on Linux",
            ))
        }

        #[cfg(target_os = "linux")]
        {
            if std::env::var_os("DISPLAY").is_none() {
                return Ok(None);
            }
            current_frontmost_app_identity().or(Ok(None))
        }
    }
}

impl AppDiscoveryBackend for LinuxAppDiscoveryBackend {
    fn backend_id(&self) -> &'static str {
        #[cfg(target_os = "linux")]
        {
            "linux-desktop"
        }

        #[cfg(not(target_os = "linux"))]
        {
            "linux-stub"
        }
    }

    fn discover_apps(&self) -> Result<Vec<InstalledApp>, PlatformError> {
        #[cfg(not(target_os = "linux"))]
        {
            Err(PlatformError::Unsupported(
                "Linux app discovery is only available on Linux",
            ))
        }

        #[cfg(target_os = "linux")]
        {
            let mut apps = Vec::new();
            let mut resolver = DesktopEntryResolver::new();
            for root in linux_desktop_entry_roots() {
                collect_desktop_entry_apps(&root, &mut apps, &mut resolver)?;
            }
            collect_running_process_apps(&mut apps)?;
            Ok(dedupe_installed_apps(apps))
        }
    }
}

#[cfg(target_os = "linux")]
impl LinuxHidBackend {
    fn telemetry_plan(&self, cache_key: &str, now: Instant) -> TelemetryProbePlan {
        let cache = self.telemetry_cache.lock().unwrap();
        let entry = cache.get(cache_key);
        TelemetryProbePlan {
            probe_dpi: should_probe_dpi(entry, now),
            probe_battery: should_probe_battery(entry, now),
            cached: DeviceTelemetrySnapshot {
                current_dpi: entry.and_then(|entry| entry.current_dpi),
                battery_level: entry.and_then(|entry| entry.battery_level),
            },
        }
    }

    fn remember_device_telemetry(
        &self,
        cache_key: String,
        telemetry: DeviceTelemetrySnapshot,
        plan: &TelemetryProbePlan,
        now: Instant,
    ) {
        let mut cache = self.telemetry_cache.lock().unwrap();
        let entry = cache.entry(cache_key).or_insert(DeviceTelemetryCacheEntry {
            current_dpi: None,
            battery_level: None,
            last_battery_probe_at: now,
            verify_after: None,
            connected: true,
        });
        entry.current_dpi = telemetry.current_dpi.or(plan.cached.current_dpi);
        entry.battery_level = telemetry.battery_level.or(plan.cached.battery_level);
        if plan.probe_battery {
            entry.last_battery_probe_at = now;
        }
        entry.verify_after = None;
        entry.connected = true;
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
            battery_level: None,
            last_battery_probe_at: now,
            verify_after: None,
            connected: true,
        });
        entry.current_dpi = Some(dpi);
        entry.verify_after = Some(now + DPI_VERIFY_DELAY);
        entry.connected = true;
    }
}

#[cfg(target_os = "linux")]
fn run_hook_worker(shared: Arc<LinuxHookShared>, stop: Arc<AtomicBool>) {
    let mut warned_setup_failure = false;

    while !stop.load(Ordering::SeqCst) {
        match MouseHookSession::open() {
            Ok(mut session) => {
                warned_setup_failure = false;
                shared.mark_hook_running(
                    true,
                    Some(format!(
                        "Grabbed Linux mouse `{}` at {}",
                        session.device_name,
                        session.device_path.display()
                    )),
                );

                match session.run(&shared, &stop) {
                    Ok(()) => {
                        if !stop.load(Ordering::SeqCst) {
                            shared.mark_hook_running(
                                false,
                                Some("Linux mouse hook stopped".to_string()),
                            );
                        }
                    }
                    Err(error) => {
                        shared.push_event(
                            DebugEventKind::Warning,
                            format!("Linux mouse hook error: {error}"),
                        );
                        shared.mark_hook_running(
                            false,
                            Some("Linux mouse hook disconnected".to_string()),
                        );
                    }
                }
            }
            Err(error) => {
                if !warned_setup_failure || shared.current_config().debug_mode {
                    shared.push_event(
                        DebugEventKind::Warning,
                        format!("Linux mouse hook unavailable: {error}"),
                    );
                    warned_setup_failure = true;
                }
                shared.mark_hook_running(false, None);
                thread::sleep(HOOK_RETRY_DELAY);
            }
        }
    }

    shared.mark_hook_running(false, None);
}

#[cfg(target_os = "linux")]
struct MouseHookSession {
    device_path: PathBuf,
    device_name: String,
    device: Device,
    mirror: VirtualDevice,
}

#[cfg(target_os = "linux")]
impl MouseHookSession {
    fn open() -> Result<Self, PlatformError> {
        let (device_path, mut device) = find_mouse_device().ok_or_else(|| {
            PlatformError::Message(
                "could not find a relative Linux mouse device with primary buttons".to_string(),
            )
        })?;
        let device_name = device.name().unwrap_or("Mouse").to_string();
        let mirror = create_virtual_mouse(&device)?;

        device
            .set_nonblocking(true)
            .map_err(|error| io_error("set Linux mouse device to non-blocking", error))?;
        device
            .grab()
            .map_err(|error| io_error("grab Linux mouse device", error))?;

        Ok(Self {
            device_path,
            device_name,
            device,
            mirror,
        })
    }

    fn run(
        &mut self,
        shared: &LinuxHookShared,
        stop: &AtomicBool,
    ) -> Result<(), PlatformError> {
        while !stop.load(Ordering::SeqCst) {
            match self.device.fetch_events() {
                Ok(events) => {
                    let batch = events.collect::<Vec<_>>();
                    if batch.is_empty() {
                        thread::sleep(Duration::from_millis(12));
                        continue;
                    }
                    self.process_batch(shared, &batch)?;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(12));
                }
                Err(error) => return Err(io_error("read Linux mouse device", error)),
            }
        }

        self.device
            .ungrab()
            .map_err(|error| io_error("release Linux mouse grab", error))?;
        Ok(())
    }

    fn process_batch(
        &mut self,
        shared: &LinuxHookShared,
        events: &[InputEvent],
    ) -> Result<(), PlatformError> {
        let config = shared.current_config();
        let mut forwarded = Vec::new();

        for event in events {
            match event.destructure() {
                EventSummary::Key(_, code, value) => {
                    self.handle_button(shared, &config, code, value, &mut forwarded, *event);
                }
                EventSummary::RelativeAxis(_, code, value) => {
                    self.handle_relative(shared, &config, code, value, &mut forwarded);
                }
                EventSummary::Synchronization(_, _, _) => {}
                _ => forwarded.push(*event),
            }
        }

        if !forwarded.is_empty() {
            self.mirror
                .emit(&forwarded)
                .map_err(|error| io_error("emit Linux mouse events", error))?;
        }

        Ok(())
    }

    fn handle_button(
        &mut self,
        shared: &LinuxHookShared,
        config: &LinuxHookConfig,
        code: KeyCode,
        value: i32,
        forwarded: &mut Vec<InputEvent>,
        original: InputEvent,
    ) {
        let control = match code {
            KeyCode::BTN_SIDE => Some(LogicalControl::Back),
            KeyCode::BTN_EXTRA => Some(LogicalControl::Forward),
            KeyCode::BTN_MIDDLE => Some(LogicalControl::Middle),
            _ => None,
        };

        let Some(control) = control else {
            forwarded.push(original);
            return;
        };

        if config.handles_control(control) {
            if value == 1 {
                shared.dispatch_control_action(config, control);
            }
            return;
        }

        forwarded.push(original);
    }

    fn handle_relative(
        &mut self,
        shared: &LinuxHookShared,
        config: &LinuxHookConfig,
        code: RelativeAxisCode,
        value: i32,
        forwarded: &mut Vec<InputEvent>,
    ) {
        match code {
            RelativeAxisCode::REL_X => {
                if shared.gesture_active_without_hid_rawxy() {
                    shared.evdev_move(value, 0);
                    return;
                }
                forwarded.push(InputEvent::new(2, code.0, value));
            }
            RelativeAxisCode::REL_Y => {
                if shared.gesture_active_without_hid_rawxy() {
                    shared.evdev_move(0, value);
                    return;
                }
                forwarded.push(InputEvent::new(2, code.0, value));
            }
            RelativeAxisCode::REL_WHEEL => {
                forwarded.push(InputEvent::new(
                    2,
                    code.0,
                    if config.invert_vertical_scroll {
                        -value
                    } else {
                        value
                    },
                ));
            }
            RelativeAxisCode::REL_HWHEEL => {
                let should_block = horizontal_scroll_control(value)
                    .is_some_and(|control| config.handles_control(control));
                if should_block {
                    if let Some(control) = horizontal_scroll_control(value) {
                        shared.dispatch_control_action(config, control);
                    }
                    return;
                }
                forwarded.push(InputEvent::new(
                    2,
                    code.0,
                    if config.invert_horizontal_scroll {
                        -value
                    } else {
                        value
                    },
                ));
            }
            _ => {
                let is_hi_res_hwheel = code.0 == 0x0C;
                let is_hi_res_vwheel = code.0 == 0x0B;

                if is_hi_res_hwheel {
                    let should_block = horizontal_scroll_control(value)
                        .is_some_and(|control| config.handles_control(control));
                    if should_block {
                        return;
                    }
                    forwarded.push(InputEvent::new(
                        2,
                        code.0,
                        if config.invert_horizontal_scroll {
                            -value
                        } else {
                            value
                        },
                    ));
                } else if is_hi_res_vwheel {
                    forwarded.push(InputEvent::new(
                        2,
                        code.0,
                        if config.invert_vertical_scroll {
                            -value
                        } else {
                            value
                        },
                    ));
                } else {
                    forwarded.push(InputEvent::new(2, code.0, value));
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for MouseHookSession {
    fn drop(&mut self) {
        let _ = self.device.ungrab();
    }
}

#[cfg(target_os = "linux")]
fn find_mouse_device() -> Option<(PathBuf, Device)> {
    let mut preferred = Vec::new();
    let mut fallback = Vec::new();

    for (path, device) in evdev::enumerate() {
        let Some(relative_axes) = device.supported_relative_axes() else {
            continue;
        };
        let Some(keys) = device.supported_keys() else {
            continue;
        };
        if !(relative_axes.contains(RelativeAxisCode::REL_X)
            && relative_axes.contains(RelativeAxisCode::REL_Y))
        {
            continue;
        }
        if !(keys.contains(KeyCode::BTN_LEFT)
            && keys.contains(KeyCode::BTN_RIGHT)
            && keys.contains(KeyCode::BTN_MIDDLE))
        {
            continue;
        }

        let has_side_buttons = keys.contains(KeyCode::BTN_SIDE) || keys.contains(KeyCode::BTN_EXTRA);
        let target = if device.input_id().vendor() == LOGI_VID {
            &mut preferred
        } else {
            &mut fallback
        };
        target.push((!has_side_buttons, path, device));
    }

    preferred
        .into_iter()
        .chain(fallback)
        .min_by_key(|entry| entry.0)
        .map(|(_, path, device)| (path, device))
}

#[cfg(target_os = "linux")]
fn create_virtual_mouse(device: &Device) -> Result<VirtualDevice, PlatformError> {
    let mut builder = VirtualDevice::builder()
        .map_err(|error| io_error("create Linux virtual mouse builder", error))?
        .name(b"Mouser Virtual Mouse")
        .input_id(device.input_id());

    if let Some(keys) = device.supported_keys() {
        builder = builder
            .with_keys(keys)
            .map_err(|error| io_error("clone Linux mouse key capabilities", error))?;
    }
    if let Some(relative_axes) = device.supported_relative_axes() {
        builder = builder
            .with_relative_axes(relative_axes)
            .map_err(|error| io_error("clone Linux mouse relative capabilities", error))?;
    }

    builder
        .build()
        .map_err(|error| io_error("create Linux virtual mouse", error))
}

#[cfg(target_os = "linux")]
fn run_gesture_worker(shared: Arc<LinuxHookShared>, stop: Arc<AtomicBool>) {
    let mut session: Option<GestureSession> = None;

    while !stop.load(Ordering::SeqCst) {
        if !shared.gesture_capture_requested() {
            if let Some(mut active_session) = session.take() {
                active_session.shutdown();
                shared.mark_gesture_connected(false, Some("Gesture listener parked".to_string()));
            }
            let mut guard = shared.gesture_state.lock().unwrap();
            while !stop.load(Ordering::SeqCst) && !shared.gesture_capture_requested() {
                guard = shared.gesture_cv.wait(guard).unwrap();
            }
            continue;
        }

        if session.is_none() {
            match connect_gesture_session(&shared) {
                Ok(active_session) => {
                    let source = active_session
                        .product_name
                        .clone()
                        .unwrap_or_else(|| format!("PID 0x{:04X}", active_session.product_id));
                    shared.mark_gesture_connected(
                        true,
                        Some(format!("Gesture listener attached to {source}")),
                    );
                    session = Some(active_session);
                }
                Err(error) => {
                    shared.mark_gesture_connected(
                        false,
                        Some(format!("Gesture listener unavailable: {error}")),
                    );
                    let guard = shared.gesture_state.lock().unwrap();
                    let _ = shared
                        .gesture_cv
                        .wait_timeout(guard, Duration::from_millis(900))
                        .unwrap();
                    continue;
                }
            }
        }

        let Some(active_session) = session.as_mut() else {
            continue;
        };

        match active_session.device.read_packet(120) {
            Ok(packet) => {
                if !packet.is_empty() {
                    active_session.handle_report(&shared, &packet);
                }
            }
            Err(error) => {
                shared.push_event(
                    DebugEventKind::Warning,
                    format!("Gesture listener lost HID stream: {error}"),
                );
                if let Some(mut failed_session) = session.take() {
                    failed_session.shutdown();
                }
                shared.mark_gesture_connected(
                    false,
                    Some("Gesture listener disconnected".to_string()),
                );
                let guard = shared.gesture_state.lock().unwrap();
                let _ = shared
                    .gesture_cv
                    .wait_timeout(guard, Duration::from_millis(500))
                    .unwrap();
            }
        }
    }

    if let Some(mut active_session) = session.take() {
        active_session.shutdown();
    }
    shared.mark_gesture_connected(false, None);
}

#[cfg(target_os = "linux")]
struct GestureSession {
    product_id: u16,
    product_name: Option<String>,
    device: HidDevice,
    dev_idx: u8,
    feature_idx: u8,
    gesture_cid: u16,
    rawxy_enabled: bool,
    held: bool,
}

#[cfg(target_os = "linux")]
impl GestureSession {
    fn handle_report(&mut self, shared: &LinuxHookShared, raw: &[u8]) {
        let Some((dev_idx, feature_idx, function, _sw, params)) = hidpp::parse_message(raw) else {
            return;
        };
        if dev_idx != self.dev_idx || feature_idx != self.feature_idx {
            return;
        }

        if function == 1 {
            if !self.rawxy_enabled || !self.held || params.len() < 4 {
                return;
            }

            let delta_x = decode_s16(params[0], params[1]);
            let delta_y = decode_s16(params[2], params[3]);
            if delta_x != 0 || delta_y != 0 {
                shared.hid_rawxy_move(delta_x, delta_y);
            }
            return;
        }

        if function != 0 {
            return;
        }

        let gesture_now = params
            .chunks_exact(2)
            .take_while(|pair| pair[0] != 0 || pair[1] != 0)
            .any(|pair| u16::from(pair[0]) << 8 | u16::from(pair[1]) == self.gesture_cid);

        if gesture_now && !self.held {
            self.held = true;
            shared.hid_gesture_down();
        } else if !gesture_now && self.held {
            self.held = false;
            shared.hid_gesture_up();
        }
    }

    fn shutdown(&mut self) {
        let flags = if self.rawxy_enabled {
            GESTURE_UNDIVERT_RAWXY_FLAGS
        } else {
            GESTURE_UNDIVERT_FLAGS
        };
        let _ = hidpp::write_request(
            &self.device,
            self.dev_idx,
            self.feature_idx,
            3,
            &[
                ((self.gesture_cid >> 8) & 0xFF) as u8,
                (self.gesture_cid & 0xFF) as u8,
                flags,
                0x00,
                0x00,
            ],
        );
        self.held = false;
    }
}

#[cfg(target_os = "linux")]
fn connect_gesture_session(shared: &LinuxHookShared) -> Result<GestureSession, PlatformError> {
    let api = HidApi::new().map_err(map_hid_error)?;
    let mut last_error = None;

    for info in vendor_hid_infos(&api) {
        let Ok(device) = info.open_device(&api) else {
            continue;
        };

        match initialize_gesture_session(shared, info, device) {
            Ok(session) => return Ok(session),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        PlatformError::Message("no Logitech gesture-capable HID interface found".to_string())
    }))
}

#[cfg(target_os = "linux")]
fn initialize_gesture_session(
    shared: &LinuxHookShared,
    info: &HidDeviceInfo,
    device: HidDevice,
) -> Result<GestureSession, PlatformError> {
    let fingerprint = fingerprint_from_hid_info(info);
    let device_info = build_connected_device_info(
        Some(info.product_id()),
        info.product_string(),
        Some(transport_label(info.bus_type())),
        Some("hidapi"),
        None,
        1000,
        fingerprint,
    );
    let gesture_candidates = gesture_candidates_for(&device_info.gesture_cids);

    for dev_idx in DEVICE_INDICES {
        let Some(feature_idx) = find_hidpp_feature(&device, dev_idx, FEAT_REPROG_V4, 250)? else {
            continue;
        };

        for gesture_cid in &gesture_candidates {
            if set_gesture_reporting(
                &device,
                dev_idx,
                feature_idx,
                *gesture_cid,
                GESTURE_RAWXY_FLAGS,
                250,
            )?
            .is_some()
            {
                shared.push_gesture_debug(format!(
                    "Diverted gesture cid 0x{:04X} with RawXY via devIdx=0x{:02X}",
                    gesture_cid, dev_idx
                ));
                return Ok(GestureSession {
                    product_id: info.product_id(),
                    product_name: info.product_string().map(str::to_string),
                    device,
                    dev_idx,
                    feature_idx,
                    gesture_cid: *gesture_cid,
                    rawxy_enabled: true,
                    held: false,
                });
            }

            if set_gesture_reporting(
                &device,
                dev_idx,
                feature_idx,
                *gesture_cid,
                GESTURE_DIVERT_FLAGS,
                250,
            )?
            .is_some()
            {
                shared.push_gesture_debug(format!(
                    "Diverted gesture cid 0x{:04X} via devIdx=0x{:02X}",
                    gesture_cid, dev_idx
                ));
                return Ok(GestureSession {
                    product_id: info.product_id(),
                    product_name: info.product_string().map(str::to_string),
                    device,
                    dev_idx,
                    feature_idx,
                    gesture_cid: *gesture_cid,
                    rawxy_enabled: false,
                    held: false,
                });
            }
        }
    }

    Err(PlatformError::Message(format!(
        "gesture diversion failed for pid 0x{:04X}",
        info.product_id()
    )))
}

#[cfg(target_os = "linux")]
fn vendor_hid_infos(api: &HidApi) -> Vec<&HidDeviceInfo> {
    api.device_list()
        .filter(|info| info.vendor_id() == LOGI_VID && info.usage_page() >= 0xFF00)
        .collect()
}

#[cfg(target_os = "linux")]
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
        battery_level: if plan.probe_battery {
            read_hidpp_battery(device)
                .ok()
                .flatten()
                .or(plan.cached.battery_level)
        } else {
            plan.cached.battery_level
        },
    }
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
fn push_unique_device(devices: &mut Vec<DeviceInfo>, device: DeviceInfo) {
    if devices.iter().all(|existing| existing.key != device.key) {
        devices.push(device);
    }
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
fn transport_label(bus_type: HidBusType) -> &'static str {
    match bus_type {
        HidBusType::Bluetooth => "Bluetooth Low Energy",
        HidBusType::Usb => "USB",
        HidBusType::I2c => "I2C",
        HidBusType::Spi => "SPI",
        HidBusType::Unknown => "Unknown transport",
    }
}

#[cfg(target_os = "linux")]
fn set_hidpp_dpi(device: &HidDevice, dpi: u16) -> Result<bool, PlatformError> {
    hidpp::set_sensor_dpi(device, BT_DEV_IDX, dpi, 1_500)
}

#[cfg(target_os = "linux")]
fn read_hidpp_current_dpi(device: &HidDevice) -> Result<Option<u16>, PlatformError> {
    hidpp::read_sensor_dpi(device, BT_DEV_IDX, 1_500)
}

#[cfg(target_os = "linux")]
fn read_hidpp_battery(device: &HidDevice) -> Result<Option<u8>, PlatformError> {
    hidpp::read_battery_level(device, BT_DEV_IDX, 1_500)
}

#[cfg(target_os = "linux")]
fn find_hidpp_feature(
    device: &HidDevice,
    dev_idx: u8,
    feature_id: u16,
    timeout_ms: i32,
) -> Result<Option<u8>, PlatformError> {
    hidpp::find_feature(device, dev_idx, feature_id, timeout_ms)
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
fn decode_s16(hi: u8, lo: u8) -> i16 {
    let value = u16::from(hi) << 8 | u16::from(lo);
    value as i16
}

#[cfg(target_os = "linux")]
fn gesture_candidates_for(gesture_cids: &[u16]) -> Vec<u16> {
    gesture::ordered_gesture_candidates(gesture_cids, &DEFAULT_GESTURE_CIDS)
}

#[cfg(target_os = "linux")]
struct X11Atoms {
    active_window: u32,
    net_wm_pid: u32,
    net_wm_name: u32,
    utf8_string: u32,
}

#[cfg(target_os = "linux")]
fn current_frontmost_app_identity() -> Result<Option<AppIdentity>, PlatformError> {
    let (connection, screen_index) = x11rb::connect(None)
        .map_err(|error| PlatformError::Message(format!("could not connect to X11: {error}")))?;
    let root = connection.setup().roots[screen_index].root;
    let atoms = intern_x11_atoms(&connection)?;

    let active_window = connection
        .get_property(false, root, atoms.active_window, AtomEnum::WINDOW, 0, 1)
        .map_err(|error| PlatformError::Message(format!("could not query X11 active window: {error}")))?
        .reply()
        .map_err(|error| PlatformError::Message(format!("could not read X11 active window: {error}")))?
        .value32()
        .and_then(|mut values| values.next());

    let Some(window) = active_window else {
        return Ok(None);
    };

    let pid = connection
        .get_property(false, window, atoms.net_wm_pid, AtomEnum::CARDINAL, 0, 1)
        .map_err(|error| PlatformError::Message(format!("could not query X11 window PID: {error}")))?
        .reply()
        .map_err(|error| PlatformError::Message(format!("could not read X11 window PID: {error}")))?
        .value32()
        .and_then(|mut values| values.next());

    let executable_path = pid.and_then(read_executable_path_for_pid);
    let executable = executable_path.as_deref().and_then(path_file_name);
    let label =
        window_label(&connection, window, &atoms).or_else(|| window_class(&connection, window));

    Ok(Some(AppIdentity {
        label,
        executable,
        executable_path,
        bundle_id: None,
        package_family_name: None,
    }))
}

#[cfg(target_os = "linux")]
fn intern_x11_atoms<C: Connection>(connection: &C) -> Result<X11Atoms, PlatformError> {
    Ok(X11Atoms {
        active_window: intern_atom(connection, b"_NET_ACTIVE_WINDOW")?,
        net_wm_pid: intern_atom(connection, b"_NET_WM_PID")?,
        net_wm_name: intern_atom(connection, b"_NET_WM_NAME")?,
        utf8_string: intern_atom(connection, b"UTF8_STRING")?,
    })
}

#[cfg(target_os = "linux")]
fn intern_atom<C: Connection>(connection: &C, name: &[u8]) -> Result<u32, PlatformError> {
    connection
        .intern_atom(false, name)
        .map_err(|error| PlatformError::Message(format!("could not intern X11 atom: {error}")))?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|error| PlatformError::Message(format!("could not resolve X11 atom: {error}")))
}

#[cfg(target_os = "linux")]
fn window_label<C: Connection>(connection: &C, window: Window, atoms: &X11Atoms) -> Option<String> {
    connection
        .get_property(
            false,
            window,
            atoms.net_wm_name,
            atoms.utf8_string,
            0,
            256,
        )
        .ok()?
        .reply()
        .ok()?
        .value8()
        .map(|bytes| bytes.collect::<Vec<_>>())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "linux")]
fn window_class<C: Connection>(connection: &C, window: Window) -> Option<String> {
    let reply = connection
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
        .ok()?
        .reply()
        .ok()?;
    let bytes = reply.value8()?.collect::<Vec<_>>();
    let parts = bytes
        .split(|byte| *byte == 0)
        .filter_map(|part| {
            let value = String::from_utf8(part.to_vec()).ok()?;
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .collect::<Vec<_>>();
    parts.into_iter().last()
}

#[cfg(target_os = "linux")]
fn read_executable_path_for_pid(pid: u32) -> Option<String> {
    fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|path| path.to_string_lossy().to_string())
}

#[cfg(target_os = "linux")]
fn linux_desktop_entry_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ];

    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(data_home).join("applications"));
    } else if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/applications"));
    }

    roots
}

#[cfg(target_os = "linux")]
fn collect_desktop_entry_apps(
    root: &Path,
    apps: &mut Vec<InstalledApp>,
    resolver: &mut DesktopEntryResolver,
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
        if path.is_dir() {
            collect_desktop_entry_apps(&path, apps, resolver)?;
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
            continue;
        }
        if let Some(app) = read_desktop_entry(&path, resolver) {
            apps.push(app);
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn read_desktop_entry(path: &Path, resolver: &mut DesktopEntryResolver) -> Option<InstalledApp> {
    let entry = parse_desktop_entry(&fs::read_to_string(path).ok()?)?;
    if !entry.is_application {
        return None;
    }
    if entry.hidden || entry.no_display {
        return None;
    }
    if !is_user_facing_app_label(&entry.name) {
        return None;
    }

    let exec = entry.exec.as_deref().or(entry.try_exec.as_deref())?;
    let (executable, executable_path) = resolver.parse_exec_command(exec);
    let source_path = entry
        .icon
        .as_deref()
        .and_then(|icon| resolver.resolve_icon_path(icon))
        .or_else(|| Some(path.to_string_lossy().to_string()));

    Some(InstalledApp {
        identity: AppIdentity {
            label: Some(entry.name),
            executable,
            executable_path,
            bundle_id: None,
            package_family_name: None,
        },
        source_kinds: vec![AppDiscoverySource::DesktopEntry],
        source_path,
    })
}

#[cfg(target_os = "linux")]
fn collect_running_process_apps(apps: &mut Vec<InstalledApp>) -> Result<(), PlatformError> {
    let proc_dir = Path::new("/proc");
    if !proc_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(proc_dir).map_err(|error| PlatformError::Io {
        path: proc_dir.display().to_string(),
        message: error.to_string(),
    })? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_string_lossy().parse::<u32>().ok() else {
            continue;
        };

        let Some(executable_path) = read_executable_path_for_pid(pid) else {
            continue;
        };
        if !is_user_facing_process(&executable_path) {
            continue;
        }

        let executable = path_file_name(&executable_path);
        let label = read_process_label(pid).or_else(|| executable.clone());
        let Some(label) = label.filter(|label| is_user_facing_app_label(label)) else {
            continue;
        };

        apps.push(InstalledApp {
            identity: AppIdentity {
                label: Some(label),
                executable,
                executable_path: Some(executable_path.clone()),
                bundle_id: None,
                package_family_name: None,
            },
            source_kinds: vec![AppDiscoverySource::RunningProcess],
            source_path: Some(executable_path),
        });
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn read_process_label(pid: u32) -> Option<String> {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn is_user_facing_app_label(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    !lower.is_empty()
        && !lower.contains("helper")
        && !lower.contains("daemon")
        && !lower.contains("service")
        && !lower.contains("setup")
        && !lower.contains("uninstall")
}

fn is_user_facing_process(path: &str) -> bool {
    let lower = path.trim().to_ascii_lowercase();
    !lower.is_empty()
        && !lower.starts_with("/usr/lib/systemd")
        && !lower.contains("/daemon")
        && !lower.contains("/dbus-")
        && !lower.contains("/pipewire")
        && !lower.contains("/wireplumber")
        && !lower.contains("/ssh-agent")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DesktopEntryRecord {
    name: String,
    exec: Option<String>,
    try_exec: Option<String>,
    icon: Option<String>,
    no_display: bool,
    hidden: bool,
    is_application: bool,
}

struct DesktopEntryResolver {
    path_dirs: Vec<PathBuf>,
    icon_search_roots: Vec<PathBuf>,
    themed_icon_bases: Vec<PathBuf>,
    executable_cache: HashMap<String, Option<String>>,
    icon_cache: HashMap<String, Option<String>>,
    themed_relative_cache: HashMap<String, Vec<PathBuf>>,
}

impl DesktopEntryResolver {
    fn new() -> Self {
        Self {
            path_dirs: executable_search_roots(),
            icon_search_roots: icon_search_roots(),
            themed_icon_bases: themed_icon_bases(),
            executable_cache: HashMap::new(),
            icon_cache: HashMap::new(),
            themed_relative_cache: HashMap::new(),
        }
    }

    fn parse_exec_command(&mut self, raw: &str) -> (Option<String>, Option<String>) {
        let tokens = split_command_line(raw)
            .into_iter()
            .map(|token| strip_desktop_field_codes(&token))
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();
        if tokens.is_empty() {
            return (None, None);
        }

        let mut index = 0usize;
        if tokens.first().is_some_and(|token| token == "env") {
            index = 1;
            while index < tokens.len()
                && (tokens[index].contains('=') || tokens[index].starts_with('-'))
            {
                index += 1;
            }
        } else {
            while index < tokens.len() && tokens[index].contains('=') {
                index += 1;
            }
        }

        let Some(command) = tokens.get(index).cloned() else {
            return (None, None);
        };

        let executable_path = self.resolve_executable_path(&command);
        let executable = executable_path
            .as_deref()
            .and_then(path_file_name)
            .or_else(|| path_file_name(&command));

        (executable, executable_path)
    }

    fn resolve_executable_path(&mut self, command: &str) -> Option<String> {
        let path = Path::new(command);
        if path.is_absolute() {
            return path.exists().then(|| path.to_string_lossy().to_string());
        }

        if let Some(resolved) = self.executable_cache.get(command) {
            return resolved.clone();
        }

        let resolved = self
            .path_dirs
            .iter()
            .map(|dir| dir.join(command))
            .find(|candidate| candidate.exists())
            .map(|candidate| candidate.to_string_lossy().to_string());
        self.executable_cache
            .insert(command.to_string(), resolved.clone());
        resolved
    }

    fn resolve_icon_path(&mut self, icon: &str) -> Option<String> {
        let icon = icon.trim();
        if icon.is_empty() {
            return None;
        }

        let path = Path::new(icon);
        if path.is_absolute() {
            return path.exists().then(|| path.to_string_lossy().to_string());
        }

        if let Some(resolved) = self.icon_cache.get(icon) {
            return resolved.clone();
        }

        let resolved = self.resolve_relative_icon_path(icon);
        self.icon_cache.insert(icon.to_string(), resolved.clone());
        resolved
    }

    fn resolve_relative_icon_path(&mut self, icon: &str) -> Option<String> {
        let file_names = icon_file_names(icon);

        for root in &self.icon_search_roots {
            for file_name in &file_names {
                let direct = root.join(file_name);
                if direct.exists() {
                    return Some(direct.to_string_lossy().to_string());
                }
            }
        }

        if !self.themed_relative_cache.contains_key(icon) {
            self.themed_relative_cache
                .insert(icon.to_string(), themed_icon_relatives(icon));
        }
        let relatives = self.themed_relative_cache.get(icon)?;

        for base in &self.themed_icon_bases {
            for relative in relatives {
                let candidate = base.join(relative);
                if candidate.exists() {
                    return Some(candidate.to_string_lossy().to_string());
                }
            }
        }

        None
    }
}

fn parse_desktop_entry(raw: &str) -> Option<DesktopEntryRecord> {
    let mut in_desktop_entry = false;
    let mut record = DesktopEntryRecord::default();

    for raw_line in raw.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }

        let (key, value) = line.split_once('=')?;
        let key = key.trim();
        let value = value.trim();

        match key {
            "Name" => record.name = value.to_string(),
            "Exec" => record.exec = (!value.is_empty()).then(|| value.to_string()),
            "TryExec" => record.try_exec = (!value.is_empty()).then(|| value.to_string()),
            "Icon" => record.icon = (!value.is_empty()).then(|| value.to_string()),
            "NoDisplay" => record.no_display = parse_desktop_bool(value),
            "Hidden" => record.hidden = parse_desktop_bool(value),
            "Type" => record.is_application = value.eq_ignore_ascii_case("application"),
            _ => {}
        }
    }

    (!record.name.is_empty()).then_some(record)
}

fn parse_desktop_bool(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes")
}

fn parse_exec_command(raw: &str) -> (Option<String>, Option<String>) {
    DesktopEntryResolver::new().parse_exec_command(raw)
}

fn split_command_line(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;

    for ch in raw.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            ch if ch.is_whitespace() && quote.is_none() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn strip_desktop_field_codes(value: &str) -> String {
    let mut result = String::new();
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            if let Some('%') = chars.peek() {
                result.push('%');
                chars.next();
            } else {
                chars.next();
            }
        } else {
            result.push(ch);
        }
    }

    result.trim().to_string()
}

fn executable_search_roots() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default()
}

fn icon_file_names(icon: &str) -> Vec<String> {
    if Path::new(icon).extension().is_some() {
        vec![icon.to_string()]
    } else {
        vec![
            format!("{icon}.png"),
            format!("{icon}.svg"),
            format!("{icon}.xpm"),
        ]
    }
}

fn icon_search_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/usr/share/pixmaps")];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(&home).join(".local/share/icons"));
        roots.push(PathBuf::from(home).join(".icons"));
    }
    roots.push(PathBuf::from("/usr/local/share/icons"));
    roots.push(PathBuf::from("/usr/share/icons"));
    roots
}

fn themed_icon_bases() -> Vec<PathBuf> {
    let mut bases = vec![
        PathBuf::from("/usr/local/share/icons/hicolor"),
        PathBuf::from("/usr/share/icons/hicolor"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        bases.push(PathBuf::from(&home).join(".local/share/icons/hicolor"));
        bases.push(PathBuf::from(home).join(".icons/hicolor"));
    }
    bases
}

fn themed_icon_relatives(icon: &str) -> Vec<PathBuf> {
    let mut relatives = Vec::new();
    let file_names = icon_file_names(icon);

    for size in ["512x512", "256x256", "128x128", "96x96", "64x64", "48x48", "32x32", "24x24", "16x16", "scalable"] {
        for kind in ["apps", "categories"] {
            for file_name in &file_names {
                relatives.push(PathBuf::from(size).join(kind).join(file_name));
            }
        }
    }

    relatives
}

fn path_file_name(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
}

#[cfg(target_os = "linux")]
fn execute_action(action_id: &str) -> Result<(), PlatformError> {
    let keys = match action_id {
        "alt_tab" => Some(&[KeyCode::KEY_LEFTALT, KeyCode::KEY_TAB][..]),
        "alt_shift_tab" => Some(&[
            KeyCode::KEY_LEFTALT,
            KeyCode::KEY_LEFTSHIFT,
            KeyCode::KEY_TAB,
        ][..]),
        "show_desktop" => Some(&[KeyCode::KEY_LEFTMETA, KeyCode::KEY_D][..]),
        "task_view" | "mission_control" => Some(&[KeyCode::KEY_LEFTMETA][..]),
        "app_expose" => Some(&[KeyCode::KEY_LEFTMETA, KeyCode::KEY_DOWN][..]),
        "launchpad" => Some(&[KeyCode::KEY_LEFTMETA, KeyCode::KEY_A][..]),
        "space_left" => Some(&[
            KeyCode::KEY_LEFTCTRL,
            KeyCode::KEY_LEFTALT,
            KeyCode::KEY_LEFT,
        ][..]),
        "space_right" => Some(&[
            KeyCode::KEY_LEFTCTRL,
            KeyCode::KEY_LEFTALT,
            KeyCode::KEY_RIGHT,
        ][..]),
        "browser_back" => Some(&[KeyCode::KEY_BACK][..]),
        "browser_forward" => Some(&[KeyCode::KEY_FORWARD][..]),
        "close_tab" => Some(&[KeyCode::KEY_LEFTCTRL, KeyCode::KEY_W][..]),
        "new_tab" => Some(&[KeyCode::KEY_LEFTCTRL, KeyCode::KEY_T][..]),
        "copy" => Some(&[KeyCode::KEY_LEFTCTRL, KeyCode::KEY_C][..]),
        "paste" => Some(&[KeyCode::KEY_LEFTCTRL, KeyCode::KEY_V][..]),
        "cut" => Some(&[KeyCode::KEY_LEFTCTRL, KeyCode::KEY_X][..]),
        "undo" => Some(&[KeyCode::KEY_LEFTCTRL, KeyCode::KEY_Z][..]),
        "select_all" => Some(&[KeyCode::KEY_LEFTCTRL, KeyCode::KEY_A][..]),
        "save" => Some(&[KeyCode::KEY_LEFTCTRL, KeyCode::KEY_S][..]),
        "find" => Some(&[KeyCode::KEY_LEFTCTRL, KeyCode::KEY_F][..]),
        "volume_up" => Some(&[KeyCode::KEY_VOLUMEUP][..]),
        "volume_down" => Some(&[KeyCode::KEY_VOLUMEDOWN][..]),
        "volume_mute" => Some(&[KeyCode::KEY_MUTE][..]),
        "play_pause" => Some(&[KeyCode::KEY_PLAYPAUSE][..]),
        "next_track" => Some(&[KeyCode::KEY_NEXTSONG][..]),
        "prev_track" => Some(&[KeyCode::KEY_PREVIOUSSONG][..]),
        "none" => Some(&[][..]),
        _ => None,
    };

    let Some(keys) = keys else {
        return Err(PlatformError::Unsupported(
            "action is not implemented on Linux",
        ));
    };

    send_key_combo(keys)
}

#[cfg(not(target_os = "linux"))]
fn execute_action(_action_id: &str) -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported(
        "Linux actions are only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
fn send_key_combo(keys: &[KeyCode]) -> Result<(), PlatformError> {
    if keys.is_empty() {
        return Ok(());
    }

    with_virtual_keyboard(|keyboard| {
        let mut batch = Vec::with_capacity(keys.len() * 2);
        for key in keys {
            batch.push(InputEvent::new(1, key.0, 1));
        }
        for key in keys.iter().rev() {
            batch.push(InputEvent::new(1, key.0, 0));
        }
        keyboard
            .emit(&batch)
            .map_err(|error| io_error("emit Linux keyboard events", error))
    })
}

#[cfg(target_os = "linux")]
fn with_virtual_keyboard(
    apply: impl FnOnce(&mut VirtualDevice) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    let keyboard = global_virtual_keyboard();
    let mut guard = keyboard.lock().unwrap();
    if guard.is_none() {
        *guard = Some(create_virtual_keyboard()?);
    }
    apply(guard.as_mut().expect("virtual keyboard should be initialized"))
}

#[cfg(target_os = "linux")]
fn global_virtual_keyboard() -> &'static Mutex<Option<VirtualDevice>> {
    static KEYBOARD: OnceLock<Mutex<Option<VirtualDevice>>> = OnceLock::new();
    KEYBOARD.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "linux")]
fn create_virtual_keyboard() -> Result<VirtualDevice, PlatformError> {
    let mut keys = AttributeSet::<KeyCode>::new();
    for key in [
        KeyCode::KEY_LEFTALT,
        KeyCode::KEY_LEFTSHIFT,
        KeyCode::KEY_LEFTCTRL,
        KeyCode::KEY_LEFTMETA,
        KeyCode::KEY_TAB,
        KeyCode::KEY_A,
        KeyCode::KEY_C,
        KeyCode::KEY_D,
        KeyCode::KEY_F,
        KeyCode::KEY_S,
        KeyCode::KEY_T,
        KeyCode::KEY_V,
        KeyCode::KEY_W,
        KeyCode::KEY_X,
        KeyCode::KEY_Z,
        KeyCode::KEY_LEFT,
        KeyCode::KEY_RIGHT,
        KeyCode::KEY_DOWN,
        KeyCode::KEY_BACK,
        KeyCode::KEY_FORWARD,
        KeyCode::KEY_VOLUMEUP,
        KeyCode::KEY_VOLUMEDOWN,
        KeyCode::KEY_MUTE,
        KeyCode::KEY_PLAYPAUSE,
        KeyCode::KEY_NEXTSONG,
        KeyCode::KEY_PREVIOUSSONG,
    ] {
        keys.insert(key);
    }

    VirtualDevice::builder()
        .map_err(|error| io_error("create Linux virtual keyboard builder", error))?
        .name(b"Mouser Virtual Keyboard")
        .input_id(InputId::new(EvdevBusType::BUS_USB, LOGI_VID, 0xC0DE, 1))
        .with_keys(&keys)
        .map_err(|error| io_error("configure Linux virtual keyboard keys", error))?
        .build()
        .map_err(|error| io_error("create Linux virtual keyboard", error))
}

#[cfg(target_os = "linux")]
fn map_hid_error(error: hidapi::HidError) -> PlatformError {
    PlatformError::Message(error.to_string())
}

fn io_error(action: &str, error: io::Error) -> PlatformError {
    PlatformError::Message(format!("{action}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exec_command_skips_env_and_field_codes() {
        let (executable, executable_path) =
            parse_exec_command("env GTK_THEME=Adwaita /usr/bin/code --unity-launch %F");

        assert_eq!(executable.as_deref(), Some("code"));
        assert_eq!(executable_path.as_deref(), Some("/usr/bin/code"));
    }

    #[test]
    fn parse_desktop_entry_reads_main_section() {
        let entry = parse_desktop_entry(
            r#"
[Desktop Entry]
Type=Application
Name=Firefox
Exec=/usr/bin/firefox %u
Icon=firefox
NoDisplay=false
"#,
        )
        .expect("desktop entry should parse");

        assert_eq!(entry.name, "Firefox");
        assert!(entry.is_application);
        assert!(!entry.no_display);
        assert_eq!(entry.exec.as_deref(), Some("/usr/bin/firefox %u"));
    }
}
