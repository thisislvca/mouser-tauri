#[cfg(target_os = "macos")]
use crate::gesture;
#[cfg(target_os = "macos")]
use crate::hidpp::{self, HidppMessage};
#[cfg(target_os = "macos")]
use crate::macos_iokit::{enumerate_iokit_infos, MacOsIoKitInfo, MacOsNativeHidDevice};
#[cfg(target_os = "macos")]
use crate::push_bounded_hook_event;
use crate::{HookBackend, HookBackendEvent, HookBackendSettings, HookCapabilities, PlatformError};
#[cfg(target_os = "macos")]
use crate::HookDeviceRoute;
#[cfg(target_os = "macos")]
use mouser_core::{
    hydrate_identity_key, resolve_known_device, Binding, DebugEventKind,
    DeviceControlCaptureKind, DeviceControlSpec, DeviceFingerprint, DeviceInfo, DeviceSettings,
    LogicalControl,
};

#[cfg(not(target_os = "macos"))]
pub struct MacOsHookBackend;

#[cfg(not(target_os = "macos"))]
impl MacOsHookBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacOsHookBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_os = "macos"))]
impl HookBackend for MacOsHookBackend {
    fn backend_id(&self) -> &'static str {
        "macos-unsupported"
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
        Err(PlatformError::Unsupported(
            "macOS actions are only available on macOS",
        ))
    }

    fn drain_events(&self) -> Vec<HookBackendEvent> {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{self, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, RwLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use core_foundation::runloop::CFRunLoop;
#[cfg(target_os = "macos")]
use core_foundation::{
    base::TCFType,
    string::{CFString, CFStringRef},
};
#[cfg(target_os = "macos")]
use core_graphics::{
    event::{
        CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions,
        CGEventTapPlacement, CGEventType, CallbackResult, EventField, KeyCode,
    },
    event_source::{CGEventSource, CGEventSourceStateID},
};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
#[cfg(target_os = "macos")]
use objc2_core_graphics::{CGEvent as ObjcCGEvent, CGEventTapLocation as ObjcCGEventTapLocation};
#[cfg(target_os = "macos")]
use objc2_foundation::NSPoint;

#[cfg(target_os = "macos")]
const FEAT_REPROG_V4: u16 = 0x1B04;
#[cfg(target_os = "macos")]
const DEVICE_INDICES: [u8; 7] = [0xFF, 1, 2, 3, 4, 5, 6];
#[cfg(target_os = "macos")]
const GESTURE_DIVERT_FLAGS: u8 = 0x03;
#[cfg(target_os = "macos")]
const GESTURE_RAWXY_FLAGS: u8 = 0x33;
#[cfg(target_os = "macos")]
const GESTURE_UNDIVERT_FLAGS: u8 = 0x02;
#[cfg(target_os = "macos")]
const GESTURE_UNDIVERT_RAWXY_FLAGS: u8 = 0x22;
#[cfg(target_os = "macos")]
const NX_PLAY: isize = 16;
#[cfg(target_os = "macos")]
const NX_NEXT: isize = 17;
#[cfg(target_os = "macos")]
const NX_PREV: isize = 18;
#[cfg(target_os = "macos")]
const NX_MUTE: isize = 7;
#[cfg(target_os = "macos")]
const NX_VOL_UP: isize = 0;
#[cfg(target_os = "macos")]
const NX_VOL_DOWN: isize = 1;
#[cfg(target_os = "macos")]
const SYMBOLIC_HOTKEY_SPACE_LEFT: u32 = 79;
#[cfg(target_os = "macos")]
const SYMBOLIC_HOTKEY_SPACE_RIGHT: u32 = 81;
#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CoreDockSendNotification(notification_name: CFStringRef, unknown: i32) -> i32;
    fn CGSGetSymbolicHotKeyValue(
        hotkey: u32,
        key_equivalent: *mut u16,
        virtual_key: *mut u16,
        modifiers: *mut u32,
    ) -> i32;
    fn CGSIsSymbolicHotKeyEnabled(hotkey: u32) -> bool;
    fn CGSSetSymbolicHotKeyEnabled(hotkey: u32, enabled: bool) -> i32;
}

#[cfg(target_os = "macos")]
#[derive(Clone, Default, PartialEq, Eq)]
struct MacOsHookConfig {
    enabled: bool,
    debug_mode: bool,
    routes: Vec<MacOsDeviceRoute>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, PartialEq, Eq)]
