use anyhow::Context;
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, RwLock};
use clowder_core::{
    read_claude_code_oauth_token, read_codex_oauth_token, AgentId, AgentRouteMode, AuthKind,
    Config, PriceTable, Protocol, Provider, Status as UsageStatus, Store, UpstreamStrategy,
    UsageRecord,
};

use crate::parser::{
    parse_anthropic_event, parse_anthropic_json, parse_openai_event, parse_openai_json, ParserKind,
    SseEvent, SseParser, UsageAccumulator,
};
use crate::upstream::{client_for, upstream_url};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub store: Store,
    pub usage_tx: broadcast::Sender<UsageRecord>,
    pub providers: Arc<RwLock<Vec<Provider>>>,
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
        price_table: Arc::new(RwLock::new(clowder_core::snapshot_table())),
    };
    state.refresh_providers().await;
    run_with_state(listen, state).await
}

pub async fn run_with_state(listen: &str, state: AppState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/_clowder/status", get(admin_status))
        .route("/_clowder/provider", post(admin_upsert_provider))
        .route("/_clowder/global-provider", post(admin_set_global_provider))
        .route("/_clowder/agent", post(admin_configure_agent))
        .route("/_clowder/sync", post(admin_sync_agents))
        .fallback(proxy_handler)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .with_context(|| format!("bind {}", listen))?;
    tracing::info!("clowder-proxy listening on http://{}", listen);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn admin_status(State(state): State<AppState>) -> Json<Value> {
    let cfg = state.config.read().await;
    let claude_oauth_ready = read_claude_code_oauth_token().is_some();
    let codex_oauth_ready = read_codex_oauth_token().is_some();
    let providers: Vec<Value> = cfg
        .providers
        .iter()
        .map(|p| {
            let auth_ready = match p.auth_kind {
                AuthKind::ApiKey => !p.api_key_encrypted.is_empty(),
                AuthKind::ClaudeCodeOauth => claude_oauth_ready || !p.api_key_encrypted.is_empty(),
                AuthKind::CodexOauth => codex_oauth_ready || !p.api_key_encrypted.is_empty(),
            };
            json!({
                "id": p.id,
                "name": p.name,
                "protocol": p.protocol.as_str(),
                "auth_kind": p.auth_kind,
                "base_url": p.base_url,
                "upstream_strategy": p.upstream_strategy.to_string(),
                "auth_ready": auth_ready,
            })
        })
        .collect();
    let agents: Vec<Value> = cfg
        .agents
        .iter()
        .map(|a| {
            let effective = cfg
                .resolve_provider_for_agent(Some(a.agent), Some(a.agent.preferred_protocol()))
                .map(|p| p.id.clone());
            json!({
                "agent": a.agent.as_str(),
                "label": a.agent.label(),
                "badge": a.agent.badge(),
                "enabled_in_switcher": a.enabled_in_switcher,
                "route_mode": a.route_mode,
                "dedicated_provider_id": a.provider_id,
                "effective_provider_id": effective,
                "default_model": a.default_model,
            })
        })
        .collect();
    Json(json!({
        "ok": true,
        "privacy": "local-only-zero-upload",
        "passthrough": "native-protocol-byte-identical",
        "claude_oauth_detected": claude_oauth_ready,
        "codex_oauth_detected": codex_oauth_ready,
        "listen": cfg.proxy.listen,
        "global_provider_id": cfg.active_provider_id,
        "current_agent": cfg.current_agent.map(|a| a.as_str()),
        "session_consent_granted": cfg.ui.session_consent_granted,
        "providers": providers,
        "agents": agents,
    }))
}

async fn admin_upsert_provider(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let auth_kind = match payload.get("auth_kind").and_then(|v| v.as_str()) {
        Some("claude_code_oauth") | Some("claude-oauth") => AuthKind::ClaudeCodeOauth,
        Some("codex_oauth") | Some("codex-oauth") => AuthKind::CodexOauth,
        _ => AuthKind::ApiKey,
    };

    let mut template = match auth_kind {
        AuthKind::ClaudeCodeOauth => Provider::claude_oauth(),
        AuthKind::CodexOauth => Provider::codex_oauth(),
        AuthKind::ApiKey => {
            let proto = Protocol::from_db(
                payload
                    .get("protocol")
                    .and_then(|v| v.as_str())
                    .unwrap_or("openai"),
            );
            Provider::new_template(proto)
        }
    };

    if let Some(id) = payload.get("id").and_then(|v| v.as_str()) {
        if !id.trim().is_empty() {
            template.id = id.trim().to_string();
        }
    }
    if let Some(name) = payload.get("name").and_then(|v| v.as_str()) {
        if !name.trim().is_empty() {
            template.name = name.trim().to_string();
        }
    }
    if let Some(base_url) = payload.get("base_url").and_then(|v| v.as_str()) {
        if !base_url.trim().is_empty() {
            template.base_url = base_url.trim().to_string();
        }
    }
    if let Some(strat) = payload.get("upstream_strategy").and_then(|v| v.as_str()) {
        template.upstream_strategy = match strat {
            "direct" => UpstreamStrategy::Direct,
            "custom" => UpstreamStrategy::Custom,
            _ => UpstreamStrategy::SystemProxy,
        };
    }
    template.custom_proxy_url = payload
        .get("custom_proxy_url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let api_key_plain = payload
        .get("api_key")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut cfg = state.config.write().await;
    let encrypted = if !api_key_plain.is_empty() {
        crate::dpapi::encrypt(api_key_plain).map_err(internal)?
    } else {
        cfg.provider_by_id(&template.id)
            .map(|p| p.api_key_encrypted.clone())
            .unwrap_or_default()
    };
    template.api_key_encrypted = encrypted;

    let id = template.id.clone();
    if let Some(existing) = cfg.providers.iter_mut().find(|p| p.id == id) {
        *existing = template;
    } else {
        cfg.providers.push(template);
    }

    if payload
        .get("set_global")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || cfg.active_provider_id.is_none()
    {
        cfg.active_provider_id = Some(id.clone());
    }
    cfg.save().map_err(internal)?;
    let providers_clone = cfg.providers.clone();
    drop(cfg);
    *state.providers.write().await = providers_clone;

    Ok(Json(json!({ "ok": true, "provider_id": id })))
}

async fn admin_set_global_provider(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let provider_id = payload
        .get("provider_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "missing provider_id".into()))?
        .to_string();
    let sync_all = payload
        .get("sync_all_agents")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let mut cfg = state.config.write().await;
    if cfg.provider_by_id(&provider_id).is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("provider '{}' not found", provider_id),
        ));
    }
    cfg.active_provider_id = Some(provider_id.clone());
    cfg.save().map_err(internal)?;
    let synced = if sync_all {
        cfg.sync_all_enabled_agents()
    } else {
        Vec::new()
    };
    Ok(Json(json!({
        "ok": true,
        "global_provider_id": provider_id,
        "synced_agents": synced,
    })))
}

