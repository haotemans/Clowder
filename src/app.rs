use eframe::egui;
use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use clowder_core::{AgentId, AgentRouteMode, Store, UsageRecord, UsageSummary};
use clowder_proxy::AppState;

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

pub struct ClowderApp {
    pub state: AppState,
    pub usage_rx: broadcast::Receiver<UsageRecord>,
    pub store: Store,
    pub page: Page,
    pub recent_usage: Vec<UsageRecord>,
    pub realtime_series: Vec<(f64, f64, f64)>,
    pub daily: Vec<clowder_core::DayUsage>,
    pub last_summary: Option<UsageSummary>,
    pub status_message: Option<String>,
    pub edit_state: crate::pages::providers::EditState,
    pub ui_mode: UiMode,
    pub mode_applied: Option<UiMode>,
    /// 右上角 1/4 圆 Agent 扇形切换器是否展开
    pub corner_menu_open: bool,
    /// 会话页按 Agent 过滤 (None 表示显示全部)
    pub session_agent_filter: Option<AgentId>,
    /// 供应商非阻塞测速结果缓存 (provider_id -> "128ms" / "超时")
    pub provider_pings: Arc<Mutex<HashMap<String, String>>>,
}

impl ClowderApp {
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
            daily: Vec::new(),
            last_summary: None,
            status_message: None,
            edit_state: Default::default(),
            ui_mode: if start_full { UiMode::Full } else { UiMode::Mini },
            mode_applied: None,
            corner_menu_open: false,
            session_agent_filter: None,
            provider_pings: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn init(cc: &eframe::CreationContext<'_>, mut app: Self) -> Self {
        install_cjk_font(&cc.egui_ctx);
        crate::theme::apply(&cc.egui_ctx);
        {
            let ctx = cc.egui_ctx.clone();
            let mut rx = app.state.usage_tx.subscribe();
            std::thread::spawn(move || {
                while rx.blocking_recv().is_ok() {
                    ctx.request_repaint();
                }
            });
        }
        if let Ok(recs) = app.store.recent_usage(200) {
            for r in recs.into_iter().rev() {
                app.push_realtime(&r);
            }
            app.recent_usage = app.store.recent_usage(50).unwrap_or_default();
        }
        app.refresh_summary();
        app
    }

    pub fn refresh_summary(&mut self) {
        let now = chrono::Utc::now();
        let day_start = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        let current_agent_slug = self
            .state
            .config
            .try_read()
            .ok()
            .and_then(|c| c.current_agent.map(|a| a.as_str().to_string()));
        if let Ok(s) = self
            .store
            .summary_since_for_agent(day_start, current_agent_slug.as_deref())
        {
            self.last_summary = Some(s);
        }
        let since = now - chrono::Duration::days(53 * 7);
        if let Ok(d) = self.store.daily_usage(since.timestamp_millis(), (53 * 7) as usize) {
            self.daily = d;
        }
    }

    fn push_realtime(&mut self, r: &UsageRecord) {
        let t = r.ts.timestamp_millis() as f64 / 1000.0;
        let tps = r.tps.unwrap_or(0.0);
        let tokens = (r.input_tokens + r.output_tokens) as f64;
        self.realtime_series.push((t, tps, tokens));
        let cutoff = t - 3600.0;
        self.realtime_series.retain(|(ts, _, _)| *ts > cutoff);
        if self.realtime_series.len() > 1000 {
            let n = self.realtime_series.len() - 1000;
            self.realtime_series.drain(0..n);
        }
    }

