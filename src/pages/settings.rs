use crate::app::ClowderApp;
use crate::theme;
use egui::{FontId, Ui};

pub fn show(ui: &mut Ui, app: &mut ClowderApp) {
    ui.label(theme::h1("设置"));
    theme::section_gap(ui);

    let mut changed = false;
    if let Ok(mut cfg) = app.state.config.try_write() {
        // proxy group
        ui.label(theme::h2("本地代理"));
        ui.add_space(12.0);
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
        ui.add_space(8.0);
        ui.checkbox(&mut cfg.proxy.auto_start, "启动时自动开启代理");
        theme::hairline(ui);

        // pricing group
        ui.label(theme::h2("价格源"));
        ui.add_space(12.0);
        ui.checkbox(&mut cfg.pricing.litellm_enabled, "拉取 LiteLLM 价目")
            .on_hover_text("raw.githubusercontent.com");
        ui.add_space(4.0);
        ui.checkbox(&mut cfg.pricing.openrouter_enabled, "拉取 OpenRouter 价目");
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(theme::meta("刷新间隔 · 小时"));
            ui.add_space(12.0);
            ui.add(
                egui::DragValue::new(&mut cfg.pricing.refresh_hours)
                    .range(1..=168),
            );
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
            }
        });
        theme::hairline(ui);

        // ui group
        ui.label(theme::h2("界面"));
        ui.add_space(12.0);
        ui.add_space(4.0);
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
            }
        }
        theme::hairline(ui);

        // data group
        ui.label(theme::h2("数据"));
        ui.add_space(12.0);
        ui.label(theme::meta(format!(
            "目录 {}",
            clowder_core::data_dir().unwrap_or_default().display()
        )));

        if changed {
            let _ = cfg.save();
        }
    } else {
        ui.label(theme::meta("配置被占用, 请稍后再试"));
    }

    if let Some(msg) = &app.status_message {
        ui.add_space(12.0);
        ui.label(theme::meta(msg));
    }
}
