//! 画布：屏幕↔图像换算、命中、拖动、绘制。渲染与指针事件共用 `View`。

use eframe::egui;

use crate::core::crop::MIN_BOX;
use crate::core::mirror::{MirrorAxis, Rect};
use crate::core::text as overlay_text;
use crate::ui::theme;

use super::Editor;
use super::overlay::{TextColor, TextOverlay};

/// 框的 8 个手柄。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Handle {
    L,
    R,
    T,
    B,
    TL,
    TR,
    BL,
    BR,
}

impl Handle {
    fn all() -> [Handle; 8] {
        [
            Handle::L,
            Handle::R,
            Handle::T,
            Handle::B,
            Handle::TL,
            Handle::TR,
            Handle::BL,
            Handle::BR,
        ]
    }

    fn cursor(self) -> egui::CursorIcon {
        match self {
            Handle::L | Handle::R => egui::CursorIcon::ResizeHorizontal,
            Handle::T | Handle::B => egui::CursorIcon::ResizeVertical,
            Handle::TL | Handle::BR => egui::CursorIcon::ResizeNwSe,
            Handle::TR | Handle::BL => egui::CursorIcon::ResizeNeSw,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub(super) enum DragMode {
    Idle,
    Move {
        origins: Vec<(u64, Rect)>,
        start: (f32, f32),
    },
    Resize {
        index: usize,
        handle: Handle,
    },
    NewBox {
        start: (f32, f32),
        current: Rect,
    },
    MoveText {
        index: usize,
        grab_dx: f32,
        grab_dy: f32,
    },
}

/// 屏幕↔图像坐标换算：渲染与指针事件共用同一处换算，保证一致性。
struct View {
    rect: egui::Rect, // 图像在屏幕上的绘制区域
    scale: f32,       // 屏幕像素 / 图像像素
}

impl View {
    fn new(avail: egui::Rect, img_w: u32, img_h: u32) -> Self {
        let scale = (avail.width() / img_w as f32).min(avail.height() / img_h as f32);
        let size = egui::vec2(img_w as f32 * scale, img_h as f32 * scale);
        let min = avail.min + (avail.size() - size) / 2.0;
        Self {
            rect: egui::Rect::from_min_size(min, size),
            scale,
        }
    }

    fn to_image(&self, pos: egui::Pos2) -> (f32, f32) {
        let v = (pos - self.rect.min) / self.scale;
        (v.x, v.y)
    }

    fn to_screen(&self, x: f32, y: f32) -> egui::Pos2 {
        self.rect.min + egui::vec2(x * self.scale, y * self.scale)
    }

    fn sel_screen_rect(&self, sel: Rect) -> egui::Rect {
        let s = sel.normalized();
        egui::Rect::from_min_max(
            self.to_screen(s.x0 as f32, s.y0 as f32),
            self.to_screen(s.x1 as f32, s.y1 as f32),
        )
    }

    fn handle_screen_rect(&self, sel: Rect, h: Handle, grow: f32) -> egui::Rect {
        let sr = self.sel_screen_rect(sel);
        let c = match h {
            Handle::TL => sr.min,
            Handle::TR => egui::pos2(sr.max.x, sr.min.y),
            Handle::BR => sr.max,
            Handle::BL => egui::pos2(sr.min.x, sr.max.y),
            Handle::T => egui::pos2(sr.center().x, sr.min.y),
            Handle::B => egui::pos2(sr.center().x, sr.max.y),
            Handle::L => egui::pos2(sr.min.x, sr.center().y),
            Handle::R => egui::pos2(sr.max.x, sr.center().y),
        };
        egui::Rect::from_center_size(c, egui::vec2(10.0 * grow, 10.0 * grow))
    }
}

/// 命中测试：角手柄优先于边手柄。容差为图像像素。
fn hit_handle(pos: (f32, f32), sel: Rect, tol_img: f32) -> Option<Handle> {
    let s = sel.normalized();
    let (px, py) = pos;
    let near = |a: f32, b: f32| (a - b).abs() <= tol_img;
    let on_l = near(px, s.x0 as f32);
    let on_r = near(px, s.x1 as f32 - 1.0);
    let on_t = near(py, s.y0 as f32);
    let on_b = near(py, s.y1 as f32 - 1.0);
    let in_y = py >= s.y0 as f32 - tol_img && py <= s.y1 as f32 + tol_img;
    let in_x = px >= s.x0 as f32 - tol_img && px <= s.x1 as f32 + tol_img;
    match () {
        _ if on_l && on_t => Some(Handle::TL),
        _ if on_r && on_t => Some(Handle::TR),
        _ if on_l && on_b => Some(Handle::BL),
        _ if on_r && on_b => Some(Handle::BR),
        _ if on_l && in_y => Some(Handle::L),
        _ if on_r && in_y => Some(Handle::R),
        _ if on_t && in_x => Some(Handle::T),
        _ if on_b && in_x => Some(Handle::B),
        _ => None,
    }
}

fn hit_boxes(pos: (f32, f32), rects: &[Rect], focus: usize) -> Option<usize> {
    let inside = |r: Rect| r.normalized().contains(pos.0 as i32, pos.1 as i32);
    if focus < rects.len() && inside(rects[focus]) {
        return Some(focus);
    }
    rects
        .iter()
        .enumerate()
        .rev()
        .find(|(_, r)| inside(**r))
        .map(|(i, _)| i)
}

fn hit_texts(ed: &Editor, pos: (f32, f32)) -> Option<usize> {
    let font = overlay_text::system_font()?;
    ed.texts
        .iter()
        .enumerate()
        .rev()
        .find(|(_, t)| {
            overlay_text::bounds(font, &t.text, t.x, t.y, t.size).is_some_and(|(x0, y0, x1, y1)| {
                let pad = 12.0;
                pos.0 >= x0 - pad && pos.0 < x1 + pad && pos.1 >= y0 - pad && pos.1 < y1 + pad
            })
        })
        .map(|(i, _)| i)
}

fn inline_text_rect(view: &View, t: &TextOverlay) -> egui::Rect {
    let font_px = (t.size * view.scale).clamp(22.0, 180.0);
    let n = t.text.chars().count().max(4) as f32;
    let w = (n * font_px * 0.95 + 56.0).clamp(320.0, (view.rect.width() * 0.9).max(320.0));
    let h = (font_px * 1.9 + 52.0).clamp(84.0, 200.0);
    egui::Rect::from_center_size(view.to_screen(t.x, t.y), egui::vec2(w, h))
}

fn show_inline_editor(ui: &mut egui::Ui, ed: &mut Editor, view: &View) {
    if !ed.text_editing {
        return;
    }
    let Some(i) = ed.text_focus else {
        return;
    };
    if i >= ed.texts.len() {
        return;
    }
    let id = ed.texts[i].id;
    let size = ed.texts[i].size;
    let color = ed.texts[i].color;
    let rect = inline_text_rect(view, &ed.texts[i]);
    let font_px = (size * view.scale).clamp(22.0, 180.0);
    let p = *theme::palette(ui.ctx());
    let mut text = ed.texts[i].text.clone();
    let need_focus = ed.text_need_focus;
    let (bg, chrome) = theme::inline_editor_chrome(color == TextColor::Black);

    let mut drag_delta = egui::Vec2::ZERO;
    egui::Area::new(egui::Id::new(("inline_text", id)))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .movable(false)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(rect.size());
            ui.set_max_width(rect.width());
            ui.visuals_mut().override_text_color = Some(chrome);
            ui.visuals_mut().weak_text_color = Some(chrome);
            egui::Frame::NONE
                .fill(bg)
                .stroke(egui::Stroke::new(2.0, p.accent))
                .corner_radius(4.0)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    let bar = ui.allocate_response(
                        egui::vec2(ui.available_width(), 22.0),
                        egui::Sense::click_and_drag(),
                    );
                    let bar = bar.on_hover_cursor(egui::CursorIcon::Move);
                    ui.painter().text(
                        bar.rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "↕ 拖动移动",
                        egui::FontId::proportional(13.0),
                        chrome,
                    );
                    if bar.dragged() {
                        drag_delta = bar.drag_delta();
                    }
                    let inner_w = (rect.width() - 28.0).max(80.0);
                    let te = egui::TextEdit::multiline(&mut text)
                        .font(egui::FontId::proportional(font_px))
                        .text_color(color.preview())
                        .desired_width(inner_w)
                        .desired_rows(1)
                        .horizontal_align(egui::Align::Center)
                        .vertical_align(egui::Align::Center)
                        .frame(egui::Frame::NONE)
                        .hint_text(egui::RichText::new("输入文字").size(font_px).color(chrome));
                    let resp = ui.add(te);
                    if need_focus {
                        resp.request_focus();
                    }
                });
        });

    if need_focus {
        ed.text_need_focus = false;
        ui.ctx().request_repaint();
    }
    if let Some(t) = ed.texts.get_mut(i)
        && t.text != text
    {
        t.text = text;
        ed.text_draft = t.text.clone();
    }
    if drag_delta != egui::Vec2::ZERO {
        let (w, h) = (ed.img.width() as f32, ed.img.height() as f32);
        if let Some(t) = ed.texts.get_mut(i) {
            t.x = (t.x + drag_delta.x / view.scale).clamp(0.0, w);
            t.y = (t.y + drag_delta.y / view.scale).clamp(0.0, h);
        }
    }
}

