use serde_json::Value;

#[derive(Debug, Default)]
pub struct UsageAccumulator {
    /// 未命中缓存的净输入 Token (已扣除 cached_tokens, 与 Anthropic 口径严格对齐, 避免重复计费)
    pub input_tokens: u64,
    /// 总输出 Token (包含 reasoning_tokens)
    pub output_tokens: u64,
    /// 缓存命中读取 Token
    pub cache_read_tokens: u64,
    /// 缓存创建写入 Token
    pub cache_write_tokens: u64,
    /// 推理思考 Token (作为 output_tokens 的子集单独展示明细)
    pub reasoning_tokens: u64,
    pub model: Option<String>,
    pub ttft_ms: Option<u64>,
    pub last_token_ms: Option<u64>,
    /// Elapsed ms between request start and stream end
    pub gen_ms: u64,
    /// Did we actually see a usage payload from upstream?
    pub saw_usage: bool,
    /// 请求侧预估输入 token (仅在内存中计数字符数，不保存任何 Prompt 文本)
    pub est_input_tokens: u64,
    /// 响应侧估算兜底: 只累计字符数，不缓存任何正文内容
    pub est_chars: u64,
    pub est_cjk: u64,
}

impl UsageAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_request_estimate(body: &[u8]) -> Self {
        let mut acc = Self::default();
        if let Ok(v) = serde_json::from_slice::<Value>(body) {
            if let Some(m) = v.get("model").and_then(|m| m.as_str()) {
                acc.model = Some(m.to_string());
            }
        }
        acc.est_input_tokens = estimate_request_input_tokens(body);
        acc
    }

    /// 纯解码阶段 TPS: 扣除首字延迟 (TTFT), 避免长上下文/推理思考把解码吞吐率拉低
    pub fn tps(&self) -> Option<f64> {
        if self.output_tokens == 0 {
            return None;
        }
        if let (Some(first), Some(last)) = (self.ttft_ms, self.last_token_ms) {
            let decode_ms = last.saturating_sub(first);
            if decode_ms >= 15 {
                return Some(self.output_tokens as f64 * 1000.0 / decode_ms as f64);
            }
        }
        let effective_ms = match self.ttft_ms {
            Some(ttft) if self.gen_ms > ttft + 10 => self.gen_ms - ttft,
            _ => self.gen_ms,
        };
        if effective_ms == 0 {
            return None;
        }
        Some(self.output_tokens as f64 * 1000.0 / effective_ms as f64)
    }

    pub fn push_text(&mut self, text: &str) {
        for c in text.chars() {
            self.est_chars += 1;
            let cp = c as u32;
            if (0x4E00..=0x9FFF).contains(&cp) || (0x3040..=0x30FF).contains(&cp) {
                self.est_cjk += 1;
            }
        }
    }

    pub fn estimated_output_tokens(&self) -> u64 {
        if self.est_chars == 0 {
            return 0;
        }
        let ascii = self.est_chars.saturating_sub(self.est_cjk);
        ((self.est_cjk as f64 / 1.6) + (ascii as f64 / 3.8)).ceil() as u64
    }

    pub fn ensure_usage(&mut self) {
        if !self.saw_usage {
            if self.output_tokens == 0 {
                self.output_tokens = self.estimated_output_tokens();
            }
            if self.input_tokens == 0 && self.est_input_tokens > 0 {
                self.input_tokens = self.est_input_tokens;
            }
        }
        if self.gen_ms == 0 {
            if let Some(ttft) = self.ttft_ms {
                self.gen_ms = ttft;
            }
        }
    }
}

pub fn estimate_request_input_tokens(body: &[u8]) -> u64 {
    if body.is_empty() {
        return 0;
    }
    let s = String::from_utf8_lossy(body);
    let mut chars = 0u64;
    let mut cjk = 0u64;
    for c in s.chars() {
        chars += 1;
        let cp = c as u32;
        if (0x4E00..=0x9FFF).contains(&cp) || (0x3040..=0x30FF).contains(&cp) {
            cjk += 1;
        }
    }
    let ascii = chars.saturating_sub(cjk);
    (((cjk as f64 / 1.6) + (ascii as f64 / 4.2)) * 0.82) as u64
}

