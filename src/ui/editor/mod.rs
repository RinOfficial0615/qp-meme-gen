//! 编辑器：状态、多选工具栏、画布/文字栏的编排。
//! 选框几何在 `core::crop`；命中与绘制在 `canvas`；文字栏在 `overlay`。

mod canvas;
mod overlay;

use std::borrow::Cow;
use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use image::RgbaImage;

use crate::core::crop::{self, MIN_BOX};
use crate::core::mirror::{self, KeepSide, MirrorAxis, Rect};
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
    axis: MirrorAxis,
    keep_side: KeepSide,
    show_original: bool,
    show_badge: bool,
    anim: Option<BoxAnim>,
}

#[derive(Clone, PartialEq)]
struct BoxSnapshot {
    id: u64,
    rect: Rect,
    axis: MirrorAxis,
    keep_side: KeepSide,
    show_original: bool,
    show_badge: bool,
}

#[derive(Clone, PartialEq)]
struct TextSnapshot {
    id: u64,
    text: String,
    x: f32,
    y: f32,
    size: f32,
    color: TextColor,
}

#[derive(Clone, PartialEq)]
pub(super) struct EditorSnapshot {
    boxes: Vec<BoxSnapshot>,
    focus: usize,
    selected_ids: Vec<u64>,
    next_id: u64,
    full_image: bool,
    texts: Vec<TextSnapshot>,
    crop_export: bool,
}

