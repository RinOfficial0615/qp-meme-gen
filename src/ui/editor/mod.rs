//! 编辑器：状态、工具栏、画布/文字栏的编排。
//! 选框几何在 `core::crop`；命中与绘制在 `canvas`；文字栏在 `overlay`。

mod canvas;
mod overlay;

use std::borrow::Cow;
use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use image::RgbaImage;

use crate::config::DirectionPref;
use crate::core::crop::{self, MIN_BOX};
use crate::core::mirror::{self, Direction, Rect};
use crate::core::text as overlay_text;
use crate::detect::Face;
use crate::ui::theme;
use crate::ui::toast::ToastKind;

use canvas::DragMode;
use overlay::{TEXT_BAR_H, TextColor, TextOverlay, default_text_size};

const ADD_STAGGER: f32 = 0.04;

pub enum EditorRequest {
    None,
    OpenNew,
    GoHome,
    RedetectFace,
    AddBox,
    Toast(ToastKind, String),
}

/// 程序性选框变化（人脸/整图/加框）的过渡动画。
struct BoxAnim {
    from: Rect,
    to: Rect,
    at: Instant,
    delay: f32,
}

struct CropBox {
    id: u64,
    rect: Rect,
    dir_pref: DirectionPref,
    show_original: bool,
    anim: Option<BoxAnim>,
}

impl CropBox {
    fn displayed(&self) -> Rect {
        let Some(a) = &self.anim else {
            return self.rect;
        };
        let t = (a.at.elapsed().as_secs_f32() - a.delay) / theme::anim::NORMAL.as_secs_f32();
        if t <= 0.0 {
            return a.from;
        }
        if t >= 1.0 {
            return a.to;
        }
        let t = theme::anim::ease_out(t);
        Rect::new(
            lerp_i32(a.from.x0, a.to.x0, t),
            lerp_i32(a.from.y0, a.to.y0, t),
            lerp_i32(a.from.x1, a.to.x1, t),
            lerp_i32(a.from.y1, a.to.y1, t),
        )
    }

    fn appeared(&self) -> bool {
        match &self.anim {
            None => true,
            Some(a) => a.at.elapsed().as_secs_f32() >= a.delay,
        }
    }

    fn animating(&self) -> bool {
        self.anim.as_ref().is_some_and(|a| {
            a.at.elapsed().as_secs_f32() - a.delay < theme::anim::NORMAL.as_secs_f32()
        })
    }
}

pub struct Editor {
    pub img: RgbaImage,
    path: Option<PathBuf>,
    boxes: Vec<CropBox>,
    focus: usize,
    next_id: u64,
    /// 整图框选模式：不允许加框。
    full_image: bool,
    /// 最近一次检测结果；`None` = 尚未检测。
    faces: Option<Vec<Face>>,
    default_dir: DirectionPref,
    original_tex: Option<egui::TextureHandle>,
    result_tex: Option<egui::TextureHandle>,
    result_img: Option<RgbaImage>,
    dirty: bool,
    drag: DragMode,
    texts: Vec<TextOverlay>,
    text_focus: Option<usize>,
    text_panel: bool,
    text_draft: String,
    text_draft_size: f32,
    text_draft_color: TextColor,
    text_need_focus: bool,
    /// 正在输入。选中但未进入输入时只显示选框，可拖动改位置。
    text_editing: bool,
    /// 多人框时是否画角上编号。小框会被挡住，可关掉。
    show_badges: bool,
    /// 仅一个框时：复制/保存只保留框内。多框时忽略。
    crop_export: bool,
}

fn lerp_i32(a: i32, b: i32, t: f32) -> i32 {
    (a as f32 + (b as f32 - a as f32) * t).round() as i32
}

fn tiny_of(r: Rect, img_w: i32, img_h: i32) -> Rect {
    let cx = r.center_x().round() as i32;
    let cy = r.center_y().round() as i32;
    Rect::new(cx - 6, cy - 6, cx + 6, cy + 6)
        .normalized()
        .clamped(img_w, img_h)
}