    pub fn poll_usage(&mut self) {
        let mut received = false;
        while let Ok(r) = self.usage_rx.try_recv() {
            self.push_realtime(&r);
            if r.status == clowder_core::Status::Ok {
                let current_agent_slug = self
                    .state
                    .config
                    .try_read()
                    .ok()
                    .and_then(|c| c.current_agent.map(|a| a.as_str().to_string()));
                let matches_filter = match (&current_agent_slug, &r.agent_id) {
                    (Some(filter), Some(actual)) => filter == actual,
                    (Some(_), None) => false,
                    (None, _) => true,
                };
                if matches_filter {
                    let s = self.last_summary.get_or_insert_with(UsageSummary::default);
                    s.input_tokens += r.input_tokens;
                    s.output_tokens += r.output_tokens;
                    s.cache_read_tokens += r.cache_read_tokens;
                    s.cache_write_tokens += r.cache_write_tokens;
                    s.reasoning_tokens += r.reasoning_tokens;
                    s.cost_usd += r.cost_usd.unwrap_or(0.0);
                    s.request_count += 1;
                    if let Some(tps) = r.tps {
                        s.avg_tps = Some(match s.avg_tps {
                            Some(prev) => (prev * (s.request_count.saturating_sub(1) as f64) + tps)
                                / (s.request_count as f64),
                            None => tps,
                        });
                    }
                }
            }
            self.recent_usage.insert(0, r);
            if self.recent_usage.len() > 100 {
                self.recent_usage.truncate(100);
            }
            received = true;
        }
        if received && self.daily.is_empty() {
            self.refresh_summary();
        }
    }
}

