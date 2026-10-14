use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub proxy: ProxyConfig,
    #[serde(default)]
    pub providers: Vec<Provider>,
    /// 总控默认供应商 ID (Global Default Provider)
    #[serde(default)]
    pub active_provider_id: Option<String>,
    /// 当前在 GUI 右上角 1/4 圆选中的上下文 Agent (None 表示“总控 Global”)
    #[serde(default)]
    pub current_agent: Option<AgentId>,
    /// 各 Agent 的专属配置与路由策略
    #[serde(default = "default_agent_profiles")]
    pub agents: Vec<AgentProfile>,
    #[serde(default)]
    pub pricing: PricingConfig,
    #[serde(default)]
    pub ui: UiConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            proxy: ProxyConfig::default(),
            providers: Vec::new(),
            active_provider_id: None,
            current_agent: None,
            agents: default_agent_profiles(),
            pricing: PricingConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum AgentId {
    ClaudeCode,
    ClaudeDesktop,
    Codex,
    CodexDesktop,
    OpenCodeV2,
    Pi,
    Dsh,
    DshDesktop,
}

impl AgentId {
    pub const ALL: [AgentId; 8] = [
        AgentId::ClaudeCode,
        AgentId::ClaudeDesktop,
        AgentId::Codex,
        AgentId::CodexDesktop,
        AgentId::OpenCodeV2,
        AgentId::Pi,
        AgentId::Dsh,
        AgentId::DshDesktop,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::ClaudeDesktop => "claude-desktop",
            Self::Codex => "codex",
            Self::CodexDesktop => "codex-desktop",
            Self::OpenCodeV2 => "opencodev2",
            Self::Pi => "pi",
            Self::Dsh => "dsh",
            Self::DshDesktop => "dsh-desktop",
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "claude-code" | "claudecode" | "claude_code" | "claude" | "cc" => Some(Self::ClaudeCode),
            "claude-desktop" | "claudedesktop" | "claude_desktop" | "cd" => Some(Self::ClaudeDesktop),
            "codex" | "codex-cli" | "cx" => Some(Self::Codex),
            "codex-desktop" | "codexdesktop" | "codex_desktop" | "xd" => Some(Self::CodexDesktop),
            "opencodev2" | "opencode" | "opencode-v2" | "oc" => Some(Self::OpenCodeV2),
            "pi" | "pi-agent" | "pi-coding-agent" => Some(Self::Pi),
            "dsh" | "deepseek-harness" | "ds" => Some(Self::Dsh),
            "dsh-desktop" | "dshdesktop" | "dsh_desktop" | "dd" => Some(Self::DshDesktop),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::ClaudeDesktop => "Claude Desktop",
            Self::Codex => "Codex CLI",
            Self::CodexDesktop => "Codex Desktop",
            Self::OpenCodeV2 => "OpenCode v2",
            Self::Pi => "Pi Agent",
            Self::Dsh => "DSH CLI",
            Self::DshDesktop => "DSH Desktop",
        }
    }

    pub fn badge(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "CC",
            Self::ClaudeDesktop => "CD",
            Self::Codex => "CX",
            Self::CodexDesktop => "XD",
            Self::OpenCodeV2 => "OC",
            Self::Pi => "PI",
            Self::Dsh => "DS",
            Self::DshDesktop => "DD",
        }
    }

    pub fn preferred_protocol(&self) -> Protocol {
        match self {
            Self::ClaudeCode | Self::ClaudeDesktop => Protocol::Anthropic,
            Self::Codex
            | Self::CodexDesktop
            | Self::OpenCodeV2
            | Self::Pi
            | Self::Dsh
            | Self::DshDesktop => Protocol::OpenAI,
        }
    }

    pub fn default_launch_cmd(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::ClaudeDesktop => "Claude",
            Self::Codex => "codex",
            Self::CodexDesktop => "Codex",
            Self::OpenCodeV2 => "opencode",
            Self::Pi => "pi",
            Self::Dsh => "npx @deepseek-ai/dsh web",
            Self::DshDesktop => "dsh-desktop",
        }
    }

    pub fn config_file_path(&self) -> Option<PathBuf> {
        let home = user_home_dir()?;
        match self {
            Self::ClaudeCode => Some(home.join(".claude").join("settings.json")),
            Self::ClaudeDesktop => {
                if let Ok(appdata) = std::env::var("APPDATA") {
                    Some(PathBuf::from(appdata).join("Claude").join("claude_desktop_config.json"))
                } else {
                    Some(home.join(".claude").join("claude_desktop_config.json"))
                }
            }
            Self::Codex | Self::CodexDesktop => {
                if let Ok(codex_home) = std::env::var("CODEX_HOME") {
                    if !codex_home.trim().is_empty() {
                        return Some(PathBuf::from(codex_home).join("config.toml"));
                    }
                }
                Some(home.join(".codex").join("config.toml"))
            }
            Self::OpenCodeV2 => Some(home.join(".config").join("opencode").join("opencode.json")),
            Self::Pi => Some(home.join(".pi").join("agent").join("models.json")),
            Self::Dsh | Self::DshDesktop => {
                if let Ok(dsh_home) = std::env::var("DSH_HOME") {
                    if !dsh_home.trim().is_empty() {
                        return Some(PathBuf::from(dsh_home).join("settings.yaml"));
                    }
                }
                Some(home.join(".dsh").join("settings.yaml"))
            }
        }
    }

    pub fn session_dir_path(&self) -> Option<PathBuf> {
        let home = user_home_dir()?;
        match self {
            Self::ClaudeCode | Self::ClaudeDesktop => Some(home.join(".claude").join("projects")),
            Self::Codex | Self::CodexDesktop => {
                if let Ok(codex_home) = std::env::var("CODEX_HOME") {
                    if !codex_home.trim().is_empty() {
                        return Some(PathBuf::from(codex_home).join("sessions"));
                    }
                }
                Some(home.join(".codex").join("sessions"))
            }
            Self::OpenCodeV2 => Some(
                home.join(".local")
                    .join("share")
                    .join("opencode")
                    .join("storage")
                    .join("session"),
            ),
            Self::Pi => Some(home.join(".pi").join("agent").join("sessions")),
            Self::Dsh | Self::DshDesktop => {
                if let Ok(dsh_home) = std::env::var("DSH_HOME") {
                    if !dsh_home.trim().is_empty() {
                        return Some(PathBuf::from(dsh_home).join("storages"));
                    }
                }
                Some(home.join(".dsh").join("storages"))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentRouteMode {
    #[default]
    FollowGlobal,
    Dedicated,
}

impl std::fmt::Display for AgentRouteMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FollowGlobal => write!(f, "跟随总控"),
            Self::Dedicated => write!(f, "独立配置"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub agent: AgentId,
    #[serde(default = "default_true")]
    pub enabled_in_switcher: bool,
    #[serde(default)]
    pub route_mode: AgentRouteMode,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub custom_launch_cmd: Option<String>,
}

impl AgentProfile {
    pub fn new(agent: AgentId) -> Self {
        Self {
            agent,
            enabled_in_switcher: true,
            route_mode: AgentRouteMode::FollowGlobal,
            provider_id: None,
            default_model: None,
            custom_launch_cmd: None,
        }
    }

    pub fn effective_launch_cmd(&self) -> &str {
        self.custom_launch_cmd
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| self.agent.default_launch_cmd())
    }
}

pub fn default_agent_profiles() -> Vec<AgentProfile> {
    AgentId::ALL.into_iter().map(AgentProfile::new).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_true")]
    pub auto_start: bool,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            auto_start: true,
        }
    }
}

