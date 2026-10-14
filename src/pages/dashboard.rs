use crate::app::ClowderApp;
use crate::theme;
use egui::{Align, FontFamily, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

const CELL: f32 = 14.0;
const MAX_WEEKS: usize = 53;
const BAND_H: f32 = 14.0 + 20.0 * 7.0 + 3.0 * 6.0 + 10.0;
const GAUGE_W: f32 = 250.0;

pub fn show(ui: &mut Ui, app: &mut ClowderApp) {
    let usd_cny = app
        .state
        .config
        .try_read()
        .ok()
        .and_then(|c| c.pricing.usd_cny);
    let s = app.last_summary.clone().unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(28.0, 14.0);
        stat(ui, "今日净输入", theme::format_short(s.input_tokens), "tokens");
        stat(ui, "今日输出", theme::format_short(s.output_tokens), "tokens");
        stat(ui, "缓存命中", theme::format_short(s.cache_read_tokens), "tokens");
        stat(ui, "缓存写入", theme::format_short(s.cache_write_tokens), "tokens");
        stat(ui, "推理思考", theme::format_short(s.reasoning_tokens), "tokens");
        stat(
            ui,
            "今日费用",
            theme::format_cost_with_rate(s.cost_usd, usd_cny),
            "",
        );
        stat(
            ui,
            "纯解码 TPS",
            s.avg_tps
                .map(|v| format!("{:.1}", v))
                .unwrap_or_else(|| "-".into()),
            "tok/s",
        );
        stat(ui, "请求数", s.request_count.to_string(), "req");
    });

    ui.add_space(14.0);
    theme::hairline(ui);
    ui.add_space(22.0);

    let full_w = ui.available_width();
    let (band, _) = ui.allocate_exact_size(Vec2::new(full_w, BAND_H), Sense::hover());
    let wall_w = (full_w - GAUGE_W - 40.0).max(200.0);
    let mut child = ui.new_child(
        egui::UiBuilder::new().max_rect(Rect::from_min_size(band.min, Vec2::new(wall_w, BAND_H))),
    );
    crate::charts::calendar_wall(&mut child, &app.daily, CELL, MAX_WEEKS);

    let current = crate::gauge::live_tps(&app.realtime_series);
    let max = crate::gauge::auto_scale(crate::gauge::peak_tps(&app.realtime_series).max(current));
    crate::gauge::draw(
        ui.painter(),
        Rect::from_min_size(
            Pos2::new(band.right() - GAUGE_W, band.top()),
            Vec2::new(GAUGE_W, BAND_H),
        ),
        current,
        max,
        38.0,
        &crate::gauge::MAIN,
    );

    ui.add_space(28.0);

    ui.label(theme::h2("最近请求"));
    ui.add_space(12.0);

    if app.recent_usage.is_empty() {
        ui.label(theme::meta("还没有请求记录"));
        return;
    }

    let avail = (ui.available_width() - 80.0).max(760.0);
    let ratios: [f32; 10] = [0.08, 0.11, 0.22, 0.13, 0.08, 0.08, 0.08, 0.09, 0.08, 0.05];
    let widths: Vec<f32> = ratios.iter().map(|r| (avail * r).floor()).collect();
    let headers = [
        "时间", "AGENT", "模型", "供应商", "净输入", "输出", "TPS", "费用", "首字/总耗", "状态",
    ];

    egui::ScrollArea::vertical()
        .max_height(360.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, (h, w)) in headers.iter().zip(widths.iter()).enumerate() {
                    let numeric = matches!(i, 4 | 5 | 6 | 7 | 8);
                    let text = RichText::new(*h)
                        .family(FontFamily::Monospace)
                        .size(10.0)
                        .color(theme::FAINT);
                    if numeric {
                        cell_right(ui, *w, text);
                    } else {
                        cell(ui, *w, text);
                    }
                }
            });
            ui.add_space(6.0);
            let rect = ui.available_rect_before_wrap();
            ui.painter().hline(
                rect.left()..=rect.right(),
                rect.top(),
                Stroke::new(1.0, theme::HAIRLINE),
            );
            ui.add_space(6.0);

            for r in app.recent_usage.iter().take(25) {
                let row_rect = ui.available_rect_before_wrap();
                if ui.rect_contains_pointer(row_rect) {
                    ui.painter().rect_filled(
                        row_rect,
                        0.0,
                        egui::Color32::from_rgba_premultiplied(15, 44, 63, 12),
                    );
                }
                ui.horizontal(|ui| {
                    cell(ui, widths[0], theme::mono_dim(r.ts.format("%H:%M:%S").to_string()));
                    cell(
                        ui,
                        widths[1],
                        theme::mono_accent(r.agent_id.as_deref().unwrap_or("global")),
                    );
                    cell(ui, widths[2], theme::mono(&r.model));
                    cell(ui, widths[3], theme::mono_dim(&r.provider_name));
                    cell_right(ui, widths[4], theme::mono(r.input_tokens.to_string()));
                    cell_right(ui, widths[5], theme::mono(r.output_tokens.to_string()));
                    cell_right(
                        ui,
                        widths[6],
                        theme::mono_accent(
                            r.tps.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "-".into()),
                        ),
                    );
                    cell_right(
                        ui,
                        widths[7],
                        theme::mono_dim(
                            r.cost_usd
                                .map(|v| theme::format_cost_with_rate(v, usd_cny))
                                .unwrap_or_else(|| "-".into()),
                        ),
                    );
                    let timing_str = match r.ttft_ms {
                        Some(ttft) => format!("{}/{}", format_ms(ttft), format_ms(r.duration_ms)),
                        None => format_ms(r.duration_ms),
                    };
                    cell_right(ui, widths[8], theme::mono_dim(timing_str));
                    let color = match r.status {
                        clowder_core::Status::Ok => theme::POSITIVE,
                        clowder_core::Status::Error => theme::NEGATIVE,
                        clowder_core::Status::Aborted => theme::DIM,
                    };
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(widths[9], 18.0), Sense::hover());
                    ui.painter().circle_filled(
                        Pos2::new(rect.left() + 6.0, rect.center().y),
                        3.5,
                        color,
                    );
                });
                ui.add_space(6.0);
            }
        });
}

fn stat(ui: &mut Ui, label: &str, value: String, unit: &str) {
    ui.vertical(|ui| {
        ui.label(theme::meta(label));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(value)
                        .family(FontFamily::Monospace)
                        .size(28.0)
                        .color(theme::TEXT)
                        .strong(),
                )
                .wrap_mode(egui::TextWrapMode::Extend),
            );
            if !unit.is_empty() {
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(unit)
                            .family(FontFamily::Monospace)
                            .size(10.0)
                            .color(theme::FAINT),
                    )
                    .wrap_mode(egui::TextWrapMode::Extend),
                );
            }
        });
    });
}

fn format_ms(ms: u64) -> String {
    if ms >= 10_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else {
        format!("{}ms", ms)
    }
}

fn cell(ui: &mut Ui, width: f32, text: RichText) {
    ui.allocate_ui(Vec2::new(width, 18.0), |ui| {
        ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Truncate));
    });
}

fn cell_right(ui: &mut Ui, width: f32, text: RichText) {
    ui.allocate_ui(Vec2::new(width, 18.0), |ui| {
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Truncate));
        });
    });
}