pub(super) fn show(ui: &mut egui::Ui, ed: &mut Editor, history_command: bool) {
    ed.refresh_result(ui.ctx());
    if ed.any_animating() {
        ui.ctx().request_repaint();
    }

    let avail = ui.available_rect_before_wrap();
    let view = View::new(avail, ed.img.width(), ed.img.height());
    let response = ui.interact(
        view.rect,
        ui.id().with("canvas"),
        egui::Sense::click_and_drag(),
    );

    let displayed: Vec<Rect> = ed.boxes.iter().map(|b| b.displayed()).collect();
    let focus = ed.focus.min(displayed.len().saturating_sub(1));

    let canvas_cursor = ui
        .ctx()
        .pointer_hover_pos()
        .map_or(egui::CursorIcon::Default, |pos| {
            let pimg = view.to_image(pos);
            if hit_texts(ed, pimg).is_some() {
                egui::CursorIcon::Move
            } else if ed.placing_text() {
                egui::CursorIcon::Text
            } else if hit_boxes(pimg, &displayed, focus).is_some() {
                egui::CursorIcon::Move
            } else if ed.full_image {
                egui::CursorIcon::Default
            } else {
                egui::CursorIcon::Crosshair
            }
        });
    let response = response.on_hover_cursor(canvas_cursor);

    let ctrl = ui.input(|i| i.modifiers.ctrl);
    if !history_command {
        handle_drag(ed, &view, &response, &displayed, ctrl);
    }

    if !history_command
        && response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let on_editor = ed.text_editing
            && ed
                .text_focus
                .and_then(|i| ed.texts.get(i))
                .is_some_and(|t| inline_text_rect(&view, t).contains(pos));
        if !on_editor {
            let p = view.to_image(pos);
            if let Some(i) = hit_texts(ed, p) {
                if ed.text_focus == Some(i) {
                    ed.begin_text_editing();
                } else {
                    ed.commit_or_drop_focused();
                    ed.select_text(i);
                }
            } else if ed.text_panel {
                ed.begin_text_at(p.0, p.1);
            } else {
                ed.commit_or_drop_focused();
                if let Some(i) = hit_boxes(p, &displayed, focus) {
                    if ui.input(|input| input.modifiers.ctrl) {
                        ed.toggle_selection(i);
                    } else {
                        ed.select_only(i);
                    }
                } else {
                    ed.clear_selection();
                }
            }
        }
    }

    let hovered_handle = if ed.drag == DragMode::Idle && ed.is_selected_index(ed.focus) {
        ui.ctx()
            .pointer_hover_pos()
            .filter(|pos| view.rect.contains(*pos))
            .and_then(|pos| {
                let pimg = view.to_image(pos);
                let sel = displayed.get(ed.focus).copied()?;
                hit_handle(pimg, sel, 12.0 / view.scale)
            })
    } else {
        None
    };

    let preview = match &ed.drag {
        DragMode::NewBox { current, .. } => Some(*current),
        _ => None,
    };
    paint(ui, ed, &view, &displayed, hovered_handle, preview);
    show_inline_editor(ui, ed, &view);

    // click-and-drag 在同一帧既可能报告 clicked 也可能报告 drag_stopped；
    // 把点击后的单选/焦点变化纳入同一个手势，避免一次点击产生两条历史。
    // 放在内联文字编辑器之后，保证同一帧的文字变化也属于这一步。
    if !history_command && (response.drag_stopped() || response.clicked()) {
        ed.finish_history_gesture();
    }
}

