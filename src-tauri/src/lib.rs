mod runtime;

use std::{
    collections::BTreeMap,
    sync::{mpsc, Mutex, MutexGuard},
    time::Duration,
};

use mouser_core::{
    AppConfig, AppDiscoverySnapshot, AppIdentity, BootstrapPayload, DebugEvent, DebugEventKind,
    DeviceInfo, DeviceRoutingChange, DeviceRoutingChangeKind, DeviceRoutingEntry,
    DeviceRoutingEvent, DeviceRoutingSnapshot, DeviceSettings, EngineSnapshot, LegacyImportReport,
    Profile, Settings,
};
use mouser_import::{import_legacy_config as import_legacy_payload, ImportSource};
#[cfg(target_os = "macos")]
use mouser_platform::macos::{MacOsAppFocusMonitor, MacOsDeviceMonitor};
#[cfg(target_os = "windows")]
use mouser_platform::windows::WindowsAppFocusMonitor;
use runtime::AppRuntime;
use serde::{Deserialize, Serialize};
use specta::Type;
use specta_typescript::{BigIntExportBehavior, Typescript};
use tauri::{
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuEvent, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager, State, Wry,
};
use tauri_specta::{collect_commands, collect_events, Builder, Event as SpectaEvent};

const TRAY_ID: &str = "main";
const TRAY_SHOW_ID: &str = "show";
const TRAY_TOGGLE_REMAPPING_ID: &str = "toggle_remapping";
const TRAY_TOGGLE_DEBUG_ID: &str = "toggle_debug";
const TRAY_QUIT_ID: &str = "quit";

struct AppState {
    runtime: Mutex<AppRuntime>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Debug, Clone)]
enum RuntimeSignal {
    DevicesChanged,
    FrontmostAppChanged(Option<AppIdentity>),
    HookDrain,
    SafetyResync,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
type RuntimeSignalTx = mpsc::SyncSender<RuntimeSignal>;

#[cfg(any(target_os = "macos", target_os = "windows"))]
const RUNTIME_SIGNAL_BUFFER: usize = 32;

type CommandResult<T> = Result<T, String>;
const RUNTIME_STATE_ERROR: &str = "runtime state is unavailable";

#[derive(Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportLegacyConfigRequest {
    source_path: Option<String>,
    raw_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, SpectaEvent)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "device_changed")]
struct DeviceChangedEvent(pub Option<DeviceInfo>);

#[derive(Debug, Clone, Serialize, Deserialize, Type, SpectaEvent)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "profile_changed")]
struct ProfileChangedEvent {
    active_profile_id: String,
    frontmost_app: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, SpectaEvent)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "engine_status_changed")]
struct EngineStatusChangedEvent(pub mouser_core::EngineStatus);

#[derive(Debug, Clone, Serialize, Deserialize, Type, SpectaEvent)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "app_discovery_changed")]
struct AppDiscoveryChangedEvent(pub AppDiscoverySnapshot);

#[derive(Debug, Clone, Serialize, Deserialize, Type, SpectaEvent)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "debug_event")]
struct DebugEventEnvelope(pub DebugEvent);

#[derive(Debug, Clone, Serialize, Deserialize, Type, SpectaEvent)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "device_routing_changed")]
struct DeviceRoutingChangedEvent(pub DeviceRoutingEvent);

#[tauri::command]
#[specta::specta]
fn bootstrap_load(state: State<'_, AppState>) -> CommandResult<BootstrapPayload> {
    with_runtime(&state, AppRuntime::bootstrap_payload)
}

#[tauri::command]
#[specta::specta]
fn config_get(state: State<'_, AppState>) -> CommandResult<AppConfig> {
    with_runtime(&state, AppRuntime::config)
}

#[tauri::command]
#[specta::specta]
fn config_save(
    app: AppHandle,
    state: State<'_, AppState>,
    config: AppConfig,
) -> Result<BootstrapPayload, String> {
    mutate_runtime_and_emit_payload(&app, &state, |runtime| runtime.save_config(config))
}

