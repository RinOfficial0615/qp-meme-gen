//! WinUI 3 (Fluent Design) 风格主题：浅/深双色板、样式应用、动画曲线。
//! 交互范式：选项类控件（拖放卡、分段选择、下拉框）未选中 = 灰边框，
//! 选中/悬停拖入 = 强调色蓝边框。

use eframe::egui::{self, Color32, CornerRadius, Stroke, Theme, ThemePreference};

pub mod anim {
    use std::time::Duration;

    /// WinUI 快速反馈 ≈ 167ms，标准 ≈ 250ms。
    pub const FAST: Duration = Duration::from_millis(167);
    pub const NORMAL: Duration = Duration::from_millis(250);

    /// ease-out cubic。
    pub fn ease_out(t: f32) -> f32 {
        1.0 - (1.0 - t).powi(3)
    }
}

pub mod metrics {
    use eframe::egui;

    /// 卡片圆角 8，控件圆角 4。
    pub const CARD_RADIUS: u8 = 8;
    pub const CONTROL_RADIUS: u8 = 4;
    pub const BUTTON_PADDING: egui::Vec2 = egui::Vec2::new(14.0, 6.0);
}

/// 动态色板：随 egui 主题切换。
#[derive(Clone, Copy)]
pub struct Palette {
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_pressed: Color32,
    /// 强调色浅底（选中块 / 拖放卡悬停填充）。
    pub accent_tint: Color32,
    pub on_accent: Color32,

    pub bg: Color32,
    pub card: Color32,
    pub subtle: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub control_pressed: Color32,

    pub text: Color32,
    pub text_secondary: Color32,
    pub stroke_control: Color32,
    pub stroke_divider: Color32,
    pub danger: Color32,
    pub success: Color32,
}

/// WinUI 3 浅色模式色板。描边/轨道要比卡片底更深一档，否则 1px 框在浅底上会消失。
pub const LIGHT: Palette = Palette {
    accent: Color32::from_rgb(0x00, 0x67, 0xC0),
    accent_hover: Color32::from_rgb(0x1A, 0x78, 0xC9),
    accent_pressed: Color32::from_rgb(0x33, 0x89, 0xD1),
    accent_tint: Color32::from_rgb(0xD0, 0xE8, 0xF9),
    on_accent: Color32::WHITE,
    bg: Color32::from_rgb(0xF3, 0xF3, 0xF3),
    card: Color32::WHITE,
    subtle: Color32::from_rgb(0xE6, 0xE6, 0xE6),
    control: Color32::WHITE,
    control_hover: Color32::from_rgb(0xF0, 0xF0, 0xF0),
    control_pressed: Color32::from_rgb(0xE4, 0xE4, 0xE4),
    text: Color32::from_rgb(0x1B, 0x1B, 0x1B),
    text_secondary: Color32::from_rgb(0x5D, 0x5D, 0x5D),
    stroke_control: Color32::from_rgb(0x8A, 0x8A, 0x8A),
    stroke_divider: Color32::from_rgb(0xC6, 0xC6, 0xC6),
    danger: Color32::from_rgb(0xC4, 0x2B, 0x1C),
    success: Color32::from_rgb(0x0F, 0x7B, 0x0F),
};

/// WinUI 3 深色模式色板。
pub const DARK: Palette = Palette {
    accent: Color32::from_rgb(0x60, 0xCD, 0xFF),
    accent_hover: Color32::from_rgb(0x7C, 0xD8, 0xFF),
    accent_pressed: Color32::from_rgb(0x49, 0xBE, 0xF2),
    accent_tint: Color32::from_rgb(0x23, 0x33, 0x3F),
    on_accent: Color32::from_rgb(0x00, 0x26, 0x40),
    bg: Color32::from_rgb(0x20, 0x20, 0x20),
    card: Color32::from_rgb(0x2A, 0x2A, 0x2A),
    subtle: Color32::from_rgb(0x2F, 0x2F, 0x2F),
    control: Color32::from_rgb(0x33, 0x33, 0x33),
    control_hover: Color32::from_rgb(0x3B, 0x3B, 0x3B),
    control_pressed: Color32::from_rgb(0x46, 0x46, 0x46),
    text: Color32::WHITE,
    text_secondary: Color32::from_rgb(0xAD, 0xAD, 0xAD),
    stroke_control: Color32::from_rgb(0x45, 0x45, 0x45),
    stroke_divider: Color32::from_rgb(0x36, 0x36, 0x36),
    danger: Color32::from_rgb(0xFF, 0x99, 0xA4),
    success: Color32::from_rgb(0x6C, 0xCB, 0x6C),
};

/// 取当前主题色板。
pub fn palette(ctx: &egui::Context) -> &'static Palette {
    match ctx.theme() {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
    }
}

/// 应用全局主题（浅/深各样式化一套），并按偏好设置跟随策略。
pub fn apply(ctx: &egui::Context, preference: ThemePreference) {
    ctx.options_mut(|o| o.theme_preference = preference);
    styleize(ctx, Theme::Light);
    styleize(ctx, Theme::Dark);
}

