//! The interface's design tokens (colors, type, spacing) and the egui style built from them.
//! See docs/explanation/about-the-interface.md and docs/reference/windows-and-panels.md#controls.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

// Surfaces, darkest to lightest.
pub const BG_CHROME: Color32 = Color32::from_rgb(0x0b, 0x0c, 0x0e);
pub const BG_APP: Color32 = Color32::from_rgb(0x0e, 0x0f, 0x11);
pub const BG_CANVAS: Color32 = Color32::from_rgb(0x11, 0x12, 0x14);
pub const BG_PANEL: Color32 = Color32::from_rgb(0x16, 0x17, 0x1a);
pub const BG_INPUT: Color32 = Color32::from_rgb(0x22, 0x24, 0x29);
pub const BG_ACTIVE: Color32 = Color32::from_rgb(0x2f, 0x32, 0x38);
pub const BORDER: Color32 = Color32::from_rgb(0x26, 0x28, 0x2d);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x33, 0x36, 0x3c);
/// Lines between rows and sections.
pub const DIVIDER: Color32 = Color32::from_rgb(0x1e, 0x20, 0x24);

// Text: values, labels, hints.
pub const TEXT: Color32 = Color32::from_rgb(0xe7, 0xe8, 0xeb);
pub const TEXT_2: Color32 = Color32::from_rgb(0xa1, 0xa4, 0xad);
pub const TEXT_3: Color32 = Color32::from_rgb(0x6b, 0x6f, 0x79);

/// Selection and focus: oklch(0.72 0.12 250).
pub const ACCENT: Color32 = Color32::from_rgb(103, 170, 237);

/// The accent at `alpha` (0 to 1), as egui's premultiplied color.
pub fn accent(alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), (alpha * 255.0).round() as u8)
}

// Socket data types, oklch(0.80 0.13 h).
pub const SOCK_COLOR: Color32 = Color32::from_rgb(233, 180, 82);
pub const SOCK_FLOAT: Color32 = Color32::from_rgb(0x8a, 0x8e, 0x98);
pub const SOCK_MATERIAL: Color32 = Color32::from_rgb(124, 213, 145);

// States.
/// Good (cached, up to date): oklch(0.84 0.11 150).
pub const GOOD: Color32 = Color32::from_rgb(149, 223, 164);
/// Attention (dirty, out of date, unbaked): oklch(0.84 0.11 80).
pub const ATTENTION: Color32 = Color32::from_rgb(240, 195, 116);
/// Errors.
pub const ERROR: Color32 = Color32::from_rgb(236, 112, 99);
/// The app's mark, oklch(0.80 0.13 80).
pub const BRAND: Color32 = SOCK_COLOR;

// Sizes, in points.
pub const H_CONTROL: f32 = 24.0;
pub const H_SECTION: f32 = 32.0;
pub const PANEL_PAD: f32 = 12.0;
pub const R_CHECK: u8 = 3;
pub const R_CONTROL: u8 = 5;
pub const R_GROUP: u8 = 7;
pub const R_WINDOW: u8 = 10;
/// Opacity of disabled controls.
pub const DISABLED_OPACITY: f32 = 0.45;

// Type sizes.
pub const FS_BODY: f32 = 12.0;
pub const FS_SECTION: f32 = 11.0;
pub const FS_CAPTION: f32 = 11.0;
pub const FS_VALUE: f32 = 11.0;

/// Geist Medium and SemiBold, and Geist Mono Medium, as named font families.
pub const MEDIUM: &str = "Geist Medium";
pub const SEMIBOLD: &str = "Geist SemiBold";
pub const MONO_MEDIUM: &str = "Geist Mono Medium";

pub fn body() -> FontId {
    FontId::new(FS_BODY, FontFamily::Proportional)
}

pub fn body_strong() -> FontId {
    FontId::new(FS_BODY, FontFamily::Name(MEDIUM.into()))
}

pub fn section() -> FontId {
    FontId::new(FS_SECTION, FontFamily::Name(SEMIBOLD.into()))
}

pub fn caption() -> FontId {
    FontId::new(FS_CAPTION, FontFamily::Proportional)
}

/// Numbers, file names, resolutions and shortcuts.
pub fn value() -> FontId {
    FontId::new(FS_VALUE, FontFamily::Monospace)
}