fn default_listen() -> String {
    "127.0.0.1:8787".to_string()
}

/// 供应商认证方式:
/// - ApiKey: 常规 DPAPI 加密 API Key
/// - ClaudeCodeOauth: 读取本机 Claude Code 会员登录凭证 (~/.claude/.credentials.json)，可在 Codex / OpenCode / Pi / DSH 中跨协议复用 Claude 会员额度
/// - CodexOauth: 读取本机 Codex ChatGPT Plus/Pro 会员登录凭证 (~/.codex/auth.json 或本地暂存)，可在 Claude Code / OpenCode / Pi 中复用 Codex 会员额度
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    #[default]
    ApiKey,
    ClaudeCodeOauth,
    CodexOauth,
}

impl std::fmt::Display for AuthKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey => write!(f, "API Key"),
            Self::ClaudeCodeOauth => write!(f, "Claude 会员 OAuth"),
            Self::CodexOauth => write!(f, "Codex 会员 OAuth"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub protocol: Protocol,
    pub base_url: String,
    /// API key, DPAPI-encrypted base64 when stored on disk
    #[serde(default)]
    pub api_key_encrypted: String,
    /// 认证类型: 支持 API Key、Claude Code 会员 OAuth、Codex 会员 OAuth
    #[serde(default)]
    pub auth_kind: AuthKind,
    /// 当跨 Agent 使用 Claude 会员 OAuth (例如在 Codex 里用 Claude 会员) 时，自动模拟 Claude Code 客户端请求头与标识
    #[serde(default = "default_true")]
    pub emulate_claude_code: bool,
    #[serde(default)]
    pub upstream_strategy: UpstreamStrategy,
    #[serde(default)]
    pub custom_proxy_url: Option<String>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Anthropic,
    OpenAI,
}

