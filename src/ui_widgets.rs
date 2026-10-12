//! The interface's controls: fields, segmented controls, toggles, checkboxes, section headers,
//! buttons, keycaps and tags, drawn with the tokens in ui_theme.rs.
//! See docs/reference/windows-and-panels.md#controls.

use std::ops::RangeInclusive;

use egui::emath::Numeric;
use egui::{Align2, Color32, CornerRadius, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, Widget};

use crate::icons::{self, Icon};
use crate::ui_theme as t;

/// How much larger than usual this Ui draws its controls: the Node Editor scales a node's
/// style with the zoom, and the controls here follow its control height.
pub fn scale(ui: &Ui) -> f32 {
    ui.spacing().interact_size.y / t::H_CONTROL
}

fn scaled(font: egui::FontId, s: f32) -> egui::FontId {
    egui::FontId::new(font.size * s, font.family)
}

// ---------------------------------------------------------------- field

/// A number with its label inside: drag to change it, click to type it.
pub struct Field<'a, T: Numeric> {
    value: &'a mut T,
    range: RangeInclusive<T>,
    label: String,
    decimals: Option<usize>,
    suffix: String,
    speed: Option<f64>,
    fill: bool,
    width: Option<f32>,
}

impl<'a, T: Numeric> Field<'a, T> {
    pub fn new(value: &'a mut T, range: RangeInclusive<T>) -> Self {
        Self { value, range, label: String::new(), decimals: None, suffix: String::new(), speed: None, fill: true, width: None }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Digits after the point (default: 3, or 0 for integers).
    pub fn decimals(mut self, d: usize) -> Self {
        self.decimals = Some(d);
        self
    }

    pub fn suffix(mut self, s: impl Into<String>) -> Self {
        self.suffix = s.into();
        self
    }

    /// Change per point dragged. Default: the whole range over the field's width.
    pub fn speed(mut self, s: f64) -> Self {
        self.speed = Some(s);
        self
    }

    /// No fill: for fields whose range is only a limit, like a brush size.
    pub fn no_fill(mut self) -> Self {
        self.fill = false;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    fn text(&self, v: f64) -> String {
        let d = self.decimals.unwrap_or(if T::INTEGRAL { 0 } else { 3 });
        format!("{v:.d$}{}", self.suffix)
    }
}

/// What a field is being typed into, kept in egui's memory between frames.
#[derive(Clone)]
struct Typing {
    text: String,
    started: bool,
}

impl<T: Numeric> Widget for Field<'_, T> {
    fn ui(self, ui: &mut Ui) -> Response {
        let s = scale(ui);
        let (body, value) = (scaled(t::body(), s), scaled(t::value(), s));
        let width = self.width.unwrap_or_else(|| ui.available_width()).max(40.0 * s);
        let (rect, mut response) = ui.allocate_exact_size(Vec2::new(width, t::H_CONTROL * s), Sense::click_and_drag());
        let id = response.id;
        let (min, max) = (self.range.start().to_f64(), self.range.end().to_f64());
        let clamp = |v: f64| {
            let v = v.clamp(min.min(max), max.max(min));
            if T::INTEGRAL { v.round() } else { v }
        };
        let current = self.value.to_f64();

        let mut typing: Option<Typing> = ui.data(|d| d.get_temp(id));
        // A click that isn't a drag starts typing.
        if typing.is_none() && response.clicked() && ui.is_enabled() {
            let d = self.decimals.unwrap_or(if T::INTEGRAL { 0 } else { 3 });
            typing = Some(Typing { text: format!("{current:.d$}"), started: false });
        }

        if let Some(mut ty) = typing {
            // The label stays where it was; the text sits where the value was drawn.
            let r = CornerRadius::same(t::R_CONTROL);
            let p = ui.painter();
            p.rect_filled(rect, r, t::BG_INPUT);
            let mut edit_rect = rect;
            if !self.label.is_empty() {
                let label = p.text(egui::pos2(rect.min.x + 9.0 * s, rect.center().y), Align2::LEFT_CENTER, &self.label, body.clone(), t::TEXT);
                edit_rect.min.x = (label.max.x + 8.0 * s).min(rect.max.x - 40.0 * s);
            }
            let edit_id = id.with("typing");
            // A custom frame replaces TextEdit's margin, so the padding goes on the frame.
            let edit = egui::TextEdit::singleline(&mut ty.text)
                .id(edit_id)
                .font(value)
                .horizontal_align(egui::Align::RIGHT)
                .vertical_align(egui::Align::Center)
                .frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 0, right: (9.0 * s).round() as i8, top: 0, bottom: 0 }));
            let out = ui.put(edit_rect, edit);
            ui.painter().rect_stroke(rect, r, Stroke::new(1.0, t::accent(0.6)), StrokeKind::Inside);
            if !ty.started {
                out.request_focus();
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit_id) {
                    let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(ty.text.chars().count()));
                    state.cursor.set_char_range(Some(all));
                    state.store(ui.ctx(), edit_id);
                }
                ty.started = true;
            }
            let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
            if esc {
                ui.data_mut(|d| d.remove::<Typing>(id));
            } else if enter || (ty.started && out.lost_focus()) {
                if let Ok(v) = ty.text.trim().trim_end_matches(self.suffix.trim()).trim().parse::<f64>() {
                    let v = clamp(v);
                    if v != current {
                        *self.value = T::from_f64(v);
                        response.mark_changed();
                    }
                }
                ui.data_mut(|d| d.remove::<Typing>(id));
            } else {
                ui.data_mut(|d| d.insert_temp(id, ty));
            }
            return response;
        }

        if response.dragged() && ui.is_enabled() {
            let dx = response.drag_delta().x as f64;
            if dx != 0.0 {
                let fine = if ui.input(|i| i.modifiers.ctrl) { 0.1 } else { 1.0 };
                let step = self.speed.unwrap_or((max - min) / width as f64);
                // Integers accumulate fractional steps until they add up to a whole one.
                let acc_id = id.with("scrub");
                let acc: f64 = ui.data(|d| d.get_temp(acc_id)).unwrap_or(current);
                let next = (acc + dx * step * fine).clamp(min.min(max), max.max(min));
                ui.data_mut(|d| d.insert_temp(acc_id, next));
                let v = clamp(next);
                if v != current {
                    *self.value = T::from_f64(v);
                    response.mark_changed();
                }
            }
        } else if response.drag_stopped() || !response.dragged() {
            ui.data_mut(|d| d.remove::<f64>(id.with("scrub")));
        }
        if response.hovered() && ui.is_enabled() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }

        if ui.is_rect_visible(rect) {
            let v = self.value.to_f64();
            let p = ui.painter();
            let r = CornerRadius::same(t::R_CONTROL);
            let bg = if response.hovered() || response.dragged() { Color32::from_rgb(0x28, 0x2a, 0x30) } else { t::BG_INPUT };
            p.rect_filled(rect, r, bg);
            if self.fill && max > min {
                let frac = ((v - min) / (max - min)).clamp(0.0, 1.0) as f32;
                if frac > 0.0 {
                    let fill_rect = Rect::from_min_max(rect.min, egui::pos2(rect.min.x + rect.width() * frac, rect.max.y));
                    let color = if ui.is_enabled() { t::accent(0.28) } else { t::BG_ACTIVE };
                    // Square where the fill ends, unless it reaches the field's end.
                    let fr = if frac >= 1.0 { r } else { CornerRadius { nw: t::R_CONTROL, sw: t::R_CONTROL, ne: 0, se: 0 } };
                    p.rect_filled(fill_rect, fr, color);
                }
            }
            if response.dragged() {
                p.rect_stroke(rect, r, Stroke::new(1.0, t::accent(0.6)), StrokeKind::Inside);
            }
            let y = rect.center().y;
            // A long label gives way to the value rather than running under it.
            let text = p.layout_no_wrap(self.text(v), value, t::TEXT);
            let value_x = rect.max.x - 9.0 * s - text.size().x;
            if !self.label.is_empty() {
                let clip = Rect::from_x_y_ranges(rect.min.x..=value_x - 4.0 * s, rect.y_range()).intersect(p.clip_rect());
                p.with_clip_rect(clip).text(egui::pos2(rect.min.x + 9.0 * s, y), Align2::LEFT_CENTER, &self.label, body, t::TEXT);
            }
            p.galley(egui::pos2(value_x, y - text.size().y / 2.0), text, t::TEXT);
        }
        response
    }
}