async fn admin_configure_agent(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let agent_str = payload
        .get("agent")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "missing agent slug".into()))?;
    let agent = AgentId::from_slug(agent_str)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, format!("unknown agent: {}", agent_str)))?;

    if payload
        .get("restore_original")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let cfg = state.config.read().await;
        let msg = cfg.restore_agent_local_config(agent).map_err(internal)?;
        return Ok(Json(json!({
            "ok": true,
            "agent": agent.as_str(),
            "restored": true,
            "message": msg,
        })));
    }

    let mut cfg = state.config.write().await;
    {
        let prof = cfg.agent_profile_mut(agent);
        if let Some(mode) = payload.get("route_mode").and_then(|v| v.as_str()) {
            prof.route_mode = match mode {
                "dedicated" | "override" | "custom" => AgentRouteMode::Dedicated,
                _ => AgentRouteMode::FollowGlobal,
            };
        }
        if let Some(pid) = payload.get("provider_id").and_then(|v| v.as_str()) {
            if pid.is_empty() || pid == "global" {
                prof.route_mode = AgentRouteMode::FollowGlobal;
                prof.provider_id = None;
            } else {
                prof.route_mode = AgentRouteMode::Dedicated;
                prof.provider_id = Some(pid.to_string());
            }
        }
        if let Some(model) = payload.get("default_model").and_then(|v| v.as_str()) {
            prof.default_model = if model.trim().is_empty() {
                None
            } else {
                Some(model.trim().to_string())
            };
        }
        if let Some(enabled) = payload.get("enabled_in_switcher").and_then(|v| v.as_bool()) {
            prof.enabled_in_switcher = enabled;
        }
    }
    if payload
        .get("switch_current")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        cfg.current_agent = Some(agent);
    }
    cfg.save().map_err(internal)?;

    let write_cfg = payload
        .get("write_local_config")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let write_msg = if write_cfg {
        Some(cfg.write_agent_local_config(agent).map_err(internal)?)
    } else {
        None
    };

    Ok(Json(json!({
        "ok": true,
        "agent": agent.as_str(),
        "message": write_msg,
    })))
}

