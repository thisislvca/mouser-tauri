#![cfg_attr(not(target_os = "windows"), allow(dead_code, unused_imports))]

use std::{
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use mouser_core::{
    build_connected_device_info, default_config, DebugEventKind, DeviceInfo, LogicalControl,
    Profile, Settings,
};

use crate::{
    AppFocusBackend, HidBackend, HidCapabilities, HookBackend, HookBackendEvent,
    HookCapabilities, PlatformError,
};

#[cfg(target_os = "windows")]
use hidapi::{BusType, DeviceInfo as HidDeviceInfo, HidApi, HidDevice};
#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::*},
    UI::{
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};

const LOGI_VID: u16 = 0x046D;
const LONG_ID: u8 = 0x11;
const LONG_LEN: usize = 20;
const BT_DEV_IDX: u8 = 0xFF;
const FEAT_ADJ_DPI: u16 = 0x2201;
const FEAT_UNIFIED_BATT: u16 = 0x1004;
const FEAT_BATTERY_STATUS: u16 = 0x1000;
const FEAT_REPROG_V4: u16 = 0x1B04;
const MY_SW: u8 = 0x0A;
const DEVICE_INDICES: [u8; 3] = [0xFF, 0x00, 0x01];
const DEFAULT_GESTURE_CIDS: [u16; 3] = [0x00C3, 0x00D7, 0x0056];
const GESTURE_DIVERT_FLAGS: u8 = 0x01;
const GESTURE_RAWXY_FLAGS: u8 = 0x05;
const GESTURE_UNDIVERT_FLAGS: u8 = 0x00;
const GESTURE_UNDIVERT_RAWXY_FLAGS: u8 = 0x04;

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

pub struct WindowsHidBackend;
pub struct WindowsAppFocusBackend;

#[derive(Clone, PartialEq, Eq)]
struct WindowsHookConfig {
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

impl WindowsHookConfig {
    fn from_runtime(settings: &Settings, profile: &Profile, enabled: bool) -> Self {
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
        self.enabled && self.action_for(control).is_some_and(|action_id| action_id != "none")
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
            .any(|control| self.action_for(control).is_some_and(|action_id| action_id != "none"))
    }
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
    pending_control: Option<LogicalControl>,
    started_at: Option<Instant>,
    last_move_at: Option<Instant>,
    delta_x: f64,
    delta_y: f64,
    cooldown_until: Option<Instant>,
    input_source: Option<GestureInputSource>,
}

struct WindowsHookShared {
    config: Mutex<WindowsHookConfig>,
    events: Mutex<Vec<HookBackendEvent>>,
    gesture_state: Mutex<GestureTrackingState>,
    hook_running: AtomicBool,
    gesture_connected: AtomicBool,
}

impl WindowsHookShared {
    fn new() -> Self {
        let config = default_config();
        let profile = config
            .active_profile()
            .cloned()
            .unwrap_or_else(|| config.profiles[0].clone());

        Self {
            config: Mutex::new(WindowsHookConfig::from_runtime(
                &config.settings,
                &profile,
                true,
            )),
            events: Mutex::new(Vec::new()),
            gesture_state: Mutex::new(GestureTrackingState::default()),
            hook_running: AtomicBool::new(false),
            gesture_connected: AtomicBool::new(false),
        }
    }

    fn current_config(&self) -> WindowsHookConfig {
        self.config.lock().unwrap().clone()
    }

    fn reconfigure(&self, settings: &Settings, profile: &Profile, enabled: bool) {
        let mut config = self.config.lock().unwrap();
        let previous = config.clone();
        let next = WindowsHookConfig::from_runtime(settings, profile, enabled);
        let changed = previous != next;
        let gesture_capture_requested = next.gesture_capture_requested();
        *config = next.clone();
        drop(config);

        if !gesture_capture_requested {
            self.reset_gesture_state();
        }

        if changed && next.debug_mode {
            self.push_event(
                DebugEventKind::Info,
                format!(
                    "Windows hook reconfigured: enabled={} debug={} gesture_capture={}",
                    next.enabled, next.debug_mode, gesture_capture_requested
                ),
            );
        }
    }

    fn push_event(&self, kind: DebugEventKind, message: impl Into<String>) {
        let mut events = self.events.lock().unwrap();
        events.push(HookBackendEvent {
            kind,
            message: message.into(),
        });
    }

    fn push_debug(&self, message: impl Into<String>) {
        if self.config.lock().unwrap().debug_mode {
            self.push_event(DebugEventKind::Info, message);
        }
    }

    fn push_gesture_debug(&self, message: impl Into<String>) {
        if self.config.lock().unwrap().debug_mode {
            self.push_event(DebugEventKind::Gesture, message);
        }
    }

    fn drain_events(&self) -> Vec<HookBackendEvent> {
        let mut events = self.events.lock().unwrap();
        std::mem::take(&mut *events)
    }

    fn gesture_capture_requested(&self) -> bool {
        self.config.lock().unwrap().gesture_capture_requested()
    }

    fn mark_hook_running(&self, running: bool, message: Option<String>) {
        let previous = self.hook_running.swap(running, Ordering::SeqCst);
        if let Some(message) = message {
            if previous != running || self.config.lock().unwrap().debug_mode {
                self.push_event(DebugEventKind::Info, message);
            }
        }
    }

    fn mark_gesture_connected(&self, connected: bool, message: Option<String>) {
        let previous = self.gesture_connected.swap(connected, Ordering::SeqCst);
        if let Some(message) = message {
            if previous != connected || self.config.lock().unwrap().debug_mode {
                self.push_event(DebugEventKind::Info, message);
            }
        }
    }

    fn dispatch_control_action(&self, config: &WindowsHookConfig, control: LogicalControl) {
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
        state.pending_control = None;
        if config.gesture_direction_enabled() && !cooldown_active(&state) {
            self.push_gesture_debug("Gesture button down");
            start_gesture_tracking(&mut state);
        } else {
            finish_gesture_tracking(&mut state);
        }
    }

    fn hid_gesture_up(&self) {
        let config = self.current_config();
        let (should_click, pending_control) = {
            let mut state = self.gesture_state.lock().unwrap();
            if !state.active {
                return;
            }

            let should_click =
                !state.triggered && config.handles_control(LogicalControl::GesturePress);
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
        let mut state = self.gesture_state.lock().unwrap();
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

    fn accumulate_gesture_delta(
        &self,
        config: &WindowsHookConfig,
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
        let idle_timed_out = state
            .last_move_at
            .is_some_and(|last_move_at| {
                now.duration_since(last_move_at).as_millis() > u128::from(config.gesture_timeout_ms)
            });
        if idle_timed_out {
            self.push_gesture_debug(format!(
                "Gesture segment reset after {} ms",
                config.gesture_timeout_ms
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
        settings: &Settings,
        profile: &Profile,
        enabled: bool,
    ) -> Result<(), PlatformError> {
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (settings, profile, enabled);
            Ok(())
        }

        #[cfg(target_os = "windows")]
        {
            self.shared.reconfigure(settings, profile, enabled);
            Ok(())
        }
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

            for info in vendor_hid_infos(&api) {
                let Ok(device) = info.open_device(&api) else {
                    continue;
                };
                if let Some(device_info) = probe_hidapi_device(&device, info) {
                    push_unique_device(&mut devices, device_info);
                }
            }

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
            for info in vendor_hid_infos(&api) {
                let Ok(device) = info.open_device(&api) else {
                    continue;
                };

                if device_key_matches(
                    device_key,
                    Some(info.product_id()),
                    info.product_string(),
                    Some(transport_label(info.bus_type())),
                    "hidapi",
                    dpi,
                ) && set_hidpp_dpi(&device, dpi)?
                {
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

    fn current_frontmost_app(&self) -> Result<Option<String>, PlatformError> {
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

            let mut process_id = 0u32;
            GetWindowThreadProcessId(hwnd, &mut process_id);
            if process_id == 0 {
                return Ok(None);
            }

            foreground_process_name(process_id).map(Some)
        }
    }
}

#[cfg(target_os = "windows")]
fn global_hook_shared() -> &'static Mutex<Option<Arc<WindowsHookShared>>> {
    static SHARED: OnceLock<Mutex<Option<Arc<WindowsHookShared>>>> = OnceLock::new();
    SHARED.get_or_init(|| Mutex::new(None))
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

            if !config.handles_control(control) {
                return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
            }

            if message == WM_XBUTTONDOWN {
                shared.dispatch_control_action(&config, control);
            }
            return 1;
        }
        WM_MBUTTONDOWN | WM_MBUTTONUP => {
            if !config.handles_control(LogicalControl::Middle) {
                return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
            }

            if message == WM_MBUTTONDOWN {
                shared.dispatch_control_action(&config, LogicalControl::Middle);
            }
            return 1;
        }
        WM_MOUSEHWHEEL => {
            let delta = hiword(data.mouseData);
            if delta != 0 {
                let control = if delta > 0 {
                    LogicalControl::HscrollLeft
                } else {
                    LogicalControl::HscrollRight
                };

                if config.handles_control(control) {
                    shared.dispatch_control_action(&config, control);
                    return 1;
                }

                if config.invert_horizontal_scroll {
                    inject_scroll(MOUSEEVENTF_HWHEEL, -delta);
                    return 1;
                }
            }
        }
        WM_MOUSEWHEEL => {
            let delta = hiword(data.mouseData);
            if delta != 0 && config.invert_vertical_scroll {
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
                    thread::sleep(Duration::from_millis(900));
                    continue;
                }
            }
        }

        let Some(active_session) = session.as_mut() else {
            continue;
        };

        match read_hid_packet(&active_session.device, 120) {
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
                shared.mark_gesture_connected(false, Some("Gesture listener disconnected".to_string()));
                thread::sleep(Duration::from_millis(500));
            }
        }
    }

    if let Some(mut active_session) = session.take() {
        active_session.shutdown();
    }
    shared.mark_gesture_connected(false, None);
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
impl GestureSession {
    fn handle_report(&mut self, shared: &WindowsHookShared, raw: &[u8]) {
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

#[cfg(target_os = "windows")]
fn connect_gesture_session(shared: &WindowsHookShared) -> Result<GestureSession, PlatformError> {
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

#[cfg(target_os = "windows")]
fn initialize_gesture_session(
    shared: &WindowsHookShared,
    info: &HidDeviceInfo,
    device: HidDevice,
) -> Result<GestureSession, PlatformError> {
    let device_info = build_connected_device_info(
        Some(info.product_id()),
        info.product_string(),
        Some(transport_label(info.bus_type())),
        Some("hidapi"),
        None,
        1000,
    );
    let gesture_candidates = gesture_candidates_for(&device_info.gesture_cids);

    for dev_idx in DEVICE_INDICES {
        let Some(feature_idx) = find_hidpp_feature(&device, dev_idx, FEAT_REPROG_V4, 250)? else {
            continue;
        };

        for gesture_cid in &gesture_candidates {
            if set_gesture_reporting(&device, dev_idx, feature_idx, *gesture_cid, GESTURE_RAWXY_FLAGS, 250)?.is_some() {
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

            if set_gesture_reporting(&device, dev_idx, feature_idx, *gesture_cid, GESTURE_DIVERT_FLAGS, 250)?.is_some() {
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

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
fn vendor_hid_infos(api: &HidApi) -> Vec<&HidDeviceInfo> {
    api.device_list()
        .filter(|info| info.vendor_id() == LOGI_VID && info.usage_page() >= 0xFF00)
        .collect()
}

#[cfg(target_os = "windows")]
fn probe_hidapi_device(device: &HidDevice, info: &HidDeviceInfo) -> Option<DeviceInfo> {
    let current_dpi = read_hidpp_current_dpi(device).ok().flatten().unwrap_or(1000);
    let battery_level = read_hidpp_battery(device).ok().flatten();
    Some(build_connected_device_info(
        Some(info.product_id()),
        info.product_string(),
        Some(transport_label(info.bus_type())),
        Some("hidapi"),
        battery_level,
        current_dpi,
    ))
}

#[cfg(target_os = "windows")]
fn push_unique_device(devices: &mut Vec<DeviceInfo>, device: DeviceInfo) {
    if devices
        .iter()
        .all(|existing| existing.key != device.key)
    {
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
    dpi: u16,
) -> bool {
    build_connected_device_info(
        product_id,
        product_name,
        transport,
        Some(source),
        None,
        dpi,
    )
    .key
        == device_key
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
    let Some(feature_index) = find_feature(device, FEAT_ADJ_DPI)? else {
        return Ok(false);
    };

    let hi = ((dpi >> 8) & 0xFF) as u8;
    let lo = (dpi & 0xFF) as u8;
    Ok(request(device, feature_index, 1, &[0, hi, lo])?.is_some())
}

#[cfg(target_os = "windows")]
fn read_hidpp_current_dpi(device: &HidDevice) -> Result<Option<u16>, PlatformError> {
    let Some(feature_index) = find_feature(device, FEAT_ADJ_DPI)? else {
        return Ok(None);
    };

    let Some(response) = request(device, feature_index, 0, &[0])? else {
        return Ok(None);
    };
    if response.len() < 2 {
        return Ok(None);
    }

    Ok(Some(u16::from(response[0]) << 8 | u16::from(response[1])))
}

#[cfg(target_os = "windows")]
fn read_hidpp_battery(device: &HidDevice) -> Result<Option<u8>, PlatformError> {
    if let Some(feature_index) = find_feature(device, FEAT_UNIFIED_BATT)? {
        if let Some(response) = request(device, feature_index, 1, &[])? {
            return Ok(response.first().copied());
        }
    }

    if let Some(feature_index) = find_feature(device, FEAT_BATTERY_STATUS)? {
        if let Some(response) = request(device, feature_index, 0, &[])? {
            return Ok(response.first().copied());
        }
    }

    Ok(None)
}

#[cfg(target_os = "windows")]
fn find_feature(device: &HidDevice, feature_id: u16) -> Result<Option<u8>, PlatformError> {
    let feature_hi = ((feature_id >> 8) & 0xFF) as u8;
    let feature_lo = (feature_id & 0xFF) as u8;
    let Some(response) = request(device, 0x00, 0, &[feature_hi, feature_lo, 0x00])? else {
        return Ok(None);
    };

    Ok(response.first().copied().filter(|feature_index| *feature_index != 0))
}

#[cfg(target_os = "windows")]
fn request(
    device: &HidDevice,
    feature_index: u8,
    function: u8,
    params: &[u8],
) -> Result<Option<Vec<u8>>, PlatformError> {
    write_request(device, feature_index, function, params)?;
    let deadline = Instant::now() + Duration::from_millis(1_500);
    let expected_reply_functions = [function, (function + 1) & 0x0F];

    while Instant::now() < deadline {
        let packet = read_hid_packet(device, 200)?;
        if packet.is_empty() {
            continue;
        }

        let Some((response_feature, response_function, response_sw, response_params)) =
            parse_message(&packet)
        else {
            continue;
        };

        if response_feature == 0xFF {
            return Ok(None);
        }

        if response_feature == feature_index
            && response_sw == MY_SW
            && expected_reply_functions.contains(&response_function)
        {
            return Ok(Some(response_params));
        }
    }

    Ok(None)
}

#[cfg(target_os = "windows")]
fn write_request(
    device: &HidDevice,
    feature_index: u8,
    function: u8,
    params: &[u8],
) -> Result<(), PlatformError> {
    let mut packet = [0u8; LONG_LEN];
    packet[0] = LONG_ID;
    packet[1] = BT_DEV_IDX;
    packet[2] = feature_index;
    packet[3] = ((function & 0x0F) << 4) | (MY_SW & 0x0F);
    for (offset, byte) in params.iter().copied().enumerate() {
        if 4 + offset < LONG_LEN {
            packet[4 + offset] = byte;
        }
    }

    device.write(&packet).map_err(map_hid_error)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn parse_message(raw: &[u8]) -> Option<(u8, u8, u8, Vec<u8>)> {
    if raw.len() < 4 {
        return None;
    }

    let offset = usize::from(matches!(raw.first(), Some(0x10) | Some(0x11)));
    if raw.len() < offset + 4 {
        return None;
    }

    let feature = raw[offset + 1];
    let function_and_sw = raw[offset + 2];
    let function = (function_and_sw >> 4) & 0x0F;
    let sw = function_and_sw & 0x0F;
    let params = raw[offset + 3..].to_vec();

    Some((feature, function, sw, params))
}

#[cfg(target_os = "windows")]
fn read_hid_packet(device: &HidDevice, timeout_ms: i32) -> Result<Vec<u8>, PlatformError> {
    let mut buffer = [0u8; 64];
    let size = device
        .read_timeout(&mut buffer, timeout_ms)
        .map_err(map_hid_error)?;
    Ok(buffer[..size].to_vec())
}

#[cfg(target_os = "windows")]
fn find_hidpp_feature(
    device: &HidDevice,
    dev_idx: u8,
    feature_id: u16,
    timeout_ms: i32,
) -> Result<Option<u8>, PlatformError> {
    let feature_hi = ((feature_id >> 8) & 0xFF) as u8;
    let feature_lo = (feature_id & 0xFF) as u8;
    let Some((_dev_idx, _feature, _function, _sw, params)) =
        hidpp_request(device, dev_idx, 0x00, 0, &[feature_hi, feature_lo, 0x00], timeout_ms)?
    else {
        return Ok(None);
    };

    Ok(params.first().copied().filter(|feature_index| *feature_index != 0))
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

#[cfg(target_os = "windows")]
fn hidpp_request(
    device: &HidDevice,
    dev_idx: u8,
    feature_idx: u8,
    function: u8,
    params: &[u8],
    timeout_ms: i32,
) -> Result<Option<(u8, u8, u8, u8, Vec<u8>)>, PlatformError> {
    write_hidpp_request(device, dev_idx, feature_idx, function, params)?;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(50) as u64);
    let expected_reply_functions = [function, (function + 1) & 0x0F];

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let packet = read_hid_packet(
            device,
            remaining.min(Duration::from_millis(80)).as_millis() as i32,
        )?;
        if packet.is_empty() {
            continue;
        }

        let Some((response_dev_idx, response_feature, response_function, response_sw, response_params)) =
            parse_hidpp_message(&packet)
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

#[cfg(target_os = "windows")]
fn write_hidpp_request(
    device: &HidDevice,
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

    device.write(&packet).map_err(map_hid_error)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn parse_hidpp_message(raw: &[u8]) -> Option<(u8, u8, u8, u8, Vec<u8>)> {
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

#[cfg(target_os = "windows")]
fn map_hid_error(error: hidapi::HidError) -> PlatformError {
    PlatformError::Message(error.to_string())
}

fn cooldown_active(state: &GestureTrackingState) -> bool {
    state.cooldown_until.is_some_and(|cooldown_until| Instant::now() < cooldown_until)
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
fn foreground_process_name(process_id: u32) -> Result<String, PlatformError> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if process.is_null() {
            return Err(PlatformError::Message(format!(
                "failed to open process {process_id}"
            )));
        }

        let result = process_image_name(process);
        CloseHandle(process as HANDLE);
        result
    }
}

#[cfg(target_os = "windows")]
unsafe fn process_image_name(process: HANDLE) -> Result<String, PlatformError> {
    let mut buffer = vec![0u16; 260];
    let mut size = buffer.len() as u32;
    if QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut size) == 0 {
        return Err(PlatformError::Message(
            "QueryFullProcessImageNameW failed".to_string(),
        ));
    }

    let path = String::from_utf16_lossy(&buffer[..size as usize]);
    Ok(Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or(path))
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
        "select_all" => vec![VK_CONTROL as u16, b'A' as u16],
        "save" => vec![VK_CONTROL as u16, b'S' as u16],
        "find" => vec![VK_CONTROL as u16, b'F' as u16],
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