// ---------------------------------------------------------------- color

/// sRGB-encoded display value (0–1) to scene-linear.
pub fn srgb_to_linear(x: f32) -> f32 {
    if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
}

/// Scene-linear to the sRGB-encoded display value, clamped to 0–1.
pub fn linear_to_srgb(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

/// `#RRGGBB` (uppercase) for an sRGB color.
pub fn to_hex(srgb: [f32; 3]) -> String {
    let b = srgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{:02X}{:02X}{:02X}", b[0], b[1], b[2])
}

/// Six hex digits, with or without `#`, either case, surrounding spaces ignored.
pub fn parse_hex(text: &str) -> Option<[f32; 3]> {
    let hex = text.trim();
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|b| b as f32 / 255.0);
    Some([c(0)?, c(2)?, c(4)?])
}

fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let f = |n: f32| {
        let k = (n + h * 6.0) % 6.0;
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [f(5.0), f(3.0), f(1.0)]
}

fn rgb_to_hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    [h, if max > 0.0 { d / max } else { 0.0 }, max]
}

fn srgb32(srgb: [f32; 3]) -> Color32 {
    let b = srgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    Color32::from_rgb(b[0], b[1], b[2])
}

/// The picker's state while it's open: the color in sRGB HSV, and the linear color it last
/// produced, so a gray keeps its hue and a color changed elsewhere is picked up.
#[derive(Clone, Copy)]
struct Picking {
    hsv: [f32; 3],
    linear: [f32; 3],
}

const PICKER_WIDTH: f32 = 232.0;