fn handle_drag(
    ed: &mut Editor,
    view: &View,
    response: &egui::Response,
    displayed: &[Rect],
    ctrl: bool,
) {
    let (img_w, img_h) = ed.img_size();
    let focus = ed.focus.min(displayed.len().saturating_sub(1));

    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let p = view.to_image(pos);
        let tol = 10.0 / view.scale;
        if let Some(i) = hit_texts(ed, p) {
            if ed.text_focus != Some(i) {
                ed.commit_or_drop_focused();
            }
            ed.begin_history_gesture();
            if ed.text_focus != Some(i) {
                ed.select_text(i);
            }
            let t = &ed.texts[i];
            ed.drag = DragMode::MoveText {
                index: i,
                grab_dx: p.0 - t.x,
                grab_dy: p.1 - t.y,
            };
        } else if ed.placing_text() {
            ed.drag = DragMode::Idle;
        } else if let Some(i) = hit_boxes(p, displayed, focus) {
            if ctrl {
                ed.drag = DragMode::Idle;
            } else {
                ed.commit_or_drop_focused();
                ed.begin_history_gesture();
                if !ed.is_selected_index(i) {
                    ed.select_only_raw(i);
                } else {
                    ed.focus = i;
                }
                let sel_i = displayed[i];
                if let Some(h) = hit_handle(p, sel_i, tol) {
                    let b = &mut ed.boxes[i];
                    b.anim = None;
                    b.rect = sel_i;
                    ed.drag = DragMode::Resize {
                        index: i,
                        handle: h,
                    };
                } else {
                    let ids = ed.selected_ids.clone();
                    let origins = ed
                        .boxes
                        .iter_mut()
                        .zip(displayed.iter().copied())
                        .filter_map(|(b, displayed)| {
                            if ids.contains(&b.id) {
                                b.anim = None;
                                b.rect = displayed;
                                Some((b.id, displayed.normalized()))
                            } else {
                                None
                            }
                        })
                        .collect();
                    ed.drag = DragMode::Move { origins, start: p };
                }
            }
        } else if !ed.full_image && !ctrl {
            ed.begin_history_gesture();
            let current = Rect::new(
                p.0.round() as i32,
                p.1.round() as i32,
                p.0.round() as i32,
                p.1.round() as i32,
            );
            ed.drag = DragMode::NewBox { start: p, current };
        } else {
            ed.drag = DragMode::Idle;
        }
    }

    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let p = view.to_image(pos);
        match &mut ed.drag {
            DragMode::Move { origins, start } => {
                let raw_dx = (p.0 - start.0).round() as i32;
                let raw_dy = (p.1 - start.1).round() as i32;
                let mut min_dx = i32::MIN / 4;
                let mut max_dx = i32::MAX / 4;
                let mut min_dy = i32::MIN / 4;
                let mut max_dy = i32::MAX / 4;
                for (_, r) in origins.iter() {
                    min_dx = min_dx.max(-r.x0);
                    max_dx = max_dx.min(img_w - r.x1);
                    min_dy = min_dy.max(-r.y0);
                    max_dy = max_dy.min(img_h - r.y1);
                }
                let dx = raw_dx.clamp(min_dx, max_dx);
                let dy = raw_dy.clamp(min_dy, max_dy);
                for (id, origin) in origins.iter() {
                    if let Some(b) = ed.boxes.iter_mut().find(|b| b.id == *id) {
                        let sel = Rect::new(
                            origin.x0 + dx,
                            origin.y0 + dy,
                            origin.x1 + dx,
                            origin.y1 + dy,
                        );
                        if sel != b.rect {
                            b.rect = sel;
                            ed.dirty = true;
                        }
                    }
                }
            }
            DragMode::Resize { index, handle } => {
                if let Some(b) = ed.boxes.get_mut(*index) {
                    let mut sel = b.rect.normalized();
                    let (px, py) = (p.0.round() as i32, p.1.round() as i32);
                    match *handle {
                        Handle::L => sel.x0 = px.min(sel.x1 - MIN_BOX),
                        Handle::R => sel.x1 = px.max(sel.x0 + MIN_BOX),
                        Handle::T => sel.y0 = py.min(sel.y1 - MIN_BOX),
                        Handle::B => sel.y1 = py.max(sel.y0 + MIN_BOX),
                        Handle::TL => {
                            sel.x0 = px.min(sel.x1 - MIN_BOX);
                            sel.y0 = py.min(sel.y1 - MIN_BOX);
                        }
                        Handle::TR => {
                            sel.x1 = px.max(sel.x0 + MIN_BOX);
                            sel.y0 = py.min(sel.y1 - MIN_BOX);
                        }
                        Handle::BL => {
                            sel.x0 = px.min(sel.x1 - MIN_BOX);
                            sel.y1 = py.max(sel.y0 + MIN_BOX);
                        }
                        Handle::BR => {
                            sel.x1 = px.max(sel.x0 + MIN_BOX);
                            sel.y1 = py.max(sel.y0 + MIN_BOX);
                        }
                    }
                    sel = sel.clamped(img_w, img_h);
                    if sel != b.rect {
                        b.rect = sel;
                        ed.dirty = true;
                    }
                }
            }
            DragMode::NewBox { start, .. } => {
                let current = Rect::new(
                    start.0.round() as i32,
                    start.1.round() as i32,
                    p.0.round() as i32,
                    p.1.round() as i32,
                )
                .normalized()
                .clamped(img_w, img_h);
                ed.drag = DragMode::NewBox {
                    start: *start,
                    current,
                };
            }
            DragMode::MoveText {
                index,
                grab_dx,
                grab_dy,
            } => {
                if let Some(t) = ed.texts.get_mut(*index) {
                    let nx = (p.0 - *grab_dx).clamp(0.0, img_w as f32);
                    let ny = (p.1 - *grab_dy).clamp(0.0, img_h as f32);
                    if t.x != nx || t.y != ny {
                        t.x = nx;
                        t.y = ny;
                        ed.dirty = true;
                    }
                }
            }
            DragMode::Idle => {}
        }
    }

    if response.drag_stopped() {
        if let DragMode::NewBox { current, .. } = &ed.drag {
            ed.commit_new_box(*current);
        }
        ed.drag = DragMode::Idle;
    }
}

