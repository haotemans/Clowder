use crate::theme;
use egui::Ui;
use egui_plot::{Legend, Line, Plot, PlotPoints};

pub fn realtime_chart(ui: &mut Ui, series: &[(f64, f64, f64)]) {
    if series.len() < 2 {
        // skeleton state: faint grid + baseline + meta text
        let height = 260.0;
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::hover(),
        );
        let painter = ui.painter();
        // grid
        for i in 0..=4 {
            let y = rect.top() + height * i as f32 / 4.0;
            painter.hline(
                rect.left()..=rect.right(),
                y,
                egui::Stroke::new(1.0, theme::HAIRLINE),
            );
        }
        for i in 0..=8 {
            let x = rect.left() + rect.width() * i as f32 / 8.0;
            painter.vline(
                x,
                rect.top()..=rect.bottom(),
                egui::Stroke::new(1.0, theme::HAIRLINE),
            );
        }
        // baseline
        painter.hline(
            rect.left()..=rect.right(),
            rect.bottom(),
            egui::Stroke::new(2.0, theme::DIM),
        );
        // message
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "等待请求",
            egui::FontId::monospace(13.0),
            theme::FAINT,
        );
        return;
    }

    let tps_points: PlotPoints = series.iter().map(|(t, tps, _)| [*t, *tps]).collect();
    let token_points: PlotPoints = series.iter().map(|(t, _, tok)| [*t, *tok]).collect();

    Plot::new("realtime")
        .legend(Legend::default())
        .show_grid(true)
        .show(ui, |plot_ui| {
            plot_ui.line(
                Line::new(tps_points)
                    .name("TPS")
                    .color(theme::ACCENT)
                    .width(2.0),
            );
            plot_ui.line(
                Line::new(token_points)
                    .name("Tokens")
                    .color(theme::DIM)
                    .width(1.0),
            );
        });
}
