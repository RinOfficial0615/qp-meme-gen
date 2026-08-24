//! 叠加文字：颜色、工具栏第二行。画布上的内联输入框在 `canvas`。

use eframe::egui;

use crate::ui::theme;

use super::Editor;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum TextColor {
    White,
    Black,
    Yellow,
    Red,
}

impl TextColor {
    pub(super) fn fill(self) -> [u8; 3] {
        match self {
            Self::White => [255, 255, 255],
            Self::Black => [20, 20, 20],
            Self::Yellow => [255, 224, 48],
            Self::Red => [232, 17, 35],
        }
    }

    pub(super) fn outline(self) -> [u8; 3] {
        match self {
            Self::Black => [255, 255, 255],
            _ => [0, 0, 0],
        }
    }

    pub(super) fn preview(self) -> egui::Color32 {
        let [r, g, b] = self.fill();
        egui::Color32::from_rgb(r, g, b)
    }
}

pub(super) struct TextOverlay {
    pub(super) id: u64,
    pub(super) text: String,
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) size: f32,
    pub(super) color: TextColor,
}

pub(super) fn default_text_size(img_w: u32, img_h: u32) -> f32 {
    ((img_w.min(img_h) as f32) * 0.1).clamp(32.0, 120.0)
}

/// 文字栏第二行，高度与主工具栏同一套 32px 行。
const TEXT_ROW_H: f32 = 32.0;
/// 顶部分割线 6 + 行间距 4 + 内容行 32。
pub(super) const TEXT_BAR_H: f32 = 42.0;

pub(super) fn show_text_bar(ui: &mut egui::Ui, ed: &mut Editor) {
    let p = *theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.set_height(TEXT_ROW_H);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("字号")
                .size(14.0)
                .color(p.text_secondary),
        );
        ui.allocate_ui_with_layout(
            egui::vec2(260.0, 28.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().slider_width = 180.0;
                let slider = ui.scope(|ui| {
                    let rail = theme::lerp_color(p.subtle, p.text, 0.38);
                    ui.visuals_mut().widgets.inactive.bg_fill = rail;
                    ui.visuals_mut().widgets.inactive.weak_bg_fill = rail;
                    ui.add(
                        egui::Slider::new(&mut ed.text_draft_size, 16.0..=160.0)
                            .integer()
                            .show_value(false)
                            .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.45 })
                            .clamping(egui::SliderClamping::Always),
                    )
                });
                let value = ui.add(
                    egui::DragValue::new(&mut ed.text_draft_size)
                        .range(16.0..=160.0)
                        .speed(1.0)
                        .max_decimals(0),
                );
                if (slider.inner.changed() || value.changed())
                    && let Some(i) = ed.text_focus
                    && let Some(t) = ed.texts.get_mut(i)
                {
                    t.size = ed.text_draft_size;
                    ed.dirty = true;
                }
            },
        );

        ui.separator();

        if theme::segmented_control(
            ui,
            &mut ed.text_draft_color,
            &[
                (TextColor::White, "白"),
                (TextColor::Black, "黑"),
                (TextColor::Yellow, "黄"),
                (TextColor::Red, "红"),
            ],
        ) && let Some(i) = ed.text_focus
            && let Some(t) = ed.texts.get_mut(i)
        {
            t.color = ed.text_draft_color;
            ed.dirty = true;
        }

        ui.separator();

        if ui.button("加到中央").clicked() {
            let (w, h) = ed.img_size();
            ed.begin_text_at(w as f32 * 0.5, h as f32 * 0.5);
        }
        if ed.text_focus.is_some()
            && ui.button("删文字").clicked()
            && let Some(i) = ed.text_focus
        {
            ed.remove_text(i);
        }

        ui.separator();

        ui.label(
            egui::RichText::new("点击添加，拖动可移动，再点一下编辑")
                .size(13.0)
                .color(p.text_secondary),
        );
    });
}
