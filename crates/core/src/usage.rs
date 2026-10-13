use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub id: String,
    pub ts: DateTime<Utc>,
    pub provider_id: String,
    pub provider_name: String,
    pub protocol: crate::config::Protocol,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub ttft_ms: Option<u64>,
    pub duration_ms: u64,
    pub tps: Option<f64>,
    pub cost_usd: Option<f64>,
    pub session_id: Option<String>,
    /// true when token counts are estimated (no upstream usage chunk)
    pub estimated: bool,
    pub status: Status,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Error,
    Aborted,
}

impl Status {
    /// 库里存纯文本; 之前存的是 serde_json 结果 (带引号), 导致 `WHERE status = 'ok'` 一条都匹配不到,
    /// 今日统计恒为 0
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Error => "error",
            Status::Aborted => "aborted",
        }
    }

    /// 兼容旧的 `"ok"` 带引号写法
    pub fn from_db(s: &str) -> Self {
        match s.trim_matches('"') {
            "error" => Status::Error,
            "aborted" => Status::Aborted,
            _ => Status::Ok,
        }
    }
}

impl UsageRecord {
    pub fn new(
        provider_id: String,
        provider_name: String,
        protocol: crate::config::Protocol,
        model: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            ts: Utc::now(),
            provider_id,
            provider_name,
            protocol,
            model,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            ttft_ms: None,
            duration_ms: 0,
            tps: None,
            cost_usd: None,
            session_id: None,
            estimated: false,
            status: Status::Ok,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageSummary {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_usd: f64,
    pub request_count: u64,
    pub avg_tps: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub started_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub request_count: u64,
    pub total_input: u64,
    pub total_output: u64,
    pub total_cost: f64,
}