const HISTORY_LIMIT: usize = 100;

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
    selected_ids: Vec<u64>,
    next_id: u64,
    /// 整图框选模式：不允许加框。
    full_image: bool,
    /// 最近一次检测结果；`None` = 尚未检测。
    faces: Option<Vec<Face>>,
    default_axis: MirrorAxis,
    default_keep_side: KeepSide,
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
    /// 仅一个框时：复制/保存只保留框内。多框时忽略。
    crop_export: bool,
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
    history_gesture: Option<EditorSnapshot>,
    history_gesture_text_continuation: bool,
    text_history_before: Option<EditorSnapshot>,
    text_style_history_before: Option<EditorSnapshot>,
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
        axis: MirrorAxis,
        keep_side: KeepSide,
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
                axis,
                keep_side: keep_side.normalized_for_axis(axis),
                show_original: false,
                show_badge: true,
                anim: None,
            }],
            focus: 0,
            selected_ids: vec![1],
            next_id: 2,
            full_image: true,
            faces: None,
            default_axis: axis,
            default_keep_side: keep_side.normalized_for_axis(axis),
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
            crop_export,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            history_gesture: None,
            history_gesture_text_continuation: false,
            text_history_before: None,
            text_style_history_before: None,
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

    pub(super) fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            boxes: self
                .boxes
                .iter()
                .map(|b| BoxSnapshot {
                    id: b.id,
                    rect: b.rect,
                    axis: b.axis,
                    keep_side: b.keep_side,
                    show_original: b.show_original,
                    show_badge: b.show_badge,
                })
                .collect(),
            focus: self.focus,
            selected_ids: self.selected_ids.clone(),
            next_id: self.next_id,
            full_image: self.full_image,
            texts: self
                .texts
                .iter()
                .map(|t| TextSnapshot {
                    id: t.id,
                    text: t.text.clone(),
                    x: t.x,
                    y: t.y,
                    size: t.size,
                    color: t.color,
                })
                .collect(),
            crop_export: self.crop_export,
        }
    }

    fn snapshot_changes_result(a: &EditorSnapshot, b: &EditorSnapshot) -> bool {
        a.full_image != b.full_image
            || a.boxes.len() != b.boxes.len()
            || a.boxes.iter().zip(&b.boxes).any(|(x, y)| {
                x.id != y.id
                    || x.rect != y.rect
                    || x.axis != y.axis
                    || x.keep_side != y.keep_side
                    || x.show_original != y.show_original
            })
            || a.texts != b.texts
    }

    fn restore_snapshot(&mut self, snapshot: EditorSnapshot) {
        let current = self.snapshot();
        let result_changed = Self::snapshot_changes_result(&current, &snapshot);
        self.boxes = snapshot
            .boxes
            .into_iter()
            .map(|b| CropBox {
                id: b.id,
                rect: b.rect,
                axis: b.axis,
                keep_side: b.keep_side,
                show_original: b.show_original,
                show_badge: b.show_badge,
                anim: None,
            })
            .collect();
        self.focus = snapshot.focus;
        self.selected_ids = snapshot.selected_ids;
        self.next_id = snapshot.next_id;
        self.full_image = snapshot.full_image;
        self.texts = snapshot
            .texts
            .into_iter()
            .map(|t| TextOverlay {
                id: t.id,
                text: t.text,
                x: t.x,
                y: t.y,
                size: t.size,
                color: t.color,
            })
            .collect();
        self.crop_export = snapshot.crop_export;
        self.drag = DragMode::Idle;
        self.text_focus = None;
        self.text_need_focus = false;
        self.text_editing = false;
        self.text_history_before = None;
        self.text_style_history_before = None;
        self.history_gesture = None;
        self.history_gesture_text_continuation = false;
        if result_changed {
            self.result_tex = None;
            self.result_img = None;
            self.dirty = true;
        }
        self.clamp_focus();
    }

    fn commit_history_before(&mut self, before: EditorSnapshot) {
        let after = self.snapshot();
        if before == after {
            return;
        }
        self.undo_stack.push(before);
        if self.undo_stack.len() > HISTORY_LIMIT {
            let excess = self.undo_stack.len() - HISTORY_LIMIT;
            self.undo_stack.drain(0..excess);
        }
        self.redo_stack.clear();
    }

    fn record_change(&mut self, f: impl FnOnce(&mut Self)) {
        if self.history_gesture.is_some() {
            f(self);
            return;
        }
        let before = self.snapshot();
        f(self);
        self.commit_history_before(before);
    }

    pub(super) fn begin_history_gesture(&mut self) {
        if self.history_gesture.is_none() {
            self.history_gesture_text_continuation = self.text_history_before.is_some();
            self.history_gesture = Some(
                self.text_history_before
                    .take()
                    .unwrap_or_else(|| self.snapshot()),
            );
        }
    }

    pub(super) fn finish_history_gesture(&mut self) {
        if let Some(before) = self.history_gesture.take() {
            self.commit_history_before(before);
            if self.history_gesture_text_continuation && self.text_editing {
                self.text_history_before = Some(self.snapshot());
            }
        }
        self.history_gesture_text_continuation = false;
    }

    pub(super) fn cancel_history_gesture(&mut self) {
        if let Some(before) = self.history_gesture.take() {
            self.restore_snapshot(before);
        }
        self.history_gesture_text_continuation = false;
    }

    pub(super) fn prepare_history_command(&mut self) {
        if self.history_gesture.is_some() {
            self.cancel_history_gesture();
        }
        if self.text_history_before.is_some() {
            self.commit_or_drop_focused();
        }
        self.finish_text_style_history();
    }

    pub(super) fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub(super) fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub(crate) fn reset_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.history_gesture = None;
        self.history_gesture_text_continuation = false;
        self.text_history_before = None;
        self.text_style_history_before = None;
    }

    pub(super) fn undo(&mut self) {
        self.prepare_history_command();
        let Some(previous) = self.undo_stack.pop() else {
            return;
        };
        self.redo_stack.push(self.snapshot());
        self.restore_snapshot(previous);
    }

    pub(super) fn redo(&mut self) {
        self.prepare_history_command();
        let Some(next) = self.redo_stack.pop() else {
            return;
        };
        self.undo_stack.push(self.snapshot());
        self.restore_snapshot(next);
    }

    pub(super) fn begin_text_style_history(&mut self, frame_before: EditorSnapshot) {
        if self.text_style_history_before.is_none() {
            self.text_style_history_before =
                Some(self.text_history_before.clone().unwrap_or(frame_before));
        }
    }

    pub(super) fn finish_text_style_history(&mut self) {
        if let Some(before) = self.text_style_history_before.take() {
            self.commit_history_before(before);
            // 文字仍在编辑时，保留样式提交后的新基线，后续输入文字继续作为
            // 下一条历史，而不是因为样式操作消费掉整段编辑的起点。
            if self.text_editing && self.text_history_before.is_some() {
                self.text_history_before = Some(self.snapshot());
            }
        }
    }

    fn selected_id_set_contains(&self, id: u64) -> bool {
        self.selected_ids.contains(&id)
    }

    pub(super) fn is_selected_index(&self, index: usize) -> bool {
        self.boxes
            .get(index)
            .is_some_and(|b| self.selected_id_set_contains(b.id))
    }

    pub(super) fn selected_count(&self) -> usize {
        self.selected_indices().len()
    }

    pub(super) fn has_selection(&self) -> bool {
        self.selected_count() > 0
    }

    /// 返回按画面层级顺序排列的当前选中框索引。
    fn selected_indices(&self) -> Vec<usize> {
        self.boxes
            .iter()
            .enumerate()
            .filter_map(|(i, b)| self.selected_id_set_contains(b.id).then_some(i))
            .collect()
    }

    fn clear_selection_raw(&mut self) {
        self.selected_ids.clear();
    }

    pub(super) fn clear_selection(&mut self) {
        self.record_change(|ed| ed.clear_selection_raw());
    }

    fn select_only_raw(&mut self, index: usize) {
        if index >= self.boxes.len() {
            return;
        }
        self.selected_ids.clear();
        self.selected_ids.push(self.boxes[index].id);
        self.focus = index;
    }

    pub(super) fn select_only(&mut self, index: usize) {
        self.record_change(|ed| ed.select_only_raw(index));
    }

    pub(super) fn toggle_selection(&mut self, index: usize) {
        self.record_change(|ed| ed.toggle_selection_raw(index));
    }

    fn toggle_selection_raw(&mut self, index: usize) {
        if index >= self.boxes.len() {
            return;
        }
        let id = self.boxes[index].id;
        if let Some(pos) = self
            .selected_ids
            .iter()
            .position(|&selected| selected == id)
        {
            self.selected_ids.remove(pos);
            if self.selected_ids.is_empty() {
                self.focus = index;
            } else if self.focus == index {
                self.focus = self
                    .boxes
                    .iter()
                    .position(|b| self.selected_id_set_contains(b.id))
                    .unwrap_or(self.focus);
            }
        } else {
            self.selected_ids.push(id);
            self.focus = index;
        }
    }

    fn select_all_raw(&mut self) {
        self.selected_ids = self.boxes.iter().map(|b| b.id).collect();
        if !self.boxes.is_empty() {
            self.focus = self.focus.min(self.boxes.len() - 1);
        }
    }

    pub(super) fn select_all(&mut self) {
        self.record_change(|ed| ed.select_all_raw());
    }

    fn selection_same<T: Copy + PartialEq>(&self, f: impl Fn(&CropBox) -> T) -> Option<T> {
        let mut it = self.selected_indices().into_iter();
        let first = it.next().and_then(|i| self.boxes.get(i)).map(&f)?;
        if it.all(|i| self.boxes.get(i).is_some_and(|b| f(b) == first)) {
            Some(first)
        } else {
            None
        }
    }

    fn selection_bool_state(&self, f: impl Fn(&CropBox) -> bool) -> Option<bool> {
        let indices = self.selected_indices();
        let first = indices.first().and_then(|&i| self.boxes.get(i)).map(&f)?;
        if indices
            .iter()
            .all(|&i| self.boxes.get(i).is_some_and(|b| f(b) == first))
        {
            Some(first)
        } else {
            None
        }
    }

    pub(super) fn selected_axis(&self) -> Option<MirrorAxis> {
        self.selection_same(|b| b.axis)
    }

    pub(super) fn selected_keep_side(&self) -> Option<KeepSide> {
        self.selection_same(|b| b.keep_side)
    }

    pub(super) fn selected_show_original(&self) -> Option<bool> {
        self.selection_bool_state(|b| b.show_original)
    }

    pub(super) fn selected_show_badge(&self) -> Option<bool> {
        self.selection_bool_state(|b| b.show_badge)
    }

    fn apply_axis_to_selection_raw(&mut self, axis: MirrorAxis) {
        let ids = self.selected_ids.clone();
        for b in &mut self.boxes {
            if ids.contains(&b.id) {
                b.axis = axis;
                b.keep_side = b.keep_side.normalized_for_axis(axis);
            }
        }
        self.dirty = true;
    }

    pub(super) fn apply_axis_to_selection(&mut self, axis: MirrorAxis) {
        self.record_change(|ed| ed.apply_axis_to_selection_raw(axis));
    }

    fn apply_keep_side_to_selection_raw(&mut self, side: KeepSide) {
        let ids = self.selected_ids.clone();
        for b in &mut self.boxes {
            if ids.contains(&b.id) {
                b.keep_side = side.normalized_for_axis(b.axis);
            }
        }
        self.dirty = true;
    }

    pub(super) fn apply_keep_side_to_selection(&mut self, side: KeepSide) {
        self.record_change(|ed| ed.apply_keep_side_to_selection_raw(side));
    }

    fn apply_show_original_to_selection_raw(&mut self, value: bool) {
        let ids = self.selected_ids.clone();
        for b in &mut self.boxes {
            if ids.contains(&b.id) {
                b.show_original = value;
            }
        }
        self.dirty = true;
    }

    pub(super) fn apply_show_original_to_selection(&mut self, value: bool) {
        self.record_change(|ed| ed.apply_show_original_to_selection_raw(value));
    }

    fn apply_show_badge_to_selection_raw(&mut self, value: bool) {
        let ids = self.selected_ids.clone();
        for b in &mut self.boxes {
            if ids.contains(&b.id) {
                b.show_badge = value;
            }
        }
    }

    pub(super) fn apply_show_badge_to_selection(&mut self, value: bool) {
        self.record_change(|ed| ed.apply_show_badge_to_selection_raw(value));
    }

    pub(super) fn set_crop_export(&mut self, value: bool) {
        self.record_change(|ed| ed.crop_export = value);
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
        self.text_history_before = Some(self.snapshot());
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

    pub(super) fn begin_text_editing(&mut self) {
        if self.text_history_before.is_none() {
            self.text_history_before = Some(self.snapshot());
        }
        self.text_editing = true;
        self.text_need_focus = true;
        self.text_panel = true;
    }

    fn commit_or_drop_focused(&mut self) {
        self.finish_text_style_history();
        let before = self.text_history_before.take();
        let Some(i) = self.text_focus else {
            if let Some(before) = before {
                self.commit_history_before(before);
            }
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
        if let Some(before) = before {
            self.commit_history_before(before);
        }
    }

    fn remove_text_raw(&mut self, i: usize) {
        if i < self.texts.len() {
            self.texts.remove(i);
            self.text_focus = None;
            self.text_need_focus = false;
            self.text_editing = false;
            self.dirty = true;
        }
    }

    pub(super) fn remove_text(&mut self, i: usize) {
        if self.text_history_before.is_some() {
            self.remove_text_raw(i);
            let before = self.text_history_before.take();
            if let Some(before) = before {
                self.commit_history_before(before);
            }
        } else {
            self.record_change(|ed| ed.remove_text_raw(i));
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
                mirror::apply_mirror_with_axis(&mut out, &src, sel, b.axis, b.keep_side);
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

    fn clamp_focus(&mut self) {
        if self.boxes.is_empty() {
            return;
        }
        if self.focus >= self.boxes.len() {
            self.focus = self.boxes.len() - 1;
        }
        self.selected_ids
            .retain(|id| self.boxes.iter().any(|b| b.id == *id));
    }

    fn existing_rects(&self) -> Vec<Rect> {
        self.boxes.iter().map(|b| b.rect).collect()
    }

    /// 程序性设置整图单框（带过渡）。
    fn apply_full_box_raw(&mut self) {
        let (w, h) = self.img_size();
        let target = Rect::new(0, 0, w, h);
        let from = self
            .boxes
            .get(self.focus)
            .map(|b| b.displayed())
            .unwrap_or(target);
        let axis = self
            .boxes
            .get(self.focus)
            .map(|b| b.axis)
            .unwrap_or(self.default_axis);
        let keep_side = self
            .boxes
            .get(self.focus)
            .map(|b| b.keep_side)
            .unwrap_or(self.default_keep_side);
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
            axis,
            keep_side: keep_side.normalized_for_axis(axis),
            show_original,
            show_badge: true,
            anim: Some(BoxAnim {
                from,
                to: target,
                at: Instant::now(),
                delay: 0.0,
            }),
        });
        self.select_only_raw(0);
        self.full_image = true;
        self.dirty = true;
    }

    pub fn apply_full_box(&mut self) {
        self.record_change(|ed| ed.apply_full_box_raw());
    }

    /// 用给定人脸重建选框（退出整图模式）。`faces` 为空时在画面中央放一个比例框。
    fn apply_face_boxes_raw(&mut self, faces: &[Face]) {
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
                axis: self.default_axis,
                keep_side: self
                    .default_keep_side
                    .normalized_for_axis(self.default_axis),
                show_original: false,
                show_badge: true,
                anim: Some(BoxAnim {
                    from: tiny_of(to, w, h),
                    to,
                    at: now,
                    delay: i as f32 * ADD_STAGGER,
                }),
            })
            .collect();
        self.next_id += targets.len() as u64;
        self.selected_ids = self.boxes.first().map(|b| vec![b.id]).unwrap_or_default();
        self.focus = 0;
        self.full_image = false;
        self.dirty = true;
    }

    pub fn apply_face_boxes(&mut self, faces: &[Face]) {
        self.record_change(|ed| ed.apply_face_boxes_raw(faces));
    }

    /// 加框：未覆盖人脸中 score 最高者；没有则放画面中央。
    fn add_box_raw(&mut self) {
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
            axis: self.default_axis,
            keep_side: self
                .default_keep_side
                .normalized_for_axis(self.default_axis),
            show_original: false,
            show_badge: true,
            anim: Some(BoxAnim {
                from: tiny_of(target, w, h),
                to: target,
                at: Instant::now(),
                delay: 0.0,
            }),
        });
        self.select_only_raw(self.boxes.len() - 1);
        self.dirty = true;
    }

    pub fn add_box(&mut self) {
        self.record_change(|ed| ed.add_box_raw());
    }

    pub(super) fn can_remove_selection(&self) -> bool {
        !self.full_image && self.boxes.len() > 1 && self.has_selection()
    }

    fn remove_selected_raw(&mut self) {
        if !self.can_remove_selection() {
            return;
        }
        let selected = self.selected_ids.clone();
        // 只有全选时才保留焦点框，确保至少留一个；单选焦点框也必须能被删除。
        let keep_id = (selected.len() == self.boxes.len())
            .then(|| self.boxes.get(self.focus).map(|b| b.id))
            .flatten();
        self.boxes
            .retain(|b| !selected.contains(&b.id) || Some(b.id) == keep_id);
        self.clamp_focus();
        if self.boxes.is_empty() {
            return;
        }
        self.focus = self.focus.min(self.boxes.len() - 1);
        self.select_only_raw(self.focus);
        self.dirty = true;
    }

    pub(super) fn remove_selected(&mut self) {
        self.record_change(|ed| ed.remove_selected_raw());
    }

    fn remove_focused_raw(&mut self) {
        if self.full_image || self.boxes.len() <= 1 {
            return;
        }
        self.boxes.remove(self.focus);
        self.clamp_focus();
        if !self.boxes.is_empty() {
            self.select_only_raw(self.focus);
        }
        self.dirty = true;
    }

    pub fn remove_focused(&mut self) {
        self.record_change(|ed| ed.remove_focused_raw());
    }

    fn commit_new_box(&mut self, sel: Rect) {
        let (w, h) = self.img_size();
        let sel = sel.normalized().clamped(w, h);
        if !sel.is_mirrorable_for(self.default_axis)
            || sel.width() < MIN_BOX
            || sel.height() < MIN_BOX
        {
            return;
        }
        let id = self.alloc_id();
        self.boxes.push(CropBox {
            id,
            rect: sel,
            axis: self.default_axis,
            keep_side: self
                .default_keep_side
                .normalized_for_axis(self.default_axis),
            show_original: false,
            show_badge: true,
            anim: None,
        });
        self.select_only_raw(self.boxes.len() - 1);
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
    let wants_keyboard = ui.ctx().egui_wants_keyboard_input();
    let mut history_command = false;
    if !wants_keyboard {
        let (undo, redo) = ui.input_mut(|i| {
            // 先匹配更具体的 Ctrl+Shift+Z，否则 consume_key 的逻辑匹配会
            // 把它当成普通 Ctrl+Z 消耗掉。
            let redo = i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::Z)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Y);
            let undo = !redo && i.consume_key(egui::Modifiers::CTRL, egui::Key::Z);
            (undo, redo)
        });
        if undo {
            ed.cancel_history_gesture();
            ed.drag = DragMode::Idle;
            ed.undo();
            history_command = true;
        } else if redo {
            ed.cancel_history_gesture();
            ed.drag = DragMode::Idle;
            ed.redo();
            history_command = true;
        }
    }
    if !wants_keyboard && !history_command {
        let (select_all, clear, delete) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::CTRL, egui::Key::A),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace),
            )
        });
        if select_all {
            ed.select_all();
        }
        if clear {
            ed.clear_selection();
        }
        if delete {
            if let Some(i) = ed.text_focus {
                ed.remove_text(i);
            } else {
                ed.remove_selected();
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

                    if ui.button("←").on_hover_text("主页").clicked() {
                        request = EditorRequest::GoHome;
                    }
                    let undo = ui
                        .add_enabled(
                            ed.can_undo(),
                            egui::Button::new(egui::RichText::new("↶").size(16.0)),
                        )
                        .on_hover_text("撤销 Ctrl+Z");
                    if undo.clicked() {
                        ed.undo();
                    }
                    let redo = ui
                        .add_enabled(
                            ed.can_redo(),
                            egui::Button::new(egui::RichText::new("↷").size(16.0)),
                        )
                        .on_hover_text("重做 Ctrl+Y");
                    if redo.clicked() {
                        ed.redo();
                    }
                    if ui.button("打开").on_hover_text("打开新图").clicked() {
                        request = EditorRequest::OpenNew;
                    }
                    ui.separator();

                    let box_count =
                        egui::RichText::new(format!("{}/{}", ed.selected_count(), ed.boxes.len()))
                            .color(p.text_secondary);

                    ui.label(egui::RichText::new("镜像").color(p.text_secondary));
                    let axis = ed.selected_axis();
                    if let Some(picked) = theme::segmented_control_optional(
                        ui,
                        axis,
                        &[
                            (MirrorAxis::Horizontal, "水平"),
                            (MirrorAxis::Vertical, "垂直"),
                        ],
                        ed.has_selection(),
                    ) {
                        ed.apply_axis_to_selection(picked);
                    }

                    let axis = ed.selected_axis();
                    let axis_for_labels = axis.unwrap_or(ed.default_axis);
                    let side_options = match axis_for_labels {
                        MirrorAxis::Horizontal => [
                            (KeepSide::Auto, "自动"),
                            (KeepSide::Left, "左"),
                            (KeepSide::Right, "右"),
                        ],
                        MirrorAxis::Vertical => [
                            (KeepSide::Auto, "自动"),
                            (KeepSide::Top, "上"),
                            (KeepSide::Bottom, "下"),
                        ],
                    };
                    ui.label(egui::RichText::new("保留").color(p.text_secondary));
                    if let Some(picked) = theme::segmented_control_optional(
                        ui,
                        axis.and_then(|_| ed.selected_keep_side()),
                        &side_options,
                        axis.is_some() && ed.has_selection(),
                    ) {
                        ed.apply_keep_side_to_selection(picked);
                    }
                    ui.separator();

                    if ui.button("人脸").on_hover_text("人脸框选").clicked() {
                        request = EditorRequest::RedetectFace;
                    }
                    if ui.button("整图").on_hover_text("整图框选").clicked() {
                        ed.apply_full_box();
                    }

                    let full = ed.full_image;
                    let add = ui.add_enabled(!full, egui::Button::new("+"));
                    let add = if full {
                        add.on_disabled_hover_text("整图框选时不能添加选框")
                    } else {
                        add.on_hover_text("按未框选的最高分人脸加框，若无则放在画面中央")
                    };
                    if add.clicked() {
                        request = EditorRequest::AddBox;
                    }

                    ui.label(box_count);
                    let del = ui
                        .add_enabled(!full && ed.can_remove_selection(), egui::Button::new("−"))
                        .on_hover_text("删除选中框");
                    if del.clicked() {
                        ed.remove_selected();
                    }
                    ui.separator();

                    let mut original_state = if ed.has_selection() {
                        ed.selected_show_original()
                    } else {
                        Some(false)
                    };
                    let original_resp = theme::tristate_checkbox(
                        ui,
                        &mut original_state,
                        "原图",
                        ed.has_selection(),
                    );
                    if original_resp.changed()
                        && let Some(value) = original_state
                    {
                        ed.apply_show_original_to_selection(value);
                    }
                    let mut badge_state = if ed.has_selection() {
                        ed.selected_show_badge()
                    } else {
                        Some(false)
                    };
                    let badge_resp =
                        theme::tristate_checkbox(ui, &mut badge_state, "角标", ed.has_selection())
                            .on_hover_text("每个选框单独显示角上编号；混合状态点击后全部开启");
                    if badge_resp.changed()
                        && let Some(value) = badge_state
                    {
                        ed.apply_show_badge_to_selection(value);
                    }
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
                        if theme::accent_button(ui, "保存")
                            .on_hover_text("保存图片")
                            .clicked()
                        {
                            request = ed.save_as();
                        }
                        if ui.button("复制").on_hover_text("复制图片").clicked() {
                            request = ed.copy_to_clipboard();
                        }
                        let crop_enabled = ed.boxes.len() == 1;
                        let mut crop_export = ed.crop_export;
                        let crop_resp = theme::accent_checkbox_enabled(
                            ui,
                            &mut crop_export,
                            "仅选框",
                            crop_enabled,
                        );
                        let crop_resp = if crop_enabled {
                            crop_resp.on_hover_text("复制和保存只保留选框内的画面")
                        } else {
                            crop_resp.on_disabled_hover_text("多个选框时输出整图，设置会保留")
                        };
                        if crop_enabled && crop_resp.changed() {
                            ed.set_crop_export(crop_export);
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
        canvas::show(ui, ed, history_command);
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
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
        assert!(ed.is_full_image());
        ed.apply_face_boxes(&[]);
        assert!(!ed.is_full_image());
        assert_eq!(ed.boxes.len(), 1);
        assert_eq!(ed.boxes[0].rect, crop::center_box(200, 100));
    }

    #[test]
    fn face_boxes_roundtrip_reuses_cache() {
        let img = RgbaImage::new(200, 200);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
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
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
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
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
        assert!(!ed.faces_cached());
        assert!(!ed.apply_cached_face_boxes(true));
        assert!(ed.is_full_image());
    }

    #[test]
    fn full_image_rejects_add_box() {
        let img = RgbaImage::new(80, 80);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
        assert!(ed.is_full_image());
        assert_eq!(ed.boxes.len(), 1);
        ed.add_box();
        assert_eq!(ed.boxes.len(), 1);
    }

    #[test]
    fn add_box_picks_highest_unused_then_center() {
        let img = RgbaImage::new(200, 200);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
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
    fn per_box_axis_and_original_are_independent() {
        let img = RgbaImage::new(120, 120);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
        ed.apply_face_boxes(&[
            face([10.0, 10.0, 50.0, 50.0], 0.9),
            face([70.0, 10.0, 110.0, 50.0], 0.8),
        ]);
        assert_eq!(ed.boxes.len(), 2);
        ed.boxes[0].axis = MirrorAxis::Horizontal;
        ed.boxes[0].keep_side = KeepSide::Left;
        ed.boxes[0].show_original = true;
        ed.boxes[1].axis = MirrorAxis::Horizontal;
        ed.boxes[1].keep_side = KeepSide::Right;
        ed.boxes[1].show_original = false;
        ed.focus = 0;
        assert_eq!(ed.boxes[ed.focus].keep_side, KeepSide::Left);
        assert!(ed.boxes[ed.focus].show_original);
        ed.focus = 1;
        assert_eq!(ed.boxes[ed.focus].keep_side, KeepSide::Right);
        assert!(!ed.boxes[ed.focus].show_original);
    }

    #[test]
    fn multi_selection_requires_explicit_bulk_value() {
        let img = RgbaImage::new(160, 100);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
        ed.apply_face_boxes(&[
            face([10.0, 10.0, 50.0, 50.0], 0.9),
            face([90.0, 10.0, 130.0, 50.0], 0.8),
        ]);
        assert_eq!(ed.selected_count(), 1);
        ed.toggle_selection(1);
        assert_eq!(ed.selected_count(), 2);
        ed.boxes[0].keep_side = KeepSide::Left;
        ed.boxes[1].keep_side = KeepSide::Right;
        assert_eq!(ed.selected_keep_side(), None);
        ed.apply_keep_side_to_selection(KeepSide::Right);
        assert_eq!(ed.boxes[0].keep_side, KeepSide::Right);
        assert_eq!(ed.boxes[1].keep_side, KeepSide::Right);

        ed.boxes[0].show_badge = true;
        ed.boxes[1].show_badge = false;
        assert_eq!(ed.selected_show_badge(), None);
        ed.apply_show_badge_to_selection(true);
        assert_eq!(
            ed.boxes.iter().map(|b| b.show_badge).collect::<Vec<_>>(),
            [true, true]
        );
    }

    #[test]
    fn select_all_and_delete_keep_one_box() {
        let img = RgbaImage::new(160, 100);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
        ed.apply_face_boxes(&[
            face([10.0, 10.0, 50.0, 50.0], 0.9),
            face([90.0, 10.0, 130.0, 50.0], 0.8),
        ]);
        ed.select_all();
        assert_eq!(ed.selected_count(), 2);
        ed.remove_selected();
        assert_eq!(ed.boxes.len(), 1);
        assert_eq!(ed.selected_count(), 1);
    }

    #[test]
    fn switching_axis_preserves_relative_keep_side() {
        let img = RgbaImage::new(100, 100);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Left, true);
        ed.apply_axis_to_selection(MirrorAxis::Vertical);
        assert_eq!(ed.boxes[0].axis, MirrorAxis::Vertical);
        assert_eq!(ed.boxes[0].keep_side, KeepSide::Top);
    }

    #[test]
    fn selection_history_restores_focus_and_redoes() {
        let img = RgbaImage::new(160, 100);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
        ed.apply_face_boxes(&[
            face([10.0, 10.0, 50.0, 50.0], 0.9),
            face([90.0, 10.0, 130.0, 50.0], 0.8),
        ]);
        ed.reset_history();
        ed.dirty = false;
        ed.select_only(1);
        assert_eq!(ed.focus, 1);
        assert_eq!(ed.selected_ids, vec![ed.boxes[1].id]);
        assert!(ed.can_undo());
        ed.undo();
        assert_eq!(ed.focus, 0);
        assert_eq!(ed.selected_ids, vec![ed.boxes[0].id]);
        assert!(
            !ed.dirty,
            "selection-only undo should not invalidate the result"
        );
        ed.redo();
        assert_eq!(ed.focus, 1);
        assert_eq!(ed.selected_ids, vec![ed.boxes[1].id]);
    }

    #[test]
    fn add_box_undo_restores_previous_selection_and_redo_readds_box() {
        let img = RgbaImage::new(160, 100);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
        ed.apply_face_boxes(&[
            face([10.0, 10.0, 50.0, 50.0], 0.9),
            face([90.0, 10.0, 130.0, 50.0], 0.8),
        ]);
        ed.reset_history();
        let previous_ids: Vec<_> = ed.boxes.iter().map(|b| b.id).collect();
        ed.add_box();
        assert_eq!(ed.boxes.len(), 3);
        let added_id = ed.boxes[2].id;
        assert_eq!(ed.selected_ids, vec![added_id]);
        ed.undo();
        assert_eq!(
            ed.boxes.iter().map(|b| b.id).collect::<Vec<_>>(),
            previous_ids
        );
        assert_eq!(ed.selected_ids, vec![ed.boxes[0].id]);
        ed.redo();
        assert_eq!(ed.boxes.len(), 3);
        assert_eq!(ed.selected_ids, vec![added_id]);
    }

    #[test]
    fn new_selection_change_clears_redo_stack() {
        let img = RgbaImage::new(160, 100);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
        ed.apply_face_boxes(&[
            face([10.0, 10.0, 50.0, 50.0], 0.9),
            face([90.0, 10.0, 130.0, 50.0], 0.8),
        ]);
        ed.reset_history();
        ed.select_only(1);
        ed.undo();
        assert!(ed.can_redo());
        ed.select_only(1);
        assert!(!ed.can_redo());
    }

    #[test]
    fn text_style_history_keeps_followup_edit_baseline() {
        let img = RgbaImage::new(100, 80);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
        ed.text_draft = "x".into();
        ed.begin_text_at(50.0, 40.0);
        ed.texts[0].text = "a".into();
        ed.begin_text_style_history(ed.snapshot());
        ed.texts[0].size = 48.0;
        ed.finish_text_style_history();
        ed.texts[0].text.push('b');
        ed.commit_or_drop_focused();

        ed.undo();
        assert_eq!(ed.texts[0].text, "a");
        assert_eq!(ed.texts[0].size, 48.0);
        ed.undo();
        assert!(ed.texts.is_empty());
    }

    #[test]
    fn focused_box_delete_is_undoable() {
        let img = RgbaImage::new(160, 100);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
        ed.apply_face_boxes(&[
            face([10.0, 10.0, 50.0, 50.0], 0.9),
            face([90.0, 10.0, 130.0, 50.0], 0.8),
        ]);
        ed.reset_history();
        ed.select_only(1);
        ed.remove_focused();
        assert_eq!(ed.boxes.len(), 1);
        ed.undo();
        assert_eq!(ed.boxes.len(), 2);
        assert_eq!(ed.focus, 1);
        assert_eq!(ed.selected_count(), 1);
    }

    #[test]
    fn selected_box_delete_removes_focus_box() {
        let img = RgbaImage::new(160, 100);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
        ed.apply_face_boxes(&[
            face([10.0, 10.0, 50.0, 50.0], 0.9),
            face([90.0, 10.0, 130.0, 50.0], 0.8),
        ]);
        ed.reset_history();
        ed.select_only(1);
        ed.remove_selected();
        assert_eq!(ed.boxes.len(), 1);
        assert_eq!(ed.selected_count(), 1);
        assert_eq!(ed.focus, 0);
    }

    #[test]
    fn place_text_at_center_and_delete() {
        let img = RgbaImage::new(80, 40);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Left, true);
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
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Left, true);
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
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Left, true);
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
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Left, true);
        ed.boxes[0].axis = MirrorAxis::Horizontal;
        ed.boxes[0].keep_side = KeepSide::Left;
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
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Left, true);
        ed.boxes[0].axis = MirrorAxis::Horizontal;
        ed.boxes[0].keep_side = KeepSide::Left;
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
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Left, true);
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
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Left, false);
        ed.apply_face_boxes(&[]);
        let out = ed.export_image();
        assert_eq!(out.dimensions(), (100, 80));
    }

    #[test]
    fn export_keeps_full_with_multiple_boxes() {
        let img = RgbaImage::new(200, 200);
        let mut ed = Editor::new(img, None, MirrorAxis::Horizontal, KeepSide::Auto, true);
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