async fn admin_sync_agents(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let cfg = state.config.read().await;
    let results = cfg.sync_all_enabled_agents();
    Ok(Json(json!({
        "ok": true,
        "results": results,
    })))
}

fn resolve_provider_credential(provider: &Provider) -> Option<String> {
    match provider.auth_kind {
        AuthKind::ClaudeCodeOauth => read_claude_code_oauth_token().or_else(|| {
            crate::dpapi::decrypt(&provider.api_key_encrypted)
                .ok()
                .filter(|k| !k.trim().is_empty())
        }),
        AuthKind::CodexOauth => read_codex_oauth_token().or_else(|| {
            crate::dpapi::decrypt(&provider.api_key_encrypted)
                .ok()
                .filter(|k| !k.trim().is_empty())
        }),
        AuthKind::ApiKey => crate::dpapi::decrypt(&provider.api_key_encrypted)
            .ok()
            .filter(|k| !k.trim().is_empty()),
    }
}

/// 各家用各家原生协议: 100% 原始字节零延迟透传，不做跨协议改写
async fn proxy_handler(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, (StatusCode, String)> {
    let path = uri.path();
    let query = uri.query();

    let (provider, route_agent, upstream_path) = resolve_provider(&state, path, &headers).await?;

    let url = upstream_url(&provider, &upstream_path, query);
    let client = client_for(&provider).map_err(internal)?;

    let resolved_cred = resolve_provider_credential(&provider);

    let mut req_headers = HeaderMap::new();
    for (k, v) in headers.iter() {
        let name = k.as_str();
        if matches!(
            name,
            "host" | "connection" | "content-length" | "transfer-encoding"
        ) || name.starts_with("x-clowder-")
        {
            continue;
        }
        if resolved_cred.is_some()
            && matches!(
                name,
                "authorization" | "x-api-key" | "x-goog-api-key" | "api-key"
            )
        {
            continue;
        }
        req_headers.insert(k.clone(), v.clone());
    }

    if let Some(cred) = resolved_cred {
        match (provider.protocol, provider.auth_kind) {
            (Protocol::Anthropic, AuthKind::ClaudeCodeOauth) => {
                req_headers.insert(
                    "authorization",
                    HeaderValue::from_str(&format!("Bearer {}", cred)).map_err(internal)?,
                );
                if !req_headers.contains_key("anthropic-version") {
                    req_headers
                        .insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
                }
            }
            (Protocol::Anthropic, _) => {
                req_headers.insert("x-api-key", HeaderValue::from_str(&cred).map_err(internal)?);
                if !req_headers.contains_key("anthropic-version") {
                    req_headers
                        .insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
                }
            }
            (Protocol::OpenAI, _) => {
                req_headers.insert(
                    "authorization",
                    HeaderValue::from_str(&format!("Bearer {}", cred)).map_err(internal)?,
                );
            }
        }
    }

    for (hk, hv) in &provider.headers {
        if let (Ok(name), Ok(val)) = (
            axum::http::header::HeaderName::from_bytes(hk.as_bytes()),
            HeaderValue::from_str(hv),
        ) {
            req_headers.insert(name, val);
        }
    }

    let req_acc = UsageAccumulator::with_request_estimate(&body);

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
    let is_stream = content_type.contains("text/event-stream");

    let session_id = extract_anonymized_session_id(&headers, route_agent.as_deref());

    if !status.is_success() {
        let body_bytes = resp.bytes().await.map_err(internal)?;
        let mut r = UsageRecord::new(
            provider.id.clone(),
            provider.name.clone(),
            provider.protocol,
            req_acc.model.unwrap_or_else(|| "unknown".to_string()),
        );
        r.session_id = session_id;
        r.agent_id = route_agent;
        r.status = UsageStatus::Error;
        let _ = state.store.insert_usage(&r);
        return response_from_bytes(status, &resp_headers, body_bytes);
    }

    if is_stream {
        handle_stream(
            state,
            provider,
            resp,
            resp_headers,
            start,
            session_id,
            route_agent,
            req_acc,
        )
        .await
    } else {
        handle_json(
            state,
            provider,
            resp,
            resp_headers,
            start,
            session_id,
            route_agent,
            req_acc,
        )
        .await
    }
}

async fn resolve_provider(
    state: &AppState,
    path: &str,
    headers: &HeaderMap,
) -> Result<(Provider, Option<String>, String), (StatusCode, String)> {
    let providers = state.providers.read().await;
    let cfg = state.config.read().await;

    if let Some(rest) = path.strip_prefix("/a/") {
        if let Some((agent_slug, after_agent)) = rest.split_once('/') {
            let agent = AgentId::from_slug(agent_slug);
            let agent_tag = Some(
                agent
                    .map(|a| a.as_str().to_string())
                    .unwrap_or_else(|| agent_slug.to_string()),
            );
            let (proto_hint, upstream_path) =
                if let Some(p) = after_agent.strip_prefix("anthropic/") {
                    (Some(Protocol::Anthropic), p.to_string())
                } else if after_agent == "anthropic" {
                    (Some(Protocol::Anthropic), String::new())
                } else if let Some(p) = after_agent.strip_prefix("openai/") {
                    (Some(Protocol::OpenAI), p.to_string())
                } else if after_agent == "openai" {
                    (Some(Protocol::OpenAI), String::new())
                } else {
                    (agent.map(|a| a.preferred_protocol()), after_agent.to_string())
                };

            let provider = cfg
                .resolve_provider_for_agent(agent, proto_hint)
                .cloned()
                .ok_or_else(|| {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("no provider configured for agent {}", agent_slug),
                    )
                })?;
            return Ok((provider, agent_tag, upstream_path));
        }
    }

    let inferred_agent = infer_agent_from_headers(headers);

    if let Some(rest) = path.strip_prefix("/p/") {
        if let Some((pid, rest_path)) = rest.split_once('/') {
            if let Some(p) = providers.iter().find(|p| p.id == pid) {
                return Ok((p.clone(), inferred_agent, rest_path.to_string()));
            }
            return Err((StatusCode::NOT_FOUND, format!("provider {} not found", pid)));
        }
    }

    let (proto, rest) = if let Some(rest) = path.strip_prefix("/anthropic/") {
        (Protocol::Anthropic, rest)
    } else if let Some(rest) = path.strip_prefix("/openai/") {
        (Protocol::OpenAI, rest)
    } else if let Some(rest) = path.strip_prefix("/openrouter/openai/") {
        if let Some(p) = providers.iter().find(|p| p.id == "openrouter") {
            return Ok((p.clone(), inferred_agent, rest.to_string()));
        }
        (Protocol::OpenAI, rest)
    } else {
        return Err((StatusCode::NOT_FOUND, "unknown route prefix".to_string()));
    };

    let agent_enum = inferred_agent.as_deref().and_then(AgentId::from_slug);
    let provider = cfg
        .resolve_provider_for_agent(agent_enum, Some(proto))
        .or_else(|| providers.iter().find(|p| p.protocol == proto))
        .cloned()
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "no active provider".to_string(),
            )
        })?;
    Ok((provider, inferred_agent, rest.to_string()))
}

