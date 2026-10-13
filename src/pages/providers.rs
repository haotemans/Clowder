use crate::app::YtsApp;
use crate::theme;
use egui::{FontFamily, FontId, RichText, Ui};
use yts_core::{Protocol, Provider, UpstreamStrategy};

#[derive(Default)]
pub struct EditState {
    pub open: bool,
    pub provider_id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub upstream_strategy: UpstreamStrategy,
    pub custom_proxy_url: String,
}

pub fn show(ui: &mut Ui, app: &mut YtsApp) {
    ui.label(theme::h1("供应商"));
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        for (label, protocol) in [
            ("+ Anthropic", Some(Protocol::Anthropic)),
            ("+ OpenAI", Some(Protocol::OpenAI)),
        ] {
            if ui
                .button(RichText::new(label).family(FontFamily::Monospace).size(12.0))
                .clicked()
            {
                if let Some(p) = protocol {
                    add_provider(app, p);
                }
            }
            ui.add_space(8.0);
        }
        if ui
            .button(
                RichText::new("+ OpenRouter")
                    .family(FontFamily::Monospace)
                    .size(12.0),
            )
            .clicked()
        {
            add_openrouter(app);
        }
    });

    ui.add_space(24.0);

    let mut delete_id: Option<String> = None;
    let mut activate_id: Option<String> = None;
    let mut edit_provider: Option<Provider> = None;

    let providers_snapshot = app.state.providers.try_read().ok().map(|p| p.clone());
    if let Some(providers) = providers_snapshot {
        let active_id = app
            .state
            .config
            .try_read()
            .ok()
            .and_then(|c| c.active_provider_id.clone());

        if providers.is_empty() {
            ui.label(theme::meta("还没有供应商"));
        }

        for p in providers.iter() {
            let is_active = active_id.as_deref() == Some(p.id.as_str());
            // 卡片: hairline 描边, 激活时加 3px 天青左条
            let frame = egui::Frame::NONE
                .fill(theme::CARD)
                .stroke(egui::Stroke::new(1.0, theme::HAIRLINE))
                .corner_radius(egui::CornerRadius::ZERO)
                .inner_margin(egui::Margin::same(0));
            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    // left strip
                    let strip_color = if is_active {
                        theme::ACCENT
                    } else {
                        theme::FAINT
                    };
                    let (rect, _) = ui
                        .allocate_exact_size(egui::vec2(3.0, 76.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 0.0, strip_color);
                    ui.add_space(20.0);

                    ui.vertical(|ui| {
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            ui.label(theme::h2(&p.name));
                            ui.add_space(10.0);
                            if is_active {
                                ui.label(theme::mono_accent("[active]"));
                            }
                        });
                        ui.add_space(6.0);
                        let line = ui.label(theme::mono_dim(format!(
                            "{:?} · {} · 上行 {}",
                            p.protocol, p.base_url, p.upstream_strategy
                        )));
                        // id 只在 /p/{id} 路由时用得到, 收进 tooltip
                        line.on_hover_text(format!("id {}", p.id));
                        ui.add_space(14.0);
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(20.0);
                        // protocol badge: A for anthropic, O for openai
                        let badge = match p.protocol {
                            Protocol::Anthropic => "A",
                            Protocol::OpenAI => "O",
                        };
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(28.0, 28.0),
                            egui::Sense::hover(),
                        );
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
                                if ui.link(RichText::new("删除").family(FontFamily::Monospace).size(11.0).color(theme::DIM)).clicked() {
                                    delete_id = Some(p.id.clone());
                                }
                                ui.add_space(14.0);
                                if ui.link(RichText::new("编辑").family(FontFamily::Monospace).size(11.0).color(theme::DIM)).clicked() {
                                    edit_provider = Some(p.clone());
                                }
                                if !is_active {
                                    ui.add_space(14.0);
                                    if ui.link(RichText::new("激活 →").family(FontFamily::Monospace).size(11.0).color(theme::ACCENT)).clicked() {
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
    }

    if let Some(p) = edit_provider {
        app.edit_state = EditState {
            open: true,
            provider_id: p.id.clone(),
            name: p.name.clone(),
            base_url: p.base_url.clone(),
            api_key: String::new(),
            upstream_strategy: p.upstream_strategy,
            custom_proxy_url: p.custom_proxy_url.clone().unwrap_or_default(),
        };
    }

    // edit window
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
                    ui.label("API Key (留空则不修改):");
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
                                "直连",
                            );
                            ui.selectable_value(
                                &mut app.edit_state.upstream_strategy,
                                UpstreamStrategy::SystemProxy,
                                "系统代理",
                            );
                            ui.selectable_value(
                                &mut app.edit_state.upstream_strategy,
                                UpstreamStrategy::Custom,
                                "自定义",
                            );
                        });
                });
                if app.edit_state.upstream_strategy == UpstreamStrategy::Custom {
                    ui.horizontal(|ui| {
                        ui.label("自定义代理 URL:");
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
                                p.upstream_strategy = app.edit_state.upstream_strategy;
                                p.custom_proxy_url = if app.edit_state.custom_proxy_url.is_empty() {
                                    None
                                } else {
                                    Some(app.edit_state.custom_proxy_url.clone())
                                };
                                if !app.edit_state.api_key.is_empty() {
                                    match yts_proxy::dpapi::encrypt(&app.edit_state.api_key) {
                                        Ok(enc) => p.api_key_encrypted = enc,
                                        Err(e) => {
                                            app.status_message = Some(format!("加密失败: {}", e))
                                        }
                                    }
                                }
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
            if cfg.active_provider_id.as_deref() == Some(id.as_str()) {
                cfg.active_provider_id = None;
            }
            let _ = cfg.save();
        }
    }

    theme::section_gap(ui);
    ui.label(theme::h2("Claude Code 配置"));
    ui.add_space(12.0);
    if ui
        .button(
            RichText::new("写入 Claude Code 配置")
                .family(FontFamily::Monospace)
                .size(12.0),
        )
        .clicked()
    {
        match write_claude_code_config(app) {
            Ok(msg) => app.status_message = Some(msg),
            Err(e) => app.status_message = Some(format!("错误: {}", e)),
        }
    }
    if let Some(msg) = &app.status_message {
        ui.add_space(8.0);
        ui.label(theme::meta(msg));
    }
}

