//! 编辑器：画布渲染、多选框交互（含过渡动画）、实时预览、保存/剪贴板。

use std::borrow::Cow;
use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use image::RgbaImage;

use crate::config::DirectionPref;
use crate::core::mirror::{self, Direction, Rect};
use crate::detect::Face;
use crate::ui::theme;
use crate::ui::toast::ToastKind;

const MIN_BOX: i32 = 4;
const FACE_IOU: f32 = 0.3;
const ADD_STAGGER: f32 = 0.04;
/// 无人脸时中央框占画面宽、高的比例。
const CENTER_BOX_FRAC: f32 = 0.4;

pub enum EditorRequest {
    None,
    OpenNew,
    GoHome,
    RedetectFace,
    AddBox,
    Toast(ToastKind, String),
}

/// 框的 8 个手柄。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Handle {
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

#[derive(Clone, Copy, PartialEq, Debug)]
enum DragMode {
    Idle,
    Move {
        index: usize,
        grab_dx: f32,
        grab_dy: f32,
    },
    Resize {
        index: usize,
        handle: Handle,
    },
    NewBox {
        start: (f32, f32),
        current: Rect,
    },
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

/// 画面中央的默认新框：宽、高各为画面的 40%。
pub fn center_box(w: i32, h: i32) -> Rect {
    let bw = ((w as f32) * CENTER_BOX_FRAC)
        .round()
        .clamp(MIN_BOX as f32, w.max(MIN_BOX) as f32) as i32;
    let bh = ((h as f32) * CENTER_BOX_FRAC)
        .round()
        .clamp(MIN_BOX as f32, h.max(MIN_BOX) as f32) as i32;
    let x0 = ((w - bw) / 2).max(0);
    let y0 = ((h - bh) / 2).max(0);
    Rect::new(x0, y0, x0 + bw, y0 + bh).clamped(w, h)
}

fn face_as_rect(face: &Face) -> Rect {
    Rect::new(
        face.bbox[0] as i32,
        face.bbox[1] as i32,
        face.bbox[2] as i32,
        face.bbox[3] as i32,
    )
    .normalized()
}

fn face_covered(face: &Face, boxes: &[Rect]) -> bool {
    let cx = ((face.bbox[0] + face.bbox[2]) * 0.5) as i32;
    let cy = ((face.bbox[1] + face.bbox[3]) * 0.5) as i32;
    let fb = face_as_rect(face);
    boxes
        .iter()
        .any(|b| b.contains(cx, cy) || b.iou(fb) > FACE_IOU)
}

/// 未覆盖人脸中 score 最高者。
pub fn pick_next_face<'a>(faces: &'a [Face], boxes: &[Rect]) -> Option<&'a Face> {
    faces
        .iter()
        .filter(|f| !face_covered(f, boxes))
        .max_by(|a, b| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

impl Editor {
    pub fn new(img: RgbaImage, path: Option<PathBuf>, dir_pref: DirectionPref) -> Self {
        let rect = Rect::new(0, 0, img.width() as i32, img.height() as i32);
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

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn img_size(&self) -> (i32, i32) {
        (self.img.width() as i32, self.img.height() as i32)
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
            vec![center_box(w, h)]
        } else {
            faces.iter().map(|f| face_box(f, w, h)).collect()
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
            .and_then(|fs| pick_next_face(fs, &existing))
            .map(|f| face_box(f, w, h))
            .unwrap_or_else(|| center_box(w, h));
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
        let mut out = self.img.clone();
        for b in &self.boxes {
            if !b.appeared() {
                continue;
            }
            let sel = b.displayed();
            if b.show_original {
                mirror::copy_rect(&mut out, &self.img, sel);
            } else {
                let dir = match b.dir_pref {
                    DirectionPref::Left => Direction::Left,
                    DirectionPref::Right => Direction::Right,
                    DirectionPref::Auto => mirror::auto_direction(&self.img, sel),
                };
                mirror::apply_mirror(&mut out, &self.img, sel, dir);
            }
        }
        let ci = egui::ColorImage::from_rgba_unmultiplied(
            [out.width() as usize, out.height() as usize],
            out.as_raw(),
        );
        self.result_tex = Some(ctx.load_texture("result", ci, egui::TextureOptions::LINEAR));
        self.result_img = Some(out);
        self.dirty = false;
    }

    fn save_as(&self) -> EditorRequest {
        let Some(out) = &self.result_img else {
            return EditorRequest::Toast(ToastKind::Error, "没有可保存的结果".into());
        };
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
        let Some(out) = &self.result_img else {
            return EditorRequest::Toast(ToastKind::Error, "没有可复制的结果".into());
        };
        let data = arboard::ImageData {
            width: out.width() as usize,
            height: out.height() as usize,
            bytes: Cow::Borrowed(out.as_raw()),
        };
        match arboard::Clipboard::new().and_then(|mut cb| cb.set_image(data)) {
            Ok(()) => EditorRequest::Toast(ToastKind::Success, "已复制到剪贴板".into()),
            Err(e) => EditorRequest::Toast(ToastKind::Error, format!("复制失败：{e}")),
        }
    }
}

/// 由人脸检测结果构造选框：双眼中点为轴心，左右对称扩张覆盖整脸并留边距。
pub fn face_box(face: &Face, img_w: i32, img_h: i32) -> Rect {
    let [x0, y0, x1, y1] = face.bbox;
    let (bw, bh) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    let eye_mid = (face.keypoints[0].x + face.keypoints[1].x) / 2.0;
    let axis = if eye_mid.is_finite() && eye_mid > x0 && eye_mid < x1 {
        eye_mid
    } else {
        (x0 + x1) / 2.0
    };
    let half = ((axis - x0).max(x1 - axis) + bw * 0.1).max(bw * 0.5);
    Rect::new(
        (axis - half) as i32,
        (y0 - bh * 0.15) as i32,
        (axis + half) as i32,
        (y1 + bh * 0.05) as i32,
    )
    .normalized()
    .clamped(img_w, img_h)
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

pub fn show(ui: &mut egui::Ui, ed: &mut Editor, enter: theme::PageEnter) -> EditorRequest {
    let mut request = EditorRequest::None;
    ed.clamp_focus();

    if !ed.full_image && ed.boxes.len() > 1 {
        let del = ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        });
        if del {
            ed.remove_focused();
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

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::accent_button(ui, "保存图片").clicked() {
                            request = ed.save_as();
                        }
                        if ui.button("复制图片").clicked() {
                            request = ed.copy_to_clipboard();
                        }
                    });
                },
            );
        });

    egui::CentralPanel::default().show(ui, |ui| {
        enter.apply(ui);
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
                if hit_boxes(pimg, &displayed, focus).is_some() {
                    egui::CursorIcon::Move
                } else if ed.full_image {
                    egui::CursorIcon::Default
                } else {
                    egui::CursorIcon::Crosshair
                }
            });
        let response = response.on_hover_cursor(canvas_cursor);

        handle_drag(ed, &view, &response, &displayed);

        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let p = view.to_image(pos);
            if let Some(i) = hit_boxes(p, &displayed, focus) {
                ed.focus = i;
            }
        }

        let hovered_handle = if ed.drag == DragMode::Idle {
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

        let preview = match ed.drag {
            DragMode::NewBox { current, .. } => Some(current),
            _ => None,
        };
        paint(ui, ed, &view, &displayed, hovered_handle, preview);
    });

    request
}