impl Editor {
    pub fn new(
        img: RgbaImage,
        path: Option<PathBuf>,
        dir_pref: DirectionPref,
        crop_export: bool,
    ) -> Self {
        let (img_w, img_h) = (img.width(), img.height());
        let rect = Rect::new(0, 0, img_w as i32, img_h as i32);
        Self {
            img,
            path,
            boxes: vec![CropBox {
                id: 1,
                rect,
                dir_pref,
                show_original: false,
                anim: None,
            }],
            focus: 0,
            next_id: 2,
            full_image: true,
            faces: None,
            default_dir: dir_pref,
            original_tex: None,
            result_tex: None,
            result_img: None,
            dirty: true,
            drag: DragMode::Idle,
            texts: Vec::new(),
            text_focus: None,
            text_panel: false,
            text_draft: String::new(),
            text_draft_size: default_text_size(img_w, img_h),
            text_draft_color: TextColor::White,
            text_need_focus: false,
            text_editing: false,
            show_badges: true,
            crop_export,
        }
    }

    pub fn is_full_image(&self) -> bool {
        self.full_image
    }

    pub fn faces_cached(&self) -> bool {
        self.faces.is_some()
    }

    pub fn set_faces(&mut self, faces: Vec<Face>) {
        self.faces = Some(faces);
    }

