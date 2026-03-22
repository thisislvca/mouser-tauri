use mouser_core::{
    default_app_catalog, normalize_app_match_value, AppDiscoverySnapshot, AppMatcher,
    AppMatcherKind, CatalogApp, DebugEventKind, DiscoveredApp, InstalledApp,
};

use super::{now_ms, AppRuntime, BackendSlot};
use mouser_core::BackendHealthState;

impl AppRuntime {
    pub fn refresh_app_discovery(&mut self) -> bool {
        let previous = self.app_discovery.clone();
        match self.app_discovery_backend.discover_apps() {
            Ok(installed_apps) => {
                let health_changed = self.mark_backend_health(
                    BackendSlot::Discovery,
                    BackendHealthState::Ready,
                    None,
                );
                self.app_discovery =
                    build_app_discovery_snapshot(&default_app_catalog(), &installed_apps);
                let changed = self.app_discovery != previous;
                if changed {
                    self.push_debug_if_enabled(
                        DebugEventKind::Info,
                        format!(
                            "Discovered {} suggested apps and {} installed apps",
                            self.app_discovery.suggested_apps.len(),
                            self.app_discovery.browse_apps.len()
                        ),
                    );
                }
                changed || health_changed
            }
            Err(error) => {
                let message = format!("App discovery refresh failed: {error}");
                self.push_debug(DebugEventKind::Warning, message.clone());
                self.mark_backend_health(
                    BackendSlot::Discovery,
                    BackendHealthState::Stale,
                    Some(message),
                )
            }
        }
    }
}

fn build_app_discovery_snapshot(
    catalog_apps: &[CatalogApp],
    installed_apps: &[InstalledApp],
) -> AppDiscoverySnapshot {
    let mut suggested_apps = Vec::new();
    let mut browse_apps = Vec::new();

    for installed_app in installed_apps {
        let catalog_match = catalog_apps.iter().find(|catalog_app| {
            catalog_app
                .matchers
                .iter()
                .any(|matcher| installed_app.identity.matches(matcher))
        });
        let discovered_app = discovered_app_from_sources(installed_app, catalog_match);
        if discovered_app.suggested {
            suggested_apps.push(discovered_app.clone());
        }
        browse_apps.push(discovered_app);
    }

    suggested_apps.sort_by(|left, right| left.label.cmp(&right.label));
    browse_apps.sort_by(|left, right| left.label.cmp(&right.label));

    AppDiscoverySnapshot {
        suggested_apps,
        browse_apps,
        last_scan_at_ms: Some(now_ms()),
        scanning: false,
    }
}

fn discovered_app_from_sources(
    installed_app: &InstalledApp,
    catalog_match: Option<&CatalogApp>,
) -> DiscoveredApp {
    let mut matchers = installed_app.identity.preferred_matchers();
    if let Some(catalog_match) = catalog_match {
        merge_matchers(&mut matchers, &catalog_match.matchers);
    }

    DiscoveredApp {
        id: catalog_match
            .map(|catalog_app| catalog_app.id.clone())
            .unwrap_or_else(|| installed_app.identity.stable_id()),
        label: catalog_match
            .map(|catalog_app| catalog_app.label.clone())
            .or_else(|| installed_app.identity.label_or_fallback())
            .unwrap_or_else(|| "Application".to_string()),
        description: installed_app.identity.detail_label(),
        matchers,
        icon_asset: catalog_match.and_then(|catalog_app| catalog_app.icon_asset.clone()),
        source_kinds: installed_app.source_kinds.clone(),
        source_path: installed_app.source_path.clone(),
        suggested: catalog_match.is_some(),
    }
}

fn merge_matchers(target: &mut Vec<AppMatcher>, source: &[AppMatcher]) {
    for matcher in source {
        let normalized = normalize_app_match_value(matcher.kind, &matcher.value);
        if target.iter().any(|existing| {
            existing.kind == matcher.kind
                && normalize_app_match_value(existing.kind, &existing.value) == normalized
        }) {
            continue;
        }
        target.push(matcher.clone());
    }
    target.sort_by(|left, right| matcher_priority(left.kind).cmp(&matcher_priority(right.kind)));
}

fn matcher_priority(kind: AppMatcherKind) -> u8 {
    match kind {
        AppMatcherKind::BundleId => 0,
        AppMatcherKind::PackageFamilyName => 1,
        AppMatcherKind::ExecutablePath => 2,
        AppMatcherKind::Executable => 3,
    }
}
