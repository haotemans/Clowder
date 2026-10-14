use crate::app::ClowderApp;
use crate::theme;
use egui::{FontFamily, FontId, RichText, Ui};
use clowder_core::{
    read_claude_code_oauth_token, read_codex_oauth_token, AgentId, AgentRouteMode, AuthKind,
    Protocol, Provider, UpstreamStrategy,
};

#[derive(Default)]
pub struct EditState {
    pub open: bool,
    pub provider_id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub auth_kind: AuthKind,
    pub upstream_strategy: UpstreamStrategy,
    pub custom_proxy_url: String,
}

pub fn show(ui: &mut Ui, app: &mut ClowderApp) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        // =========================================================
        // 1. 总控中心 (Global Control Center)
        // =========================================================
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::h2("总控中心 · 全局默认供应商"));
            ui.add_space(12.0);
            ui.label(theme::meta(
                "各家用各家原生协议 · 100% 字节级零延迟透传 · 写入前自动备份 (.clowder.bak)",
            ));
        });
        ui.add_space(12.0);

        let providers_snapshot = app
            .state
            .providers
            .try_read()
            .ok()
            .map(|p| p.clone())
            .unwrap_or_default();

        let (mut global_id, current_agent, mut agents_snapshot) = {
            if let Ok(cfg) = app.state.config.try_read() {
                (
                    cfg.active_provider_id.clone(),
                    cfg.current_agent,
                    cfg.agents.clone(),
                )
            } else {
                (None, None, Vec::new())
            }
        };

        let mut global_changed = false;
        let mut trigger_sync_all = false;

        egui::Frame::NONE
            .fill(theme::CARD)
            .stroke(egui::Stroke::new(1.0, theme::HAIRLINE))
            .inner_margin(egui::Margin::symmetric(20, 14))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(theme::mono("总控默认供应商:"));
                    ui.add_space(8.0);
                    let selected_label = global_id
                        .as_ref()
                        .and_then(|id| providers_snapshot.iter().find(|p| &p.id == id))
                        .map(|p| format!("{} ({:?})", p.name, p.protocol))
                        .unwrap_or_else(|| "未选择".to_string());

                    egui::ComboBox::from_id_salt("global_default_provider")
                        .selected_text(selected_label)
                        .width(230.0)
                        .show_ui(ui, |ui| {
                            for p in &providers_snapshot {
                                let label =
                                    format!("{} ({:?} · {})", p.name, p.protocol, p.auth_kind);
                                if ui
                                    .selectable_value(&mut global_id, Some(p.id.clone()), label)
                                    .changed()
                                {
                                    global_changed = true;
                                }
                            }
                        });

                    ui.add_space(12.0);
                    if ui
                        .button(
                            RichText::new("一键配置所有启用 Agent")
                                .family(FontFamily::Monospace)
                                .size(12.0),
                        )
                        .on_hover_text(
                            "将本地代理路由写入所有启用 Agent 的配置文件 (自动备份 .clowder.bak)",
                        )
                        .clicked()
                    {
                        trigger_sync_all = true;
                    }

                    ui.add_space(8.0);
                    if ui
                        .button(
                            RichText::new("展开右上角 1/4 圆切换器")
                                .family(FontFamily::Monospace)
                                .size(12.0),
                        )
                        .clicked()
                    {
                        app.corner_menu_open = !app.corner_menu_open;
                    }
                });
            });

        if global_changed {
            if let Ok(mut cfg) = app.state.config.try_write() {
                cfg.active_provider_id = global_id.clone();
                let _ = cfg.save();
            }
        }
        if trigger_sync_all {
            if let Ok(cfg) = app.state.config.try_read() {
                let msgs = cfg.sync_all_enabled_agents();
                app.status_message = Some(format!("已同步 {} 个 Agent 配置", msgs.len()));
            }
        }

        ui.add_space(24.0);

        // =========================================================
        // 2. Agent 启动与配置面板 (双层响应式卡片，窄屏不挤压)
        // =========================================================
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::h2("Agent 启动与独立配置"));
            ui.add_space(12.0);
            if let Some(cur) = current_agent {
                ui.label(theme::mono_accent(format!(
                    "[当前右上角选中: {} ({})]",
                    cur.label(),
                    cur.badge()
                )));
                ui.add_space(8.0);
                if ui
                    .link(
                        RichText::new("切回总控全部显示")
                            .family(FontFamily::Monospace)
                            .size(11.0),
                    )
                    .clicked()
                {
                    if let Ok(mut cfg) = app.state.config.try_write() {
                        cfg.current_agent = None;
                        let _ = cfg.save();
                    }
                }
            } else {
                ui.label(theme::meta(
                    "可点击右上角 1/4 圆快速切换单个 Agent，或在下方直接为任意 Agent 单独指定供应商",
                ));
            }
        });
        ui.add_space(12.0);

        let mut agents_modified = false;
        let mut write_single_agent: Option<AgentId> = None;
        let mut restore_single_agent: Option<AgentId> = None;
        let mut launch_single_agent: Option<(AgentId, String)> = None;

        for prof in agents_snapshot.iter_mut().filter(|a| a.enabled_in_switcher) {
            let is_focused = current_agent == Some(prof.agent);
            let border_col = if is_focused {
                theme::ACCENT
            } else {
                theme::HAIRLINE
            };
            egui::Frame::NONE
                .fill(theme::CARD)
                .stroke(egui::Stroke::new(if is_focused { 1.5 } else { 1.0 }, border_col))
                .inner_margin(egui::Margin::symmetric(18, 12))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let (badge_rect, _) =
                                ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::hover());
                            ui.painter().rect_filled(
                                badge_rect,
                                0.0,
                                if is_focused {
                                    theme::ACCENT
                                } else {
                                    theme::PANEL
                                },
                            );
                            ui.painter().text(
                                badge_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                prof.agent.badge(),
                                FontId::monospace(11.5),
                                if is_focused {
                                    theme::INVERT
                                } else {
                                    theme::TEXT
                                },
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(prof.agent.label())
                                    .family(FontFamily::Monospace)
                                    .size(14.0)
                                    .color(theme::TEXT)
                                    .strong(),
                            );
                            ui.add_space(10.0);

                            let mut mode = prof.route_mode;
                            egui::ComboBox::from_id_salt(("agent_mode", prof.agent.as_str()))
                                .selected_text(mode.to_string())
                                .width(92.0)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_value(
                                            &mut mode,
                                            AgentRouteMode::FollowGlobal,
                                            "跟随总控",
                                        )
                                        .changed()
                                    {
                                        prof.route_mode = mode;
                                        agents_modified = true;
                                    }
                                    if ui
                                        .selectable_value(
                                            &mut mode,
                                            AgentRouteMode::Dedicated,
                                            "独立配置",
                                        )
                                        .changed()
                                    {
                                        prof.route_mode = mode;
                                        if prof.provider_id.is_none() {
                                            prof.provider_id = global_id.clone().or_else(|| {
                                                providers_snapshot.first().map(|p| p.id.clone())
                                            });
                                        }
                                        agents_modified = true;
                                    }
                                });

                            if prof.route_mode == AgentRouteMode::Dedicated {
                                ui.add_space(6.0);
                                let sel_prov_name = prof
                                    .provider_id
                                    .as_ref()
                                    .and_then(|id| {
                                        providers_snapshot.iter().find(|p| &p.id == id)
                                    })
                                    .map(|p| p.name.clone())
                                    .unwrap_or_else(|| "选择供应商".to_string());
                                egui::ComboBox::from_id_salt((
                                    "agent_dedicated_prov",
                                    prof.agent.as_str(),
                                ))
                                .selected_text(sel_prov_name)
                                .width(160.0)
                                .show_ui(ui, |ui| {
                                    for p in &providers_snapshot {
                                        if ui
                                            .selectable_value(
                                                &mut prof.provider_id,
                                                Some(p.id.clone()),
                                                format!("{} ({})", p.name, p.auth_kind),
                                            )
                                            .changed()
                                        {
                                            agents_modified = true;
                                        }
                                    }
                                });
                            }

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .button(
                                            RichText::new("启动")
                                                .family(FontFamily::Monospace)
                                                .size(11.5),
                                        )
                                        .on_hover_text(format!(
                                            "执行启动命令: {}",
                                            prof.effective_launch_cmd()
                                        ))
                                        .clicked()
                                    {
                                        launch_single_agent = Some((
                                            prof.agent,
                                            prof.effective_launch_cmd().to_string(),
                                        ));
                                    }
                                    ui.add_space(6.0);
                                    if ui
                                        .button(
                                            RichText::new("还原原配置")
                                                .family(FontFamily::Monospace)
                                                .size(11.0),
                                        )
                                        .on_hover_text(
                                            "从 .clowder.bak 还原该 Agent 的原始配置，恢复原生直连",
                                        )
                                        .clicked()
                                    {
                                        restore_single_agent = Some(prof.agent);
                                    }
                                    ui.add_space(6.0);
                                    if ui
                                        .button(
                                            RichText::new("写入配置")
                                                .family(FontFamily::Monospace)
                                                .size(11.5),
                                        )
                                        .on_hover_text(
                                            "首次写入自动备份 .clowder.bak，仅增量写入代理地址",
                                        )
                                        .clicked()
                                    {
                                        write_single_agent = Some(prof.agent);
                                    }
                                },
                            );
                        });

                        ui.add_space(6.0);

                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(36.0);
                            ui.label(theme::meta(format!(
                                "代理路由: /a/{}/{}",
                                prof.agent.as_str(),
                                prof.agent.preferred_protocol().as_str()
                            )));
                            ui.add_space(16.0);
                            ui.label(theme::meta("默认模型:"));
                            let mut model_buf = prof.default_model.clone().unwrap_or_default();
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut model_buf)
                                        .hint_text("留空保持 Agent 原生默认模型")
                                        .font(FontId::monospace(11.5))
                                        .desired_width(180.0),
                                )
                                .changed()
                            {
                                prof.default_model = if model_buf.trim().is_empty() {
                                    None
                                } else {
                                    Some(model_buf)
                                };
                                agents_modified = true;
                            }
                            if let Some(cfg_path) = prof.agent.config_file_path() {
                                ui.add_space(12.0);
                                let exists_tag = if cfg_path.exists() {
                                    "配置文件已就绪"
                                } else {
                                    "待首次生成"
                                };
                                ui.label(theme::meta(format!("[{}]", exists_tag)))
                                    .on_hover_text(cfg_path.display().to_string());
                            }
                        });
                    });
                });
            ui.add_space(6.0);
        }

        if agents_modified {
            if let Ok(mut cfg) = app.state.config.try_write() {
                cfg.agents = agents_snapshot;
                let _ = cfg.save();
            }
        }
        if let Some(ag) = write_single_agent {
            if let Ok(cfg) = app.state.config.try_read() {
                match cfg.write_agent_local_config(ag) {
                    Ok(msg) => app.status_message = Some(msg),
                    Err(e) => app.status_message = Some(format!("写入失败: {}", e)),
                }
            }
        }
        if let Some(ag) = restore_single_agent {
            if let Ok(cfg) = app.state.config.try_read() {
                match cfg.restore_agent_local_config(ag) {
                    Ok(msg) => app.status_message = Some(msg),
                    Err(e) => app.status_message = Some(format!("还原失败: {}", e)),
                }
            }
        }
        if let Some((ag, cmd)) = launch_single_agent {
            match spawn_agent_process(&cmd) {
                Ok(_) => {
                    app.status_message = Some(format!("已启动 {} ({})", ag.label(), cmd));
                }
                Err(e) => {
                    app.status_message = Some(format!("启动 {} 失败: {}", ag.label(), e));
                }
            }
        }

        ui.add_space(24.0);

        // =========================================================
        // 3. 供应商池管理 (支持一键测试梯子/直连连通延迟)
        // =========================================================
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::h2("供应商池"));
            ui.add_space(16.0);
            for (label, protocol) in [
                ("+ Anthropic Key", Protocol::Anthropic),
                ("+ OpenAI Key", Protocol::OpenAI),
            ] {
                if ui
                    .button(RichText::new(label).family(FontFamily::Monospace).size(12.0))
                    .clicked()
                {
                    add_provider(app, protocol);
                }
                ui.add_space(6.0);
            }
            if ui
                .button(
                    RichText::new("+ OpenRouter")
                        .family(FontFamily::Monospace)
                        .size(12.0),
                )
                .clicked()
            {
                add_preset_provider(app, Provider::openrouter());
            }
            ui.add_space(6.0);
            if ui
                .button(
                    RichText::new("+ Claude 官方 OAuth")
                        .family(FontFamily::Monospace)
                        .size(12.0),
                )
                .on_hover_text("读取本机 ~/.claude/.credentials.json 的 Claude 官方 OAuth 凭证")
                .clicked()
            {
                add_preset_provider(app, Provider::claude_oauth());
            }
            ui.add_space(6.0);
            if ui
                .button(
                    RichText::new("+ Codex 官方 OAuth")
                        .family(FontFamily::Monospace)
                        .size(12.0),
                )
                .on_hover_text("读取本机 ~/.codex/auth.json 的 Codex 官方 OAuth 凭证")
                .clicked()
            {
                add_preset_provider(app, Provider::codex_oauth());
            }
        });

        ui.add_space(14.0);

        let claude_oauth_ok = read_claude_code_oauth_token().is_some();
        let codex_oauth_ok = read_codex_oauth_token().is_some();
        let pings_snapshot = app
            .provider_pings
            .lock()
            .ok()
            .map(|m| m.clone())
            .unwrap_or_default();

        let mut delete_id: Option<String> = None;
        let mut activate_id: Option<String> = None;
        let mut edit_provider: Option<Provider> = None;
        let mut ping_provider: Option<Provider> = None;

        if providers_snapshot.is_empty() {
            ui.label(theme::meta("还没有供应商，请点击上方按钮添加"));
        }

        for p in providers_snapshot.iter() {
            let is_active = global_id.as_deref() == Some(p.id.as_str());
            let frame = egui::Frame::NONE
                .fill(theme::CARD)
                .stroke(egui::Stroke::new(1.0, theme::HAIRLINE))
                .corner_radius(egui::CornerRadius::ZERO)
                .inner_margin(egui::Margin::same(0));
            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    let strip_color = if is_active {
                        theme::ACCENT
                    } else {
                        theme::FAINT
                    };
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(3.0, 74.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 0.0, strip_color);
                    ui.add_space(20.0);

                    ui.vertical(|ui| {
                        ui.add_space(12.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(theme::h2(&p.name));
                            ui.add_space(10.0);
                            if is_active {
                                ui.label(theme::mono_accent("[总控默认]"));
                            }
                            if let Some(ping_res) = pings_snapshot.get(&p.id) {
                                ui.label(theme::mono_accent(format!("[{}]", ping_res)));
                            }
                            match p.auth_kind {
                                AuthKind::ClaudeCodeOauth => {
                                    let status_txt = if claude_oauth_ok {
                                        "[已读取 ~/.claude/.credentials.json]"
                                    } else {
                                        "[未检测到凭证，请先运行 claude /login]"
                                    };
                                    ui.label(theme::mono_accent(status_txt));
                                }
                                AuthKind::CodexOauth => {
                                    let status_txt = if codex_oauth_ok {
                                        "[已读取 ~/.codex/auth.json]"
                                    } else {
                                        "[未检测到凭证，请先运行 codex login]"
                                    };
                                    ui.label(theme::mono_accent(status_txt));
                                }
                                AuthKind::ApiKey => {}
                            }
                        });
                        ui.add_space(4.0);
                        let line = ui.label(theme::mono_dim(format!(
                            "id: {} · {:?} · 认证: {} · {} · 上行: {}",
                            p.id, p.protocol, p.auth_kind, p.base_url, p.upstream_strategy
                        )));
                        line.on_hover_text(format!("直连路由: /p/{}", p.id));
                        ui.add_space(12.0);
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(20.0);
                        let badge = match p.protocol {
                            Protocol::Anthropic => "A",
                            Protocol::OpenAI => "O",
                        };
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                        ui.painter().rect_stroke(
                            rect,
                            0.0,
                            egui::Stroke::new(1.0, theme::HAIRLINE),
                            egui::StrokeKind::Outside,
                        );
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            badge,
                            FontId::monospace(14.0),
                            if is_active { theme::ACCENT } else { theme::DIM },
                        );
                        ui.add_space(16.0);
                        ui.vertical(|ui| {
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                if ui
                                    .link(
                                        RichText::new("删除")
                                            .family(FontFamily::Monospace)
                                            .size(11.0)
                                            .color(theme::DIM),
                                    )
                                    .clicked()
                                {
                                    delete_id = Some(p.id.clone());
                                }
                                ui.add_space(12.0);
                                if ui
                                    .link(
                                        RichText::new("测速")
                                            .family(FontFamily::Monospace)
                                            .size(11.0)
                                            .color(theme::DIM),
                                    )
                                    .on_hover_text("测试经过当前上行策略 (梯子/直连) 到该供应商的连通延迟")
                                    .clicked()
                                {
                                    ping_provider = Some(p.clone());
                                }
                                ui.add_space(12.0);
                                if ui
                                    .link(
                                        RichText::new("编辑")
                                            .family(FontFamily::Monospace)
                                            .size(11.0)
                                            .color(theme::DIM),
                                    )
                                    .clicked()
                                {
                                    edit_provider = Some(p.clone());
                                }
                                if !is_active {
                                    ui.add_space(12.0);
                                    if ui
                                        .link(
                                            RichText::new("设为总控默认 ->")
                                                .family(FontFamily::Monospace)
                                                .size(11.0)
                                                .color(theme::ACCENT),
                                        )
                                        .clicked()
                                    {
                                        activate_id = Some(p.id.clone());
                                    }
                                }
                            });
                            ui.add_space(14.0);
                        });
                        ui.add_space(20.0);
                    });
                });
            });
            ui.add_space(8.0);
        }

        if let Some(p) = ping_provider {
            let map = app.provider_pings.clone();
            let ctx = ui.ctx().clone();
            if let Ok(mut guard) = map.lock() {
                guard.insert(p.id.clone(), "测速中...".to_string());
            }
            tokio::spawn(async move {
                let start = std::time::Instant::now();
                let res = match clowder_proxy::upstream::client_for(&p) {
                    Ok(client) => {
                        let url = format!("{}/v1/models", p.base_url.trim_end_matches('/'));
                        match client.get(&url).send().await {
                            Ok(_) => format!("连通 {}ms", start.elapsed().as_millis()),
                            Err(_) => "连接失败".to_string(),
                        }
                    }
                    Err(_) => "代理错误".to_string(),
                };
                if let Ok(mut guard) = map.lock() {
                    guard.insert(p.id, res);
                }
                ctx.request_repaint();
            });
        }

        if let Some(p) = edit_provider {
            app.edit_state = EditState {
                open: true,
                provider_id: p.id.clone(),
                name: p.name.clone(),
                base_url: p.base_url.clone(),
                api_key: String::new(),
                auth_kind: p.auth_kind,
                upstream_strategy: p.upstream_strategy,
                custom_proxy_url: p.custom_proxy_url.clone().unwrap_or_default(),
            };
        }

        if let Some(id) = activate_id {
            if let Ok(mut cfg) = app.state.config.try_write() {
                cfg.active_provider_id = Some(id);
                let _ = cfg.save();
            }
        }
        if let Some(id) = delete_id {
            if let Ok(mut providers) = app.state.providers.try_write() {
                providers.retain(|p| p.id != id);
            }
            if let Ok(mut cfg) = app.state.config.try_write() {
                cfg.providers.retain(|p| p.id != id);
                if cfg.active_provider_id.as_deref() == Some(id.as_str()) {
                    cfg.active_provider_id = None;
                }
                let _ = cfg.save();
            }
        }

        if let Some(msg) = &app.status_message {
            ui.add_space(12.0);
            ui.label(theme::mono_accent(msg));
        }
    });

    if app.edit_state.open {
        let ctx = ui.ctx().clone();
        let mut close = false;
        egui::Window::new("编辑供应商")
            .collapsible(false)
            .resizable(false)
            .show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("名称:");
                    ui.text_edit_singleline(&mut app.edit_state.name);
                });
                ui.horizontal(|ui| {
                    ui.label("Base URL:");
                    ui.text_edit_singleline(&mut app.edit_state.base_url);
                });
                ui.horizontal(|ui| {
                    ui.label("认证方式:");
                    egui::ComboBox::from_id_salt("auth_kind_combo")
                        .selected_text(app.edit_state.auth_kind.to_string())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut app.edit_state.auth_kind,
                                AuthKind::ApiKey,
                                "API Key",
                            );
                            ui.selectable_value(
                                &mut app.edit_state.auth_kind,
                                AuthKind::ClaudeCodeOauth,
                                "Claude 官方 OAuth (~/.claude/.credentials.json)",
                            );
                            ui.selectable_value(
                                &mut app.edit_state.auth_kind,
                                AuthKind::CodexOauth,
                                "Codex 官方 OAuth (~/.codex/auth.json)",
                            );
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("API Key (留空则不修改/透传客户端原生认证):");
                    ui.add(egui::TextEdit::singleline(&mut app.edit_state.api_key).password(true));
                });
                ui.horizontal(|ui| {
                    ui.label("上行策略:");
                    egui::ComboBox::from_id_salt("upstream_strategy")
                        .selected_text(app.edit_state.upstream_strategy.to_string())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut app.edit_state.upstream_strategy,
                                UpstreamStrategy::Direct,
                                "直连 (国内免梯子)",
                            );
                            ui.selectable_value(
                                &mut app.edit_state.upstream_strategy,
                                UpstreamStrategy::SystemProxy,
                                "系统代理 (自动串联梯子)",
                            );
                            ui.selectable_value(
                                &mut app.edit_state.upstream_strategy,
                                UpstreamStrategy::Custom,
                                "指定梯子端口",
                            );
                        });
                });
                if app.edit_state.upstream_strategy == UpstreamStrategy::Custom {
                    ui.horizontal(|ui| {
                        ui.label("梯子代理 URL (如 http://127.0.0.1:7890):");
                        ui.text_edit_singleline(&mut app.edit_state.custom_proxy_url);
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button("保存").clicked() {
                        if let Ok(mut providers) = app.state.providers.try_write() {
                            if let Some(p) = providers
                                .iter_mut()
                                .find(|p| p.id == app.edit_state.provider_id)
                            {
                                p.name = app.edit_state.name.clone();
                                p.base_url = app.edit_state.base_url.clone();
                                p.auth_kind = app.edit_state.auth_kind;
                                p.upstream_strategy = app.edit_state.upstream_strategy;
                                p.custom_proxy_url = if app.edit_state.custom_proxy_url.is_empty() {
                                    None
                                } else {
                                    Some(app.edit_state.custom_proxy_url.clone())
                                };
                                if !app.edit_state.api_key.is_empty() {
                                    match clowder_proxy::dpapi::encrypt(&app.edit_state.api_key) {
                                        Ok(enc) => p.api_key_encrypted = enc,
                                        Err(e) => {
                                            app.status_message = Some(format!("加密失败: {}", e))
                                        }
                                    }
                                }
                            }
                            if let Ok(mut cfg) = app.state.config.try_write() {
                                cfg.providers = providers.clone();
                                let _ = cfg.save();
                            }
                        }
                        close = true;
                    }
                    if ui.button("取消").clicked() {
                        close = true;
                    }
                });
            });
        if close {
            app.edit_state.open = false;
        }
    }
}

