use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelPrice {
    pub model: String,
    /// USD per 1M input tokens
    pub input_per_m: f64,
    /// USD per 1M output tokens
    pub output_per_m: f64,
    /// USD per 1M cache read tokens
    #[serde(default)]
    pub cache_read_per_m: f64,
    /// USD per 1M cache write tokens
    #[serde(default)]
    pub cache_write_per_m: f64,
    pub source: PriceSource,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PriceSource {
    Litellm,
    OpenRouter,
    LocalOverride,
    #[default]
    Snapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PriceTable {
    #[serde(default)]
    pub models: HashMap<String, ModelPrice>,
    pub fetched_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl PriceTable {
    pub fn lookup(&self, model: &str) -> Option<&ModelPrice> {
        // exact first
        if let Some(p) = self.models.get(model) {
            return Some(p);
        }
        // common suffix fallback: openrouter/anthropic/claude-3-5-sonnet -> claude-3-5-sonnet
        let norm = model.replace("openrouter/", "").replace("anthropic/", "");
        if let Some(p) = self.models.get(&norm) {
            return Some(p);
        }
        // litellm keys often contain provider prefix like "claude-3-5-sonnet-20241022"
        self.models
            .iter()
            .find(|(k, _)| k.contains(&norm) || norm.contains(k.as_str()))
            .map(|(_, v)| v)
    }

    pub fn cost_usd(
        &self,
        model: &str,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
    ) -> Option<f64> {
        let p = self.lookup(model)?;
        Some(
            (input as f64 / 1_000_000.0) * p.input_per_m
                + (output as f64 / 1_000_000.0) * p.output_per_m
                + (cache_read as f64 / 1_000_000.0) * p.cache_read_per_m
                + (cache_write as f64 / 1_000_000.0) * p.cache_write_per_m,
        )
    }

    pub fn merge(&mut self, other: PriceTable) {
        for (k, v) in other.models {
            self.models.insert(k, v);
        }
    }
}

/// Compile-time fallback snapshot (kept tiny; only most common models)
pub const SNAPSHOT_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/pricing_snapshot.json"
));

pub fn snapshot_table() -> PriceTable {
    serde_json::from_str(SNAPSHOT_JSON).unwrap_or_default()
}

pub fn cache_path() -> anyhow::Result<std::path::PathBuf> {
    crate::config::pricing_cache_path()
}

pub fn load_cache() -> anyhow::Result<Option<PriceTable>> {
    let path = cache_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(&path)?;
    let table: PriceTable = serde_json::from_str(&s)?;
    Ok(Some(table))
}

pub fn save_cache(table: &PriceTable) -> anyhow::Result<()> {
    let path = cache_path()?;
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let s = serde_json::to_string_pretty(table)?;
    std::fs::write(&path, s)?;
    Ok(())
}

/// Resolve effective price table: local override > cache > snapshot
pub fn resolve_table(overrides: &[(String, ModelPrice)]) -> PriceTable {
    let mut table = load_cache().ok().flatten().unwrap_or_else(snapshot_table);
    for (model, price) in overrides {
        table.models.insert(model.clone(), price.clone());
    }
    table
}

/// LiteLLM 条目只取 4 个价格字段; 走 serde_json::Value 会把整份文件 (~2MB) 放大成几十 MB 常驻峰值
#[derive(Debug, Deserialize)]
struct LitellmEntry {
    #[serde(rename = "input_cost_per_token", default)]
    input: Option<f64>,
    #[serde(rename = "output_cost_per_token", default)]
    output: Option<f64>,
    #[serde(rename = "cache_read_input_token_cost", default)]
    cache_read: Option<f64>,
    #[serde(rename = "cache_creation_input_token_cost", default)]
    cache_write: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterRoot {
    #[serde(default)]
    data: Vec<OpenRouterModel>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModel {
    id: String,
    #[serde(default)]
    pricing: OpenRouterPricing,
}

#[derive(Debug, Default, Deserialize)]
struct OpenRouterPricing {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    completion: Option<String>,
}

/// 直接按目标类型解析; 上游结构变化时退回 Value 路径, 保证价目不因为一次格式漂移就失效
pub fn parse_litellm_str(text: &str) -> PriceTable {
    match serde_json::from_str::<std::collections::HashMap<String, LitellmEntry>>(text) {
        Ok(entries) => {
            let mut table = PriceTable {
                fetched_at: Some(chrono::Utc::now()),
                ..Default::default()
            };
            for (model, e) in entries {
                let Some(input) = e.input.map(|v| v * 1_000_000.0) else {
                    continue;
                };
                table.models.insert(
                    model.clone(),
                    ModelPrice {
                        model,
                        input_per_m: input,
                        output_per_m: e.output.map(|v| v * 1_000_000.0).unwrap_or(input),
                        cache_read_per_m: e
                            .cache_read
                            .map(|v| v * 1_000_000.0)
                            .unwrap_or_default(),
                        cache_write_per_m: e
                            .cache_write
                            .map(|v| v * 1_000_000.0)
                            .unwrap_or_default(),
                        source: PriceSource::Litellm,
                    },
                );
            }
            table
        }
        Err(e) => {
            tracing::warn!("litellm typed parse failed ({e}), falling back to Value");
            match serde_json::from_str::<serde_json::Value>(text) {
                Ok(json) => parse_litellm_json(&json),
                Err(_) => PriceTable::default(),
            }
        }
    }
}

pub fn parse_openrouter_str(text: &str) -> PriceTable {
    match serde_json::from_str::<OpenRouterRoot>(text) {
        Ok(root) => {
            let mut table = PriceTable {
                fetched_at: Some(chrono::Utc::now()),
                ..Default::default()
            };
            for m in root.data {
                let per_m = |v: &Option<String>| {
                    v.as_deref()
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0)
                        * 1_000_000.0
                };
                table.models.insert(
                    m.id.clone(),
                    ModelPrice {
                        model: m.id,
                        input_per_m: per_m(&m.pricing.prompt),
                        output_per_m: per_m(&m.pricing.completion),
                        cache_read_per_m: 0.0,
                        cache_write_per_m: 0.0,
                        source: PriceSource::OpenRouter,
                    },
                );
            }
            table
        }
        Err(e) => {
            tracing::warn!("openrouter typed parse failed ({e}), falling back to Value");
            match serde_json::from_str::<serde_json::Value>(text) {
                Ok(json) => parse_openrouter_json(&json),
                Err(_) => PriceTable::default(),
            }
        }
    }
}

/// Parse LiteLLM model_prices_and_context_window.json format
pub fn parse_litellm_json(json: &serde_json::Value) -> PriceTable {
    let mut table = PriceTable {
        fetched_at: Some(chrono::Utc::now()),
        ..Default::default()
    };
    let Some(obj) = json.as_object() else {
        return table;
    };
    for (model, info) in obj {
        let Some(input) = info
            .get("input_cost_per_token")
            .and_then(|v| v.as_f64())
            .map(|v| v * 1_000_000.0)
        else {
            continue;
        };
        let output = info
            .get("output_cost_per_token")
            .and_then(|v| v.as_f64())
            .map(|v| v * 1_000_000.0)
            .unwrap_or(input);
        let cache_read = info
            .get("cache_read_input_token_cost")
            .and_then(|v| v.as_f64())
            .map(|v| v * 1_000_000.0)
            .unwrap_or_default();
        let cache_write = info
            .get("cache_creation_input_token_cost")
            .and_then(|v| v.as_f64())
            .map(|v| v * 1_000_000.0)
            .unwrap_or_default();
        table.models.insert(
            model.clone(),
            ModelPrice {
                model: model.clone(),
                input_per_m: input,
                output_per_m: output,
                cache_read_per_m: cache_read,
                cache_write_per_m: cache_write,
                source: PriceSource::Litellm,
            },
        );
    }
    table
}

/// Parse OpenRouter /api/v1/models format
pub fn parse_openrouter_json(json: &serde_json::Value) -> PriceTable {
    let mut table = PriceTable {
        fetched_at: Some(chrono::Utc::now()),
        ..Default::default()
    };
    let Some(data) = json.get("data").and_then(|v| v.as_array()) else {
        return table;
    };
    for item in data {
        let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(pricing) = item.get("pricing") else {
            continue;
        };
        let prompt = pricing
            .get("prompt")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let completion = pricing
            .get("completion")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        // openrouter prices are per-token
        table.models.insert(
            id.to_string(),
            ModelPrice {
                model: id.to_string(),
                input_per_m: prompt * 1_000_000.0,
                output_per_m: completion * 1_000_000.0,
                cache_read_per_m: 0.0,
                cache_write_per_m: 0.0,
                source: PriceSource::OpenRouter,
            },
        );
    }
    table
}