pub struct SseParser {
    buffer: Vec<u8>,
    scan_cursor: usize,
    kind: ParserKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserKind {
    Anthropic,
    OpenAi,
}

pub enum SseEvent {
    Data(String),
    Done,
}

impl SseParser {
    pub fn new(kind: ParserKind) -> Self {
        Self {
            buffer: Vec::new(),
            scan_cursor: 0,
            kind,
        }
    }

    pub fn kind(&self) -> ParserKind {
        self.kind
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<(SseEvent, Vec<u8>)> {
        let mut events = Vec::new();
        self.buffer.extend_from_slice(chunk);
        if self.buffer.len() > 2 * 1024 * 1024 {
            self.buffer.clear();
            self.scan_cursor = 0;
            return events;
        }
        loop {
            let start = self.scan_cursor.saturating_sub(3);
            let Some((rel_pos, sep)) = find_event_end(&self.buffer[start..]) else {
                self.scan_cursor = self.buffer.len();
                break;
            };
            let pos = start + rel_pos;
            let block: Vec<u8> = self.buffer.drain(..pos + sep).collect();
            self.scan_cursor = 0;

            let block_str = String::from_utf8_lossy(&block);
            let mut data_lines: Vec<&str> = Vec::new();
            for line in block_str.lines() {
                if let Some(data) = line.strip_prefix("data:") {
                    data_lines.push(data.trim_start());
                }
            }
            if !data_lines.is_empty() {
                let d = data_lines.join("\n");
                if d == "[DONE]" {
                    events.push((SseEvent::Done, block));
                } else {
                    events.push((SseEvent::Data(d), block));
                }
            }
        }
        events
    }

    pub fn flush(&mut self) -> Vec<(SseEvent, Vec<u8>)> {
        let mut events = Vec::new();
        if !self.buffer.is_empty() {
            let block = std::mem::take(&mut self.buffer);
            self.scan_cursor = 0;
            let block_str = String::from_utf8_lossy(&block);
            for line in block_str.lines() {
                if let Some(data) = line.strip_prefix("data:") {
                    let d = data.trim_start().to_string();
                    if d == "[DONE]" {
                        events.push((SseEvent::Done, block.clone()));
                    } else {
                        events.push((SseEvent::Data(d), block.clone()));
                    }
                }
            }
        }
        events
    }
}

fn find_event_end(buf: &[u8]) -> Option<(usize, usize)> {
    match (
        buf.windows(2).position(|w| w == b"\n\n"),
        buf.windows(4).position(|w| w == b"\r\n\r\n"),
    ) {
        (Some(a), Some(b)) => Some(if b < a { (b, 4) } else { (a, 2) }),
        (Some(a), None) => Some((a, 2)),
        (None, Some(b)) => Some((b, 4)),
        (None, None) => None,
    }
}

pub fn parse_anthropic_event(acc: &mut UsageAccumulator, json: &Value, gen_so_far_ms: u64) {
    let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match event_type {
        "message_start" => {
            if let Some(msg) = json.get("message") {
                if let Some(usage) = msg.get("usage") {
                    acc.input_tokens = usage
                        .get("input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(acc.input_tokens);
                    acc.cache_read_tokens = usage
                        .get("cache_read_input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(acc.cache_read_tokens);
                    acc.cache_write_tokens = usage
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(acc.cache_write_tokens);
                    if let Some(out) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                        if out > 0 {
                            acc.output_tokens = out;
                        }
                    }
                    acc.saw_usage = true;
                }
                if let Some(model) = msg.get("model").and_then(|v| v.as_str()) {
                    acc.model = Some(model.to_string());
                }
            }
        }
        "content_block_delta" => {
            if acc.ttft_ms.is_none() {
                acc.ttft_ms = Some(gen_so_far_ms);
            }
            acc.last_token_ms = Some(gen_so_far_ms);
            acc.gen_ms = gen_so_far_ms;
            if let Some(delta) = json.get("delta") {
                if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                    acc.push_text(text);
                } else if let Some(thinking) = delta.get("thinking").and_then(|v| v.as_str()) {
                    acc.push_text(thinking);
                } else if let Some(partial_json) = delta.get("partial_json").and_then(|v| v.as_str())
                {
                    acc.push_text(partial_json);
                }
            }
        }
        "message_delta" => {
            acc.last_token_ms = Some(gen_so_far_ms);
            acc.gen_ms = gen_so_far_ms;
            if let Some(usage) = json.get("usage") {
                if let Some(out) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                    acc.output_tokens = out;
                    acc.saw_usage = true;
                }
                if let Some(inp) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                    acc.input_tokens = inp;
                }
                if let Some(cr) = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()) {
                    acc.cache_read_tokens = cr;
                }
                if let Some(cw) = usage
                    .get("cache_creation_input_tokens")
                    .and_then(|v| v.as_u64())
                {
                    acc.cache_write_tokens = cw;
                }
            }
        }
        "message_stop" => {
            acc.gen_ms = gen_so_far_ms;
        }
        _ => {}
    }
}