#[tauri::command]
#[specta::specta]
fn app_settings_update(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<BootstrapPayload, String> {
    mutate_runtime_and_emit_payload(&app, &state, |runtime| {
        runtime.update_app_settings(settings)
    })
}

#[tauri::command]
#[specta::specta]
fn device_defaults_update(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: DeviceSettings,
) -> Result<BootstrapPayload, String> {
    mutate_runtime_and_emit_payload(&app, &state, |runtime| {
        runtime.update_device_defaults(settings)
    })
}

#[tauri::command]
#[specta::specta]
fn profiles_create(
    app: AppHandle,
    state: State<'_, AppState>,
    profile: Profile,
) -> Result<BootstrapPayload, String> {
    mutate_runtime_and_emit_payload(&app, &state, |runtime| runtime.create_profile(profile))
}

#[tauri::command]
#[specta::specta]
fn profiles_update(
    app: AppHandle,
    state: State<'_, AppState>,
    profile: Profile,
) -> Result<BootstrapPayload, String> {
    mutate_runtime_and_emit_payload(&app, &state, |runtime| runtime.update_profile(profile))
}

#[tauri::command]
#[specta::specta]
fn profiles_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<BootstrapPayload, String> {
    mutate_runtime_and_emit_payload(&app, &state, |runtime| runtime.delete_profile(&profile_id))
}

#[tauri::command]
#[specta::specta]
fn app_discovery_refresh(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<BootstrapPayload, String> {
    mutate_runtime_and_emit_payload(&app, &state, AppRuntime::refresh_app_discovery)
}

#[tauri::command]
#[specta::specta]
fn app_icon_load(source_path: String) -> CommandResult<Option<String>> {
    Ok(mouser_platform::load_native_app_icon(&source_path)
        .ok()
        .flatten())
}

#[tauri::command]
#[specta::specta]
fn devices_update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
    settings: DeviceSettings,
) -> Result<BootstrapPayload, String> {
    mutate_runtime_and_emit_payload(&app, &state, |runtime| {
        runtime.update_managed_device_settings(&device_key, settings)
    })
}

#[tauri::command]
#[specta::specta]
fn devices_update_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
    profile_id: Option<String>,
) -> Result<BootstrapPayload, String> {
    mutate_runtime_and_emit_payload(&app, &state, |runtime| {
        runtime.update_managed_device_profile(&device_key, profile_id)
    })
}

#[tauri::command]
#[specta::specta]
fn devices_update_nickname(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
    nickname: Option<String>,
) -> Result<BootstrapPayload, String> {
    mutate_runtime_and_emit_payload(&app, &state, |runtime| {
        runtime.update_managed_device_nickname(&device_key, nickname)
    })
}

#[tauri::command]
#[specta::specta]
fn devices_reset_to_factory(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
) -> Result<BootstrapPayload, String> {
    mutate_runtime_and_emit_payload(&app, &state, |runtime| {
        runtime.reset_managed_device_to_factory_defaults(&device_key)
    })
}

#[tauri::command]
#[specta::specta]
fn devices_list(state: State<'_, AppState>) -> CommandResult<Vec<DeviceInfo>> {
    with_runtime(&state, AppRuntime::devices)
}

#[tauri::command]
#[specta::specta]
fn devices_add(
    app: AppHandle,
    state: State<'_, AppState>,
    model_key: String,
) -> Result<BootstrapPayload, String> {
    mutate_runtime_and_emit_payload(&app, &state, |runtime| {
        runtime.add_managed_device(&model_key)
    })
}

#[tauri::command]
#[specta::specta]
fn devices_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
) -> Result<BootstrapPayload, String> {
    mutate_runtime_and_emit_payload(&app, &state, |runtime| {
        runtime.remove_managed_device(&device_key)
    })
}

#[tauri::command]
#[specta::specta]
fn devices_select(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
) -> Result<EngineSnapshot, String> {
    let payload = mutate_runtime_and_emit_payload(&app, &state, |runtime| {
        runtime.select_device(&device_key)
    })?;
    Ok(payload.engine_snapshot)
}

#[tauri::command]
#[specta::specta]
fn devices_select_mock(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
) -> Result<EngineSnapshot, String> {
    devices_select(app, state, device_key)
}