struct MacOsDeviceRoute {
    managed_device_key: String,
    resolved_profile_id: String,
    live_device: DeviceInfo,
    device_settings: DeviceSettings,
    bindings: BTreeMap<LogicalControl, String>,
    device_controls: Vec<DeviceControlSpec>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReprogRoute {
    control: LogicalControl,
    cids: Vec<u16>,
    rawxy_enabled: bool,
}

#[cfg(target_os = "macos")]
impl MacOsHookConfig {
    fn from_runtime(settings: &HookBackendSettings, enabled: bool) -> Self {
        Self {
            enabled,
            debug_mode: settings.debug_mode,
            routes: settings
                .routes
                .iter()
                .cloned()
                .map(MacOsDeviceRoute::from_runtime)
                .collect(),
        }
    }

    fn summary(&self) -> String {
        self.routes
            .iter()
            .map(MacOsDeviceRoute::summary)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn gesture_capture_requested(&self) -> bool {
        self.enabled && self.routes.iter().any(MacOsDeviceRoute::gesture_capture_requested)
    }
}

#[cfg(target_os = "macos")]
impl MacOsDeviceRoute {
    fn from_runtime(route: HookDeviceRoute) -> Self {
        Self {
            managed_device_key: route.managed_device_key,
            resolved_profile_id: route.resolved_profile_id,
            device_controls: route.live_device.controls.clone(),
            bindings: bindings_map(&route.bindings),
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
        .any(|control| self.action_for(control).is_some())
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

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GestureInputSource {
    HidRawxy,
}

#[cfg(target_os = "macos")]
impl GestureInputSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::HidRawxy => "hid_rawxy",
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
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

#[cfg(target_os = "macos")]
struct MacOsHookShared {
    config: RwLock<Arc<MacOsHookConfig>>,
    events: Mutex<Vec<HookBackendEvent>>,
    intercepting: AtomicBool,
    gesture_connected: AtomicBool,
}

#[cfg(target_os = "macos")]
impl MacOsHookShared {
    fn new() -> Self {
        Self {
            config: RwLock::new(Arc::new(MacOsHookConfig::default())),
            events: Mutex::new(Vec::new()),
            intercepting: AtomicBool::new(false),
            gesture_connected: AtomicBool::new(false),
        }
    }

    fn push_event(&self, kind: DebugEventKind, message: impl Into<String>) {
        let mut events = self.events.lock().unwrap();
        push_bounded_hook_event(&mut events, kind, message);
    }

    fn push_debug(&self, message: impl Into<String>) {
        let debug_enabled = self.config.read().unwrap().debug_mode;
        if debug_enabled {
            self.push_event(DebugEventKind::Info, message);
        }
    }

    fn push_gesture_debug(&self, message: impl Into<String>) {
        let debug_enabled = self.config.read().unwrap().debug_mode;
        if debug_enabled {
            self.push_event(DebugEventKind::Gesture, message);
        }
    }

    fn reconfigure(&self, settings: &HookBackendSettings, enabled: bool) {
        let next = Arc::new(MacOsHookConfig::from_runtime(settings, enabled));
        let changed = {
            let mut config = self.config.write().unwrap();
            if config.as_ref() == next.as_ref() {
                false
            } else {
                *config = Arc::clone(&next);
                true
            }
        };

        if changed && next.debug_mode {
            self.push_event(
                DebugEventKind::Info,
                format!("Hook routes -> {}", next.summary()),
            );
        }

        if !next.gesture_capture_requested() {
            self.mark_gesture_connected(false, None);
        }
    }

    fn current_config(&self) -> Arc<MacOsHookConfig> {
        Arc::clone(&self.config.read().unwrap())
    }

    fn mark_gesture_connected(&self, connected: bool, message: Option<String>) {
        self.gesture_connected.store(connected, Ordering::SeqCst);
        if let Some(message) = message {
            self.push_event(DebugEventKind::Info, message);
        }
    }

    fn handle_event(&self, event_type: CGEventType, event: &CGEvent) -> CallbackResult {
        match event_type {
            CGEventType::TapDisabledByTimeout => {
                self.push_event(
                    DebugEventKind::Warning,
                    "CGEventTap disabled by timeout; macOS stopped dispatching live remap events.",
                );
                CallbackResult::Keep
            }
            CGEventType::TapDisabledByUserInput => {
                self.push_event(
                    DebugEventKind::Warning,
                    "CGEventTap disabled by user input; Accessibility permission may need to be re-granted.",
                );
                CallbackResult::Keep
            }
            CGEventType::MouseMoved | CGEventType::OtherMouseDragged => {
                self.handle_motion_event(event)
            }
            CGEventType::OtherMouseDown => self.handle_other_mouse_event(event, true),
            CGEventType::OtherMouseUp => self.handle_other_mouse_event(event, false),
            CGEventType::ScrollWheel => self.handle_scroll_event(event),
            _ => CallbackResult::Keep,
        }
    }

    fn handle_other_mouse_event(&self, event: &CGEvent, is_down: bool) -> CallbackResult {
        let button = event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER);
        let _ = (button, is_down);
        CallbackResult::Keep
    }

    fn handle_scroll_event(&self, event: &CGEvent) -> CallbackResult {
        let _ = event;
        CallbackResult::Keep
    }

    fn handle_motion_event(&self, event: &CGEvent) -> CallbackResult {
        let _ = event;
        CallbackResult::Keep
    }

    fn dispatch_route_control_action(&self, route: &MacOsDeviceRoute, control: LogicalControl) {
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

#[cfg(target_os = "macos")]
pub struct MacOsHookBackend {
    shared: Arc<MacOsHookShared>,
    run_loop: Arc<Mutex<Option<CFRunLoop>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    gesture_stop: Arc<AtomicBool>,
    gesture_worker: Mutex<Option<JoinHandle<()>>>,
}

#[cfg(target_os = "macos")]
impl MacOsHookBackend {
    pub fn new() -> Self {
        let shared = Arc::new(MacOsHookShared::new());
        let run_loop = Arc::new(Mutex::new(None));
        let (startup_tx, startup_rx) = mpsc::channel::<Result<(), ()>>();
        let startup_signal = Arc::new(Mutex::new(Some(startup_tx)));
        let gesture_stop = Arc::new(AtomicBool::new(false));

        let worker_shared = Arc::clone(&shared);
        let worker_loop = Arc::clone(&run_loop);
        let worker_signal = Arc::clone(&startup_signal);
        let callback_shared = Arc::clone(&shared);

        let handle = thread::Builder::new()
            .name("mouser-macos-eventtap".to_string())
            .spawn(move || {
                let startup_signal_for_run = Arc::clone(&startup_signal);
                let startup_signal_for_panic = Arc::clone(&startup_signal);
                let worker_signal_for_run = Arc::clone(&worker_signal);
                let worker_shared_for_run = Arc::clone(&worker_shared);
                let worker_shared_for_panic = Arc::clone(&worker_shared);
                let worker_loop_for_run = Arc::clone(&worker_loop);
                let worker_loop_for_panic = Arc::clone(&worker_loop);

                let run = panic::catch_unwind(AssertUnwindSafe(move || {
                    let current_run_loop = CFRunLoop::get_current();
                    *worker_loop_for_run.lock().unwrap() = Some(current_run_loop.clone());
                    let worker_shared_for_enable = Arc::clone(&worker_shared_for_run);
                    let worker_signal_for_enable = Arc::clone(&worker_signal_for_run);

                    let result = CGEventTap::with_enabled(
                        CGEventTapLocation::Session,
                        CGEventTapPlacement::HeadInsertEventTap,
                        CGEventTapOptions::Default,
                        vec![
                            CGEventType::MouseMoved,
                            CGEventType::OtherMouseDown,
                            CGEventType::OtherMouseUp,
                            CGEventType::OtherMouseDragged,
                            CGEventType::ScrollWheel,
                        ],
                        move |_proxy, event_type, event| {
                            callback_shared.handle_event(event_type, event)
                        },
                        || {
                            worker_shared_for_enable.intercepting.store(true, Ordering::SeqCst);
                            if let Some(tx) = worker_signal_for_enable.lock().unwrap().take() {
                                let _ = tx.send(Ok(()));
                            }
                            CFRunLoop::run_current();
                        },
                    );

                    *worker_loop_for_run.lock().unwrap() = None;
                    worker_shared_for_run.intercepting.store(false, Ordering::SeqCst);

                    if result.is_err() {
                        worker_shared_for_run.push_event(
                            DebugEventKind::Warning,
                            "Failed to start macOS CGEventTap. Grant Accessibility access in System Settings > Privacy & Security > Accessibility.",
                        );
                        if let Some(tx) = startup_signal_for_run.lock().unwrap().take() {
                            let _ = tx.send(Err(()));
                        }
                    }
                }));

                if let Err(payload) = run {
                    *worker_loop_for_panic.lock().unwrap() = None;
                    worker_shared_for_panic.intercepting.store(false, Ordering::SeqCst);
                    let panic_message = payload
                        .downcast_ref::<&str>()
                        .map(|message| (*message).to_string())
                        .or_else(|| payload.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown panic payload".to_string());
                    worker_shared_for_panic.push_event(
                        DebugEventKind::Warning,
                        format!("macOS event tap thread panicked: {panic_message}"),
                    );
                    if let Some(tx) = startup_signal_for_panic.lock().unwrap().take() {
                        let _ = tx.send(Err(()));
                    }
                }
            })
            .ok();

        match startup_rx.recv_timeout(Duration::from_millis(800)) {
            Ok(Ok(())) => {}
            Ok(Err(())) => {}
            Err(_) => {
                shared.push_event(
                    DebugEventKind::Warning,
                    "Timed out while starting the macOS event tap; live remapping may stay unavailable until the next launch.",
                );
            }
        }

        let gesture_shared = Arc::clone(&shared);
        let gesture_stop_flag = Arc::clone(&gesture_stop);
        let gesture_worker = thread::Builder::new()
            .name("mouser-macos-gesture".to_string())
            .spawn(move || run_gesture_worker(gesture_shared, gesture_stop_flag))
            .ok();

        Self {
            shared,
            run_loop,
            worker: Mutex::new(handle),
            gesture_stop,
            gesture_worker: Mutex::new(gesture_worker),
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacOsHookBackend {
    fn drop(&mut self) {
        if let Some(run_loop) = self.run_loop.lock().unwrap().clone() {
            run_loop.stop();
        }

        if let Some(handle) = self.worker.lock().unwrap().take() {
            let _ = handle.join();
        }

        self.gesture_stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.gesture_worker.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

#[cfg(target_os = "macos")]
impl HookBackend for MacOsHookBackend {
    fn backend_id(&self) -> &'static str {
        let intercepting = self.shared.intercepting.load(Ordering::SeqCst);
        let gesture_connected = self.shared.gesture_connected.load(Ordering::SeqCst);

        if intercepting && gesture_connected {
            "macos-eventtap+iokit-gesture"
        } else if intercepting {
            "macos-eventtap"
        } else if gesture_connected {
            "macos-iokit-gesture"
        } else {
            "macos-eventtap-unavailable"
        }
    }

    fn capabilities(&self) -> HookCapabilities {
        let intercepting = self.shared.intercepting.load(Ordering::SeqCst);
        HookCapabilities {
            can_intercept_buttons: intercepting,
            can_intercept_scroll: intercepting,
            supports_gesture_diversion: self.shared.gesture_connected.load(Ordering::SeqCst),
        }
    }

    fn configure(
        &self,
        settings: &HookBackendSettings,
        enabled: bool,
    ) -> Result<(), PlatformError> {
        self.shared.reconfigure(settings, enabled);
        Ok(())
    }

    fn execute_action(&self, action_id: &str) -> Result<(), PlatformError> {
        execute_action(action_id)
    }

    fn drain_events(&self) -> Vec<HookBackendEvent> {
        let mut events = self.shared.events.lock().unwrap();
        std::mem::take(&mut *events)
    }
}

#[cfg(target_os = "macos")]
fn bindings_map(bindings: &[Binding]) -> BTreeMap<LogicalControl, String> {
    bindings
        .iter()
        .map(|binding| (binding.control, binding.action_id.clone()))
        .collect()
}

#[cfg(target_os = "macos")]
struct GestureSession {
    route: MacOsDeviceRoute,
    info: MacOsIoKitInfo,
    device: MacOsNativeHidDevice,
    dev_idx: u8,
    feature_idx: u8,
    routes: Vec<ReprogRoute>,
    active_cids: BTreeSet<u16>,
    gesture_active: bool,
    tracking_state: GestureTrackingState,
}

#[cfg(target_os = "macos")]
impl GestureSession {
    fn route_key(&self) -> &str {
        &self.route.managed_device_key
    }

    fn product_label(&self) -> String {
        self.info
            .product_string
            .clone()
            .unwrap_or_else(|| format!("PID 0x{:04X}", self.info.product_id))
    }

    fn matches_route(&self, route: &MacOsDeviceRoute) -> bool {
        &self.route == route
    }

    fn handle_report(&mut self, shared: &MacOsHookShared, raw: &[u8]) {
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

    fn handle_hid_gesture_down(&mut self, shared: &MacOsHookShared) {
        let route_key = self.route_key().to_string();
        let state = &mut self.tracking_state;
        if state.active {
            return;
        }

        state.active = true;
        state.triggered = false;
        shared.push_gesture_debug(format!("Gesture button down [{route_key}]"));

        if self.route.gesture_direction_enabled() && !cooldown_active(state) {
            start_gesture_tracking(state);
        } else {
            state.tracking = false;
            state.triggered = false;
        }
    }

    fn handle_hid_gesture_up(&mut self, shared: &MacOsHookShared) {
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

    fn handle_hid_rawxy_move(&mut self, shared: &MacOsHookShared, delta_x: i16, delta_y: i16) {
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
        shared: &MacOsHookShared,
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
                > u128::from(self.route.device_settings.gesture_timeout_ms)
        });
        if idle_timed_out {
            shared.push_gesture_debug(format!(
                "Gesture segment reset after {} ms [{}]",
                self.route.device_settings.gesture_timeout_ms,
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
            f64::from(self.route.device_settings.gesture_threshold),
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

#[cfg(target_os = "macos")]
fn run_gesture_worker(shared: Arc<MacOsHookShared>, stop: Arc<AtomicBool>) {
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
            thread::sleep(Duration::from_millis(180));
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

        let infos = match enumerate_iokit_infos() {
            Ok(infos) => infos,
            Err(error) => {
                shared.mark_gesture_connected(
                    false,
                    Some(format!("Gesture listener unavailable: {error}")),
                );
                thread::sleep(Duration::from_millis(900));
                continue;
            }
        };

        let mut last_error = None;
        for route in desired_routes {
            if sessions.contains_key(&route.managed_device_key) {
                continue;
            }
            match try_build_gesture_session_for_route(&shared, &route, &infos) {
                Ok(session) => {
                    let source = session.product_label();
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
            thread::sleep(Duration::from_millis(500));
            continue;
        }

        shared.mark_gesture_connected(true, None);

        let mut disconnected = Vec::new();
        for (route_key, active_session) in sessions.iter_mut() {
            match active_session.device.read_timeout(12) {
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
            if let Some(mut failed_session) = sessions.remove(&route_key) {
                failed_session.shutdown();
            }
        }
    }

    for (_, mut session) in sessions {
        session.shutdown();
    }
    shared.mark_gesture_connected(false, None);
}

#[cfg(target_os = "macos")]
fn try_build_gesture_session_for_route(
    shared: &MacOsHookShared,
    route: &MacOsDeviceRoute,
    infos: &[MacOsIoKitInfo],
) -> Result<GestureSession, PlatformError> {
    let mut last_error = None;

    for info in infos
        .iter()
        .filter(|info| iokit_info_matches_route(info, route))
        .cloned()
    {
        for candidate in iokit_open_candidates(&info) {
        let Ok(device) = MacOsNativeHidDevice::open(&candidate) else {
            continue;
        };

            match initialize_gesture_session(shared, route.clone(), info.clone(), device) {
            Ok(session) => return Ok(session),
            Err(error) => last_error = Some(error),
        }
    }
    }

    Err(last_error.unwrap_or_else(|| {
        PlatformError::Message(format!(
            "could not initialize gesture diversion for {}",
            route.managed_device_key
        ))
    }))
}

#[cfg(target_os = "macos")]
fn initialize_gesture_session(
    shared: &MacOsHookShared,
    route: MacOsDeviceRoute,
    info: MacOsIoKitInfo,
    device: MacOsNativeHidDevice,
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
                info,
                device,
                dev_idx,
                feature_idx,
                routes: routes.clone(),
                active_cids: BTreeSet::new(),
                gesture_active: false,
                tracking_state: GestureTrackingState::default(),
            });
        }
    }

    Err(PlatformError::Message(format!(
        "logitech reprog diversion failed for pid 0x{:04X}",
        info.product_id
    )))
}

#[cfg(target_os = "macos")]
fn try_initialize_reprog_session(
    shared: &MacOsHookShared,
    device: &MacOsNativeHidDevice,
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

#[cfg(target_os = "macos")]
fn collect_active_cids(params: &[u8]) -> BTreeSet<u16> {
    params
        .chunks_exact(2)
        .take_while(|pair| pair[0] != 0 || pair[1] != 0)
        .map(|pair| u16::from(pair[0]) << 8 | u16::from(pair[1]))
        .collect()
}

#[cfg(target_os = "macos")]
fn iokit_info_matches_route(info: &MacOsIoKitInfo, route: &MacOsDeviceRoute) -> bool {
    iokit_info_matches_active_target(
        info,
        Some(route.live_device.model_key.as_str()),
        route.live_device.fingerprint.identity_key.as_deref(),
    )
}

#[cfg(target_os = "macos")]
fn iokit_info_matches_active_target(
    info: &MacOsIoKitInfo,
    model_key: Option<&str>,
    identity_key: Option<&str>,
) -> bool {
    if let Some(identity_key) = normalized_identity_key(identity_key) {
        return normalized_identity_key(
            fingerprint_from_iokit_info(info).identity_key.as_deref(),
        ) == Some(identity_key);
    }

    model_key.is_some_and(|model_key| {
        resolve_known_device(Some(info.product_id), info.product_string.as_deref())
            .map(|spec| spec.key == model_key)
            .unwrap_or(false)
    })
}

#[cfg(target_os = "macos")]
fn normalized_identity_key(value: Option<&str>) -> Option<&str> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    })
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
fn find_hidpp_feature(
    device: &MacOsNativeHidDevice,
    dev_idx: u8,
    feature_id: u16,
    timeout_ms: i32,
) -> Result<Option<u8>, PlatformError> {
    hidpp::find_feature(device, dev_idx, feature_id, timeout_ms)
}

#[cfg(target_os = "macos")]
fn set_gesture_reporting(
    device: &MacOsNativeHidDevice,
    dev_idx: u8,
    feature_idx: u8,
    gesture_cid: u16,
    flags: u8,
    timeout_ms: i32,
) -> Result<Option<HidppMessage>, PlatformError> {
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

#[cfg(target_os = "macos")]
fn decode_s16(hi: u8, lo: u8) -> i16 {
    let value = u16::from(hi) << 8 | u16::from(lo);
    value as i16
}

#[cfg(target_os = "macos")]
fn cooldown_active(state: &GestureTrackingState) -> bool {
    state
        .cooldown_until
        .is_some_and(|cooldown_until| Instant::now() < cooldown_until)
}

#[cfg(target_os = "macos")]
fn start_gesture_tracking(state: &mut GestureTrackingState) {
    let now = Instant::now();
    state.tracking = true;
    state.started_at = Some(now);
    state.last_move_at = Some(now);
    state.delta_x = 0.0;
    state.delta_y = 0.0;
    state.input_source = None;
}

#[cfg(target_os = "macos")]
fn finish_gesture_tracking(state: &mut GestureTrackingState) {
    state.tracking = false;
    state.started_at = None;
    state.last_move_at = None;
    state.delta_x = 0.0;
    state.delta_y = 0.0;
    state.input_source = None;
}

#[cfg(target_os = "macos")]
fn detect_gesture_control(
    delta_x: f64,
    delta_y: f64,
    threshold: f64,
    deadzone: f64,
) -> Option<LogicalControl> {
    gesture::detect_gesture_control(delta_x, delta_y, threshold, deadzone)
}

#[cfg(target_os = "macos")]
fn execute_action(action_id: &str) -> Result<(), PlatformError> {
    if execute_private_macos_action(action_id)? {
        return Ok(());
    }

    match action_id {
        "none" => Ok(()),
        "alt_tab" => send_key_combo(&[KeyCode::COMMAND, KeyCode::TAB]),
        "alt_shift_tab" => send_key_combo(&[KeyCode::COMMAND, KeyCode::SHIFT, KeyCode::TAB]),
        "show_desktop" => send_key_combo(&[KeyCode::F11]),
        "task_view" => send_key_combo(&[KeyCode::CONTROL, KeyCode::UP_ARROW]),
        "mission_control" => send_key_combo(&[KeyCode::CONTROL, KeyCode::UP_ARROW]),
        "app_expose" => send_key_combo(&[KeyCode::CONTROL, KeyCode::DOWN_ARROW]),
        "launchpad" => send_key_combo(&[KeyCode::F4]),
        "space_left" => send_key_combo(&[KeyCode::CONTROL, KeyCode::LEFT_ARROW]),
        "space_right" => send_key_combo(&[KeyCode::CONTROL, KeyCode::RIGHT_ARROW]),
        "browser_back" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_LEFT_BRACKET]),
        "browser_forward" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_RIGHT_BRACKET]),
        "close_tab" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_W]),
        "new_tab" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_T]),
        "copy" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_C]),
        "paste" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_V]),
        "cut" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_X]),
        "undo" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_Z]),
        "redo" => send_key_combo(&[KeyCode::COMMAND, KeyCode::SHIFT, KeyCode::ANSI_Z]),
        "select_all" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_A]),
        "save" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_S]),
        "find" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_F]),
        "screen_capture" => {
            send_key_combo(&[KeyCode::COMMAND, KeyCode::SHIFT, KeyCode::ANSI_4])
        }
        "emoji_picker" => {
            send_key_combo(&[KeyCode::CONTROL, KeyCode::COMMAND, KeyCode::SPACE])
        }
        "volume_up" => send_media_key(NX_VOL_UP),
        "volume_down" => send_media_key(NX_VOL_DOWN),
        "volume_mute" => send_media_key(NX_MUTE),
        "play_pause" => send_media_key(NX_PLAY),
        "next_track" => send_media_key(NX_NEXT),
        "prev_track" => send_media_key(NX_PREV),
        unsupported => Err(PlatformError::Message(format!(
            "unsupported action `{unsupported}`"
        ))),
    }
}