/// 绘制右上角 1/4 圆常驻触发器与双层防裁切扇形切换面板 (带平滑过渡动画与放射刻度线)
fn draw_top_right_quarter_switcher(ctx: &egui::Context, app: &mut ClowderApp) {
    let (show_switcher, current_agent, enabled_agents, global_prov_name) = {
        let Ok(cfg) = app.state.config.try_read() else {
            return;
        };
        if !cfg.ui.show_corner_switcher {
            return;
        }
        let enabled: Vec<(AgentId, AgentRouteMode, String)> = cfg
            .agents
            .iter()
            .filter(|a| a.enabled_in_switcher)
            .map(|a| {
                let prov_name = cfg
                    .resolve_provider_for_agent(Some(a.agent), Some(a.agent.preferred_protocol()))
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "未配供应商".to_string());
                (a.agent, a.route_mode, prov_name)
            })
            .collect();
        let g_name = cfg
            .active_provider()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "未设置".to_string());
        (true, cfg.current_agent, enabled, g_name)
    };
    if !show_switcher {
        return;
    }

    let screen = ctx.screen_rect();
    let corner = egui::Pos2::new(screen.right(), screen.top());
    let trigger_r = 48.0_f32;

    let anim_t = ctx.animate_bool_responsive(
        egui::Id::new("top_right_quarter_fan_anim"),
        app.corner_menu_open,
    );

    // 1. 常驻右上角 1/4 圆按钮
    egui::Area::new(egui::Id::new("top_right_quarter_trigger"))
        .fixed_pos(egui::Pos2::new(corner.x - trigger_r, corner.y))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(trigger_r, trigger_r), egui::Sense::click());
            let hovered = resp.hovered();
            let fill = if app.corner_menu_open || hovered {
                crate::theme::ACCENT
            } else {
                crate::theme::NEEDLE
            };
            let origin = egui::Pos2::new(rect.right(), rect.top());
            let mut pts = Vec::with_capacity(28);
            pts.push(origin);
            let steps = 24;
            for i in 0..=steps {
                let theta = (PI * 0.5) + (PI * 0.5) * (i as f32 / steps as f32);
                let x = origin.x + theta.cos() * trigger_r;
                let y = origin.y + theta.sin() * trigger_r;
                pts.push(egui::Pos2::new(x, y));
            }
            ui.painter().add(egui::Shape::convex_polygon(
                pts,
                fill,
                egui::Stroke::new(1.5, crate::theme::INVERT),
            ));

            let badge_text = current_agent.map(|a| a.badge()).unwrap_or("ALL");
            let label_pos =
                egui::Pos2::new(origin.x - trigger_r * 0.42, origin.y + trigger_r * 0.40);
            ui.painter().text(
                label_pos,
                egui::Align2::CENTER_CENTER,
                badge_text,
                egui::FontId::monospace(12.0),
                crate::theme::INVERT,
            );

            let tip = match current_agent {
                Some(a) => format!("当前 Agent: {} (点击展开/收起 1/4 圆切换器)", a.label()),
                None => format!("当前: 总控模式 [{}] (点击展开/收起 1/4 圆切换器)", global_prov_name),
            };
            if resp.on_hover_text(tip).clicked() {
                app.corner_menu_open = !app.corner_menu_open;
            }
        });

    if anim_t <= 0.01 {
        return;
    }

    // 2. 展开态: 双层同心圆弧防重叠排布 + 放射刻度线 + 边界安全夹紧
    let max_fan_r = 286.0_f32;
    let fan_r = trigger_r + (max_fan_r - trigger_r) * anim_t;

    egui::Area::new(egui::Id::new("top_right_quarter_fan_popup"))
        .fixed_pos(egui::Pos2::new(corner.x - fan_r - 12.0, corner.y))
        .order(egui::Order::Tooltip)
        .show(ctx, |ui| {
            let (rect, bg_resp) = ui.allocate_exact_size(
                egui::vec2(fan_r + 12.0, fan_r + 12.0),
                egui::Sense::click(),
            );
            let origin = egui::Pos2::new(rect.right(), rect.top());

            // 扇形主底板
            let mut fan_pts = Vec::with_capacity(42);
            fan_pts.push(origin);
            let steps = 36;
            for i in 0..=steps {
                let theta = (PI * 0.5) + (PI * 0.5) * (i as f32 / steps as f32);
                let x = origin.x + theta.cos() * fan_r;
                let y = origin.y + theta.sin() * fan_r;
                fan_pts.push(egui::Pos2::new(x, y));
            }
            ui.painter().add(egui::Shape::convex_polygon(
                fan_pts,
                egui::Color32::from_rgba_premultiplied(245, 248, 251, 248),
                egui::Stroke::new(1.5, crate::theme::ACCENT),
            ));

            if anim_t < 0.65 {
                return;
            }

            // 放射角度虚线与两条同心轨道弧线 (r = 134, r = 222)
            for ray_i in 1..4 {
                let ray_theta = (PI * 0.5) + (PI * 0.5) * (ray_i as f32 / 4.0);
                let p1 = egui::Pos2::new(
                    origin.x + ray_theta.cos() * (trigger_r + 8.0),
                    origin.y + ray_theta.sin() * (trigger_r + 8.0),
                );
                let p2 = egui::Pos2::new(
                    origin.x + ray_theta.cos() * (fan_r - 8.0),
                    origin.y + ray_theta.sin() * (fan_r - 8.0),
                );
                ui.painter()
                    .line_segment([p1, p2], egui::Stroke::new(0.8, crate::theme::HAIRLINE));
            }
            for r_ring in [134.0_f32 * anim_t, 222.0_f32 * anim_t] {
                let arc_pts: Vec<egui::Pos2> = (0..=28)
                    .map(|i| {
                        let theta = (PI * 0.5) + (PI * 0.5) * (i as f32 / 28.0);
                        egui::Pos2::new(
                            origin.x + theta.cos() * r_ring,
                            origin.y + theta.sin() * r_ring,
                        )
                    })
                    .collect();
                ui.painter().add(egui::Shape::line(
                    arc_pts,
                    egui::Stroke::new(1.0, crate::theme::HAIRLINE),
                ));
            }

            let mut next_selection: Option<Option<AgentId>> = None;
            let mut jump_to_config = false;

            // 内核总控节点 + 跳转配置快捷钮
            {
                let theta_all = PI * 0.75;
                let center_all = egui::Pos2::new(
                    origin.x + theta_all.cos() * 78.0,
                    origin.y + theta_all.sin() * 78.0,
                );
                let btn_rect = egui::Rect::from_center_size(center_all, egui::vec2(68.0, 26.0));
                let resp = ui.interact(
                    btn_rect,
                    ui.id().with("fan_all_global"),
                    egui::Sense::click(),
                );
                let is_sel = current_agent.is_none();
                let bg = if is_sel {
                    crate::theme::ACCENT
                } else if resp.hovered() {
                    crate::theme::ACCENT_SOFT
                } else {
                    crate::theme::CARD
                };
                let fg = if is_sel {
                    crate::theme::INVERT
                } else {
                    crate::theme::TEXT
                };
                ui.painter().rect(
                    btn_rect,
                    egui::CornerRadius::same(13),
                    bg,
                    egui::Stroke::new(1.2, crate::theme::ACCENT),
                    egui::StrokeKind::Outside,
                );
                ui.painter().text(
                    btn_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "ALL 总控",
                    egui::FontId::monospace(10.5),
                    fg,
                );
                if resp
                    .on_hover_text(format!(
                        "切换到总控全局视角 (默认供应商: {})\n再次点击进入 Agent 与供应商配置页",
                        global_prov_name
                    ))
                    .clicked()
                {
                    if current_agent.is_none() {
                        jump_to_config = true;
                    }
                    next_selection = Some(None);
                }
            }

            // 双层同心弧排布可用 Agent
            let total = enabled_agents.len();
            let inner_count = if total > 4 { 3 } else { total.min(2) };
            for (idx, (agent, route_mode, prov_name)) in enabled_agents.iter().enumerate() {
                let (radius, theta) = if idx < inner_count {
                    let t = if inner_count <= 1 {
                        0.5
                    } else {
                        idx as f32 / (inner_count as f32 - 1.0)
                    };
                    (138.0_f32, PI * (0.62 + 0.26 * t))
                } else {
                    let outer_idx = idx - inner_count;
                    let outer_total = (total - inner_count).max(1);
                    let t = if outer_total <= 1 {
                        0.5
                    } else {
                        outer_idx as f32 / (outer_total as f32 - 1.0)
                    };
                    (222.0_f32, PI * (0.58 + 0.34 * t))
                };

                let pill_size = egui::vec2(66.0, 26.0);
                let raw_x = origin.x + theta.cos() * radius;
                let raw_y = origin.y + theta.sin() * radius;
                let clamped_x = raw_x.min(origin.x - pill_size.x * 0.5 - 8.0);
                let clamped_y = raw_y.max(origin.y + pill_size.y * 0.5 + 8.0);
                let pill_rect =
                    egui::Rect::from_center_size(egui::Pos2::new(clamped_x, clamped_y), pill_size);

                let resp = ui.interact(
                    pill_rect,
                    ui.id().with(("fan_agent", agent.as_str())),
                    egui::Sense::click(),
                );
                let is_sel = current_agent == Some(*agent);
                let bg = if is_sel {
                    crate::theme::ACCENT
                } else if resp.hovered() {
                    crate::theme::ACCENT_SOFT
                } else {
                    crate::theme::CARD
                };
                let stroke_col = if *route_mode == AgentRouteMode::Dedicated {
                    crate::theme::POSITIVE
                } else {
                    crate::theme::HAIRLINE
                };
                let fg = if is_sel {
                    crate::theme::INVERT
                } else {
                    crate::theme::TEXT
                };
                ui.painter().rect(
                    pill_rect,
                    egui::CornerRadius::same(13),
                    bg,
                    egui::Stroke::new(1.2, stroke_col),
                    egui::StrokeKind::Outside,
                );
                ui.painter().text(
                    pill_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("{} {}", agent.badge(), short_agent_tag(*agent)),
                    egui::FontId::monospace(10.5),
                    fg,
                );
                let tip = format!(
                    "{}\n路由策略: {}\n当前供应商: {}\n单击切换该 Agent (右键或再次点击已选中项直达配置页)",
                    agent.label(),
                    route_mode,
                    prov_name
                );
                let resp = resp.on_hover_text(tip);
                if resp. secondary_clicked() {
                    jump_to_config = true;
                    next_selection = Some(Some(*agent));
                } else if resp.clicked() {
                    if is_sel {
                        jump_to_config = true;
                    }
                    next_selection = Some(Some(*agent));
                }
            }

            // 右上角顶点收起区
            let close_corner = egui::Rect::from_min_size(
                egui::Pos2::new(origin.x - trigger_r, origin.y),
                egui::vec2(trigger_r, trigger_r),
            );
            let close_resp = ui.interact(
                close_corner,
                ui.id().with("fan_close_corner"),
                egui::Sense::click(),
            );
            ui.painter().text(
                egui::Pos2::new(origin.x - trigger_r * 0.42, origin.y + trigger_r * 0.40),
                egui::Align2::CENTER_CENTER,
                "×",
                egui::FontId::monospace(16.0),
                crate::theme::ACCENT,
            );
            if close_resp.clicked() {
                app.corner_menu_open = false;
            } else if let Some(chosen) = next_selection {
                if let Ok(mut cfg) = app.state.config.try_write() {
                    cfg.current_agent = chosen;
                    let _ = cfg.save();
                }
                app.session_agent_filter = chosen;
                app.refresh_summary();
                if jump_to_config || app.page == Page::Settings {
                    app.page = Page::Providers;
                }
                app.corner_menu_open = false;
            } else if bg_resp.clicked() {
                app.corner_menu_open = false;
            }
        });
}