impl Protocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Protocol::Anthropic => "anthropic",
            Protocol::OpenAI => "openai",
        }
    }

    pub fn from_db(s: &str) -> Self {
        match s.trim_matches('"') {
            "anthropic" => Protocol::Anthropic,
            _ => Protocol::OpenAI,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamStrategy {
    Direct,
    #[default]
    SystemProxy,
    Custom,
}

impl std::fmt::Display for UpstreamStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Direct => write!(f, "直连 (国内免梯子)"),
            Self::SystemProxy => write!(f, "系统代理 (自动串联梯子)"),
            Self::Custom => write!(f, "指定梯子端口"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingConfig {
    #[serde(default = "default_true")]
    pub litellm_enabled: bool,
    #[serde(default = "default_true")]
    pub openrouter_enabled: bool,
    #[serde(default = "default_refresh_hours")]
    pub refresh_hours: u64,
    #[serde(default)]
    pub usd_cny: Option<f64>,
}

impl Default for PricingConfig {
    fn default() -> Self {
        Self {
            litellm_enabled: true,
            openrouter_enabled: true,
            refresh_hours: default_refresh_hours(),
            usd_cny: None,
        }
    }
}

fn default_refresh_hours() -> u64 {
    24
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default)]
    pub theme: Theme,
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default = "default_true")]
    pub show_corner_switcher: bool,
    #[serde(default = "default_true")]
    pub show_dashboard_tab: bool,
    #[serde(default = "default_true")]
    pub show_providers_tab: bool,
    #[serde(default = "default_true")]
    pub show_sessions_tab: bool,
    #[serde(default)]
    pub session_consent_granted: bool,
    #[serde(default = "default_true")]
    pub scan_local_agent_sessions: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            minimize_to_tray: true,
            autostart: false,
            show_corner_switcher: true,
            show_dashboard_tab: true,
            show_providers_tab: true,
            show_sessions_tab: true,
            session_consent_granted: false,
            scan_local_agent_sessions: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

fn default_true() -> bool {
    true
}

impl Provider {
    pub fn new_template(protocol: Protocol) -> Self {
        let (id, name, base_url) = match protocol {
            Protocol::Anthropic => ("anthropic", "Anthropic", "https://api.anthropic.com"),
            Protocol::OpenAI => ("openai", "OpenAI", "https://api.openai.com"),
        };
        Self {
            id: id.to_string(),
            name: name.to_string(),
            protocol,
            base_url: base_url.to_string(),
            api_key_encrypted: String::new(),
            auth_kind: AuthKind::ApiKey,
            emulate_claude_code: false,
            upstream_strategy: UpstreamStrategy::SystemProxy,
            custom_proxy_url: None,
            headers: Vec::new(),
        }
    }