#[tauri::command]
#[specta::specta]
fn import_legacy_config(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ImportLegacyConfigRequest,
) -> Result<LegacyImportReport, String> {
    let report = import_legacy_payload(ImportSource {
        source_path: request.source_path,
        raw_json: request.raw_json,
    })
    .map_err(|error| error.to_string())?;

    mutate_runtime_and_emit_payload(&app, &state, |runtime| {
        runtime.apply_imported_config(report.config.clone())
    })?;
    Ok(report)
}

#[tauri::command]
#[specta::specta]
fn debug_clear_log(app: AppHandle, state: State<'_, AppState>) -> Result<EngineSnapshot, String> {
    let payload = with_runtime_mut(&state, |runtime| {
        runtime.clear_debug_log();
        runtime.bootstrap_payload()
    })?;
    emit_runtime_events(&app, &payload, &[], false, None)?;
    Ok(payload.engine_snapshot)
}

fn lock_runtime<'a>(state: &'a State<'_, AppState>) -> CommandResult<MutexGuard<'a, AppRuntime>> {
    state
        .runtime
        .lock()
        .map_err(|_| RUNTIME_STATE_ERROR.to_string())
}

fn with_runtime<T>(
    state: &State<'_, AppState>,
    f: impl FnOnce(&AppRuntime) -> T,
) -> CommandResult<T> {
    let runtime = lock_runtime(state)?;
    Ok(f(&runtime))
}

fn with_runtime_mut<T>(
    state: &State<'_, AppState>,
    f: impl FnOnce(&mut AppRuntime) -> T,
) -> CommandResult<T> {
    let mut runtime = lock_runtime(state)?;
    Ok(f(&mut runtime))
}

fn mutate_runtime_and_emit<T>(
    app: &AppHandle,
    state: &State<'_, AppState>,
    f: impl FnOnce(&mut AppRuntime) -> T,
) -> Result<(T, BootstrapPayload), String> {
    let (result, payload, debug_events, app_discovery_changed, device_routing_event) =
        with_runtime_mut(state, |runtime| {
            let previous_debug_cursor = runtime.debug_event_cursor();
            let previous_app_discovery = runtime.app_discovery_snapshot();
            let previous_device_routing = runtime.device_routing_snapshot();
            let result = f(runtime);
            let payload = runtime.bootstrap_payload();
            let debug_events = runtime.debug_events_since(previous_debug_cursor);
            let app_discovery_changed = payload.app_discovery != previous_app_discovery;
            let device_routing_event = build_device_routing_event(
                &previous_device_routing,
                &payload.engine_snapshot.device_routing,
            );
            (
                result,
                payload,
                debug_events,
                app_discovery_changed,
                device_routing_event,
            )
        })?;
    emit_runtime_events(
        app,
        &payload,
        &debug_events,
        app_discovery_changed,
        device_routing_event.as_ref(),
    )?;
    Ok((result, payload))
}

fn mutate_runtime_and_emit_payload<T>(
    app: &AppHandle,
    state: &State<'_, AppState>,
    f: impl FnOnce(&mut AppRuntime) -> T,
) -> Result<BootstrapPayload, String> {
    let (_, payload) = mutate_runtime_and_emit(app, state, f)?;
    Ok(payload)
}

fn with_manager_runtime<M, T>(manager: &M, f: impl FnOnce(&mut AppRuntime) -> T) -> CommandResult<T>
where
    M: Manager<Wry>,
{
    let state = manager.state::<AppState>();
    let mut runtime = state
        .runtime
        .lock()
        .map_err(|_| RUNTIME_STATE_ERROR.to_string())?;
    Ok(f(&mut runtime))
}

