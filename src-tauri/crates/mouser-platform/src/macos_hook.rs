#[cfg(target_os = "macos")]
use crate::gesture;
#[cfg(target_os = "macos")]
use crate::hidpp::{self, HidppMessage};
#[cfg(target_os = "macos")]
use crate::macos_iokit::{
    enumerate_iokit_infos, MacOsInputValueEvent, MacOsIoKitInfo, MacOsNativeHidDevice,
};
#[cfg(target_os = "macos")]
use crate::HookDeviceRoute;
#[cfg(target_os = "macos")]
use crate::{backend_debug_logging_enabled, emit_backend_console_log};
#[cfg(target_os = "macos")]
use crate::{horizontal_scroll_control, push_bounded_hook_event};
use crate::{HookBackend, HookBackendEvent, HookBackendSettings, HookCapabilities, PlatformError};
#[cfg(target_os = "macos")]
use mouser_core::{
    hydrate_identity_key, resolve_known_device, Binding, DebugEventKind, DebugLogGroup,
    DebugLogGroups, DeviceControlCaptureKind, DeviceControlSpec, DeviceFingerprint, DeviceInfo,
    DeviceSettings, LogicalControl,
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
    collections::{BTreeMap, BTreeSet, VecDeque},
    panic::{self, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Condvar, Mutex, RwLock,
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
        CGEventTapPlacement, CGEventType, CallbackResult, EventField, KeyCode, ScrollEventUnit,
    },
    event_source::{CGEventSource, CGEventSourceStateID},
    geometry::CGPoint,
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
const SCROLL_INVERT_MARKER: i64 = 0x4D4F5553;
#[cfg(target_os = "macos")]
const THUMB_WHEEL_TRACKPAD_MARKER: i64 = 0x4D575450;
#[cfg(target_os = "macos")]
const SCROLL_WHEEL_EVENT_SCROLL_PHASE_FIELD: u32 = 99;
#[cfg(target_os = "macos")]
const SCROLL_WHEEL_EVENT_MOMENTUM_PHASE_FIELD: u32 = 123;
#[cfg(target_os = "macos")]
const SCROLL_WHEEL_TRACKPAD_POLL_INTERVAL_MS: u64 = 8;
#[cfg(target_os = "macos")]
const SCROLL_WHEEL_TRACKPAD_SMOOTHING_FACTOR: f64 = 0.45;
#[cfg(target_os = "macos")]
const SCROLL_WHEEL_TRACKPAD_MIN_STEP: f64 = 1.0;
#[cfg(target_os = "macos")]
const SCROLL_WHEEL_TRACKPAD_MAX_STEP: f64 = 12.0;
#[cfg(target_os = "macos")]
const THUMB_WHEEL_USAGE_PAGE_GENERIC_DESKTOP: u32 = 0x0001;
#[cfg(target_os = "macos")]
const THUMB_WHEEL_USAGE_PAGE_CONSUMER: u32 = 0x000C;
#[cfg(target_os = "macos")]
const THUMB_WHEEL_USAGE_AC_PAN: u32 = 0x0238;
#[cfg(target_os = "macos")]
const BTN_MIDDLE: i64 = 2;
#[cfg(target_os = "macos")]
const BTN_BACK: i64 = 3;
#[cfg(target_os = "macos")]
const BTN_FORWARD: i64 = 4;
#[cfg(target_os = "macos")]
const THUMB_WHEEL_MATCH_WINDOW_MS: u64 = 40;
#[cfg(target_os = "macos")]
const HID_DEBUG_LOG_INTERVAL_MS: u64 = 500;
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
    debug_log_groups: DebugLogGroups,
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
            debug_log_groups: settings.debug_log_groups.clone(),
            routes: settings
                .routes
                .iter()
                .cloned()
                .map(MacOsDeviceRoute::from_runtime)
                .collect(),
        }
    }

    fn debug_logging_enabled(&self, group: DebugLogGroup) -> bool {
        backend_debug_logging_enabled(self.debug_mode, &self.debug_log_groups, group)
    }

    fn summary(&self) -> String {
        self.routes
            .iter()
            .map(MacOsDeviceRoute::summary)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn gesture_capture_requested(&self) -> bool {
        self.enabled
            && self
                .routes
                .iter()
                .any(MacOsDeviceRoute::gesture_capture_requested)
    }

    fn unique_route_for_control(&self, control: LogicalControl) -> Option<MacOsDeviceRoute> {
        if !self.enabled {
            return None;
        }

        let mut matches = self
            .routes
            .iter()
            .filter(|route| route.handles_control(control))
            .cloned();
        let route = matches.next()?;
        matches.next().is_none().then_some(route)
    }

    fn unique_vertical_inversion_route(&self) -> Option<MacOsDeviceRoute> {
        if !self.enabled {
            return None;
        }

        let mut matches = self
            .routes
            .iter()
            .filter(|route| route.device_settings.invert_vertical_scroll)
            .cloned();
        let route = matches.next()?;
        matches.next().is_none().then_some(route)
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

    fn thumb_wheel_trackpad_requested(&self) -> bool {
        self.device_settings.macos_thumb_wheel_simulate_trackpad
    }

    fn thumb_wheel_hid_requested(&self) -> bool {
        self.thumb_wheel_trackpad_requested()
            || self.handles_control(LogicalControl::HscrollLeft)
            || self.handles_control(LogicalControl::HscrollRight)
            || self.device_settings.invert_horizontal_scroll
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
#[derive(Clone, Copy, Debug, Default)]
struct ScrollAxisDeltas {
    line: i64,
    fixed: i64,
    point: i64,
}

#[cfg(target_os = "macos")]
impl ScrollAxisDeltas {
    fn is_zero(self) -> bool {
        self.line == 0 && self.fixed == 0 && self.point == 0
    }

    fn direction(self) -> std::cmp::Ordering {
        preferred_scroll_delta(self).cmp(&0)
    }

    fn inverted(self) -> Self {
        Self {
            line: -self.line,
            fixed: -self.fixed,
            point: -self.point,
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug)]
enum ThumbWheelTrackpadPhase {
    Began,
    Changed,
    Ended,
}

#[cfg(target_os = "macos")]
impl ThumbWheelTrackpadPhase {
    fn as_scroll_phase(self) -> i64 {
        match self {
            Self::Began => 1,
            Self::Changed => 2,
            Self::Ended => 4,
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Default)]
struct ThumbWheelGestureState {
    active: bool,
    emitted_motion: bool,
    end_requested: bool,
    pending_point_delta: f64,
    last_wheel_at: Option<Instant>,
    last_location: CGPoint,
    last_flags: CGEventFlags,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug)]
struct ThumbWheelHidSample {
    observed_at: Instant,
    direction: std::cmp::Ordering,
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct ThumbWheelRouteState {
    gesture: ThumbWheelGestureState,
    hid_samples: VecDeque<ThumbWheelHidSample>,
}

#[cfg(target_os = "macos")]
enum ThumbWheelWorkerState {
    Action(
        CGPoint,
        CGEventFlags,
        ThumbWheelTrackpadPhase,
        ScrollAxisDeltas,
    ),
    Wait,
    WaitTimeout(Duration),
}

#[cfg(target_os = "macos")]
fn thumb_wheel_worker_state_for_state(
    state: &mut ThumbWheelGestureState,
    hold_timeout: Duration,
    now: Instant,
) -> ThumbWheelWorkerState {
    if !state.active {
        return ThumbWheelWorkerState::Wait;
    }

    let location = state.last_location;
    let flags = state.last_flags;
    if let Some(step) = next_thumb_wheel_point_step(state.pending_point_delta) {
        state.pending_point_delta -= step as f64;
        if state.pending_point_delta.abs() < 0.5 {
            state.pending_point_delta = 0.0;
        }
        let phase = if state.emitted_motion {
            ThumbWheelTrackpadPhase::Changed
        } else {
            state.emitted_motion = true;
            ThumbWheelTrackpadPhase::Began
        };
        return ThumbWheelWorkerState::Action(
            location,
            flags,
            phase,
            ScrollAxisDeltas {
                line: 0,
                fixed: step,
                point: step,
            },
        );
    }

    let should_end = state.end_requested
        || state
            .last_wheel_at
            .is_some_and(|last_wheel_at| now.duration_since(last_wheel_at) >= hold_timeout);
    if !should_end {
        return state
            .last_wheel_at
            .and_then(|last_wheel_at| hold_timeout.checked_sub(now.duration_since(last_wheel_at)))
            .map(ThumbWheelWorkerState::WaitTimeout)
            .unwrap_or(ThumbWheelWorkerState::Wait);
    }

    let emitted_motion = state.emitted_motion;
    *state = ThumbWheelGestureState::default();
    if !emitted_motion {
        return ThumbWheelWorkerState::Wait;
    }

    ThumbWheelWorkerState::Action(
        location,
        flags,
        ThumbWheelTrackpadPhase::Ended,
        ScrollAxisDeltas::default(),
    )
}

#[cfg(target_os = "macos")]
struct MacOsHookShared {
    config: RwLock<Arc<MacOsHookConfig>>,
    events: Mutex<Vec<HookBackendEvent>>,
    hid_debug_timestamps: Mutex<BTreeMap<String, Instant>>,
    thumb_wheel_states: Mutex<BTreeMap<String, ThumbWheelRouteState>>,
    thumb_wheel_cv: Condvar,
    intercepting: AtomicBool,
    gesture_connected: AtomicBool,
}

#[cfg(target_os = "macos")]
impl MacOsHookShared {
    fn new() -> Self {
        Self {
            config: RwLock::new(Arc::new(MacOsHookConfig::default())),
            events: Mutex::new(Vec::new()),
            hid_debug_timestamps: Mutex::new(BTreeMap::new()),
            thumb_wheel_states: Mutex::new(BTreeMap::new()),
            thumb_wheel_cv: Condvar::new(),
            intercepting: AtomicBool::new(false),
            gesture_connected: AtomicBool::new(false),
        }
    }

    fn push_event(&self, kind: DebugEventKind, message: impl Into<String>) {
        let mut events = self.events.lock().unwrap();
        push_bounded_hook_event(&mut events, kind, message);
    }

    fn push_status(&self, group: DebugLogGroup, kind: DebugEventKind, message: impl Into<String>) {
        let message = message.into();
        self.log_console(group, kind, &message);
        self.push_event(kind, message);
    }

    fn log_console(&self, group: DebugLogGroup, kind: DebugEventKind, message: impl Into<String>) {
        let message = message.into();
        let config = self.current_config();
        if config.debug_logging_enabled(group) {
            emit_backend_console_log("macos", kind, group, &message);
        }
    }

    fn push_debug(&self, message: impl Into<String>) {
        self.log_console(DebugLogGroup::HookRouting, DebugEventKind::Info, message);
    }

    fn push_hid_debug_rate_limited(&self, key: impl Into<String>, message: impl Into<String>) {
        let config = self.current_config();
        if !config.debug_logging_enabled(DebugLogGroup::Hid) {
            return;
        }

        let key = key.into();
        let now = Instant::now();
        let mut timestamps = self.hid_debug_timestamps.lock().unwrap();
        if timestamps.get(&key).is_some_and(|last| {
            now.duration_since(*last) < Duration::from_millis(HID_DEBUG_LOG_INTERVAL_MS)
        }) {
            return;
        }
        timestamps.insert(key, now);
        drop(timestamps);

        emit_backend_console_log(
            "macos",
            DebugEventKind::Info,
            DebugLogGroup::Hid,
            &message.into(),
        );
    }

    fn push_thumb_wheel_debug(&self, message: impl Into<String>) {
        self.log_console(DebugLogGroup::ThumbWheel, DebugEventKind::Info, message);
    }

    fn push_gesture_debug(&self, message: impl Into<String>) {
        self.log_console(DebugLogGroup::Gestures, DebugEventKind::Gesture, message);
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

        if changed && next.debug_logging_enabled(DebugLogGroup::HookRouting) {
            emit_backend_console_log(
                "macos",
                DebugEventKind::Info,
                DebugLogGroup::HookRouting,
                &format!("Hook routes -> {}", next.summary()),
            );
        }

        self.retain_thumb_wheel_routes(&next);

        if !next.gesture_capture_requested() {
            self.mark_gesture_connected(false, None);
        }
    }

    fn current_config(&self) -> Arc<MacOsHookConfig> {
        Arc::clone(&self.config.read().unwrap())
    }

    fn retain_thumb_wheel_routes(&self, config: &MacOsHookConfig) {
        let trackpad_routes = config
            .routes
            .iter()
            .filter(|route| route.thumb_wheel_hid_requested())
            .map(|route| route.managed_device_key.as_str())
            .collect::<BTreeSet<_>>();
        let active_trackpad_keys = config
            .routes
            .iter()
            .filter(|route| route.thumb_wheel_trackpad_requested())
            .map(|route| route.managed_device_key.as_str())
            .collect::<BTreeSet<_>>();
        let mut states = self.thumb_wheel_states.lock().unwrap();
        states.retain(|route_key, state| {
            let keep = trackpad_routes.contains(route_key.as_str());
            if keep && !active_trackpad_keys.contains(route_key.as_str()) {
                state.gesture = ThumbWheelGestureState::default();
            }
            keep
        });
        self.thumb_wheel_cv.notify_all();
    }

    fn note_thumb_wheel_hid_event(&self, route_key: &str, event: MacOsInputValueEvent) {
        let direction = event.value.cmp(&0);
        if direction == std::cmp::Ordering::Equal {
            return;
        }

        let mut states = self.thumb_wheel_states.lock().unwrap();
        let state = states.entry(route_key.to_string()).or_default();
        state.hid_samples.push_back(ThumbWheelHidSample {
            observed_at: event.observed_at,
            direction,
        });
        prune_thumb_wheel_hid_samples(
            &mut state.hid_samples,
            event.observed_at,
            Duration::from_millis(THUMB_WHEEL_MATCH_WINDOW_MS),
        );
    }

    fn match_thumb_wheel_route(
        &self,
        config: &MacOsHookConfig,
        event: &CGEvent,
    ) -> Option<(MacOsDeviceRoute, ScrollAxisDeltas)> {
        let deltas = extract_thumb_wheel_scroll_deltas(event)?;
        let direction = deltas.direction();
        if direction == std::cmp::Ordering::Equal {
            return None;
        }

        let now = Instant::now();
        let window = Duration::from_millis(THUMB_WHEEL_MATCH_WINDOW_MS);
        let mut states = self.thumb_wheel_states.lock().unwrap();
        let mut matches = Vec::new();

        for route in config
            .routes
            .iter()
            .filter(|route| route.thumb_wheel_hid_requested())
        {
            let Some(state) = states.get_mut(&route.managed_device_key) else {
                continue;
            };
            prune_thumb_wheel_hid_samples(&mut state.hid_samples, now, window);
            if state
                .hid_samples
                .front()
                .is_some_and(|sample| sample.direction == direction)
            {
                matches.push(route.clone());
            }
        }

        if matches.len() != 1 {
            return None;
        }

        let route = matches.pop().unwrap();
        if let Some(state) = states.get_mut(&route.managed_device_key) {
            let _ = state.hid_samples.pop_front();
        }
        Some((route, deltas))
    }

    fn enqueue_thumb_wheel_trackpad_scroll(
        &self,
        route: &MacOsDeviceRoute,
        mut deltas: ScrollAxisDeltas,
        location: CGPoint,
        flags: CGEventFlags,
    ) {
        if route.device_settings.invert_horizontal_scroll {
            deltas = deltas.inverted();
        }
        let now = Instant::now();
        let started = {
            let mut states = self.thumb_wheel_states.lock().unwrap();
            let state = states.entry(route.managed_device_key.clone()).or_default();
            let started = !state.gesture.active;
            state.gesture.active = true;
            state.gesture.end_requested = false;
            state.gesture.pending_point_delta += deltas.point as f64;
            state.gesture.last_wheel_at = Some(now);
            state.gesture.last_location = location;
            state.gesture.last_flags = flags;
            started
        };

        self.thumb_wheel_cv.notify_all();

        if started {
            self.push_thumb_wheel_debug(format!(
                "Thumb wheel trackpad swipe started [{}]",
                route.managed_device_key
            ));
        }
    }

    fn reset_thumb_wheel_state(&self, route_key: &str) {
        let mut states = self.thumb_wheel_states.lock().unwrap();
        if let Some(state) = states.get_mut(route_key) {
            state.gesture = ThumbWheelGestureState::default();
        }
        self.thumb_wheel_cv.notify_all();
    }

    fn note_thumb_wheel_pointer_move(&self, event: &CGEvent) {
        let delta_x = event.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_X);
        let delta_y = event.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y);
        if delta_x == 0 && delta_y == 0 {
            return;
        }

        let mut states = self.thumb_wheel_states.lock().unwrap();
        for state in states.values_mut() {
            if !state.gesture.active {
                continue;
            }
            state.gesture.end_requested = true;
            state.gesture.last_location = event.location();
            state.gesture.last_flags = event.get_flags();
        }
        self.thumb_wheel_cv.notify_all();
    }

    fn thumb_wheel_worker_state(&self, now: Instant) -> ThumbWheelWorkerState {
        let config = self.current_config();
        let mut states = self.thumb_wheel_states.lock().unwrap();
        let mut wait_timeout: Option<Duration> = None;

        for route in config
            .routes
            .iter()
            .filter(|route| route.thumb_wheel_trackpad_requested())
        {
            let Some(state) = states.get_mut(&route.managed_device_key) else {
                continue;
            };
            let hold_timeout = Duration::from_millis(u64::from(
                route
                    .device_settings
                    .macos_thumb_wheel_trackpad_hold_timeout_ms,
            ));
            match thumb_wheel_worker_state_for_state(&mut state.gesture, hold_timeout, now) {
                ThumbWheelWorkerState::Action(location, flags, phase, deltas) => {
                    return ThumbWheelWorkerState::Action(location, flags, phase, deltas);
                }
                ThumbWheelWorkerState::Wait => {}
                ThumbWheelWorkerState::WaitTimeout(duration) => {
                    wait_timeout =
                        Some(wait_timeout.map_or(duration, |current| current.min(duration)));
                }
            }
        }

        wait_timeout
            .map(ThumbWheelWorkerState::WaitTimeout)
            .unwrap_or(ThumbWheelWorkerState::Wait)
    }

    fn pending_thumb_wheel_hid_summary(&self, now: Instant) -> Option<String> {
        let window = Duration::from_millis(THUMB_WHEEL_MATCH_WINDOW_MS);
        let mut states = self.thumb_wheel_states.lock().unwrap();
        let summaries = states
            .iter_mut()
            .filter_map(|(route_key, state)| {
                prune_thumb_wheel_hid_samples(&mut state.hid_samples, now, window);
                if state.hid_samples.is_empty() {
                    return None;
                }

                let directions = state
                    .hid_samples
                    .iter()
                    .map(|sample| match sample.direction {
                        std::cmp::Ordering::Less => "left",
                        std::cmp::Ordering::Greater => "right",
                        std::cmp::Ordering::Equal => "neutral",
                    })
                    .collect::<Vec<_>>()
                    .join(",");

                Some(format!(
                    "{}:count={} dirs={}",
                    route_key,
                    state.hid_samples.len(),
                    directions
                ))
            })
            .collect::<Vec<_>>();

        (!summaries.is_empty()).then(|| summaries.join(" | "))
    }

    fn mark_gesture_connected(&self, connected: bool, message: Option<String>) {
        self.gesture_connected.store(connected, Ordering::SeqCst);
        if let Some(message) = message {
            self.push_status(DebugLogGroup::Gestures, DebugEventKind::Info, message);
        }
    }

    fn handle_event(&self, event_type: CGEventType, event: &CGEvent) -> CallbackResult {
        match event_type {
            CGEventType::TapDisabledByTimeout => {
                self.push_status(
                    DebugLogGroup::HookRouting,
                    DebugEventKind::Warning,
                    "CGEventTap disabled by timeout; macOS stopped dispatching live remap events.",
                );
                CallbackResult::Keep
            }
            CGEventType::TapDisabledByUserInput => {
                self.push_status(
                    DebugLogGroup::HookRouting,
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
        let Some(control) = control_for_button(button) else {
            return CallbackResult::Keep;
        };

        let config = self.current_config();
        let Some(route) = config.unique_route_for_control(control) else {
            return CallbackResult::Keep;
        };

        if is_down {
            self.dispatch_route_control_action(&route, control);
        }

        CallbackResult::Drop
    }

    fn handle_scroll_event(&self, event: &CGEvent) -> CallbackResult {
        match event.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA) {
            SCROLL_INVERT_MARKER | THUMB_WHEEL_TRACKPAD_MARKER => {
                return CallbackResult::Keep;
            }
            _ => {}
        }

        let config = self.current_config();
        let now = Instant::now();
        let pending_hid_summary = self.pending_thumb_wheel_hid_summary(now);
        let Some((route, deltas)) = self.match_thumb_wheel_route(config.as_ref(), event) else {
            if let Some(pending_hid_summary) = pending_hid_summary {
                let vertical = read_scroll_axis_deltas(
                    event,
                    EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1,
                    EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1,
                    EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1,
                );
                let horizontal = read_scroll_axis_deltas(
                    event,
                    EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2,
                    EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_2,
                    EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2,
                );
                self.push_thumb_wheel_debug(format!(
                    "Unmatched scroll event continuous={} vertical(line={} fixed={} point={}) horizontal(line={} fixed={} point={}) pending_hid={}",
                    event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_IS_CONTINUOUS),
                    vertical.line,
                    vertical.fixed,
                    vertical.point,
                    horizontal.line,
                    horizontal.fixed,
                    horizontal.point,
                    pending_hid_summary
                ));
            }

            if event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_IS_CONTINUOUS) == 0 {
                let vertical = read_scroll_axis_deltas(
                    event,
                    EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1,
                    EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1,
                    EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1,
                );
                if !vertical.is_zero()
                    && config.unique_vertical_inversion_route().is_some()
                    && post_inverted_scroll_event(event, true, false).is_ok()
                {
                    return CallbackResult::Drop;
                }
            }
            return CallbackResult::Keep;
        };
        self.push_thumb_wheel_debug(format!(
            "Matched thumb wheel scroll [{}] line={} fixed={} point={}",
            route.managed_device_key, deltas.line, deltas.fixed, deltas.point
        ));

        if route.thumb_wheel_trackpad_requested() {
            self.enqueue_thumb_wheel_trackpad_scroll(
                &route,
                deltas,
                event.location(),
                event.get_flags(),
            );
            return CallbackResult::Drop;
        }

        if let Some(control) = horizontal_scroll_control(preferred_scroll_delta(deltas) as i32) {
            if route.handles_control(control) {
                self.dispatch_route_control_action(&route, control);
                return CallbackResult::Drop;
            }
        }

        if route.device_settings.invert_horizontal_scroll
            && post_inverted_scroll_event(event, false, true).is_ok()
        {
            return CallbackResult::Drop;
        }

        CallbackResult::Keep
    }

    fn handle_motion_event(&self, event: &CGEvent) -> CallbackResult {
        self.note_thumb_wheel_pointer_move(event);
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
            self.push_status(
                DebugLogGroup::HookRouting,
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
    thumb_wheel_stop: Arc<AtomicBool>,
    thumb_wheel_input_worker: Mutex<Option<JoinHandle<()>>>,
    thumb_wheel_trackpad_worker: Mutex<Option<JoinHandle<()>>>,
}

#[cfg(target_os = "macos")]
impl MacOsHookBackend {
    pub fn new() -> Self {
        let shared = Arc::new(MacOsHookShared::new());
        let run_loop = Arc::new(Mutex::new(None));
        let (startup_tx, startup_rx) = mpsc::channel::<Result<(), ()>>();
        let startup_signal = Arc::new(Mutex::new(Some(startup_tx)));
        let gesture_stop = Arc::new(AtomicBool::new(false));
        let thumb_wheel_stop = Arc::new(AtomicBool::new(false));

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
                        worker_shared_for_run.push_status(
                            DebugLogGroup::HookRouting,
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
                    worker_shared_for_panic.push_status(
                        DebugLogGroup::HookRouting,
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
                shared.push_status(
                    DebugLogGroup::HookRouting,
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

        let thumb_wheel_input_shared = Arc::clone(&shared);
        let thumb_wheel_input_stop = Arc::clone(&thumb_wheel_stop);
        let thumb_wheel_input_worker = thread::Builder::new()
            .name("mouser-macos-thumb-wheel".to_string())
            .spawn(move || {
                run_thumb_wheel_input_worker(thumb_wheel_input_shared, thumb_wheel_input_stop)
            })
            .ok();

        let thumb_wheel_trackpad_shared = Arc::clone(&shared);
        let thumb_wheel_trackpad_stop = Arc::clone(&thumb_wheel_stop);
        let thumb_wheel_trackpad_worker = thread::Builder::new()
            .name("mouser-macos-thumb-wheel-trackpad".to_string())
            .spawn(move || {
                run_thumb_wheel_trackpad_worker(
                    thumb_wheel_trackpad_shared,
                    thumb_wheel_trackpad_stop,
                )
            })
            .ok();

        Self {
            shared,
            run_loop,
            worker: Mutex::new(handle),
            gesture_stop,
            gesture_worker: Mutex::new(gesture_worker),
            thumb_wheel_stop,
            thumb_wheel_input_worker: Mutex::new(thumb_wheel_input_worker),
            thumb_wheel_trackpad_worker: Mutex::new(thumb_wheel_trackpad_worker),
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

        self.thumb_wheel_stop.store(true, Ordering::SeqCst);
        self.shared.thumb_wheel_cv.notify_all();
        if let Some(handle) = self.thumb_wheel_input_worker.lock().unwrap().take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.thumb_wheel_trackpad_worker.lock().unwrap().take() {
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
                self.route.device_settings.gesture_timeout_ms, route_key,
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
        self.routes
            .iter()
            .any(|route| route.control == LogicalControl::GesturePress && route.cids.contains(&cid))
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
struct ThumbWheelSession {
    route: MacOsDeviceRoute,
    info: MacOsIoKitInfo,
    device: MacOsNativeHidDevice,
}

#[cfg(target_os = "macos")]
impl ThumbWheelSession {
    fn matches_route(&self, route: &MacOsDeviceRoute) -> bool {
        &self.route == route
    }

    fn route_key(&self) -> &str {
        &self.route.managed_device_key
    }

    fn product_label(&self) -> String {
        self.info
            .product_string
            .clone()
            .unwrap_or_else(|| format!("PID 0x{:04X}", self.info.product_id))
    }

    fn handle_value_event(&mut self, shared: &MacOsHookShared, event: MacOsInputValueEvent) {
        let usage_page = event.usage_page;
        let usage = event.usage;
        let value = event.value;
        let Some(event) = thumb_wheel_input_value(&self.info, event) else {
            if value != 0
                && matches!(
                    usage_page,
                    THUMB_WHEEL_USAGE_PAGE_CONSUMER | THUMB_WHEEL_USAGE_PAGE_GENERIC_DESKTOP
                )
            {
                shared.push_hid_debug_rate_limited(
                    format!("{}:{usage_page:04x}:{usage:04x}", self.route_key()),
                    format!(
                        "Ignored HID value={} usage=0x{:04X}/0x{:04X} [{}]",
                        value,
                        usage_page,
                        usage,
                        self.route_key()
                    ),
                );
            }
            return;
        };
        shared.push_thumb_wheel_debug(format!(
            "Thumb wheel HID delta value={} usage=0x{:04X}/0x{:04X} [{}]",
            event.value,
            event.usage_page,
            event.usage,
            self.route_key()
        ));
        shared.note_thumb_wheel_hid_event(self.route_key(), event);
    }
}

#[cfg(target_os = "macos")]
fn run_thumb_wheel_input_worker(shared: Arc<MacOsHookShared>, stop: Arc<AtomicBool>) {
    let mut sessions = BTreeMap::<String, ThumbWheelSession>::new();
    let mut last_idle_summary = None::<String>;
    let mut last_global_warning = None::<String>;
    let mut last_route_failures = BTreeMap::<String, String>::new();

    while !stop.load(Ordering::SeqCst) {
        let config = shared.current_config();
        let desired_routes = if config.enabled {
            config
                .routes
                .iter()
                .filter(|route| route.thumb_wheel_hid_requested())
                .cloned()
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        if desired_routes.is_empty() {
            last_global_warning = None;
            last_route_failures.clear();
            if config.debug_mode {
                let idle_summary = describe_thumb_wheel_route_diagnostics(&config.routes);
                if last_idle_summary.as_deref() != Some(idle_summary.as_str()) {
                    let message = format!(
                        "Thumb wheel HID listener idle: no routes request thumb-wheel handling ({idle_summary})"
                    );
                    shared.log_console(DebugLogGroup::ThumbWheel, DebugEventKind::Info, message);
                    last_idle_summary = Some(idle_summary);
                }
            } else {
                last_idle_summary = None;
            }
            let removed_keys = sessions.keys().cloned().collect::<Vec<_>>();
            for route_key in removed_keys {
                let _ = sessions.remove(&route_key);
                shared.reset_thumb_wheel_state(&route_key);
            }
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
            let _ = sessions.remove(&key);
            shared.reset_thumb_wheel_state(&key);
            let _ = last_route_failures.remove(&key);
        }
        let removed_keys = sessions
            .keys()
            .filter(|key| !desired_by_key.contains_key(key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        for key in removed_keys {
            let _ = sessions.remove(&key);
            shared.reset_thumb_wheel_state(&key);
            let _ = last_route_failures.remove(&key);
        }
        last_route_failures.retain(|key, _| desired_by_key.contains_key(key.as_str()));

        let infos = match enumerate_iokit_infos() {
            Ok(infos) => {
                last_global_warning = None;
                infos
            }
            Err(error) => {
                let message = format!("Thumb wheel HID listener unavailable: {error}");
                if last_global_warning.as_deref() != Some(message.as_str()) {
                    shared.log_console(
                        DebugLogGroup::ThumbWheel,
                        DebugEventKind::Warning,
                        &message,
                    );
                    shared.push_event(DebugEventKind::Warning, message.clone());
                    last_global_warning = Some(message);
                }
                thread::sleep(Duration::from_millis(900));
                continue;
            }
        };
        last_idle_summary = None;

        for route in desired_routes {
            if sessions.contains_key(&route.managed_device_key) {
                let _ = last_route_failures.remove(&route.managed_device_key);
                continue;
            }
            match try_build_thumb_wheel_session_for_route(&shared, &route, &infos) {
                Ok(session) => {
                    let _ = last_route_failures.remove(&route.managed_device_key);
                    let message = format!(
                        "Thumb wheel HID listener attached to {} for {}",
                        session.product_label(),
                        route.managed_device_key
                    );
                    shared.log_console(DebugLogGroup::ThumbWheel, DebugEventKind::Info, message);
                    sessions.insert(route.managed_device_key.clone(), session);
                }
                Err(error) => {
                    let message = format!(
                        "Thumb wheel HID listener unavailable for {}: {error}",
                        route.managed_device_key
                    );
                    if last_route_failures
                        .get(&route.managed_device_key)
                        .map(String::as_str)
                        != Some(message.as_str())
                    {
                        shared.log_console(
                            DebugLogGroup::ThumbWheel,
                            DebugEventKind::Warning,
                            &message,
                        );
                        shared.push_event(DebugEventKind::Warning, message.clone());
                        last_route_failures.insert(route.managed_device_key.clone(), message);
                    }
                }
            }
        }

        let mut disconnected = Vec::new();
        for (route_key, session) in sessions.iter_mut() {
            match session.device.read_value_timeout(12) {
                Ok(Some(event)) => session.handle_value_event(&shared, event),
                Ok(None) => {}
                Err(error) => {
                    let message = format!(
                        "Thumb wheel HID listener lost device stream for {}: {error}",
                        route_key
                    );
                    shared.log_console(
                        DebugLogGroup::ThumbWheel,
                        DebugEventKind::Warning,
                        &message,
                    );
                    shared.push_event(DebugEventKind::Warning, message);
                    disconnected.push(route_key.clone());
                }
            }
        }
        for route_key in disconnected {
            let _ = sessions.remove(&route_key);
            shared.reset_thumb_wheel_state(&route_key);
            let _ = last_route_failures.remove(&route_key);
        }
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
                    shared.log_console(
                        DebugLogGroup::Gestures,
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
fn run_thumb_wheel_trackpad_worker(shared: Arc<MacOsHookShared>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        match shared.thumb_wheel_worker_state(Instant::now()) {
            ThumbWheelWorkerState::Action(location, flags, phase, deltas) => {
                if let Err(error) = post_thumb_wheel_trackpad_event(location, flags, phase, deltas)
                {
                    shared.push_event(
                        DebugEventKind::Warning,
                        format!("Thumb wheel trackpad swipe delivery failed: {error}"),
                    );
                }
                thread::sleep(Duration::from_millis(
                    SCROLL_WHEEL_TRACKPAD_POLL_INTERVAL_MS,
                ));
            }
            ThumbWheelWorkerState::Wait => {
                let guard = shared.thumb_wheel_states.lock().unwrap();
                let _guard = shared.thumb_wheel_cv.wait(guard).unwrap();
            }
            ThumbWheelWorkerState::WaitTimeout(duration) => {
                let guard = shared.thumb_wheel_states.lock().unwrap();
                let _ = shared.thumb_wheel_cv.wait_timeout(guard, duration).unwrap();
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn try_build_thumb_wheel_session_for_route(
    shared: &MacOsHookShared,
    route: &MacOsDeviceRoute,
    infos: &[MacOsIoKitInfo],
) -> Result<ThumbWheelSession, PlatformError> {
    let matched_infos = infos
        .iter()
        .filter(|info| iokit_info_matches_route(info, route))
        .cloned()
        .collect::<Vec<_>>();
    let candidates = matched_infos
        .iter()
        .filter(|info| thumb_wheel_iokit_candidate(info))
        .cloned()
        .collect::<Vec<_>>();
    let mut open_failures = Vec::new();

    for info in &candidates {
        for candidate in iokit_open_candidates(info) {
            match MacOsNativeHidDevice::open(&candidate) {
                Ok(device) => {
                    return Ok(ThumbWheelSession {
                        route: route.clone(),
                        info: info.clone(),
                        device,
                    });
                }
                Err(error) => {
                    open_failures.push(format!("{} -> {}", describe_iokit_info(&candidate), error));
                }
            }
        }
    }

    let route_identity =
        normalized_identity_key(route.live_device.fingerprint.identity_key.as_deref())
            .unwrap_or("<none>");
    let inventory = describe_iokit_info_list(infos);
    let matched = describe_iokit_info_list(&matched_infos);
    let candidate_summary = describe_iokit_info_list(&candidates);
    let failure_summary = describe_diagnostic_list(&open_failures);
    shared.push_thumb_wheel_debug(format!(
        "Thumb wheel HID attach failed for {} model={} identity={}: matched={} candidates={} open_failures={} inventory={}",
        route.managed_device_key,
        route.live_device.model_key,
        route_identity,
        matched,
        candidate_summary,
        failure_summary,
        inventory
    ));

    Err(PlatformError::Message(format!(
        "could not initialize thumb wheel HID session for {} (model={} identity={} matched={} candidates={} open_failures={} inventory={})",
        route.managed_device_key,
        route.live_device.model_key,
        route_identity,
        matched,
        candidate_summary,
        failure_summary,
        inventory
    )))
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
    {
        for candidate in iokit_open_candidates(info) {
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
            if set_gesture_reporting(device, dev_idx, feature_idx, *cid, flags, 250)?.is_some() {
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
        return normalized_identity_key(fingerprint_from_iokit_info(info).identity_key.as_deref())
            == Some(identity_key);
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
fn describe_iokit_info(info: &MacOsIoKitInfo) -> String {
    let mut parts = vec![
        format!("pid=0x{:04X}", info.product_id),
        format!("up=0x{:04X}", info.usage_page),
        format!("usage=0x{:04X}", info.usage),
    ];
    if let Some(transport) = info.transport.as_deref() {
        parts.push(format!("transport=\"{transport}\""));
    }
    if let Some(product) = info.product_string.as_deref() {
        parts.push(format!("product=\"{product}\""));
    }
    if let Some(serial) = info.serial_number.as_deref() {
        parts.push(format!("serial=\"{serial}\""));
    }
    if let Some(location_id) = info.location_id {
        parts.push(format!("location=0x{location_id:08X}"));
    }
    if let Some(identity_key) = fingerprint_from_iokit_info(info).identity_key {
        parts.push(format!("identity=\"{identity_key}\""));
    }
    parts.join(" ")
}

#[cfg(target_os = "macos")]
fn describe_iokit_info_list(infos: &[MacOsIoKitInfo]) -> String {
    if infos.is_empty() {
        "<none>".to_string()
    } else {
        infos
            .iter()
            .map(describe_iokit_info)
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

#[cfg(target_os = "macos")]
fn describe_diagnostic_list(items: &[String]) -> String {
    if items.is_empty() {
        "<none>".to_string()
    } else {
        items.join(" | ")
    }
}

#[cfg(target_os = "macos")]
fn describe_thumb_wheel_route_diagnostics(routes: &[MacOsDeviceRoute]) -> String {
    if routes.is_empty() {
        return "no configured routes".to_string();
    }

    routes
        .iter()
        .map(|route| {
            format!(
                "{}: trackpad_sim={} invert_horizontal_scroll={} hscroll_left={} hscroll_right={}",
                route.managed_device_key,
                route.device_settings.macos_thumb_wheel_simulate_trackpad,
                route.device_settings.invert_horizontal_scroll,
                route
                    .action_for(LogicalControl::HscrollLeft)
                    .unwrap_or("none"),
                route
                    .action_for(LogicalControl::HscrollRight)
                    .unwrap_or("none"),
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(target_os = "macos")]
fn thumb_wheel_iokit_candidate(info: &MacOsIoKitInfo) -> bool {
    (info.usage_page == 0x0001 && info.usage == 0x0002)
        || (info.usage_page == 0 && info.usage == 0)
        || info.transport.as_deref() == Some("Bluetooth Low Energy")
}

#[cfg(target_os = "macos")]
fn thumb_wheel_input_value(
    info: &MacOsIoKitInfo,
    event: MacOsInputValueEvent,
) -> Option<MacOsInputValueEvent> {
    let _ = info;
    ((event.usage_page == THUMB_WHEEL_USAGE_PAGE_CONSUMER
        && event.usage == THUMB_WHEEL_USAGE_AC_PAN)
        && event.value != 0)
        .then_some(event)
}

#[cfg(target_os = "macos")]
fn prune_thumb_wheel_hid_samples(
    samples: &mut VecDeque<ThumbWheelHidSample>,
    now: Instant,
    window: Duration,
) {
    while samples
        .front()
        .is_some_and(|sample| now.duration_since(sample.observed_at) > window)
    {
        let _ = samples.pop_front();
    }
}

#[cfg(target_os = "macos")]
fn preferred_scroll_delta(deltas: ScrollAxisDeltas) -> i64 {
    if deltas.fixed != 0 {
        deltas.fixed
    } else if deltas.point != 0 {
        deltas.point
    } else {
        deltas.line
    }
}

#[cfg(target_os = "macos")]
fn control_for_button(button: i64) -> Option<LogicalControl> {
    match button {
        BTN_MIDDLE => Some(LogicalControl::Middle),
        BTN_BACK => Some(LogicalControl::Back),
        BTN_FORWARD => Some(LogicalControl::Forward),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn post_inverted_scroll_event(
    event: &CGEvent,
    invert_vertical: bool,
    invert_horizontal: bool,
) -> Result<(), PlatformError> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| PlatformError::Message("failed to create CGEventSource".to_string()))?;

    let vertical_point =
        event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1) as i32;
    let horizontal_point =
        event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2) as i32;

    let inverted = CGEvent::new_scroll_event(
        source,
        ScrollEventUnit::PIXEL,
        2,
        if invert_vertical {
            -vertical_point
        } else {
            vertical_point
        },
        if invert_horizontal {
            -horizontal_point
        } else {
            horizontal_point
        },
        0,
    )
    .map_err(|_| PlatformError::Message("failed to create inverted scroll event".to_string()))?;

    inverted.set_flags(event.get_flags());
    inverted.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SCROLL_INVERT_MARKER);

    copy_scroll_axis(
        event,
        &inverted,
        EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1,
        invert_vertical,
    );
    copy_scroll_axis(
        event,
        &inverted,
        EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2,
        invert_horizontal,
    );
    copy_scroll_axis(
        event,
        &inverted,
        EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1,
        invert_vertical,
    );
    copy_scroll_axis(
        event,
        &inverted,
        EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_2,
        invert_horizontal,
    );
    copy_scroll_axis(
        event,
        &inverted,
        EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1,
        invert_vertical,
    );
    copy_scroll_axis(
        event,
        &inverted,
        EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2,
        invert_horizontal,
    );

    inverted.post(CGEventTapLocation::HID);
    Ok(())
}

#[cfg(target_os = "macos")]
fn copy_scroll_axis(source: &CGEvent, target: &CGEvent, field: u32, invert: bool) {
    let value = source.get_integer_value_field(field);
    if value != 0 {
        target.set_integer_value_field(field, if invert { -value } else { value });
    }
}

#[cfg(target_os = "macos")]
fn extract_thumb_wheel_scroll_deltas(event: &CGEvent) -> Option<ScrollAxisDeltas> {
    if event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_IS_CONTINUOUS) != 0 {
        return None;
    }

    let vertical = read_scroll_axis_deltas(
        event,
        EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1,
        EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1,
        EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1,
    );
    if !vertical.is_zero() {
        return None;
    }

    let mut horizontal = read_scroll_axis_deltas(
        event,
        EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2,
        EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_2,
        EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2,
    );
    if horizontal.is_zero() {
        return None;
    }

    if horizontal.point == 0 {
        horizontal.point = if horizontal.line != 0 {
            horizontal.line
        } else {
            approximate_point_delta_from_fixed(horizontal.fixed)
        };
    }
    if horizontal.line == 0 {
        horizontal.line = horizontal.point.signum();
    }
    if horizontal.fixed == 0 {
        horizontal.fixed = horizontal.point;
    }

    Some(horizontal)
}

#[cfg(target_os = "macos")]
fn read_scroll_axis_deltas(
    event: &CGEvent,
    line_field: u32,
    fixed_field: u32,
    point_field: u32,
) -> ScrollAxisDeltas {
    ScrollAxisDeltas {
        line: event.get_integer_value_field(line_field),
        fixed: event.get_integer_value_field(fixed_field),
        point: event.get_integer_value_field(point_field),
    }
}

#[cfg(target_os = "macos")]
fn approximate_point_delta_from_fixed(fixed: i64) -> i64 {
    if fixed == 0 {
        return 0;
    }

    if fixed.abs() >= 1024 {
        let scaled = fixed / 65_536;
        if scaled != 0 {
            return scaled;
        }
    }

    fixed.signum().clamp(-1, 1) * fixed.abs().min(120)
}

#[cfg(target_os = "macos")]
fn next_thumb_wheel_point_step(pending: f64) -> Option<i64> {
    if pending.abs() < 0.5 {
        return None;
    }

    let magnitude = if pending.abs() <= SCROLL_WHEEL_TRACKPAD_MIN_STEP {
        pending.abs()
    } else {
        (pending.abs() * SCROLL_WHEEL_TRACKPAD_SMOOTHING_FACTOR).clamp(
            SCROLL_WHEEL_TRACKPAD_MIN_STEP,
            SCROLL_WHEEL_TRACKPAD_MAX_STEP,
        )
    };
    let step = pending.signum() * magnitude;
    Some(step.round() as i64)
}

#[cfg(target_os = "macos")]
fn post_thumb_wheel_trackpad_event(
    location: CGPoint,
    flags: CGEventFlags,
    phase: ThumbWheelTrackpadPhase,
    horizontal: ScrollAxisDeltas,
) -> Result<(), PlatformError> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| PlatformError::Message("failed to create CGEventSource".to_string()))?;

    let event = CGEvent::new_scroll_event(
        source,
        ScrollEventUnit::PIXEL,
        2,
        0,
        clamp_i64_to_i32(horizontal.point),
        0,
    )
    .map_err(|_| {
        PlatformError::Message("failed to create thumb wheel trackpad event".to_string())
    })?;

    event.set_location(location);
    event.set_flags(flags);
    event.set_integer_value_field(
        EventField::EVENT_SOURCE_USER_DATA,
        THUMB_WHEEL_TRACKPAD_MARKER,
    );
    event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_IS_CONTINUOUS, 1);
    event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1, 0);
    event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2, horizontal.line);
    event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1, 0);
    event.set_integer_value_field(
        EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_2,
        horizontal.fixed,
    );
    event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1, 0);
    event.set_integer_value_field(
        EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2,
        horizontal.point,
    );
    event.set_integer_value_field(
        SCROLL_WHEEL_EVENT_SCROLL_PHASE_FIELD,
        phase.as_scroll_phase(),
    );
    event.set_integer_value_field(SCROLL_WHEEL_EVENT_MOMENTUM_PHASE_FIELD, 0);

    event.post(CGEventTapLocation::Session);
    Ok(())
}

#[cfg(target_os = "macos")]
fn clamp_i64_to_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
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
        "screen_capture" => send_key_combo(&[KeyCode::COMMAND, KeyCode::SHIFT, KeyCode::ANSI_4]),
        "emoji_picker" => send_key_combo(&[KeyCode::CONTROL, KeyCode::COMMAND, KeyCode::SPACE]),
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use mouser_core::{
        default_device_settings, DeviceFingerprint, DeviceSupportLevel, DeviceSupportMatrix,
    };

    fn test_route(
        key: &str,
        bindings: &[(LogicalControl, &str)],
        configure: impl FnOnce(&mut DeviceSettings),
    ) -> MacOsDeviceRoute {
        let mut device_settings = default_device_settings();
        configure(&mut device_settings);

        MacOsDeviceRoute {
            managed_device_key: key.to_string(),
            resolved_profile_id: format!("profile-{key}"),
            live_device: DeviceInfo {
                key: key.to_string(),
                model_key: "mx_master_3".to_string(),
                display_name: "MX Master 3".to_string(),
                nickname: None,
                product_id: Some(0xB023),
                product_name: Some("MX Master 3".to_string()),
                transport: Some("Bluetooth Low Energy".to_string()),
                source: Some("iokit".to_string()),
                ui_layout: "mx_master_3".to_string(),
                image_asset: "mx_master_3.png".to_string(),
                supported_controls: bindings.iter().map(|(control, _)| *control).collect(),
                controls: Vec::new(),
                support: DeviceSupportMatrix {
                    level: DeviceSupportLevel::Full,
                    supports_battery_status: true,
                    supports_dpi_configuration: true,
                    has_interactive_layout: true,
                    notes: Vec::new(),
                },
                gesture_cids: Vec::new(),
                dpi_min: 200,
                dpi_max: 8000,
                dpi_inferred: false,
                dpi_source_kind: None,
                connected: true,
                battery: None,
                battery_level: None,
                current_dpi: 1000,
                fingerprint: DeviceFingerprint::default(),
            },
            device_settings,
            bindings: bindings
                .iter()
                .map(|(control, action_id)| (*control, (*action_id).to_string()))
                .collect(),
            device_controls: Vec::new(),
        }
    }

    fn test_config(routes: Vec<MacOsDeviceRoute>) -> MacOsHookConfig {
        MacOsHookConfig {
            enabled: true,
            debug_mode: false,
            debug_log_groups: DebugLogGroups::default(),
            routes,
        }
    }

    fn test_ble_info() -> MacOsIoKitInfo {
        MacOsIoKitInfo {
            product_id: 0xB023,
            usage_page: 0x0001,
            usage: 0x0006,
            transport: Some("Bluetooth Low Energy".to_string()),
            product_string: Some("MX Master 3 Mac".to_string()),
            serial_number: Some("DA8CDEBAE58BC195".to_string()),
            location_id: Some(0x63BFF755),
        }
    }

    #[test]
    fn thumb_wheel_input_filter_accepts_consumer_ac_pan() {
        let event = MacOsInputValueEvent {
            usage_page: THUMB_WHEEL_USAGE_PAGE_CONSUMER,
            usage: THUMB_WHEEL_USAGE_AC_PAN,
            value: -1,
            observed_at: Instant::now(),
        };

        assert!(thumb_wheel_input_value(&test_ble_info(), event).is_some());
    }

    #[test]
    fn thumb_wheel_input_filter_rejects_ble_generic_desktop_x() {
        let event = MacOsInputValueEvent {
            usage_page: THUMB_WHEEL_USAGE_PAGE_GENERIC_DESKTOP,
            usage: 0x0030,
            value: -29,
            observed_at: Instant::now(),
        };

        assert!(thumb_wheel_input_value(&test_ble_info(), event).is_none());
    }

    #[test]
    fn thumb_wheel_input_filter_rejects_unrelated_hid_values() {
        let event = MacOsInputValueEvent {
            usage_page: THUMB_WHEEL_USAGE_PAGE_GENERIC_DESKTOP,
            usage: 0x0038,
            value: 1,
            observed_at: Instant::now(),
        };

        assert!(thumb_wheel_input_value(&test_ble_info(), event).is_none());
    }

    #[test]
    fn thumb_wheel_candidate_accepts_ble_route_interface() {
        assert!(thumb_wheel_iokit_candidate(&test_ble_info()));
    }

    #[test]
    fn control_for_button_maps_supported_buttons() {
        assert_eq!(control_for_button(BTN_MIDDLE), Some(LogicalControl::Middle));
        assert_eq!(control_for_button(BTN_BACK), Some(LogicalControl::Back));
        assert_eq!(
            control_for_button(BTN_FORWARD),
            Some(LogicalControl::Forward)
        );
        assert_eq!(control_for_button(99), None);
    }

    #[test]
    fn unique_route_for_control_requires_an_unambiguous_match() {
        let config = test_config(vec![
            test_route(
                "mx_master_3-1",
                &[(LogicalControl::Back, "mission_control")],
                |_| {},
            ),
            test_route(
                "mx_master_3-2",
                &[(LogicalControl::Middle, "launchpad")],
                |_| {},
            ),
        ]);

        let route = config.unique_route_for_control(LogicalControl::Back);
        assert_eq!(
            route
                .as_ref()
                .map(|route| route.managed_device_key.as_str()),
            Some("mx_master_3-1")
        );

        let ambiguous = test_config(vec![
            test_route(
                "mx_master_3-1",
                &[(LogicalControl::Back, "mission_control")],
                |_| {},
            ),
            test_route(
                "mx_master_3-2",
                &[(LogicalControl::Back, "launchpad")],
                |_| {},
            ),
        ]);
        assert!(ambiguous
            .unique_route_for_control(LogicalControl::Back)
            .is_none());
    }

    #[test]
    fn unique_vertical_inversion_route_requires_a_single_route() {
        let config = test_config(vec![
            test_route("mx_master_3-1", &[], |settings| {
                settings.invert_vertical_scroll = true;
            }),
            test_route("mx_master_3-2", &[], |_| {}),
        ]);
        assert_eq!(
            config
                .unique_vertical_inversion_route()
                .as_ref()
                .map(|route| route.managed_device_key.as_str()),
            Some("mx_master_3-1")
        );

        let ambiguous = test_config(vec![
            test_route("mx_master_3-1", &[], |settings| {
                settings.invert_vertical_scroll = true;
            }),
            test_route("mx_master_3-2", &[], |settings| {
                settings.invert_vertical_scroll = true;
            }),
        ]);
        assert!(ambiguous.unique_vertical_inversion_route().is_none());
    }

    #[test]
    fn thumb_wheel_worker_blocks_while_idle() {
        let mut state = ThumbWheelGestureState::default();
        assert!(matches!(
            thumb_wheel_worker_state_for_state(
                &mut state,
                Duration::from_millis(500),
                Instant::now(),
            ),
            ThumbWheelWorkerState::Wait
        ));
    }

    #[test]
    fn thumb_wheel_worker_emits_motion_when_pending_delta_exists() {
        let now = Instant::now();
        let mut state = ThumbWheelGestureState {
            active: true,
            pending_point_delta: 6.0,
            last_wheel_at: Some(now),
            last_location: CGPoint { x: 10.0, y: 20.0 },
            last_flags: CGEventFlags::empty(),
            ..ThumbWheelGestureState::default()
        };

        match thumb_wheel_worker_state_for_state(&mut state, Duration::from_millis(500), now) {
            ThumbWheelWorkerState::Action(_, _, ThumbWheelTrackpadPhase::Began, deltas) => {
                assert!(deltas.point > 0);
                assert!(state.emitted_motion);
            }
            _ => panic!("expected thumb-wheel worker to emit a began action"),
        }
    }

    #[test]
    fn thumb_wheel_worker_ends_after_hold_timeout() {
        let now = Instant::now();
        let mut state = ThumbWheelGestureState {
            active: true,
            emitted_motion: true,
            last_wheel_at: Some(now - Duration::from_millis(600)),
            last_location: CGPoint { x: 0.0, y: 0.0 },
            last_flags: CGEventFlags::empty(),
            ..ThumbWheelGestureState::default()
        };

        match thumb_wheel_worker_state_for_state(&mut state, Duration::from_millis(500), now) {
            ThumbWheelWorkerState::Action(_, _, ThumbWheelTrackpadPhase::Ended, deltas) => {
                assert!(deltas.is_zero());
                assert!(!state.active);
            }
            _ => panic!("expected thumb-wheel worker to end the simulated swipe"),
        }
    }
}