/// Installs the fonts and the style.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.text_styles = [
            (TextStyle::Small, caption()),
            (TextStyle::Body, body()),
            (TextStyle::Button, body()),
            (TextStyle::Monospace, value()),
            (TextStyle::Heading, FontId::new(14.0, FontFamily::Name(SEMIBOLD.into()))),
        ]
        .into();

        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.override_text_color = None;
        v.panel_fill = BG_PANEL;
        v.window_fill = BG_PANEL;
        v.extreme_bg_color = BG_INPUT;
        v.text_edit_bg_color = Some(BG_INPUT);
        v.faint_bg_color = BG_CANVAS;
        v.code_bg_color = BG_INPUT;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(R_WINDOW);
        v.menu_corner_radius = CornerRadius::same(R_GROUP);
        v.window_shadow = egui::Shadow { offset: [0, 12], blur: 40, spread: 0, color: Color32::from_black_alpha(110) };
        v.popup_shadow = egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(90) };
        v.selection.bg_fill = accent(0.35);
        v.selection.stroke = Stroke::new(1.0, TEXT);
        v.hyperlink_color = Color32::from_rgb(122, 189, 255);
        v.warn_fg_color = ATTENTION;
        v.error_fg_color = ERROR;
        v.weak_text_color = Some(TEXT_3);
        v.slider_trailing_fill = true;
        v.disabled_alpha = DISABLED_OPACITY;
        v.striped = false;

        let r = CornerRadius::same(R_CONTROL);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = BG_PANEL;
        w.noninteractive.weak_bg_fill = BG_PANEL;
        w.noninteractive.bg_stroke = Stroke::new(1.0, DIVIDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.bg_fill = BG_INPUT;
        w.inactive.weak_bg_fill = BG_INPUT;
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.hovered.bg_fill = BG_ACTIVE;
        w.hovered.weak_bg_fill = BG_ACTIVE;
        w.hovered.bg_stroke = Stroke::NONE;
        w.hovered.fg_stroke = Stroke::new(1.0, TEXT);
        w.active.bg_fill = BORDER_STRONG;
        w.active.weak_bg_fill = BORDER_STRONG;
        w.active.bg_stroke = Stroke::new(1.0, accent(0.6));
        w.active.fg_stroke = Stroke::new(1.0, TEXT);
        w.open.bg_fill = BG_ACTIVE;
        w.open.weak_bg_fill = BG_ACTIVE;
        w.open.bg_stroke = Stroke::NONE;
        w.open.fg_stroke = Stroke::new(1.0, TEXT);
        for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            s.corner_radius = r;
            s.expansion = 0.0;
        }

        let s = &mut style.spacing;
        s.item_spacing = egui::vec2(8.0, 6.0);
        s.button_padding = egui::vec2(9.0, 4.0);
        s.interact_size = egui::vec2(40.0, H_CONTROL);
        s.icon_width = 14.0;
        s.icon_width_inner = 8.0;
        s.icon_spacing = 6.0;
        s.window_margin = egui::Margin::same(PANEL_PAD as i8);
        s.menu_margin = egui::Margin::same(6);
        s.combo_height = 320.0;
        s.indent = 14.0;
    });
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.to_owned(), Arc::new(egui::FontData::from_static(bytes)));
    };
    add("Geist", include_bytes!("fonts/Geist-Regular.ttf"));
    add(MEDIUM, include_bytes!("fonts/Geist-Medium.ttf"));
    add(SEMIBOLD, include_bytes!("fonts/Geist-SemiBold.ttf"));
    add("Geist Mono", include_bytes!("fonts/GeistMono-Regular.ttf"));
    add(MONO_MEDIUM, include_bytes!("fonts/GeistMono-Medium.ttf"));
    // egui's own fonts stay as fallbacks for symbols Geist doesn't have (▸, ✓, emoji).
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let family = |first: &str| std::iter::once(first.to_owned()).chain(fallback.iter().cloned()).collect::<Vec<_>>();
    fonts.families.insert(FontFamily::Proportional, family("Geist"));
    fonts.families.insert(FontFamily::Monospace, family("Geist Mono"));
    fonts.families.insert(FontFamily::Name(MEDIUM.into()), family(MEDIUM));
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), family(SEMIBOLD));
    fonts.families.insert(FontFamily::Name(MONO_MEDIUM.into()), family(MONO_MEDIUM));
    ctx.set_fonts(fonts);
}