fn emit_runtime_updates_if_changed(
    app: &AppHandle,
    f: impl FnOnce(&mut AppRuntime) -> runtime::RuntimeUpdateEffect,
) -> Result<(), String> {
    let result = with_manager_runtime(app, |runtime| {
        let previous_device_routing = runtime.device_routing_snapshot();
        let effect = f(runtime);
        let payload = effect.payload_changed.then(|| runtime.bootstrap_payload());
        let device_routing_event = payload.as_ref().and_then(|payload| {
            build_device_routing_event(
                &previous_device_routing,
                &payload.engine_snapshot.device_routing,
            )
        });
        (
            payload,
            effect.debug_events,
            effect.app_discovery_changed,
            device_routing_event,
        )
    })?;

    let (payload, debug_events, app_discovery_changed, device_routing_event) = result;
    if let Some(payload) = payload {
        emit_runtime_events(
            app,
            &payload,
            &debug_events,
            app_discovery_changed,
            device_routing_event.as_ref(),
        )?;
    } else if let Some(device_routing_event) = device_routing_event {
        DeviceRoutingChangedEvent(device_routing_event)
            .emit(app)
            .map_err(|error| error.to_string())?;
    } else if !debug_events.is_empty() {
        emit_debug_events(app, &debug_events)?;
    }

    Ok(())
}

fn emit_runtime_events(
    app: &AppHandle,
    payload: &BootstrapPayload,
    debug_events: &[DebugEvent],
    app_discovery_changed: bool,
    device_routing_event: Option<&DeviceRoutingEvent>,
) -> Result<(), String> {
    sync_tray_menu(app, payload)?;

    DeviceChangedEvent(payload.engine_snapshot.active_device.clone())
        .emit(app)
        .map_err(|error| error.to_string())?;

    if let Some(device_routing_event) = device_routing_event {
        DeviceRoutingChangedEvent(device_routing_event.clone())
            .emit(app)
            .map_err(|error| error.to_string())?;
    }

    ProfileChangedEvent {
        active_profile_id: payload
            .engine_snapshot
            .engine_status
            .active_profile_id
            .clone(),
        frontmost_app: payload.engine_snapshot.engine_status.frontmost_app.clone(),
    }
    .emit(app)
    .map_err(|error| error.to_string())?;

    EngineStatusChangedEvent(payload.engine_snapshot.engine_status.clone())
        .emit(app)
        .map_err(|error| error.to_string())?;

    if app_discovery_changed {
        AppDiscoveryChangedEvent(payload.app_discovery.clone())
            .emit(app)
            .map_err(|error| error.to_string())?;
    }

    emit_debug_events(app, debug_events)
}