pub fn parse_openai_event(acc: &mut UsageAccumulator, json: &Value, gen_so_far_ms: u64) {
    if acc.model.is_none() {
        if let Some(m) = json
            .get("model")
            .and_then(|v| v.as_str())
            .or_else(|| json.pointer("/response/model").and_then(|v| v.as_str()))
        {
            acc.model = Some(m.to_string());
        }
    }

    if let Some(ev_type) = json.get("type").and_then(|v| v.as_str()) {
        match ev_type {
            "response.output_text.delta" | "response.reasoning_summary_text.delta" => {
                if acc.ttft_ms.is_none() {
                    acc.ttft_ms = Some(gen_so_far_ms);
                }
                acc.last_token_ms = Some(gen_so_far_ms);
                acc.gen_ms = gen_so_far_ms;
                if let Some(delta) = json.get("delta").and_then(|v| v.as_str()) {
                    acc.push_text(delta);
                }
            }
            "response.completed" | "response.done" => {
                acc.gen_ms = gen_so_far_ms;
                if let Some(resp) = json.get("response") {
                    apply_openai_usage_object(acc, resp.get("usage"));
                }
            }
            _ => {}
        }
    }

    if let Some(choices) = json.get("choices").and_then(|v| v.as_array()) {
        if !choices.is_empty() {
            if let Some(delta) = choices[0].get("delta") {
                let has_token = delta.get("content").and_then(|v| v.as_str()).is_some()
                    || delta.get("reasoning_content").and_then(|v| v.as_str()).is_some()
                    || delta.get("tool_calls").is_some();
                if has_token {
                    if acc.ttft_ms.is_none() {
                        acc.ttft_ms = Some(gen_so_far_ms);
                    }
                    acc.last_token_ms = Some(gen_so_far_ms);
                    acc.gen_ms = gen_so_far_ms;
                }
                if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                    acc.push_text(content);
                }
                if let Some(rc) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                    acc.push_text(rc);
                }
            }
        }
    }

    if json.get("usage").is_some() {
        apply_openai_usage_object(acc, json.get("usage"));
        acc.gen_ms = gen_so_far_ms;
    }
}