fn short_agent_tag(agent: AgentId) -> &'static str {
    match agent {
        AgentId::ClaudeCode => "Code",
        AgentId::ClaudeDesktop => "CDesk",
        AgentId::Codex => "Codex",
        AgentId::CodexDesktop => "XDesk",
        AgentId::OpenCodeV2 => "OpenC",
        AgentId::Pi => "Pi",
        AgentId::Dsh => "DSH",
        AgentId::DshDesktop => "DDesk",
    }
}

fn draw_mini_mode_icon(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
    let stroke_col = if resp.hovered() {
        crate::theme::ACCENT
    } else {
        crate::theme::DIM
    };
    let outer = rect.shrink(4.0);
    ui.painter().rect_stroke(
        outer,
        1.0,
        egui::Stroke::new(1.2, stroke_col),
        egui::StrokeKind::Inside,
    );
    let inner = egui::Rect::from_min_max(
        egui::Pos2::new(outer.center().x - 1.0, outer.center().y - 1.0),
        outer.max,
    );
    ui.painter().rect_filled(inner, 0.0, stroke_col);
    resp
}

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
            for up in 0..3 {
                let base = (0..up).fold(dir.to_path_buf(), |p, _| {
                    p.parent().map(|q| q.to_path_buf()).unwrap_or(p)
                });
                dirs.push(base.join("assets").join("fonts"));
            }
            dirs.push(dir.join("fonts"));
        }
    }
    dirs.push(std::path::Path::new("assets").join("fonts"));
    if let Ok(data) = clowder_core::data_dir() {
        dirs.push(data.join("fonts"));
    }
    dirs
}

