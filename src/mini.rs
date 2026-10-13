use crate::app::ClowderApp;
use crate::mini_theme as mt;
use egui::{Color32, FontFamily, FontId, Pos2, RichText, Stroke, Ui, Vec2};
use std::f32::consts::PI;

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

        // draw label & buttons directly
        let painter = ui.painter();
        painter.text(
            drag_zone.left_center(),
            egui::Align2::LEFT_CENTER,
            "Clowder",
            FontId::monospace(11.0),
            mt::TEXT_FAINT,
        );

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
            small_stat(ui, "缓存读", format_short(s.cache_read_tokens));
            small_stat(ui, "缓存写", format_short(s.cache_write_tokens));
            small_stat(ui, "今日费", format!("${:.3}", s.cost_usd));
        });
    });
}

fn draw_gauge(painter: &egui::Painter, rect: egui::Rect, app: &ClowderApp) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) / 2.0 - 10.0;

    let current_tps = app
        .realtime_series
        .last()
        .map(|(_, tps, _)| *tps)
        .unwrap_or(0.0);
    let recent_max = app
        .realtime_series
        .iter()
        .rev()
        .take(60)
        .map(|(_, tps, _)| *tps)
        .fold(0.0_f64, f64::max);
    let max_tps = (recent_max.max(100.0)).max(current_tps * 1.5).max(50.0);

    // outer ring
    painter.circle_stroke(center, radius, Stroke::new(1.5, mt::HAIRLINE));

    // ticks: 11 marks around (speedometer style, 270° sweep from bottom-left to bottom-right)
    let start_angle = PI * 0.75;
    let sweep_total = PI * 1.5;
    for i in 0..=10 {
        let t = i as f32 / 10.0;
        let angle = start_angle + sweep_total * t;
        let (cos, sin) = (angle.cos(), angle.sin());
        let outer = radius - 4.0;
        let inner = radius - 14.0;
        let p1 = Pos2::new(center.x + cos * outer, center.y - sin * outer);
        let p2 = Pos2::new(center.x + cos * inner, center.y - sin * inner);
        let stroke = if i % 5 == 0 {
            Stroke::new(1.5, mt::TEXT_MUTED)
        } else {
            Stroke::new(1.0, mt::TEXT_FAINT)
        };
        painter.line_segment([p1, p2], stroke);
    }

    // active arc: 柔和青
    let value_ratio = (current_tps / max_tps).clamp(0.0, 1.0) as f32;
    let arc_end_angle = start_angle + sweep_total * value_ratio;
    let arc_points: Vec<Pos2> = (0..=64)
        .map(|i| {
            let t = i as f32 / 64.0;
            let angle = start_angle + (arc_end_angle - start_angle) * t;
            Pos2::new(
                center.x + angle.cos() * radius,
                center.y - angle.sin() * radius,
            )
        })
        .collect();
    if arc_points.len() > 1 {
        painter.add(egui::Shape::line(
            arc_points,
            Stroke::new(4.0, mt::ACCENT_CYAN),
        ));
    }

    // needle
    let needle_tip = Pos2::new(
        center.x + arc_end_angle.cos() * (radius - 24.0),
        center.y - arc_end_angle.sin() * (radius - 24.0),
    );
    painter.line_segment([center, needle_tip], Stroke::new(2.0, mt::NEEDLE));
    painter.circle_filled(center, 4.0, mt::NEEDLE);
    painter.circle_stroke(center, 4.0, Stroke::new(1.0, Color32::WHITE));

    // center value: serif 大数字
    painter.text(
        Pos2::new(center.x, center.y + 48.0),
        egui::Align2::CENTER_CENTER,
        format!("{:.1}", current_tps),
        FontId::new(46.0, FontFamily::Name("serif-display".into())),
        mt::ACCENT_CYAN,
    );
    painter.text(
        Pos2::new(center.x, center.y + 78.0),
        egui::Align2::CENTER_CENTER,
        "tokens/s",
        FontId::monospace(11.0),
        mt::TEXT_MUTED,
    );
    painter.text(
        Pos2::new(center.x, rect.bottom() - 6.0),
        egui::Align2::CENTER_BOTTOM,
        format!("0 — {:.0}", max_tps),
        FontId::monospace(9.0),
        mt::TEXT_FAINT,
    );
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

fn format_short(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
