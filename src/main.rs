// release 版双击启动时不带控制台; debug 版留着, cargo run 能直接看日志
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod charts;
mod gauge;
mod logger;
mod mini;
mod mini_theme;
mod pages;
mod theme;

use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use clowder_core::{
    db_path, migrate_legacy_data, read_claude_code_oauth_token, read_codex_oauth_token, AgentId,
    AgentRouteMode, AuthKind, Config, Protocol, Provider, Store, UpstreamStrategy,
};
use clowder_proxy::AppState;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.get(1).map(|s| s.as_str()) == Some("cli") {
        return run_cli(&args[2..]);
    }

    let headless = args.iter().any(|a| a == "-s" || a == "--server");
    let start_full = args.iter().any(|a| a == "-f" || a == "--full");

    let migration = migrate_legacy_data();
    logger::init(headless);
    match migration {
        Ok(0) => {}
        Ok(n) => tracing::info!("migrated {n} file(s) from the legacy yuantuanswitch data dir"),
        Err(e) => tracing::warn!("legacy data migration failed, starting with empty config: {e:#}"),
    }
    let result = run(headless, start_full);
    if let Err(e) = &result {
        tracing::error!("fatal: {e:#}");
        if !headless {
            logger::show_fatal(&format!("{e:#}"));
        }
    }
    result
}