/// 关键精度修复:
/// OpenAI (`/v1/chat/completions`, `/v1/responses`) 与 DeepSeek 的 `prompt_tokens` / `input_tokens`
/// 包含 `cached_tokens` (`prompt_cache_hit_tokens`) 在内!
/// 若不从 `prompt_tokens` 中扣除 `cached_tokens`，会导致缓存命中部分被按「全价输入 + 缓存读取价」双重计费、输入 Token 虚高。
fn apply_openai_usage_object(acc: &mut UsageAccumulator, usage_opt: Option<&Value>) {
    let Some(usage) = usage_opt else {
        return;
    };
    if usage.is_null() {
        return;
    }

    let cached = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .or_else(|| usage.get("prompt_cache_hit_tokens"))
        .or_else(|| usage.get("cache_read_input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if cached > 0 {
        acc.cache_read_tokens = cached;
    }

    if let Some(cache_write) = usage
        .get("cache_creation_input_tokens")
        .or_else(|| usage.pointer("/prompt_tokens_details/cache_write_tokens"))
        .and_then(|v| v.as_u64())
    {
        acc.cache_write_tokens = cache_write;
    }

    if let Some(raw_prompt) = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|v| v.as_u64())
    {
        // 若上游给了 prompt_cache_miss_tokens 则直接用，否则用 raw_prompt - cached 得到净未缓存输入 Token
        let uncached_input = usage
            .get("prompt_cache_miss_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or_else(|| raw_prompt.saturating_sub(acc.cache_read_tokens));
        acc.input_tokens = uncached_input;
        acc.saw_usage = true;
    }

    if let Some(out) = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|v| v.as_u64())
    {
        acc.output_tokens = out;
        acc.saw_usage = true;
    }

    if let Some(reasoning) = usage
        .pointer("/completion_tokens_details/reasoning_tokens")
        .or_else(|| usage.pointer("/output_tokens_details/reasoning_tokens"))
        .and_then(|v| v.as_u64())
    {
        acc.reasoning_tokens = reasoning;
    }
}

pub fn parse_anthropic_json(acc: &mut UsageAccumulator, body: &[u8]) {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return;
    };
    if let Some(m) = v.get("model").and_then(|v| v.as_str()) {
        acc.model = Some(m.to_string());
    }
    if let Some(usage) = v.get("usage") {
        acc.input_tokens = usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(acc.input_tokens);
        acc.output_tokens = usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(acc.output_tokens);
        acc.cache_read_tokens = usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        acc.cache_write_tokens = usage
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        acc.saw_usage = true;
    }
    if !acc.saw_usage {
        if let Some(content) = v.get("content").and_then(|c| c.as_array()) {
            for block in content {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    acc.push_text(t);
                }
            }
        }
        acc.ensure_usage();
    }
}

pub fn parse_openai_json(acc: &mut UsageAccumulator, body: &[u8]) {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return;
    };
    if let Some(m) = v.get("model").and_then(|v| v.as_str()) {
        acc.model = Some(m.to_string());
    }
    apply_openai_usage_object(acc, v.get("usage"));
    if !acc.saw_usage {
        if let Some(choices) = v.get("choices").and_then(|c| c.as_array()) {
            for ch in choices {
                if let Some(content) = ch.pointer("/message/content").and_then(|t| t.as_str()) {
                    acc.push_text(content);
                }
            }
        }
        acc.ensure_usage();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_responses_api_deducts_cached_from_input_tokens() {
        let mut acc = UsageAccumulator::new();
        let ev: Value = serde_json::from_str(
            r#"{
                "type": "response.completed",
                "response": {
                    "model": "gpt-5.4",
                    "usage": {
                        "input_tokens": 1200,
                        "output_tokens": 350,
                        "input_tokens_details": { "cached_tokens": 800 },
                        "output_tokens_details": { "reasoning_tokens": 120 }
                    }
                }
            }"#,
        )
        .unwrap();
        parse_openai_event(&mut acc, &ev, 500);
        assert_eq!(acc.model.as_deref(), Some("gpt-5.4"));
        // 1200 总输入 - 800 缓存命中 = 400 净未缓存输入
        assert_eq!(acc.input_tokens, 400);
        assert_eq!(acc.cache_read_tokens, 800);
        assert_eq!(acc.output_tokens, 350);
        assert_eq!(acc.reasoning_tokens, 120);
        assert!(acc.saw_usage);
    }
}
