use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use mouser_core::{
    AppConfig, AppIdentity, BootstrapPayload, DebugEventKind, DeviceInfo, DeviceSettings,
    EngineSnapshot, LegacyImportReport, Profile, Settings,
};
use mouser_import::{import_legacy_config as import_legacy_payload, ImportSource};

use super::{
    build_device_routing_event,
    events::{
        RuntimeBackgroundUpdate, RuntimeMutationResult, RuntimeNotification, RuntimeUpdateEffect,
    },
    AppRuntime, RuntimeError, RuntimeResult,
};

const HOOK_DRAIN_INTERVAL: Duration = Duration::from_millis(500);
const SAFETY_RESYNC_INTERVAL: Duration = Duration::from_secs(30);
const FOCUS_FALLBACK_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct RuntimeNotifier {
    tx: mpsc::Sender<RuntimeMessage>,
}

impl RuntimeNotifier {
    pub fn notify(&self, notification: RuntimeNotification) -> RuntimeResult<()> {
        self.tx
            .send(RuntimeMessage::Notification(notification))
            .map_err(|_| RuntimeError::StateUnavailable)
    }
}

enum RuntimeRequest {
    BootstrapLoad,
    ConfigGet,
    DevicesList,
    ConfigSave(AppConfig),
    AppSettingsUpdate(Settings),
    DeviceDefaultsUpdate(DeviceSettings),
    ProfilesCreate(Profile),
    ProfilesUpdate(Profile),
    ProfilesDelete(String),
    DevicesUpdateSettings {
        device_key: String,
        settings: DeviceSettings,
    },
    DevicesUpdateProfile {
        device_key: String,
        profile_id: Option<String>,
    },
    DevicesUpdateNickname {
        device_key: String,
        nickname: Option<String>,
    },
    DevicesResetToFactory(String),
    DevicesAdd(String),
    DevicesRemove(String),
    DevicesSelect(String),
    ImportLegacyConfig {
        source_path: Option<String>,
        raw_json: Option<String>,
    },
    DebugClearLog,
    SetEnabled(bool),
    SetDebugMode(bool),
    RefreshAppDiscovery,
}

enum RuntimeResponse {
    Bootstrap(Box<BootstrapPayload>),
    Config(Box<AppConfig>),
    Devices(Vec<DeviceInfo>),
    Payload(Box<RuntimeMutationResult<BootstrapPayload>>),
    Engine(Box<RuntimeMutationResult<EngineSnapshot>>),
    Import(Box<RuntimeMutationResult<LegacyImportReport>>),
}

enum RuntimeMessage {
    Request(RuntimeRequest, mpsc::Sender<RuntimeResult<RuntimeResponse>>),
    Notification(RuntimeNotification),
    Shutdown,
}

#[derive(Default)]
struct PendingNotifications {
    startup_sync: bool,
    devices_changed: bool,
    hook_drain: bool,
    safety_resync: bool,
    refresh_app_discovery: bool,
    frontmost_app: Option<Option<AppIdentity>>,
    debug_events: Vec<(DebugEventKind, String)>,
}

impl PendingNotifications {
    fn push(&mut self, notification: RuntimeNotification) {
        match notification {
            RuntimeNotification::StartupSync => self.startup_sync = true,
            RuntimeNotification::DevicesChanged => self.devices_changed = true,
            RuntimeNotification::FrontmostAppChanged(frontmost_app) => {
                self.frontmost_app = Some(frontmost_app);
            }
            RuntimeNotification::HookDrain => self.hook_drain = true,
            RuntimeNotification::SafetyResync => self.safety_resync = true,
            RuntimeNotification::RefreshAppDiscovery => self.refresh_app_discovery = true,
            RuntimeNotification::RecordDebugEvent { kind, message } => {
                self.debug_events.push((kind, message));
            }
        }
    }
}

