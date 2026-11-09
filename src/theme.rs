//! Colour palette, fonts and egui visuals for the light and dark themes.

use eframe::egui;
use egui::{Color32, FontData, FontDefinitions, FontFamily, Rounding, Stroke};
use std::fs;

/// Every colour the UI paints with. Drawing code reads colours from here
/// rather than hard-coding them, so both themes stay consistent.
#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Window background behind the cards.
    pub bg: Color32,
    /// Cards, header, detail panel.
    pub surface: Color32,
    /// Inputs, icon wells, subtle fills.
    pub surface_alt: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub on_accent: Color32,
    pub success: Color32,
    pub success_soft: Color32,
    pub warning: Color32,
    pub warning_soft: Color32,
    pub danger: Color32,
    pub danger_soft: Color32,
    pub shadow: Color32,
}

impl Palette {
    pub fn light() -> Self {
        Self {
            dark: false,
            bg: rgb(244, 245, 249),
            surface: rgb(255, 255, 255),
            surface_alt: rgb(240, 242, 247),
            border: rgb(226, 229, 238),
            border_strong: rgb(203, 208, 222),
            text: rgb(22, 26, 40),
            text_muted: rgb(96, 103, 125),
            text_faint: rgb(146, 152, 172),
            accent: rgb(91, 84, 232),
            accent_soft: rgb(236, 235, 254),
            on_accent: rgb(255, 255, 255),
            success: rgb(22, 140, 88),
            success_soft: rgb(225, 245, 235),
            warning: rgb(196, 112, 10),
            warning_soft: rgb(253, 242, 222),
            danger: rgb(206, 50, 64),
            danger_soft: rgb(252, 230, 232),
            shadow: Color32::from_black_alpha(18),
        }
    }

    pub fn dark() -> Self {
        Self {
            dark: true,
            bg: rgb(17, 18, 24),
            surface: rgb(26, 28, 36),
            surface_alt: rgb(34, 37, 47),
            border: rgb(44, 47, 60),
            border_strong: rgb(64, 68, 86),
            text: rgb(232, 234, 242),
            text_muted: rgb(158, 164, 186),
            text_faint: rgb(110, 116, 138),
            accent: rgb(129, 120, 255),
            accent_soft: rgb(40, 38, 74),
            on_accent: rgb(255, 255, 255),
            success: rgb(74, 205, 140),
            success_soft: rgb(24, 52, 40),
            warning: rgb(240, 170, 70),
            warning_soft: rgb(58, 44, 22),
            danger: rgb(255, 110, 120),
            danger_soft: rgb(66, 28, 34),
            shadow: Color32::from_black_alpha(70),
        }
    }