fn styleize(ctx: &egui::Context, theme: Theme) {
    let p = match theme {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
    };
    let mut style = (*ctx.style_of(theme)).clone();
    let v = &mut style.visuals;

    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0, p.stroke_control);
    v.extreme_bg_color = p.control;
    v.faint_bg_color = p.subtle;
    // 不强制覆盖字色：选中项要用 selection.stroke（强调色底 + 反色字）。
    v.override_text_color = None;
    v.weak_text_color = Some(p.text_secondary);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, p.on_accent);

    let rest = |w: &mut egui::style::WidgetVisuals, fill: Color32, stroke: Color32| {
        w.weak_bg_fill = fill;
        w.bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
        w.fg_stroke = Stroke::new(1.0, p.text);
        w.corner_radius = CornerRadius::same(metrics::CONTROL_RADIUS);
        w.expansion = 0.0;
    };

    rest(
        &mut v.widgets.noninteractive,
        Color32::TRANSPARENT,
        p.stroke_divider,
    );
    v.widgets.noninteractive.bg_fill = p.bg;
    rest(&mut v.widgets.inactive, p.control, p.stroke_control);
    rest(&mut v.widgets.hovered, p.control_hover, p.stroke_control);
    rest(&mut v.widgets.active, p.control_pressed, p.stroke_control);
    rest(&mut v.widgets.open, p.control, p.accent);

    v.window_corner_radius = CornerRadius::same(metrics::CARD_RADIUS);
    v.window_shadow = egui::epaint::Shadow::NONE;
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: match theme {
            Theme::Light => Color32::from_black_alpha(36),
            Theme::Dark => Color32::from_black_alpha(80),
        },
    };

    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = metrics::BUTTON_PADDING;
    style.spacing.interact_size.y = 28.0;
    style.interaction.selectable_labels = false;
    style.interaction.multi_widget_text_select = false;

    ctx.set_style_of(theme, style);
}

fn card_shadow(ctx: &egui::Context) -> egui::epaint::Shadow {
    match ctx.theme() {
        Theme::Light => egui::epaint::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(28),
        },
        Theme::Dark => egui::epaint::Shadow::NONE,
    }
}

/// 卡片框架（Fluent Layer）：卡片底色、8px 圆角、可见描边；浅色加一层轻阴影。
pub fn card_frame(ctx: &egui::Context) -> egui::Frame {
    let p = palette(ctx);
    egui::Frame::default()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.stroke_divider))
        .corner_radius(CornerRadius::same(metrics::CARD_RADIUS))
        .shadow(card_shadow(ctx))
        .inner_margin(egui::Margin::same(20))
}

/// 主按钮（强调色填充，悬停/按下按 Fluent 强调色递进）。不叠字，避免字号跳变。
pub fn accent_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let p = *palette(ui.ctx());
    let id = ui.next_auto_id();
    let prev = ui.ctx().read_response(id);
    let pressed = prev.as_ref().is_some_and(|r| r.is_pointer_button_down_on());
    let hot = prev.as_ref().is_some_and(|r| r.hovered());
    let fill = if pressed {
        p.accent_pressed
    } else if hot {
        p.accent_hover
    } else {
        p.accent
    };
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(p.on_accent))
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(metrics::CONTROL_RADIUS))
            .min_size(egui::vec2(0.0, 30.0)),
    )
}

/// 分段选择器：未选中 = 灰边框透明底，选中 = 蓝边框 + 强调色浅底。
pub fn segmented_control<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let p = *palette(ui.ctx());
    let mut changed = false;
    let frame = egui::Frame::default()
        .fill(p.subtle)
        .corner_radius(CornerRadius::same(metrics::CONTROL_RADIUS + 2))
        .inner_margin(egui::Margin::same(2));
    frame.show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (v, label) in options {
            let selected = *value == *v;
            let text =
                egui::RichText::new(*label).color(if selected { p.text } else { p.text_secondary });
            let id = ui.next_auto_id();
            let hovered = ui
                .ctx()
                .read_response(id)
                .is_some_and(|r| r.hovered() && !r.is_pointer_button_down_on());
            let fill = if selected {
                p.accent_tint
            } else if hovered {
                p.control_hover
            } else {
                Color32::TRANSPARENT
            };
            let btn = egui::Button::new(text)
                .fill(fill)
                .stroke(Stroke::new(
                    1.0,
                    if selected { p.accent } else { p.stroke_control },
                ))
                .corner_radius(CornerRadius::same(metrics::CONTROL_RADIUS));
            let resp = ui.add(btn);
            if resp.clicked() {
                *value = *v;
                changed = true;
            }
        }
    });
    changed
}