/// A swatch that opens a color picker. `linear` is scene-linear; the picker shows and edits
/// sRGB display values. See docs/reference/windows-and-panels.md#color-picker.
pub fn color_edit(ui: &mut Ui, linear: &mut [f32; 3]) -> Response {
    let srgb = linear.map(linear_to_srgb);
    let (rect, swatch) = ui.allocate_exact_size(ui.spacing().interact_size, Sense::click());
    if ui.is_rect_visible(rect) {
        let r = CornerRadius::same(t::R_CONTROL);
        let p = ui.painter();
        p.rect_filled(rect, r, srgb32(srgb));
        let stroke = if swatch.hovered() { t::TEXT_3 } else { t::BORDER_STRONG };
        p.rect_stroke(rect, r, Stroke::new(1.0, stroke), StrokeKind::Inside);
    }
    let popup_id = swatch.id.with("picker");
    let mut out = swatch.clone();
    egui::Popup::menu(&swatch)
        .id(popup_id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(PICKER_WIDTH);
            let mut st = ui
                .data(|d| d.get_temp::<Picking>(popup_id))
                .filter(|s| s.linear == *linear)
                .unwrap_or(Picking { hsv: rgb_to_hsv(srgb), linear: *linear });
            let before = st.hsv;
            let mut typed = None;

            out |= sv_square(ui, &mut st.hsv);
            out |= hue_strip(ui, &mut st.hsv);
            let mut rgb = hsv_to_rgb(st.hsv);
            // Untouched values show the color itself, not its HSV round trip.
            if st.hsv == before {
                rgb = srgb;
            }
            let fw = ((PICKER_WIDTH - 2.0 * ui.spacing().item_spacing.x) / 3.0).floor();
            ui.horizontal(|ui| {
                for (k, name) in ["R", "G", "B"].into_iter().enumerate() {
                    let r = ui.add(Field::new(&mut rgb[k], 0.0..=1.0).label(name).decimals(3).width(fw));
                    if r.changed() {
                        typed = Some(rgb);
                    }
                    out |= r;
                }
            });
            let hex = hex_field(ui, &mut rgb);
            if hex.changed() {
                typed = Some(rgb);
            }
            out |= hex;

            if let Some(rgb) = typed {
                // A gray, or black, keeps the hue (and saturation) it had.
                let [h, s, v] = rgb_to_hsv(rgb);
                st.hsv = [if s > 0.0 && v > 0.0 { h } else { st.hsv[0] }, if v > 0.0 { s } else { st.hsv[1] }, v];
            }
            if st.hsv != before || typed.is_some() {
                *linear = typed.unwrap_or_else(|| hsv_to_rgb(st.hsv)).map(srgb_to_linear);
                st.linear = *linear;
                out.mark_changed();
            }
            ui.data_mut(|d| d.insert_temp(popup_id, st));
        });
    out
}

/// Saturation (left to right) and value (bottom to top) at the current hue.
fn sv_square(ui: &mut Ui, hsv: &mut [f32; 3]) -> Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(PICKER_WIDTH), Sense::click_and_drag());
    if let Some(p) = r.interact_pointer_pos().filter(|_| r.clicked() || r.dragged()) {
        hsv[1] = ((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
        hsv[2] = 1.0 - ((p.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
    }
    if ui.is_rect_visible(rect) {
        const N: u32 = 24;
        let mut mesh = egui::Mesh::default();
        for j in 0..=N {
            for i in 0..=N {
                let (s, v) = (i as f32 / N as f32, 1.0 - j as f32 / N as f32);
                let pos = rect.min + Vec2::new(s * rect.width(), (1.0 - v) * rect.height());
                mesh.colored_vertex(pos, srgb32(hsv_to_rgb([hsv[0], s, v])));
                if i < N && j < N {
                    let k = j * (N + 1) + i;
                    mesh.add_triangle(k, k + 1, k + N + 1);
                    mesh.add_triangle(k + 1, k + N + 2, k + N + 1);
                }
            }
        }
        let p = ui.painter();
        p.add(mesh);
        p.rect_stroke(rect, 0.0, Stroke::new(1.0, t::BORDER_STRONG), StrokeKind::Inside);
        let at = rect.min + Vec2::new(hsv[1] * rect.width(), (1.0 - hsv[2]) * rect.height());
        let ring = if hsv[2] > 0.6 && hsv[1] < 0.4 { Color32::BLACK } else { Color32::WHITE };
        p.circle_stroke(at, 5.0, Stroke::new(1.5, ring));
    }
    r
}

/// The hues, left to right, with a line at the current one.
fn hue_strip(ui: &mut Ui, hsv: &mut [f32; 3]) -> Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::new(PICKER_WIDTH, 14.0), Sense::click_and_drag());
    if let Some(p) = r.interact_pointer_pos().filter(|_| r.clicked() || r.dragged()) {
        hsv[0] = ((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
    }
    if ui.is_rect_visible(rect) {
        const N: u32 = 36;
        let mut mesh = egui::Mesh::default();
        for i in 0..=N {
            let h = i as f32 / N as f32;
            let c = srgb32(hsv_to_rgb([h, 1.0, 1.0]));
            let x = rect.min.x + h * rect.width();
            mesh.colored_vertex(egui::pos2(x, rect.min.y), c);
            mesh.colored_vertex(egui::pos2(x, rect.max.y), c);
            if i < N {
                let k = 2 * i;
                mesh.add_triangle(k, k + 1, k + 2);
                mesh.add_triangle(k + 1, k + 3, k + 2);
            }
        }
        let p = ui.painter();
        p.add(mesh);
        let x = rect.min.x + hsv[0] * rect.width();
        p.rect_filled(Rect::from_center_size(egui::pos2(x, rect.center().y), Vec2::new(3.0, rect.height() + 4.0)), 1.0, Color32::WHITE);
    }
    r
}

/// `#RRGGBB` of an sRGB color: click to type or paste one, select it to copy it. The response
/// is changed only when a color is applied.
pub fn hex_field(ui: &mut Ui, srgb: &mut [f32; 3]) -> Response {
    let width = ui.available_width().max(80.0);
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(width, t::H_CONTROL), Sense::hover());
    let id = resp.id;
    let edit_id = id.with("edit");
    // What's being typed lives in memory while the field has focus.
    let focused = ui.memory(|m| m.has_focus(edit_id));
    let mut text = ui.data(|d| d.get_temp::<String>(id)).filter(|_| focused).unwrap_or_else(|| to_hex(*srgb));
    let r = CornerRadius::same(t::R_CONTROL);
    ui.painter().rect_filled(rect, r, t::BG_INPUT);
    let label = ui.painter().text(egui::pos2(rect.min.x + 9.0, rect.center().y), Align2::LEFT_CENTER, "Hex", t::body(), t::TEXT);
    let edit_rect = Rect::from_min_max(egui::pos2(label.max.x + 8.0, rect.min.y), rect.max);
    let edit = egui::TextEdit::singleline(&mut text)
        .id(edit_id)
        .font(t::value())
        .horizontal_align(egui::Align::RIGHT)
        .vertical_align(egui::Align::Center)
        .frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 0, right: 9, top: 0, bottom: 0 }));
    let out = ui.put(edit_rect, edit);
    if out.gained_focus() {
        // Select it all, so a paste replaces it and Ctrl+C copies it.
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit_id) {
            let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()));
            state.cursor.set_char_range(Some(all));
            state.store(ui.ctx(), edit_id);
        }
    }
    if out.has_focus() {
        ui.painter().rect_stroke(rect, r, Stroke::new(1.0, t::accent(0.6)), StrokeKind::Inside);
        ui.data_mut(|d| d.insert_temp(id, text));
    } else {
        if out.lost_focus() && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            if let Some(c) = parse_hex(&text).filter(|c| to_hex(*c) != to_hex(*srgb)) {
                *srgb = c;
                resp.mark_changed();
            }
        }
        ui.data_mut(|d| d.remove::<String>(id));
    }
    resp
}