    pub fn for_mode(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    /// A stable, pleasant colour derived from a string, for the letter
    /// placeholder shown when an icon has no preview.
    pub fn tint_for(&self, key: &str) -> (Color32, Color32) {
        const HUES: [(u8, u8, u8); 8] = [
            (91, 84, 232),
            (14, 150, 170),
            (220, 90, 60),
            (40, 150, 90),
            (200, 70, 140),
            (210, 140, 20),
            (60, 120, 220),
            (130, 90, 200),
        ];
        let h = key
            .bytes()
            .fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
        let (r, g, b) = HUES[(h as usize) % HUES.len()];
        let fg = Color32::from_rgb(r, g, b);
        let bg = if self.dark {
            Color32::from_rgba_unmultiplied(r, g, b, 48)
        } else {
            Color32::from_rgba_unmultiplied(r, g, b, 42)
        };
        (fg, bg)
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Font family used for headings and emphasised labels.
pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

/// Use a system UI font when one is installed (egui's bundled Ubuntu-Light
/// looks thin and dated), keeping egui's own fonts as fallbacks so emoji and
/// symbol glyphs still render.
pub fn install_fonts(ctx: &egui::Context) {
    const REGULAR: &[&str] = &[
        "/usr/share/fonts/inter/Inter-Regular.otf",
        "/usr/share/fonts/rsms-inter-fonts/Inter-Regular.otf",
        "/usr/share/fonts/opentype/inter/Inter-Regular.otf",
        "/usr/share/fonts/TTF/Inter-Regular.ttf",
        "/usr/share/fonts/abattis-cantarell-fonts/Cantarell-Regular.otf",
        "/usr/share/fonts/cantarell/Cantarell-Regular.otf",
        "/usr/share/fonts/opentype/cantarell/Cantarell-Regular.otf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ];
    const BOLD: &[&str] = &[
        "/usr/share/fonts/inter/Inter-SemiBold.otf",
        "/usr/share/fonts/rsms-inter-fonts/Inter-SemiBold.otf",
        "/usr/share/fonts/opentype/inter/Inter-SemiBold.otf",
        "/usr/share/fonts/TTF/Inter-SemiBold.ttf",
        "/usr/share/fonts/abattis-cantarell-fonts/Cantarell-Bold.otf",
        "/usr/share/fonts/cantarell/Cantarell-Bold.otf",
        "/usr/share/fonts/opentype/cantarell/Cantarell-Bold.otf",
        "/usr/share/fonts/truetype/noto/NotoSans-SemiBold.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
        "/usr/share/fonts/noto/NotoSans-Bold.ttf",
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
    ];

    let mut fonts = FontDefinitions::default();
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let first = |paths: &[&str]| paths.iter().find_map(|p| fs::read(p).ok());

    let mut proportional = fallbacks.clone();
    if let Some(data) = first(REGULAR) {
        fonts.font_data.insert("ui-regular".into(), FontData::from_owned(data));
        proportional.insert(0, "ui-regular".into());
    }
    let mut bold_family = proportional.clone();
    if let Some(data) = first(BOLD) {
        fonts.font_data.insert("ui-bold".into(), FontData::from_owned(data));
        bold_family.insert(0, "ui-bold".into());
    }

    fonts.families.insert(FontFamily::Proportional, proportional);
    fonts.families.insert(bold(), bold_family);
    ctx.set_fonts(fonts);
}

/// Detect whether the desktop prefers a dark theme.
pub fn system_prefers_dark(cc: &eframe::CreationContext<'_>) -> bool {
    if let Some(theme) = cc.integration_info.system_theme {
        return theme == eframe::Theme::Dark;
    }
    // winit can't tell on most Linux desktops; ask GNOME's setting directly.
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("dark"))
        .unwrap_or(false)
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };

    v.override_text_color = None;
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.window_rounding = Rounding::same(12.0);
    v.menu_rounding = Rounding::same(8.0);
    v.extreme_bg_color = p.surface_alt; // text edit background
    v.faint_bg_color = p.surface_alt;
    v.code_bg_color = p.surface_alt;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.selection.bg_fill = if p.dark {
        Color32::from_rgba_unmultiplied(129, 120, 255, 90)
    } else {
        Color32::from_rgba_unmultiplied(91, 84, 232, 60)
    };
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.window_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 8.0),
        blur: 24.0,
        spread: 0.0,
        color: p.shadow,
    };
    v.popup_shadow = v.window_shadow;

    let r = Rounding::same(8.0);
    let w = &mut v.widgets;

    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border); // separators
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.noninteractive.rounding = r;

    w.inactive.bg_fill = p.surface_alt;
    w.inactive.weak_bg_fill = p.surface_alt;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.inactive.rounding = r;

    w.hovered.bg_fill = p.surface_alt;
    w.hovered.weak_bg_fill = if p.dark { rgb(42, 45, 58) } else { rgb(232, 234, 242) };
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.border_strong);
    w.hovered.fg_stroke = Stroke::new(1.5_f32, p.text);
    w.hovered.rounding = r;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.accent_soft;
    w.active.weak_bg_fill = p.accent_soft;
    w.active.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.active.fg_stroke = Stroke::new(1.5_f32, p.text);
    w.active.rounding = r;
    w.active.expansion = 0.0;

    w.open = w.active;

    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.scroll.bar_width = 8.0;
        s.spacing.scroll.floating = true;
    });
}
