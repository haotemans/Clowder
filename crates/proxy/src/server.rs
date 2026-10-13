use anyhow::Context;
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::Response,
    Router,
};
use bytes::Bytes;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, RwLock};
use yts_core::{Config, PriceTable, Protocol, Provider, Status as UsageStatus, Store, UsageRecord};

use crate::parser::{
    parse_anthropic_event, parse_openai_event, parse_openai_json, ParserKind, SseEvent, SseParser,
    UsageAccumulator,
};
use crate::upstream::{client_for, upstream_url};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub store: Store,
    pub usage_tx: broadcast::Sender<UsageRecord>,
    pub providers: Arc<RwLock<Vec<Provider>>>, // live cache to avoid locking config every req
    pub price_table: Arc<RwLock<PriceTable>>,
}

impl AppState {
    pub async fn refresh_providers(&self) {
        let cfg = self.config.read().await;
        let mut prov = self.providers.write().await;
        *prov = cfg.providers.clone();
    }

    pub async fn active_provider(&self) -> Option<Provider> {
        let cfg = self.config.read().await;
        cfg.active_provider().cloned()
    }
}

pub async fn run_proxy(listen: &str, config: Config, store: Store) -> anyhow::Result<()> {
    let (usage_tx, _) = broadcast::channel(1024);
    let state = AppState {
        config: Arc::new(RwLock::new(config)),
        store,
        usage_tx,
        providers: Arc::new(RwLock::new(Vec::new())),
        price_table: Arc::new(RwLock::new(yts_core::snapshot_table())),
    };
    state.refresh_providers().await;
    run_with_state(listen, state).await
}

pub async fn run_with_state(listen: &str, state: AppState) -> anyhow::Result<()> {
    let app = Router::new().fallback(proxy_handler).with_state(state);

    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .with_context(|| format!("bind {}", listen))?;
    tracing::info!("yts-proxy listening on http://{}", listen);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn proxy_handler(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, (StatusCode, String)> {
    let path = uri.path();
    let query = uri.query();

    let (provider, upstream_path) = resolve_provider(&state, path).await?;

    let url = upstream_url(&provider, &upstream_path, query);
    let client = client_for(&provider).map_err(internal)?;

    let mut req_headers = HeaderMap::new();
    for (k, v) in headers.iter() {
        let name = k.as_str();
        // strip hop-by-hop and auth headers we will replace
        if matches!(
            name,
            "host"
                | "connection"
                | "content-length"
                | "transfer-encoding"
                | "authorization"
                | "x-api-key"
                | "x-goog-api-key"
                | "api-key"
        ) {
            continue;
        }
        req_headers.insert(k.clone(), v.clone());
    }

    // inject provider auth
    if let Ok(key) = crate::dpapi::decrypt(&provider.api_key_encrypted) {
        match provider.protocol {
            Protocol::Anthropic => {
                req_headers.insert("x-api-key", HeaderValue::from_str(&key).map_err(internal)?);
                req_headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
            }
            Protocol::OpenAI => {
                req_headers.insert(
                    "authorization",
                    HeaderValue::from_str(&format!("Bearer {}", key)).map_err(internal)?,
                );
            }
        }
    }

    let is_sse = path.contains("/complete")
        || path.contains("/messages")
        || path.contains("/chat/completions")
        || path.contains("/responses");

    // Bytes -> reqwest::Body 是零拷贝引用计数; to_vec() 会把整个 prompt 再复制一份
    let upstream_req = client
        .request(method.clone(), &url)
        .headers(req_headers)
        .body(body);

    let start = Instant::now();
    let resp = upstream_req.send().await.map_err(internal)?;
    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let content_type = resp_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let is_stream = content_type.contains("text/event-stream") || is_sse;

    let session_id = extract_session_id(&headers);

    if !status.is_success() {
        let body_bytes = resp.bytes().await.map_err(internal)?;
        let mut r = UsageRecord::new(
            provider.id.clone(),
            provider.name.clone(),
            provider.protocol,
            "unknown".to_string(),
        );
        r.session_id = session_id;
        r.status = UsageStatus::Error;
        let _ = state.store.insert_usage(&r);
        return response_from_bytes(status, &resp_headers, body_bytes);
    }

    if is_stream {
        handle_stream(state, provider, resp, resp_headers, start, session_id).await
    } else {
        handle_json(state, provider, resp, resp_headers, start, session_id).await
    }
}

async fn resolve_provider(
    state: &AppState,
    path: &str,
) -> Result<(Provider, String), (StatusCode, String)> {
    let providers = state.providers.read().await;
    let cfg = state.config.read().await;

    // /p/{provider_id}/rest...
    if let Some(rest) = path.strip_prefix("/p/") {
        if let Some((pid, rest_path)) = rest.split_once('/') {
            if let Some(p) = providers.iter().find(|p| p.id == pid) {
                return Ok((p.clone(), rest_path.to_string()));
            }
            return Err((StatusCode::NOT_FOUND, format!("provider {} not found", pid)));
        }
    }

    // default route by protocol
    let (proto, rest) = if let Some(rest) = path.strip_prefix("/anthropic/") {
        (Protocol::Anthropic, rest)
    } else if let Some(rest) = path.strip_prefix("/openai/") {
        (Protocol::OpenAI, rest)
    } else if let Some(rest) = path.strip_prefix("/openrouter/openai/") {
        if let Some(p) = providers.iter().find(|p| p.id == "openrouter") {
            return Ok((p.clone(), rest.to_string()));
        }
        (Protocol::OpenAI, rest)
    } else {
        return Err((StatusCode::NOT_FOUND, "unknown route prefix".to_string()));
    };

    let provider = providers
        .iter()
        .find(|p| p.protocol == proto)
        .or_else(|| cfg.active_provider())
        .cloned()
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "no active provider".to_string(),
            )
        })?;
    Ok((provider, rest.to_string()))
}

