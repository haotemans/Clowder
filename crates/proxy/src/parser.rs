use serde_json::Value;

#[derive(Debug, Default)]
pub struct UsageAccumulator {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub model: Option<String>,
    pub ttft_ms: Option<u64>,
    /// Elapsed ms between first token and stream end
    pub gen_ms: u64,
    /// Did we actually see a usage payload?
    pub saw_usage: bool,
    /// 估算兜底: 只累计字符数, 不缓存正文 (长回答不再整份留在内存里)
    pub est_chars: u64,
    pub est_cjk: u64,
}

impl UsageAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tps(&self) -> Option<f64> {
        if self.output_tokens == 0 || self.gen_ms == 0 {
            return None;
        }
        Some(self.output_tokens as f64 * 1000.0 / self.gen_ms as f64)
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
        ((self.est_cjk as f64 / 1.8) + (ascii as f64 / 3.8)) as u64
    }

    pub fn ensure_usage(&mut self) {
        if !self.saw_usage && self.output_tokens == 0 {
            self.output_tokens = self.estimated_output_tokens();
        }
        if self.gen_ms == 0 {
            if let Some(ttft) = self.ttft_ms {
                self.gen_ms = ttft;
            }
        }
    }
}

pub struct SseParser {
    buffer: Vec<u8>,
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
            kind,
        }
    }

    pub fn kind(&self) -> ParserKind {
        self.kind
    }

    /// Feed raw bytes; split complete SSE events. Returns each event plus its raw bytes
    /// so the server can forward them byte-identical to the client.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<(SseEvent, Vec<u8>)> {
        let mut events = Vec::new();
        self.buffer.extend_from_slice(chunk);
        while let Some((pos, sep)) = find_event_end(&self.buffer) {
            let block: Vec<u8> = self.buffer.drain(..pos + sep).collect();
            let block_str = String::from_utf8_lossy(&block);
            let mut data_payload: Option<String> = None;
            for line in block_str.lines() {
                if let Some(data) = line.strip_prefix("data:") {
                    let d = data.trim_start();
                    data_payload = Some(d.to_string());
                }
            }
            if let Some(d) = data_payload {
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

/// SSE 事件分隔符允许 \n\n 和 \r\n\r\n; 只认前者的话, CRLF 上游会让 buffer 一直涨到流结束.
/// 返回 (分隔符起点, 分隔符长度)
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
                        .unwrap_or(0);
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
                if let Some(model) = msg.get("model").and_then(|v| v.as_str()) {
                    acc.model = Some(model.to_string());
                }
            }
        }
        "content_block_delta" => {
            if acc.ttft_ms.is_none() {
                acc.ttft_ms = Some(gen_so_far_ms);
            }
            acc.gen_ms = gen_so_far_ms;
            if let Some(delta) = json.get("delta") {
                if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                    acc.push_text(text);
                }
            }
        }
        "message_delta" => {
            acc.gen_ms = gen_so_far_ms;
            if let Some(usage) = json.get("usage") {
                if let Some(out) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                    acc.output_tokens = out;
                    acc.saw_usage = true;
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
        if let Some(m) = json.get("model").and_then(|v| v.as_str()) {
            acc.model = Some(m.to_string());
        }
    }
    if let Some(choices) = json.get("choices").and_then(|v| v.as_array()) {
        if !choices.is_empty() {
            if acc.ttft_ms.is_none() {
                acc.ttft_ms = Some(gen_so_far_ms);
            }
            acc.gen_ms = gen_so_far_ms;
            if let Some(delta) = choices[0].get("delta") {
                if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                    acc.push_text(content);
                }
            }
        }
    }
    if let Some(usage) = json.get("usage") {
        acc.input_tokens = usage
            .get("prompt_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(acc.input_tokens);
        acc.output_tokens = usage
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(acc.output_tokens);
        acc.saw_usage = true;
        if let Some(details) = usage
            .get("prompt_cache_hit_tokens")
            .and_then(|v| v.as_u64())
        {
            acc.cache_read_tokens = details;
        }
        acc.gen_ms = gen_so_far_ms;
    }
}

pub fn parse_openai_json(acc: &mut UsageAccumulator, body: &[u8]) {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return;
    };
    if let Some(m) = v.get("model").and_then(|v| v.as_str()) {
        acc.model = Some(m.to_string());
    }
    if let Some(usage) = v.get("usage") {
        acc.input_tokens = usage
            .get("prompt_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        acc.output_tokens = usage
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        acc.saw_usage = true;
        if let Some(hit) = usage
            .get("prompt_cache_hit_tokens")
            .and_then(|v| v.as_u64())
        {
            acc.cache_read_tokens = hit;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_split() {
        let mut p = SseParser::new(ParserKind::Anthropic);
        let ev = p.feed(b"event: message_start\ndata: {\"a\":1}\n\ndata: {\"b\":2}\n\n");
        assert_eq!(ev.len(), 2);
        assert!(p.buffer.is_empty());
    }

    #[test]
    fn test_sse_split_crlf() {
        let mut p = SseParser::new(ParserKind::OpenAi);
        let ev = p.feed(b"data: {\"a\":1}\r\n\r\ndata: [DONE]\r\n\r\n");
        assert_eq!(ev.len(), 2);
        assert!(matches!(ev[1].0, SseEvent::Done));
        assert!(p.buffer.is_empty());
        // 原始字节要原样转发给客户端
        assert_eq!(ev[0].1, b"data: {\"a\":1}\r\n\r\n".to_vec());
    }

    #[test]
    fn test_estimation_counts_chars_not_text() {
        let mut acc = UsageAccumulator::new();
        acc.push_text("你好 world");
        assert_eq!(acc.est_chars, 8);
        assert_eq!(acc.est_cjk, 2);
        acc.ensure_usage();
        // 2/1.8 + 6/3.8 = 2.63 -> 2
        assert_eq!(acc.output_tokens, 2);
    }
}