/// WinUI 3 复选框：未选中 = 实心底 + 灰边（悬停蓝边），选中 = 强调色底 + 对勾。
pub fn accent_checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str) -> egui::Response {
    let p = *palette(ui.ctx());
    let box_side = 16.0;
    let gap = 6.0;
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font_id, p.text);
    let height = ui.spacing().interact_size.y.max(box_side);
    let width = box_side + gap + galley.size().x + 4.0;
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *checked, label)
    });

    if resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }

    let t = anim::ease_out(ui.ctx().animate_bool_with_time(
        resp.id.with("on"),
        *checked,
        anim::FAST.as_secs_f32(),
    ));
    let hover = hover_t(
        ui,
        resp.id.with("hov"),
        resp.hovered() && !resp.is_pointer_button_down_on(),
    );
    let pressed = resp.is_pointer_button_down_on();

    let box_rect = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + 2.0 + box_side * 0.5, rect.center().y),
        egui::vec2(box_side, box_side),
    );
    // 未选中必须用不透明控件底，不能走透明：和顶栏插值时会先闪深色，
    // 落到 hover 灰后又和工具栏底几乎一样。
    let rest_fill = if pressed {
        p.control_pressed
    } else {
        lerp_color(p.control, p.accent_tint, hover * 0.45)
    };
    let on_fill = if pressed {
        p.accent_pressed
    } else {
        lerp_color(p.accent, p.accent_hover, hover)
    };
    let fill = lerp_color(rest_fill, on_fill, t);
    let rest_stroke = lerp_color(p.stroke_control, p.accent, hover);
    let stroke = Stroke::new(1.0, lerp_color(rest_stroke, p.accent, t));

    let painter = ui.painter();
    painter.rect(
        box_rect,
        CornerRadius::same(metrics::CONTROL_RADIUS),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );

    if t > 0.01 {
        let c = box_rect.center();
        let s = box_side * (0.72 + 0.08 * t);
        let col = Color32::from_rgba_unmultiplied(
            p.on_accent.r(),
            p.on_accent.g(),
            p.on_accent.b(),
            (t * 255.0) as u8,
        );
        let mark = Stroke::new(1.5, col);
        painter.line_segment(
            [
                egui::pos2(c.x - s * 0.22, c.y + s * 0.02),
                egui::pos2(c.x - s * 0.04, c.y + s * 0.18),
            ],
            mark,
        );
        painter.line_segment(
            [
                egui::pos2(c.x - s * 0.04, c.y + s * 0.18),
                egui::pos2(c.x + s * 0.24, c.y - s * 0.16),
            ],
            mark,
        );
    }

    // CJK 行框比字面高一截，按墨水区域中心对齐勾选框，而不是 galley.size。
    let ink = galley.mesh_bounds;
    let text_y = if ink.height() > 0.0 {
        box_rect.center().y - ink.center().y
    } else {
        box_rect.center().y - galley.size().y * 0.5
    };
    painter.galley(egui::pos2(box_rect.max.x + gap, text_y), galley, p.text);
    resp
}

/// 悬停增亮动画系数（0..1）。
pub fn hover_t(ui: &egui::Ui, id: egui::Id, hovered: bool) -> f32 {
    ui.ctx()
        .animate_bool_with_time(id, hovered, anim::FAST.as_secs_f32())
}

/// 颜色线性插值。
pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)) as u8;
    Color32::from_rgba_unmultiplied(
        f(a.r(), b.r()),
        f(a.g(), b.g()),
        f(a.b(), b.b()),
        f(a.a(), b.a()),
    )
}

/// 下拉项：选中 = 强调色底 + 反色字。
pub fn combo_choice<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    label: &str,
) -> bool {
    let p = *palette(ui.ctx());
    let selected = *current == value;
    let text = egui::RichText::new(label).color(if selected { p.on_accent } else { p.text });
    if ui.selectable_label(selected, text).clicked() {
        *current = value;
        true
    } else {
        false
    }
}

/// 主页 ↔ 编辑器：WinUI Entrance（淡入 + 水平滑入）。
#[derive(Clone, Copy)]
pub struct PageEnter {
    pub t: f32,
    pub dir: f32,
}

impl PageEnter {
    pub fn done() -> Self {
        Self { t: 1.0, dir: 0.0 }
    }

    pub fn from_start(at: std::time::Instant, dir: f32) -> Self {
        let raw = (at.elapsed().as_secs_f32() / anim::NORMAL.as_secs_f32()).min(1.0);
        Self {
            t: anim::ease_out(raw),
            dir,
        }
    }

    pub fn apply(self, ui: &mut egui::Ui) {
        let t = self.t.clamp(0.0, 1.0);
        ui.multiply_opacity(0.25 + 0.75 * t);
        let dx = (1.0 - t) * 36.0 * self.dir;
        ui.ctx().set_transform_layer(
            ui.layer_id(),
            if dx.abs() < 0.5 {
                egui::emath::TSTransform::IDENTITY
            } else {
                egui::emath::TSTransform::from_translation(egui::vec2(dx, 0.0))
            },
        );
        if t < 1.0 {
            ui.ctx().request_repaint();
        }
    }
}
