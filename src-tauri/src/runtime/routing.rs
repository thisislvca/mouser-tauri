use std::collections::{BTreeMap, BTreeSet};

use mouser_core::{
    active_device_with_layout, build_managed_device_info, profile_for_supported_controls,
    DeviceAttributionStatus, DeviceInfo, DeviceMatchKind, DeviceRoutingChange,
    DeviceRoutingChangeKind, DeviceRoutingEntry, DeviceRoutingEvent, DeviceRoutingSnapshot,
    DeviceSettings, ManagedDevice, Profile,
};
use mouser_platform::HookDeviceRoute;

use super::AppRuntime;

pub(super) struct DeviceResolution {
    pub(super) assignments: BTreeMap<String, LiveDeviceAssignment>,
    pub(super) managed_devices: Vec<DeviceInfo>,
}

impl DeviceResolution {
    pub(super) fn connected_ids(&self) -> BTreeSet<String> {
        self.assignments.keys().cloned().collect()
    }

    pub(super) fn selected_live_device_index(
        &self,
        selected_device_key: Option<&str>,
    ) -> Option<usize> {
        selected_device_key.and_then(|device_key| {
            self.assignments
                .get(device_key)
                .map(|assignment| assignment.live_index)
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct LiveDeviceAssignment {
    pub(super) live_index: usize,
    pub(super) match_kind: DeviceMatchKind,
}

impl AppRuntime {
    pub(super) fn device_resolution(&self) -> DeviceResolution {
        let assignments = self.matched_live_device_indexes();
        self.device_resolution_with_assignments(assignments)
    }

    pub(super) fn device_resolution_with_assignments(
        &self,
        assignments: BTreeMap<String, LiveDeviceAssignment>,
    ) -> DeviceResolution {
        let managed_devices = self
            .config
            .managed_devices
            .iter()
            .map(|device| {
                let live = assignments
                    .get(&device.id)
                    .and_then(|assignment| self.detected_devices.get(assignment.live_index));
                build_managed_device_info(device, live)
            })
            .collect();

        DeviceResolution {
            assignments,
            managed_devices,
        }
    }

    pub(super) fn active_device_from_resolution(
        &self,
        resolution: &DeviceResolution,
    ) -> Option<DeviceInfo> {
        self.config
            .managed_devices
            .iter()
            .find(|device| Some(device.id.as_str()) == self.selected_device_key.as_deref())
            .and_then(|managed| {
                resolution
                    .managed_devices
                    .iter()
                    .find(|device| device.key == managed.id)
                    .cloned()
                    .map(|device| {
                        active_device_with_layout(
                            device,
                            managed.settings.manual_layout_override.as_deref(),
                            self.catalog.layouts(),
                        )
                    })
            })
    }

    pub(super) fn device_routing_snapshot_from_resolution(
        &self,
        resolution: &DeviceResolution,
    ) -> DeviceRoutingSnapshot {
        let managed_by_id = self
            .config
            .managed_devices
            .iter()
            .map(|device| (device.id.as_str(), device))
            .collect::<BTreeMap<_, _>>();

        let assigned_live = resolution
            .assignments
            .iter()
            .map(|(managed_device_key, assignment)| {
                (
                    assignment.live_index,
                    (managed_device_key.as_str(), assignment.match_kind),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let connected_model_counts =
            connected_model_counts_for_assignments(&resolution.assignments, &self.detected_devices);

        let mut entries = self
            .detected_devices
            .iter()
            .enumerate()
            .map(|(live_index, live)| {
                let managed = assigned_live
                    .get(&live_index)
                    .and_then(|(managed_device_key, _)| managed_by_id.get(managed_device_key))
                    .copied();
                let device_profile_id = managed.and_then(|device| device.profile_id.clone());
                let resolved_profile_id = managed.map(|device| {
                    self.config.resolved_profile_id(
                        device.profile_id.as_deref(),
                        self.frontmost_app.as_ref(),
                    )
                });
                let managed_device_key = managed.map(|device| device.id.clone());
                let managed_display_name = managed.map(|device| device.display_name.clone());
                let match_kind = assigned_live
                    .get(&live_index)
                    .map(|(_, match_kind)| *match_kind)
                    .unwrap_or(DeviceMatchKind::Unmanaged);
                let filtered_profile = resolved_profile_id
                    .as_deref()
                    .and_then(|profile_id| self.config.profile_by_id(profile_id))
                    .map(|profile| {
                        profile_for_supported_controls(profile, &live.supported_controls)
                    });

                DeviceRoutingEntry {
                    live_device_key: live.key.clone(),
                    live_model_key: live.model_key.clone(),
                    live_display_name: live.display_name.clone(),
                    live_identity_key: live.fingerprint.identity_key.clone(),
                    managed_device_key: managed_device_key.clone(),
                    managed_display_name,
                    device_profile_id,
                    resolved_profile_id,
                    match_kind,
                    is_active_target: managed_device_key.as_deref().is_some_and(|device_key| {
                        Some(device_key) == self.selected_device_key.as_deref()
                    }),
                    hook_eligible: managed.is_some_and(|device| {
                        device_hook_eligible(device, filtered_profile.as_ref())
                    }),
                    attribution_status: attribution_status_for_match(
                        match_kind,
                        connected_model_counts
                            .get(live.model_key.as_str())
                            .copied()
                            .unwrap_or_default(),
                    ),
                    source_hints: source_hints_for_live_device(live),
                }
            })
            .collect::<Vec<_>>();

        entries.sort_by(|left, right| left.live_device_key.cmp(&right.live_device_key));
        DeviceRoutingSnapshot { entries }
    }

    pub(super) fn hook_device_routes(&self, resolution: &DeviceResolution) -> Vec<HookDeviceRoute> {
        let connected_model_counts =
            connected_model_counts_for_assignments(&resolution.assignments, &self.detected_devices);
        self.config
            .managed_devices
            .iter()
            .filter_map(|managed| {
                let assignment = resolution.assignments.get(&managed.id)?;
                let live = self.detected_devices.get(assignment.live_index)?.clone();
                if !hook_route_is_authoritative(
                    assignment.match_kind,
                    connected_model_counts
                        .get(live.model_key.as_str())
                        .copied()
                        .unwrap_or_default(),
                ) {
                    return None;
                }
                let resolved_profile_id = self.config.resolved_profile_id(
                    managed.profile_id.as_deref(),
                    self.frontmost_app.as_ref(),
                );
                let profile = self.config.profile_by_id(&resolved_profile_id)?;
                let filtered_profile =
                    profile_for_supported_controls(profile, &live.supported_controls);
                Some(HookDeviceRoute {
                    managed_device_key: managed.id.clone(),
                    resolved_profile_id,
                    live_device: live,
                    bindings: filtered_profile.bindings,
                    device_settings: managed.settings.clone(),
                })
            })
            .collect()
    }

    pub(super) fn matched_live_device_indexes(&self) -> BTreeMap<String, LiveDeviceAssignment> {
        let mut assignments = BTreeMap::new();
        let mut remaining_indexes = (0..self.detected_devices.len()).collect::<Vec<_>>();

        for device in &self.config.managed_devices {
            let Some(identity_key) = normalized_identity_key(device.identity_key.as_deref()) else {
                continue;
            };
            if let Some(position) = remaining_indexes.iter().position(|index| {
                let live = &self.detected_devices[*index];
                live.model_key == device.model_key
                    && normalized_identity_key(live.fingerprint.identity_key.as_deref())
                        == Some(identity_key)
            }) {
                let index = remaining_indexes.remove(position);
                assignments.insert(
                    device.id.clone(),
                    LiveDeviceAssignment {
                        live_index: index,
                        match_kind: DeviceMatchKind::Identity,
                    },
                );
            }
        }

        for device in &self.config.managed_devices {
            if assignments.contains_key(&device.id) {
                continue;
            }
            if let Some(position) = remaining_indexes.iter().position(|index| {
                live_matches_managed_device(device, &self.detected_devices[*index])
            }) {
                let index = remaining_indexes.remove(position);
                assignments.insert(
                    device.id.clone(),
                    LiveDeviceAssignment {
                        live_index: index,
                        match_kind: DeviceMatchKind::ModelFallback,
                    },
                );
            }
        }

        assignments
    }

    pub(super) fn selected_managed_device(&self) -> Option<&ManagedDevice> {
        self.selected_device_key.as_ref().and_then(|device_key| {
            self.config
                .managed_devices
                .iter()
                .find(|device| &device.id == device_key)
        })
    }

    pub(super) fn selected_managed_device_mut(&mut self) -> Option<&mut ManagedDevice> {
        let selected_device_key = self.selected_device_key.clone()?;
        self.config
            .managed_devices
            .iter_mut()
            .find(|device| device.id == selected_device_key)
    }

    pub(super) fn selected_device_settings(&self) -> &DeviceSettings {
        self.selected_managed_device()
            .map(|device| &device.settings)
            .unwrap_or(&self.config.device_defaults)
    }

    pub(super) fn active_profile(&self) -> Option<&Profile> {
        self.config.profile_by_id(&self.resolved_profile_id)
    }
}

pub(crate) fn build_device_routing_event(
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

pub(super) fn normalized_identity_key(value: Option<&str>) -> Option<&str> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

pub(super) fn live_matches_managed_device(managed: &ManagedDevice, live: &DeviceInfo) -> bool {
    if live.model_key != managed.model_key {
        return false;
    }

    match (
        normalized_identity_key(managed.identity_key.as_deref()),
        normalized_identity_key(live.fingerprint.identity_key.as_deref()),
    ) {
        (Some(managed_identity), Some(live_identity)) => managed_identity == live_identity,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

fn device_hook_eligible(managed: &ManagedDevice, profile: Option<&Profile>) -> bool {
    managed.settings.invert_horizontal_scroll
        || managed.settings.invert_vertical_scroll
        || managed.settings.macos_thumb_wheel_simulate_trackpad
        || profile.is_some_and(|profile| {
            profile
                .bindings
                .iter()
                .any(|binding| binding.action_id.as_str() != "none")
        })
}

fn connected_model_counts_for_assignments<'a>(
    assignments: &BTreeMap<String, LiveDeviceAssignment>,
    detected_devices: &'a [DeviceInfo],
) -> BTreeMap<&'a str, usize> {
    assignments
        .values()
        .filter_map(|assignment| detected_devices.get(assignment.live_index))
        .fold(BTreeMap::<&'a str, usize>::new(), |mut counts, live| {
            *counts.entry(live.model_key.as_str()).or_default() += 1;
            counts
        })
}

fn hook_route_is_authoritative(match_kind: DeviceMatchKind, connected_model_count: usize) -> bool {
    matches!(
        attribution_status_for_match(match_kind, connected_model_count),
        DeviceAttributionStatus::Ready | DeviceAttributionStatus::ModelFallback
    )
}

fn attribution_status_for_match(
    match_kind: DeviceMatchKind,
    connected_model_count: usize,
) -> DeviceAttributionStatus {
    match match_kind {
        DeviceMatchKind::Identity => DeviceAttributionStatus::Ready,
        DeviceMatchKind::ModelFallback => {
            if connected_model_count > 1 {
                DeviceAttributionStatus::Ambiguous
            } else {
                DeviceAttributionStatus::ModelFallback
            }
        }
        DeviceMatchKind::Unmanaged => DeviceAttributionStatus::Unmanaged,
    }
}

fn source_hints_for_live_device(live: &DeviceInfo) -> Vec<String> {
    let mut hints = Vec::new();

    if let Some(identity_key) = normalized_identity_key(live.fingerprint.identity_key.as_deref()) {
        hints.push(identity_key.to_string());
    }
    if let Some(serial_number) = normalized_identity_key(live.fingerprint.serial_number.as_deref())
    {
        hints.push(format!("serial:{serial_number}"));
    }
    if let Some(source) = live
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        hints.push(format!("source:{source}"));
    }
    if let Some(transport) = live
        .transport
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        hints.push(format!("transport:{transport}"));
    }
    if let Some(hid_path) = live
        .fingerprint
        .hid_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        hints.push(format!("path:{hid_path}"));
    }
    if let Some(location_id) = live.fingerprint.location_id {
        hints.push(format!("location:{location_id:08x}"));
    }

    hints
}
