//! TUI app state — vendors, tab selection, per-vendor snapshot cache.

use std::time::Duration;

use chrono::Utc;
use reqwest::Client;

use crate::cache::DEFAULT_TTL;
use crate::config::Config;
use crate::error::Result;
use crate::theme::Theme;
use crate::vendor::{VendorId, VendorOutcome};

/// What we display per vendor — raw snapshot + fetch metadata for native
/// panel rendering, or an error message when the fetch failed.
///
/// `Ready` is boxed because the snapshot is much larger than the other two
/// variants (silences `clippy::large_enum_variant`).
#[derive(Debug, Clone)]
pub enum TabState {
    Loading,
    Ready(Box<ReadyTab>),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct ReadyTab {
    pub snapshot: crate::usage::VendorSnapshot,
    pub stale: bool,
    pub last_error: Option<(u16, String)>,
    pub cache_age: Option<std::time::Duration>,
}

#[derive(Debug)]
pub struct App {
    pub vendors: Vec<VendorId>,
    pub active: usize,
    pub tabs: Vec<TabState>,
    pub theme: Theme,
    pub last_refresh: chrono::DateTime<chrono::Utc>,
    pub quit: bool,
    /// When `Some`, the Settings overlay is open and consuming key events.
    pub settings: Option<crate::tui::settings::SettingsState>,
}

impl App {
    pub fn new(vendors: Vec<VendorId>) -> Self {
        let n = vendors.len();
        Self {
            vendors,
            active: 0,
            tabs: vec![TabState::Loading; n],
            theme: Theme::default().merged_with_omarchy(),
            last_refresh: Utc::now(),
            quit: false,
            settings: None,
        }
    }

    /// Construct with an initial active tab — usually `[ui] primary` from
    /// config. Silently falls through to index 0 if the requested vendor
    /// isn't in `vendors` (e.g. it was disabled).
    pub fn new_with_primary(vendors: Vec<VendorId>, primary: Option<VendorId>) -> Self {
        let mut app = Self::new(vendors);
        if let Some(p) = primary {
            if let Some(idx) = app.vendors.iter().position(|v| *v == p) {
                app.active = idx;
            }
        }
        app
    }

    pub fn active_vendor(&self) -> Option<VendorId> {
        self.vendors.get(self.active).copied()
    }

    pub fn next_tab(&mut self) {
        if !self.vendors.is_empty() {
            self.active = (self.active + 1) % self.vendors.len();
        }
    }

    pub fn prev_tab(&mut self) {
        if !self.vendors.is_empty() {
            self.active = (self.active + self.vendors.len() - 1) % self.vendors.len();
        }
    }
}

/// Fetch and render one vendor — returns a `TabState`.
pub async fn refresh_one(
    client: &Client,
    config: &Config,
    _theme: &Theme,
    vendor: VendorId,
) -> TabState {
    match build_outcome(client, config, vendor).await {
        Ok(outcome) => TabState::Ready(Box::new(ReadyTab {
            snapshot: outcome.snapshot,
            stale: outcome.stale,
            last_error: outcome.last_error,
            cache_age: outcome.cache_age,
        })),
        Err(e) => TabState::Error(e.to_string()),
    }
}

async fn build_outcome(
    client: &Client,
    config: &Config,
    vendor: VendorId,
) -> Result<VendorOutcome> {
    match vendor {
        VendorId::Anthropic => {
            let cache = crate::cache::Cache::for_vendor("anthropic")?;
            let creds_path = config
                .anthropic
                .credentials_path
                .clone()
                .unwrap_or_else(|| crate::anthropic::creds::default_path().unwrap_or_default());
            let endpoints = crate::anthropic::fetch::Endpoints::default();
            let outcome = crate::anthropic::fetch_snapshot(
                client,
                &creds_path,
                &cache,
                &endpoints,
                DEFAULT_TTL,
            )
            .await?;
            Ok(crate::vendor::VendorOutcome {
                snapshot: crate::usage::VendorSnapshot::Anthropic(outcome.snapshot),
                stale: outcome.stale,
                last_error: outcome.last_error,
                cache_age: outcome.cache_age,
            })
        }
        VendorId::Openrouter => {
            let api_key = crate::config::resolve_api_key(
                "OpenRouter",
                &config.openrouter.api_key_env,
                config.openrouter.api_key.as_deref(),
            )?;
            let cache = crate::cache::Cache::for_vendor("openrouter")?;
            let endpoints = crate::openrouter::fetch::Endpoints::default();
            let outcome = crate::openrouter::fetch_snapshot(client, &api_key, &cache, &endpoints, DEFAULT_TTL).await?;
            Ok(outcome.into())
        }
        VendorId::Zai => {
            let api_key = crate::config::resolve_api_key(
                "Zai",
                &config.zai.api_key_env,
                config.zai.api_key.as_deref(),
            )?;
            let cache = crate::cache::Cache::for_vendor("zai")?;
            let endpoints = crate::zai::fetch::Endpoints::default();
            let outcome = crate::zai::fetch_snapshot(
                client,
                &api_key,
                &cache,
                &endpoints,
                DEFAULT_TTL,
                config.zai.plan_tier.as_deref(),
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::Openai => {
            let cache = crate::cache::Cache::for_vendor("openai")?;
            let creds_path = config
                .openai
                .codex_auth_path
                .clone()
                .unwrap_or_else(|| crate::openai::creds::default_path().unwrap_or_default());
            let endpoints = crate::openai::fetch::Endpoints::default();
            let outcome = crate::openai::fetch_snapshot(client, &creds_path, &cache, &endpoints, DEFAULT_TTL).await?;
            Ok(outcome.into())
        }
    }
}

/// Convenience for the watch-driven binary: how long to wait between
/// automatic refreshes.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
