use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub proxy: ProxyConfig,
    #[serde(default)]
    pub providers: Vec<Provider>,
    #[serde(default)]
    pub active_provider_id: Option<String>,
    #[serde(default)]
    pub pricing: PricingConfig,
    #[serde(default)]
    pub ui: UiConfig,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub protocol: Protocol,
    pub base_url: String,
    /// API key, DPAPI-encrypted base64 when stored on disk
    pub api_key_encrypted: String,
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
    /// 库里存纯文本; 之前存的是 serde_json 序列化结果 (带引号), SQL 里 `= 'anthropic'` 永远匹配不上
    pub fn as_str(&self) -> &'static str {
        match self {
            Protocol::Anthropic => "anthropic",
            Protocol::OpenAI => "openai",
        }
    }

    /// 兼容旧的 `"anthropic"` 带引号写法
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
    #[default]
    Direct,
    SystemProxy,
    Custom,
}

impl std::fmt::Display for UpstreamStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Direct => write!(f, "直连"),
            Self::SystemProxy => write!(f, "系统代理"),
            Self::Custom => write!(f, "自定义"),
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
    /// USD -> CNY rate; None = don't convert
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
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            minimize_to_tray: true,
            autostart: false,
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
            upstream_strategy: UpstreamStrategy::Direct,
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
            upstream_strategy: UpstreamStrategy::Direct,
            custom_proxy_url: None,
            headers: Vec::new(),
        }
    }
}

pub fn data_dir() -> anyhow::Result<PathBuf> {
    let proj = directories::ProjectDirs::from("com", "clowder", "clowder")
        .ok_or_else(|| anyhow::anyhow!("cannot determine data dir"))?;
    Ok(proj.data_local_dir().to_path_buf())
}

/// 改名前的工程标识, 老数据躺在它下面
fn legacy_data_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "yuantuanswitch", "yuantuanswitch")
        .map(|p| p.data_local_dir().to_path_buf())
}

/// 一次性把旧 yuantuanswitch 目录搬到 clowder 目录, 顺带把 yts.* 文件名改成 clowder.*
/// (yts.db / yts.db-wal / yts.db-shm / yts.log)。新目录非空就什么都不做, 旧目录只拷不删,
/// 留个回滚余地。返回迁移的文件数。
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

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            let cfg = Config::default();
            cfg.save()?;
            return Ok(cfg);
        }
        let s = std::fs::read_to_string(&path)?;
        let cfg: Config = toml::from_str(&s)?;
        Ok(cfg)
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
}