fn spawn_agent_process(cmd_line: &str) -> anyhow::Result<()> {
    let parts: Vec<&str> = cmd_line.split_whitespace().collect();
    let Some((prog, args)) = parts.split_first() else {
        return Err(anyhow::anyhow!("empty launch command"));
    };
    std::process::Command::new(prog).args(args).spawn()?;
    Ok(())
}

fn add_provider(app: &mut ClowderApp, protocol: Protocol) {
    let mut p = Provider::new_template(protocol);
    if let Ok(mut providers) = app.state.providers.try_write() {
        let base_id = p.id.clone();
        let mut idx = 1usize;
        while providers.iter().any(|existing| existing.id == p.id) {
            idx += 1;
            p.id = format!("{}-{}", base_id, idx);
            p.name = format!("{} #{}", p.name, idx);
        }
        providers.push(p);
        if let Ok(mut cfg) = app.state.config.try_write() {
            cfg.providers = providers.clone();
            if cfg.active_provider_id.is_none() {
                cfg.active_provider_id = cfg.providers.first().map(|first| first.id.clone());
            }
            let _ = cfg.save();
        }
    }
}

fn add_preset_provider(app: &mut ClowderApp, preset: Provider) {
    if let Ok(mut providers) = app.state.providers.try_write() {
        let pid = preset.id.clone();
        if !providers.iter().any(|existing| existing.id == pid) {
            providers.push(preset);
            if let Ok(mut cfg) = app.state.config.try_write() {
                cfg.providers = providers.clone();
                if cfg.active_provider_id.is_none() {
                    cfg.active_provider_id = Some(pid);
                }
                let _ = cfg.save();
            }
        }
    }
}