#[cfg(target_os = "macos")]
fn execute_private_macos_action(action_id: &str) -> Result<bool, PlatformError> {
    match action_id {
        "mission_control" => send_dock_notification("com.apple.expose.awake"),
        "app_expose" => send_dock_notification("com.apple.expose.front.awake"),
        "show_desktop" => send_dock_notification("com.apple.showdesktop.awake"),
        "launchpad" => send_dock_notification("com.apple.launchpad.toggle"),
        "space_left" => post_symbolic_hotkey(SYMBOLIC_HOTKEY_SPACE_LEFT),
        "space_right" => post_symbolic_hotkey(SYMBOLIC_HOTKEY_SPACE_RIGHT),
        _ => Ok(false),
    }
}

#[cfg(target_os = "macos")]
fn send_key_combo(keys: &[u16]) -> Result<(), PlatformError> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| PlatformError::Message("failed to create CGEventSource".to_string()))?;
    let flags = modifier_flags(keys);

    for keycode in keys {
        let event = CGEvent::new_keyboard_event(source.clone(), *keycode, true)
            .map_err(|_| PlatformError::Message("failed to create key-down event".to_string()))?;
        if !flags.is_empty() {
            event.set_flags(flags);
        }
        event.post(CGEventTapLocation::HID);
    }

    if keys.len() > 1 {
        thread::sleep(Duration::from_millis(45));
    }

    for keycode in keys.iter().rev() {
        let event = CGEvent::new_keyboard_event(source.clone(), *keycode, false)
            .map_err(|_| PlatformError::Message("failed to create key-up event".to_string()))?;
        event.post(CGEventTapLocation::HID);
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn modifier_flags(keys: &[u16]) -> CGEventFlags {
    let mut flags = CGEventFlags::empty();
    for key in keys {
        match *key {
            KeyCode::COMMAND | KeyCode::RIGHT_COMMAND => {
                flags |= CGEventFlags::CGEventFlagCommand;
            }
            KeyCode::SHIFT | KeyCode::RIGHT_SHIFT => {
                flags |= CGEventFlags::CGEventFlagShift;
            }
            KeyCode::OPTION | KeyCode::RIGHT_OPTION => {
                flags |= CGEventFlags::CGEventFlagAlternate;
            }
            KeyCode::CONTROL | KeyCode::RIGHT_CONTROL => {
                flags |= CGEventFlags::CGEventFlagControl;
            }
            _ => {}
        }
    }
    flags
}

#[cfg(target_os = "macos")]
fn send_dock_notification(notification_name: &str) -> Result<bool, PlatformError> {
    let notification = CFString::new(notification_name);
    let result = unsafe { CoreDockSendNotification(notification.as_concrete_TypeRef(), 0) };
    Ok(result == 0)
}

#[cfg(target_os = "macos")]
fn post_symbolic_hotkey(hotkey: u32) -> Result<bool, PlatformError> {
    let mut key_equivalent = 0u16;
    let mut virtual_key = 0u16;
    let mut modifiers = 0u32;
    let result = unsafe {
        CGSGetSymbolicHotKeyValue(
            hotkey,
            &mut key_equivalent,
            &mut virtual_key,
            &mut modifiers,
        )
    };
    if result != 0 {
        return Ok(false);
    }

    let was_enabled = unsafe { CGSIsSymbolicHotKeyEnabled(hotkey) };
    if !was_enabled {
        unsafe {
            CGSSetSymbolicHotKeyEnabled(hotkey, true);
        }
    }

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| PlatformError::Message("failed to create CGEventSource".to_string()))?;
    let flags = CGEventFlags::from_bits_truncate(u64::from(modifiers));

    let key_down = CGEvent::new_keyboard_event(source.clone(), virtual_key, true)
        .map_err(|_| PlatformError::Message("failed to create key-down event".to_string()))?;
    key_down.set_flags(flags);
    key_down.post(CGEventTapLocation::Session);

    let key_up = CGEvent::new_keyboard_event(source, virtual_key, false)
        .map_err(|_| PlatformError::Message("failed to create key-up event".to_string()))?;
    key_up.set_flags(flags);
    key_up.post(CGEventTapLocation::Session);

    thread::sleep(Duration::from_millis(50));

    if !was_enabled {
        unsafe {
            CGSSetSymbolicHotKeyEnabled(hotkey, false);
        }
    }

    Ok(true)
}

#[cfg(target_os = "macos")]
fn send_media_key(key_id: isize) -> Result<(), PlatformError> {
    let down = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::SystemDefined,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags(0xA00),
        0.0,
        0,
        None,
        8,
        (key_id << 16) | (0xA << 8),
        -1,
    )
    .ok_or_else(|| PlatformError::Message("failed to create media-key down event".to_string()))?;

    let up = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::SystemDefined,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags(0xB00),
        0.0,
        0,
        None,
        8,
        (key_id << 16) | (0xB << 8),
        -1,
    )
    .ok_or_else(|| PlatformError::Message("failed to create media-key up event".to_string()))?;

    let down_cg = down.CGEvent().ok_or_else(|| {
        PlatformError::Message("media-key down event missing CGEvent".to_string())
    })?;
    let up_cg = up
        .CGEvent()
        .ok_or_else(|| PlatformError::Message("media-key up event missing CGEvent".to_string()))?;

    ObjcCGEvent::post(ObjcCGEventTapLocation::HIDEventTap, Some(&down_cg));
    ObjcCGEvent::post(ObjcCGEventTapLocation::HIDEventTap, Some(&up_cg));
    Ok(())
}