    /// 已有检测缓存则按单人/多人重建选框，不跑推理。
    pub fn apply_cached_face_boxes(&mut self, multi: bool) -> bool {
        let Some(faces) = &self.faces else {
            return false;
        };
        let picked: Vec<Face> = if faces.is_empty() {
            Vec::new()
        } else if multi {
            faces.clone()
        } else {
            let primary = faces
                .iter()
                .max_by(|a, b| {
                    a.area()
                        .partial_cmp(&b.area())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .copied()
                .expect("faces 非空");
            vec![primary]
        };
        self.apply_face_boxes(&picked);
        true
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn img_size(&self) -> (i32, i32) {
        (self.img.width() as i32, self.img.height() as i32)
    }

    fn placing_text(&self) -> bool {
        self.text_panel
    }

    fn clamp_text_focus(&mut self) {
        if let Some(i) = self.text_focus
            && i >= self.texts.len()
        {
            self.text_focus = None;
        }
    }

    fn select_text(&mut self, i: usize) {
        if i >= self.texts.len() {
            return;
        }
        self.text_focus = Some(i);
        self.text_panel = true;
        let t = &self.texts[i];
        self.text_draft = t.text.clone();
        self.text_draft_size = t.size;
        self.text_draft_color = t.color;
        self.text_need_focus = false;
        self.text_editing = false;
    }

    #[cfg(test)]
    fn place_text(&mut self, x: f32, y: f32) {
        let text = self.text_draft.trim().to_string();
        if text.is_empty() {
            return;
        }
        let (w, h) = self.img_size();
        let id = self.alloc_id();
        self.texts.push(TextOverlay {
            id,
            text,
            x: x.clamp(0.0, w as f32),
            y: y.clamp(0.0, h as f32),
            size: self.text_draft_size.clamp(16.0, 160.0),
            color: self.text_draft_color,
        });
        self.text_focus = Some(self.texts.len() - 1);
        self.dirty = true;
    }

    fn begin_text_at(&mut self, x: f32, y: f32) {
        self.commit_or_drop_focused();
        let (w, h) = self.img_size();
        let id = self.alloc_id();
        self.texts.push(TextOverlay {
            id,
            text: String::new(),
            x: x.clamp(0.0, w as f32),
            y: y.clamp(0.0, h as f32),
            size: self.text_draft_size.clamp(16.0, 160.0),
            color: self.text_draft_color,
        });
        self.text_focus = Some(self.texts.len() - 1);
        self.text_draft.clear();
        self.text_need_focus = true;
        self.text_editing = true;
        self.text_panel = true;
    }

    fn commit_or_drop_focused(&mut self) {
        let Some(i) = self.text_focus else {
            return;
        };
        if self.texts.get(i).is_some_and(|t| t.text.trim().is_empty()) {
            self.texts.remove(i);
        } else if i < self.texts.len() {
            self.dirty = true;
        }
        self.text_focus = None;
        self.text_need_focus = false;
        self.text_editing = false;
    }

    fn remove_text(&mut self, i: usize) {
        if i < self.texts.len() {
            self.texts.remove(i);
            self.text_focus = None;
            self.text_need_focus = false;
            self.text_editing = false;
            self.dirty = true;
        }
    }

    /// 文字先画到源图，再按选框镜像，效果与照片像素相同。
    fn compose_result(&self) -> RgbaImage {
        self.compose_skipping(None)
    }

    /// 复制/保存用：单框且勾了「仅框选处」时裁到框内，否则整图。
    fn export_image(&self) -> RgbaImage {
        let out = self.compose_result();
        if self.crop_export && self.boxes.len() == 1 {
            mirror::crop_to_rect(&out, self.boxes[0].rect)
        } else {
            out
        }
    }

    fn compose_skipping(&self, skip: Option<usize>) -> RgbaImage {
        let mut src = self.img.clone();
        if let Some(font) = overlay_text::system_font() {
            for (i, t) in self.texts.iter().enumerate() {
                if skip == Some(i) {
                    continue;
                }
                overlay_text::draw(
                    &mut src,
                    font,
                    &t.text,
                    (t.x, t.y),
                    t.size,
                    t.color.fill(),
                    t.color.outline(),
                );
            }
        }
        let mut out = src.clone();
        for b in &self.boxes {
            if !b.appeared() {
                continue;
            }
            let sel = b.displayed();
            if b.show_original {
                mirror::copy_rect(&mut out, &src, sel);
            } else {
                let dir = match b.dir_pref {
                    DirectionPref::Left => Direction::Left,
                    DirectionPref::Right => Direction::Right,
                    DirectionPref::Auto => mirror::auto_direction(&self.img, sel),
                };
                mirror::apply_mirror(&mut out, &src, sel, dir);
            }
        }
        out
    }

    fn tick_anims(&mut self) {
        for b in &mut self.boxes {
            let done = b.anim.as_ref().is_some_and(|a| {
                a.at.elapsed().as_secs_f32() - a.delay >= theme::anim::NORMAL.as_secs_f32()
            });
            if done && let Some(a) = b.anim.take() {
                b.rect = a.to;
            }
        }
    }

    fn any_animating(&self) -> bool {
        self.boxes.iter().any(|b| b.animating())
    }

    fn focused_mut(&mut self) -> &mut CropBox {
        let i = self.focus.min(self.boxes.len().saturating_sub(1));
        &mut self.boxes[i]
    }

    fn clamp_focus(&mut self) {
        if self.boxes.is_empty() {
            return;
        }
        if self.focus >= self.boxes.len() {
            self.focus = self.boxes.len() - 1;
        }
    }

    fn existing_rects(&self) -> Vec<Rect> {
        self.boxes.iter().map(|b| b.rect).collect()
    }

    /// 程序性设置整图单框（带过渡）。
    pub fn apply_full_box(&mut self) {
        let (w, h) = self.img_size();
        let target = Rect::new(0, 0, w, h);
        let from = self
            .boxes
            .get(self.focus)
            .map(|b| b.displayed())
            .unwrap_or(target);
        let dir_pref = self
            .boxes
            .get(self.focus)
            .map(|b| b.dir_pref)
            .unwrap_or(self.default_dir);
        let show_original = self
            .boxes
            .get(self.focus)
            .map(|b| b.show_original)
            .unwrap_or(false);
        let id = self.alloc_id();
        self.boxes.clear();
        self.boxes.push(CropBox {
            id,
            rect: target,
            dir_pref,
            show_original,
            anim: Some(BoxAnim {
                from,
                to: target,
                at: Instant::now(),
                delay: 0.0,
            }),
        });
        self.focus = 0;
        self.full_image = true;
        self.dirty = true;
    }

    /// 用给定人脸重建选框（退出整图模式）。`faces` 为空时在画面中央放一个比例框。
    pub fn apply_face_boxes(&mut self, faces: &[Face]) {
        let (w, h) = self.img_size();
        let now = Instant::now();
        let targets: Vec<Rect> = if faces.is_empty() {
            vec![crop::center_box(w, h)]
        } else {
            faces.iter().map(|f| crop::face_box(f, w, h)).collect()
        };
        self.boxes = targets
            .iter()
            .enumerate()
            .map(|(i, &to)| CropBox {
                id: self.next_id + i as u64,
                rect: to,
                dir_pref: self.default_dir,
                show_original: false,
                anim: Some(BoxAnim {
                    from: tiny_of(to, w, h),
                    to,
                    at: now,
                    delay: i as f32 * ADD_STAGGER,
                }),
            })
            .collect();
        self.next_id += targets.len() as u64;
        self.focus = 0;
        self.full_image = false;
        self.dirty = true;
    }

    /// 加框：未覆盖人脸中 score 最高者；没有则放画面中央。
    pub fn add_box(&mut self) {
        if self.full_image {
            return;
        }
        let (w, h) = self.img_size();
        let existing = self.existing_rects();
        let target = self
            .faces
            .as_deref()
            .and_then(|fs| crop::pick_next_face(fs, &existing))
            .map(|f| crop::face_box(f, w, h))
            .unwrap_or_else(|| crop::center_box(w, h));
        let id = self.alloc_id();
        self.boxes.push(CropBox {
            id,
            rect: target,
            dir_pref: self.default_dir,
            show_original: false,
            anim: Some(BoxAnim {
                from: tiny_of(target, w, h),
                to: target,
                at: Instant::now(),
                delay: 0.0,
            }),
        });
        self.focus = self.boxes.len() - 1;
        self.dirty = true;
    }

    pub fn remove_focused(&mut self) {
        if self.full_image || self.boxes.len() <= 1 {
            return;
        }
        self.boxes.remove(self.focus);
        self.clamp_focus();
        self.dirty = true;
    }

    fn commit_new_box(&mut self, sel: Rect) {
        let (w, h) = self.img_size();
        let sel = sel.normalized().clamped(w, h);
        if !sel.is_mirrorable() || sel.width() < MIN_BOX || sel.height() < MIN_BOX {
            return;
        }
        let id = self.alloc_id();
        self.boxes.push(CropBox {
            id,
            rect: sel,
            dir_pref: self.default_dir,
            show_original: false,
            anim: None,
        });
        self.focus = self.boxes.len() - 1;
        self.dirty = true;
    }

    /// 重新计算镜像结果与纹理。
    pub fn refresh_result(&mut self, ctx: &egui::Context) {
        if self.original_tex.is_none() {
            let ci = egui::ColorImage::from_rgba_unmultiplied(
                [self.img.width() as usize, self.img.height() as usize],
                self.img.as_raw(),
            );
            self.original_tex =
                Some(ctx.load_texture("original", ci, egui::TextureOptions::LINEAR));
        }
        self.tick_anims();
        if !self.dirty && !self.any_animating() && self.result_img.is_some() {
            return;
        }
        let skip = if self.text_editing {
            self.text_focus
        } else {
            None
        };
        let out = self.compose_skipping(skip);
        let ci = egui::ColorImage::from_rgba_unmultiplied(
            [out.width() as usize, out.height() as usize],
            out.as_raw(),
        );
        self.result_tex = Some(ctx.load_texture("result", ci, egui::TextureOptions::LINEAR));
        self.result_img = Some(out);
        self.dirty = false;
    }

    fn save_as(&self) -> EditorRequest {
        let out = self.export_image();
        let default_name = self
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "meme".into());
        let picked = rfd::FileDialog::new()
            .set_file_name(format!("{default_name}_qp.png"))
            .add_filter("图片", &["png", "jpg", "jpeg", "bmp", "webp"])
            .save_file();
        match picked {
            None => EditorRequest::None,
            Some(path) => match out.save(&path) {
                Ok(()) => {
                    EditorRequest::Toast(ToastKind::Success, format!("已保存到 {}", path.display()))
                }
                Err(e) => EditorRequest::Toast(ToastKind::Error, format!("保存失败：{e}")),
            },
        }
    }

    fn copy_to_clipboard(&self) -> EditorRequest {
        let out = self.export_image();
        let data = arboard::ImageData {
            width: out.width() as usize,
            height: out.height() as usize,
            bytes: Cow::Owned(out.into_raw()),
        };
        match arboard::Clipboard::new().and_then(|mut cb| cb.set_image(data)) {
            Ok(()) => EditorRequest::Toast(ToastKind::Success, "已复制到剪贴板".into()),
            Err(e) => EditorRequest::Toast(ToastKind::Error, format!("复制失败：{e}")),
        }
    }
}

pub fn show(ui: &mut egui::Ui, ed: &mut Editor, enter: theme::PageEnter) -> EditorRequest {
    let mut request = EditorRequest::None;
    ed.clamp_focus();

    ed.clamp_text_focus();
    if !ui.ctx().egui_wants_keyboard_input() {
        let del = ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        });
        if del {
            if let Some(i) = ed.text_focus {
                ed.remove_text(i);
            } else if !ed.full_image && ed.boxes.len() > 1 {
                ed.remove_focused();
            }
        }
    }