// ---------------------------------------------------------------- segmented control

/// One choice of a few, in one box. Labels that start with a digit use the value font.
pub fn segmented<V: PartialEq + Clone>(ui: &mut Ui, value: &mut V, options: &[(V, &str)]) -> Response {
    segmented_width(ui, value, options, ui.available_width())
}

pub fn segmented_width<V: PartialEq + Clone>(ui: &mut Ui, value: &mut V, options: &[(V, &str)], width: f32) -> Response {
    segmented_enabled(ui, value, options, width, &[])
}

/// A segmented control whose options can be disabled: `enabled[k]` (missing = enabled).
pub fn segmented_enabled<V: PartialEq + Clone>(ui: &mut Ui, value: &mut V, options: &[(V, &str)], width: f32, enabled: &[bool]) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(width, t::H_CONTROL), Sense::hover());
    let p = ui.painter().clone();
    p.rect(rect, CornerRadius::same(6), t::BG_APP, Stroke::new(1.0, t::BORDER), StrokeKind::Inside);
    let inner = rect.shrink(2.0);
    let n = options.len().max(1) as f32;
    let w = (inner.width() - 2.0 * (n - 1.0)) / n;
    for (k, (option, label)) in options.iter().enumerate() {
        let r = Rect::from_min_size(egui::pos2(inner.min.x + k as f32 * (w + 2.0), inner.min.y), Vec2::new(w, inner.height()));
        let on = enabled.get(k).copied().unwrap_or(true);
        let resp = ui.interact(r, response.id.with(k), if on { Sense::click() } else { Sense::hover() });
        let selected = *value == *option;
        if selected {
            p.rect_filled(r, CornerRadius::same(4), t::BG_ACTIVE);
        } else if resp.hovered() && on {
            p.rect_filled(r, CornerRadius::same(4), t::BG_INPUT);
        }
        let font = if label.starts_with(|c: char| c.is_ascii_digit()) { t::value() } else { t::body() };
        let color = if selected { t::TEXT } else if on { t::TEXT_2 } else { t::TEXT_3 };
        p.text(r.center(), Align2::CENTER_CENTER, *label, font, color);
        if resp.clicked() && !selected && on {
            *value = option.clone();
            response.mark_changed();
        }
    }
    response
}

// ---------------------------------------------------------------- toggle and checkbox

/// A switch, for settings that take effect at once.
pub struct Toggle<'a>(pub &'a mut bool);

impl Widget for Toggle<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let s = scale(ui);
        let (row, _) = ui.allocate_exact_size(Vec2::new(26.0, t::H_CONTROL) * s, Sense::hover());
        let rect = Rect::from_center_size(row.center(), Vec2::new(26.0, 15.0) * s);
        let mut response = ui.interact(rect, ui.next_auto_id().with("toggle"), Sense::click());
        if response.clicked() {
            *self.0 = !*self.0;
            response.mark_changed();
        }
        let on = ui.ctx().animate_bool_with_time(response.id, *self.0, 0.1);
        let p = ui.painter();
        let track = lerp_color(t::BORDER_STRONG, t::ACCENT, on);
        p.rect_filled(rect, CornerRadius::same(8), track);
        let x = egui::lerp(rect.min.x + 7.5 * s..=rect.max.x - 7.5 * s, on);
        p.circle_filled(egui::pos2(x, rect.center().y), 5.5 * s, lerp_color(t::TEXT_2, Color32::WHITE, on));
        response
    }
}

