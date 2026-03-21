use std::collections::HashMap;

use mouser_core::{DeviceControlCaptureKind, DeviceControlSpec, LogicalControl};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReprogRoute {
    pub control: LogicalControl,
    pub cids: Vec<u16>,
    pub rawxy_enabled: bool,
}

pub(crate) fn action_for(
    bindings: &HashMap<LogicalControl, String>,
    control: LogicalControl,
) -> Option<&str> {
    bindings
        .get(&control)
        .map(String::as_str)
        .filter(|action_id| *action_id != "none")
}

pub(crate) fn handles_control(
    bindings: &HashMap<LogicalControl, String>,
    control: LogicalControl,
) -> bool {
    action_for(bindings, control).is_some()
}

pub(crate) fn gesture_direction_enabled(bindings: &HashMap<LogicalControl, String>) -> bool {
    [
        LogicalControl::GestureLeft,
        LogicalControl::GestureRight,
        LogicalControl::GestureUp,
        LogicalControl::GestureDown,
    ]
    .into_iter()
    .any(|control| handles_control(bindings, control))
}

pub(crate) fn gesture_route(
    device_controls: &[DeviceControlSpec],
    bindings: &HashMap<LogicalControl, String>,
) -> Option<ReprogRoute> {
    let gesture_requested = [
        LogicalControl::GesturePress,
        LogicalControl::GestureLeft,
        LogicalControl::GestureRight,
        LogicalControl::GestureUp,
        LogicalControl::GestureDown,
    ]
    .into_iter()
    .any(|control| handles_control(bindings, control));
    if !gesture_requested {
        return None;
    }

    device_controls.iter().find_map(|control| {
        (control.control == LogicalControl::GesturePress && !control.reprog_cids.is_empty()).then(
            || ReprogRoute {
                control: LogicalControl::GesturePress,
                cids: control.reprog_cids.clone(),
                rawxy_enabled: gesture_direction_enabled(bindings),
            },
        )
    })
}

pub(crate) fn reprog_routes(
    device_controls: &[DeviceControlSpec],
    bindings: &HashMap<LogicalControl, String>,
) -> Vec<ReprogRoute> {
    let mut routes = Vec::new();

    if let Some(route) = gesture_route(device_controls, bindings) {
        routes.push(route);
    }

    for control in device_controls {
        if control.capture_kind != DeviceControlCaptureKind::ReprogButton
            || !handles_control(bindings, control.control)
            || control.reprog_cids.is_empty()
        {
            continue;
        }

        routes.push(ReprogRoute {
            control: control.control,
            cids: control.reprog_cids.clone(),
            rawxy_enabled: false,
        });
    }

    routes
}

pub(crate) fn route_summary(
    managed_device_key: &str,
    resolved_profile_id: &str,
    bindings: &HashMap<LogicalControl, String>,
) -> String {
    let bindings = [
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
            bindings.get(&control).map(String::as_str).unwrap_or("none")
        )
    })
    .collect::<Vec<_>>()
    .join(", ");

    format!("{managed_device_key}:{resolved_profile_id} [{bindings}]")
}
