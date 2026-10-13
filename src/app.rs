use eframe::egui;
use tokio::sync::broadcast;
use yts_core::{Store, UsageRecord};
use yts_proxy::AppState;

use crate::pages::{dashboard, providers, sessions, settings};

/// 事件驱动重绘之外的兜底心跳
const HEARTBEAT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Providers,
    Sessions,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    Full,
    Mini,
}

pub struct YtsApp {
    pub state: AppState,
    pub usage_rx: broadcast::Receiver<UsageRecord>,
    pub store: Store,
    pub page: Page,
    pub recent_usage: Vec<UsageRecord>,
    pub realtime_series: Vec<(f64, f64, f64)>,
    pub last_summary: Option<yts_core::UsageSummary>,
    pub status_message: Option<String>,
    pub edit_state: crate::pages::providers::EditState,
    pub ui_mode: UiMode,
    pub mode_applied: Option<UiMode>,
}

impl YtsApp {
    pub fn new(
        state: AppState,
        usage_rx: broadcast::Receiver<UsageRecord>,
        store: Store,
        start_full: bool,
    ) -> Self {
        Self {
            state,
            usage_rx,
            store,
            page: Page::Dashboard,
            recent_usage: Vec::new(),
            realtime_series: Vec::new(),
            last_summary: None,
            status_message: None,
            edit_state: Default::default(),
            ui_mode: if start_full { UiMode::Full } else { UiMode::Mini },
            mode_applied: None,
        }
    }

    pub fn init(cc: &eframe::CreationContext<'_>, mut app: Self) -> Self {
        install_cjk_font(&cc.egui_ctx);
        crate::theme::apply(&cc.egui_ctx);
        // usage 事件到达才重绘; 之前是固定 500ms 一次, 后台空转
        {
            let ctx = cc.egui_ctx.clone();
            let mut rx = app.state.usage_tx.subscribe();
            std::thread::spawn(move || {
                while rx.blocking_recv().is_ok() {
                    ctx.request_repaint();
                }
            });
        }
        // load recent history
        if let Ok(recs) = app.store.recent_usage(200) {
            for r in recs.into_iter().rev() {
                app.push_realtime(&r);
            }
            app.recent_usage = app.store.recent_usage(50).unwrap_or_default();
        }
        app.refresh_summary();
        app
    }

    fn refresh_summary(&mut self) {
        let now = chrono::Utc::now();
        let day_start = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        if let Ok(s) = self.store.summary_since(day_start) {
            self.last_summary = Some(s);
        }
    }

    fn push_realtime(&mut self, r: &UsageRecord) {
        let t = r.ts.timestamp_millis() as f64 / 1000.0;
        let tps = r.tps.unwrap_or(0.0);
        let tokens = (r.input_tokens + r.output_tokens) as f64;
        self.realtime_series.push((t, tps, tokens));
        // keep last 60 min
        let cutoff = t - 3600.0;
        self.realtime_series.retain(|(ts, _, _)| *ts > cutoff);
        if self.realtime_series.len() > 1000 {
            let n = self.realtime_series.len() - 1000;
            self.realtime_series.drain(0..n);
        }
    }

    pub fn poll_usage(&mut self) {
        while let Ok(r) = self.usage_rx.try_recv() {
            self.push_realtime(&r);
            self.recent_usage.insert(0, r);
            if self.recent_usage.len() > 100 {
                self.recent_usage.truncate(100);
            }
            self.refresh_summary();
        }
    }
}

/// 字体候选顺序: 本地子集 (python scripts/build-fonts.py 生成) > 系统全量字体.
/// 系统 msyh.ttc/simsun.ttc 各 ~19MB, egui 常驻内存约 3 倍; 子集 ~1.8MB 所以必须优先.
fn find_font(subset_names: &[&str], system_paths: &[&str]) -> Option<Vec<u8>> {
    for dir in font_dirs() {
        for name in subset_names {
            if let Ok(bytes) = std::fs::read(dir.join(name)) {
                return Some(bytes);
            }
        }
    }
    for path in system_paths {
        if let Ok(bytes) = std::fs::read(path) {
            return Some(bytes);
        }
    }
    None
}

fn font_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // target/release/../.. = 仓库根; 发布版则把 assets/fonts 放在 exe 旁边
            for up in 0..3 {
                let base = (0..up).fold(dir.to_path_buf(), |p, _| {
                    p.parent().map(|q| q.to_path_buf()).unwrap_or(p)
                });
                dirs.push(base.join("assets").join("fonts"));
            }
            dirs.push(dir.join("fonts"));
        }
    }
    dirs.push(std::path::Path::new("assets").join("fonts")); // cargo run 的工作目录
    if let Ok(data) = yts_core::data_dir() {
        dirs.push(data.join("fonts"));
    }
    dirs
}