fn add_provider(app: &mut YtsApp, protocol: Protocol) {
    let p = Provider::new_template(protocol);
    if let Ok(mut providers) = app.state.providers.try_write() {
        providers.push(p);
    }
}

fn add_openrouter(app: &mut YtsApp) {
    let p = Provider::openrouter();
    if let Ok(mut providers) = app.state.providers.try_write() {
        providers.push(p);
    }
}

fn write_claude_code_config(app: &mut YtsApp) -> anyhow::Result<String> {
    let (provider, listen) = {
        let cfg = app
            .state
            .config
            .try_read()
            .map_err(|_| anyhow::anyhow!("config locked"))?;
        let p = cfg
            .active_provider()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no active provider"))?;
        (p, cfg.proxy.listen.clone())
    };

    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME"))?;
    let dir = std::path::Path::new(&home).join(".claude");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("settings.json");

    let mut json: serde_json::Value = if path.exists() {
        serde_json::from_str(&std::fs::read_to_string(&path)?)?
    } else {
        serde_json::json!({})
    };

    let env = json
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("invalid settings.json"))?
        .entry("env")
        .or_insert_with(|| serde_json::json!({}));
    let env_obj = env
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("invalid env in settings.json"))?;

    match provider.protocol {
        Protocol::Anthropic => {
            env_obj.insert(
                "ANTHROPIC_BASE_URL".into(),
                serde_json::Value::String(format!("http://{}/anthropic", listen)),
            );
            // do NOT write ANTHROPIC_AUTH_TOKEN; proxy injects real key
        }
        Protocol::OpenAI => {
            env_obj.insert(
                "OPENAI_BASE_URL".into(),
                serde_json::Value::String(format!("http://{}/openai", listen)),
            );
        }
    }

    std::fs::write(&path, serde_json::to_string_pretty(&json)?)?;
    Ok(format!("已写入 {}", path.display()))
}
