#[cfg(target_os = "macos")]
use crate::macos_iokit::{enumerate_iokit_infos, MacOsIoKitInfo, MacOsNativeHidDevice};
#[cfg(target_os = "macos")]
use crate::{horizontal_scroll_control, push_bounded_hook_event};
use crate::{HookBackend, HookBackendEvent, HookCapabilities, PlatformError};
#[cfg(target_os = "macos")]
use mouser_core::{
    build_connected_device_info, hydrate_identity_key, Binding, DebugEventKind, DeviceFingerprint,
    LogicalControl,
};
use mouser_core::{Profile, Settings};

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
        _settings: &Settings,
        _profile: &Profile,
        _enabled: bool,
    ) -> Result<(), PlatformError> {
        Ok(())
    }

    fn drain_events(&self) -> Vec<HookBackendEvent> {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
use std::{
    collections::BTreeMap,
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
        CGEventTapPlacement, CGEventType, CallbackResult, EventField, KeyCode, ScrollEventUnit,
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
const BTN_MIDDLE: i64 = 2;
#[cfg(target_os = "macos")]
const BTN_BACK: i64 = 3;
#[cfg(target_os = "macos")]
const BTN_FORWARD: i64 = 4;
#[cfg(target_os = "macos")]
const SCROLL_INVERT_MARKER: i64 = 0x4D4F5553;
#[cfg(target_os = "macos")]
const LONG_ID: u8 = 0x11;
#[cfg(target_os = "macos")]
const LONG_LEN: usize = 20;
#[cfg(target_os = "macos")]
const MY_SW: u8 = 0x0A;
#[cfg(target_os = "macos")]
const FEAT_REPROG_V4: u16 = 0x1B04;
#[cfg(target_os = "macos")]
const DEVICE_INDICES: [u8; 7] = [0xFF, 1, 2, 3, 4, 5, 6];
#[cfg(target_os = "macos")]
const DEFAULT_GESTURE_CIDS: [u16; 2] = [0x00C3, 0x00D7];
#[cfg(target_os = "macos")]
const GESTURE_DIVERT_FLAGS: u8 = 0x03;
#[cfg(target_os = "macos")]
const GESTURE_RAWXY_FLAGS: u8 = 0x33;
#[cfg(target_os = "macos")]
const GESTURE_UNDIVERT_FLAGS: u8 = 0x02;
#[cfg(target_os = "macos")]
const GESTURE_UNDIVERT_RAWXY_FLAGS: u8 = 0x22;
#[cfg(target_os = "macos")]
type HidppMessage = (u8, u8, u8, u8, Vec<u8>);

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
    profile_id: String,
    enabled: bool,
    debug_mode: bool,
    invert_vertical_scroll: bool,
    invert_horizontal_scroll: bool,
    gesture_threshold: u16,
    gesture_deadzone: u16,
    gesture_timeout_ms: u32,
    gesture_cooldown_ms: u32,
    bindings: BTreeMap<LogicalControl, String>,
}

#[cfg(target_os = "macos")]
impl MacOsHookConfig {
    fn from_runtime(settings: &Settings, profile: &Profile, enabled: bool) -> Self {
        Self {
            profile_id: profile.id.clone(),
            enabled,
            debug_mode: settings.debug_mode,
            invert_vertical_scroll: settings.invert_vertical_scroll,
            invert_horizontal_scroll: settings.invert_horizontal_scroll,
            gesture_threshold: settings.gesture_threshold,
            gesture_deadzone: settings.gesture_deadzone,
            gesture_timeout_ms: settings.gesture_timeout_ms,
            gesture_cooldown_ms: settings.gesture_cooldown_ms,
            bindings: bindings_map(&profile.bindings),
        }
    }

    fn action_for(&self, control: LogicalControl) -> Option<&str> {
        self.bindings
            .get(&control)
            .map(String::as_str)
            .filter(|action_id| *action_id != "none")
    }

    fn summary(&self) -> String {
        [
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
        .join(", ")
    }

    fn handles_control(&self, control: LogicalControl) -> bool {
        self.enabled && self.action_for(control).is_some()
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
        self.enabled
            && [
                LogicalControl::GesturePress,
                LogicalControl::GestureLeft,
                LogicalControl::GestureRight,
                LogicalControl::GestureUp,
                LogicalControl::GestureDown,
            ]
            .into_iter()
            .any(|control| self.action_for(control).is_some())
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GestureInputSource {
    EventTap,
    HidRawxy,
}

#[cfg(target_os = "macos")]
impl GestureInputSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::EventTap => "event_tap",
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
    pending_control: Option<LogicalControl>,
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
    gesture_state: Mutex<GestureTrackingState>,
    events: Mutex<Vec<HookBackendEvent>>,
    intercepting: AtomicBool,
    gesture_connected: AtomicBool,
}

#[cfg(target_os = "macos")]
impl MacOsHookShared {
    fn new() -> Self {
        Self {
            config: RwLock::new(Arc::new(MacOsHookConfig::default())),
            gesture_state: Mutex::new(GestureTrackingState::default()),
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

    fn reconfigure(&self, settings: &Settings, profile: &Profile, enabled: bool) {
        let next = Arc::new(MacOsHookConfig::from_runtime(settings, profile, enabled));
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
                format!("Hook profile -> {}", next.profile_id),
            );
            self.push_event(
                DebugEventKind::Info,
                format!("Hook mappings -> {}", next.summary()),
            );
        }

        if !next.gesture_capture_requested() {
            self.reset_gesture_state();
        }
    }

    fn current_config(&self) -> Arc<MacOsHookConfig> {
        Arc::clone(&self.config.read().unwrap())
    }

    fn gesture_capture_requested(&self) -> bool {
        self.current_config().gesture_capture_requested()
    }

    fn mark_gesture_connected(&self, connected: bool, message: Option<String>) {
        self.gesture_connected.store(connected, Ordering::SeqCst);
        if !connected {
            self.reset_gesture_state();
        }
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
                self.handle_gesture_motion_event(event)
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
        if !config.handles_control(control) {
            return CallbackResult::Keep;
        }

        if is_down {
            self.dispatch_control_action(&config, control);
        }

        CallbackResult::Drop
    }

    fn handle_scroll_event(&self, event: &CGEvent) -> CallbackResult {
        if event.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA) == SCROLL_INVERT_MARKER
        {
            return CallbackResult::Keep;
        }

        let config = self.current_config();
        let horizontal_fixed =
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_2);
        if let Some(control) = horizontal_scroll_control(horizontal_fixed as i32) {
            if config.handles_control(control) {
                self.dispatch_control_action(config.as_ref(), control);
                return CallbackResult::Drop;
            }
        }

        if (config.invert_vertical_scroll || config.invert_horizontal_scroll)
            && post_inverted_scroll_event(
                event,
                config.invert_vertical_scroll,
                config.invert_horizontal_scroll,
            )
            .is_ok()
        {
            return CallbackResult::Drop;
        }

        CallbackResult::Keep
    }

    fn handle_gesture_motion_event(&self, event: &CGEvent) -> CallbackResult {
        let config = self.current_config();
        if !(config.enabled && config.gesture_direction_enabled()) {
            return CallbackResult::Keep;
        }

        let delta_x = event.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_X) as f64;
        let delta_y = event.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y) as f64;
        if delta_x == 0.0 && delta_y == 0.0 {
            return CallbackResult::Keep;
        }

        let mut state = self.gesture_state.lock().unwrap();
        if !state.active {
            return CallbackResult::Keep;
        }

        if state.input_source == Some(GestureInputSource::HidRawxy) {
            return CallbackResult::Drop;
        }

        self.accumulate_gesture_delta(
            &config,
            &mut state,
            delta_x,
            delta_y,
            GestureInputSource::EventTap,
        );
        CallbackResult::Drop
    }

    fn hid_gesture_down(&self) {
        let config = self.current_config();
        let mut state = self.gesture_state.lock().unwrap();
        if state.active {
            return;
        }

        state.active = true;
        state.triggered = false;
        state.pending_control = None;
        self.push_gesture_debug("Gesture button down");

        if config.gesture_direction_enabled() && !cooldown_active(&state) {
            start_gesture_tracking(&mut state);
        } else {
            state.tracking = false;
            state.triggered = false;
        }
    }

    fn hid_gesture_up(&self) {
        let config = self.current_config();
        let (should_click, pending_control) = {
            let mut state = self.gesture_state.lock().unwrap();
            if !state.active {
                return;
            }

            let should_click = !state.triggered;
            let pending_control = state.pending_control.take();
            state.active = false;
            finish_gesture_tracking(&mut state);
            state.triggered = false;
            (should_click, pending_control)
        };

        self.push_gesture_debug(format!(
            "Gesture button up click_candidate={}",
            should_click
        ));

        if should_click {
            self.dispatch_control_action(&config, LogicalControl::GesturePress);
        } else if let Some(control) = pending_control {
            self.dispatch_control_action(&config, control);
        }
    }

    fn hid_rawxy_move(&self, delta_x: i16, delta_y: i16) {
        let config = self.current_config();
        if !(config.enabled && config.gesture_direction_enabled()) {
            return;
        }

        let mut state = self.gesture_state.lock().unwrap();
        if !state.active {
            return;
        }

        self.accumulate_gesture_delta(
            &config,
            &mut state,
            f64::from(delta_x),
            f64::from(delta_y),
            GestureInputSource::HidRawxy,
        );
    }

    fn reset_gesture_state(&self) {
        let mut state = self.gesture_state.lock().unwrap();
        *state = GestureTrackingState::default();
    }

    fn dispatch_control_action(&self, config: &MacOsHookConfig, control: LogicalControl) {
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

    fn accumulate_gesture_delta(
        &self,
        config: &MacOsHookConfig,
        state: &mut GestureTrackingState,
        delta_x: f64,
        delta_y: f64,
        source: GestureInputSource,
    ) {
        if !(config.gesture_direction_enabled() && state.active) {
            return;
        }

        if cooldown_active(state) {
            return;
        }

        if !state.tracking {
            self.push_gesture_debug(format!("Gesture tracking started via {}", source.as_str()));
            start_gesture_tracking(state);
        }

        let now = Instant::now();
        let idle_timed_out = state.last_move_at.is_some_and(|last_move_at| {
            now.duration_since(last_move_at).as_millis() > u128::from(config.gesture_timeout_ms)
        });
        if idle_timed_out {
            self.push_gesture_debug(format!(
                "Gesture segment reset after {} ms",
                config.gesture_timeout_ms
            ));
            start_gesture_tracking(state);
        }

        if source == GestureInputSource::HidRawxy
            && state.input_source == Some(GestureInputSource::EventTap)
        {
            self.push_gesture_debug("Gesture source promoted from event_tap to hid_rawxy");
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
            f64::from(config.gesture_threshold),
            f64::from(config.gesture_deadzone),
        ) {
            state.triggered = true;
            state.pending_control = Some(control);
            self.push_gesture_debug(format!(
                "Gesture detected {} source={} dx={} dy={}",
                control.label(),
                source.as_str(),
                state.delta_x as i32,
                state.delta_y as i32,
            ));
            state.cooldown_until =
                Some(Instant::now() + Duration::from_millis(u64::from(config.gesture_cooldown_ms)));
            finish_gesture_tracking(state);
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
        settings: &Settings,
        profile: &Profile,
        enabled: bool,
    ) -> Result<(), PlatformError> {
        self.shared.reconfigure(settings, profile, enabled);
        Ok(())
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
    info: MacOsIoKitInfo,
    device: MacOsNativeHidDevice,
    dev_idx: u8,
    feature_idx: u8,
    gesture_cid: u16,
    rawxy_enabled: bool,
    held: bool,
}

#[cfg(target_os = "macos")]
impl GestureSession {
    fn handle_report(&mut self, shared: &MacOsHookShared, raw: &[u8]) {
        let Some((dev_idx, feature_idx, function, _sw, params)) = parse_hidpp_message(raw) else {
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
        let _ = write_hidpp_request(
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

#[cfg(target_os = "macos")]
fn run_gesture_worker(shared: Arc<MacOsHookShared>, stop: Arc<AtomicBool>) {
    let mut session: Option<GestureSession> = None;

    while !stop.load(Ordering::SeqCst) {
        if !shared.gesture_capture_requested() {
            if let Some(mut active_session) = session.take() {
                active_session.shutdown();
                shared.mark_gesture_connected(false, Some("Gesture listener parked".to_string()));
            }
            thread::sleep(Duration::from_millis(180));
            continue;
        }

        if session.is_none() {
            match connect_gesture_session(&shared) {
                Ok(active_session) => {
                    let source = active_session
                        .info
                        .product_string
                        .clone()
                        .unwrap_or_else(|| format!("PID 0x{:04X}", active_session.info.product_id));
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
                    thread::sleep(Duration::from_millis(900));
                    continue;
                }
            }
        }

        let Some(active_session) = session.as_mut() else {
            continue;
        };

        match active_session.device.read_timeout(120) {
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
                thread::sleep(Duration::from_millis(500));
            }
        }
    }

    if let Some(mut active_session) = session.take() {
        active_session.shutdown();
    }
    shared.mark_gesture_connected(false, None);
}

#[cfg(target_os = "macos")]
fn connect_gesture_session(shared: &MacOsHookShared) -> Result<GestureSession, PlatformError> {
    let infos = enumerate_iokit_infos()?;
    let mut last_error = None;

    for info in infos {
        match try_build_gesture_session(shared, &info) {
            Ok(session) => return Ok(session),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        PlatformError::Message("no Logitech gesture-capable HID interface found".to_string())
    }))
}

#[cfg(target_os = "macos")]
fn try_build_gesture_session(
    shared: &MacOsHookShared,
    info: &MacOsIoKitInfo,
) -> Result<GestureSession, PlatformError> {
    let mut last_error = None;

    for candidate in iokit_open_candidates(info) {
        let Ok(device) = MacOsNativeHidDevice::open(&candidate) else {
            continue;
        };

        match initialize_gesture_session(shared, info.clone(), device) {
            Ok(session) => return Ok(session),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        PlatformError::Message(format!(
            "could not initialize gesture diversion for pid 0x{:04X}",
            info.product_id
        ))
    }))
}

#[cfg(target_os = "macos")]
fn initialize_gesture_session(
    shared: &MacOsHookShared,
    info: MacOsIoKitInfo,
    device: MacOsNativeHidDevice,
) -> Result<GestureSession, PlatformError> {
    let transport = iokit_transport_label(info.transport.as_deref());
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
    let device_info = build_connected_device_info(
        Some(info.product_id),
        info.product_string.as_deref(),
        transport.as_deref(),
        Some("iokit"),
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
                    info,
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
                    info,
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
        info.product_id
    )))
}

#[cfg(target_os = "macos")]
fn gesture_candidates_for(gesture_cids: &[u16]) -> Vec<u16> {
    let mut ordered = Vec::new();

    for cid in gesture_cids
        .iter()
        .copied()
        .chain(DEFAULT_GESTURE_CIDS.into_iter())
    {
        if !ordered.contains(&cid) {
            ordered.push(cid);
        }
    }

    ordered
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
fn iokit_transport_label(transport: Option<&str>) -> Option<String> {
    transport.map(|value| match value {
        "Bluetooth" => "Bluetooth Low Energy".to_string(),
        other => other.to_string(),
    })
}

#[cfg(target_os = "macos")]
fn find_hidpp_feature(
    device: &MacOsNativeHidDevice,
    dev_idx: u8,
    feature_id: u16,
    timeout_ms: i32,
) -> Result<Option<u8>, PlatformError> {
    let feature_hi = ((feature_id >> 8) & 0xFF) as u8;
    let feature_lo = (feature_id & 0xFF) as u8;
    let Some((_dev_idx, _feature, _function, _sw, params)) = hidpp_request(
        device,
        dev_idx,
        0x00,
        0,
        &[feature_hi, feature_lo, 0x00],
        timeout_ms,
    )?
    else {
        return Ok(None);
    };

    Ok(params
        .first()
        .copied()
        .filter(|feature_index| *feature_index != 0))
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
    hidpp_request(
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
fn hidpp_request(
    device: &MacOsNativeHidDevice,
    dev_idx: u8,
    feature_idx: u8,
    function: u8,
    params: &[u8],
    timeout_ms: i32,
) -> Result<Option<HidppMessage>, PlatformError> {
    write_hidpp_request(device, dev_idx, feature_idx, function, params)?;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(50) as u64);
    let expected_reply_functions = [function, (function + 1) & 0x0F];

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let packet =
            device.read_timeout(remaining.min(Duration::from_millis(80)).as_millis() as i32)?;
        if packet.is_empty() {
            continue;
        }

        let Some((
            response_dev_idx,
            response_feature,
            response_function,
            response_sw,
            response_params,
        )) = parse_hidpp_message(&packet)
        else {
            continue;
        };

        if response_feature == 0xFF {
            return Ok(None);
        }

        if response_dev_idx == dev_idx
            && response_feature == feature_idx
            && response_sw == MY_SW
            && expected_reply_functions.contains(&response_function)
        {
            return Ok(Some((
                response_dev_idx,
                response_feature,
                response_function,
                response_sw,
                response_params,
            )));
        }
    }

    Ok(None)
}

#[cfg(target_os = "macos")]
fn write_hidpp_request(
    device: &MacOsNativeHidDevice,
    dev_idx: u8,
    feature_idx: u8,
    function: u8,
    params: &[u8],
) -> Result<(), PlatformError> {
    let mut packet = [0u8; LONG_LEN];
    packet[0] = LONG_ID;
    packet[1] = dev_idx;
    packet[2] = feature_idx;
    packet[3] = ((function & 0x0F) << 4) | (MY_SW & 0x0F);
    for (offset, byte) in params.iter().copied().enumerate() {
        if 4 + offset < LONG_LEN {
            packet[4 + offset] = byte;
        }
    }
    device.write_report(&packet)
}

#[cfg(target_os = "macos")]
fn parse_hidpp_message(raw: &[u8]) -> Option<HidppMessage> {
    if raw.len() < 4 {
        return None;
    }

    let offset = usize::from(matches!(raw.first(), Some(0x10) | Some(0x11)));
    if raw.len() < offset + 4 {
        return None;
    }

    let dev_idx = raw[offset];
    let feature = raw[offset + 1];
    let function_and_sw = raw[offset + 2];
    let function = (function_and_sw >> 4) & 0x0F;
    let sw = function_and_sw & 0x0F;
    let params = raw[offset + 3..].to_vec();

    Some((dev_idx, feature, function, sw, params))
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
    let abs_x = delta_x.abs();
    let abs_y = delta_y.abs();
    let dominant = abs_x.max(abs_y);
    if dominant < threshold.max(5.0) {
        return None;
    }

    let cross_limit = deadzone.max(dominant * 0.35);
    if abs_x > abs_y {
        if abs_y > cross_limit {
            return None;
        }
        if delta_x < 0.0 {
            Some(LogicalControl::GestureLeft)
        } else {
            Some(LogicalControl::GestureRight)
        }
    } else {
        if abs_x > cross_limit {
            return None;
        }
        if delta_y < 0.0 {
            Some(LogicalControl::GestureUp)
        } else {
            Some(LogicalControl::GestureDown)
        }
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
        "select_all" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_A]),
        "save" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_S]),
        "find" => send_key_combo(&[KeyCode::COMMAND, KeyCode::ANSI_F]),
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