    pub fn openrouter() -> Self {
        Self {
            id: "openrouter".to_string(),
            name: "OpenRouter".to_string(),
            protocol: Protocol::OpenAI,
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key_encrypted: String::new(),
            auth_kind: AuthKind::ApiKey,
            emulate_claude_code: false,
            upstream_strategy: UpstreamStrategy::SystemProxy,
            custom_proxy_url: None,
            headers: Vec::new(),
        }
    }

    /// Claude Code Pro/Max 会员 OAuth 供应商模板:
    /// 自动读取 ~/.claude/.credentials.json，支持通过本地协议转换桥直接在 Codex / OpenCode / Pi / DSH 中使用 Claude 会员
    pub fn claude_oauth() -> Self {
        Self {
            id: "claude-oauth".to_string(),
            name: "Claude Code 会员 (OAuth)".to_string(),
            protocol: Protocol::Anthropic,
            base_url: "https://api.anthropic.com".to_string(),
            api_key_encrypted: String::new(),
            auth_kind: AuthKind::ClaudeCodeOauth,
            emulate_claude_code: true,
            upstream_strategy: UpstreamStrategy::SystemProxy,
            custom_proxy_url: None,
            headers: Vec::new(),
        }
    }

    /// Codex ChatGPT Plus/Pro 会员 OAuth 供应商模板:
    /// 自动读取 ~/.codex/auth.json (或本地安全暂存)，支持通过本地协议转换桥在 Claude Code / OpenCode / Pi 中使用 Codex 会员
    pub fn codex_oauth() -> Self {
        Self {
            id: "codex-oauth".to_string(),
            name: "Codex ChatGPT 会员 (OAuth)".to_string(),
            protocol: Protocol::OpenAI,
            base_url: "https://api.openai.com/v1".to_string(),
            api_key_encrypted: String::new(),
            auth_kind: AuthKind::CodexOauth,
            emulate_claude_code: false,
            upstream_strategy: UpstreamStrategy::SystemProxy,
            custom_proxy_url: None,
            headers: Vec::new(),
        }
    }
}

pub fn user_home_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(PathBuf::from)
}

