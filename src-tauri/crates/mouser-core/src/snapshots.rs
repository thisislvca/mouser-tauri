use crate::{
    AppIdentity, DebugEvent, DeviceInfo, DeviceRoutingSnapshot, EngineSnapshot, EngineStatus,
};

pub struct EngineSnapshotState<'a> {
    pub enabled: bool,
    pub active_profile_id: String,
    pub frontmost_app: Option<&'a AppIdentity>,
    pub debug_mode: bool,
    pub debug_log: Vec<DebugEvent>,
}

pub fn build_engine_snapshot(
    devices: Vec<DeviceInfo>,
    detected_devices: Vec<DeviceInfo>,
    device_routing: DeviceRoutingSnapshot,
    active_device_key: Option<String>,
    active_device: Option<DeviceInfo>,
    state: EngineSnapshotState<'_>,
) -> EngineSnapshot {
    EngineSnapshot {
        devices,
        detected_devices,
        device_routing,
        active_device_key: active_device_key.clone(),
        active_device: active_device.clone(),
        engine_status: EngineStatus {
            enabled: state.enabled,
            connected: active_device
                .as_ref()
                .is_some_and(|device| device.connected),
            active_profile_id: state.active_profile_id,
            frontmost_app: state.frontmost_app.and_then(AppIdentity::label_or_fallback),
            selected_device_key: active_device_key,
            debug_mode: state.debug_mode,
            debug_log: state.debug_log,
        },
    }
}
