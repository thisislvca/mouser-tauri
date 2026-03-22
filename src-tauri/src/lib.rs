mod config;
mod runtime;

use mouser_core::{
    AppConfig, AppDiscoverySnapshot, BootstrapPayload, DebugEvent, DeviceInfo, DeviceRoutingEvent,
    DeviceRoutingSnapshot, EngineSnapshot, LegacyImportReport, Settings,
};
#[cfg(target_os = "macos")]
use objc2::{AnyThread, MainThreadMarker};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplication, NSImage};
#[cfg(target_os = "macos")]
use objc2_foundation::NSData;
#[cfg(target_os = "macos")]
use mouser_platform::macos::{MacOsAppFocusMonitor, MacOsDeviceMonitor};
#[cfg(target_os = "windows")]
use mouser_platform::windows::WindowsAppFocusMonitor;
use serde::{Deserialize, Serialize};
use specta::Type;
use specta_typescript::{BigIntExportBehavior, Typescript};
use tauri::{
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuEvent, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager, State, Wry,
};
use tauri_specta::{collect_commands, collect_events, Builder, Event as SpectaEvent};

#[cfg(test)]
use runtime::build_device_routing_event;
use runtime::{
    RuntimeBackgroundUpdate, RuntimeError, RuntimeMutationResult, RuntimeNotification,
    RuntimeNotifier, RuntimeService,
};

const TRAY_ID: &str = "main";
const TRAY_SHOW_ID: &str = "show";
const TRAY_TOGGLE_REMAPPING_ID: &str = "toggle_remapping";
const TRAY_TOGGLE_DEBUG_ID: &str = "toggle_debug";
const TRAY_QUIT_ID: &str = "quit";

#[cfg(target_os = "macos")]
const TRAY_TEMPLATE_ICON: tauri::image::Image<'_> =
    tauri::include_image!("./icons/tray-icon-template.png");

struct AppState {
    runtime: RuntimeService,
}

type CommandResult<T> = Result<T, RuntimeError>;

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

fn runtime_service<'a>(state: &'a State<'a, AppState>) -> &'a RuntimeService {
    &state.inner().runtime
}

#[tauri::command]
#[specta::specta]
fn bootstrap_load(state: State<'_, AppState>) -> CommandResult<BootstrapPayload> {
    runtime_service(&state).bootstrap_load()
}

#[tauri::command]
#[specta::specta]
fn config_save(
    app: AppHandle,
    state: State<'_, AppState>,
    config: AppConfig,
) -> CommandResult<BootstrapPayload> {
    let result = runtime_service(&state).config_save(config)?;
    emit_mutation_result(&app, result)
}

