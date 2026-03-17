use std::sync::Mutex;

use mouser_core::{
    AppConfig, BootstrapPayload, DebugEvent, DeviceInfo, EngineSnapshot, LegacyImportReport,
    Profile,
};
use mouser_import::{import_legacy_config as import_legacy_payload, ImportSource};
use mouser_mock::MockRuntime;
use serde::Deserialize;
use tauri::{
    menu::{MenuBuilder, MenuEvent, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State, Wry,
};

struct AppState {
    runtime: Mutex<MockRuntime>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportLegacyConfigRequest {
    source_path: Option<String>,
    raw_json: Option<String>,
}

#[tauri::command]
fn bootstrap_load(state: State<'_, AppState>) -> Result<BootstrapPayload, String> {
    Ok(state.runtime.lock().unwrap().bootstrap_payload())
}

#[tauri::command]
fn config_get(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.runtime.lock().unwrap().config())
}

#[tauri::command]
fn config_save(
    app: AppHandle,
    state: State<'_, AppState>,
    config: AppConfig,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = {
        let mut runtime = state.runtime.lock().unwrap();
        runtime.save_config(config);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    };
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
fn profiles_create(
    app: AppHandle,
    state: State<'_, AppState>,
    profile: Profile,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = {
        let mut runtime = state.runtime.lock().unwrap();
        runtime.create_profile(profile);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    };
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
fn profiles_update(
    app: AppHandle,
    state: State<'_, AppState>,
    profile: Profile,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = {
        let mut runtime = state.runtime.lock().unwrap();
        runtime.update_profile(profile);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    };
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
fn profiles_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<BootstrapPayload, String> {
    let (payload, debug_event) = {
        let mut runtime = state.runtime.lock().unwrap();
        runtime.delete_profile(&profile_id);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    };
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload)
}

#[tauri::command]
fn devices_list(state: State<'_, AppState>) -> Result<Vec<DeviceInfo>, String> {
    Ok(state.runtime.lock().unwrap().devices())
}

#[tauri::command]
fn devices_select_mock(
    app: AppHandle,
    state: State<'_, AppState>,
    device_key: String,
) -> Result<EngineSnapshot, String> {
    let (payload, debug_event) = {
        let mut runtime = state.runtime.lock().unwrap();
        runtime.select_device(&device_key);
        (runtime.bootstrap_payload(), runtime.last_debug_event())
    };
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(payload.engine_snapshot)
}

#[tauri::command]
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

    let debug_event = {
        let mut runtime = state.runtime.lock().unwrap();
        runtime.apply_imported_config(report.config.clone());
        runtime.last_debug_event()
    };

    let payload = state.runtime.lock().unwrap().bootstrap_payload();
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(report)
}

fn emit_runtime_events(
    app: &AppHandle,
    payload: &BootstrapPayload,
    debug_event: Option<DebugEvent>,
) -> Result<(), String> {
    app.emit(
        "device_changed",
        payload.engine_snapshot.active_device.clone(),
    )
    .map_err(|error| error.to_string())?;
    app.emit(
        "profile_changed",
        serde_json::json!({
            "activeProfileId": payload.config.active_profile_id.clone(),
            "frontmostApp": payload.engine_snapshot.engine_status.frontmost_app.clone(),
        }),
    )
    .map_err(|error| error.to_string())?;
    app.emit(
        "engine_status_changed",
        payload.engine_snapshot.engine_status.clone(),
    )
    .map_err(|error| error.to_string())?;
    if let Some(debug_event) = debug_event {
        app.emit("debug_event", debug_event)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItemBuilder::with_id("show", "Show Mouser").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&show, &quit]).build()?;
    let icon = app.default_window_icon().cloned();
    let builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(
            |app: &AppHandle<Wry>, event: MenuEvent| match event.id.as_ref() {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "quit" => app.exit(0),
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            runtime: Mutex::new(MockRuntime::new()),
        })
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            setup_tray(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap_load,
            config_get,
            config_save,
            profiles_create,
            profiles_update,
            profiles_delete,
            devices_list,
            devices_select_mock,
            import_legacy_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running mouser-tauri");
}
