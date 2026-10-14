use crate::app::ClowderApp;
use crate::mini_theme as mt;
use crate::theme;
use clowder_core::AgentId;
use egui::{Color32, FontFamily, FontId, Pos2, RichText, Stroke, Ui, Vec2};

/// mini 的浅色玻璃主题: 只在切到 mini 时设一次, 不要每帧 set_visuals
pub fn apply_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.window_fill = Color32::TRANSPARENT;
    visuals.panel_fill = Color32::TRANSPARENT;
    ctx.set_visuals(visuals);
}

pub fn show(ctx: &egui::Context, app: &mut ClowderApp) {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(Color32::TRANSPARENT))
        .show(ctx, |ui| {
            let full_rect = ui.max_rect();
            ui.painter().rect(
                full_rect,
                egui::CornerRadius::same(14),
                mt::BG_GLASS,
                Stroke::new(1.0, mt::HAIRLINE),
                egui::StrokeKind::Outside,
            );

            let content = full_rect.shrink(14.0);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
            show_content(ctx, &mut child, app);
        });

    if ctx.input(|i| {
        i.pointer
            .button_double_clicked(egui::PointerButton::Primary)
    }) {
        app.ui_mode = crate::app::UiMode::Full;
    }
}

fn show_content(ctx: &egui::Context, ui: &mut Ui, app: &mut ClowderApp) {
    ui.vertical(|ui| {
        let strip_h = 24.0;
        let (strip_rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), strip_h), egui::Sense::hover());

        // 左侧 Agent 快捷切换胶囊 (在小窗也能看到当前 Agent 并一键切换)
        let pill_rect =
            egui::Rect::from_min_size(strip_rect.min, egui::vec2(76.0, strip_h - 2.0));
        let pill_resp = ui.interact(pill_rect, ui.id().with("mini_agent_pill"), egui::Sense::click());
        let current_badge = app
            .state
            .config
            .try_read()
            .ok()
            .and_then(|c| c.current_agent)
            .map(|a| format!("{} {}", a.badge(), a.as_str()))
            .unwrap_or_else(|| "ALL 总控".to_string());
        ui.painter().rect(
            pill_rect,
            egui::CornerRadius::same(10),
            if pill_resp.hovered() {
                mt::HAIRLINE
            } else {
                Color32::from_rgba_premultiplied(255, 255, 255, 180)
            },
            Stroke::new(1.0, mt::HAIRLINE),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            pill_rect.center(),
            egui::Align2::CENTER_CENTER,
            current_badge,
            FontId::monospace(10.0),
            mt::TEXT_DARK,
        );
        if pill_resp.on_hover_text("点击切换当前监控的 Agent").clicked() {
            cycle_mini_agent(app);
        }

        let btn_size = egui::vec2(22.0, strip_h);
        let close_rect = egui::Rect::from_min_size(
            Pos2::new(strip_rect.right() - btn_size.x, strip_rect.top()),
            btn_size,
        );
        let expand_rect = egui::Rect::from_min_size(
            Pos2::new(close_rect.left() - btn_size.x - 4.0, strip_rect.top()),
            btn_size,
        );
        let drag_zone = egui::Rect::from_min_max(
            Pos2::new(pill_rect.right() + 4.0, strip_rect.top()),
            Pos2::new(expand_rect.left() - 4.0, strip_rect.bottom()),
        );
        let drag_resp = ui.interact(
            drag_zone,
            ui.id().with("title_drag"),
            egui::Sense::click_and_drag(),
        );
        if drag_resp.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        let painter = ui.painter();

        // expand button (自绘矢量方框，防缺字豆腐块)
        let expand_resp =
            ui.interact(expand_rect, ui.id().with("expand_btn"), egui::Sense::click());
        let expand_color = if expand_resp.hovered() {
            mt::ACCENT_CYAN
        } else {
            mt::TEXT_MUTED
        };
        let box_rect = egui::Rect::from_center_size(expand_rect.center(), egui::vec2(10.0, 10.0));
        painter.rect_stroke(
            box_rect,
            1.0,
            Stroke::new(1.3, expand_color),
            egui::StrokeKind::Inside,
        );
        if expand_resp.on_hover_text("展开主界面").clicked() {
            app.ui_mode = crate::app::UiMode::Full;
        }

        // close button
        let close_resp = ui.interact(close_rect, ui.id().with("close_btn"), egui::Sense::click());
        let close_color = if close_resp.hovered() {
            mt::NEEDLE
        } else {
            mt::TEXT_MUTED
        };
        painter.text(
            close_rect.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            FontId::monospace(16.0),
            close_color,
        );
        if close_resp.clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        ui.add_space(6.0);

        let gauge_height = 220.0;
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), gauge_height),
            egui::Sense::hover(),
        );
        draw_gauge(ui.painter(), rect, app);

        ui.add_space(4.0);

        ui.horizontal(|ui| {
            let s = app.last_summary.clone().unwrap_or_default();
            small_stat(ui, "缓存读", theme::format_short(s.cache_read_tokens));
            small_stat(ui, "缓存写", theme::format_short(s.cache_write_tokens));
            small_stat(ui, "今日费", theme::format_cost(s.cost_usd));
        });
    });
}

fn cycle_mini_agent(app: &mut ClowderApp) {
    if let Ok(mut cfg) = app.state.config.try_write() {
        let enabled: Vec<AgentId> = cfg
            .agents
            .iter()
            .filter(|a| a.enabled_in_switcher)
            .map(|a| a.agent)
            .collect();
        if enabled.is_empty() {
            cfg.current_agent = None;
        } else {
            cfg.current_agent = match cfg.current_agent {
                None => Some(enabled[0]),
                Some(cur) => {
                    let pos = enabled.iter().position(|a| *a == cur);
                    match pos {
                        Some(i) if i + 1 < enabled.len() => Some(enabled[i + 1]),
                        _ => None,
                    }
                }
            };
        }
        let _ = cfg.save();
    }
    app.refresh_summary();
}

fn draw_gauge(painter: &egui::Painter, rect: egui::Rect, app: &ClowderApp) {
    let current = crate::gauge::live_tps(&app.realtime_series);
    let max = crate::gauge::scale(crate::gauge::peak_tps(&app.realtime_series).max(current));
    crate::gauge::draw(painter, rect, current, max, 46.0, &crate::gauge::MINI);
}

fn small_stat(ui: &mut Ui, label: &str, value: String) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(label)
                .family(FontFamily::Monospace)
                .size(9.0)
                .color(mt::TEXT_FAINT),
        );
        ui.add_space(2.0);
        ui.label(
            RichText::new(value)
                .family(FontFamily::Monospace)
                .size(15.0)
                .color(mt::TEXT_DARK)
                .strong(),
        );
    });
    ui.add_space(18.0);
}
