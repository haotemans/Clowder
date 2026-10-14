use crate::app::ClowderApp;
use crate::theme;
use clowder_core::AgentId;
use egui::{FontFamily, RichText, Ui};

pub fn show(ui: &mut Ui, app: &mut ClowderApp) {
    let (consent_granted, scan_local, enabled_agents, usd_cny) = {
        if let Ok(cfg) = app.state.config.try_read() {
            let list: Vec<AgentId> = cfg
                .agents
                .iter()
                .filter(|a| a.enabled_in_switcher)
                .map(|a| a.agent)
                .collect();
            (
                cfg.ui.session_consent_granted,
                cfg.ui.scan_local_agent_sessions,
                list,
                cfg.pricing.usd_cny,
            )
        } else {
            (false, false, Vec::new(), None)
        }
    };

    if !consent_granted {
        egui::Frame::NONE
            .fill(theme::CARD)
            .stroke(egui::Stroke::new(1.5, theme::ACCENT))
            .inner_margin(egui::Margin::symmetric(28, 24))
            .show(ui, |ui| {
                ui.label(theme::h2("开启跨 Agent 统一会话管理 (需用户授权)"));
                ui.add_space(10.0);
                ui.label(theme::mono_dim(
                    "开启后，Clowder 将在本地为您统一聚合管理 Claude Code、Claude Desktop、Codex、Codex Desktop、OpenCode v2、Pi、DSH、DSH Desktop 的会话记录：",
                ));
                ui.add_space(8.0);
                ui.label(theme::meta(
                    "1. 仅在本地 SQLite 与本机目录只读解析会话时间、请求次数、输入/输出 Token 与费用，0 网络上传；",
                ));
                ui.label(theme::meta(
                    "2. 您可随时在「设置 -> 隐私与显示管理」中撤回授权或关闭本地会话目录扫描。",
                ));
                ui.add_space(16.0);
                if ui
                    .button(
                        RichText::new("我已知晓并同意开启统一会话管理")
                            .family(FontFamily::Monospace)
                            .size(13.0),
                    )
                    .clicked()
                {
                    if let Ok(mut cfg) = app.state.config.try_write() {
                        cfg.ui.session_consent_granted = true;
                        let _ = cfg.save();
                    }
                }
            });
        return;
    }

    ui.horizontal_wrapped(|ui| {
        let all_selected = app.session_agent_filter.is_none();
        if ui
            .selectable_label(
                all_selected,
                RichText::new("全部 Agent")
                    .family(FontFamily::Monospace)
                    .size(12.0),
            )
            .clicked()
        {
            app.session_agent_filter = None;
        }
        for ag in &enabled_agents {
            let selected = app.session_agent_filter == Some(*ag);
            if ui
                .selectable_label(
                    selected,
                    RichText::new(format!("{} {}", ag.badge(), ag.label()))
                        .family(FontFamily::Monospace)
                        .size(12.0),
                )
                .clicked()
            {
                app.session_agent_filter = Some(*ag);
            }
        }

        ui.add_space(16.0);
        if ui
            .button(
                RichText::new("导出 CSV")
                    .family(FontFamily::Monospace)
                    .size(12.0),
            )
            .clicked()
        {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "clowder-usage-{}.csv",
                chrono::Utc::now().format("%Y%m%d-%H%M%S")
            ));
            match app.store.export_csv(&path) {
                Ok(_) => app.status_message = Some(format!("已导出: {}", path.display())),
                Err(e) => app.status_message = Some(format!("导出失败: {}", e)),
            }
        }
        if let Some(msg) = &app.status_message {
            ui.add_space(12.0);
            ui.label(theme::meta(msg));
        }
    });

    ui.add_space(18.0);

    let filter_slug = app.session_agent_filter.map(|a| a.as_str());
    match app.store.sessions_filtered(filter_slug, 120, scan_local) {
        Ok(sessions) => {
            if sessions.is_empty() {
                ui.label(theme::meta("当前筛选条件下还没有会话记录"));
                return;
            }
            let avail = (ui.available_width() - 72.0).max(740.0);
            let ratios: [f32; 9] = [0.12, 0.08, 0.24, 0.14, 0.10, 0.07, 0.08, 0.08, 0.09];
            let widths: Vec<f32> = ratios.iter().map(|r| (avail * r).floor()).collect();
            let headers = [
                "AGENT",
                "来源",
                "SESSION ID",
                "开始时间",
                "最近活跃",
                "请求数",
                "输入 TOK",
                "输出 TOK",
                "费用",
            ];

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (h, w) in headers.iter().zip(widths.iter()) {
                        ui.allocate_ui(egui::vec2(*w, 18.0), |ui| {
                            ui.label(
                                RichText::new(*h)
                                    .family(FontFamily::Monospace)
                                    .size(10.0)
                                    .color(theme::FAINT),
                            );
                        });
                    }
                });
                theme::hairline(ui);

                for s in sessions {
                    ui.horizontal(|ui| {
                        ui.allocate_ui(egui::vec2(widths[0], 20.0), |ui| {
                            ui.add(
                                egui::Label::new(theme::mono_accent(&s.agent_id))
                                    .wrap_mode(egui::TextWrapMode::Truncate),
                            );
                        });
                        ui.allocate_ui(egui::vec2(widths[1], 20.0), |ui| {
                            let tag = if s.source_kind == "local" {
                                "本地历史"
                            } else {
                                "实时代理"
                            };
                            ui.label(theme::meta(tag));
                        });
                        ui.allocate_ui(egui::vec2(widths[2], 20.0), |ui| {
                            ui.add(
                                egui::Label::new(theme::mono_dim(&s.session_id))
                                    .wrap_mode(egui::TextWrapMode::Truncate),
                            );
                        });
                        ui.allocate_ui(egui::vec2(widths[3], 20.0), |ui| {
                            ui.label(theme::mono_dim(
                                s.started_at.format("%Y-%m-%d %H:%M").to_string(),
                            ));
                        });
                        ui.allocate_ui(egui::vec2(widths[4], 20.0), |ui| {
                            ui.label(theme::mono_dim(
                                s.last_activity.format("%H:%M:%S").to_string(),
                            ));
                        });
                        ui.allocate_ui(egui::vec2(widths[5], 20.0), |ui| {
                            ui.label(theme::mono(s.request_count.to_string()));
                        });
                        ui.allocate_ui(egui::vec2(widths[6], 20.0), |ui| {
                            ui.label(theme::mono(theme::format_short(s.total_input)));
                        });
                        ui.allocate_ui(egui::vec2(widths[7], 20.0), |ui| {
                            ui.label(theme::mono(theme::format_short(s.total_output)));
                        });
                        ui.allocate_ui(egui::vec2(widths[8], 20.0), |ui| {
                            ui.label(theme::mono_dim(theme::format_cost_with_rate(
                                s.total_cost,
                                usd_cny,
                            )));
                        });
                    });
                    ui.add_space(4.0);
                }
            });
        }
        Err(e) => {
            ui.label(theme::meta(format!("加载失败: {}", e)));
        }
    }
}