#[allow(clippy::too_many_arguments)]
async fn handle_stream(
    state: AppState,
    provider: Provider,
    resp: reqwest::Response,
    resp_headers: HeaderMap,
    start: Instant,
    session_id: Option<String>,
    agent_id: Option<String>,
    mut acc: UsageAccumulator,
) -> Result<Response, (StatusCode, String)> {
    let kind = match provider.protocol {
        Protocol::Anthropic => ParserKind::Anthropic,
        Protocol::OpenAI => ParserKind::OpenAi,
    };
    let mut parser = SseParser::new(kind);
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(64);
    let state2 = state.clone();
    let provider2 = provider.clone();
    let provider_id = provider.id.clone();
    let provider_name = provider.name.clone();

    tokio::spawn(async move {
        let mut stream = resp.bytes_stream();
        let mut stream_status = UsageStatus::Ok;

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    // 100% 原始字节零延迟转发给客户端，随后在旁路做统计
                    if tx.send(Ok(bytes.clone())).await.is_err() {
                        stream_status = UsageStatus::Aborted;
                        break;
                    }

                    let events = parser.feed(&bytes);
                    for (ev, _) in events {
                        if let SseEvent::Data(d) = &ev {
                            if !d.is_empty() {
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
                        }
                    }
                }
                Err(e) => {
                    stream_status = UsageStatus::Error;
                    let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
                    break;
                }
            }
        }
        for (ev, _) in parser.flush() {
            if let SseEvent::Data(d) = &ev {
                if !d.is_empty() {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(d) {
                        let gen_ms = start.elapsed().as_millis() as u64;
                        match kind {
                            ParserKind::Anthropic => parse_anthropic_event(&mut acc, &json, gen_ms),
                            ParserKind::OpenAi => parse_openai_event(&mut acc, &json, gen_ms),
                        }
                    }
                }
            }
        }
        acc.ensure_usage();
        let mut rec = UsageRecord::new(
            provider_id,
            provider_name,
            provider2.protocol,
            acc.model.clone().unwrap_or_else(|| "unknown".to_string()),
        );
        rec.session_id = session_id;
        rec.agent_id = agent_id;
        rec.input_tokens = acc.input_tokens;
        rec.output_tokens = acc.output_tokens;
        rec.cache_read_tokens = acc.cache_read_tokens;
        rec.cache_write_tokens = acc.cache_write_tokens;
        rec.reasoning_tokens = acc.reasoning_tokens;
        rec.ttft_ms = acc.ttft_ms;
        rec.duration_ms = start.elapsed().as_millis() as u64;
        rec.tps = acc.tps();
        rec.estimated = !acc.saw_usage;
        rec.status = stream_status;
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

#[allow(clippy::too_many_arguments)]
async fn handle_json(
    state: AppState,
    provider: Provider,
    resp: reqwest::Response,
    resp_headers: HeaderMap,
    start: Instant,
    session_id: Option<String>,
    agent_id: Option<String>,
    mut acc: UsageAccumulator,
) -> Result<Response, (StatusCode, String)> {
    let status = resp.status();
    let body_bytes = resp.bytes().await.map_err(internal)?;
    match provider.protocol {
        Protocol::Anthropic => parse_anthropic_json(&mut acc, &body_bytes),
        Protocol::OpenAI => parse_openai_json(&mut acc, &body_bytes),
    }
    let duration_ms = start.elapsed().as_millis() as u64;
    acc.gen_ms = duration_ms;
    let mut rec = UsageRecord::new(
        provider.id.clone(),
        provider.name.clone(),
        provider.protocol,
        acc.model.clone().unwrap_or_else(|| "unknown".to_string()),
    );
    rec.session_id = session_id;
    rec.agent_id = agent_id;
    rec.input_tokens = acc.input_tokens;
    rec.output_tokens = acc.output_tokens;
    rec.cache_read_tokens = acc.cache_read_tokens;
    rec.cache_write_tokens = acc.cache_write_tokens;
    rec.reasoning_tokens = acc.reasoning_tokens;
    rec.duration_ms = duration_ms;
    rec.tps = acc.tps();
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

fn infer_agent_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(h) = headers.get("x-clowder-agent").and_then(|v| v.to_str().ok()) {
        if let Some(a) = AgentId::from_slug(h) {
            return Some(a.as_str().to_string());
        }
        return Some(h.to_string());
    }
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ua.contains("claude-code") || ua.contains("claude-cli") {
        Some(AgentId::ClaudeCode.as_str().to_string())
    } else if ua.contains("claude-desktop") || ua.contains("claude/") {
        Some(AgentId::ClaudeDesktop.as_str().to_string())
    } else if ua.contains("codex-desktop") {
        Some(AgentId::CodexDesktop.as_str().to_string())
    } else if ua.contains("codex") {
        Some(AgentId::Codex.as_str().to_string())
    } else if ua.contains("opencode") {
        Some(AgentId::OpenCodeV2.as_str().to_string())
    } else if ua.contains("pi-coding-agent") || ua.starts_with("pi/") {
        Some(AgentId::Pi.as_str().to_string())
    } else if ua.contains("dsh-desktop") {
        Some(AgentId::DshDesktop.as_str().to_string())
    } else if ua.contains("dsh") || ua.contains("deepseek-harness") {
        Some(AgentId::Dsh.as_str().to_string())
    } else {
        None
    }
}

fn extract_anonymized_session_id(headers: &HeaderMap, agent_id: Option<&str>) -> Option<String> {
    if let Some(raw) = headers
        .get("x-session-id")
        .or_else(|| headers.get("x-claude-session-id"))
        .or_else(|| headers.get("x-codex-session-id"))
        .and_then(|v| v.to_str().ok())
    {
        return Some(anonymize_token(raw));
    }
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok())?;
    let day = chrono::Utc::now().format("%Y%m%d");
    Some(format!(
        "{}-{}",
        agent_id.unwrap_or_else(|| ua.split('/').next().unwrap_or("client")),
        day
    ))
}

fn anonymize_token(s: &str) -> String {
    if s.contains('\\') || s.contains('/') || s.len() > 48 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in s.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        format!("sess-{:016x}", h)
    } else {
        s.to_string()
    }
}

fn internal(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