/// A checkbox with an optional label, for choosing items of a set.
pub struct Check<'a> {
    value: &'a mut bool,
    label: String,
}

impl<'a> Check<'a> {
    pub fn new(value: &'a mut bool, label: impl Into<String>) -> Self {
        Self { value, label: label.into() }
    }
}

impl Widget for Check<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let s = scale(ui);
        let galley = (!self.label.is_empty()).then(|| {
            ui.painter().layout_no_wrap(self.label.clone(), scaled(t::body(), s), t::TEXT)
        });
        let text_w = galley.as_ref().map_or(0.0, |g| g.size().x + 8.0 * s);
        let (rect, mut response) = ui.allocate_exact_size(Vec2::new(14.0 * s + text_w, t::H_CONTROL * s), Sense::click());
        if response.clicked() {
            *self.value = !*self.value;
            response.mark_changed();
        }
        let p = ui.painter();
        let b = Rect::from_min_size(egui::pos2(rect.min.x, rect.center().y - 7.0 * s), Vec2::splat(14.0 * s));
        let r = CornerRadius::same(t::R_CHECK);
        if *self.value {
            p.rect_filled(b, r, t::ACCENT);
            let pts = [b.min + Vec2::new(3.5, 7.2) * s, b.min + Vec2::new(6.0, 9.8) * s, b.min + Vec2::new(10.5, 4.5) * s];
            p.line(pts.to_vec(), Stroke::new(1.6 * s, t::BG_APP));
        } else {
            let stroke = if response.hovered() { t::TEXT_3 } else { Color32::from_rgb(0x44, 0x47, 0x4e) };
            p.rect_stroke(b, r, Stroke::new(1.0, stroke), StrokeKind::Inside);
        }
        if let Some(g) = galley {
            let color = if *self.value { t::TEXT } else { t::TEXT_2 };
            p.galley_with_override_text_color(egui::pos2(b.max.x + 8.0 * s, rect.center().y - g.size().y / 2.0), g, color);
        }
        response
    }
}

/// A choice in a grid of chips, like the channels a layer affects: a dot and a name, lit when on.
pub struct Chip<'a> {
    value: &'a mut bool,
    label: String,
    width: f32,
}

impl<'a> Chip<'a> {
    pub fn new(value: &'a mut bool, label: impl Into<String>, width: f32) -> Self {
        Self { value, label: label.into(), width }
    }
}

impl Widget for Chip<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, mut response) = ui.allocate_exact_size(Vec2::new(self.width, t::H_CONTROL), Sense::click());
        if response.clicked() {
            *self.value = !*self.value;
            response.mark_changed();
        }
        let p = ui.painter();
        let r = CornerRadius::same(t::R_CONTROL);
        let dot = egui::pos2(rect.min.x + 12.0, rect.center().y);
        if *self.value {
            p.rect(rect, r, t::BG_INPUT, Stroke::new(1.0, t::BORDER_STRONG), StrokeKind::Inside);
            p.circle_filled(dot, 3.0, t::ACCENT);
        } else {
            let stroke = if response.hovered() { t::BORDER_STRONG } else { t::BORDER };
            p.rect_stroke(rect, r, Stroke::new(1.0, stroke), StrokeKind::Inside);
            p.circle_stroke(dot, 2.5, Stroke::new(1.0, t::TEXT_3));
        }
        let color = if *self.value { t::TEXT } else { t::TEXT_3 };
        p.with_clip_rect(rect.shrink(2.0)).text(egui::pos2(rect.min.x + 21.0, rect.center().y), Align2::LEFT_CENTER, &self.label, t::body(), color);
        response
    }
}

