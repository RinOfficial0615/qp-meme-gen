//! 设置面板：默认框选模式、翻转方向、仅框选处、外观。返回值 = 配置是否被改动。

use eframe::egui;

use crate::config::{Appearance, Config, CropMode, DirectionPref};
use crate::ui::theme;

pub fn show(ui: &mut egui::Ui, cfg: &mut Config) -> bool {
    let before = cfg.clone();

    egui::Grid::new("settings_grid")
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, |ui| {
            ui.label("默认框选模式：");
            egui::ComboBox::from_id_salt("crop_mode")
                .selected_text(match cfg.default_crop_mode {
                    CropMode::Single => "单人脸检测",
                    CropMode::Multi => "多人脸检测",
                    CropMode::Full => "整张图片框选",
                })
                .show_ui(ui, |ui| {
                    theme::combo_choice(
                        ui,
                        &mut cfg.default_crop_mode,
                        CropMode::Single,
                        "单人脸检测",
                    );
                    theme::combo_choice(
                        ui,
                        &mut cfg.default_crop_mode,
                        CropMode::Multi,
                        "多人脸检测",
                    );
                    theme::combo_choice(
                        ui,
                        &mut cfg.default_crop_mode,
                        CropMode::Full,
                        "整张图片框选",
                    );
                });
            ui.end_row();

            ui.label("默认翻转方向：");
            egui::ComboBox::from_id_salt("direction")
                .selected_text(match cfg.default_direction {
                    DirectionPref::Left => "保留左半",
                    DirectionPref::Right => "保留右半",
                    DirectionPref::Auto => "自动",
                })
                .show_ui(ui, |ui| {
                    theme::combo_choice(
                        ui,
                        &mut cfg.default_direction,
                        DirectionPref::Auto,
                        "自动",
                    );
                    theme::combo_choice(
                        ui,
                        &mut cfg.default_direction,
                        DirectionPref::Left,
                        "保留左半",
                    );
                    theme::combo_choice(
                        ui,
                        &mut cfg.default_direction,
                        DirectionPref::Right,
                        "保留右半",
                    );
                });
            ui.end_row();

            ui.label("默认仅保留框选处：");
            let _ = theme::accent_checkbox(ui, &mut cfg.default_crop_export, "开启")
                .on_hover_text("打开图片后，单框时复制和保存只保留选框内的画面");
            ui.end_row();

            ui.label("外观：");
            egui::ComboBox::from_id_salt("appearance")
                .selected_text(match cfg.appearance {
                    Appearance::System => "跟随系统",
                    Appearance::Light => "浅色",
                    Appearance::Dark => "深色",
                })
                .show_ui(ui, |ui| {
                    theme::combo_choice(ui, &mut cfg.appearance, Appearance::System, "跟随系统");
                    theme::combo_choice(ui, &mut cfg.appearance, Appearance::Light, "浅色");
                    theme::combo_choice(ui, &mut cfg.appearance, Appearance::Dark, "深色");
                });
            ui.end_row();
        });

    *cfg != before
}