/// 读取本机 Claude Code OAuth 会员凭证 (~/.claude/.credentials.json 或 $CLAUDE_CONFIG_DIR/.credentials.json)
/// 仅在本机内存读取 accessToken 用于向 api.anthropic.com 发起请求，0 第三方上传
pub fn read_claude_code_oauth_token() -> Option<String> {
    let cred_path = std::env::var("CLAUDE_CONFIG_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|d| PathBuf::from(d).join(".credentials.json"))
        .or_else(|| {
            let home = user_home_dir()?;
            let dot = home.join(".claude").join(".credentials.json");
            if dot.exists() {
                Some(dot)
            } else {
                Some(home.join(".claude").join("credentials.json"))
            }
        })?;

    let raw = std::fs::read_to_string(cred_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    json.pointer("/claudeAiOauth/accessToken")
        .or_else(|| json.get("accessToken"))
        .or_else(|| json.get("access_token"))
        .or_else(|| json.get("apiKey"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 读取本机 Codex OAuth 会员凭证 (~/.codex/auth.json 或 Clowder 本地暂存的 codex-login-stash.json)
pub fn read_codex_oauth_token() -> Option<String> {
    let candidates: Vec<PathBuf> = [
        std::env::var("CODEX_HOME")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(|d| PathBuf::from(d).join("auth.json")),
        user_home_dir().map(|h| h.join(".codex").join("auth.json")),
        data_dir().ok().map(|d| d.join("codex-login-stash.json")),
    ]
    .into_iter()
    .flatten()
    .collect();

    for path in candidates {
        if !path.exists() {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        if let Some(tok) = json
            .pointer("/tokens/access_token")
            .or_else(|| json.get("access_token"))
            .or_else(|| json.get("OPENAI_API_KEY"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "clowder-proxy")
        {
            return Some(tok);
        }
    }
    None
}

/// 保护本机 ~/.codex/auth.json 官方 OAuth 登录状态，自动暂存一份到 data/codex-login-stash.json
pub fn stash_codex_official_login() {
    let Some(home) = user_home_dir() else {
        return;
    };
    let auth_path = home.join(".codex").join("auth.json");
    if !auth_path.exists() {
        return;
    }
    let Ok(raw) = std::fs::read_to_string(&auth_path) else {
        return;
    };
    if raw.contains("access_token") && !raw.contains("clowder-proxy") {
        if let Ok(dir) = data_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("codex-login-stash.json"), raw);
        }
    }
}

pub fn data_dir() -> anyhow::Result<PathBuf> {
    let proj = directories::ProjectDirs::from("com", "clowder", "clowder")
        .ok_or_else(|| anyhow::anyhow!("cannot determine data dir"))?;
    Ok(proj.data_local_dir().to_path_buf())
}

fn legacy_data_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "yuantuanswitch", "yuantuanswitch")
        .map(|p| p.data_local_dir().to_path_buf())
}

pub fn migrate_legacy_data() -> anyhow::Result<usize> {
    let new_dir = data_dir()?;
    let Some(legacy) = legacy_data_dir().filter(|p| p.exists()) else {
        return Ok(0);
    };
    let dirty = std::fs::read_dir(&new_dir)
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);
    if dirty {
        return Ok(0);
    }
    std::fs::create_dir_all(&new_dir)?;
    copy_tree(&legacy, &new_dir)
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> anyhow::Result<usize> {
    let mut copied = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let name = match name.strip_prefix("yts.") {
            Some(rest) => format!("clowder.{rest}"),
            None => name,
        };
        let target = to.join(name);
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            std::fs::create_dir_all(&target)?;
            copied += copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() && !target.exists() {
            std::fs::copy(entry.path(), &target)?;
            copied += 1;
        }
    }
    Ok(copied)
}

pub fn config_path() -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join("config.toml"))
}

pub fn db_path() -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join("clowder.db"))
}

pub fn pricing_cache_path() -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join("pricing_cache.json"))
}