fn run_cli(args: &[String]) -> anyhow::Result<()> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("status");
    let mut cfg = Config::load()?;

    match sub {
        "status" => {
            println!("listen: http://{}", cfg.proxy.listen);
            println!("privacy: local-only (0 upload, DPAPI encrypted keys, anonymized sessions)");
            println!("passthrough: same-protocol byte-identical / cross-protocol auto-bridge");
            println!(
                "local_oauth_detected: claude_code={} codex={}",
                read_claude_code_oauth_token().is_some(),
                read_codex_oauth_token().is_some()
            );
            println!(
                "global_provider_id: {}",
                cfg.active_provider_id.as_deref().unwrap_or("(none)")
            );
            println!("providers ({}):", cfg.providers.len());
            for p in &cfg.providers {
                println!(
                    "  - id={} name=\"{}\" protocol={} auth={} base_url={} upstream={}",
                    p.id,
                    p.name,
                    p.protocol.as_str(),
                    p.auth_kind,
                    p.base_url,
                    p.upstream_strategy
                );
            }
            println!("agents ({}):", cfg.agents.len());
            for a in &cfg.agents {
                let eff = cfg
                    .resolve_provider_for_agent(Some(a.agent), Some(a.agent.preferred_protocol()))
                    .map(|p| p.id.as_str())
                    .unwrap_or("(none)");
                println!(
                    "  - {} ({}) mode={} effective_provider={} model={} visible={}",
                    a.agent.as_str(),
                    a.agent.label(),
                    a.route_mode,
                    eff,
                    a.default_model.as_deref().unwrap_or("(default)"),
                    a.enabled_in_switcher
                );
            }
        }
        "add-oauth" => {
            // clowder cli add-oauth <claude|codex> [--set-global]
            let kind = args
                .get(1)
                .map(|s| s.as_str())
                .ok_or_else(|| anyhow::anyhow!("Usage: clowder cli add-oauth <claude|codex> [--set-global]"))?;
            let preset = match kind {
                "claude" | "claude-code" | "claude-oauth" => Provider::claude_oauth(),
                "codex" | "codex-oauth" | "chatgpt" => Provider::codex_oauth(),
                _ => anyhow::bail!("Unknown OAuth kind '{}'. Use 'claude' or 'codex'", kind),
            };
            let pid = preset.id.clone();
            if let Some(existing) = cfg.providers.iter_mut().find(|p| p.id == pid) {
                *existing = preset;
            } else {
                cfg.providers.push(preset);
            }
            if args.iter().any(|a| a == "--set-global") || cfg.active_provider_id.is_none() {
                cfg.active_provider_id = Some(pid.clone());
            }
            cfg.save()?;
            println!("ok: OAuth membership provider '{}' registered", pid);
        }
        "add-provider" => {
            if args.len() < 6 {
                anyhow::bail!(
                    "Usage: clowder cli add-provider <id> <name> <anthropic|openai> <base_url> <api_key> [direct|system_proxy|custom] [custom_proxy_url]"
                );
            }
            let id = args[1].clone();
            let name = args[2].clone();
            let protocol = Protocol::from_db(&args[3]);
            let base_url = args[4].clone();
            let api_key_encrypted = if args[5].is_empty() {
                String::new()
            } else {
                clowder_proxy::dpapi::encrypt(&args[5])?
            };
            let upstream_strategy = match args.get(6).map(|s| s.as_str()) {
                Some("direct") => UpstreamStrategy::Direct,
                Some("custom") => UpstreamStrategy::Custom,
                _ => UpstreamStrategy::SystemProxy,
            };
            let custom_proxy_url = args.get(7).cloned();

            if let Some(existing) = cfg.providers.iter_mut().find(|p| p.id == id) {
                existing.name = name;
                existing.protocol = protocol;
                existing.base_url = base_url;
                if !api_key_encrypted.is_empty() {
                    existing.api_key_encrypted = api_key_encrypted;
                }
                existing.upstream_strategy = upstream_strategy;
                existing.custom_proxy_url = custom_proxy_url;
            } else {
                cfg.providers.push(Provider {
                    id: id.clone(),
                    name,
                    protocol,
                    base_url,
                    api_key_encrypted,
                    auth_kind: AuthKind::ApiKey,
                    emulate_claude_code: false,
                    upstream_strategy,
                    custom_proxy_url,
                    headers: Vec::new(),
                });
            }
            if cfg.active_provider_id.is_none() {
                cfg.active_provider_id = Some(id.clone());
            }
            cfg.save()?;
            println!("ok: provider '{}' saved locally (DPAPI encrypted)", id);
        }
        "set-global" => {
            let pid = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("Usage: clowder cli set-global <provider_id> [--sync]"))?;
            if cfg.provider_by_id(pid).is_none() {
                anyhow::bail!("provider '{}' not found", pid);
            }
            cfg.active_provider_id = Some(pid.clone());
            cfg.save()?;
            println!("ok: global provider set to '{}'", pid);
            if args.iter().any(|a| a == "--sync") {
                for msg in cfg.sync_all_enabled_agents() {
                    println!("  {}", msg);
                }
            }
        }
        "set-agent" => {
            if args.len() < 3 {
                anyhow::bail!(
                    "Usage: clowder cli set-agent <agent_slug> <global|provider_id> [default_model]"
                );
            }
            let agent = AgentId::from_slug(&args[1])
                .ok_or_else(|| anyhow::anyhow!("unknown agent '{}'", args[1]))?;
            let target = args[2].as_str();
            let model = args.get(3).cloned();
            {
                let prof = cfg.agent_profile_mut(agent);
                if target.eq_ignore_ascii_case("global") || target == "follow_global" {
                    prof.route_mode = AgentRouteMode::FollowGlobal;
                    prof.provider_id = None;
                } else {
                    prof.route_mode = AgentRouteMode::Dedicated;
                    prof.provider_id = Some(target.to_string());
                }
                if let Some(m) = model {
                    prof.default_model = if m.is_empty() { None } else { Some(m) };
                }
            }
            cfg.save()?;
            let msg = cfg.write_agent_local_config(agent)?;
            println!("ok: {}", msg);
        }
        "restore-agent" => {
            let slug = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("Usage: clowder cli restore-agent <agent_slug>"))?;
            let agent = AgentId::from_slug(slug)
                .ok_or_else(|| anyhow::anyhow!("unknown agent '{}'", slug))?;
            let msg = cfg.restore_agent_local_config(agent)?;
            println!("ok: {}", msg);
        }
        "sync-all" => {
            for msg in cfg.sync_all_enabled_agents() {
                println!("{}", msg);
            }
        }
        _ => {
            anyhow::bail!(
                "Unknown cli subcommand '{}'. Available: status, add-oauth, add-provider, set-global, set-agent, restore-agent, sync-all",
                sub
            );
        }
    }
    Ok(())
}

