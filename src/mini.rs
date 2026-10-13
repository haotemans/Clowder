use crate::app::ClowderApp;
use crate::mini_theme as mt;
use crate::theme;
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

    // double-click toggles back to full
    if ctx.input(|i| {
        i.pointer
            .button_double_clicked(egui::PointerButton::Primary)
    }) {
        app.ui_mode = crate::app::UiMode::Full;
    }
}

fn show_content(ctx: &egui::Context, ui: &mut Ui, app: &mut ClowderApp) {
    ui.vertical(|ui| {
        // title-bar: left label, right buttons; drag strip sits behind/around
        let strip_h = 24.0;
        let (strip_rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), strip_h),
            egui::Sense::hover(),
        );
        // define button rects inside strip
        let btn_size = egui::vec2(22.0, strip_h);
        let close_rect = egui::Rect::from_min_size(
            Pos2::new(strip_rect.right() - btn_size.x, strip_rect.top()),
            btn_size,
        );
        let expand_rect = egui::Rect::from_min_size(
            Pos2::new(close_rect.left() - btn_size.x - 4.0, strip_rect.top()),
            btn_size,
        );
        // drag zone = strip area minus buttons
        let drag_zone = egui::Rect::from_min_max(
            strip_rect.min,
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

        // draw buttons directly (标题位不给字, 整条都是拖拽区)
        let painter = ui.painter();

        // expand button
        let expand_resp = ui.interact(
            expand_rect,
            ui.id().with("expand_btn"),
            egui::Sense::click(),
        );
        let expand_color = if expand_resp.hovered() {
            mt::ACCENT_CYAN
        } else {
            mt::TEXT_MUTED
        };
        painter.text(
            expand_rect.center(),
            egui::Align2::CENTER_CENTER,
            "□",
            FontId::monospace(13.0),
            expand_color,
        );
        if expand_resp.clicked() {
            app.ui_mode = crate::app::UiMode::Full;
        }

        // close button
        let close_resp = ui.interact(
            close_rect,
            ui.id().with("close_btn"),
            egui::Sense::click(),
        );
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

        // gauge
        let gauge_height = 220.0;
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), gauge_height),
            egui::Sense::hover(),
        );
        draw_gauge(ui.painter(), rect, app);

        ui.add_space(4.0);

        // bottom stats
        ui.horizontal(|ui| {
            let s = app.last_summary.clone().unwrap_or_default();
            small_stat(ui, "缓存读", theme::format_short(s.cache_read_tokens));
            small_stat(ui, "缓存写", theme::format_short(s.cache_write_tokens));
            small_stat(ui, "今日费", theme::format_cost(s.cost_usd));
        });
    });
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

