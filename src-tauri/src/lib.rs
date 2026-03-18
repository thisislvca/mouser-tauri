mod runtime;

use std::{
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use mouser_core::{
    AppConfig, AppDiscoverySnapshot, BootstrapPayload, DebugEvent, DeviceInfo, DeviceSettings,
    EngineSnapshot, LegacyImportReport, Profile, Settings,
};
use mouser_import::{import_legacy_config as import_legacy_payload, ImportSource};
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
    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.save_config(config);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
#[specta::specta]
fn app_settings_update(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.update_app_settings(settings);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
#[specta::specta]
fn device_defaults_update(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: DeviceSettings,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.update_device_defaults(settings);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
#[specta::specta]
fn profiles_create(
    app: AppHandle,
    state: State<'_, AppState>,
    profile: Profile,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.create_profile(profile);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
#[specta::specta]
fn profiles_update(
    app: AppHandle,
    state: State<'_, AppState>,
    profile: Profile,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.update_profile(profile);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
#[specta::specta]
fn profiles_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.delete_profile(&profile_id);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
#[specta::specta]
fn app_discovery_refresh(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.refresh_app_discovery();
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
#[specta::specta]
fn devices_update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
    settings: DeviceSettings,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.update_managed_device_settings(&device_key, settings);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
#[specta::specta]
fn devices_update_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
    profile_id: Option<String>,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.update_managed_device_profile(&device_key, profile_id);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
#[specta::specta]
fn devices_update_nickname(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
    nickname: Option<String>,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.update_managed_device_nickname(&device_key, nickname);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
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
    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.add_managed_device(&model_key);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
#[specta::specta]
fn devices_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.remove_managed_device(&device_key);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
#[specta::specta]
fn devices_select(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
) -> Result<EngineSnapshot, String> {
    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.select_device(&device_key);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
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

    let (payload, debug_event) = with_runtime_mut(&state, |runtime| {
        runtime.apply_imported_config(report.config.clone());
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    })?;
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(report)
}

#[tauri::command]
#[specta::specta]
fn debug_clear_log(app: AppHandle, state: State<'_, AppState>) -> Result<EngineSnapshot, String> {
    let payload = with_runtime_mut(&state, |runtime| {
        runtime.clear_debug_log();
        runtime.bootstrap_payload()
    })?;
    emit_runtime_events(&app, &payload, None)?;
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

fn emit_runtime_events(
    app: &AppHandle,
    payload: &BootstrapPayload,
    debug_event: Option<DebugEvent>,
) -> Result<(), String> {
    sync_tray_menu(app, payload)?;

    DeviceChangedEvent(payload.engine_snapshot.active_device.clone())
        .emit(app)
        .map_err(|error| error.to_string())?;

    ProfileChangedEvent {
        active_profile_id: payload.config.active_profile_id.clone(),
        frontmost_app: payload.engine_snapshot.engine_status.frontmost_app.clone(),
    }
    .emit(app)
    .map_err(|error| error.to_string())?;

    EngineStatusChangedEvent(payload.engine_snapshot.engine_status.clone())
        .emit(app)
        .map_err(|error| error.to_string())?;

    AppDiscoveryChangedEvent(payload.app_discovery.clone())
        .emit(app)
        .map_err(|error| error.to_string())?;

    if let Some(debug_event) = debug_event {
        DebugEventEnvelope(debug_event)
            .emit(app)
            .map_err(|error| error.to_string())?;
    }

    Ok(())
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
                    let Ok((payload, debug_event)) = with_manager_runtime(app, |runtime| {
                        let next_enabled = !runtime.enabled();
                        runtime.set_enabled(next_enabled);
                        (runtime.bootstrap_payload(), runtime.last_debug_event())
                    }) else {
                        return;
                    };
                    let _ = emit_runtime_events(app, &payload, debug_event);
                }
                TRAY_TOGGLE_DEBUG_ID => {
                    let Ok((payload, debug_event, debug_mode)) =
                        with_manager_runtime(app, |runtime| {
                            let next_debug_mode = !runtime.debug_mode();
                            runtime.set_debug_mode(next_debug_mode);
                            (
                                runtime.bootstrap_payload(),
                                runtime.last_debug_event(),
                                next_debug_mode,
                            )
                        })
                    else {
                        return;
                    };
                    let _ = emit_runtime_events(app, &payload, debug_event);
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

fn spawn_runtime_poller(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(900));

        let Ok((changed, payload, debug_event)) = with_manager_runtime(&app, |runtime| {
            let changed = runtime.poll();
            let payload = if changed {
                Some(runtime.bootstrap_payload())
            } else {
                None
            };
            let debug_event = if changed {
                runtime.last_debug_event()
            } else {
                None
            };
            (changed, payload, debug_event)
        }) else {
            break;
        };

        if changed {
            if let Some(payload) = payload {
                let _ = emit_runtime_events(&app, &payload, debug_event);
            }
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
            profiles_create,
            profiles_update,
            profiles_delete,
            devices_list,
            devices_add,
            devices_update_settings,
            devices_update_profile,
            devices_update_nickname,
            devices_remove,
            devices_select,
            devices_select_mock,
            import_legacy_config,
            debug_clear_log
        ])
        .events(collect_events![
            DeviceChangedEvent,
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
            spawn_runtime_poller(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running mouser-tauri");
}