fn run(headless: bool, start_full: bool) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let _guard = rt.enter();
    let config = Config::load()?;
    let db = db_path()?;
    if let Some(p) = db.parent() {
        std::fs::create_dir_all(p)?;
    }
    let store = Store::open(&db)?;

    if headless {
        let listen = config.proxy.listen.clone();
        return rt.block_on(async move { clowder_proxy::run_proxy(&listen, config, store).await });
    }

    let (usage_tx, usage_rx) = broadcast::channel(1024);
    let state = AppState {
        config: Arc::new(RwLock::new(config.clone())),
        store: store.clone(),
        usage_tx: usage_tx.clone(),
        providers: Arc::new(RwLock::new(config.providers.clone())),
        price_table: Arc::new(RwLock::new(clowder_core::resolve_table(&[]))),
    };

    let proxy_state = state.clone();
    rt.spawn(async move {
        let listen = {
            let cfg = proxy_state.config.read().await;
            cfg.proxy.listen.clone()
        };
        if let Err(e) = clowder_proxy::run_with_state(&listen, proxy_state).await {
            tracing::error!("proxy error: {}", e);
        }
    });

    let pricing_state = state.clone();
    rt.spawn(async move {
        loop {
            let (litellm_on, or_on, hours) = {
                let cfg = pricing_state.config.read().await;
                (
                    cfg.pricing.litellm_enabled,
                    cfg.pricing.openrouter_enabled,
                    cfg.pricing.refresh_hours,
                )
            };
            let mut table = clowder_core::resolve_table(&[]);
            let mut updated = false;

            let fresh = table
                .fetched_at
                .map(|t| {
                    chrono::Utc::now().signed_duration_since(t).num_seconds() < hours as i64 * 3600
                })
                .unwrap_or(false);
            if !fresh {
                if litellm_on {
                    match reqwest::get("https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json")
                        .await
                    {
                        Ok(resp) => match resp.text().await {
                            Ok(text) => {
                                let t = clowder_core::parse_litellm_str(&text);
                                updated |= !t.models.is_empty();
                                table.merge(t);
                            }
                            Err(e) => tracing::warn!("litellm pricing read failed: {}", e),
                        },
                        Err(e) => tracing::warn!("litellm pricing fetch failed: {}", e),
                    }
                }
                if or_on {
                    match reqwest::get("https://openrouter.ai/api/v1/models").await {
                        Ok(resp) => match resp.text().await {
                            Ok(text) => {
                                let t = clowder_core::parse_openrouter_str(&text);
                                updated |= !t.models.is_empty();
                                table.merge(t);
                            }
                            Err(e) => tracing::warn!("openrouter pricing read failed: {}", e),
                        },
                        Err(e) => tracing::warn!("openrouter pricing fetch failed: {}", e),
                    }
                }
            }

            if updated {
                table.fetched_at = Some(chrono::Utc::now());
                let _ = clowder_core::save_cache(&table);
                *pricing_state.price_table.write().await = table;
            }

            tokio::time::sleep(std::time::Duration::from_secs(hours.max(1) * 3600)).await;
        }
    });

    let app = app::ClowderApp::new(state, usage_rx, store, start_full);

    let mut builder = egui::ViewportBuilder::default().with_icon(load_icon());
    builder = if start_full {
        builder
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([960.0, 620.0])
            .with_max_inner_size([2400.0, 1600.0])
    } else {
        builder
            .with_inner_size([300.0, 340.0])
            .with_min_inner_size([280.0, 320.0])
            .with_max_inner_size([360.0, 420.0])
            .with_decorations(false)
            .with_always_on_top()
    };
    let native_options = eframe::NativeOptions {
        viewport: builder,
        ..Default::default()
    };

    eframe::run_native(
        "Clowder",
        native_options,
        Box::new(|cc| Ok(Box::new(app::ClowderApp::init(cc, app)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))
}

fn load_icon() -> egui::IconData {
    let size = 32usize;
    let mut rgba = vec![0u8; size * size * 4];
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            if d < 14.0 {
                let idx = (y * size + x) * 4;
                rgba[idx] = 59;
                rgba[idx + 1] = 130;
                rgba[idx + 2] = 246;
                rgba[idx + 3] = 255;
            }
        }
    }
    egui::IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    }
}
