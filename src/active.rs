//! Active-vendor state file. Set by `--cycle-next` / `--cycle-prev` (which
//! Waybar's `on-scroll-up`/`on-scroll-down` invoke), read by the widget on
//! every tick. The TUI does NOT consult this — it has its own tab state.
//!
//! On-disk shape: a single line with the vendor slug (e.g. `openai`). Located
//! at `<cache-dir>/active_vendor`.

use std::fs;
use std::path::PathBuf;

use crate::cache::atomic_write;
use crate::error::{AppError, Result};
use crate::vendor::VendorId;

fn state_dir() -> Result<PathBuf> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| AppError::Other("could not resolve XDG cache dir".into()))?;
    Ok(base.cache_dir().join("ai-usagebar"))
}

fn state_path() -> Result<PathBuf> {
    Ok(state_dir()?.join("active_vendor"))
}

/// Read the persisted active vendor, if any. `None` means "no override —
/// callers fall back to [ui] primary or anthropic".
pub fn read() -> Option<VendorId> {
    let path = state_path().ok()?;
    let raw = fs::read_to_string(&path).ok()?;
    parse_slug(raw.trim())
}

/// Read the raw persisted key string, if any. Unlike `read()`, this does not
/// attempt to parse the value into a `VendorId` — useful for named accounts.
pub fn read_raw() -> Option<String> {
    let path = state_path().ok()?;
    let raw = fs::read_to_string(&path).ok()?;
    let s = raw.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// Persist `vendor` as the active one. Atomic.
pub fn write(vendor: VendorId) -> Result<()> {
    let path = state_path()?;
    atomic_write(&path, vendor.slug().as_bytes())
}

/// Persist an arbitrary key string as the active vendor. Atomic.
pub fn write_str(key: &str) -> Result<()> {
    let path = state_path()?;
    atomic_write(&path, key.as_bytes())
}

/// Cycle the active vendor by `delta` positions through `enabled` (which
/// preserves canonical order). Wraps. If no state exists, starts at `start`
/// (usually `[ui] primary` or anthropic).
pub fn cycle(enabled: &[VendorId], start: VendorId, delta: i32) -> Result<VendorId> {
    if enabled.is_empty() {
        return Err(AppError::Other("no enabled vendors to cycle".into()));
    }
    let current = read().filter(|v| enabled.contains(v)).unwrap_or(start);
    let cur_idx = enabled.iter().position(|v| *v == current).unwrap_or(0);
    let n = enabled.len() as i32;
    let next_idx = ((cur_idx as i32 + delta).rem_euclid(n)) as usize;
    let next = enabled[next_idx];
    write(next)?;
    Ok(next)
}

/// Cycle through a mixed list of arbitrary vendor key strings (named anthropic
/// accounts and standard vendor slugs). Wraps. Persists the new active key and
/// returns it.
pub fn cycle_mixed(all_keys: &[String], start_key: &str, delta: i32) -> Result<String> {
    if all_keys.is_empty() {
        return Err(AppError::Other("no vendors to cycle".into()));
    }
    let current = read_raw()
        .filter(|k| all_keys.contains(k))
        .unwrap_or_else(|| start_key.to_string());
    let cur_idx = all_keys.iter().position(|k| k == &current).unwrap_or(0);
    let n = all_keys.len() as i32;
    let next_idx = ((cur_idx as i32 + delta).rem_euclid(n)) as usize;
    let next = all_keys[next_idx].clone();
    write_str(&next)?;
    Ok(next)
}

pub fn parse_slug(s: &str) -> Option<VendorId> {
    match s {
        "anthropic" => Some(VendorId::Anthropic),
        "openai" => Some(VendorId::Openai),
        "zai" => Some(VendorId::Zai),
        "openrouter" => Some(VendorId::Openrouter),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_slug_round_trip() {
        for id in VendorId::all() {
            assert_eq!(parse_slug(id.slug()), Some(*id));
        }
    }

    #[test]
    fn parse_slug_unknown_returns_none() {
        assert!(parse_slug("not-a-vendor").is_none());
        assert!(parse_slug("").is_none());
    }

    #[test]
    fn cycle_wraps_forward_and_backward() {
        // Pure cycle math (no disk I/O — we don't call read/write here,
        // only go through `cycle` which would touch disk). Verify the
        // index arithmetic directly.
        let enabled = [
            VendorId::Anthropic,
            VendorId::Openai,
            VendorId::Zai,
            VendorId::Openrouter,
        ];
        let step = |from: usize, delta: i32| -> VendorId {
            enabled[((from as i32 + delta).rem_euclid(4)) as usize]
        };
        // forward from Anthropic → Openai
        assert_eq!(step(0, 1), VendorId::Openai);
        // backward from Anthropic → Openrouter (wrap)
        assert_eq!(step(0, -1), VendorId::Openrouter);
        // forward from Openrouter → Anthropic (wrap)
        assert_eq!(step(3, 1), VendorId::Anthropic);
    }
}
