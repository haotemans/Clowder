use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, Style, TextStyle, Visuals};

// ---- 冷白主界面色板: 与 mini 的"桌面玻璃小表"同一族 ----
pub const BG: Color32 = Color32::from_rgb(0xED, 0xF1, 0xF5);
pub const PANEL: Color32 = Color32::from_rgb(0xF5, 0xF8, 0xFB);
pub const CARD: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const HAIRLINE: Color32 = Color32::from_rgb(0xD2, 0xDB, 0xE4);
pub const TEXT: Color32 = Color32::from_rgb(0x1F, 0x29, 0x33);
pub const DIM: Color32 = Color32::from_rgb(0x5C, 0x6B, 0x7A);
pub const FAINT: Color32 = Color32::from_rgb(0x7C, 0x8B, 0x99);
pub const ACCENT: Color32 = Color32::from_rgb(0x0E, 0x7F, 0xB8);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0xBA, 0xE6, 0xFD);
pub const POSITIVE: Color32 = Color32::from_rgb(0x0F, 0x8A, 0x5F);
pub const NEGATIVE: Color32 = Color32::from_rgb(0xC0, 0x39, 0x2B);
pub const INVERT: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const NEEDLE: Color32 = Color32::from_rgb(0x0F, 0x2C, 0x3F);

pub fn apply(ctx: &egui::Context) {
    let mut style: Style = (*ctx.style()).clone();

    style.text_styles = [
        (TextStyle::Heading, FontId::new(18.0, FontFamily::Monospace)),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        (TextStyle::Button, FontId::new(13.0, FontFamily::Monospace)),
        (TextStyle::Small, FontId::new(11.0, FontFamily::Monospace)),
    ]
    .into();

    let visuals = build_visuals();
    style.visuals = visuals.clone();
    ctx.set_style(style);
    ctx.set_visuals(visuals);
}

pub fn build_visuals() -> Visuals {
    let mut visuals = Visuals::light();
    visuals.dark_mode = false;
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = CARD;
    visuals.faint_bg_color = PANEL;
    visuals.code_bg_color = PANEL;
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT_SOFT;
    visuals.selection.stroke = Stroke::new(1.0, NEEDLE);

    let mut w = visuals.widgets.clone();
    w.noninteractive.corner_radius = CornerRadius::ZERO;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, HAIRLINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);

    w.inactive.corner_radius = CornerRadius::ZERO;
    w.inactive.bg_fill = CARD;
    w.inactive.bg_stroke = Stroke::new(1.0, HAIRLINE);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);

    w.hovered.corner_radius = CornerRadius::ZERO;
    w.hovered.bg_fill = CARD;
    w.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    w.hovered.fg_stroke = Stroke::new(1.0, ACCENT);

    w.active.corner_radius = CornerRadius::ZERO;
    w.active.bg_fill = ACCENT;
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.0, INVERT);

    w.open.corner_radius = CornerRadius::ZERO;
    w.open.bg_fill = CARD;
    w.open.bg_stroke = Stroke::new(1.0, HAIRLINE);
    w.open.fg_stroke = Stroke::new(1.0, TEXT);

    visuals.widgets = w;
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.window_stroke = Stroke::new(1.0, HAIRLINE);
    visuals.menu_corner_radius = CornerRadius::ZERO;
    visuals
}

pub fn assert_visuals(ctx: &egui::Context) {
    let current = ctx.style().visuals.clone();
    if current.panel_fill != BG {
        ctx.set_visuals(build_visuals());
    }
}

pub fn h2(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into())
        .family(FontFamily::Proportional)
        .size(20.0)
        .color(TEXT)
        .strong()
}

pub fn meta(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into())
        .family(FontFamily::Monospace)
        .size(11.0)
        .color(DIM)
}

pub fn mono(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into())
        .family(FontFamily::Monospace)
        .size(13.0)
        .color(TEXT)
}

pub fn mono_dim(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into())
        .family(FontFamily::Monospace)
        .size(13.0)
        .color(DIM)
}

pub fn mono_accent(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into())
        .family(FontFamily::Monospace)
        .size(13.0)
        .color(ACCENT)
}

pub fn format_short(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

pub fn format_cost(v: f64) -> String {
    format_cost_with_rate(v, None)
}

/// 当用户在「设置」中填写了 USD/CNY 汇率时，自动折算为人民币 (¥)，否则显示美元 ($)
pub fn format_cost_with_rate(usd: f64, usd_cny: Option<f64>) -> String {
    let (val, sym) = match usd_cny.filter(|r| *r > 0.0) {
        Some(rate) => (usd * rate, "¥"),
        None => (usd, "$"),
    };
    if val > 0.0 && val.abs() < 0.01 {
        format!("{}{:.4}", sym, val)
    } else {
        format!("{}{:.2}", sym, val)
    }
}

pub fn hairline(ui: &mut egui::Ui) {
    ui.add_space(8.0);
    let rect = ui.available_rect_before_wrap();
    let y = rect.top();
    ui.painter().hline(
        rect.left()..=rect.right(),
        y,
        Stroke::new(1.0, HAIRLINE),
    );
    ui.add_space(8.0);
}

pub fn section_gap(ui: &mut egui::Ui) {
    ui.add_space(40.0);
}
