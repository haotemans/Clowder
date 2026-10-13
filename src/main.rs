// release 版双击启动时不带控制台; debug 版留着, cargo run 能直接看日志
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod charts;
mod logger;
mod mini;
mod mini_theme;
mod pages;
mod theme;

use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use clowder_core::{db_path, migrate_legacy_data, Config, Store};
use clowder_proxy::AppState;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let headless = args.iter().any(|a| a == "-s" || a == "--server");
    // 默认开小窗; -f / --full 直接开主界面
    let start_full = args.iter().any(|a| a == "-f" || a == "--full");

    // 改名 (yuantuanswitch → Clowder) 后的一次性数据迁移, 必须在 logger::init 之前:
    // 日志文件一落到新 data 目录, 迁移就会把非空目录判定成"已经迁过"而跳过
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

    // start proxy in background
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

    // pricing refresh background task
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

            // 缓存还在有效期内就不拉, 省掉两次大 JSON 的内存峰值和启动时的流量
            let fresh = table
                .fetched_at
                .map(|t| chrono::Utc::now().signed_duration_since(t).num_seconds() < hours as i64 * 3600)
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
                // write().await: try_write 在 GUI 持有读锁时会静默失败, 价目几小时都不更新
                *pricing_state.price_table.write().await = table;
            }

            tokio::time::sleep(std::time::Duration::from_secs(hours.max(1) * 3600)).await;
        }
    });

    let app = app::ClowderApp::new(state, usage_rx, store, start_full);

    // 默认开小窗; -f 直接开主界面, 窗口参数要和 app 的初始 mode 一致, 否则会先弹一个 300px 的框
    let mut builder = egui::ViewportBuilder::default().with_icon(load_icon());
    builder = if start_full {
        builder
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 600.0])
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
    // simple generated 32x32 icon: blue circle
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
                rgba[idx] = 59; // R
                rgba[idx + 1] = 130; // G
                rgba[idx + 2] = 246; // B
                rgba[idx + 3] = 255; // A
            }
        }
    }
    egui::IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    }
}
