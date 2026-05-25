//! The always-exits-0 orchestrator. Dispatches on `--vendor`, fetches a
//! snapshot, renders the right output mode, and prints. Catches every error
//! into a fallback `⚠` JSON / pretty line so Waybar never hides the module.

use std::io::Write;
use std::time::Duration;

use chrono::Utc;
use reqwest::Client;

use crate::anthropic::{self, fetch::FetchOutcome};
use crate::cache::{Cache, DEFAULT_TTL};
use crate::config::Config;
use crate::error::{AppError, Result};
use crate::openai;
use crate::openrouter;
use crate::theme::Theme;
use crate::vendor::{HTTP_CLIENT_TIMEOUT, RenderOpts, VendorOutcome};
use crate::waybar::WaybarOutput;
use crate::widget::cli::{Cli, Vendor};
use crate::widget::pretty::print_pretty;
use crate::widget::render::{DEFAULT_FORMAT, RenderInput, render_anthropic};
use crate::zai;

/// Entry point — runs to completion and ALWAYS returns Ok with exit code 0
/// in the caller. Mirrors claudebar's `die()` invariant.
pub async fn run(cli: Cli) -> i32 {
    // Scroll-cycle short-circuit: don't render, just bump state + signal waybar.
    if cli.cycle_next || cli.cycle_prev {
        return run_cycle(&cli).await;
    }
    if let Some(secs) = cli.watch {
        return run_watch(cli, secs).await;
    }
    run_once(&cli, &mut std::io::stdout()).await;
    0
}

/// Cycle to the next/prev enabled vendor and signal waybar to refresh.
/// Includes named Anthropic accounts (from `[[anthropic.extra]]`) as distinct
/// cycle positions. Always exits 0 — Waybar swallows non-zero exits anyway.
async fn run_cycle(cli: &Cli) -> i32 {
    let config = Config::load().unwrap_or_default();

    // Build the full ordered key list: named Anthropic accounts first, then
    // other standard vendors in canonical order.
    let mut all_keys: Vec<String> = config.anthropic.enabled_keys();
    for id in [
        crate::vendor::VendorId::Openai,
        crate::vendor::VendorId::Zai,
        crate::vendor::VendorId::Openrouter,
    ] {
        if config.is_enabled(id) {
            all_keys.push(id.slug().to_string());
        }
    }

    if all_keys.is_empty() {
        return 0;
    }

    let start_key = all_keys[0].clone();
    let delta = if cli.cycle_next { 1 } else { -1 };
    let _ = crate::active::cycle_mixed(&all_keys, &start_key, delta);

    // Refresh the bar immediately. The Waybar module's `signal: 13` setting
    // means SIGRTMIN+13 re-runs the exec. SIGRTMIN is libc-dependent; the
    // shell-safe value on Linux glibc is signal 47 (= SIGRTMIN(34)+13).
    crate::waybar::request_refresh();
    0
}

async fn run_watch(cli: Cli, secs: u64) -> i32 {
    let interval = Duration::from_secs(secs.max(1));
    loop {
        // Clear screen + home cursor.
        print!("\x1b[2J\x1b[H");
        let _ = std::io::stdout().flush();
        run_once(&cli, &mut std::io::stdout()).await;
        println!();
        eprintln!("(re-rendering every {secs}s — press Ctrl-C to exit)");
        tokio::select! {
            _ = tokio::time::sleep(interval) => continue,
            _ = tokio::signal::ctrl_c() => return 0,
        }
    }
}

async fn run_once(cli: &Cli, out: &mut impl Write) {
    let output = match build_output(cli).await {
        Ok(o) => o,
        Err(e) => fallback(&e, cli),
    };

    if cli.output_json() {
        let _ = out.write_all(output.to_json_line().as_bytes());
    } else {
        let _ = print_pretty(out, &output);
    }
    let _ = out.flush();
}

async fn build_output(cli: &Cli) -> Result<WaybarOutput> {
    let config = Config::load().unwrap_or_default();
    let vendor = cli.resolved_vendor(&config);
    if !config.is_enabled(vendor.to_id()) {
        return Err(AppError::Other(format!(
            "vendor {:?} is disabled in ~/.config/ai-usagebar/config.toml",
            vendor
        )));
    }
    match vendor {
        Vendor::Anthropic => anthropic_output(cli, &config).await,
        Vendor::Openrouter => openrouter_output(cli, &config).await,
        Vendor::Openai => openai_output(cli, &config).await,
        Vendor::Zai => zai_output(cli, &config).await,
    }
}

