# Clowder

Rust 实现的 **本地优先** LLM API 供应商切换 + 实时 token/TPS 统计工具 (Windows)。

## 核心特性

- **实时统计**: 本地 HTTP 代理拦截流式/非流式请求, 逐 chunk 解析 input/output token、TTFT、TPS、费用。
- **0 上传**: 除调用真实上游 API 外, 所有数据 (API key、日志、统计) 只留本地; 无遥测、无上报。
- **更省**: egui 原生 GUI (无 C 依赖, 单 exe ~10MB), SQLite WAL 存储, release 极致优化。中文字体走仓库内置的 OFL 子集, 空闲常驻内存 ~88 MB、CPU 几乎为 0 (见「字体子集」)。
- **防梯子干扰**: 客户端指向 `127.0.0.1:8787` (loopback), 上行由 GUI 独立配置 (直连/系统代理/自定义), 开全局梯子也不影响统计。
- **多协议**: 同时透传 Anthropic (`/anthropic/*`) 与 OpenAI (`/openai/*`) 协议, 内置 OpenRouter (`/openrouter/openai/*`)。
- **价格同步**: 自动拉取 LiteLLM 价目 + OpenRouter 价目, 本地缓存, 断网时回落到编译期内置快照; 支持本地覆盖。

## 快速开始

### 构建

```powershell
cargo build --release
.\target\release\clowder.exe
```

### 字体子集

`assets/fonts/clowder-cjk-{sans,serif}.ttf` 是 **Noto Sans SC / Noto Serif SC** (SIL OFL 1.1) 先 instanced 到
wght=400、再裁到 GB2312 一二级字 + 界面用字后的静态子集, 合计约 5MB, 随仓库和 release 一起分发。

系统 `msyh.ttc` / `simsun.ttc` 各约 19MB, 而 egui 要把整份字体常驻、ab_glyph 解析后再翻倍,
两份就是 +96MB; 换成子集后进程稳定 ~88MB。微软字体的派生子集不能分发, 所以走 OFL 这条路。

要重新生成: `python scripts/build-fonts.py` (需 fonttools, 且本机装有那两个 Noto VF)。
分发前补上 `assets/fonts/OFL.txt` (OFL 1.1 全文), 打 tag 的 CI 会检查。

### 配置供应商

1. 打开 GUI → 供应商 → 添加 Anthropic / OpenAI / OpenRouter。
2. 编辑供应商, 填入 Base URL 与 API Key (key 用 Windows DPAPI 加密存储, 仅当前用户可解)。
3. 选上行策略: 直连 / 系统代理 / 自定义 (如 `http://127.0.0.1:7890`)。
4. 点击 ✅ 激活, 然后点击 **写入 Claude Code 配置**。

### 使用 Claude Code

写入后 Claude Code 的 `~/.claude/settings.json` 会指向本地代理:

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "http://127.0.0.1:8787/anthropic"
  }
}
```

发起请求, GUI 仪表盘实时显示每次 input/output token、TPS 曲线、费用。

### 使用其他客户端 (LiteLLM / OpenRouter / 任意 OpenAI 兼容)

把 `base_url` 指向:

- Anthropic: `http://127.0.0.1:8787/anthropic`
- OpenAI: `http://127.0.0.1:8787/openai`
- OpenRouter: `http://127.0.0.1:8787/openrouter/openai`
- 指定供应商: `http://127.0.0.1:8787/p/{provider_id}`

## 数据位置

- 配置: `%LOCALAPPDATA%\clowder\clowder\data\config.toml`
- 数据库: `%LOCALAPPDATA%\clowder\clowder\data\clowder.db`
- 价格缓存: `%LOCALAPPDATA%\clowder\clowder\data\pricing_cache.json`
- 日志: `%LOCALAPPDATA%\clowder\clowder\data\clowder.log`
- 字体子集: 仓库 `assets/fonts/` (开发) 或 `%LOCALAPPDATA%\clowder\clowder\data\fonts\` (发布)

旧版叫 yuantuanswitch, 数据在 `%LOCALAPPDATA%\yuantuanswitch\yuantuanswitch\data\`; 首次启动会自动整份搬到上面的新路径 (`yts.db`/`yts.log` 一并改名), 旧目录只拷不删, 留作回滚。

## Headless 模式

```powershell
.\clowder.exe -s     # 只跑代理 + 统计, 不开窗; 日志走 stdout, 可重定向
.\clowder.exe -f     # 直接开主界面 (默认开贴桌面的小窗, 双击小窗或点 □ 进主界面)
```

GUI 子系统下终端不会等 `-s` 退出, 要前台阻塞用 `start /wait .\clowder.exe -s`。

只启动代理 + 统计, 不开 GUI。

## 与原版 ccswitch / cc-switch 对比

本机实测 (2026-10-13, 系统缩放 175%, 空闲状态, 单位 MB):

| 维度 | 原版 cc-switch (Tauri) | Clowder |
|---|---|---|
| 主进程 WorkingSet | 62.8 | 91.6 |
| 主进程 Private Bytes | 15.4 | 88.1 |
| 空闲 CPU (每 60s) | 未测 | ~0.04s |
| WebView2 子进程 (6 个) WorkingSet | 437.4 | 无 |
| **整棵进程树 WorkingSet** | **500.2** | **91.6** |
| **整棵进程树 Private Bytes** | **334.8** | **88.1** |
| 二进制 | 60-200 MB | 10.4 MB |
| 统计精度 | 依赖上游返回 | 流式逐 chunk 解析 + 本地估算兜底 |
| 实时性 | 读取落盘日志 | 代理拦截, 毫秒级 |
| 网络隔离 | 可能受全局代理影响 | loopback + 独立上行策略, 免疫 |
| 数据上传 | 可能有遥测 | 0 上传 |

任务管理器只看 `cc-switch.exe` 会以为它只有 60 MB, 实际 UI 全在 `msedgewebview2.exe` 那 6 个子进程里。
`-s` headless (只有代理 + SQLite) 实测 Private 3.7 MB。
GUI 空壳 (完全不加载中文字体) 的地板价约 58 MB Private / 86 MB WorkingSet — 这是 egui + glow + winit 的固定成本, 再往下就得换 GUI 方案。
复测脚本: `powershell -File scripts/mem-watch.ps1 -Label X -Max 120 -Step 5`。

## 许可证

MIT
