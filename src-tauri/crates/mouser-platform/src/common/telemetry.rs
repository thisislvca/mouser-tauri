#![cfg_attr(not(any(target_os = "linux", target_os = "windows")), allow(dead_code))]

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use mouser_core::{DeviceBatteryInfo, DeviceFingerprint};

#[derive(Debug, Clone)]
pub(crate) struct DeviceTelemetryCacheEntry {
    pub current_dpi: Option<u16>,
    pub battery: Option<DeviceBatteryInfo>,
    pub last_battery_probe_at: Instant,
    pub verify_after: Option<Instant>,
    pub connected: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DeviceTelemetrySnapshot {
    pub current_dpi: Option<u16>,
    pub battery: Option<DeviceBatteryInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct TelemetryProbePlan {
    pub probe_dpi: bool,
    pub probe_battery: bool,
    pub cached: DeviceTelemetrySnapshot,
}

pub(crate) fn telemetry_plan(
    cache: &BTreeMap<String, DeviceTelemetryCacheEntry>,
    cache_key: &str,
    now: Instant,
    battery_cache_ttl: Duration,
) -> TelemetryProbePlan {
    let entry = cache.get(cache_key);
    TelemetryProbePlan {
        probe_dpi: should_probe_dpi(entry, now),
        probe_battery: should_probe_battery(entry, now, battery_cache_ttl),
        cached: DeviceTelemetrySnapshot {
            current_dpi: entry.and_then(|entry| entry.current_dpi),
            battery: entry.and_then(|entry| entry.battery.clone()),
        },
    }
}

pub(crate) fn remember_device_telemetry(
    cache: &mut BTreeMap<String, DeviceTelemetryCacheEntry>,
    cache_key: String,
    telemetry: DeviceTelemetrySnapshot,
    plan: &TelemetryProbePlan,
    now: Instant,
    preserve_cached_values: bool,
) {
    let entry = cache.entry(cache_key).or_insert(DeviceTelemetryCacheEntry {
        current_dpi: None,
        battery: None,
        last_battery_probe_at: now,
        verify_after: None,
        connected: true,
    });

    entry.connected = true;
    entry.current_dpi = if preserve_cached_values {
        telemetry
            .current_dpi
            .or(plan.cached.current_dpi)
            .or(entry.current_dpi)
    } else {
        telemetry.current_dpi.or(entry.current_dpi)
    };

    if plan.probe_dpi || preserve_cached_values {
        entry.verify_after = None;
    }

    if plan.probe_battery {
        entry.battery = telemetry.battery.clone().or_else(|| {
            preserve_cached_values
                .then(|| plan.cached.battery.clone())
                .flatten()
        });
        entry.last_battery_probe_at = now;
    } else if preserve_cached_values && entry.battery.is_none() {
        entry.battery = plan.cached.battery.clone();
    }
}

pub(crate) fn note_connected_devices(
    cache: &mut BTreeMap<String, DeviceTelemetryCacheEntry>,
    connected_cache_keys: &BTreeSet<String>,
) {
    for (cache_key, entry) in cache.iter_mut() {
        entry.connected = connected_cache_keys.contains(cache_key);
    }
}

pub(crate) fn note_dpi_write(
    cache: &mut BTreeMap<String, DeviceTelemetryCacheEntry>,
    cache_key: String,
    dpi: u16,
    now: Instant,
    verify_delay: Duration,
) {
    let entry = cache.entry(cache_key).or_insert(DeviceTelemetryCacheEntry {
        current_dpi: None,
        battery: None,
        last_battery_probe_at: now,
        verify_after: None,
        connected: true,
    });
    entry.current_dpi = Some(dpi);
    entry.verify_after = Some(now + verify_delay);
    entry.connected = true;
}

pub(crate) fn telemetry_cache_key_with_fallback(
    product_id: Option<u16>,
    transport: Option<&str>,
    fingerprint: &DeviceFingerprint,
    fallback: impl FnOnce() -> String,
) -> String {
    if let Some(identity_key) = fingerprint
        .identity_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!("identity:{identity_key}");
    }

    if let Some(serial_number) = fingerprint
        .serial_number
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!(
            "serial:{:04x}:{serial_number}",
            product_id.unwrap_or_default()
        );
    }

    let _ = (product_id, transport);
    fallback()
}

fn should_probe_dpi(entry: Option<&DeviceTelemetryCacheEntry>, now: Instant) -> bool {
    let Some(entry) = entry else {
        return true;
    };

    !entry.connected
        || entry.current_dpi.is_none()
        || entry
            .verify_after
            .is_some_and(|verify_after| now >= verify_after)
}

fn should_probe_battery(
    entry: Option<&DeviceTelemetryCacheEntry>,
    now: Instant,
    battery_cache_ttl: Duration,
) -> bool {
    let Some(entry) = entry else {
        return true;
    };

    !entry.connected || now.duration_since(entry.last_battery_probe_at) >= battery_cache_ttl
}

pub(crate) fn should_probe_dpi_entry(
    entry: Option<&DeviceTelemetryCacheEntry>,
    now: Instant,
) -> bool {
    should_probe_dpi(entry, now)
}

pub(crate) fn should_probe_battery_entry(
    entry: Option<&DeviceTelemetryCacheEntry>,
    now: Instant,
    battery_cache_ttl: Duration,
) -> bool {
    should_probe_battery(entry, now, battery_cache_ttl)
}