async fn openai_output(cli: &Cli, config: &Config) -> Result<WaybarOutput> {
    let client = http_client()?;
    let cache = vendor_cache(cli, "openai")?;
    let creds_path = match config.openai.codex_auth_path.as_deref() {
        Some(p) => p.to_path_buf(),
        None => openai::creds::default_path()?,
    };
    let endpoints = openai::fetch::Endpoints::default();
    let outcome =
        match openai::fetch_snapshot(&client, &creds_path, &cache, &endpoints, DEFAULT_TTL).await {
            Ok(o) => o,
            Err(e) if e.is_transient() => return Ok(WaybarOutput::loading(cli.icon.as_deref())),
            Err(e) => return Err(e),
        };

    let theme = theme_from_cli(cli);
    let snap = outcome.snapshot.clone();
    let vendor_outcome: VendorOutcome = outcome.into();
    let opts = RenderOpts::from_cli(cli);
    Ok(openai::vendor::render(
        &vendor_outcome,
        &snap,
        &theme,
        &opts,
        chrono::Utc::now(),
    ))
}

async fn zai_output(cli: &Cli, config: &Config) -> Result<WaybarOutput> {
    let api_key = crate::config::resolve_api_key(
        "Zai",
        &config.zai.api_key_env,
        config.zai.api_key.as_deref(),
    )?;
    let client = http_client()?;
    let cache = vendor_cache(cli, "zai")?;
    let endpoints = zai::fetch::Endpoints::default();
    let outcome = match zai::fetch_snapshot(
        &client,
        &api_key,
        &cache,
        &endpoints,
        DEFAULT_TTL,
        config.zai.plan_tier.as_deref(),
    )
    .await
    {
        Ok(o) => o,
        Err(e) if e.is_transient() => return Ok(WaybarOutput::loading(cli.icon.as_deref())),
        Err(e) => return Err(e),
    };

    let theme = theme_from_cli(cli);
    let snap = outcome.snapshot.clone();
    let vendor_outcome: VendorOutcome = outcome.into();
    let opts = RenderOpts::from_cli(cli);
    Ok(zai::vendor::render(
        &vendor_outcome,
        &snap,
        &theme,
        &opts,
        chrono::Utc::now(),
    ))
}

async fn openrouter_output(cli: &Cli, config: &Config) -> Result<WaybarOutput> {
    let api_key = crate::config::resolve_api_key(
        "OpenRouter",
        &config.openrouter.api_key_env,
        config.openrouter.api_key.as_deref(),
    )?;
    let client = http_client()?;
    let cache = vendor_cache(cli, "openrouter")?;
    let endpoints = openrouter::fetch::Endpoints::default();
    let outcome = match openrouter::fetch_snapshot(
        &client,
        &api_key,
        &cache,
        &endpoints,
        DEFAULT_TTL,
    )
    .await
    {
        Ok(o) => o,
        Err(e) if e.is_transient() => return Ok(WaybarOutput::loading(cli.icon.as_deref())),
        Err(e) => return Err(e),
    };

    let theme = theme_from_cli(cli);

    let snap = outcome.snapshot.clone();
    let vendor_outcome: VendorOutcome = outcome.into();
    let opts = RenderOpts::from_cli(cli);
    Ok(openrouter::vendor::render(
        &vendor_outcome,
        &snap,
        &theme,
        &opts,
        chrono::Utc::now(),
    ))
}

async fn anthropic_output(cli: &Cli, config: &Config) -> Result<WaybarOutput> {
    let client = http_client()?;
    let (creds_path, cache_key) = resolve_anthropic_account(cli, config)?;
    let cache = vendor_cache(cli, &cache_key)?;
    let endpoints = anthropic::fetch::Endpoints::default();
    let outcome = match anthropic::fetch_snapshot(
        &client,
        &creds_path,
        &cache,
        &endpoints,
        DEFAULT_TTL,
    )
    .await
    {
        Ok(o) => o,
        Err(e) if e.is_transient() => {
            // Mirror claudebar's `loading_network` path.
            return Ok(WaybarOutput::loading(cli.icon.as_deref()));
        }
        Err(e) => return Err(e),
    };

    let theme = theme_from_cli(cli);
    Ok(render_with_theme(&outcome, &theme, cli))
}

/// Resolve which Anthropic account to use, returning (credentials_path, cache_key).
///
/// Precedence:
///   1. `--creds-path` CLI flag (debug/test)
///   2. Named account matching the active vendor key in state file
///   3. Primary `[anthropic]` configured credentials path
///   4. Default `~/.claude/.credentials.json`
fn resolve_anthropic_account(cli: &Cli, config: &Config) -> Result<(std::path::PathBuf, String)> {
    if let Some(p) = cli.creds_path.as_deref() {
        return Ok((p.to_path_buf(), "anthropic".to_string()));
    }

    if let Some(active_key) = crate::active::read_raw() {
        // Primary named account?
        if config.anthropic.vendor.as_deref() == Some(&*active_key) {
            let path = expand_home_opt(config.anthropic.credentials_path.as_deref())
                .unwrap_or_else(|| anthropic::creds::default_path().unwrap_or_default());
            return Ok((path, format!("anthropic-{active_key}")));
        }
        // Extra named account?
        if let Some(extra) = config.anthropic.find_extra(&active_key) {
            let path = expand_home_opt(extra.credentials_path.as_deref())
                .unwrap_or_else(|| anthropic::creds::default_path().unwrap_or_default());
            return Ok((path, format!("anthropic-{active_key}")));
        }
    }

    // Default: primary configured path (or built-in default).
    let path = expand_home_opt(config.anthropic.credentials_path.as_deref())
        .unwrap_or_else(|| anthropic::creds::default_path().unwrap_or_default());
    Ok((path, "anthropic".to_string()))
}

