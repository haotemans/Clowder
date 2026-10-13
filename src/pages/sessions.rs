use crate::app::ClowderApp;
use crate::theme;
use egui::{FontFamily, RichText, Ui};

pub fn show(ui: &mut Ui, app: &mut ClowderApp) {
    ui.horizontal(|ui| {
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

    theme::section_gap(ui);

    match app.store.sessions(100) {
        Ok(sessions) => {
            if sessions.is_empty() {
                ui.label(theme::meta("还没有会话"));
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                // header
                ui.horizontal(|ui| {
                    for h in ["SESSION ID", "STARTED", "LAST", "REQS", "IN", "OUT", "COST"] {
                        ui.label(
                            RichText::new(h)
                                .family(FontFamily::Monospace)
                                .size(10.0)
                                .color(theme::FAINT),
                        );
                        ui.add_space(28.0);
                    }
                });
                theme::hairline(ui);

                for s in sessions {
                    ui.horizontal(|ui| {
                        ui.label(theme::mono_dim(&s.session_id));
                        ui.add_space(28.0);
                        ui.label(theme::mono_dim(
                            s.started_at.format("%Y-%m-%d %H:%M").to_string(),
                        ));
                        ui.add_space(28.0);
                        ui.label(theme::mono_dim(s.last_activity.format("%H:%M:%S").to_string()));
                        ui.add_space(28.0);
                        ui.label(theme::mono(s.request_count.to_string()));
                        ui.add_space(28.0);
                        ui.label(theme::mono(s.total_input.to_string()));
                        ui.add_space(28.0);
                        ui.label(theme::mono(s.total_output.to_string()));
                        ui.add_space(28.0);
                        ui.label(theme::mono_dim(format!("${:.6}", s.total_cost)));
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