#[tauri::command]
#[specta::specta]
fn app_discovery_refresh(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<BootstrapPayload> {
    let result = runtime_service(&state).app_discovery_refresh()?;
    emit_mutation_result(&app, result)
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
fn devices_reset_to_factory(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
) -> CommandResult<BootstrapPayload> {
    let result = runtime_service(&state).devices_reset_to_factory(device_key)?;
    emit_mutation_result(&app, result)
}

#[tauri::command]
#[specta::specta]
fn devices_add(
    app: AppHandle,
    state: State<'_, AppState>,
    model_key: String,
) -> CommandResult<BootstrapPayload> {
    let result = runtime_service(&state).devices_add(model_key)?;
    emit_mutation_result(&app, result)
}

#[tauri::command]
#[specta::specta]
fn devices_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
) -> CommandResult<BootstrapPayload> {
    let result = runtime_service(&state).devices_remove(device_key)?;
    emit_mutation_result(&app, result)
}

#[tauri::command]
#[specta::specta]
fn devices_select(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
) -> CommandResult<EngineSnapshot> {
    let result = runtime_service(&state).devices_select(device_key)?;
    emit_engine_mutation_result(&app, result)
}

#[tauri::command]
#[specta::specta]
fn import_legacy_config(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ImportLegacyConfigRequest,
) -> CommandResult<LegacyImportReport> {
    let result =
        runtime_service(&state).import_legacy_config(request.source_path, request.raw_json)?;
    emit_import_mutation_result(&app, result)
}

fn emit_runtime_events(
    app: &AppHandle,
    payload: &BootstrapPayload,
    debug_events: &[DebugEvent],
    app_discovery_changed: bool,
    device_routing_event: Option<&DeviceRoutingEvent>,
) -> CommandResult<()> {
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

fn emit_debug_events(app: &AppHandle, debug_events: &[DebugEvent]) -> CommandResult<()> {
    for debug_event in debug_events {
        DebugEventEnvelope(debug_event.clone())
            .emit(app)
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn emit_background_update(app: &AppHandle, update: RuntimeBackgroundUpdate) -> CommandResult<()> {
    if let Some(payload) = update.payload {
        emit_runtime_events(
            app,
            &payload,
            &update.debug_events,
            update.app_discovery_changed,
            update.device_routing_event.as_ref(),
        )?;
    } else if let Some(device_routing_event) = update.device_routing_event {
        DeviceRoutingChangedEvent(device_routing_event)
            .emit(app)
            .map_err(|error| error.to_string())?;
    } else if !update.debug_events.is_empty() {
        emit_debug_events(app, &update.debug_events)?;
    }

    Ok(())
}

fn emit_mutation_result(
    app: &AppHandle,
    result: RuntimeMutationResult<BootstrapPayload>,
) -> CommandResult<BootstrapPayload> {
    emit_runtime_events(
        app,
        &result.payload,
        &result.debug_events,
        result.app_discovery_changed,
        result.device_routing_event.as_ref(),
    )?;
    Ok(result.result)
}

fn emit_engine_mutation_result(
    app: &AppHandle,
    result: RuntimeMutationResult<EngineSnapshot>,
) -> CommandResult<EngineSnapshot> {
    let engine_snapshot = result.result;
    emit_runtime_events(
        app,
        &result.payload,
        &result.debug_events,
        result.app_discovery_changed,
        result.device_routing_event.as_ref(),
    )?;
    Ok(engine_snapshot)
}

fn emit_import_mutation_result(
    app: &AppHandle,
    result: RuntimeMutationResult<LegacyImportReport>,
) -> CommandResult<LegacyImportReport> {
    let report = result.result;
    emit_runtime_events(
        app,
        &result.payload,
        &result.debug_events,
        result.app_discovery_changed,
        result.device_routing_event.as_ref(),
    )?;
    Ok(report)
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

#[cfg(target_os = "macos")]
struct RuntimeMonitors {
    _device_monitor: Option<MacOsDeviceMonitor>,
    _app_focus_monitor: Option<MacOsAppFocusMonitor>,
}

#[cfg(target_os = "windows")]
struct RuntimeMonitors {
    _app_focus_monitor: Option<WindowsAppFocusMonitor>,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct RuntimeMonitors;

fn push_runtime_debug_event(
    runtime: &RuntimeNotifier,
    kind: mouser_core::DebugEventKind,
    message: impl Into<String>,
) {
    let _ = runtime.notify(RuntimeNotification::RecordDebugEvent {
        kind,
        message: message.into(),
    });
}

#[cfg(target_os = "macos")]
fn start_runtime_monitors(app: &tauri::App) -> CommandResult<RuntimeMonitors> {
    let state = app.state::<AppState>();
    let runtime = &state.inner().runtime;
    let notifier = runtime.notifier();

    let device_notifier = notifier.clone();
    let device_monitor = match MacOsDeviceMonitor::new(move || {
        let _ = device_notifier.notify(RuntimeNotification::DevicesChanged);
    }) {
        Ok(monitor) => Some(monitor),
        Err(error) => {
            push_runtime_debug_event(
                &notifier,
                mouser_core::DebugEventKind::Warning,
                format!("macOS HID device monitor unavailable: {error}"),
            );
            runtime.start_device_polling()?;
            None
        }
    };

    let focus_notifier = notifier.clone();
    let app_focus_monitor = match MacOsAppFocusMonitor::new(move |frontmost_app| {
        let _ = focus_notifier.notify(RuntimeNotification::FrontmostAppChanged(frontmost_app));
    }) {
        Ok(monitor) => Some(monitor),
        Err(error) => {
            push_runtime_debug_event(
                &notifier,
                mouser_core::DebugEventKind::Warning,
                format!("macOS app-focus monitor unavailable: {error}"),
            );
            runtime.start_focus_fallback()?;
            None
        }
    };

    Ok(RuntimeMonitors {
        _device_monitor: device_monitor,
        _app_focus_monitor: app_focus_monitor,
    })
}

#[cfg(target_os = "windows")]
fn start_runtime_monitors(app: &tauri::App) -> CommandResult<RuntimeMonitors> {
    let state = app.state::<AppState>();
    let runtime = &state.inner().runtime;
    let notifier = runtime.notifier();

    let focus_notifier = notifier.clone();
    let app_focus_monitor = match WindowsAppFocusMonitor::new(move |frontmost_app| {
        let _ = focus_notifier.notify(RuntimeNotification::FrontmostAppChanged(frontmost_app));
    }) {
        Ok(monitor) => Some(monitor),
        Err(error) => {
            push_runtime_debug_event(
                &notifier,
                mouser_core::DebugEventKind::Warning,
                format!("Windows app-focus monitor unavailable: {error}"),
            );
            runtime.start_focus_fallback()?;
            None
        }
    };

    Ok(RuntimeMonitors {
        _app_focus_monitor: app_focus_monitor,
    })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn start_runtime_monitors(_app: &tauri::App) -> CommandResult<RuntimeMonitors> {
    Ok(RuntimeMonitors)
}

fn sync_tray_menu(app: &AppHandle<Wry>, payload: &BootstrapPayload) -> CommandResult<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };

    let menu = build_tray_menu(
        app,
        payload.engine_snapshot.engine_status.enabled,
        payload.engine_snapshot.engine_status.debug_mode,
    )
    .map_err(|error| error.to_string())?;

    Ok(tray
        .set_menu(Some(menu))
        .map_err(|error| error.to_string())?)
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let runtime = &state.inner().runtime;
    let bootstrap = runtime
        .bootstrap_load()
        .unwrap_or_else(|_| BootstrapPayload {
            config: AppConfig {
                version: 4,
                active_profile_id: "default".to_string(),
                profiles: Vec::new(),
                managed_devices: Vec::new(),
                settings: Settings {
                    start_minimized: true,
                    start_at_login: false,
                    appearance_mode: mouser_core::AppearanceMode::System,
                    debug_mode: false,
                    debug_log_groups: mouser_core::default_debug_log_groups(),
                },
                device_defaults: mouser_core::default_device_settings(),
            },
            available_actions: Vec::new(),
            known_apps: Vec::new(),
            app_discovery: mouser_core::default_app_discovery_snapshot(),
            supported_devices: Vec::new(),
            layouts: Vec::new(),
            engine_snapshot: EngineSnapshot {
                devices: Vec::new(),
                detected_devices: Vec::new(),
                device_routing: DeviceRoutingSnapshot::default(),
                active_device_key: None,
                active_device: None,
                engine_status: mouser_core::EngineStatus {
                    enabled: true,
                    connected: false,
                    active_profile_id: "default".to_string(),
                    frontmost_app: None,
                    selected_device_key: None,
                    debug_mode: false,
                    debug_log: Vec::new(),
                    runtime_health: mouser_core::RuntimeHealth::default(),
                },
            },
            platform_capabilities: mouser_core::PlatformCapabilities {
                platform: "unknown".to_string(),
                windows_supported: true,
                macos_supported: true,
                live_hooks_available: false,
                live_hid_available: false,
                tray_ready: true,
                mapping_engine_ready: false,
                gesture_diversion_available: false,
                active_hid_backend: "unknown".to_string(),
                active_hook_backend: "unknown".to_string(),
                active_focus_backend: "unknown".to_string(),
                hidapi_available: false,
                iokit_available: false,
            },
            manual_layout_choices: Vec::new(),
        });
    let remapping_enabled = bootstrap.engine_snapshot.engine_status.enabled;
    let debug_mode = bootstrap.engine_snapshot.engine_status.debug_mode;
    let menu = build_tray_menu(app, remapping_enabled, debug_mode)?;
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(
            |app: &AppHandle<Wry>, event: MenuEvent| match event.id.as_ref() {
                TRAY_SHOW_ID => show_main_window(app),
                TRAY_TOGGLE_REMAPPING_ID => {
                    let state = app.state::<AppState>();
                    let runtime = &state.inner().runtime;
                    let current = runtime.bootstrap_load();
                    let next_enabled = current
                        .ok()
                        .map(|payload| !payload.engine_snapshot.engine_status.enabled)
                        .unwrap_or(true);
                    let Ok(result) = runtime.set_enabled(next_enabled) else {
                        return;
                    };
                    let _ = emit_runtime_events(
                        app,
                        &result.payload,
                        &result.debug_events,
                        result.app_discovery_changed,
                        result.device_routing_event.as_ref(),
                    );
                }
                TRAY_TOGGLE_DEBUG_ID => {
                    let state = app.state::<AppState>();
                    let runtime = &state.inner().runtime;
                    let current = runtime.bootstrap_load();
                    let next_debug_mode = current
                        .ok()
                        .map(|payload| !payload.engine_snapshot.engine_status.debug_mode)
                        .unwrap_or(true);
                    let Ok(result) = runtime.set_debug_mode(next_debug_mode) else {
                        return;
                    };
                    let _ = emit_runtime_events(
                        app,
                        &result.payload,
                        &result.debug_events,
                        result.app_discovery_changed,
                        result.device_routing_event.as_ref(),
                    );
                    if next_debug_mode {
                        show_main_window(app);
                    }
                }
                TRAY_QUIT_ID => app.exit(0),
                _ => {}
            },
        );

    #[cfg(target_os = "macos")]
    builder
        .icon(TRAY_TEMPLATE_ICON)
        .icon_as_template(true)
        .build(app)?;

    #[cfg(not(target_os = "macos"))]
    {
        if let Some(icon) = app.default_window_icon().cloned() {
            builder.icon(icon).build(app)?;
        } else {
            builder.build(app)?;
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn set_macos_dock_icon() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    let icon_data = NSData::with_bytes(include_bytes!("../icons/icon.icns"));
    let Some(icon) = NSImage::initWithData(NSImage::alloc(), &icon_data) else {
        return;
    };

    unsafe {
        NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&icon));
    }
}

#[cfg(not(target_os = "macos"))]
fn set_macos_dock_icon() {}

pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            bootstrap_load,
            config_save,
            app_discovery_refresh,
            app_icon_load,
            devices_add,
            devices_reset_to_factory,
            devices_remove,
            devices_select,
            import_legacy_config,
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
        .typ::<RuntimeError>()
}

pub fn export_bindings() -> CommandResult<()> {
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
    Ok(std::fs::write(output_path, generated).map_err(|error| error.to_string())?)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(debug_assertions)]
    export_bindings().expect("failed to export specta bindings");

    let specta_builder = specta_builder();
    let runtime = RuntimeService::new(None);

    let app = tauri::Builder::default()
        .manage(AppState { runtime })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(specta_builder.invoke_handler())
        .build(tauri::generate_context!())
        .expect("error while building mouser-tauri");

    set_macos_dock_icon();
    setup_tray(&app).expect("failed to set up tray");
    specta_builder.mount_events(&app);

    {
        let state = app.state::<AppState>();
        let runtime = &state.inner().runtime;
        let app_handle = app.handle().clone();
        runtime
            .attach_listener(move |update| {
                let _ = emit_background_update(&app_handle, update);
            })
            .expect("failed to attach runtime listener");
        runtime
            .start_background()
            .expect("failed to start runtime background tasks");
    }

    let _runtime_monitors = start_runtime_monitors(&app).expect("failed to start runtime monitors");

    app.run(|_, _| {});
}

#[cfg(test)]
mod tests {
    use super::*;
    use mouser_core::{
        DeviceAttributionStatus, DeviceMatchKind, DeviceRoutingChangeKind, DeviceRoutingEntry,
    };

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