    egui::Panel::top("editor_toolbar")
        .frame(
            egui::Frame::default()
                .fill(theme::palette(ui.ctx()).subtle)
                .stroke(egui::Stroke::new(
                    1.0,
                    theme::palette(ui.ctx()).stroke_divider,
                ))
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ui, |ui| {
            enter.apply(ui);
            let p = *theme::palette(ui.ctx());
            // 先定行高再铺控件，避免后面更高的分段选择把行撑开、
            // 已经排好的左侧按钮对不齐。
            let bar_h = 32.0;
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), bar_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_height(bar_h);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if ui.button("← 主页").clicked() {
                        request = EditorRequest::GoHome;
                    }
                    if ui.button("打开新图…").clicked() {
                        request = EditorRequest::OpenNew;
                    }
                    ui.separator();

                    ui.label(egui::RichText::new("方向").color(p.text_secondary));
                    let mut dir_changed = false;
                    {
                        let b = ed.focused_mut();
                        if theme::segmented_control(
                            ui,
                            &mut b.dir_pref,
                            &[
                                (DirectionPref::Auto, "自动"),
                                (DirectionPref::Left, "保留左半"),
                                (DirectionPref::Right, "保留右半"),
                            ],
                        ) {
                            dir_changed = true;
                        }
                    }
                    if dir_changed {
                        ed.dirty = true;
                    }
                    ui.separator();

                    if ui.button("人脸框选").clicked() {
                        request = EditorRequest::RedetectFace;
                    }
                    if ui.button("整图框选").clicked() {
                        ed.apply_full_box();
                    }

                    let full = ed.full_image;
                    let add = ui.add_enabled(!full, egui::Button::new("+ 加框"));
                    let add = if full {
                        add.on_disabled_hover_text("整图框选时不能添加选框")
                    } else {
                        add.on_hover_text("按未框选的最高分人脸加框，若无则放在画面中央")
                    };
                    if add.clicked() {
                        request = EditorRequest::AddBox;
                    }

                    let can_del = !ed.full_image && ed.boxes.len() > 1;
                    if can_del {
                        let n = ed.boxes.len();
                        let idx = ed.focus + 1;
                        ui.label(
                            egui::RichText::new(format!("{idx} / {n}")).color(p.text_secondary),
                        );
                        if ui.button("删框").clicked() {
                            ed.remove_focused();
                        }
                    }
                    ui.separator();

                    let mut orig_changed = false;
                    {
                        let b = ed.focused_mut();
                        if theme::accent_checkbox(ui, &mut b.show_original, "查看原图").changed()
                        {
                            orig_changed = true;
                        }
                    }
                    if orig_changed {
                        ed.dirty = true;
                    }
                    let _ = theme::accent_checkbox(ui, &mut ed.show_badges, "显示角标");
                    ui.separator();
                    if theme::toggle_button(ui, ed.text_panel, "文字")
                        .on_hover_text("文字模式：点击图片添加文字")
                        .clicked()
                    {
                        ed.text_panel = !ed.text_panel;
                        if ed.text_panel {
                            if overlay_text::system_font().is_none() {
                                request = EditorRequest::Toast(
                                    ToastKind::Error,
                                    "未找到系统中文字体，无法绘制文字".into(),
                                );
                            }
                        } else {
                            ed.commit_or_drop_focused();
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::accent_button(ui, "保存图片").clicked() {
                            request = ed.save_as();
                        }
                        if ui.button("复制图片").clicked() {
                            request = ed.copy_to_clipboard();
                        }
                        if ed.boxes.len() == 1 {
                            let _ = theme::accent_checkbox(ui, &mut ed.crop_export, "仅框选处")
                                .on_hover_text("复制和保存只保留选框内的画面");
                        }
                    });
                },
            );
            let bar_t = theme::anim::ease_out(ui.ctx().animate_bool_with_time(
                ui.id().with("text_bar_open"),
                ed.text_panel,
                theme::anim::NORMAL.as_secs_f32(),
            ));
            if bar_t > 0.001 {
                // allocate_ui 会按子控件 min_rect 再撑高，面板瞬间跳满、只有内部被 clip。
                // 先占死动画高度，子 UI 画进这块，整栏（含工具栏底）一起长。
                ui.spacing_mut().item_spacing.y = 0.0;
                let h = (TEXT_BAR_H * bar_t).max(1.0);
                let (rect, _) = ui
                    .allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
                let mut bar_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("text_bar")
                        .max_rect(rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                bar_ui.set_clip_rect(rect);
                bar_ui.multiply_opacity(bar_t);
                bar_ui.separator();
                bar_ui.add_space(4.0);
                overlay::show_text_bar(&mut bar_ui, ed);
            }
        });

    egui::CentralPanel::default().show(ui, |ui| {
        enter.apply(ui);
        canvas::show(ui, ed);
    });

    request
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::Point2;

    fn face(bbox: [f32; 4], score: f32) -> Face {
        Face {
            bbox,
            keypoints: {
                let mut k = [Point2::default(); 5];
                k[0] = Point2 {
                    x: (bbox[0] + bbox[2]) * 0.4,
                    y: bbox[1] + 20.0,
                };
                k[1] = Point2 {
                    x: (bbox[0] + bbox[2]) * 0.6,
                    y: bbox[1] + 20.0,
                };
                k
            },
            score,
        }
    }

    #[test]
    fn face_select_without_faces_uses_center_not_full() {
        let img = RgbaImage::new(200, 100);
        let mut ed = Editor::new(img, None, DirectionPref::Auto, true);
        assert!(ed.is_full_image());
        ed.apply_face_boxes(&[]);
        assert!(!ed.is_full_image());
        assert_eq!(ed.boxes.len(), 1);
        assert_eq!(ed.boxes[0].rect, crop::center_box(200, 100));
    }

    #[test]
    fn face_boxes_roundtrip_reuses_cache() {
        let img = RgbaImage::new(200, 200);
        let mut ed = Editor::new(img, None, DirectionPref::Auto, true);
        let f_hi = face([20.0, 20.0, 60.0, 60.0], 0.99);
        let f_lo = face([120.0, 20.0, 160.0, 60.0], 0.7);
        ed.apply_face_boxes(&[f_hi, f_lo]);
        ed.set_faces(vec![f_hi, f_lo]);
        let rects: Vec<_> = ed.boxes.iter().map(|b| b.rect).collect();
        ed.apply_full_box();
        assert!(ed.is_full_image());
        assert!(ed.faces_cached());
        assert!(ed.apply_cached_face_boxes(true));
        assert!(!ed.is_full_image());
        assert_eq!(ed.boxes.len(), 2);
        assert_eq!(ed.boxes[0].rect, rects[0]);
        assert_eq!(ed.boxes[1].rect, rects[1]);
    }

    #[test]
    fn cached_face_boxes_single_picks_largest() {
        let img = RgbaImage::new(200, 200);
        let mut ed = Editor::new(img, None, DirectionPref::Auto, true);
        let f_hi = face([20.0, 20.0, 80.0, 80.0], 0.9);
        let f_lo = face([120.0, 20.0, 150.0, 50.0], 0.8);
        ed.set_faces(vec![f_hi, f_lo]);
        assert!(ed.apply_cached_face_boxes(false));
        assert_eq!(ed.boxes.len(), 1);
        assert_eq!(ed.boxes[0].rect, crop::face_box(&f_hi, 200, 200));
    }

    #[test]
    fn apply_cached_face_boxes_noop_without_cache() {
        let img = RgbaImage::new(80, 80);
        let mut ed = Editor::new(img, None, DirectionPref::Auto, true);
        assert!(!ed.faces_cached());
        assert!(!ed.apply_cached_face_boxes(true));
        assert!(ed.is_full_image());
    }

    #[test]
    fn full_image_rejects_add_box() {
        let img = RgbaImage::new(80, 80);
        let mut ed = Editor::new(img, None, DirectionPref::Auto, true);
        assert!(ed.is_full_image());
        assert_eq!(ed.boxes.len(), 1);
        ed.add_box();
        assert_eq!(ed.boxes.len(), 1);
    }

    #[test]
    fn add_box_picks_highest_unused_then_center() {
        let img = RgbaImage::new(200, 200);
        let mut ed = Editor::new(img, None, DirectionPref::Auto, true);
        let f_hi = face([20.0, 20.0, 60.0, 60.0], 0.99);
        let f_lo = face([120.0, 20.0, 160.0, 60.0], 0.7);
        ed.apply_face_boxes(&[f_hi]);
        ed.set_faces(vec![f_hi, f_lo]);
        assert!(!ed.is_full_image());
        assert_eq!(ed.boxes.len(), 1);
        ed.add_box();
        assert_eq!(ed.boxes.len(), 2);
        assert_eq!(ed.boxes[1].rect, crop::face_box(&f_lo, 200, 200));
        ed.add_box();
        assert_eq!(ed.boxes.len(), 3);
        assert_eq!(ed.boxes[2].rect, crop::center_box(200, 200));
    }

    #[test]
    fn per_box_direction_and_original_are_independent() {
        let img = RgbaImage::new(120, 120);
        let mut ed = Editor::new(img, None, DirectionPref::Auto, true);
        ed.apply_face_boxes(&[
            face([10.0, 10.0, 50.0, 50.0], 0.9),
            face([70.0, 10.0, 110.0, 50.0], 0.8),
        ]);
        assert_eq!(ed.boxes.len(), 2);
        ed.boxes[0].dir_pref = DirectionPref::Left;
        ed.boxes[0].show_original = true;
        ed.boxes[1].dir_pref = DirectionPref::Right;
        ed.boxes[1].show_original = false;
        ed.focus = 0;
        assert_eq!(ed.boxes[ed.focus].dir_pref, DirectionPref::Left);
        assert!(ed.boxes[ed.focus].show_original);
        ed.focus = 1;
        assert_eq!(ed.boxes[ed.focus].dir_pref, DirectionPref::Right);
        assert!(!ed.boxes[ed.focus].show_original);
    }

    #[test]
    fn place_text_at_center_and_delete() {
        let img = RgbaImage::new(80, 40);
        let mut ed = Editor::new(img, None, DirectionPref::Left, true);
        ed.text_draft = "强".into();
        ed.place_text(20.0, 10.0);
        assert_eq!(ed.texts.len(), 1);
        assert_eq!(ed.texts[0].x, 20.0);
        assert_eq!(ed.text_focus, Some(0));
        ed.remove_text(0);
        assert!(ed.texts.is_empty());
        assert!(ed.text_focus.is_none());
    }

    #[test]
    fn begin_text_empty_drops_on_commit() {
        let img = RgbaImage::new(80, 40);
        let mut ed = Editor::new(img, None, DirectionPref::Left, true);
        ed.begin_text_at(20.0, 10.0);
        assert_eq!(ed.texts.len(), 1);
        assert!(ed.texts[0].text.is_empty());
        assert!(ed.text_panel);
        ed.commit_or_drop_focused();
        assert!(ed.texts.is_empty());
        assert!(ed.text_focus.is_none());
    }

    #[test]
    fn select_text_keeps_it_and_allows_move() {
        let img = RgbaImage::new(80, 40);
        let mut ed = Editor::new(img, None, DirectionPref::Left, true);
        ed.text_draft = "强".into();
        ed.place_text(20.0, 10.0);
        ed.commit_or_drop_focused();
        assert_eq!(ed.texts.len(), 1);
        assert!(ed.text_focus.is_none());
        ed.select_text(0);
        assert_eq!(ed.text_focus, Some(0));
        assert!(!ed.text_editing);
        ed.texts[0].x = 50.0;
        ed.texts[0].y = 18.0;
        assert_eq!(ed.texts[0].x, 50.0);
        assert_eq!(ed.texts[0].y, 18.0);
        assert_eq!(ed.texts[0].text, "强");
    }

    #[test]
    fn selected_text_color_shows_in_compose_without_editing() {
        let Some(_) = overlay_text::system_font() else {
            eprintln!("skip: no system CJK font");
            return;
        };
        let img = RgbaImage::from_pixel(80, 80, image::Rgba([0, 0, 0, 255]));
        let mut ed = Editor::new(img, None, DirectionPref::Left, true);
        ed.boxes[0].dir_pref = DirectionPref::Left;
        ed.boxes[0].show_original = true;
        ed.text_draft = "Q".into();
        ed.text_draft_size = 36.0;
        ed.text_draft_color = TextColor::White;
        ed.place_text(40.0, 40.0);
        ed.select_text(0);
        assert!(!ed.text_editing);
        let white = ed.compose_result();
        ed.texts[0].color = TextColor::Yellow;
        let yellow = ed.compose_result();
        assert_ne!(
            white.as_raw(),
            yellow.as_raw(),
            "选中未编辑时改颜色应出现在合成图上"
        );
    }

    #[test]
    fn overlay_text_is_mirrored_like_pixels() {
        let Some(_) = overlay_text::system_font() else {
            eprintln!("skip: no system CJK font");
            return;
        };
        let img = RgbaImage::from_pixel(100, 50, image::Rgba([0, 0, 0, 255]));
        let mut ed = Editor::new(img, None, DirectionPref::Left, true);
        ed.boxes[0].dir_pref = DirectionPref::Left;
        ed.text_draft = "Q".into();
        ed.text_draft_size = 28.0;
        ed.place_text(25.0, 25.0);
        let out = ed.compose_result();
        let left = (0..50u32)
            .flat_map(|y| (0..50u32).map(move |x| (x, y)))
            .filter(|&(x, y)| out.get_pixel(x, y).0[0] > 20)
            .count();
        assert!(left > 10, "text should paint the kept half, got {left} px");
        for y in 0..50u32 {
            for x in 50..100u32 {
                let sx = 99 - x;
                assert_eq!(
                    out.get_pixel(x, y),
                    out.get_pixel(sx, y),
                    "pixel ({x},{y}) should mirror ({sx},{y})"
                );
            }
        }
    }

    #[test]
    fn export_crops_single_box_when_enabled() {
        let img = RgbaImage::from_fn(100, 80, |x, y| image::Rgba([x as u8, y as u8, 0, 255]));
        let mut ed = Editor::new(img, None, DirectionPref::Left, true);
        ed.apply_face_boxes(&[]);
        assert_eq!(ed.boxes.len(), 1);
        assert!(ed.crop_export);
        let r = ed.boxes[0].rect;
        let full = ed.compose_result();
        let out = ed.export_image();
        assert_eq!(out.width() as i32, r.width());
        assert_eq!(out.height() as i32, r.height());
        assert_eq!(
            out.get_pixel(0, 0),
            full.get_pixel(r.x0 as u32, r.y0 as u32)
        );
    }

    #[test]
    fn export_keeps_full_when_crop_disabled() {
        let img = RgbaImage::new(100, 80);
        let mut ed = Editor::new(img, None, DirectionPref::Left, false);
        ed.apply_face_boxes(&[]);
        let out = ed.export_image();
        assert_eq!(out.dimensions(), (100, 80));
    }

    #[test]
    fn export_keeps_full_with_multiple_boxes() {
        let img = RgbaImage::new(200, 200);
        let mut ed = Editor::new(img, None, DirectionPref::Auto, true);
        ed.apply_face_boxes(&[
            face([10.0, 10.0, 50.0, 50.0], 0.9),
            face([70.0, 10.0, 110.0, 50.0], 0.8),
        ]);
        assert_eq!(ed.boxes.len(), 2);
        assert!(ed.crop_export);
        let out = ed.export_image();
        assert_eq!(out.dimensions(), (200, 200));
    }
}