fn backup_file_once(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut bak = path.as_os_str().to_os_string();
    bak.push(".clowder.bak");
    let bak_path = PathBuf::from(bak);
    if !bak_path.exists() {
        std::fs::copy(path, &bak_path)?;
    }
    Ok(())
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            let cfg = Config::default();
            cfg.save()?;
            return Ok(cfg);
        }
        let s = std::fs::read_to_string(&path)?;
        let mut cfg: Config = toml::from_str(&s)?;
        cfg.ensure_all_agents();
        Ok(cfg)
    }

    pub fn ensure_all_agents(&mut self) {
        for agent in AgentId::ALL {
            if !self.agents.iter().any(|a| a.agent == agent) {
                self.agents.push(AgentProfile::new(agent));
            }
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s = toml::to_string_pretty(self)?;
        std::fs::write(&path, s)?;
        Ok(())
    }

    pub fn active_provider(&self) -> Option<&Provider> {
        self.active_provider_id
            .as_ref()
            .and_then(|id| self.providers.iter().find(|p| &p.id == id))
    }

    pub fn provider_by_id(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }

    pub fn agent_profile(&self, agent: AgentId) -> Option<&AgentProfile> {
        self.agents.iter().find(|a| a.agent == agent)
    }

    pub fn agent_profile_mut(&mut self, agent: AgentId) -> &mut AgentProfile {
        self.ensure_all_agents();
        self.agents
            .iter_mut()
            .find(|a| a.agent == agent)
            .expect("agent profile ensured")
    }

    pub fn resolve_provider_for_agent(
        &self,
        agent: Option<AgentId>,
        fallback_proto: Option<Protocol>,
    ) -> Option<&Provider> {
        if let Some(ag) = agent {
            if let Some(prof) = self.agent_profile(ag) {
                if prof.route_mode == AgentRouteMode::Dedicated {
                    if let Some(pid) = &prof.provider_id {
                        if let Some(p) = self.provider_by_id(pid) {
                            return Some(p);
                        }
                    }
                }
            }
        }
        if let Some(global) = self.active_provider() {
            return Some(global);
        }
        if let Some(proto) = fallback_proto.or_else(|| agent.map(|a| a.preferred_protocol())) {
            if let Some(p) = self.providers.iter().find(|p| p.protocol == proto) {
                return Some(p);
            }
        }
        self.providers.first()
    }

    /// 非侵入式写入目标 Agent 本地配置:
    /// - 每个 Agent 始终保持自己最原生的协议请求路径 (例如 Claude Code 始终发 /anthropic, Codex 始终发 /openai/v1 Responses)；
    /// - 当绑定的供应商协议与 Agent 原生协议不同时 (例如在 Codex 里绑定了 Claude 会员 OAuth)，由 Clowder 本地代理桥自动完成 Responses <-> Anthropic Messages 互转!
    pub fn write_agent_local_config(&self, agent: AgentId) -> anyhow::Result<String> {
        let provider = self
            .resolve_provider_for_agent(Some(agent), Some(agent.preferred_protocol()))
            .ok_or_else(|| anyhow::anyhow!("未找到可用供应商，请先在总控或当前 Agent 下添加并激活供应商"))?;
        let listen = &self.proxy.listen;
        let slug = agent.as_str();
        let default_model = self
            .agent_profile(agent)
            .and_then(|p| p.default_model.clone())
            .filter(|s| !s.trim().is_empty());

        let path = agent
            .config_file_path()
            .ok_or_else(|| anyhow::anyhow!("无法定位用户主目录"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        backup_file_once(&path)?;

        match agent {
            AgentId::ClaudeCode => {
                let mut json: serde_json::Value = if path.exists() {
                    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_default())
                        .unwrap_or_else(|_| serde_json::json!({}))
                } else {
                    serde_json::json!({})
                };
                if !json.is_object() {
                    json = serde_json::json!({});
                }
                let env = json
                    .as_object_mut()
                    .unwrap()
                    .entry("env")
                    .or_insert_with(|| serde_json::json!({}));
                if !env.is_object() {
                    *env = serde_json::json!({});
                }
                let env_obj = env.as_object_mut().unwrap();
                // Claude Code 始终指向 /a/claude-code/anthropic; 若底层绑定了 Codex OAuth 或 OpenAI 供应商，Clowder 自动做 Messages -> Responses/Chat 互转
                env_obj.insert(
                    "ANTHROPIC_BASE_URL".into(),
                    serde_json::Value::String(format!("http://{}/a/{}/anthropic", listen, slug)),
                );
                env_obj.insert(
                    "NO_PROXY".into(),
                    serde_json::Value::String("127.0.0.1,localhost".into()),
                );
                env_obj.insert(
                    "no_proxy".into(),
                    serde_json::Value::String("127.0.0.1,localhost".into()),
                );
                if let Some(m) = default_model {
                    env_obj.insert("ANTHROPIC_MODEL".into(), serde_json::Value::String(m));
                }
                std::fs::write(&path, serde_json::to_string_pretty(&json)?)?;
            }
            AgentId::ClaudeDesktop => {
                let mut json: serde_json::Value = if path.exists() {
                    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_default())
                        .unwrap_or_else(|_| serde_json::json!({}))
                } else {
                    serde_json::json!({})
                };
                if !json.is_object() {
                    json = serde_json::json!({});
                }
                let obj = json.as_object_mut().unwrap();
                obj.insert(
                    "thirdPartyInference".into(),
                    serde_json::json!({
                        "credentialKind": "static_api_key",
                        "gatewayBaseUrl": format!("http://{}/a/{}/anthropic", listen, slug),
                        "gatewayAuthScheme": "x-api-key",
                        "gatewayApiKey": "clowder-proxy"
                    }),
                );
                std::fs::write(&path, serde_json::to_string_pretty(&json)?)?;
            }
            AgentId::Codex | AgentId::CodexDesktop => {
                // 先保护性备份 ~/.codex/auth.json 的官方登录凭证，绝不破坏用户原有 Codex OAuth 登录
                stash_codex_official_login();

                let mut doc: toml::Value = if path.exists() {
                    toml::from_str(&std::fs::read_to_string(&path).unwrap_or_default())
                        .unwrap_or_else(|_| toml::Value::Table(Default::default()))
                } else {
                    toml::Value::Table(Default::default())
                };
                if let Some(table) = doc.as_table_mut() {
                    table.insert(
                        "model_provider".into(),
                        toml::Value::String("clowder".into()),
                    );
                    if let Some(m) = default_model.or_else(|| {
                        if provider.protocol == Protocol::Anthropic {
                            Some("claude-sonnet-4-5".into())
                        } else {
                            None
                        }
                    }) {
                        table.insert("model".into(), toml::Value::String(m));
                    }
                    let providers_entry = table
                        .entry("model_providers")
                        .or_insert_with(|| toml::Value::Table(Default::default()));
                    if let Some(providers_tbl) = providers_entry.as_table_mut() {
                        let mut clowder_tbl = toml::map::Map::new();
                        clowder_tbl
                            .insert("name".into(), toml::Value::String("Clowder Proxy".into()));
                        // Codex 始终发往 /a/codex/openai/v1 (wire_api = "responses")；
                        // 若底层选了 Claude Code 会员 (OAuth) 或 Anthropic 供应商，Clowder 自动将 /responses 转为 /v1/messages!
                        clowder_tbl.insert(
                            "base_url".into(),
                            toml::Value::String(format!("http://{}/a/{}/openai/v1", listen, slug)),
                        );
                        clowder_tbl
                            .insert("wire_api".into(), toml::Value::String("responses".into()));
                        providers_tbl.insert("clowder".into(), toml::Value::Table(clowder_tbl));
                    }
                }
                std::fs::write(&path, toml::to_string_pretty(&doc)?)?;
            }
            AgentId::OpenCodeV2 => {
                let mut json: serde_json::Value = if path.exists() {
                    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_default())
                        .unwrap_or_else(|_| serde_json::json!({}))
                } else {
                    serde_json::json!({})
                };
                if !json.is_object() {
                    json = serde_json::json!({});
                }
                let obj = json.as_object_mut().unwrap();
                obj.entry("$schema").or_insert_with(|| {
                    serde_json::Value::String("https://opencode.ai/config.json".into())
                });
                let prov = obj
                    .entry("provider")
                    .or_insert_with(|| serde_json::json!({}));
                if let Some(prov_obj) = prov.as_object_mut() {
                    prov_obj.insert(
                        "clowder".into(),
                        serde_json::json!({
                            "name": format!("Clowder ({})", provider.name),
                            "options": {
                                "baseURL": format!("http://{}/a/{}/openai/v1", listen, slug),
                                "apiKey": "clowder-proxy"
                            }
                        }),
                    );
                }
                std::fs::write(&path, serde_json::to_string_pretty(&json)?)?;
            }
            AgentId::Pi => {
                let mut json: serde_json::Value = if path.exists() {
                    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_default())
                        .unwrap_or_else(|_| serde_json::json!({}))
                } else {
                    serde_json::json!({})
                };
                if !json.is_object() {
                    json = serde_json::json!({});
                }
                let providers_val = json
                    .as_object_mut()
                    .unwrap()
                    .entry("providers")
                    .or_insert_with(|| serde_json::json!({}));
                if !providers_val.is_object() {
                    *providers_val = serde_json::json!({});
                }
                let model_id = default_model.unwrap_or_else(|| "claude-sonnet-4-5".into());
                let api_kind = match provider.protocol {
                    Protocol::Anthropic => "anthropic-messages",
                    Protocol::OpenAI => "openai-completions",
                };
                let base_url = match provider.protocol {
                    Protocol::Anthropic => format!("http://{}/a/{}/anthropic", listen, slug),
                    Protocol::OpenAI => format!("http://{}/a/{}/openai/v1", listen, slug),
                };
                providers_val.as_object_mut().unwrap().insert(
                    "clowder".into(),
                    serde_json::json!({
                        "baseUrl": base_url,
                        "api": api_kind,
                        "apiKey": "clowder-proxy",
                        "models": [
                            {
                                "id": model_id,
                                "name": format!("{} via Clowder", provider.name),
                                "reasoning": true,
                                "contextWindow": 128000,
                                "maxTokens": 16384
                            }
                        ]
                    }),
                );
                std::fs::write(&path, serde_json::to_string_pretty(&json)?)?;
            }
            AgentId::Dsh | AgentId::DshDesktop => {
                let model_id = default_model.unwrap_or_else(|| "deepseek-v4-flash".into());
                let base_url = match provider.protocol {
                    Protocol::Anthropic => format!("http://{}/a/{}/anthropic", listen, slug),
                    Protocol::OpenAI => format!("http://{}/a/{}/openai/v1", listen, slug),
                };
                let api_kind = match provider.protocol {
                    Protocol::Anthropic => "anthropic-messages",
                    Protocol::OpenAI => "openai-completions",
                };
                let yaml_content = format!(
                    "# Managed by Clowder Local Proxy\n\
agent-default-model:\n\
  provider: clowder\n\
  model: {model_id}\n\
llm-pi-ai:\n\
  providers:\n\
    clowder:\n\
      apiKeyEnv: CLOWDER_API_KEY\n\
      api: {api_kind}\n\
      baseURL: {base_url}\n\
      defaultInput: [text, image]\n\
      models:\n\
        - id: {model_id}\n\
          input: [text, image]\n"
                );
                std::fs::write(&path, yaml_content)?;
                if let Some(parent) = path.parent() {
                    let env_path = parent.join(".env");
                    backup_file_once(&env_path)?;
                    let env_line = format!(
                        "CLOWDER_API_KEY=clowder-proxy\nDEEPSEEK_BASE_URL={base_url}\nDEEPSEEK_API_KEY=clowder-proxy\nNO_PROXY=127.0.0.1,localhost\nno_proxy=127.0.0.1,localhost\n"
                    );
                    let _ = std::fs::write(env_path, env_line);
                }
            }
        }

        let cross_note = if (matches!(agent, AgentId::Codex | AgentId::CodexDesktop | AgentId::OpenCodeV2)
            && provider.protocol == Protocol::Anthropic)
            || (matches!(agent, AgentId::ClaudeCode | AgentId::ClaudeDesktop)
                && provider.protocol == Protocol::OpenAI)
        {
            " [已启用跨协议桥接]"
        } else {
            ""
        };

        Ok(format!(
            "已将 [{}] 配置写入 {} (供应商: {}{})",
            agent.label(),
            path.display(),
            provider.name,
            cross_note
        ))
    }

    pub fn restore_agent_local_config(&self, agent: AgentId) -> anyhow::Result<String> {
        let path = agent
            .config_file_path()
            .ok_or_else(|| anyhow::anyhow!("无法定位用户主目录"))?;
        let mut bak = path.as_os_str().to_os_string();
        bak.push(".clowder.bak");
        let bak_path = PathBuf::from(bak);
        if bak_path.exists() {
            std::fs::copy(&bak_path, &path)?;
            return Ok(format!(
                "已从备份还原 [{}] 原始配置 ({})",
                agent.label(),
                path.display()
            ));
        }
        Ok(format!(
            "[{}] 未发现 .clowder.bak 备份，无需还原",
            agent.label()
        ))
    }

    pub fn sync_all_enabled_agents(&self) -> Vec<String> {
        let mut results = Vec::new();
        for prof in &self.agents {
            if prof.enabled_in_switcher {
                match self.write_agent_local_config(prof.agent) {
                    Ok(msg) => results.push(msg),
                    Err(e) => results.push(format!("[{}] 写入失败: {}", prof.agent.label(), e)),
                }
            }
        }
        results
    }
}
