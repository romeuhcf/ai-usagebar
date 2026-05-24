# ai-usagebar

Waybar widget + tabbed TUI for AI plan usage across **Anthropic Claude**, **OpenAI Codex/ChatGPT**, **Z.AI (GLM)**, and **OpenRouter**.

Rust port of [`claudebar`](https://github.com/mryll/claudebar) (drop-in compatible) extended to four vendors. Same minimalist Pango-bordered tooltip design, same Omarchy theme auto-detection, same flock-protected OAuth refresh — but tested, modular, and reliable instead of 865 lines of bash.

## Features

- **Per-vendor Waybar modules** — same JSON output shape as claudebar.
- **Tabbed TUI** (`ai-usagebar-tui`) — interactive view with Tab/h/l switching, per-tab refresh, auto-refresh every 60s. Native ratatui widgets fill the available terminal width with consistent layout across vendors.
- **Scroll-to-cycle on the bar** — wire `on-scroll-up` / `on-scroll-down` and a single bar item cycles through your enabled vendors.
- **Config-driven primary vendor** — set `[ui] primary` once; the widget shows that vendor by default and the TUI opens on its tab.
- **Local-testing UX** — `--pretty` renders ANSI-colored terminal output (auto-detects TTY); `--watch N` re-renders every N seconds.
- **Drop-in claudebar compatibility** — all the same flags (`--icon`, `--format`, `--tooltip-format`, `--pace-tolerance`, `--format-pace-color`, `--tooltip-pace-pts`, `--color-*`) and `{placeholders}`.
- **Always exits 0** — Waybar hides modules that don't.
- **Atomic cache writes + flock** — multi-monitor Waybar instances coexist without API stampedes.
- **Transient-vs-hard error split** — DNS/timeout failures show a quiet `Loading…`; HTTP 4xx/5xx surface the code in the tooltip.
- **Live API smoke test suite** — `make smoke` hits the real (undocumented) endpoints to surface schema drift.

## Install

### Arch (AUR)

Two packages — pick one:

```bash
yay -S ai-usagebar-bin    # prebuilt binary from GitHub Releases (fast, ~5s install)
yay -S ai-usagebar        # compiles from source (~30-60s, hermetic)
```

The `-bin` variant downloads the same x86_64 ELF that CI built and tested; the source variant compiles locally with your toolchain. Both install identical binaries to `/usr/bin/`. If you already have one, switching is `yay -S` the other — pacman handles the swap via `conflicts`/`provides`.

### From source

```bash
cargo build --release
sudo make install                  # → /usr/local/bin
# or
make install PREFIX=$HOME/.local   # → ~/.local/bin
```

## Authentication

Each vendor authenticates differently. Anthropic and OpenAI use OAuth credentials that their official CLIs already wrote to disk — **no env vars needed.** Z.AI and OpenRouter use API keys, which you can pass via env var OR (for users who don't source secrets in their shell) inline in `config.toml`.

| Vendor | Method | Action required |
|---|---|---|
| Anthropic | OAuth, read from `~/.claude/.credentials.json` | Run `claude` once to log in. Token auto-refreshes. |
| OpenAI | OAuth, read from `~/.codex/auth.json` | Run `codex login` once. Token auto-refreshes. |
| Z.AI | API key (`ZAI_API_KEY` env or `[zai] api_key` in config) | Set either. |
| OpenRouter | API key (`OPENROUTER_API_KEY` env or `[openrouter] api_key` in config) | Set either. |

### Credential resolution order (for API-key vendors)

For each API-key vendor, ai-usagebar checks in this order:

1. **Env var named by `api_key_env`** in config (defaults: `ZAI_API_KEY`, `OPENROUTER_API_KEY`). If set + non-empty, used.
2. **Inline `api_key`** in the same config section.
3. Otherwise, **error** with a message naming both options.

### Security

- If you put inline `api_key` values in config, `chmod 600 ~/.config/ai-usagebar/config.toml`. The default behavior reads only env vars, which is safer when your config might be world-readable.
- Don't commit your config dir if you check it into dotfiles unless you've redacted `api_key` lines.
- OAuth credential files (`~/.claude/.credentials.json`, `~/.codex/auth.json`) are managed by their respective CLIs and already chmod-protected.

## Configuration

`~/.config/ai-usagebar/config.toml` (optional — defaults enable all four vendors). Full example:

```toml
[ui]
# Which vendor the widget shows when --vendor is omitted, AND which tab
# is selected when the TUI opens. Defaults to anthropic when not set.
# primary = "anthropic"   # anthropic | openai | zai | openrouter

[anthropic]
enabled = true
# credentials_path = "/home/you/.claude/.credentials.json"

[openai]
enabled = true
# codex_auth_path = "/home/you/.codex/auth.json"

[zai]
enabled = true
api_key_env = "ZAI_API_KEY"
# api_key = "..."          # used if ZAI_API_KEY is unset; chmod 600 the file!
# plan_tier = "lite"       # lite | pro | max — display-only

[openrouter]
enabled = true
api_key_env = "OPENROUTER_API_KEY"
# api_key = "sk-or-v1-..."
```

## Quick start

```bash
# Local testing — auto-detects TTY and renders human-readable output.
ai-usagebar                        # uses [ui] primary (defaults to anthropic)
ai-usagebar --vendor openai
ai-usagebar --vendor zai
ai-usagebar --vendor openrouter

# Force Waybar JSON (e.g. piping into jq).
ai-usagebar --json

# Live preview while iterating on --format / --tooltip-format.
ai-usagebar --vendor openrouter --watch 5

# Interactive TUI with tabs.
ai-usagebar-tui
```

## Waybar config

### Single module, scroll-to-cycle (recommended)

One bar item that you can scroll through to cycle vendors. The TUI on-click shows them all:

```jsonc
"modules-right": ["custom/aibar", ...],

"custom/aibar": {
    "exec": "ai-usagebar --format '{vendor_short} {session_pct}% · {session_reset}'",
    "return-type": "json",
    "interval": 300,
    "signal": 13,
    "tooltip": true,
    "on-click": "ai-usagebar-tui",
    "on-scroll-up":   "ai-usagebar --cycle-next",
    "on-scroll-down": "ai-usagebar --cycle-prev"
}
```

The `{vendor_short}` placeholder always expands to a 3-letter vendor ID (`cld` / `gpt` / `zai` / `opr`) so the bar text tells you which vendor is currently cycled. The other usage placeholders (`{session_pct}` for Anthropic, `{oai_session_pct}` for OpenAI, etc.) are vendor-specific — to use one format string that works for all four cycled vendors, prefer the generic versions where available (currently `{session_pct}` works for Anthropic only; the other vendors expose their own `{oai_*}` / `{zai_*}` / `{or_*}` families that fall through to empty for vendors that don't define them).

`signal: 13` lets the scroll-cycle commands refresh the bar instantly (via `SIGRTMIN+13`) instead of waiting for the next 300s interval.

### Per-vendor modules

If you'd rather see them all at once:

```jsonc
"modules-right": ["custom/claude", "custom/openai", "custom/openrouter", "custom/zai"],

"custom/claude": {
    "exec": "ai-usagebar --vendor anthropic --icon '󰚩'",
    "return-type": "json",
    "interval": 300,
    "tooltip": true,
    "on-click": "ai-usagebar-tui"
},
"custom/openai": {
    "exec": "ai-usagebar --vendor openai --icon '󱢆'",
    "return-type": "json",
    "interval": 300,
    "tooltip": true
},
"custom/openrouter": {
    "exec": "ai-usagebar --vendor openrouter --icon '󱙺' --format '{or_balance} · {or_used_today}'",
    "return-type": "json",
    "interval": 600,
    "tooltip": true
},
"custom/zai": {
    "exec": "ai-usagebar --vendor zai --icon '󰚩'",
    "return-type": "json",
    "interval": 300,
    "tooltip": true
}
```

> Why 300s? The Anthropic and OpenAI Codex endpoints are undocumented and rate-limit aggressively below ~300s. The cache TTL is 60s so multi-monitor instances coexist, but Waybar's polling interval should stay at 300s.

## Hyprland: float the TUI window

By default Hyprland tiles the TUI. To make `ai-usagebar-tui` open as a floating window, add this to `~/.config/hypr/hyprland.conf` (or any sourced `.conf`):

```ini
# Float ai-usagebar-tui when launched via omarchy-launch-tui (which sets the
# app-id from the binary basename).
windowrulev2 = float, class:^(org\.omarchy\.ai-usagebar-tui)$
windowrulev2 = size 80% 70%, class:^(org\.omarchy\.ai-usagebar-tui)$
windowrulev2 = center, class:^(org\.omarchy\.ai-usagebar-tui)$
```

Then `hyprctl reload` (no logout needed). If you launch the TUI differently (`kitty -e ai-usagebar-tui`, etc.) replace the `class` regex with whatever your terminal reports — `hyprctl clients` will show it.

## Vendor support matrix

| Vendor | Endpoint | What you see |
|---|---|---|
| **Anthropic** | `api.anthropic.com/api/oauth/usage` (undocumented) | Session (5h), Weekly (7d), Sonnet (7d), Extra usage $ |
| **OpenAI** | `chatgpt.com/backend-api/wham/usage` (undocumented; used by official `codex` CLI) | Codex 5h, Codex weekly, Code-review weekly, Credits |
| **Z.AI** | `api.z.ai/api/monitor/usage/quota/limit` (undocumented) | Session 5h, Weekly 7d, MCP tools monthly |
| **OpenRouter** | `openrouter.ai/api/v1/{credits,key}` (documented) | Balance, today/week/month spend, free vs paid tier |

### Endpoint stability

Three of the four endpoints are undocumented. The Anthropic and OpenAI endpoints are used by their respective official CLIs (`claude` and `codex`) — disappearing them would break those tools too, so they're more durable than scraped web endpoints. Z.AI's monitor endpoint is reverse-engineered from a third-party plugin; treat it as the most fragile.

When an endpoint drifts, **run `make smoke`** — the live API tests check the exact fields we depend on and produce a precise failure pointing at what changed. Paste the failure back into Claude Code and the affected `types.rs` can be updated mechanically.

## Format placeholders

### Shared / Anthropic (claudebar-compatible)

| Placeholder | Example |
|---|---|
| `{plan}` | `Max 5x` |
| `{session_pct}`, `{session_reset}`, `{session_bar}`, `{session_elapsed}` | `62`, `1h 30m`, `█████████████░░░░░░░`, `58` |
| `{session_pace}`, `{session_pace_indicator}`, `{session_pace_pct}`, `{session_pace_pts}`, `{session_pace_delta}`, `{session_pace_abs_delta}` | `↑`, `↑`, `12% ahead`, `4pts ahead`, `4`, `4` |
| `{weekly_*}` | same family for the 7d window |
| `{sonnet_*}` | same family for the 7d Sonnet window (empty when absent) |
| `{extra_spent}`, `{extra_limit}`, `{extra_pct}`, `{extra_bar}` | `$2.50`, `$50.00`, `5`, `█░░░░░░░░░░░░░░░░░░░` |

### OpenAI (Codex OAuth)

`{oai_plan}`, `{oai_session_pct}`, `{oai_session_reset}`, `{oai_session_elapsed}`, `{oai_session_pace}`, `{oai_session_pace_indicator}`, `{oai_weekly_*}` (same family), `{oai_code_review_pct}`, `{oai_credit_balance}`, `{oai_local_msgs}`, `{oai_cloud_msgs}`

### Z.AI

`{zai_plan}`, `{zai_session_pct}`, `{zai_session_reset}`, `{zai_weekly_pct}`, `{zai_weekly_reset}`, `{zai_mcp_pct}`, `{zai_mcp_reset}`

### OpenRouter

`{or_label}`, `{or_balance}`, `{or_total}`, `{or_used}`, `{or_used_today}`, `{or_used_week}`, `{or_used_month}`, `{or_consumed_pct}`, `{or_free_tier}`, `{or_limit}`, `{or_limit_remaining}`, `{or_balance_bar}`

## Local development

```bash
ai-usagebar --watch 5                              # iterate on --format live
ai-usagebar --vendor openrouter --format '{or_balance} · today {or_used_today}'

make test                                          # unit + integration
source ~/.config/zsh/secrets                       # only needed for live smoke
make smoke                                         # live API drift detection
make clippy                                        # cargo clippy -D warnings
```

## TUI controls

- `Tab` / `l` / `→` — next tab
- `Shift+Tab` / `h` / `←` — previous tab
- `r` — refresh active tab
- `R` — refresh all tabs
- `s` — open Settings overlay (primary vendor + API keys)
- `q` / `Esc` / `Ctrl-C` — quit

Auto-refresh runs every 60 seconds in the background.

### Settings overlay

Press `s` while the TUI is open. The overlay lets you:

- Pick the **primary vendor** that the widget defaults to and that the TUI selects on startup. Use `←` / `→` to cycle.
- Enter your **Z.AI API key** and **OpenRouter API key** inline. Keys are masked as you type; press `Ctrl-V` to reveal/hide. Env vars (`ZAI_API_KEY`, `OPENROUTER_API_KEY`) still win at runtime if they're set — the inline key is the fallback.

Key bindings inside the overlay:

- `Tab` / `↑↓` — move between fields
- `←` / `→` — cycle primary-vendor selection (only on the vendor field)
- `Ctrl-V` — toggle key visibility on the focused key field
- `Ctrl-S` — save and close
- `Esc` — discard and close

Save writes to `~/.config/ai-usagebar/config.toml` via `toml_edit` so your existing comments and unrelated fields are preserved. The file is automatically `chmod 600`ed on save, so inline keys aren't world-readable.

## Theming

- One Dark palette by default.
- Auto-merges with the active Omarchy theme at `~/.config/omarchy/current/theme/colors.toml`.
- Per-color overrides: `--color-low`, `--color-mid`, `--color-high`, `--color-critical` (claudebar-compatible).

## Acknowledgements

Direct reverse-engineering reference for the OpenAI and Anthropic OAuth endpoints came from [`claudebar`](https://github.com/mryll/claudebar) and [`codexbar`](https://github.com/mryll/codexbar) (both by mryll). The visual design — bordered Pango tooltip, severity colors, pacing math — is theirs; this project is a faithful Rust port plus multi-vendor extension.

## License

MIT.