fn emit_debug_events(app: &AppHandle, debug_events: &[DebugEvent]) -> Result<(), String> {
    for debug_event in debug_events {
        DebugEventEnvelope(debug_event.clone())
            .emit(app)
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn build_device_routing_event(
    previous: &DeviceRoutingSnapshot,
    next: &DeviceRoutingSnapshot,
) -> Option<DeviceRoutingEvent> {
    if previous == next {
        return None;
    }

    let previous_by_key = previous
        .entries
        .iter()
        .map(|entry| (entry.live_device_key.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let next_by_key = next
        .entries
        .iter()
        .map(|entry| (entry.live_device_key.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();

    for (live_device_key, next_entry) in &next_by_key {
        match previous_by_key.get(live_device_key) {
            None => changes.push(device_routing_change(
                DeviceRoutingChangeKind::Connected,
                next_entry,
            )),
            Some(previous_entry) => {
                if previous_entry.managed_device_key != next_entry.managed_device_key
                    || previous_entry.match_kind != next_entry.match_kind
                {
                    changes.push(device_routing_change(
                        DeviceRoutingChangeKind::Reassigned,
                        next_entry,
                    ));
                }
                if previous_entry.is_active_target != next_entry.is_active_target {
                    changes.push(device_routing_change(
                        DeviceRoutingChangeKind::ActiveTargetChanged,
                        next_entry,
                    ));
                }
                if previous_entry.resolved_profile_id != next_entry.resolved_profile_id {
                    changes.push(device_routing_change(
                        DeviceRoutingChangeKind::ResolvedProfileChanged,
                        next_entry,
                    ));
                }
            }
        }
    }

    for (live_device_key, previous_entry) in &previous_by_key {
        if !next_by_key.contains_key(live_device_key) {
            changes.push(device_routing_change(
                DeviceRoutingChangeKind::Disconnected,
                previous_entry,
            ));
        }
    }

    Some(DeviceRoutingEvent {
        snapshot: next.clone(),
        changes,
    })
}

fn device_routing_change(
    kind: DeviceRoutingChangeKind,
    entry: &DeviceRoutingEntry,
) -> DeviceRoutingChange {
    DeviceRoutingChange {
        kind,
        live_device_key: entry.live_device_key.clone(),
        managed_device_key: entry.managed_device_key.clone(),
        resolved_profile_id: entry.resolved_profile_id.clone(),
        match_kind: Some(entry.match_kind),
    }
}

#[cfg(target_os = "macos")]
fn push_runtime_debug_event(app: &AppHandle, kind: DebugEventKind, message: impl Into<String>) {
    let message = message.into();
    let Ok((payload, debug_events)) = with_manager_runtime(app, |runtime| {
        let previous_debug_cursor = runtime.debug_event_cursor();
        runtime.record_debug_event(kind, message);
        let debug_events = runtime.debug_events_since(previous_debug_cursor);
        (runtime.bootstrap_payload(), debug_events)
    }) else {
        return;
    };
    let _ = emit_runtime_events(app, &payload, &debug_events, false, None);
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn enqueue_runtime_signal(tx: &RuntimeSignalTx, signal: RuntimeSignal) -> bool {
    match tx.try_send(signal) {
        Ok(()) | Err(mpsc::TrySendError::Full(_)) => true,
        Err(mpsc::TrySendError::Disconnected(_)) => false,
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn spawn_periodic_runtime_signal(tx: RuntimeSignalTx, signal: RuntimeSignal, interval: Duration) {
    std::thread::spawn(move || loop {
        std::thread::sleep(interval);
        if !enqueue_runtime_signal(&tx, signal.clone()) {
            break;
        }
    });
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn spawn_focus_fallback(app: AppHandle, tx: RuntimeSignalTx) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));

        let Ok((_hid_backend, app_focus_backend, _hook_backend)) =
            with_manager_runtime(&app, |runtime| runtime.poll_backends())
        else {
            break;
        };

        let frontmost_app = app_focus_backend.current_frontmost_app().ok().flatten();
        if !enqueue_runtime_signal(&tx, RuntimeSignal::FrontmostAppChanged(frontmost_app)) {
            break;
        }
    });
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn run_runtime_monitor(app: AppHandle, rx: mpsc::Receiver<RuntimeSignal>) {
    while let Ok(signal) = rx.recv() {
        let (devices, frontmost_app, hook_events) = match signal {
            RuntimeSignal::DevicesChanged => {
                let Ok((hid_backend, _app_focus_backend, hook_backend)) =
                    with_manager_runtime(&app, |runtime| runtime.poll_backends())
                else {
                    break;
                };
                (
                    Some(hid_backend.list_devices()),
                    None,
                    hook_backend.drain_events(),
                )
            }
            RuntimeSignal::FrontmostAppChanged(frontmost_app) => {
                let Ok((_hid_backend, _app_focus_backend, hook_backend)) =
                    with_manager_runtime(&app, |runtime| runtime.poll_backends())
                else {
                    break;
                };
                (None, Some(Ok(frontmost_app)), hook_backend.drain_events())
            }
            RuntimeSignal::HookDrain => {
                let Ok((_hid_backend, _app_focus_backend, hook_backend)) =
                    with_manager_runtime(&app, |runtime| runtime.poll_backends())
                else {
                    break;
                };
                (None, None, hook_backend.drain_events())
            }
            RuntimeSignal::SafetyResync => {
                let Ok((hid_backend, app_focus_backend, hook_backend)) =
                    with_manager_runtime(&app, |runtime| runtime.poll_backends())
                else {
                    break;
                };
                (
                    Some(hid_backend.list_devices()),
                    Some(app_focus_backend.current_frontmost_app()),
                    hook_backend.drain_events(),
                )
            }
        };

        if emit_runtime_updates_if_changed(&app, move |runtime| {
            runtime.apply_runtime_updates(devices, frontmost_app, hook_events)
        })
        .is_err()
        {
            break;
        }
    }
}

#[cfg(target_os = "macos")]
fn spawn_runtime_monitor(app: AppHandle) {
    let (tx, rx) = mpsc::sync_channel::<RuntimeSignal>(RUNTIME_SIGNAL_BUFFER);
    let monitor_app = app.clone();
    std::thread::spawn(move || run_runtime_monitor(monitor_app, rx));

    spawn_periodic_runtime_signal(
        tx.clone(),
        RuntimeSignal::HookDrain,
        Duration::from_millis(500),
    );
    spawn_periodic_runtime_signal(
        tx.clone(),
        RuntimeSignal::SafetyResync,
        Duration::from_secs(30),
    );

    let device_signal_tx = tx.clone();
    match MacOsDeviceMonitor::new(move || {
        let _ = enqueue_runtime_signal(&device_signal_tx, RuntimeSignal::DevicesChanged);
    }) {
        Ok(monitor) => std::mem::forget(monitor),
        Err(error) => {
            push_runtime_debug_event(
                &app,
                DebugEventKind::Warning,
                format!("macOS HID device monitor unavailable: {error}"),
            );
            spawn_periodic_runtime_signal(
                tx.clone(),
                RuntimeSignal::DevicesChanged,
                Duration::from_secs(5),
            );
        }
    }

    let focus_signal_tx = tx.clone();
    match MacOsAppFocusMonitor::new(move |frontmost_app| {
        let _ = enqueue_runtime_signal(
            &focus_signal_tx,
            RuntimeSignal::FrontmostAppChanged(frontmost_app),
        );
    }) {
        Ok(monitor) => std::mem::forget(monitor),
        Err(error) => {
            push_runtime_debug_event(
                &app,
                DebugEventKind::Warning,
                format!("macOS app-focus monitor unavailable: {error}"),
            );
            spawn_focus_fallback(app, tx);
        }
    }
}

#[cfg(target_os = "windows")]
fn spawn_runtime_monitor(app: AppHandle) {
    let (tx, rx) = mpsc::sync_channel::<RuntimeSignal>(RUNTIME_SIGNAL_BUFFER);
    let monitor_app = app.clone();
    std::thread::spawn(move || run_runtime_monitor(monitor_app, rx));

    spawn_periodic_runtime_signal(
        tx.clone(),
        RuntimeSignal::HookDrain,
        Duration::from_millis(500),
    );
    spawn_periodic_runtime_signal(
        tx.clone(),
        RuntimeSignal::SafetyResync,
        Duration::from_secs(30),
    );
    spawn_periodic_runtime_signal(
        tx.clone(),
        RuntimeSignal::DevicesChanged,
        Duration::from_secs(5),
    );

    let focus_signal_tx = tx.clone();
    match WindowsAppFocusMonitor::new(move |frontmost_app| {
        let _ = enqueue_runtime_signal(
            &focus_signal_tx,
            RuntimeSignal::FrontmostAppChanged(frontmost_app),
        );
    }) {
        Ok(monitor) => std::mem::forget(monitor),
        Err(error) => {
            push_runtime_debug_event(
                &app,
                DebugEventKind::Warning,
                format!("Windows app-focus monitor unavailable: {error}"),
            );
            spawn_focus_fallback(app, tx);
        }
    }
}

fn build_tray_menu<M: Manager<Wry>>(
    manager: &M,
    remapping_enabled: bool,
    debug_mode: bool,
) -> tauri::Result<tauri::menu::Menu<Wry>> {
    let show = MenuItemBuilder::with_id(TRAY_SHOW_ID, "Open Mouser").build(manager)?;
    let remapping = CheckMenuItemBuilder::with_id(TRAY_TOGGLE_REMAPPING_ID, "Enable Remapping")
        .checked(remapping_enabled)
        .build(manager)?;
    let debug = CheckMenuItemBuilder::with_id(TRAY_TOGGLE_DEBUG_ID, "Debug Mode")
        .checked(debug_mode)
        .build(manager)?;
    let quit = MenuItemBuilder::with_id(TRAY_QUIT_ID, "Quit Mouser").build(manager)?;

    MenuBuilder::new(manager)
        .items(&[&show, &remapping, &debug])
        .separator()
        .item(&quit)
        .build()
}

fn show_main_window(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn sync_tray_menu(app: &AppHandle<Wry>, payload: &BootstrapPayload) -> Result<(), String> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };

    let menu = build_tray_menu(
        app,
        payload.engine_snapshot.engine_status.enabled,
        payload.engine_snapshot.engine_status.debug_mode,
    )
    .map_err(|error| error.to_string())?;

    tray.set_menu(Some(menu)).map_err(|error| error.to_string())
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let (remapping_enabled, debug_mode) =
        with_manager_runtime(app, |runtime| (runtime.enabled(), runtime.debug_mode()))
            .unwrap_or((true, false));
    let menu = build_tray_menu(app, remapping_enabled, debug_mode)?;
    let icon = app.default_window_icon().cloned();
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .icon_as_template(cfg!(target_os = "macos"))
        .show_menu_on_left_click(true)
        .on_menu_event(
            |app: &AppHandle<Wry>, event: MenuEvent| match event.id.as_ref() {
                TRAY_SHOW_ID => show_main_window(app),
                TRAY_TOGGLE_REMAPPING_ID => {
                    let Ok((payload, debug_events)) = with_manager_runtime(app, |runtime| {
                        let previous_debug_cursor = runtime.debug_event_cursor();
                        let next_enabled = !runtime.enabled();
                        runtime.set_enabled(next_enabled);
                        (
                            runtime.bootstrap_payload(),
                            runtime.debug_events_since(previous_debug_cursor),
                        )
                    }) else {
                        return;
                    };
                    let _ = emit_runtime_events(app, &payload, &debug_events, false, None);
                }
                TRAY_TOGGLE_DEBUG_ID => {
                    let Ok((payload, debug_events, debug_mode)) =
                        with_manager_runtime(app, |runtime| {
                            let previous_debug_cursor = runtime.debug_event_cursor();
                            let next_debug_mode = !runtime.debug_mode();
                            runtime.set_debug_mode(next_debug_mode);
                            (
                                runtime.bootstrap_payload(),
                                runtime.debug_events_since(previous_debug_cursor),
                                next_debug_mode,
                            )
                        })
                    else {
                        return;
                    };
                    let _ = emit_runtime_events(app, &payload, &debug_events, false, None);
                    if debug_mode {
                        show_main_window(app);
                    }
                }
                TRAY_QUIT_ID => app.exit(0),
                _ => {}
            },
        );

    if let Some(icon) = icon {
        builder.icon(icon).build(app)?;
    } else {
        builder.build(app)?;
    }

    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn spawn_runtime_poller(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(900));

        let Ok((hid_backend, app_focus_backend, hook_backend)) =
            with_manager_runtime(&app, |runtime| runtime.poll_backends())
        else {
            break;
        };

        let devices = hid_backend.list_devices();
        let frontmost_app = app_focus_backend.current_frontmost_app();
        let hook_events = hook_backend.drain_events();

        if emit_runtime_updates_if_changed(&app, move |runtime| {
            runtime.apply_poll_results(devices, frontmost_app, hook_events)
        })
        .is_err()
        {
            break;
        }
    });
}

pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            bootstrap_load,
            config_get,
            config_save,
            app_settings_update,
            device_defaults_update,
            app_discovery_refresh,
            app_icon_load,
            profiles_create,
            profiles_update,
            profiles_delete,
            devices_list,
            devices_add,
            devices_update_settings,
            devices_update_profile,
            devices_update_nickname,
            devices_reset_to_factory,
            devices_remove,
            devices_select,
            devices_select_mock,
            import_legacy_config,
            debug_clear_log
        ])
        .events(collect_events![
            DeviceChangedEvent,
            DeviceRoutingChangedEvent,
            ProfileChangedEvent,
            EngineStatusChangedEvent,
            AppDiscoveryChangedEvent,
            DebugEventEnvelope
        ])
        .typ::<BootstrapPayload>()
        .typ::<AppConfig>()
        .typ::<AppDiscoverySnapshot>()
        .typ::<DeviceInfo>()
        .typ::<EngineSnapshot>()
        .typ::<LegacyImportReport>()
        .typ::<ImportLegacyConfigRequest>()
}

