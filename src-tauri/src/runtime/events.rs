use mouser_core::{AppIdentity, BootstrapPayload, DebugEvent, DebugEventKind, DeviceRoutingEvent};

#[derive(Debug)]
pub struct RuntimeMutationResult<T> {
    pub result: T,
    pub payload: BootstrapPayload,
    pub debug_events: Vec<DebugEvent>,
    pub app_discovery_changed: bool,
    pub device_routing_event: Option<DeviceRoutingEvent>,
}

#[derive(Debug)]
pub struct RuntimeBackgroundUpdate {
    pub payload: Option<BootstrapPayload>,
    pub debug_events: Vec<DebugEvent>,
    pub app_discovery_changed: bool,
    pub device_routing_event: Option<DeviceRoutingEvent>,
}

#[derive(Debug, Clone)]
pub enum RuntimeNotification {
    StartupSync,
    DevicesChanged,
    FrontmostAppChanged(Option<AppIdentity>),
    HookDrain,
    SafetyResync,
    RefreshAppDiscovery,
    RecordDebugEvent {
        kind: DebugEventKind,
        message: String,
    },
}

pub(crate) struct RuntimeUpdateEffect {
    pub payload_changed: bool,
    pub app_discovery_changed: bool,
    pub debug_events: Vec<DebugEvent>,
}