fn paint(
    ui: &mut egui::Ui,
    ed: &Editor,
    view: &View,
    displayed: &[Rect],
    hovered_handle: Option<Handle>,
    preview: Option<Rect>,
) {
    let p = *theme::palette(ui.ctx());
    let painter = ui.painter_at(view.rect);
    let uv_full = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let img_w = ed.img.width() as f32;
    let img_h = ed.img.height() as f32;

    if let Some(tex) = ed.result_tex.as_ref() {
        painter.image(tex.id(), view.rect, uv_full, egui::Color32::WHITE);
    }

    painter.rect_filled(view.rect, 0.0, theme::canvas::DIM);

    let punch = |painter: &egui::Painter, sel: Rect, tex: &egui::TextureHandle| {
        let s = sel.normalized();
        if s.width() < 1 || s.height() < 1 {
            return;
        }
        let sr = view.sel_screen_rect(s);
        let uv = egui::Rect::from_min_max(
            egui::pos2(s.x0 as f32 / img_w, s.y0 as f32 / img_h),
            egui::pos2(s.x1 as f32 / img_w, s.y1 as f32 / img_h),
        );
        painter.image(tex.id(), sr, uv, egui::Color32::WHITE);
    };

    if let Some(tex) = ed.result_tex.as_ref() {
        for (i, (b, sel)) in ed.boxes.iter().zip(displayed.iter()).enumerate() {
            if (i == ed.focus && ed.is_selected_index(i)) || !b.appeared() {
                continue;
            }
            punch(&painter, *sel, tex);
        }
        if ed.is_selected_index(ed.focus)
            && let Some((b, sel)) = ed.boxes.get(ed.focus).zip(displayed.get(ed.focus))
            && b.appeared()
        {
            punch(&painter, *sel, tex);
        }
    }

    // 未聚焦框：次级描边 + 淡轴。聚焦框最后画。
    for (i, (b, sel)) in ed.boxes.iter().zip(displayed.iter()).enumerate() {
        if (i == ed.focus && ed.is_selected_index(i)) || !b.appeared() {
            continue;
        }
        let sr = view.sel_screen_rect(sel.normalized());
        let over = ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|pos| sr.contains(pos));
        let t = theme::hover_t(ui, ui.id().with(("box_h", b.id)), over);
        let selected = ed.is_selected_index(i);
        let base = if selected {
            theme::lerp_color(p.accent_tint, p.accent, 0.55)
        } else {
            theme::lerp_color(p.stroke_control, p.accent, 0.55)
        };
        let stroke = theme::lerp_color(base, p.accent, t);
        painter.rect_stroke(
            sr,
            0.0,
            egui::Stroke::new(1.5 + 0.5 * t, stroke),
            egui::StrokeKind::Inside,
        );
        paint_axis(&painter, sr, b.axis, if selected { 0.65 } else { 0.45 });
        let badge_t = theme::anim::ease_out(ui.ctx().animate_bool_with_time(
            ui.id().with(("badge_vis", b.id)),
            b.show_badge && displayed.len() > 1,
            theme::anim::FAST.as_secs_f32(),
        ));
        if badge_t > 0.01 {
            paint_badge(ui, &painter, sr, b.id, i, selected, p, badge_t);
        }
        let marker_t = theme::anim::ease_out(ui.ctx().animate_bool_with_time(
            ui.id().with(("selection_marker", b.id)),
            selected,
            theme::anim::FAST.as_secs_f32(),
        ));
        if marker_t > 0.01 {
            paint_selection_marker(
                &painter,
                sr,
                b.show_badge && displayed.len() > 1,
                p,
                marker_t,
            );
        }
    }

    if ed.is_selected_index(ed.focus)
        && let Some((b, sel)) = ed.boxes.get(ed.focus).zip(displayed.get(ed.focus))
        && b.appeared()
    {
        let s = sel.normalized();
        let sr = view.sel_screen_rect(s);
        let inside = ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|pos| sr.contains(pos));
        let border_t = theme::hover_t(ui, ui.id().with(("box_border", b.id)), inside);
        let focus_t = theme::hover_t(ui, ui.id().with(("box_focus", b.id)), true);
        let border_color = theme::lerp_color(p.accent, p.accent_hover, border_t);
        painter.rect_stroke(
            sr,
            0.0,
            egui::Stroke::new(2.0 + 1.0 * border_t, border_color),
            egui::StrokeKind::Inside,
        );
        paint_axis(&painter, sr, b.axis, 0.55 + 0.45 * focus_t);
        let badge_t = theme::anim::ease_out(ui.ctx().animate_bool_with_time(
            ui.id().with(("badge_vis", b.id)),
            b.show_badge && displayed.len() > 1,
            theme::anim::FAST.as_secs_f32(),
        ));
        if badge_t > 0.01 {
            paint_badge(ui, &painter, sr, b.id, ed.focus, true, p, badge_t);
        }
        for h in Handle::all() {
            let is_hovered = hovered_handle == Some(h);
            let t = theme::hover_t(ui, ui.id().with(("handle", b.id, h as usize)), is_hovered);
            let grow = 1.0 + 0.45 * t;
            let hr = view.handle_screen_rect(s, h, grow);
            let fill = theme::lerp_color(p.accent, p.accent_hover, t);
            painter.rect_filled(hr, 1.5, fill);
            painter.rect_stroke(
                hr,
                1.5,
                egui::Stroke::new(1.0, p.card),
                egui::StrokeKind::Inside,
            );
            let hit = hr.expand(4.0);
            ui.interact(
                hit,
                ui.id().with(("handle_hit", b.id, h as usize)),
                egui::Sense::hover(),
            )
            .on_hover_cursor(h.cursor());
        }
    }

    if let Some(sel) = preview {
        let sr = view.sel_screen_rect(sel.normalized());
        painter.rect_stroke(
            sr,
            0.0,
            egui::Stroke::new(1.5, p.accent),
            egui::StrokeKind::Inside,
        );
        painter.rect_filled(
            sr,
            0.0,
            theme::with_alpha(p.accent, theme::canvas::PREVIEW_ALPHA),
        );
        paint_axis(&painter, sr, ed.default_axis, 0.7);
    }

    if let Some(font) = overlay_text::system_font() {
        for (i, t) in ed.texts.iter().enumerate() {
            if ed.text_editing && ed.text_focus == Some(i) {
                continue;
            }
            let Some((x0, y0, x1, y1)) = overlay_text::bounds(font, &t.text, t.x, t.y, t.size)
            else {
                continue;
            };
            let sr = egui::Rect::from_min_max(view.to_screen(x0, y0), view.to_screen(x1, y1))
                .expand(3.0);
            let focused = ed.text_focus == Some(i);
            if focused {
                painter.rect_stroke(
                    sr,
                    2.0,
                    egui::Stroke::new(2.0, p.accent),
                    egui::StrokeKind::Outside,
                );
                painter.text(
                    sr.center_top() + egui::vec2(0.0, -4.0),
                    egui::Align2::CENTER_BOTTOM,
                    "拖动移动 · 再点一下编辑",
                    egui::FontId::proportional(12.0),
                    p.accent,
                );
            } else {
                painter.rect_stroke(
                    sr,
                    2.0,
                    egui::Stroke::new(1.0, p.stroke_control),
                    egui::StrokeKind::Outside,
                );
            }
        }
    }
}

