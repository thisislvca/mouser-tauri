mod runtime;

use std::{sync::Mutex, time::Duration};

use mouser_core::{
    AppConfig, BootstrapPayload, DebugEvent, DeviceInfo, EngineSnapshot, LegacyImportReport,
    Profile,
};
use mouser_import::{import_legacy_config as import_legacy_payload, ImportSource};
use runtime::AppRuntime;
use serde::{Deserialize, Serialize};
use specta::Type;
use specta_typescript::{BigIntExportBehavior, Typescript};
use tauri::{
    menu::{MenuBuilder, MenuEvent, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager, State, Wry,
};
use tauri_specta::{collect_commands, collect_events, Builder, Event as SpectaEvent};

struct AppState {
    runtime: Mutex<AppRuntime>,
}

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
#[tauri_specta(event_name = "debug_event")]
struct DebugEventEnvelope(pub DebugEvent);

#[tauri::command]
#[specta::specta]
fn bootstrap_load(state: State<'_, AppState>) -> Result<BootstrapPayload, String> {
    Ok(state.runtime.lock().unwrap().bootstrap_payload())
}

#[tauri::command]
#[specta::specta]
fn config_get(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.runtime.lock().unwrap().config())
}

#[tauri::command]
#[specta::specta]
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
#[specta::specta]
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
#[specta::specta]
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
#[specta::specta]
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
#[specta::specta]
fn devices_list(state: State<'_, AppState>) -> Result<Vec<DeviceInfo>, String> {
    Ok(state.runtime.lock().unwrap().devices())
}

#[tauri::command]
#[specta::specta]
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

    let debug_event = {
        let mut runtime = state.runtime.lock().unwrap();
        runtime.apply_imported_config(report.config.clone());
        runtime.last_debug_event()
    };

    let payload = state.runtime.lock().unwrap().bootstrap_payload();
    emit_runtime_events(&app, &payload, debug_event)?;
    Ok(report)
}

#[tauri::command]
#[specta::specta]
fn debug_clear_log(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<EngineSnapshot, String> {
    let payload = {
        let mut runtime = state.runtime.lock().unwrap();
        runtime.clear_debug_log();
        runtime.bootstrap_payload()
    };
    emit_runtime_events(&app, &payload, None)?;
    Ok(payload.engine_snapshot)
}

fn emit_runtime_events(
    app: &AppHandle,
    payload: &BootstrapPayload,
    debug_event: Option<DebugEvent>,
) -> Result<(), String> {
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

    if let Some(debug_event) = debug_event {
        DebugEventEnvelope(debug_event)
            .emit(app)
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

fn spawn_runtime_poller(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(900));

        let (changed, payload, debug_event) = {
            let state = app.state::<AppState>();
            let mut runtime = state.runtime.lock().unwrap();
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
            profiles_create,
            profiles_update,
            profiles_delete,
            devices_list,
            devices_select_mock,
            import_legacy_config,
            debug_clear_log
        ])
        .events(collect_events![
            DeviceChangedEvent,
            ProfileChangedEvent,
            EngineStatusChangedEvent,
            DebugEventEnvelope
        ])
        .typ::<BootstrapPayload>()
        .typ::<AppConfig>()
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