fn handle_drag(ed: &mut Editor, view: &View, response: &egui::Response, displayed: &[Rect]) {
    let (img_w, img_h) = ed.img_size();
    let focus = ed.focus.min(displayed.len().saturating_sub(1));

    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let p = view.to_image(pos);
        let tol = 10.0 / view.scale;
        if let Some(sel) = displayed.get(focus).copied() {
            if let Some(h) = hit_handle(p, sel, tol) {
                let b = &mut ed.boxes[focus];
                b.anim = None;
                b.rect = sel;
                ed.drag = DragMode::Resize {
                    index: focus,
                    handle: h,
                };
            } else if let Some(i) = hit_boxes(p, displayed, focus) {
                ed.focus = i;
                let sel_i = displayed[i];
                let b = &mut ed.boxes[i];
                b.anim = None;
                b.rect = sel_i;
                ed.drag = DragMode::Move {
                    index: i,
                    grab_dx: p.0 - sel_i.x0 as f32,
                    grab_dy: p.1 - sel_i.y0 as f32,
                };
            } else if !ed.full_image {
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
    }

    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let p = view.to_image(pos);
        match ed.drag {
            DragMode::Move {
                index,
                grab_dx,
                grab_dy,
            } => {
                if let Some(b) = ed.boxes.get_mut(index) {
                    let mut sel = b.rect.normalized();
                    let (w, h) = (sel.width(), sel.height());
                    let nx0 = (p.0 - grab_dx).round().clamp(0.0, (img_w - w) as f32) as i32;
                    let ny0 = (p.1 - grab_dy).round().clamp(0.0, (img_h - h) as f32) as i32;
                    sel = Rect::new(nx0, ny0, nx0 + w, ny0 + h);
                    if sel != b.rect {
                        b.rect = sel;
                        ed.dirty = true;
                    }
                }
            }
            DragMode::Resize { index, handle } => {
                if let Some(b) = ed.boxes.get_mut(index) {
                    let mut sel = b.rect.normalized();
                    let (px, py) = (p.0.round() as i32, p.1.round() as i32);
                    match handle {
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
                ed.drag = DragMode::NewBox { start, current };
            }
            DragMode::Idle => {}
        }
    }

    if response.drag_stopped() {
        if let DragMode::NewBox { current, .. } = ed.drag {
            ed.commit_new_box(current);
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

    let dim = egui::Color32::from_black_alpha(70);
    painter.rect_filled(view.rect, 0.0, dim);

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
            if i == ed.focus || !b.appeared() {
                continue;
            }
            punch(&painter, *sel, tex);
        }
        if let Some((b, sel)) = ed.boxes.get(ed.focus).zip(displayed.get(ed.focus))
            && b.appeared()
        {
            punch(&painter, *sel, tex);
        }
    }

    // 未聚焦框：次级描边 + 淡轴。聚焦框最后画。
    for (i, (b, sel)) in ed.boxes.iter().zip(displayed.iter()).enumerate() {
        if i == ed.focus || !b.appeared() {
            continue;
        }
        let sr = view.sel_screen_rect(sel.normalized());
        let over = ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|pos| sr.contains(pos));
        let t = theme::hover_t(ui, ui.id().with(("box_h", b.id)), over);
        let stroke = theme::lerp_color(
            theme::lerp_color(p.stroke_control, p.accent, 0.55),
            p.accent,
            t,
        );
        painter.rect_stroke(
            sr,
            0.0,
            egui::Stroke::new(1.5 + 0.5 * t, stroke),
            egui::StrokeKind::Inside,
        );
        paint_axis(&painter, sr, 0.45);
        if ed.boxes.len() > 1 {
            paint_badge(ui, &painter, sr, b.id, i, false, p);
        }
    }

    if let Some((b, sel)) = ed.boxes.get(ed.focus).zip(displayed.get(ed.focus))
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
        paint_axis(&painter, sr, 0.55 + 0.45 * focus_t);
        if ed.boxes.len() > 1 {
            paint_badge(ui, &painter, sr, b.id, ed.focus, true, p);
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
        let fill =
            egui::Color32::from_rgba_unmultiplied(p.accent.r(), p.accent.g(), p.accent.b(), 28);
        painter.rect_filled(sr, 0.0, fill);
        paint_axis(&painter, sr, 0.7);
    }
}

fn paint_axis(painter: &egui::Painter, sr: egui::Rect, alpha: f32) {
    let axis_x = (sr.min.x + sr.max.x) / 2.0;
    let dash = 6.0;
    let mut y = sr.min.y;
    let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
    let white = egui::Stroke::new(1.5, egui::Color32::from_white_alpha(a));
    let shadow = egui::Stroke::new(1.5, egui::Color32::from_black_alpha((90.0 * alpha) as u8));
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

fn paint_badge(
    ui: &egui::Ui,
    painter: &egui::Painter,
    sr: egui::Rect,
    id: u64,
    index: usize,
    focused: bool,
    p: theme::Palette,
) {
    let t = theme::hover_t(ui, ui.id().with(("badge", id)), focused);
    let r = 10.0 + 1.0 * t;
    let c = sr.min + egui::vec2(r + 4.0, r + 4.0);
    let fill = if focused {
        p.accent
    } else {
        theme::lerp_color(p.card, p.accent_tint, 0.6)
    };
    let fg = if focused { p.on_accent } else { p.text };
    painter.circle_filled(c, r, fill);
    painter.circle_stroke(
        c,
        r,
        egui::Stroke::new(1.0, if focused { p.card } else { p.accent }),
    );
    painter.text(
        c,
        egui::Align2::CENTER_CENTER,
        format!("{}", index + 1),
        egui::FontId::proportional(11.0),
        fg,
    );
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
    fn face_box_symmetric_around_eyes() {
        let face = Face {
            bbox: [100.0, 50.0, 300.0, 250.0],
            keypoints: {
                let mut k = [Point2::default(); 5];
                k[0] = Point2 { x: 150.0, y: 120.0 };
                k[1] = Point2 { x: 250.0, y: 120.0 };
                k
            },
            score: 0.9,
        };
        let r = face_box(&face, 400, 400);
        // 轴 = 双眼中点 200；框必须关于 200 对称
        assert_eq!(r.x0 + r.x1, 400);
        // 覆盖整脸并留边距
        assert!(r.x0 <= 100 && r.x1 >= 300);
        // 顶部多扩：y0 < 50
        assert!(r.y0 < 50);
    }

    #[test]
    fn face_box_clamped_to_image() {
        let face = Face {
            bbox: [0.0, 0.0, 100.0, 100.0],
            keypoints: {
                let mut k = [Point2::default(); 5];
                k[0] = Point2 { x: 30.0, y: 40.0 };
                k[1] = Point2 { x: 70.0, y: 40.0 };
                k
            },
            score: 0.9,
        };
        let r = face_box(&face, 200, 200);
        assert!(r.x0 >= 0 && r.y0 >= 0 && r.x1 <= 200 && r.y1 <= 200);
        assert!(r.is_mirrorable());
    }

    #[test]
    fn hit_handle_corners_before_edges() {
        let sel = Rect::new(10, 10, 100, 100);
        assert_eq!(hit_handle((10.0, 10.0), sel, 3.0), Some(Handle::TL));
        assert_eq!(hit_handle((99.0, 10.0), sel, 3.0), Some(Handle::TR));
        assert_eq!(hit_handle((10.0, 50.0), sel, 3.0), Some(Handle::L));
        assert_eq!(hit_handle((50.0, 50.0), sel, 3.0), None);
    }

    #[test]
    fn pick_next_face_highest_unused_score() {
        let faces = [
            face([10.0, 10.0, 40.0, 40.0], 0.6),
            face([80.0, 10.0, 120.0, 50.0], 0.95),
            face([10.0, 80.0, 50.0, 120.0], 0.8),
        ];
        let used = [Rect::new(70, 0, 130, 60)];
        let next = pick_next_face(&faces, &used).unwrap();
        assert!((next.score - 0.8).abs() < f32::EPSILON);
        assert_eq!(pick_next_face(&faces, &[]).unwrap().score, 0.95);
        let all_used = [
            Rect::new(0, 0, 50, 50),
            Rect::new(70, 0, 130, 60),
            Rect::new(0, 70, 60, 130),
        ];
        assert!(pick_next_face(&faces, &all_used).is_none());
    }

    #[test]
    fn center_box_is_centered() {
        let r = center_box(200, 100);
        assert_eq!(r.center_x(), 100.0);
        assert_eq!(r.center_y(), 50.0);
        assert_eq!(r.width(), 80);
        assert_eq!(r.height(), 40);
        assert!(r.x0 >= 0 && r.x1 <= 200 && r.y0 >= 0 && r.y1 <= 100);
    }

    #[test]
    fn face_select_without_faces_uses_center_not_full() {
        let img = RgbaImage::new(200, 100);
        let mut ed = Editor::new(img, None, DirectionPref::Auto);
        assert!(ed.is_full_image());
        ed.apply_face_boxes(&[]);
        assert!(!ed.is_full_image());
        assert_eq!(ed.boxes.len(), 1);
        assert_eq!(ed.boxes[0].rect, center_box(200, 100));
    }

    #[test]
    fn hit_boxes_prefers_focus_then_topmost() {
        let rects = [Rect::new(0, 0, 50, 50), Rect::new(20, 20, 80, 80)];
        assert_eq!(hit_boxes((25.0, 25.0), &rects, 0), Some(0));
        assert_eq!(hit_boxes((25.0, 25.0), &rects, 1), Some(1));
        assert_eq!(hit_boxes((60.0, 60.0), &rects, 0), Some(1));
        assert_eq!(hit_boxes((90.0, 90.0), &rects, 0), None);
    }

    #[test]
    fn full_image_rejects_add_box() {
        let img = RgbaImage::new(80, 80);
        let mut ed = Editor::new(img, None, DirectionPref::Auto);
        assert!(ed.is_full_image());
        assert_eq!(ed.boxes.len(), 1);
        ed.add_box();
        assert_eq!(ed.boxes.len(), 1);
    }

    #[test]
    fn add_box_picks_highest_unused_then_center() {
        let img = RgbaImage::new(200, 200);
        let mut ed = Editor::new(img, None, DirectionPref::Auto);
        let f_hi = face([20.0, 20.0, 60.0, 60.0], 0.99);
        let f_lo = face([120.0, 20.0, 160.0, 60.0], 0.7);
        ed.apply_face_boxes(&[f_hi]);
        ed.set_faces(vec![f_hi, f_lo]);
        assert!(!ed.is_full_image());
        assert_eq!(ed.boxes.len(), 1);
        ed.add_box();
        assert_eq!(ed.boxes.len(), 2);
        assert_eq!(ed.boxes[1].rect, face_box(&f_lo, 200, 200));
        ed.add_box();
        assert_eq!(ed.boxes.len(), 3);
        assert_eq!(ed.boxes[2].rect, center_box(200, 200));
    }

    #[test]
    fn per_box_direction_and_original_are_independent() {
        let img = RgbaImage::new(120, 120);
        let mut ed = Editor::new(img, None, DirectionPref::Auto);
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
}