/// Expand a leading `~/` to `$HOME/`. Returns `None` when `path` is `None`.
fn expand_home_opt(path: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    let p = path?;
    let s = p.to_str()?;
    if let Some(rest) = s.strip_prefix("~/") {
        let home = std::env::var_os("HOME")?;
        Some(std::path::PathBuf::from(home).join(rest))
    } else {
        Some(p.to_path_buf())
    }
}

fn render_with_theme(outcome: &FetchOutcome, theme: &Theme, cli: &Cli) -> WaybarOutput {
    let format_owned = cli
        .format
        .clone()
        .unwrap_or_else(|| DEFAULT_FORMAT.to_string());
    let input = RenderInput {
        outcome,
        theme,
        format: &format_owned,
        tooltip_format: cli.tooltip_format.as_deref(),
        icon: cli.icon.as_deref(),
        pace_tolerance: cli.pace_tolerance,
        format_pace_color: cli.format_pace_color,
        tooltip_pace_pts: cli.tooltip_pace_pts,
        now: Utc::now(),
    };
    render_anthropic(&input)
}

fn http_client() -> Result<Client> {
    Client::builder()
        .timeout(HTTP_CLIENT_TIMEOUT)
        .build()
        .map_err(|e| AppError::Other(format!("http client init: {e}")))
}

fn vendor_cache(cli: &Cli, vendor: &str) -> Result<Cache> {
    match cli.cache_dir.as_deref() {
        Some(p) => Ok(Cache::at(p.join(vendor))),
        None => Cache::for_vendor(vendor),
    }
}

fn theme_from_cli(cli: &Cli) -> Theme {
    Theme::default().merged_with_omarchy().with_overrides(
        cli.color_low.clone(),
        cli.color_mid.clone(),
        cli.color_high.clone(),
        cli.color_critical.clone(),
    )
}

/// Fallback output when everything goes wrong — always renders a `⚠` widget.
fn fallback(err: &AppError, _cli: &Cli) -> WaybarOutput {
    let tooltip = match err {
        AppError::Credentials(m) => format!("Credentials error.\n{m}"),
        AppError::Http { status, body } => format!("HTTP {status}\n{body}"),
        AppError::Schema(m) => format!("API schema drift.\n{m}"),
        AppError::Io { path, source } => format!("I/O error at {}.\n{source}", path.display()),
        AppError::Other(m) | AppError::Transport(m) => m.clone(),
        AppError::Json(e) => format!("JSON error: {e}"),
        AppError::Toml(e) => format!("TOML error: {e}"),
        AppError::IoBare(e) => format!("I/O error: {e}"),
    };
    WaybarOutput::error(&tooltip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anthropic::fetch::FetchOutcome;
    use crate::usage::{AnthropicSnapshot, UsageWindow};

    fn cli_default() -> Cli {
        // clap's Default isn't derived for us; build from parse_from with no
        // args to get the canonical defaults.
        use clap::Parser;
        Cli::parse_from(["ai-usagebar"])
    }

    fn dummy_outcome() -> FetchOutcome {
        FetchOutcome {
            snapshot: AnthropicSnapshot {
                plan: "Test".into(),
                session: UsageWindow {
                    utilization_pct: 25,
                    resets_at: None,
                    window_duration: chrono::Duration::hours(5),
                },
                weekly: UsageWindow {
                    utilization_pct: 10,
                    resets_at: None,
                    window_duration: chrono::Duration::days(7),
                },
                sonnet: None,
                extra: None,
            },
            stale: false,
            last_error: None,
            cache_age: None,
        }
    }

    #[test]
    fn render_with_theme_uses_cli_overrides() {
        let cli = {
            let mut c = cli_default();
            c.format = Some("test:{session_pct}".into());
            c.color_low = Some("#123456".into());
            c
        };
        let outcome = dummy_outcome();
        let theme = Theme::default().with_overrides(cli.color_low.clone(), None, None, None);
        let out = render_with_theme(&outcome, &theme, &cli);
        // Bar text should contain our format substitution, wrapped in the
        // overridden low-color span.
        assert!(out.text.contains("test:25"));
        assert!(out.text.contains("#123456"));
    }

    #[test]
    fn fallback_wraps_credentials_error_in_warning() {
        let err = AppError::Credentials("missing token".into());
        let out = fallback(&err, &cli_default());
        assert_eq!(out.text, "⚠");
        assert!(out.tooltip.contains("missing token"));
    }
}