pub fn export_bindings() -> Result<(), String> {
    let builder = specta_builder();
    let output_path = format!("{}/../src/lib/bindings.ts", env!("CARGO_MANIFEST_DIR"));
    builder
        .export(
            Typescript::default().bigint(BigIntExportBehavior::Number),
            &output_path,
        )
        .map_err(|error| error.to_string())?;

    let generated = std::fs::read_to_string(&output_path).map_err(|error| error.to_string())?;
    let generated = generated.replace(
        "import {\n\tinvoke as TAURI_INVOKE,\n\tChannel as TAURI_CHANNEL,\n} from \"@tauri-apps/api/core\";",
        "import {\n\tinvoke as TAURI_INVOKE,\n} from \"@tauri-apps/api/core\";",
    );
    let generated = format!("// @ts-nocheck\n{generated}");
    std::fs::write(output_path, generated).map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(debug_assertions)]
    export_bindings().expect("failed to export specta bindings");

    let specta_builder = specta_builder();

    tauri::Builder::default()
        .manage(AppState {
            runtime: Mutex::new(AppRuntime::new(None)),
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(specta_builder.invoke_handler())
        .setup(move |app| {
            setup_tray(app)?;
            specta_builder.mount_events(app);
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            spawn_runtime_monitor(app.handle().clone());
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            spawn_runtime_poller(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running mouser-tauri");
}

#[cfg(test)]
mod tests {
    use super::*;
    use mouser_core::{DeviceAttributionStatus, DeviceMatchKind};

    fn routing_entry(
        live_device_key: &str,
        managed_device_key: Option<&str>,
        resolved_profile_id: Option<&str>,
        match_kind: DeviceMatchKind,
        is_active_target: bool,
    ) -> DeviceRoutingEntry {
        DeviceRoutingEntry {
            live_device_key: live_device_key.to_string(),
            live_model_key: "mx_master_3s".to_string(),
            live_display_name: "MX Master 3S".to_string(),
            live_identity_key: Some(format!("serial:{live_device_key}")),
            managed_device_key: managed_device_key.map(str::to_string),
            managed_display_name: managed_device_key.map(|_| "MX Master 3S".to_string()),
            device_profile_id: managed_device_key.map(|_| "device_mx_master_3s".to_string()),
            resolved_profile_id: resolved_profile_id.map(str::to_string),
            match_kind,
            is_active_target,
            hook_eligible: managed_device_key.is_some(),
            attribution_status: match match_kind {
                DeviceMatchKind::Identity => DeviceAttributionStatus::Ready,
                DeviceMatchKind::ModelFallback => DeviceAttributionStatus::ModelFallback,
                DeviceMatchKind::Unmanaged => DeviceAttributionStatus::Unmanaged,
            },
            source_hints: vec![format!("serial:{live_device_key}")],
        }
    }

    #[test]
    fn build_device_routing_event_reports_connected_devices() {
        let previous = DeviceRoutingSnapshot::default();
        let next = DeviceRoutingSnapshot {
            entries: vec![routing_entry(
                "live-device",
                Some("mx-master"),
                Some("device_mx_master_3s"),
                DeviceMatchKind::Identity,
                true,
            )],
        };

        let event = build_device_routing_event(&previous, &next).expect("expected routing event");
        assert_eq!(event.changes.len(), 1);
        assert_eq!(event.changes[0].kind, DeviceRoutingChangeKind::Connected);
        assert_eq!(event.changes[0].live_device_key, "live-device");
    }

    #[test]
    fn build_device_routing_event_reports_target_and_profile_changes() {
        let previous = DeviceRoutingSnapshot {
            entries: vec![routing_entry(
                "live-device",
                Some("mx-master"),
                Some("default"),
                DeviceMatchKind::Identity,
                false,
            )],
        };
        let next = DeviceRoutingSnapshot {
            entries: vec![routing_entry(
                "live-device",
                Some("mx-master"),
                Some("vscode"),
                DeviceMatchKind::Identity,
                true,
            )],
        };

        let event = build_device_routing_event(&previous, &next).expect("expected routing event");
        assert_eq!(event.changes.len(), 2);
        assert!(event
            .changes
            .iter()
            .any(|change| change.kind == DeviceRoutingChangeKind::ActiveTargetChanged));
        assert!(event
            .changes
            .iter()
            .any(|change| change.kind == DeviceRoutingChangeKind::ResolvedProfileChanged));
    }
}