struct ThreadStop {
    flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ThreadStop {
    fn new(flag: Arc<AtomicBool>, handle: JoinHandle<()>) -> Self {
        Self {
            flag,
            handle: Some(handle),
        }
    }

    fn stop(&mut self) {
        self.flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub struct RuntimeService {
    tx: mpsc::Sender<RuntimeMessage>,
    worker: Mutex<Option<JoinHandle<()>>>,
    background_threads: Mutex<Vec<ThreadStop>>,
    emitter: Mutex<Option<JoinHandle<()>>>,
    background_updates_rx: Mutex<Option<mpsc::Receiver<RuntimeBackgroundUpdate>>>,
}

impl RuntimeService {
    pub fn new(config_path: Option<std::path::PathBuf>) -> Self {
        let runtime = AppRuntime::new(config_path);
        Self::from_runtime(runtime)
    }

    pub(crate) fn from_runtime(runtime: AppRuntime) -> Self {
        let (tx, rx) = mpsc::channel::<RuntimeMessage>();
        let (background_updates_tx, background_updates_rx) = mpsc::channel();

        let worker = thread::Builder::new()
            .name("mouser-runtime".to_string())
            .spawn(move || run_runtime_service(runtime, rx, background_updates_tx))
            .expect("failed to spawn runtime service");

        Self {
            tx,
            worker: Mutex::new(Some(worker)),
            background_threads: Mutex::new(Vec::new()),
            emitter: Mutex::new(None),
            background_updates_rx: Mutex::new(Some(background_updates_rx)),
        }
    }

    pub fn attach_listener<F>(&self, listener: F) -> RuntimeResult<()>
    where
        F: Fn(RuntimeBackgroundUpdate) + Send + 'static,
    {
        let receiver = self
            .background_updates_rx
            .lock()
            .map_err(|_| RuntimeError::StateUnavailable)?
            .take()
            .ok_or_else(|| {
                RuntimeError::operation("attach_listener", "runtime listener is already attached")
            })?;

        let handle = thread::Builder::new()
            .name("mouser-runtime-events".to_string())
            .spawn(move || {
                while let Ok(update) = receiver.recv() {
                    listener(update);
                }
            })
            .map_err(|error| RuntimeError::operation("attach_listener", error.to_string()))?;

        *self
            .emitter
            .lock()
            .map_err(|_| RuntimeError::StateUnavailable)? = Some(handle);
        Ok(())
    }

    pub fn start_background(&self) -> RuntimeResult<()> {
        self.spawn_startup_sync();
        self.spawn_periodic_notification(RuntimeNotification::HookDrain, HOOK_DRAIN_INTERVAL)?;
        self.spawn_periodic_notification(
            RuntimeNotification::SafetyResync,
            SAFETY_RESYNC_INTERVAL,
        )?;
        self.spawn_app_discovery_start()?;

        #[cfg(target_os = "windows")]
        self.start_device_polling()?;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        self.start_linux_background()?;

        Ok(())
    }

    pub fn notifier(&self) -> RuntimeNotifier {
        RuntimeNotifier {
            tx: self.tx.clone(),
        }
    }

    pub fn bootstrap_load(&self) -> RuntimeResult<BootstrapPayload> {
        match self.request(RuntimeRequest::BootstrapLoad)? {
            RuntimeResponse::Bootstrap(payload) => Ok(*payload),
            _ => Err(RuntimeError::operation(
                "bootstrap_load",
                "unexpected runtime bootstrap response",
            )),
        }
    }

    pub fn config_get(&self) -> RuntimeResult<AppConfig> {
        match self.request(RuntimeRequest::ConfigGet)? {
            RuntimeResponse::Config(config) => Ok(*config),
            _ => Err(RuntimeError::operation(
                "config_get",
                "unexpected runtime config response",
            )),
        }
    }

    pub fn devices_list(&self) -> RuntimeResult<Vec<DeviceInfo>> {
        match self.request(RuntimeRequest::DevicesList)? {
            RuntimeResponse::Devices(devices) => Ok(devices),
            _ => Err(RuntimeError::operation(
                "devices_list",
                "unexpected runtime devices response",
            )),
        }
    }

    pub fn config_save(
        &self,
        config: AppConfig,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::ConfigSave(config))? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "config_save",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn app_settings_update(
        &self,
        settings: Settings,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::AppSettingsUpdate(settings))? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "app_settings_update",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn device_defaults_update(
        &self,
        settings: DeviceSettings,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::DeviceDefaultsUpdate(settings))? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "device_defaults_update",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn profiles_create(
        &self,
        profile: Profile,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::ProfilesCreate(profile))? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "profiles_create",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn profiles_update(
        &self,
        profile: Profile,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::ProfilesUpdate(profile))? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "profiles_update",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn profiles_delete(
        &self,
        profile_id: String,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::ProfilesDelete(profile_id))? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "profiles_delete",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn devices_update_settings(
        &self,
        device_key: String,
        settings: DeviceSettings,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::DevicesUpdateSettings {
            device_key,
            settings,
        })? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "devices_update_settings",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn devices_update_profile(
        &self,
        device_key: String,
        profile_id: Option<String>,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::DevicesUpdateProfile {
            device_key,
            profile_id,
        })? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "devices_update_profile",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn devices_update_nickname(
        &self,
        device_key: String,
        nickname: Option<String>,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::DevicesUpdateNickname {
            device_key,
            nickname,
        })? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "devices_update_nickname",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn devices_reset_to_factory(
        &self,
        device_key: String,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::DevicesResetToFactory(device_key))? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "devices_reset_to_factory",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn devices_add(
        &self,
        model_key: String,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::DevicesAdd(model_key))? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "devices_add",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn devices_remove(
        &self,
        device_key: String,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::DevicesRemove(device_key))? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "devices_remove",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn devices_select(
        &self,
        device_key: String,
    ) -> RuntimeResult<RuntimeMutationResult<EngineSnapshot>> {
        match self.request(RuntimeRequest::DevicesSelect(device_key))? {
            RuntimeResponse::Engine(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "devices_select",
                "unexpected runtime engine response",
            )),
        }
    }

    pub fn import_legacy_config(
        &self,
        source_path: Option<String>,
        raw_json: Option<String>,
    ) -> RuntimeResult<RuntimeMutationResult<LegacyImportReport>> {
        match self.request(RuntimeRequest::ImportLegacyConfig {
            source_path,
            raw_json,
        })? {
            RuntimeResponse::Import(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "import_legacy_config",
                "unexpected runtime import response",
            )),
        }
    }

    pub fn debug_clear_log(&self) -> RuntimeResult<RuntimeMutationResult<EngineSnapshot>> {
        match self.request(RuntimeRequest::DebugClearLog)? {
            RuntimeResponse::Engine(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "debug_clear_log",
                "unexpected runtime engine response",
            )),
        }
    }

