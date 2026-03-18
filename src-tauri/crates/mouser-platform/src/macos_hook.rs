use crate::{HookBackend, HookBackendEvent, HookCapabilities, PlatformError};
use mouser_core::{Binding, DebugEventKind, LogicalControl, Profile, Settings};

#[cfg(not(target_os = "macos"))]
pub struct MacOsHookBackend;

#[cfg(not(target_os = "macos"))]
impl MacOsHookBackend {
    pub fn new() -> Self {
        Self
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

    fn configure(&self, _settings: &Settings, _profile: &Profile) -> Result<(), PlatformError> {
        Ok(())
    }

    fn drain_events(&self) -> Vec<HookBackendEvent> {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[cfg(target_os = "macos")]
use core_foundation::runloop::CFRunLoop;
#[cfg(target_os = "macos")]
use core_graphics::{
    event::{
        CallbackResult, CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions,
        CGEventTapPlacement, CGEventType, EventField, KeyCode, ScrollEventUnit,
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
#[derive(Clone, Default, PartialEq, Eq)]
struct MacOsHookConfig {
    profile_id: String,
    debug_mode: bool,
    invert_vertical_scroll: bool,
    invert_horizontal_scroll: bool,
    bindings: BTreeMap<LogicalControl, String>,
}

#[cfg(target_os = "macos")]
impl MacOsHookConfig {
    fn from_runtime(settings: &Settings, profile: &Profile) -> Self {
        Self {
            profile_id: profile.id.clone(),
            debug_mode: settings.debug_mode,
            invert_vertical_scroll: settings.invert_vertical_scroll,
            invert_horizontal_scroll: settings.invert_horizontal_scroll,
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
}

#[cfg(target_os = "macos")]
struct MacOsHookShared {
    config: Mutex<MacOsHookConfig>,
    events: Mutex<Vec<HookBackendEvent>>,
    intercepting: AtomicBool,
}

#[cfg(target_os = "macos")]
impl MacOsHookShared {
    fn new() -> Self {
        Self {
            config: Mutex::new(MacOsHookConfig::default()),
            events: Mutex::new(Vec::new()),
            intercepting: AtomicBool::new(false),
        }
    }

    fn push_event(&self, kind: DebugEventKind, message: impl Into<String>) {
        let mut events = self.events.lock().unwrap();
        events.push(HookBackendEvent {
            kind,
            message: message.into(),
        });
        if events.len() > 128 {
            let excess = events.len() - 128;
            events.drain(0..excess);
        }
    }

    fn push_debug(&self, message: impl Into<String>) {
        let debug_enabled = self.config.lock().unwrap().debug_mode;
        if debug_enabled {
            self.push_event(DebugEventKind::Info, message);
        }
    }

    fn reconfigure(&self, settings: &Settings, profile: &Profile) {
        let next = MacOsHookConfig::from_runtime(settings, profile);
        let changed = {
            let mut config = self.config.lock().unwrap();
            if *config == next {
                false
            } else {
                *config = next.clone();
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
    }

    fn current_config(&self) -> MacOsHookConfig {
        self.config.lock().unwrap().clone()
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
        let Some(action_id) = config.action_for(control).map(str::to_string) else {
            return CallbackResult::Keep;
        };

        if is_down {
            self.push_debug(format!("Mapped {} -> {}", control.label(), action_id));
            if let Err(error) = execute_action(&action_id) {
                self.push_event(
                    DebugEventKind::Warning,
                    format!("Action `{action_id}` failed: {error}"),
                );
            }
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
        if horizontal_fixed != 0 {
            let control = if horizontal_fixed > 0 {
                LogicalControl::HscrollRight
            } else {
                LogicalControl::HscrollLeft
            };

            if let Some(action_id) = config.action_for(control).map(str::to_string) {
                self.push_debug(format!("Mapped {} -> {}", control.label(), action_id));
                if let Err(error) = execute_action(&action_id) {
                    self.push_event(
                        DebugEventKind::Warning,
                        format!("Action `{action_id}` failed: {error}"),
                    );
                }
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
}

#[cfg(target_os = "macos")]
pub struct MacOsHookBackend {
    shared: Arc<MacOsHookShared>,
    run_loop: Arc<Mutex<Option<CFRunLoop>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

#[cfg(target_os = "macos")]
impl MacOsHookBackend {
    pub fn new() -> Self {
        let shared = Arc::new(MacOsHookShared::new());
        let run_loop = Arc::new(Mutex::new(None));
        let (startup_tx, startup_rx) = mpsc::channel::<Result<(), ()>>();
        let startup_signal = Arc::new(Mutex::new(Some(startup_tx)));

        let worker_shared = Arc::clone(&shared);
        let worker_loop = Arc::clone(&run_loop);
        let worker_signal = Arc::clone(&startup_signal);
        let callback_shared = Arc::clone(&shared);

        let handle = thread::Builder::new()
            .name("mouser-macos-eventtap".to_string())
            .spawn(move || {
                let current_run_loop = CFRunLoop::get_current();
                *worker_loop.lock().unwrap() = Some(current_run_loop.clone());

                let result = CGEventTap::with_enabled(
                    CGEventTapLocation::Session,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::Default,
                    vec![
                        CGEventType::OtherMouseDown,
                        CGEventType::OtherMouseUp,
                        CGEventType::ScrollWheel,
                        CGEventType::TapDisabledByTimeout,
                        CGEventType::TapDisabledByUserInput,
                    ],
                    move |_proxy, event_type, event| {
                        callback_shared.handle_event(event_type, event)
                    },
                    || {
                        worker_shared.intercepting.store(true, Ordering::SeqCst);
                        if let Some(tx) = worker_signal.lock().unwrap().take() {
                            let _ = tx.send(Ok(()));
                        }
                        CFRunLoop::run_current();
                    },
                );

                *worker_loop.lock().unwrap() = None;
                worker_shared.intercepting.store(false, Ordering::SeqCst);

                if result.is_err() {
                    worker_shared.push_event(
                        DebugEventKind::Warning,
                        "Failed to start macOS CGEventTap. Grant Accessibility access in System Settings > Privacy & Security > Accessibility.",
                    );
                    if let Some(tx) = startup_signal.lock().unwrap().take() {
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

        Self {
            shared,
            run_loop,
            worker: Mutex::new(handle),
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
    }
}

#[cfg(target_os = "macos")]
impl HookBackend for MacOsHookBackend {
    fn backend_id(&self) -> &'static str {
        if self.shared.intercepting.load(Ordering::SeqCst) {
            "macos-eventtap"
        } else {
            "macos-eventtap-unavailable"
        }
    }

    fn capabilities(&self) -> HookCapabilities {
        let intercepting = self.shared.intercepting.load(Ordering::SeqCst);
        HookCapabilities {
            can_intercept_buttons: intercepting,
            can_intercept_scroll: intercepting,
            supports_gesture_diversion: false,
        }
    }

    fn configure(&self, settings: &Settings, profile: &Profile) -> Result<(), PlatformError> {
        self.shared.reconfigure(settings, profile);
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
    match action_id {
        "none" => Ok(()),
        "alt_tab" => send_key_combo(&[KeyCode::COMMAND, KeyCode::TAB]),
        "alt_shift_tab" => send_key_combo(&[KeyCode::COMMAND, KeyCode::SHIFT, KeyCode::TAB]),
        "show_desktop" => send_key_combo(&[KeyCode::F11]),
        "task_view" => send_key_combo(&[KeyCode::CONTROL, KeyCode::UP_ARROW]),
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

    let down_cg = down
        .CGEvent()
        .ok_or_else(|| PlatformError::Message("media-key down event missing CGEvent".to_string()))?;
    let up_cg = up
        .CGEvent()
        .ok_or_else(|| PlatformError::Message("media-key up event missing CGEvent".to_string()))?;

    ObjcCGEvent::post(ObjcCGEventTapLocation::HIDEventTap, Some(&down_cg));
    ObjcCGEvent::post(ObjcCGEventTapLocation::HIDEventTap, Some(&up_cg));
    Ok(())
}
