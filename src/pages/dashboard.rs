use crate::app::ClowderApp;
use crate::theme;
use egui::{FontFamily, RichText, Ui};

pub fn show(ui: &mut Ui, app: &mut ClowderApp) {
    // --- stats row: big serif-feel numbers with sub-caption, hairline separated ---
    let s = app.last_summary.clone().unwrap_or_default();
    let cols: [(&str, String, bool, String); 5] = [
        (
            "今日输入",
            format_tokens(s.input_tokens),
            true,
            "tokens".into(),
        ),
        (
            "今日输出",
            format_tokens(s.output_tokens),
            true,
            "tokens".into(),
        ),
        (
            "今日费用",
            format!("${:.4}", s.cost_usd),
            true,
            "usd".into(),
        ),
        (
            "平均 TPS",
            s.avg_tps
                .map(|v| format!("{:.1}", v))
                .unwrap_or_else(|| "-".into()),
            false,
            "tok/s".into(),
        ),
        ("请求数", s.request_count.to_string(), false, "req".into()),
    ];
    ui.horizontal(|ui| {
        let total = cols.len();
        for (i, (label, value, highlight, unit)) in cols.iter().enumerate() {
            stat_column(ui, label, value, *highlight, unit);
            if i + 1 < total {
                ui.add_space(32.0);
                stat_divider(ui);
                ui.add_space(32.0);
            }
        }
    });

    ui.add_space(48.0);

    // --- chart with axis decoration ---
    ui.horizontal(|ui| {
        ui.label(theme::meta("TPS / Tokens · 60 min"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(theme::mono_accent("● TPS"));
            ui.add_space(12.0);
            ui.label(theme::mono_dim("— Tokens"));
        });
    });
    ui.add_space(10.0);
    egui::Frame::NONE
        .stroke(egui::Stroke::new(1.0, theme::HAIRLINE))
        .corner_radius(egui::CornerRadius::ZERO)
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            crate::charts::realtime_chart(ui, &app.realtime_series);
        });

    ui.add_space(48.0);

    // --- recent requests: strict table with column widths and right-aligned numbers ---
    ui.label(theme::h2("最近请求"));
    ui.add_space(14.0);

    // column layout: TIME | MODEL | PROVIDER | IN | OUT | TPS | COST | MS | STATUS
    // widths picked for ~1500px content
    let widths: [f32; 9] = [80.0, 200.0, 120.0, 70.0, 70.0, 70.0, 90.0, 60.0, 60.0];
    let headers = ["TIME", "MODEL", "PROVIDER", "IN", "OUT", "TPS", "COST", "MS", "●"];

    egui::ScrollArea::vertical().show(ui, |ui| {
        // header
        ui.horizontal(|ui| {
            for (h, w) in headers.iter().zip(widths.iter()) {
                ui.allocate_ui(egui::vec2(*w, 14.0), |ui| {
                    ui.label(
                        RichText::new(*h)
                            .family(FontFamily::Monospace)
                            .size(10.0)
                            .color(theme::FAINT),
                    );
                });
            }
        });
        ui.add_space(6.0);
        let rect = ui.available_rect_before_wrap();
        ui.painter().hline(
            rect.left()..=rect.left() + widths.iter().sum::<f32>(),
            rect.top(),
            egui::Stroke::new(1.0, theme::HAIRLINE),
        );
        ui.add_space(6.0);

        for r in app.recent_usage.iter().take(20) {
            let row_rect = ui.available_rect_before_wrap();
            let hovered = ui.rect_contains_pointer(row_rect);
            if hovered {
                ui.painter().rect_filled(
                    row_rect,
                    0.0,
                    egui::Color32::from_rgba_premultiplied(15, 44, 63, 12),
                );
            }
            ui.horizontal(|ui| {
                cell_text(ui, widths[0], theme::mono_dim(r.ts.format("%H:%M:%S").to_string()));
                cell_text(ui, widths[1], theme::mono(&r.model));
                cell_text(ui, widths[2], theme::mono_dim(&r.provider_name));
                cell_text_right(ui, widths[3], theme::mono(r.input_tokens.to_string()));
                cell_text_right(ui, widths[4], theme::mono(r.output_tokens.to_string()));
                cell_text_right(
                    ui,
                    widths[5],
                    theme::mono_accent(r.tps.map(|v| format!("{:.1}", v)).unwrap_or_default()),
                );
                cell_text_right(
                    ui,
                    widths[6],
                    theme::mono_dim(r.cost_usd.map(|v| format!("${:.5}", v)).unwrap_or_default()),
                );
                cell_text_right(ui, widths[7], theme::mono_dim(format!("{}", r.duration_ms)));
                let (c, t) = match r.status {
                    clowder_core::Status::Ok => (theme::POSITIVE, "●"),
                    clowder_core::Status::Error => (theme::NEGATIVE, "●"),
                    clowder_core::Status::Aborted => (theme::DIM, "●"),
                };
                cell_text(ui, widths[8], RichText::new(t).color(c));
            });
            ui.add_space(6.0);
        }
    });
}

fn stat_column(ui: &mut Ui, label: &str, value: &str, highlight: bool, unit: &str) {
    ui.vertical(|ui| {
        ui.label(theme::meta(label));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let text_color = if highlight { theme::ACCENT } else { theme::TEXT };
            ui.label(
                RichText::new(value)
                    .family(FontFamily::Name("serif-display".into()))
                    .size(52.0)
                    .color(text_color)
                    .strong(),
            );
            ui.add_space(8.0);
            ui.add(egui::Label::new(
                RichText::new(unit)
                    .family(FontFamily::Monospace)
                    .size(10.0)
                    .color(theme::FAINT),
            ).wrap_mode(egui::TextWrapMode::Extend));
        });
        ui.add_space(6.0);
        // 高亮列下 1px 天青线
        if highlight {
            let rect = ui.available_rect_before_wrap();
            ui.painter().hline(
                rect.left()..=rect.left() + 40.0,
                rect.top(),
                egui::Stroke::new(2.0, theme::ACCENT),
            );
        }
    });
}

fn stat_divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 72.0), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.top()..=rect.bottom(),
        egui::Stroke::new(1.0, theme::HAIRLINE),
    );
}

fn cell_text(ui: &mut Ui, width: f32, text: RichText) {
    ui.allocate_ui(egui::vec2(width, 18.0), |ui| {
        ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Truncate));
    });
}

fn cell_text_right(ui: &mut Ui, width: f32, text: RichText) {
    ui.allocate_ui(egui::vec2(width, 18.0), |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Truncate));
        });
    });
}

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
