//! 仪表盘的两个自绘部件: 周期表 (每天一格) 与实时表盘的配色/绘制都在 gauge.rs
//! 折线图撤掉了: TPS 与 Tokens 量级差两个数量级, 共用一条 y 轴时两条曲线都读不出来

use crate::theme;
use chrono::{Datelike, NaiveDate};
use clowder_core::DayUsage;
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};

/// 强度分档: 冷白 -> 天青, 与主界面同一系
const RAMP: [Color32; 5] = [
    Color32::from_rgb(0xD7, 0xE9, 0xF5),
    Color32::from_rgb(0xA8, 0xD3, 0xEA),
    Color32::from_rgb(0x5F, 0xB4, 0xDA),
    Color32::from_rgb(0x2A, 0x95, 0xC8),
    Color32::from_rgb(0x0E, 0x7F, 0xB8),
];
const EMPTY: Color32 = Color32::from_rgb(0xE3, 0xE9, 0xF0);
const GAP: f32 = 3.0;
const LABEL_H: f32 = 14.0;
const MIN_WEEKS: usize = 13;

/// 每天一格的周期表: 列是周、行是周一到周日, 颜色深浅看当日 token 总量。
/// 回看跨度按最早那条记录决定 (至少 13 周, 上限 max_weeks), 新装机不会铺一屏空格子;
/// 列数定了之后再把格子放大填满可用宽度
pub fn calendar_wall(ui: &mut Ui, days: &[DayUsage], base_cell: f32, max_weeks: usize) {
    let width = ui.available_width();
    let today = chrono::Utc::now().date_naive();
    let needed = days
        .first()
        .and_then(|d| NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").ok())
        .map(|earliest| (today - earliest).num_days().max(0) as usize / 7 + 3)
        .unwrap_or(MIN_WEEKS);
    let fit = (((width + GAP) / (base_cell + GAP)) as usize).max(MIN_WEEKS);
    let weeks = needed
        .clamp(MIN_WEEKS, max_weeks.max(MIN_WEEKS))
        .min(fit.max(MIN_WEEKS));
    let cell = ((width - GAP * (weeks as f32 - 1.0)) / weeks as f32).clamp(6.0, 20.0);
    let grid_h = cell * 7.0 + GAP * 6.0;
    let (block, _) = ui.allocate_exact_size(Vec2::new(width, LABEL_H + grid_h), Sense::hover());
    let origin = Pos2::new(block.left(), block.top() + LABEL_H);
    let painter = ui.painter();

    let start = monday_of(today).unwrap_or(today) - chrono::Duration::weeks(weeks as i64 - 1);
    let max_tokens = days.iter().map(|d| d.tokens).max().unwrap_or(0);
    let by_date: std::collections::HashMap<&str, &DayUsage> =
        days.iter().map(|d| (d.date.as_str(), d)).collect();

    let mut last_month = 0u32;
    // 用 -100 而不是 isize::MIN 当哨兵: 下面要做减法, MIN 会直接下溢
    let mut last_label_col: isize = -100;
    for col in 0..weeks {
        let col_date = start + chrono::Duration::weeks(col as i64);
        // 跨月的那一列标一次月份; 挨得太近的两列只留后面那个, 否则 "2月 3月" 会糊在一起
        if col_date.month() != last_month && col as isize - last_label_col >= 3 {
            painter.text(
                Pos2::new(origin.x + col as f32 * (cell + GAP), block.top() + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{}月", col_date.month()),
                FontId::monospace(9.0),
                theme::FAINT,
            );
            last_month = col_date.month();
            last_label_col = col as isize;
        } else if col_date.month() != last_month {
            last_month = col_date.month();
        }

        for row in 0..7 {
            let date = col_date + chrono::Duration::days(row as i64);
            if date > today {
                continue;
            }
            let key = date.format("%Y-%m-%d").to_string();
            let rec = by_date.get(key.as_str()).copied();
            let rect = Rect::from_min_size(
                Pos2::new(
                    origin.x + col as f32 * (cell + GAP),
                    origin.y + row as f32 * (cell + GAP),
                ),
                Vec2::new(cell, cell),
            );

            let fill = match (rec, max_tokens) {
                (Some(d), m) if m > 0 && d.tokens > 0 => {
                    // 开方压缩: 线性映射会让一两个高峰日把其余格子全压成空白
                    let ratio = (d.tokens as f64 / m as f64).sqrt();
                    let idx = ((ratio * RAMP.len() as f64) as usize).min(RAMP.len() - 1);
                    RAMP[idx]
                }
                (Some(_), _) => RAMP[0],
                (None, _) => EMPTY,
            };
            painter.rect_filled(rect, 2.0, fill);
            if date == today {
                painter.rect_stroke(rect, 2.0, Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
            }

            let resp = ui.interact(rect, ui.id().with(("cal", col, row)), Sense::hover());
            let tip = match rec {
                Some(d) => format!(
                    "{} · {} tok · ${:.2} · {} req",
                    d.date,
                    theme::format_short(d.tokens),
                    d.cost_usd,
                    d.requests
                ),
                None => format!("{} · 无请求", date.format("%Y-%m-%d")),
            };
            resp.on_hover_text(tip);
        }
    }
}

fn monday_of(d: NaiveDate) -> Option<NaiveDate> {
    let offset = d.weekday().num_days_from_monday() as i64;
    d.checked_sub_days(chrono::Days::new(offset as u64))
}