fn paint_axis(painter: &egui::Painter, sr: egui::Rect, axis: MirrorAxis, alpha: f32) {
    let dash = 6.0;
    let (white, shadow) = theme::canvas::axis_strokes(alpha);
    match axis {
        MirrorAxis::Horizontal => {
            let axis_x = (sr.min.x + sr.max.x) / 2.0;
            let mut y = sr.min.y;
            while y < sr.max.y {
                let y2 = (y + dash).min(sr.max.y);
                painter.line_segment(
                    [egui::pos2(axis_x + 0.8, y), egui::pos2(axis_x + 0.8, y2)],
                    shadow,
                );
                painter.line_segment([egui::pos2(axis_x, y), egui::pos2(axis_x, y2)], white);
                y += dash * 2.0;
            }
        }
        MirrorAxis::Vertical => {
            let axis_y = (sr.min.y + sr.max.y) / 2.0;
            let mut x = sr.min.x;
            while x < sr.max.x {
                let x2 = (x + dash).min(sr.max.x);
                painter.line_segment(
                    [egui::pos2(x, axis_y + 0.8), egui::pos2(x2, axis_y + 0.8)],
                    shadow,
                );
                painter.line_segment([egui::pos2(x, axis_y), egui::pos2(x2, axis_y)], white);
                x += dash * 2.0;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_badge(
    ui: &egui::Ui,
    painter: &egui::Painter,
    sr: egui::Rect,
    id: u64,
    index: usize,
    focused: bool,
    p: theme::Palette,
    appear: f32,
) {
    let appear = appear.clamp(0.0, 1.0);
    let t = theme::hover_t(ui, ui.id().with(("badge", id)), focused);
    let r = (10.0 + 1.0 * t) * (0.82 + 0.18 * appear);
    let c = sr.min + egui::vec2(r + 4.0, r + 4.0);
    let fade = |c: egui::Color32| theme::with_alpha(c, (appear * 255.0) as u8);
    let fill = fade(if focused {
        p.accent
    } else {
        theme::lerp_color(p.card, p.accent_tint, 0.6)
    });
    let fg = fade(if focused { p.on_accent } else { p.text });
    painter.circle_filled(c, r, fill);
    painter.circle_stroke(
        c,
        r,
        egui::Stroke::new(1.0, fade(if focused { p.card } else { p.accent })),
    );
    painter.text(
        c,
        egui::Align2::CENTER_CENTER,
        format!("{}", index + 1),
        egui::FontId::proportional(11.0),
        fg,
    );
}

fn paint_selection_marker(
    painter: &egui::Painter,
    sr: egui::Rect,
    badge_visible: bool,
    p: theme::Palette,
    appear: f32,
) {
    let appear = appear.clamp(0.0, 1.0);
    if appear <= 0.01 {
        return;
    }
    let size = 15.0 * (0.8 + 0.2 * appear);
    let center = if badge_visible {
        sr.max - egui::vec2(size * 0.7, size * 0.7)
    } else {
        sr.min + egui::vec2(size * 0.7, size * 0.7)
    };
    let rect = egui::Rect::from_center_size(center, egui::vec2(size, size));
    let alpha = (appear * 255.0) as u8;
    let fill = theme::with_alpha(p.accent, alpha);
    let stroke = theme::with_alpha(p.card, alpha);
    painter.rect(
        rect,
        3.0,
        fill,
        egui::Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    let c = rect.center();
    let s = size * 0.7;
    let mark = egui::Stroke::new(1.5, theme::with_alpha(p.on_accent, alpha));
    painter.line_segment(
        [
            egui::pos2(c.x - s * 0.28, c.y),
            egui::pos2(c.x - s * 0.04, c.y + s * 0.22),
        ],
        mark,
    );
    painter.line_segment(
        [
            egui::pos2(c.x - s * 0.04, c.y + s * 0.22),
            egui::pos2(c.x + s * 0.32, c.y - s * 0.24),
        ],
        mark,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_handle_corners_before_edges() {
        let sel = Rect::new(10, 10, 100, 100);
        assert_eq!(hit_handle((10.0, 10.0), sel, 3.0), Some(Handle::TL));
        assert_eq!(hit_handle((99.0, 10.0), sel, 3.0), Some(Handle::TR));
        assert_eq!(hit_handle((10.0, 50.0), sel, 3.0), Some(Handle::L));
        assert_eq!(hit_handle((50.0, 50.0), sel, 3.0), None);
    }

    #[test]
    fn hit_boxes_prefers_focus_then_topmost() {
        let rects = [Rect::new(0, 0, 50, 50), Rect::new(20, 20, 80, 80)];
        assert_eq!(hit_boxes((25.0, 25.0), &rects, 0), Some(0));
        assert_eq!(hit_boxes((25.0, 25.0), &rects, 1), Some(1));
        assert_eq!(hit_boxes((60.0, 60.0), &rects, 0), Some(1));
        assert_eq!(hit_boxes((90.0, 90.0), &rects, 0), None);
    }
}
