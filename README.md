# Clowder

Rust 实现的 **本地优先** 多 Agent LLM API 供应商总控/独立切换 + 实时 Token / 纯解码 TPS 统计与统一会话管理工具 (Windows)。

## 核心特性

- **右上角 1/4 圆 Agent 扇形切换器**: 窗口右上角常驻 1/4 圆触发器，点击即刻展开扇形可用 Agent 面板（支持 `Claude Code`、`Claude Desktop`、`Codex CLI`、`Codex Desktop`、`OpenCode v2`、`Pi Agent`、`DSH CLI`、`DSH Desktop` 及 `总控 ALL`），一键切换视角与配置。
- **总控默认 + 单 Agent 独立配置**:
  - **总控模式 (`FollowGlobal`)**: 在总控中心指定一个默认供应商，所有 Agent 自动复用，一键同步全部配置文件。
  - **独立配置 (`Dedicated`)**: 可针对任意单个 Agent 单独绑定不同供应商与默认模型，通过 `/a/{agent_slug}/*` 专属本地代理路径实现内存级零重启秒切。
- **高精度实时统计**:
  - 完整支持 Anthropic Messages (流式 SSE + 非流式 JSON)、OpenAI Chat Completions (自动注入 `stream_options.include_usage = true`) 与 OpenAI `/v1/responses` (Codex CLI / Desktop)。
  - 精确解析 `input_tokens`、`output_tokens`、`cache_read_tokens` (含 `cached_tokens` / `prompt_cache_hit_tokens`)、`cache_write_tokens` 与 `reasoning_tokens`。
  - **纯解码 TPS**: 自动扣除首字延迟 (`TTFT`)，以 `(last_token_ms - ttft_ms)` 计算真实解码吞吐率；流中断时具备请求侧 + 响应侧双端字符级估算兜底。
- **用户授权的统一会话管理与显示开关**:
  - 在取得用户明确授权后，支持跨 Agent 汇总实时代理会话与各 Agent 本地会话目录元数据（`~/.claude/projects`、`~/.codex/sessions`、`~/.pi/agent/sessions`、`~/.dsh/storages` 等）。
  - 所有页签、右上角 1/4 圆及单个 Agent 的显隐均可在「设置」页自由开关。
- **内置 AI 自动配置 Skill (`skills/clowder-agent-config/SKILL.md`)**:
  - 提供 `clowder.exe cli ...` 子命令与 `http://127.0.0.1:8787/_clowder/*` 本地管理接口，支持 AI 自动完成 DPAPI 加密与多 Agent 配置文件写入。

## 快速开始

### 安装 (给最终用户)

到 [Releases](https://github.com/haotemans/Clowder/releases) 下载 `clowder-<版本>-setup.exe`, 双击即装:

- 默认走 **per-user** (`%LOCALAPPDATA%\Programs\Clowder`), 不需要管理员权限;
  首屏勾选 "Install for all users" 才会装到 `C:\Program Files\Clowder` (要 UAC)。
- 装完有开始菜单项 + 桌面快捷方式 + 卸载入口 (设置 → 应用 → 已安装的应用)。
- 便捷 zip 版 (`clowder-windows.zip`) 仅需解压即用, 但无快捷方式、无卸载器。

### 构建与运行

```powershell
cargo build --release
.\target\release\clowder.exe -f   # 直接打开完整主界面 (默认开桌面玻璃小窗)
.\target\release\clowder.exe -s   # Headless 仅跑代理与统计
```

打安装包 (需本机装有 NSIS 3, `choco install nsis` 或到官网下载):

```powershell
makensis /DVERSION=0.1.0 installer/clowder.nsi
# 产出 clowder-0.1.0-setup.exe
```

打 `v*` tag 推上去后, CI 会自动跑 `installer (NSIS)` 步骤并把 setup.exe 挂到 Release。

### AI Skill / CLI 命令行配置

```powershell
# 查看当前状态
.\target\release\clowder.exe cli status

# 添加供应商 (自动使用 Windows DPAPI 加密 API Key)
.\target\release\clowder.exe cli add-provider openrouter "OpenRouter" openai https://openrouter.ai/api/v1 sk-or-v1-xxx direct

# 设置总控默认供应商并一键写入所有已启用 Agent 的配置文件
.\target\release\clowder.exe cli set-global openrouter --sync

# 为单个 Agent (如 pi 或 dsh-desktop) 独立指定供应商与默认模型
.\target\release\clowder.exe cli set-agent pi openrouter claude-sonnet-4-5
.\target\release\clowder.exe cli set-agent dsh-desktop openrouter deepseek-v4-flash
```

### 字体子集

`assets/fonts/clowder-cjk-{sans,serif}.ttf` 是 **Noto Sans SC / Noto Serif SC** (SIL OFL 1.1) 先 instanced 到
wght=400、再裁到 GB2312 一二级字 + 界面用字后的静态子集, 合计约 5MB, 随仓库和 release 一起分发。

要重新生成: `python scripts/build-fonts.py` (需 fonttools, 且本机装有那两个 Noto VF)。
分发前必须附上 `assets/fonts/OFL.txt` (OFL 1.1 全文), 打 tag 的 CI 会做 license gate 检查。

## 支持的 Agent 与专属代理路径

| Agent | 标识 (`agent_slug`) | 本地代理路由 | 自动写入配置文件路径 |
|---|---|---|---|
| Claude Code | `claude-code` | `http://127.0.0.1:8787/a/claude-code/anthropic` | `~/.claude/settings.json` |
| Claude Desktop | `claude-desktop` | `http://127.0.0.1:8787/a/claude-desktop/anthropic` | `%APPDATA%\Claude\claude_desktop_config.json` |
| Codex CLI | `codex` | `http://127.0.0.1:8787/a/codex/openai/v1` | `~/.codex/config.toml` |
| Codex Desktop | `codex-desktop` | `http://127.0.0.1:8787/a/codex-desktop/openai/v1` | `~/.codex/config.toml` |
| OpenCode v2 | `opencodev2` | `http://127.0.0.1:8787/a/opencodev2/openai/v1` | `~/.config/opencode/opencode.json` |
| Pi Coding Agent | `pi` | `http://127.0.0.1:8787/a/pi/openai/v1` | `~/.pi/agent/models.json` |
| DSH CLI | `dsh` | `http://127.0.0.1:8787/a/dsh/openai/v1` | `~/.dsh/settings.yaml` + `~/.dsh/.env` |
| DSH Desktop | `dsh-desktop` | `http://127.0.0.1:8787/a/dsh-desktop/openai/v1` | `~/.dsh/settings.yaml` + `~/.dsh/.env` |

## 许可证

MIT