    pub fn set_enabled(
        &self,
        enabled: bool,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::SetEnabled(enabled))? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "set_enabled",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn set_debug_mode(
        &self,
        enabled: bool,
    ) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::SetDebugMode(enabled))? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "set_debug_mode",
                "unexpected runtime payload response",
            )),
        }
    }

    pub fn app_discovery_refresh(&self) -> RuntimeResult<RuntimeMutationResult<BootstrapPayload>> {
        match self.request(RuntimeRequest::RefreshAppDiscovery)? {
            RuntimeResponse::Payload(result) => Ok(*result),
            _ => Err(RuntimeError::operation(
                "app_discovery_refresh",
                "unexpected runtime payload response",
            )),
        }
    }

    #[cfg(test)]
    pub(crate) fn enqueue_notification(
        &self,
        notification: RuntimeNotification,
    ) -> RuntimeResult<()> {
        self.notifier().notify(notification)
    }

    fn request(&self, request: RuntimeRequest) -> RuntimeResult<RuntimeResponse> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.send_message(RuntimeMessage::Request(request, reply_tx))?;
        reply_rx
            .recv()
            .map_err(|_| RuntimeError::StateUnavailable)?
    }

    fn send_message(&self, message: RuntimeMessage) -> RuntimeResult<()> {
        self.tx
            .send(message)
            .map_err(|_| RuntimeError::StateUnavailable)
    }

    fn spawn_startup_sync(&self) {
        let _ = self.send_message(RuntimeMessage::Notification(
            RuntimeNotification::StartupSync,
        ));
    }

    fn spawn_app_discovery_start(&self) -> RuntimeResult<()> {
        self.send_message(RuntimeMessage::Notification(
            RuntimeNotification::RefreshAppDiscovery,
        ))
    }

    fn spawn_periodic_notification(
        &self,
        notification: RuntimeNotification,
        interval: Duration,
    ) -> RuntimeResult<()> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let tx = self.tx.clone();
        let handle = thread::Builder::new()
            .name("mouser-runtime-periodic".to_string())
            .spawn(move || loop {
                if worker_stop.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(interval);
                if worker_stop.load(Ordering::SeqCst) {
                    break;
                }
                if tx
                    .send(RuntimeMessage::Notification(notification.clone()))
                    .is_err()
                {
                    break;
                }
            })
            .map_err(|error| {
                RuntimeError::operation("spawn_periodic_notification", error.to_string())
            })?;

        self.background_threads
            .lock()
            .map_err(|_| RuntimeError::StateUnavailable)?
            .push(ThreadStop::new(stop, handle));
        Ok(())
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) fn start_focus_fallback(&self) -> RuntimeResult<()> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let tx = self.tx.clone();
        let handle = thread::Builder::new()
            .name("mouser-focus-fallback".to_string())
            .spawn(move || loop {
                if worker_stop.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(FOCUS_FALLBACK_INTERVAL);
                if worker_stop.load(Ordering::SeqCst) {
                    break;
                }
                if tx
                    .send(RuntimeMessage::Notification(
                        RuntimeNotification::SafetyResync,
                    ))
                    .is_err()
                {
                    break;
                }
            })
            .map_err(|error| RuntimeError::operation("start_focus_fallback", error.to_string()))?;

        self.background_threads
            .lock()
            .map_err(|_| RuntimeError::StateUnavailable)?
            .push(ThreadStop::new(stop, handle));
        Ok(())
    }

    pub(crate) fn start_device_polling(&self) -> RuntimeResult<()> {
        self.spawn_periodic_notification(
            RuntimeNotification::DevicesChanged,
            Duration::from_secs(5),
        )
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn start_linux_background(&self) -> RuntimeResult<()> {
        self.spawn_periodic_notification(
            RuntimeNotification::SafetyResync,
            Duration::from_millis(900),
        )
    }
}

impl Drop for RuntimeService {
    fn drop(&mut self) {
        let _ = self.tx.send(RuntimeMessage::Shutdown);

        if let Ok(mut threads) = self.background_threads.lock() {
            for thread in threads.iter_mut() {
                thread.stop();
            }
            threads.clear();
        }

        if let Ok(mut worker) = self.worker.lock() {
            if let Some(handle) = worker.take() {
                let _ = handle.join();
            }
        }

        if let Ok(mut emitter) = self.emitter.lock() {
            if let Some(handle) = emitter.take() {
                let _ = handle.join();
            }
        }
    }
}

fn run_runtime_service(
    mut runtime: AppRuntime,
    rx: mpsc::Receiver<RuntimeMessage>,
    background_updates_tx: mpsc::Sender<RuntimeBackgroundUpdate>,
) {
    let mut deferred = None;

    loop {
        let message = if let Some(message) = deferred.take() {
            message
        } else {
            match rx.recv() {
                Ok(message) => message,
                Err(_) => break,
            }
        };

        match message {
            RuntimeMessage::Request(request, reply_tx) => {
                let response = handle_request(&mut runtime, request, &background_updates_tx);
                let _ = reply_tx.send(response);
            }
            RuntimeMessage::Notification(notification) => {
                let mut pending = PendingNotifications::default();
                pending.push(notification);

                while let Ok(next) = rx.try_recv() {
                    match next {
                        RuntimeMessage::Notification(notification) => pending.push(notification),
                        other => {
                            deferred = Some(other);
                            break;
                        }
                    }
                }

                for update in handle_notifications(&mut runtime, pending) {
                    let _ = background_updates_tx.send(update);
                }
            }
            RuntimeMessage::Shutdown => break,
        }
    }
}