fn insert_font(fonts: &mut egui::FontDefinitions, name: &str, bytes: Vec<u8>) {
    fonts
        .font_data
        .insert(name.to_string(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
}

fn install_cjk_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 正文/等宽的中文兜底
    if let Some(bytes) = find_font(
        &["yts-cjk-sans.ttf"],
        &[
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\Deng.ttf",
            r"C:\Windows\Fonts\simhei.ttf",
            r"C:\Windows\Fonts\simsun.ttc",
        ],
    ) {
        insert_font(&mut fonts, "cjk", bytes);
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("cjk".to_string());
        }
    }

    // serif-display: 拉丁用 Georgia, 中文用宋体
    let mut serif_family: Vec<String> = Vec::new();
    if let Some(bytes) = find_font(
        &["yts-serif-latin.ttf"],
        &[
            r"C:\Windows\Fonts\georgia.ttf",
            r"C:\Windows\Fonts\georgiab.ttf",
            r"C:\Windows\Fonts\times.ttf",
        ],
    ) {
        insert_font(&mut fonts, "serif-display", bytes);
        serif_family.push("serif-display".to_string());
    }
    if let Some(bytes) = find_font(
        &["yts-cjk-serif.ttf"],
        &[r"C:\Windows\Fonts\simsun.ttc", r"C:\Windows\Fonts\simsunb.ttf"],
    ) {
        insert_font(&mut fonts, "serif-cjk", bytes);
        serif_family.push("serif-cjk".to_string());
    }
    if fonts.font_data.contains_key("cjk") {
        serif_family.push("cjk".to_string());
    }
    // serif-display 不能空: epaint 遇到未绑定的 FontFamily::Name 会 panic, panic=abort 下进程直接消失
    if serif_family.is_empty() {
        serif_family = fonts
            .families
            .get(&egui::FontFamily::Proportional)
            .and_then(|f| f.first().cloned())
            .into_iter()
            .collect();
    }
    fonts.families.insert(
        egui::FontFamily::Name("serif-display".into()),
        serif_family,
    );
    ctx.set_fonts(fonts);
}

impl eframe::App for YtsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 主界面走冷白主题; mini 自己那套玻璃主题在切换时单独设
        if self.ui_mode == UiMode::Full {
            crate::theme::assert_visuals(ctx);
        }
        self.poll_usage();
        // apply pending UI mode switch
        if self.mode_applied != Some(self.ui_mode) {
            match self.ui_mode {
                UiMode::Full => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1280.0, 800.0)));
                    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(960.0, 600.0)));
                    ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(egui::vec2(2400.0, 1600.0)));
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                        egui::WindowLevel::Normal,
                    ));
                    // mini 会覆盖 visuals, 切回来要重新套一次
                    crate::theme::apply(ctx);
                }
                UiMode::Mini => {
                    crate::mini::apply_visuals(ctx);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(300.0, 340.0)));
                    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(280.0, 320.0)));
                    ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(egui::vec2(360.0, 420.0)));
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                        egui::WindowLevel::AlwaysOnTop,
                    ));
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(
                "yuantuanswitch".to_string(),
            ));
            self.mode_applied = Some(self.ui_mode);
        }

        if self.ui_mode == UiMode::Mini {
            crate::mini::show(ctx, self);
            ctx.request_repaint_after(HEARTBEAT);
            return;
        }

        // top bar
        egui::TopBottomPanel::top("top")
            .exact_height(60.0)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    // logo: 3px 天青竖条 + 字标
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(3.0, 22.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(rect, 0.0, crate::theme::ACCENT);
                    ui.add_space(14.0);
                    ui.label(
                        egui::RichText::new("yuantuanswitch")
                            .family(egui::FontFamily::Monospace)
                            .size(16.0)
                            .color(crate::theme::TEXT)
                            .strong(),
                    );
                    ui.add_space(48.0);
                    // nav: 文字链接, 激活项带天青下划线
                    for (page, label) in [
                        (Page::Dashboard, "仪表盘"),
                        (Page::Providers, "供应商"),
                        (Page::Sessions, "会话"),
                        (Page::Settings, "设置"),
                    ] {
                        let selected = self.page == page;
                        let text = egui::RichText::new(label)
                            .family(egui::FontFamily::Monospace)
                            .size(13.0)
                            .color(if selected {
                                crate::theme::TEXT
                            } else {
                                crate::theme::DIM
                            });
                        let resp = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                        if selected {
                            let rect = resp.rect;
                            let y = rect.bottom() + 4.0;
                            ui.painter().hline(
                                rect.left()..=rect.right(),
                                y,
                                egui::Stroke::new(2.0, crate::theme::ACCENT),
                            );
                        }
                        if resp.clicked() {
                            self.page = page;
                        }
                        ui.add_space(28.0);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(24.0);
                        // mini mode toggle
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("◱")
                                    .size(12.0)
                                    .color(crate::theme::DIM),
                            ).fill(egui::Color32::TRANSPARENT))
                            .on_hover_text("最小化到小窗")
                            .clicked()
                        {
                            self.ui_mode = UiMode::Mini;
                        }
                        ui.add_space(12.0);
                        // 在线指示: 天青圆点 + LISTENING + 地址
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(6.0, 6.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().circle_filled(
                            rect.center(),
                            3.0,
                            crate::theme::ACCENT,
                        );
                        ui.add_space(8.0);
                        let text = if let Ok(cfg) = self.state.config.try_read() {
                            format!("http://{}", cfg.proxy.listen)
                        } else {
                            "…".to_string()
                        };
                        ui.label(
                            egui::RichText::new(text)
                                .family(egui::FontFamily::Monospace)
                                .size(12.0)
                                .color(crate::theme::DIM),
                        );
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::BG)
                    .inner_margin(egui::Margin::symmetric(40, 32)),
            )
            .show(ctx, |ui| match self.page {
            Page::Dashboard => dashboard::show(ui, self),
            Page::Providers => providers::show(ui, self),
            Page::Sessions => sessions::show(ui, self),
            Page::Settings => settings::show(ui, self),
        });

        // 兜底心跳: usage 事件已经会主动触发重绘, 这里只防漏事件/跨天刷新
        ctx.request_repaint_after(HEARTBEAT);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Ok(mut cfg) = self.state.config.try_write() {
            if let Ok(providers) = self.state.providers.try_read() {
                cfg.providers = providers.clone();
                let _ = cfg.save();
            }
        }
    }
}
