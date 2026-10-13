//! 浅色冷调透明风 (小窗专用)
//! 与主界面同一套冷白 + 天青; mini 是"贴在桌面的玻璃小表", 只比主界面多一层透明

use egui::Color32;

// 冷色玻璃风配色
pub const BG_GLASS: Color32 = Color32::from_rgba_premultiplied(240, 245, 250, 235); // 冷白 95%
pub const HAIRLINE: Color32 = Color32::from_rgb(0xD1, 0xDA, 0xE4); // 蓝灰 hairline
pub const TEXT_DARK: Color32 = Color32::from_rgb(0x1F, 0x29, 0x33); // 冷墨 (不是纯黑)
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x6B, 0x7A, 0x89); // 冷灰
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x9C, 0xA9, 0xB5); // 弱化
pub const ACCENT_CYAN: Color32 = Color32::from_rgb(0x0E, 0xA5, 0xE9); // 天青
pub const NEEDLE: Color32 = Color32::from_rgb(0x0F, 0x2C, 0x3F); // 深藏青