fn handle_request(
    runtime: &mut AppRuntime,
    request: RuntimeRequest,
    background_updates_tx: &mpsc::Sender<RuntimeBackgroundUpdate>,
) -> RuntimeResult<RuntimeResponse> {
    match request {
        RuntimeRequest::BootstrapLoad => Ok(RuntimeResponse::Bootstrap(Box::new(
            runtime.bootstrap_payload(),
        ))),
        RuntimeRequest::ConfigGet => Ok(RuntimeResponse::Config(Box::new(runtime.config()))),
        RuntimeRequest::DevicesList => Ok(RuntimeResponse::Devices(runtime.devices())),
        RuntimeRequest::ConfigSave(config) => {
            Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
                runtime,
                |runtime| runtime.save_config(config),
                |runtime| runtime.bootstrap_payload(),
            )?)))
        }
        RuntimeRequest::AppSettingsUpdate(settings) => {
            Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
                runtime,
                |runtime| runtime.update_app_settings(settings),
                |runtime| runtime.bootstrap_payload(),
            )?)))
        }
        RuntimeRequest::DeviceDefaultsUpdate(settings) => {
            Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
                runtime,
                |runtime| runtime.update_device_defaults(settings),
                |runtime| runtime.bootstrap_payload(),
            )?)))
        }
        RuntimeRequest::ProfilesCreate(profile) => {
            Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
                runtime,
                |runtime| runtime.create_profile(profile),
                |runtime| runtime.bootstrap_payload(),
            )?)))
        }
        RuntimeRequest::ProfilesUpdate(profile) => {
            Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
                runtime,
                |runtime| runtime.update_profile(profile),
                |runtime| runtime.bootstrap_payload(),
            )?)))
        }
        RuntimeRequest::ProfilesDelete(profile_id) => {
            Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
                runtime,
                |runtime| runtime.delete_profile(&profile_id),
                |runtime| runtime.bootstrap_payload(),
            )?)))
        }
        RuntimeRequest::DevicesUpdateSettings {
            device_key,
            settings,
        } => Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
            runtime,
            |runtime| runtime.update_managed_device_settings(&device_key, settings),
            |runtime| runtime.bootstrap_payload(),
        )?))),
        RuntimeRequest::DevicesUpdateProfile {
            device_key,
            profile_id,
        } => Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
            runtime,
            |runtime| runtime.update_managed_device_profile(&device_key, profile_id),
            |runtime| runtime.bootstrap_payload(),
        )?))),
        RuntimeRequest::DevicesUpdateNickname {
            device_key,
            nickname,
        } => Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
            runtime,
            |runtime| runtime.update_managed_device_nickname(&device_key, nickname),
            |runtime| runtime.bootstrap_payload(),
        )?))),
        RuntimeRequest::DevicesResetToFactory(device_key) => {
            Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
                runtime,
                |runtime| runtime.reset_managed_device_to_factory_defaults(&device_key),
                |runtime| runtime.bootstrap_payload(),
            )?)))
        }
        RuntimeRequest::DevicesAdd(model_key) => {
            Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
                runtime,
                |runtime| runtime.add_managed_device(&model_key).map(|_| ()),
                |runtime| runtime.bootstrap_payload(),
            )?)))
        }
        RuntimeRequest::DevicesRemove(device_key) => {
            Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
                runtime,
                |runtime| runtime.remove_managed_device(&device_key),
                |runtime| runtime.bootstrap_payload(),
            )?)))
        }
        RuntimeRequest::DevicesSelect(device_key) => {
            Ok(RuntimeResponse::Engine(Box::new(capture_mutation(
                runtime,
                |runtime| {
                    runtime.select_device(&device_key);
                    Ok(())
                },
                |runtime| runtime.engine_snapshot(),
            )?)))
        }
        RuntimeRequest::ImportLegacyConfig {
            source_path,
            raw_json,
        } => {
            let report = import_legacy_payload(ImportSource {
                source_path,
                raw_json,
            })
            .map_err(|error| RuntimeError::LegacyImport {
                message: error.to_string(),
            })?;
            let imported_config = report.config.clone();
            Ok(RuntimeResponse::Import(Box::new(capture_mutation(
                runtime,
                |runtime| runtime.apply_imported_config(imported_config),
                |_| report,
            )?)))
        }
        RuntimeRequest::DebugClearLog => Ok(RuntimeResponse::Engine(Box::new(capture_mutation(
            runtime,
            |runtime| {
                runtime.clear_debug_log();
                Ok(())
            },
            |runtime| runtime.engine_snapshot(),
        )?))),
        RuntimeRequest::SetEnabled(enabled) => {
            Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
                runtime,
                |runtime| {
                    runtime.set_enabled(enabled);
                    Ok(())
                },
                |runtime| runtime.bootstrap_payload(),
            )?)))
        }
        RuntimeRequest::SetDebugMode(enabled) => {
            Ok(RuntimeResponse::Payload(Box::new(capture_mutation(
                runtime,
                |runtime| runtime.set_debug_mode(enabled),
                |runtime| runtime.bootstrap_payload(),
            )?)))
        }
        RuntimeRequest::RefreshAppDiscovery => {
            if !runtime.app_discovery.scanning {
                let scanning_started = capture_mutation(
                    runtime,
                    |runtime| {
                        runtime.app_discovery.scanning = true;
                        Ok(())
                    },
                    |runtime| runtime.bootstrap_payload(),
                )?;
                let _ = background_updates_tx.send(RuntimeBackgroundUpdate {
                    payload: Some(scanning_started.payload),
                    debug_events: scanning_started.debug_events,
                    app_discovery_changed: scanning_started.app_discovery_changed,
                    device_routing_event: scanning_started.device_routing_event,
                });
            }

            let result = capture_mutation(
                runtime,
                |runtime| {
                    runtime.refresh_app_discovery();
                    runtime.app_discovery.scanning = false;
                    Ok(())
                },
                |runtime| runtime.bootstrap_payload(),
            )?;
            Ok(RuntimeResponse::Payload(Box::new(result)))
        }
    }
}