fn insert_font(fonts: &mut egui::FontDefinitions, name: &str, bytes: Vec<u8>) {
    fonts.font_data.insert(
        name.to_string(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
}

fn install_cjk_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    if let Some(bytes) = find_font(
        &["clowder-cjk-sans.ttf"],
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

    let mut serif_family: Vec<String> = Vec::new();
    if let Some(bytes) = find_font(
        &["clowder-serif-latin.ttf"],
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
        &["clowder-cjk-serif.ttf"],
        &[r"C:\Windows\Fonts\simsun.ttc", r"C:\Windows\Fonts\simsunb.ttf"],
    ) {
        insert_font(&mut fonts, "serif-cjk", bytes);
        serif_family.push("serif-cjk".to_string());
    }
    if fonts.font_data.contains_key("cjk") {
        serif_family.push("cjk".to_string());
    }
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

impl eframe::App for ClowderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.ui_mode == UiMode::Full {
            crate::theme::assert_visuals(ctx);
        }
        self.poll_usage();
        if self.mode_applied != Some(self.ui_mode) {
            match self.ui_mode {
                UiMode::Full => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1280.0, 820.0)));
                    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(960.0, 620.0)));
                    ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(egui::vec2(2400.0, 1600.0)));
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                        egui::WindowLevel::Normal,
                    ));
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
            ctx.send_viewport_cmd(egui::ViewportCommand::Title("Clowder".to_string()));
            self.mode_applied = Some(self.ui_mode);
        }

        if self.ui_mode == UiMode::Mini {
            crate::mini::show(ctx, self);
            ctx.request_repaint_after(HEARTBEAT);
            return;
        }

        let (show_dash, show_prov, show_sess, current_agent_label) = {
            if let Ok(cfg) = self.state.config.try_read() {
                let ag_label = cfg
                    .current_agent
                    .map(|a| format!("Agent: {}", a.label()))
                    .unwrap_or_else(|| "视角: 总控 Global".to_string());
                (
                    cfg.ui.show_dashboard_tab,
                    cfg.ui.show_providers_tab,
                    cfg.ui.show_sessions_tab,
                    ag_label,
                )
            } else {
                (true, true, true, "视角: 总控 Global".to_string())
            }
        };

        // top bar
        egui::TopBottomPanel::top("top")
            .exact_height(60.0)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(3.0, 22.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 0.0, crate::theme::ACCENT);
                    ui.add_space(14.0);
                    ui.label(
                        egui::RichText::new("Clowder")
                            .family(egui::FontFamily::Monospace)
                            .size(16.0)
                            .color(crate::theme::TEXT)
                            .strong(),
                    );
                    ui.add_space(36.0);

                    let mut nav_items = Vec::new();
                    if show_dash {
                        nav_items.push((Page::Dashboard, "仪表盘"));
                    }
                    if show_prov {
                        nav_items.push((Page::Providers, "Agent 与供应商"));
                    }
                    if show_sess {
                        nav_items.push((Page::Sessions, "统一会话"));
                    }
                    nav_items.push((Page::Settings, "设置"));

                    for (page, label) in nav_items {
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
                        ui.add_space(26.0);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(60.0);
                        if draw_mini_mode_icon(ui)
                            .on_hover_text("最小化到桌面玻璃小窗")
                            .clicked()
                        {
                            self.ui_mode = UiMode::Mini;
                        }
                        ui.add_space(12.0);
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                        ui.painter()
                            .circle_filled(rect.center(), 3.0, crate::theme::ACCENT);
                        ui.add_space(8.0);
                        let text = if let Ok(cfg) = self.state.config.try_read() {
                            format!("http://{}", cfg.proxy.listen)
                        } else {
                            "...".to_string()
                        };
                        ui.label(
                            egui::RichText::new(text)
                                .family(egui::FontFamily::Monospace)
                                .size(12.0)
                                .color(crate::theme::DIM),
                        );
                        ui.add_space(16.0);
                        ui.label(
                            egui::RichText::new(current_agent_label)
                                .family(egui::FontFamily::Monospace)
                                .size(12.0)
                                .color(crate::theme::ACCENT),
                        );
                    });
                });
            });

        let avail_w = ctx.screen_rect().width();
        let h_margin = if avail_w < 1100.0 { 24 } else { 40 };

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::BG)
                    .inner_margin(egui::Margin::symmetric(h_margin, 24)),
            )
            .show(ctx, |ui| match self.page {
                Page::Dashboard => dashboard::show(ui, self),
                Page::Providers => providers::show(ui, self),
                Page::Sessions => sessions::show(ui, self),
                Page::Settings => settings::show(ui, self),
            });

        draw_top_right_quarter_switcher(ctx, self);

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