fn lerp_color(a: Color32, b: Color32, f: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

// ---------------------------------------------------------------- section header

/// A foldable section: small-caps title with ▸/▾, a thin line above, optional text on the right.
pub struct Section {
    title: String,
    id_salt: Option<String>,
    default_open: bool,
    right: Option<String>,
}

impl Section {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), id_salt: None, default_open: true, right: None }
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// A summary on the right of the header, in the hint color.
    pub fn right(mut self, text: impl Into<String>) -> Self {
        self.right = Some(text.into());
        self
    }

    pub fn id_salt(mut self, salt: impl Into<String>) -> Self {
        self.id_salt = Some(salt.into());
        self
    }

    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        self.show_with(ui, |_| {}, body)
    }

    /// Like `show`, with widgets drawn at the right end of the header (e.g. an add button).
    pub fn show_with<R>(self, ui: &mut Ui, actions: impl FnOnce(&mut Ui), body: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        self.show_with_state(ui, &mut (), |ui, _| actions(ui), |ui, _| body(ui))
    }

    /// Like `show_with`, handing `state` to the header actions and then to the body, for when
    /// both change the same thing.
    pub fn show_with_state<S, R>(
        self,
        ui: &mut Ui,
        state_arg: &mut S,
        actions: impl FnOnce(&mut Ui, &mut S),
        body: impl FnOnce(&mut Ui, &mut S) -> R,
    ) -> Option<R> {
        let id = ui.make_persistent_id(("section", self.id_salt.as_deref().unwrap_or(&self.title)));
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, self.default_open);
        let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), t::H_SECTION), Sense::click());
        if response.clicked() {
            state.toggle(ui);
        }
        // The line above runs through the surrounding padding to the panel's edges, so it gets
        // a painter clipped wider than this Ui.
        let clip = ui.clip_rect();
        let wide = Rect::from_x_y_ranges(clip.x_range().expand(t::PANEL_PAD), clip.y_range());
        let line = rect.x_range().expand(t::PANEL_PAD);
        ui.ctx().layer_painter(ui.layer_id()).with_clip_rect(wide).hline(line, rect.min.y + 1.0, Stroke::new(2.0, t::BORDER_STRONG));
        let p = ui.painter();
        let open = state.openness(ui.ctx());
        let c = egui::pos2(rect.min.x + 4.0, rect.center().y);
        let tri = if open > 0.5 {
            vec![c + Vec2::new(-3.0, -1.5), c + Vec2::new(3.0, -1.5), c + Vec2::new(0.0, 2.5)]
        } else {
            vec![c + Vec2::new(-1.5, -3.0), c + Vec2::new(2.5, 0.0), c + Vec2::new(-1.5, 3.0)]
        };
        p.add(egui::Shape::convex_polygon(tri, t::TEXT_3, Stroke::NONE));
        let title = RichText::new(self.title.to_uppercase()).font(t::section()).color(t::TEXT_2).extra_letter_spacing(0.6);
        let galley = egui::WidgetText::from(title).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body);
        p.galley(egui::pos2(rect.min.x + 14.0, rect.center().y - galley.size().y / 2.0), galley, t::TEXT_2);
        if let Some(right) = &self.right {
            p.text(egui::pos2(rect.max.x, rect.center().y), Align2::RIGHT_CENTER, right, t::caption(), t::TEXT_3);
        }
        // Actions sit over the header's right end.
        let actions_rect = Rect::from_min_max(egui::pos2(rect.max.x - 120.0, rect.min.y), rect.max);
        // A detached child, so the actions don't move this Ui's cursor.
        let mut actions_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(id.with("actions"))
                .max_rect(actions_rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        actions(&mut actions_ui, state_arg);
        let out = state.show_body_unindented(ui, |ui| {
            let r = body(ui, state_arg);
            ui.add_space(8.0);
            r
        });
        out.map(|o| o.inner)
    }
}

// ---------------------------------------------------------------- buttons, keycaps, tags

/// The one main action of a surface: light fill, dark text.
pub fn primary(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).font(t::body_strong()).color(t::BG_APP)).fill(t::TEXT).min_size(Vec2::new(0.0, t::H_CONTROL))
}

/// An ordinary action.
pub fn secondary(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(t::TEXT)).fill(t::BG_INPUT).min_size(Vec2::new(0.0, t::H_CONTROL))
}

/// A quiet action, like Cancel: no fill.
pub fn ghost(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(t::TEXT_2)).frame(false).min_size(Vec2::new(0.0, t::H_CONTROL))
}

/// A quiet action shown as an icon, like Clear: no fill until hovered.
pub fn ghost_icon(icon: Icon) -> impl Widget {
    move |ui: &mut Ui| {
        let enabled = ui.is_enabled();
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(t::H_CONTROL), if enabled { Sense::click() } else { Sense::hover() });
        if resp.hovered() && enabled {
            ui.painter().rect_filled(rect, t::R_CONTROL, t::BG_INPUT);
        }
        let color = if resp.hovered() && enabled { t::TEXT } else { t::TEXT_2 };
        icons::paint(ui.painter(), icon, rect.shrink(4.0), color);
        resp
    }
}

/// A keyboard shortcut on a dark chip.
pub fn keycap(ui: &mut Ui, keys: &str) -> Response {
    chip(ui, keys, t::FS_VALUE - 0.5, t::BG_INPUT, t::TEXT_2)
}

#[derive(Clone, Copy)]
pub enum TagKind {
    Good,
    Attention,
}

/// A small state label: green for good, amber for attention.
pub fn tag(ui: &mut Ui, text: &str, kind: TagKind) -> Response {
    let color = match kind {
        TagKind::Good => t::GOOD,
        TagKind::Attention => t::ATTENTION,
    };
    let bg = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 36);
    chip(ui, text, 9.5, bg, color)
}

fn chip(ui: &mut Ui, text: &str, size: f32, bg: Color32, fg: Color32) -> Response {
    let font = egui::FontId::new(size, egui::FontFamily::Monospace);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, fg);
    let size = galley.size() + Vec2::new(10.0, 2.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(t::R_CHECK), bg);
    ui.painter().galley(rect.min + Vec2::new(5.0, 1.0), galley, fg);
    response
}

/// Width of the label column of settings rows.
pub const LABEL_WIDTH: f32 = 104.0;

/// A settings row: the label in the label color, then `add` filling the rest of the width.
pub fn setting<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        // A disabled Ui fades everything it paints, label included (Visuals::disabled_alpha).
        let (r, _) = ui.allocate_exact_size(Vec2::new(LABEL_WIDTH, t::H_CONTROL), Sense::hover());
        ui.painter().with_clip_rect(r).text(r.left_center(), Align2::LEFT_CENTER, label, t::body(), t::TEXT_2);
        add(ui)
    })
    .inner
}

