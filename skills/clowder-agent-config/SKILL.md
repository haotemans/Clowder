---
name: clowder-agent-config
description: Configure and manage Clowder local LLM proxy providers, native OAuth or API Key switching, global default routing, and per-agent overrides for Claude Code, Claude Desktop, Codex CLI, Codex Desktop, OpenCode v2, Pi Coding Agent, DSH (DeepSeek Harness), and DSH Desktop. Use when the user asks to add or switch API or native OAuth providers, set up a global provider across agents, configure or restore a coding agent, or troubleshoot local proxy and VPN port chaining in Clowder.
allowed-tools: default_api
---

# Clowder Agent Config

Configure the Clowder local LLM proxy (`127.0.0.1:8787`), manage API Key and native OAuth providers, and bind either a global default provider or a dedicated per-agent provider across supported coding agents while keeping 100% native protocol byte-identical pass-through and zero personal data upload.

## When to Use

- The user wants to add, edit, or switch an LLM API provider (Anthropic, OpenAI, OpenRouter, DeepSeek, or custom gateway) or native official OAuth account in Clowder.
- The user wants to set a global default provider in Clowder so compatible coding agents share it automatically, or configure a dedicated provider for one specific agent (`claude-code`, `claude-desktop`, `codex`, `codex-desktop`, `opencodev2`, `pi`, `dsh`, or `dsh-desktop`).
- The user wants to restore an agent's original native configuration (`.clowder.bak`) or verify that Clowder is chaining properly with a local VPN/proxy port (`7890` / `10809`).

## Steps

### 1. Inspect Current Clowder Status

Check if the Clowder proxy daemon is running on `http://127.0.0.1:8787` or query via the CLI subcommand. The status output never exposes plaintext keys or personal paths.

- Query via HTTP admin endpoint (when Clowder is running):

```bash
curl -s http://127.0.0.1:8787/_clowder/status
```

- Query via CLI (works whether Clowder is running or stopped):

```powershell
clowder.exe cli status
```

### 2. Add or Update a Provider (API Key or Native Official OAuth)

Never write raw plaintext API keys into `%LOCALAPPDATA%\clowder\clowder\data\config.toml` by hand because `api_key_encrypted` requires Windows DPAPI encryption. Always use the CLI or the local HTTP admin endpoint. Each agent uses its own native protocol (`Anthropic Messages` for Claude Code/Desktop; `OpenAI Responses` for Codex; `OpenAI Chat` or `Anthropic` for OpenCode, Pi, and DSH).

- Add an API Key provider via CLI:

```powershell
clowder.exe cli add-provider <id> "<Display Name>" <anthropic|openai> <base_url> "<api_key>" [direct|system_proxy|custom] [custom_proxy_url]
```

- Or register a native official OAuth account (`claude` for Claude Code `~/.claude/.credentials.json`, or `codex` for Codex `~/.codex/auth.json` with automatic login stash protection):

```powershell
clowder.exe cli add-oauth <claude|codex>
```

- Add or update a provider via HTTP Admin API:

```bash
curl -s -X POST http://127.0.0.1:8787/_clowder/provider \
  -H "Content-Type: application/json" \
  -d '{
    "id": "openrouter",
    "name": "OpenRouter",
    "protocol": "openai",
    "base_url": "https://openrouter.ai/api/v1",
    "api_key": "sk-or-v1-...",
    "upstream_strategy": "system_proxy",
    "set_global": true
  }'
```

### 3. Configure Global Control vs. Single-Agent Override

Clowder supports `follow_global` (inherits the global default provider) and `dedicated` (uses an agent-specific provider). Before modifying any agent config file, Clowder automatically creates a `<config_file>.clowder.bak` backup and only merges the provider endpoint fields so existing MCP servers, plugins, permissions, and prompts remain untouched.

- Set the global default provider and sync all enabled agents at once:

```powershell
clowder.exe cli set-global <provider_id> --sync
```

- Bind a dedicated provider to one specific agent (or revert it to `global`):

```powershell
# Bind a dedicated provider to one agent
clowder.exe cli set-agent <agent_slug> <provider_id> [default_model]

# Revert an agent back to following the global default provider
clowder.exe cli set-agent <agent_slug> global [default_model]
```

- Restore an agent's original configuration file (`.clowder.bak`) to return to direct native connection:

```powershell
clowder.exe cli restore-agent <agent_slug>
```

### 4. Target Agent Slugs and Local Config Reference

Each agent points its local config file to its dedicated Clowder route (`http://127.0.0.1:8787/a/<agent_slug>/...`) so subsequent provider switches inside Clowder take effect in memory without rewriting files:

- `claude-code`: `~/.claude/settings.json` -> `http://127.0.0.1:8787/a/claude-code/anthropic`
- `claude-desktop`: `%APPDATA%\Claude\claude_desktop_config.json` -> `http://127.0.0.1:8787/a/claude-desktop/anthropic`
- `codex` and `codex-desktop`: `~/.codex/config.toml` -> `http://127.0.0.1:8787/a/codex/openai/v1` (`wire_api = "responses"`)
- `opencodev2`: `~/.config/opencode/opencode.json` -> `http://127.0.0.1:8787/a/opencodev2/openai/v1`
- `pi`: `~/.pi/agent/models.json` -> `http://127.0.0.1:8787/a/pi/openai/v1`
- `dsh` and `dsh-desktop`: `~/.dsh/settings.yaml` and `~/.dsh/.env` -> `http://127.0.0.1:8787/a/dsh/openai/v1`

## Gotchas

- **Two-Stage Proxy Chaining with VPN/Clash/v2rayN**: Agent configs automatically include `NO_PROXY=127.0.0.1,localhost` so the first hop (`Agent -> 127.0.0.1:8787`) is never hijacked by the terminal VPN, while Clowder uses `SystemProxy` (reading Windows Registry `Internet Settings` and probing ports `7890`/`7897`/`10809`/`10808`/`2080`) for the second hop (`Clowder :8787 -> VPN Port -> Overseas API`). For domestic APIs (DeepSeek, Kimi, GLM), set upstream strategy to `direct`.
- **Native Protocol Pass-Through**: Keep each agent paired with providers that speak its native protocol (`Anthropic` for Claude Code/Desktop, `OpenAI` for Codex). Clowder forwards request bodies and SSE chunks 100% byte-for-byte without protocol translation overhead.
- **Zero Personal Data Upload**: Never transmit local file paths, session contents, API keys, or OAuth tokens to any third party.