fn handle_notifications(
    runtime: &mut AppRuntime,
    pending: PendingNotifications,
) -> Vec<RuntimeBackgroundUpdate> {
    let mut updates = Vec::new();

    if pending.startup_sync {
        let update = capture_mutation(
            runtime,
            |runtime| {
                runtime.sync_hook_backend();
                Ok(())
            },
            |_| (),
        )
        .expect("startup sync should not fail");
        updates.push(RuntimeBackgroundUpdate {
            payload: Some(runtime.bootstrap_payload()),
            debug_events: update.debug_events,
            app_discovery_changed: false,
            device_routing_event: update.device_routing_event,
        });
    }

    for (kind, message) in pending.debug_events {
        let update = capture_mutation(
            runtime,
            |runtime| {
                runtime.record_debug_event(kind, message);
                Ok(())
            },
            |_| (),
        )
        .expect("recording debug events should not fail");
        updates.push(RuntimeBackgroundUpdate {
            payload: None,
            debug_events: update.debug_events,
            app_discovery_changed: false,
            device_routing_event: update.device_routing_event,
        });
    }

    if pending.refresh_app_discovery {
        if !runtime.app_discovery.scanning {
            let scanning_started = capture_mutation(
                runtime,
                |runtime| {
                    runtime.app_discovery.scanning = true;
                    Ok(())
                },
                |_| (),
            )
            .expect("marking discovery as scanning should not fail");
            updates.push(RuntimeBackgroundUpdate {
                payload: Some(runtime.bootstrap_payload()),
                debug_events: scanning_started.debug_events,
                app_discovery_changed: scanning_started.app_discovery_changed,
                device_routing_event: scanning_started.device_routing_event,
            });
        }

        let completed = capture_mutation(
            runtime,
            |runtime| {
                runtime.refresh_app_discovery();
                runtime.app_discovery.scanning = false;
                Ok(())
            },
            |_| (),
        )
        .expect("refreshing app discovery should not fail");
        updates.push(RuntimeBackgroundUpdate {
            payload: Some(runtime.bootstrap_payload()),
            debug_events: completed.debug_events,
            app_discovery_changed: true,
            device_routing_event: completed.device_routing_event,
        });
    }

    if pending.safety_resync {
        let (hid_backend, app_focus_backend, hook_backend) = runtime.poll_backends();
        let devices = Some(hid_backend.list_devices());
        let frontmost_app = Some(app_focus_backend.current_frontmost_app());
        let hook_events = hook_backend.drain_events();
        let update = capture_runtime_update(runtime, |runtime| {
            runtime.apply_runtime_updates(devices, frontmost_app, hook_events)
        });
        if update.payload.is_some()
            || update.device_routing_event.is_some()
            || !update.debug_events.is_empty()
        {
            updates.push(update);
        }
    } else {
        if pending.devices_changed {
            let (hid_backend, _app_focus_backend, hook_backend) = runtime.poll_backends();
            let devices = Some(hid_backend.list_devices());
            let update = capture_runtime_update(runtime, |runtime| {
                runtime.apply_runtime_updates(devices, None, hook_backend.drain_events())
            });
            if update.payload.is_some()
                || update.device_routing_event.is_some()
                || !update.debug_events.is_empty()
            {
                updates.push(update);
            }
        }

        if let Some(frontmost_app) = pending.frontmost_app {
            let (_hid_backend, _app_focus_backend, hook_backend) = runtime.poll_backends();
            let update = capture_runtime_update(runtime, |runtime| {
                runtime.apply_runtime_updates(
                    None,
                    Some(Ok(frontmost_app)),
                    hook_backend.drain_events(),
                )
            });
            if update.payload.is_some()
                || update.device_routing_event.is_some()
                || !update.debug_events.is_empty()
            {
                updates.push(update);
            }
        }

        if pending.hook_drain {
            let (_hid_backend, _app_focus_backend, hook_backend) = runtime.poll_backends();
            let update = capture_runtime_update(runtime, |runtime| {
                runtime.apply_runtime_updates(None, None, hook_backend.drain_events())
            });
            if update.payload.is_some()
                || update.device_routing_event.is_some()
                || !update.debug_events.is_empty()
            {
                updates.push(update);
            }
        }
    }

    updates
}

