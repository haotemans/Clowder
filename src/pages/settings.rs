use crate::app::ClowderApp;
use crate::theme;
use egui::{FontId, Ui};

pub fn show(ui: &mut Ui, app: &mut ClowderApp) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut changed = false;
        if let Ok(mut cfg) = app.state.config.try_write() {
            // 1. 显示与功能模块开关
            ui.label(theme::h2("显示功能与右上角 1/4 圆切换器"));
            ui.add_space(10.0);
            changed |= ui
                .checkbox(
                    &mut cfg.ui.show_corner_switcher,
                    "显示右上角 1/4 圆 Agent 扇形切换器 (点击弹出当前可用 Agent)",
                )
                .changed();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                changed |= ui
                    .checkbox(&mut cfg.ui.show_dashboard_tab, "显示「仪表盘」页签")
                    .changed();
                ui.add_space(16.0);
                changed |= ui
                    .checkbox(&mut cfg.ui.show_providers_tab, "显示「Agent 与供应商」页签")
                    .changed();
                ui.add_space(16.0);
                changed |= ui
                    .checkbox(&mut cfg.ui.show_sessions_tab, "显示「统一会话」页签")
                    .changed();
            });

            ui.add_space(10.0);
            ui.label(theme::meta("勾选要在右上角 1/4 圆切换器及配置面板中显示的 Agent:"));
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for prof in cfg.agents.iter_mut() {
                    let label = format!("{} ({})", prof.agent.label(), prof.agent.badge());
                    if ui.checkbox(&mut prof.enabled_in_switcher, label).changed() {
                        changed = true;
                    }
                    ui.add_space(12.0);
                }
            });
            theme::hairline(ui);

            // 2. 统一会话管理与隐私授权
            ui.label(theme::h2("统一会话管理与隐私授权"));
            ui.add_space(10.0);
            changed |= ui
                .checkbox(
                    &mut cfg.ui.session_consent_granted,
                    "已取得用户同意：开启跨 Agent 统一会话管理",
                )
                .changed();
            ui.add_space(4.0);
            ui.add_enabled_ui(cfg.ui.session_consent_granted, |ui| {
                changed |= ui
                    .checkbox(
                        &mut cfg.ui.scan_local_agent_sessions,
                        "同时扫描各 Agent 本地会话目录元数据 (~/.claude, ~/.codex, ~/.pi, ~/.dsh, ~/.config/opencode)",
                    )
                    .changed();
            });
            theme::hairline(ui);

            // 3. 本地代理
            ui.label(theme::h2("本地代理"));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(theme::meta("监听地址"));
                ui.add_space(12.0);
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut cfg.proxy.listen)
                            .font(FontId::monospace(13.0))
                            .desired_width(180.0),
                    )
                    .changed()
                {
                    changed = true;
                }
            });
            ui.add_space(6.0);
            changed |= ui
                .checkbox(&mut cfg.proxy.auto_start, "启动时自动开启代理")
                .changed();
            theme::hairline(ui);

            // 4. 价格源
            ui.label(theme::h2("价格源与汇率"));
            ui.add_space(10.0);
            changed |= ui
                .checkbox(&mut cfg.pricing.litellm_enabled, "拉取 LiteLLM 价目")
                .on_hover_text("raw.githubusercontent.com")
                .changed();
            ui.add_space(4.0);
            changed |= ui
                .checkbox(&mut cfg.pricing.openrouter_enabled, "拉取 OpenRouter 价目")
                .changed();
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(theme::meta("刷新间隔 · 小时"));
                ui.add_space(12.0);
                changed |= ui
                    .add(egui::DragValue::new(&mut cfg.pricing.refresh_hours).range(1..=168))
                    .changed();
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(theme::meta("汇率 USD/CNY"))
                    .on_hover_text("留空则不换算人民币");
                ui.add_space(12.0);
                let mut s = cfg
                    .pricing
                    .usd_cny
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut s)
                            .font(FontId::monospace(13.0))
                            .desired_width(100.0),
                    )
                    .changed()
                {
                    cfg.pricing.usd_cny = s.parse::<f64>().ok();
                    changed = true;
                }
            });
            theme::hairline(ui);

            // 5. 系统与数据目录
            ui.label(theme::h2("系统与数据"));
            ui.add_space(10.0);
            let prev_autostart = cfg.ui.autostart;
            ui.checkbox(&mut cfg.ui.autostart, "开机自启动");
            if cfg.ui.autostart != prev_autostart {
                let result = if cfg.ui.autostart {
                    crate::autostart::enable()
                } else {
                    crate::autostart::disable()
                };
                if let Err(e) = result {
                    app.status_message = Some(format!("自启动设置失败: {}", e));
                    cfg.ui.autostart = prev_autostart;
                } else {
                    changed = true;
                }
            }
            ui.add_space(8.0);
            ui.label(theme::meta(format!(
                "数据目录: {}",
                clowder_core::data_dir().unwrap_or_default().display()
            )));
            ui.label(theme::meta(format!(
                "AI 配置管理接口: http://{}/_clowder/status",
                cfg.proxy.listen
            )));

            if changed {
                let _ = cfg.save();
            }
        } else {
            ui.label(theme::meta("配置被占用, 请稍后再试"));
        }
    });
}
