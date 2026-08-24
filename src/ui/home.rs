//! 主页：标题区 + 拖放卡片 + 设置卡片（WinUI 3 风格）。

use eframe::egui;

use crate::config::Config;
use crate::ui::{settings, theme};

pub enum HomeAction {
    None,
    OpenDialog,
    ConfigChanged,
}

pub fn show(ui: &mut egui::Ui, cfg: &mut Config) -> HomeAction {
    let mut action = HomeAction::None;

    ui.vertical_centered(|ui| {
        let p = *theme::palette(ui.ctx());
        ui.add_space(36.0);

        let title_font = egui::FontId::proportional(30.0);
        let mark = egui::TextFormat {
            font_id: title_font.clone(),
            color: p.mark,
            valign: egui::Align::Center,
            extra_letter_spacing: 0.0,
            expand_bg: 0.0,
            ..Default::default()
        };
        let body = egui::TextFormat {
            font_id: title_font,
            color: p.text,
            valign: egui::Align::Center,
            extra_letter_spacing: 0.0,
            expand_bg: 0.0,
            ..Default::default()
        };
        let mut title = egui::text::LayoutJob::default();
        title.append("❗", 0.0, mark.clone());
        title.append("❓", 0.0, mark.clone());
        title.append("强强", 0.0, body);
        title.append("❓", 0.0, mark.clone());
        title.append("❗", 0.0, mark);
        ui.label(title);
        ui.label(
            egui::RichText::new("对称镜像梗图生成器")
                .size(14.0)
                .color(p.text_secondary),
        );
        ui.add_space(28.0);

        // 拖放卡片
        let card_w = ui.available_width().min(460.0);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(card_w, 200.0), egui::Sense::click());

        // 悬停（含内部「选择图片」按钮）/ 有文件拖入：灰框 → 蓝框
        let files_over = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let over_card = ui.rect_contains_pointer(rect) || response.hovered();
        let t = theme::hover_t(ui, ui.id().with("drop_card"), over_card || files_over);
        let stroke_color = theme::lerp_color(p.stroke_divider, p.accent, t);
        let fill = theme::lerp_color(p.card, p.accent_tint, t);

        ui.painter().rect(
            rect,
            egui::CornerRadius::same(theme::metrics::CARD_RADIUS),
            fill,
            egui::Stroke::new(1.0 + t, stroke_color),
            egui::StrokeKind::Inside,
        );

        let center = rect.center();
        ui.painter().text(
            center - egui::vec2(0.0, 34.0),
            egui::Align2::CENTER_CENTER,
            "拖入图片",
            egui::FontId::proportional(20.0),
            p.text,
        );
        ui.painter().text(
            center - egui::vec2(0.0, 6.0),
            egui::Align2::CENTER_CENTER,
            "支持拖到窗口、拖到程序图标、或 Ctrl+V 粘贴",
            egui::FontId::proportional(12.0),
            p.text_secondary,
        );

        // 打开按钮（强调色），放在卡片下半部
        let btn_rect =
            egui::Rect::from_center_size(center + egui::vec2(0.0, 44.0), egui::vec2(140.0, 32.0));
        let mut btn_ui = ui.new_child(egui::UiBuilder::new().max_rect(btn_rect));
        btn_ui.centered_and_justified(|ui| {
            if theme::accent_button(ui, "选择图片").clicked() {
                action = HomeAction::OpenDialog;
            }
        });

        if response.clicked() {
            action = HomeAction::OpenDialog;
        }

        ui.add_space(28.0);

        // 设置卡片
        let settings_w = ui.available_width().min(460.0);
        let settings_rect = egui::Rect::from_min_size(
            egui::pos2(
                ui.cursor().min.x + (ui.available_width() - settings_w) / 2.0,
                ui.cursor().min.y,
            ),
            egui::vec2(settings_w, 236.0),
        );
        let mut card_ui = ui.new_child(egui::UiBuilder::new().max_rect(settings_rect));
        theme::card_frame(ui.ctx()).show(&mut card_ui, |ui| {
            ui.label(
                egui::RichText::new("设置")
                    .size(15.0)
                    .color(p.text)
                    .strong(),
            );
            ui.add_space(8.0);
            if settings::show(ui, cfg) {
                action = HomeAction::ConfigChanged;
            }
        });
        ui.advance_cursor_after_rect(settings_rect);
    });

    action
}
