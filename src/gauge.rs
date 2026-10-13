//! 上半圆表盘: 小窗与仪表盘共用一套画法
//! 0 在左端点、满量程在右端点, 中间只写实时 TPS 数字, 不加单位与量程文字

use crate::mini_theme as mt;
use crate::theme;
use egui::{Color32, FontFamily, FontId, Pos2, Stroke};
use std::f32::consts::PI;

pub struct Palette {
    pub track: Color32,
    pub tick: Color32,
    pub tick_major: Color32,
    pub arc: Color32,
    pub needle: Color32,
    pub hub: Color32,
    pub value: Color32,
}

/// 小窗那套冷白玻璃色
pub const MINI: Palette = Palette {
    track: mt::HAIRLINE,
    tick: mt::TEXT_FAINT,
    tick_major: mt::TEXT_MUTED,
    arc: mt::ACCENT_CYAN,
    needle: mt::NEEDLE,
    hub: Color32::WHITE,
    value: mt::ACCENT_CYAN,
};

/// 主界面那套 (天青加深, 白底撑得住)
pub const MAIN: Palette = Palette {
    track: theme::HAIRLINE,
    tick: theme::FAINT,
    tick_major: theme::DIM,
    arc: theme::ACCENT,
    needle: theme::NEEDLE,
    hub: Color32::WHITE,
    value: theme::ACCENT,
};

/// 实时 TPS 取最新一个请求的值; 超过 5s 没有新记录就开始线性回落, 15s 归零,
/// 免得表盘一直挂着几十分钟前那个数
pub fn live_tps(series: &[(f64, f64, f64)]) -> f64 {
    let Some((t, tps, _)) = series.last().copied() else {
        return 0.0;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(t);
    let age = (now - t).max(0.0);
    if age <= 5.0 {
        tps
    } else {
        tps * (1.0 - (age - 5.0) / 10.0).clamp(0.0, 1.0)
    }
}

/// 量程用的峰值: 近 60 个采样点的最大值
pub fn peak_tps(series: &[(f64, f64, f64)]) -> f64 {
    series
        .iter()
        .rev()
        .take(60)
        .map(|(_, tps, _)| *tps)
        .fold(0.0_f64, f64::max)
}

/// 默认 0-1000; 只有真的冲过 1000 才按 500 一档往上跳, 免得刻度一直抖
pub fn scale(peak: f64) -> f64 {
    if peak <= 1000.0 {
        1000.0
    } else {
        (peak * 1.1 / 500.0).ceil() * 500.0
    }
}

/// 主界面表盘用的自适应量程: 按峰值挑一档最小的整数刻度, 让 20-120 TPS 这种常见速度
/// 也能把指针推到表盘中段。阶梯取整而不是连续值, 免得量程每来一条记录就抖一下
pub fn auto_scale(peak: f64) -> f64 {
    const STEPS: [f64; 7] = [100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0];
    STEPS
        .into_iter()
        .find(|s| *s >= peak)
        .unwrap_or_else(|| (peak * 1.25 / 5_000.0).ceil() * 5_000.0)
}

pub fn draw(
    painter: &egui::Painter,
    rect: egui::Rect,
    current_tps: f64,
    max_tps: f64,
    value_size: f32,
    pal: &Palette,
) {
    // 半径受两处约束: 卡片宽度的一半, 以及给数字留出的高度
    let radius = (rect.width() / 2.0 - 10.0).max(20.0).min(rect.height() - value_size * 1.9);
    let center = Pos2::new(rect.center().x, rect.top() + radius + 10.0);
    let angle = |ratio: f64| PI - (ratio.clamp(0.0, 1.0) as f32) * PI;
    let at = |angle: f32, r: f32| Pos2::new(center.x + angle.cos() * r, center.y - angle.sin() * r);

    // 刻度: 11 档, 两端与正中加重
    for i in 0..=10 {
        let a = angle(i as f64 / 10.0);
        let stroke = if i % 5 == 0 {
            Stroke::new(1.5, pal.tick_major)
        } else {
            Stroke::new(1.0, pal.tick)
        };
        painter.line_segment([at(a, radius - 4.0), at(a, radius - 14.0)], stroke);
    }

    // 满量程轨道
    let track: Vec<Pos2> = (0..=64)
        .map(|i| at(angle(i as f64 / 64.0), radius))
        .collect();
    painter.add(egui::Shape::line(track, Stroke::new(1.5, pal.track)));

    // 实时值弧
    let ratio = (current_tps / max_tps).clamp(0.0, 1.0);
    let end_angle = angle(ratio);
    if ratio > 0.004 {
        let arc: Vec<Pos2> = (0..=64)
            .map(|i| {
                let t = i as f64 / 64.0;
                at(PI + (end_angle - PI) * t as f32, radius)
            })
            .collect();
        painter.add(egui::Shape::line(arc, Stroke::new(4.0, pal.arc)));
    }

    // 指针
    let tip = at(end_angle, radius - 20.0);
    painter.line_segment([center, tip], Stroke::new(2.0, pal.needle));
    painter.circle_filled(center, 4.0, pal.needle);
    painter.circle_stroke(center, 4.0, Stroke::new(1.0, pal.hub));

    painter.text(
        Pos2::new(center.x, center.y + value_size + 4.0),
        egui::Align2::CENTER_CENTER,
        format!("{:.1}", current_tps),
        FontId::new(value_size, FontFamily::Name("serif-display".into())),
        pal.value,
    );
}