async fn handle_stream(
    state: AppState,
    provider: Provider,
    resp: reqwest::Response,
    resp_headers: HeaderMap,
    start: Instant,
    session_id: Option<String>,
) -> Result<Response, (StatusCode, String)> {
    let kind = match provider.protocol {
        Protocol::Anthropic => ParserKind::Anthropic,
        Protocol::OpenAI => ParserKind::OpenAi,
    };
    let mut parser = SseParser::new(kind);
    let mut acc = UsageAccumulator::new();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(32);
    let state2 = state.clone();
    let provider2 = provider.clone();
    let provider_id = provider.id.clone();
    let provider_name = provider.name.clone();

    tokio::spawn(async move {
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    let events = parser.feed(&bytes);
                    for (ev, raw) in events {
                        match &ev {
                            SseEvent::Data(d) => {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(d) {
                                    let gen_ms = start.elapsed().as_millis() as u64;
                                    match kind {
                                        ParserKind::Anthropic => {
                                            parse_anthropic_event(&mut acc, &json, gen_ms)
                                        }
                                        ParserKind::OpenAi => {
                                            parse_openai_event(&mut acc, &json, gen_ms)
                                        }
                                    }
                                }
                            }
                            SseEvent::Done => {}
                        }
                        if tx.send(Ok(Bytes::from(raw))).await.is_err() {
                            return;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
                    return;
                }
            }
        }
        // flush trailing
        for (ev, raw) in parser.flush() {
            if let SseEvent::Data(d) = &ev {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(d) {
                    let gen_ms = start.elapsed().as_millis() as u64;
                    match kind {
                        ParserKind::Anthropic => parse_anthropic_event(&mut acc, &json, gen_ms),
                        ParserKind::OpenAi => parse_openai_event(&mut acc, &json, gen_ms),
                    }
                }
            }
            let _ = tx.send(Ok(Bytes::from(raw))).await;
        }
        acc.ensure_usage();
        let mut rec = UsageRecord::new(
            provider_id,
            provider_name,
            provider2.protocol,
            acc.model.clone().unwrap_or_else(|| "unknown".to_string()),
        );
        rec.session_id = session_id;
        rec.input_tokens = acc.input_tokens;
        rec.output_tokens = acc.output_tokens;
        rec.cache_read_tokens = acc.cache_read_tokens;
        rec.cache_write_tokens = acc.cache_write_tokens;
        rec.ttft_ms = acc.ttft_ms;
        rec.duration_ms = start.elapsed().as_millis() as u64;
        rec.tps = acc.tps();
        rec.estimated = !acc.saw_usage;
        if let Ok(table) = state2.price_table.try_read() {
            rec.cost_usd = table.cost_usd(
                &rec.model,
                rec.input_tokens,
                rec.output_tokens,
                rec.cache_read_tokens,
                rec.cache_write_tokens,
            );
        }
        state2.store.insert_usage(&rec).ok();
        let _ = state2.usage_tx.send(rec);
    });

    let body = Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(rx));
    let mut builder = Response::builder().status(StatusCode::OK);
    for (k, v) in resp_headers.iter() {
        if matches!(
            k.as_str(),
            "content-length" | "transfer-encoding" | "connection" | "content-encoding"
        ) {
            continue;
        }
        builder = builder.header(k, v);
    }
    builder
        .body(body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_json(
    state: AppState,
    provider: Provider,
    resp: reqwest::Response,
    resp_headers: HeaderMap,
    start: Instant,
    session_id: Option<String>,
) -> Result<Response, (StatusCode, String)> {
    let status = resp.status();
    let body_bytes = resp.bytes().await.map_err(internal)?;
    let mut acc = UsageAccumulator::new();
    parse_openai_json(&mut acc, &body_bytes);
    let mut rec = UsageRecord::new(
        provider.id.clone(),
        provider.name.clone(),
        provider.protocol,
        acc.model.clone().unwrap_or_else(|| "unknown".to_string()),
    );
    rec.session_id = session_id;
    rec.input_tokens = acc.input_tokens;
    rec.output_tokens = acc.output_tokens;
    rec.cache_read_tokens = acc.cache_read_tokens;
    rec.cache_write_tokens = acc.cache_write_tokens;
    rec.duration_ms = start.elapsed().as_millis() as u64;
    rec.estimated = !acc.saw_usage;
    if let Ok(table) = state.price_table.try_read() {
        rec.cost_usd = table.cost_usd(
            &rec.model,
            rec.input_tokens,
            rec.output_tokens,
            rec.cache_read_tokens,
            rec.cache_write_tokens,
        );
    }
    state.store.insert_usage(&rec).ok();
    let _ = state.usage_tx.send(rec);
    response_from_bytes(status, &resp_headers, body_bytes)
}

fn response_from_bytes(
    status: StatusCode,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<Response, (StatusCode, String)> {
    let mut builder = Response::builder().status(status);
    for (k, v) in headers.iter() {
        if matches!(
            k.as_str(),
            "content-length" | "transfer-encoding" | "connection"
        ) {
            continue;
        }
        builder = builder.header(k, v);
    }
    builder
        .body(Body::from(body))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn extract_session_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            headers
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
}

fn internal(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