/// The drag-and-drop payload released over `resp` this frame, if it is a `T`. Unlike
/// `Response::dnd_release_payload`, a payload of another type is left for the next check:
/// egui's drops it whatever its type.
pub fn dnd_release<T: std::any::Any + Send + Sync>(resp: &Response) -> Option<std::sync::Arc<T>> {
    if egui::DragAndDrop::has_payload_of_type::<T>(&resp.ctx) { resp.dnd_release_payload::<T>() } else { None }
}

/// A label in the hint color.
pub fn hint(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text.into()).color(t::TEXT_3).font(t::caption()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput};

    /// Runs one frame showing a 200-point field over 0..=1 at the top left, with `events`.
    fn frame(ctx: &egui::Context, value: &mut f32, events: Vec<Event>, modifiers: Modifiers) -> Response {
        // Each frame is 50 ms later, so separate gestures aren't merged into triple clicks.
        let time = ctx.input(|i| i.time) + 0.05;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0))),
            time: Some(time),
            events: std::iter::once(Event::ModifiersChanged(modifiers)).chain(events).collect(),
            ..Default::default()
        };
        let mut resp = None;
        let mut out = ctx.run_ui(input, |ui| {
            resp = Some(ui.add(Field::new(value, 0.0..=1.0).label("Roughness").width(200.0)));
        });
        // No renderer here: drop the font atlas uploads deliberately (epaint panics otherwise).
        out.textures_delta.clear();
        resp.unwrap()
    }

    fn press(pos: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers }
    }

    /// Drags from `from` to `to` over several frames.
    fn drag(ctx: &egui::Context, v: &mut f32, from: f32, to: f32, m: Modifiers) {
        let y = 12.0;
        frame(ctx, v, vec![Event::PointerMoved(Pos2::new(from, y))], m);
        frame(ctx, v, vec![press(Pos2::new(from, y), true, m)], m);
        for k in 1..=10 {
            let x = from + (to - from) * k as f32 / 10.0;
            frame(ctx, v, vec![Event::PointerMoved(Pos2::new(x, y))], m);
        }
        frame(ctx, v, vec![press(Pos2::new(to, y), false, m)], m);
        frame(ctx, v, vec![], m);
    }

    /// A drop zone checking for another payload type first still gets its own payload.
    #[test]
    fn dnd_release_leaves_other_payload_types() {
        let ctx = egui::Context::default();
        let at = Pos2::new(50.0, 50.0);
        let mut got = (None, None);
        for (k, events) in [vec![Event::PointerMoved(at)], vec![press(at, true, Modifiers::NONE)], vec![press(at, false, Modifiers::NONE)]].into_iter().enumerate() {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0))),
                time: Some(k as f64 * 0.05),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                if k == 1 {
                    egui::DragAndDrop::set_payload(ui.ctx(), String::from("texture"));
                }
                let r = ui.allocate_rect(Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0)), Sense::hover());
                if k == 2 {
                    got = (dnd_release::<u32>(&r), dnd_release::<String>(&r));
                }
            });
            out.textures_delta.clear();
        }
        assert!(got.0.is_none());
        assert_eq!(got.1.as_deref().map(String::as_str), Some("texture"));
    }

    #[test]
    fn drag_covers_range_over_width() {
        let ctx = egui::Context::default();
        let mut v = 0.0f32;
        drag(&ctx, &mut v, 20.0, 120.0, Modifiers::NONE);
        assert!((v - 0.5).abs() < 0.02, "{v}");
        // Clamped at the end of the range.
        drag(&ctx, &mut v, 20.0, 380.0, Modifiers::NONE);
        assert_eq!(v, 1.0);
    }

    #[test]
    fn ctrl_drag_is_ten_times_finer() {
        let ctx = egui::Context::default();
        let mut v = 0.0f32;
        drag(&ctx, &mut v, 20.0, 120.0, Modifiers::CTRL);
        assert!((v - 0.05).abs() < 0.005, "{v}");
    }

    #[test]
    fn click_without_drag_keeps_value() {
        let ctx = egui::Context::default();
        let mut v = 0.3f32;
        drag(&ctx, &mut v, 50.0, 50.0, Modifiers::NONE);
        assert_eq!(v, 0.3);
    }

    #[test]
    fn click_types_a_value() {
        let ctx = egui::Context::default();
        let m = Modifiers::NONE;
        let at = Pos2::new(50.0, 12.0);
        let type_value = |ctx: &egui::Context, v: &mut f32, text: &str, key: egui::Key| {
            for _ in 0..12 {
                frame(ctx, v, vec![], m);
            }
            frame(ctx, v, vec![Event::PointerMoved(at)], m);
            frame(ctx, v, vec![press(at, true, m)], m);
            frame(ctx, v, vec![press(at, false, m)], m);
            frame(ctx, v, vec![], m);
            // The current text is selected, so typing replaces it.
            frame(ctx, v, vec![Event::Text(text.into())], m);
            frame(ctx, v, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: m }], m);
            frame(ctx, v, vec![], m);
        };
        let mut v = 0.5f32;
        type_value(&ctx, &mut v, "0.25", egui::Key::Enter);
        assert_eq!(v, 0.25);
        // Out of range: clamped. Not a number, or Escape: unchanged.
        type_value(&ctx, &mut v, "7", egui::Key::Enter);
        assert_eq!(v, 1.0);
        type_value(&ctx, &mut v, "abc", egui::Key::Enter);
        assert_eq!(v, 1.0);
        type_value(&ctx, &mut v, "0.1", egui::Key::Escape);
        assert_eq!(v, 1.0);
    }

    #[test]
    fn srgb_round_trips_and_mid_gray() {
        // sRGB 0.5 (#808080 is 128/255) is about 0.214 linear.
        assert!((srgb_to_linear(0.5) - 0.214).abs() < 0.001);
        assert!((srgb_to_linear(128.0 / 255.0) - 0.2158).abs() < 0.0005);
        for i in 0..=255 {
            let x = i as f32 / 255.0;
            assert!((linear_to_srgb(srgb_to_linear(x)) - x).abs() < 1e-5, "{x}");
        }
        // Above 1 clamps.
        assert!((linear_to_srgb(4.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn hex_parses_and_formats() {
        assert_eq!(to_hex([0.5, 0.5, 0.5]), "#808080");
        assert_eq!(to_hex([1.0, 0.0, 42.0 / 255.0]), "#FF002A");
        let c = [0x2a as f32 / 255.0, 0x4c as f32 / 255.0, 0x7d as f32 / 255.0];
        for text in ["#2A4C7D", "2a4c7d", "  #2a4C7D ", "2A4C7D"] {
            assert_eq!(parse_hex(text), Some(c), "{text}");
        }
        for text in ["", "#", "#2A4C7", "#2A4C7D0", "#GG0000", "red", "# 2A4C7D", "#2A4C7D;"] {
            assert_eq!(parse_hex(text), None, "{text}");
        }
        // Agrees with scripts' hex colors.
        let lin = crate::script::parse_color("#2A4C7D").unwrap();
        assert_eq!(c.map(srgb_to_linear), lin);
    }

    #[test]
    fn hsv_round_trips() {
        for rgb in [[0.2, 0.4, 0.6], [1.0, 0.0, 0.0], [0.0, 1.0, 0.5], [0.9, 0.1, 0.7], [0.5, 0.5, 0.5], [0.0; 3]] {
            let back = hsv_to_rgb(rgb_to_hsv(rgb));
            for k in 0..3 {
                assert!((back[k] - rgb[k]).abs() < 1e-5, "{rgb:?} -> {back:?}");
            }
        }
        assert_eq!(rgb_to_hsv([0.5, 0.5, 0.5]), [0.0, 0.0, 0.5]);
    }

    /// Runs one frame of a hex field at the top left.
    fn hex_frame(ctx: &egui::Context, srgb: &mut [f32; 3], events: Vec<Event>) -> Response {
        let time = ctx.input(|i| i.time) + 0.05;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0))),
            time: Some(time),
            events,
            ..Default::default()
        };
        let mut resp = None;
        let mut out = ctx.run_ui(input, |ui| {
            ui.set_width(200.0);
            resp = Some(hex_field(ui, srgb));
        });
        out.textures_delta.clear();
        resp.unwrap()
    }

    #[test]
    fn hex_field_types_and_pastes() {
        let ctx = egui::Context::default();
        let at = Pos2::new(150.0, 12.0);
        let m = Modifiers::NONE;
        // Clicks the field, sends `input` (typing or a paste) and a key; returns whether it reported a change.
        let enter = |ctx: &egui::Context, c: &mut [f32; 3], input: Event, key: egui::Key| {
            for _ in 0..4 {
                hex_frame(ctx, c, vec![]);
            }
            hex_frame(ctx, c, vec![Event::PointerMoved(at)]);
            hex_frame(ctx, c, vec![press(at, true, m)]);
            hex_frame(ctx, c, vec![press(at, false, m)]);
            hex_frame(ctx, c, vec![]);
            let mut changed = hex_frame(ctx, c, vec![input]).changed();
            changed |= hex_frame(ctx, c, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: m }]).changed();
            changed |= hex_frame(ctx, c, vec![]).changed();
            changed
        };
        let mut c = [0.5f32; 3];
        assert!(enter(&ctx, &mut c, Event::Text("ff8000".into()), egui::Key::Enter));
        assert_eq!(to_hex(c), "#FF8000");
        // The whole text is selected, so a paste replaces it.
        assert!(enter(&ctx, &mut c, Event::Paste(" #2a4c7d ".into()), egui::Key::Enter));
        assert_eq!(to_hex(c), "#2A4C7D");
        // Not a color, or Escape: unchanged, and no change reported.
        assert!(!enter(&ctx, &mut c, Event::Text("nope".into()), egui::Key::Enter));
        assert!(!enter(&ctx, &mut c, Event::Text("000000".into()), egui::Key::Escape));
        assert_eq!(to_hex(c), "#2A4C7D");
    }

    #[test]
    fn drag_does_not_start_typing() {
        let ctx = egui::Context::default();
        let mut v = 0.0f32;
        drag(&ctx, &mut v, 20.0, 120.0, Modifiers::NONE);
        // Had the drag started typing, the text box would take this drag instead.
        drag(&ctx, &mut v, 120.0, 20.0, Modifiers::NONE);
        assert!(v.abs() < 0.02, "{v}");
    }
}