fn capture_mutation<T, R>(
    runtime: &mut AppRuntime,
    mutate: impl FnOnce(&mut AppRuntime) -> RuntimeResult<T>,
    read_result: impl FnOnce(&AppRuntime) -> R,
) -> RuntimeResult<RuntimeMutationResult<R>> {
    let previous_debug_cursor = runtime.debug_event_cursor();
    let previous_app_discovery = runtime.app_discovery_snapshot();
    let previous_device_routing = runtime.device_routing_snapshot();
    let _ = mutate(runtime)?;
    let result = read_result(runtime);
    let payload = runtime.bootstrap_payload();
    let debug_events = runtime.debug_events_since(previous_debug_cursor);
    let app_discovery_changed = payload.app_discovery != previous_app_discovery;
    let device_routing_event = build_device_routing_event(
        &previous_device_routing,
        &payload.engine_snapshot.device_routing,
    );

    Ok(RuntimeMutationResult {
        result,
        payload,
        debug_events,
        app_discovery_changed,
        device_routing_event,
    })
}

fn capture_runtime_update(
    runtime: &mut AppRuntime,
    f: impl FnOnce(&mut AppRuntime) -> RuntimeUpdateEffect,
) -> RuntimeBackgroundUpdate {
    let previous_device_routing = runtime.device_routing_snapshot();
    let effect = f(runtime);
    let payload = effect.payload_changed.then(|| runtime.bootstrap_payload());
    let device_routing_event = payload.as_ref().and_then(|payload| {
        build_device_routing_event(
            &previous_device_routing,
            &payload.engine_snapshot.device_routing,
        )
    });

    RuntimeBackgroundUpdate {
        payload,
        debug_events: effect.debug_events,
        app_discovery_changed: effect.app_discovery_changed,
        device_routing_event,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::Receiver,
        Arc,
    };

    use mouser_core::{
        default_app_discovery_snapshot, default_config, default_device_settings,
        default_profile_bindings, AppDiscoverySource, AppMatcher, AppMatcherKind,
        DeviceFingerprint, DeviceRoutingChangeKind, DeviceSupportLevel, DeviceSupportMatrix,
        InstalledApp, ManagedDevice, Profile,
    };
    use mouser_platform::{
        AppDiscoveryBackend, AppFocusBackend, HidBackend, HidCapabilities, HookBackend,
        HookBackendEvent, HookBackendSettings, HookCapabilities, PlatformError,
        StaticDeviceCatalog,
    };

    struct StateHidBackend {
        list_calls: Arc<AtomicUsize>,
        devices: Arc<Mutex<Vec<DeviceInfo>>>,
    }

    struct CountingHookBackend;
    struct CountingFocusBackend;
    struct CountingDiscoveryBackend;
    struct StateDiscoveryBackend {
        apps: Arc<Mutex<Vec<InstalledApp>>>,
    }

    impl StateHidBackend {
        fn new(list_calls: Arc<AtomicUsize>, devices: Vec<DeviceInfo>) -> Self {
            Self {
                list_calls,
                devices: Arc::new(Mutex::new(devices)),
            }
        }

        fn set_devices(&self, devices: Vec<DeviceInfo>) {
            *self.devices.lock().unwrap() = devices;
        }
    }

    impl StateDiscoveryBackend {
        fn new(apps: Vec<InstalledApp>) -> Self {
            Self {
                apps: Arc::new(Mutex::new(apps)),
            }
        }
    }

    impl HidBackend for StateHidBackend {
        fn backend_id(&self) -> &'static str {
            "counting-hid"
        }

        fn capabilities(&self) -> HidCapabilities {
            HidCapabilities {
                can_enumerate_devices: true,
                can_read_battery: false,
                can_read_dpi: false,
                can_write_dpi: false,
            }
        }

        fn list_devices(&self) -> Result<Vec<DeviceInfo>, PlatformError> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.devices.lock().unwrap().clone())
        }

        fn set_device_dpi(&self, _device_key: &str, _dpi: u16) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    impl HookBackend for CountingHookBackend {
        fn backend_id(&self) -> &'static str {
            "counting-hook"
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
            Ok(())
        }

        fn drain_events(&self) -> Vec<HookBackendEvent> {
            Vec::new()
        }
    }

    impl AppFocusBackend for CountingFocusBackend {
        fn backend_id(&self) -> &'static str {
            "counting-focus"
        }

        fn current_frontmost_app(&self) -> Result<Option<AppIdentity>, PlatformError> {
            Ok(Some(AppIdentity {
                label: Some("VS Code".to_string()),
                executable: Some("code".to_string()),
                ..AppIdentity::default()
            }))
        }
    }

    impl AppDiscoveryBackend for CountingDiscoveryBackend {
        fn backend_id(&self) -> &'static str {
            "counting-discovery"
        }

        fn discover_apps(&self) -> Result<Vec<InstalledApp>, PlatformError> {
            Ok(Vec::new())
        }
    }

    impl AppDiscoveryBackend for StateDiscoveryBackend {
        fn backend_id(&self) -> &'static str {
            "stateful-discovery"
        }

        fn discover_apps(&self) -> Result<Vec<InstalledApp>, PlatformError> {
            Ok(self.apps.lock().unwrap().clone())
        }
    }

    fn live_device(identity_key: Option<&str>) -> DeviceInfo {
        DeviceInfo {
            key: "live-device".to_string(),
            model_key: "mx_master_3s".to_string(),
            display_name: "MX Master 3S".to_string(),
            nickname: None,
            product_id: Some(0xB034),
            product_name: Some("MX Master 3S".to_string()),
            transport: Some("Bluetooth Low Energy".to_string()),
            source: Some("hidapi".to_string()),
            ui_layout: "mx_master".to_string(),
            image_asset: "/assets/mouse.png".to_string(),
            supported_controls: Vec::new(),
            controls: Vec::new(),
            support: DeviceSupportMatrix {
                level: DeviceSupportLevel::Experimental,
                supports_battery_status: false,
                supports_dpi_configuration: false,
                has_interactive_layout: false,
                notes: Vec::new(),
            },
            gesture_cids: vec![0x00C3, 0x00D7],
            dpi_min: 200,
            dpi_max: 8000,
            dpi_inferred: false,
            dpi_source_kind: None,
            connected: true,
            battery: None,
            battery_level: None,
            current_dpi: 1000,
            fingerprint: DeviceFingerprint {
                identity_key: identity_key.map(str::to_string),
                ..DeviceFingerprint::default()
            },
        }
    }

    fn managed_device_for_live(live: &DeviceInfo) -> ManagedDevice {
        ManagedDevice {
            id: "managed-mx-master".to_string(),
            model_key: live.model_key.clone(),
            display_name: live.display_name.clone(),
            nickname: None,
            profile_id: None,
            identity_key: live.fingerprint.identity_key.clone(),
            settings: default_device_settings(),
            created_at_ms: super::super::now_ms(),
            last_seen_at_ms: None,
            last_seen_transport: live.transport.clone(),
        }
    }

    fn test_runtime_with_backends(
        config: AppConfig,
        hid_backend: Arc<dyn HidBackend>,
        app_focus_backend: Arc<dyn AppFocusBackend>,
        app_discovery_backend: Box<dyn AppDiscoveryBackend>,
    ) -> AppRuntime {
        let mut runtime = AppRuntime {
            catalog: StaticDeviceCatalog::new(),
            config_store: Box::new(crate::config::JsonConfigStore::new(
                std::env::temp_dir().join(format!(
                    "mouser-service-test-{}.json",
                    super::super::now_ms()
                )),
            )),
            hid_backend,
            hook_backend: Arc::new(CountingHookBackend),
            app_focus_backend,
            app_discovery_backend,
            resolved_profile_id: config.active_profile_id.clone(),
            config,
            detected_devices: Vec::new(),
            selected_device_key: None,
            frontmost_app: None,
            app_discovery: default_app_discovery_snapshot(),
            enabled: true,
            runtime_health: mouser_core::RuntimeHealth::default(),
            debug_log: std::collections::VecDeque::new(),
            next_debug_seq: 0,
        };
        runtime.ensure_selected_device();
        runtime.sync_active_profile();
        runtime
    }

    fn test_runtime(list_calls: Arc<AtomicUsize>) -> AppRuntime {
        test_runtime_with_backends(
            default_config(),
            Arc::new(StateHidBackend::new(list_calls, vec![live_device(None)])),
            Arc::new(CountingFocusBackend),
            Box::new(CountingDiscoveryBackend),
        )
    }

    fn recv_update(rx: &Receiver<RuntimeBackgroundUpdate>) -> RuntimeBackgroundUpdate {
        rx.recv_timeout(Duration::from_secs(1))
            .expect("expected runtime background update")
    }

    #[test]
    fn startup_is_non_blocking_until_background_starts() {
        let list_calls = Arc::new(AtomicUsize::new(0));
        let service = RuntimeService::from_runtime(test_runtime(Arc::clone(&list_calls)));

        let payload = service.bootstrap_load().unwrap();
        assert!(payload.engine_snapshot.detected_devices.is_empty());
        assert_eq!(list_calls.load(Ordering::SeqCst), 0);

        service
            .enqueue_notification(RuntimeNotification::SafetyResync)
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert!(list_calls.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn notification_bursts_still_apply_latest_frontmost_app() {
        let list_calls = Arc::new(AtomicUsize::new(0));
        let service = RuntimeService::from_runtime(test_runtime(list_calls));

        service
            .enqueue_notification(RuntimeNotification::FrontmostAppChanged(Some(
                AppIdentity {
                    label: Some("App One".to_string()),
                    executable: Some("one".to_string()),
                    ..AppIdentity::default()
                },
            )))
            .unwrap();
        service
            .enqueue_notification(RuntimeNotification::FrontmostAppChanged(Some(
                AppIdentity {
                    label: Some("App Two".to_string()),
                    executable: Some("two".to_string()),
                    ..AppIdentity::default()
                },
            )))
            .unwrap();

        std::thread::sleep(Duration::from_millis(50));
        let payload = service.bootstrap_load().unwrap();
        assert_eq!(
            payload
                .engine_snapshot
                .engine_status
                .frontmost_app
                .as_deref(),
            Some("App Two")
        );
    }

    #[test]
    fn refresh_app_discovery_emits_scanning_start_then_completion() {
        let list_calls = Arc::new(AtomicUsize::new(0));
        let discovery_backend = StateDiscoveryBackend::new(vec![InstalledApp {
            identity: AppIdentity {
                label: Some("Local Tool".to_string()),
                executable: Some("tool".to_string()),
                executable_path: Some("/Applications/Tool.app/Contents/MacOS/tool".to_string()),
                ..AppIdentity::default()
            },
            source_kinds: vec![AppDiscoverySource::DesktopEntry],
            source_path: Some("/Applications/Tool.app".to_string()),
        }]);
        let service = RuntimeService::from_runtime(test_runtime_with_backends(
            default_config(),
            Arc::new(StateHidBackend::new(list_calls, Vec::new())),
            Arc::new(CountingFocusBackend),
            Box::new(discovery_backend),
        ));
        let (tx, rx) = mpsc::channel();
        service
            .attach_listener(move |update| {
                let _ = tx.send(update);
            })
            .unwrap();

        service
            .enqueue_notification(RuntimeNotification::RefreshAppDiscovery)
            .unwrap();

        let started = recv_update(&rx);
        let completed = recv_update(&rx);
        let started_payload = started.payload.expect("expected scanning-start payload");
        let completed_payload = completed
            .payload
            .expect("expected app-discovery completion payload");

        assert!(started_payload.app_discovery.scanning);
        assert!(!completed_payload.app_discovery.scanning);
        assert!(completed.app_discovery_changed);
        assert_eq!(completed_payload.app_discovery.browse_apps.len(), 1);
    }

    #[test]
    fn device_notifications_emit_connected_and_disconnected_routing_events() {
        let list_calls = Arc::new(AtomicUsize::new(0));
        let hid_backend = Arc::new(StateHidBackend::new(Arc::clone(&list_calls), Vec::new()));
        let live = live_device(Some("identity:mx-master"));
        let mut config = default_config();
        config.managed_devices = vec![managed_device_for_live(&live)];
        config.ensure_invariants();

        let service = RuntimeService::from_runtime(test_runtime_with_backends(
            config,
            hid_backend.clone(),
            Arc::new(CountingFocusBackend),
            Box::new(CountingDiscoveryBackend),
        ));
        let (tx, rx) = mpsc::channel();
        service
            .attach_listener(move |update| {
                let _ = tx.send(update);
            })
            .unwrap();

        hid_backend.set_devices(vec![live.clone()]);
        service
            .enqueue_notification(RuntimeNotification::DevicesChanged)
            .unwrap();
        let connected = recv_update(&rx);
        let connected_payload = connected
            .payload
            .expect("expected connected routing payload");
        let connected_event = connected
            .device_routing_event
            .expect("expected connected routing event");
        assert_eq!(connected_payload.engine_snapshot.detected_devices.len(), 1);
        assert!(connected_event
            .changes
            .iter()
            .any(|change| change.kind == DeviceRoutingChangeKind::Connected));

        hid_backend.set_devices(Vec::new());
        service
            .enqueue_notification(RuntimeNotification::DevicesChanged)
            .unwrap();
        let disconnected = recv_update(&rx);
        let disconnected_event = disconnected
            .device_routing_event
            .expect("expected disconnected routing event");
        assert!(disconnected_event
            .changes
            .iter()
            .any(|change| change.kind == DeviceRoutingChangeKind::Disconnected));
    }

    #[test]
    fn frontmost_app_change_updates_resolved_profile_for_connected_device() {
        let list_calls = Arc::new(AtomicUsize::new(0));
        let live = live_device(Some("identity:mx-master"));
        let managed = managed_device_for_live(&live);
        let mut config = default_config();
        config.profiles.push(Profile {
            id: "code".to_string(),
            label: "Code".to_string(),
            app_matchers: vec![AppMatcher {
                kind: AppMatcherKind::Executable,
                value: "Code.exe".to_string(),
            }],
            bindings: default_profile_bindings(),
        });
        config.managed_devices = vec![managed.clone()];
        config.ensure_invariants();

        let mut runtime = test_runtime_with_backends(
            config,
            Arc::new(StateHidBackend::new(list_calls, vec![live.clone()])),
            Arc::new(CountingFocusBackend),
            Box::new(CountingDiscoveryBackend),
        );
        runtime.detected_devices = vec![live];
        runtime.selected_device_key = Some(managed.id.clone());
        runtime.sync_active_profile();

        let service = RuntimeService::from_runtime(runtime);
        let (tx, rx) = mpsc::channel();
        service
            .attach_listener(move |update| {
                let _ = tx.send(update);
            })
            .unwrap();

        service
            .enqueue_notification(RuntimeNotification::FrontmostAppChanged(Some(
                AppIdentity {
                    label: Some("Visual Studio Code".to_string()),
                    executable: Some("Code.exe".to_string()),
                    ..AppIdentity::default()
                },
            )))
            .unwrap();

        let update = recv_update(&rx);
        let payload = update.payload.expect("expected profile update payload");
        let routing_event = update
            .device_routing_event
            .expect("expected resolved-profile routing event");

        assert_eq!(
            payload.engine_snapshot.engine_status.active_profile_id,
            "code"
        );
        assert!(routing_event.changes.iter().any(|change| {
            change.kind == DeviceRoutingChangeKind::ResolvedProfileChanged
                && change.resolved_profile_id.as_deref() == Some("code")
        }));
    }
}
