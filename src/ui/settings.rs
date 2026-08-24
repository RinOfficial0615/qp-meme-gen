//! 设置面板：默认框选模式、镜像方式/保留侧、仅框选处、外观。返回值 = 配置是否被改动。

use eframe::egui;

use crate::config::{Appearance, Config, CropMode, KeepSide, MirrorAxis};
use crate::ui::theme;

pub fn show(ui: &mut egui::Ui, cfg: &mut Config) -> bool {
    let before = cfg.clone();
    cfg.default_keep_side = cfg
        .default_keep_side
        .normalized_for_axis(cfg.default_mirror_axis);

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

            ui.label("默认镜像方式：");
            let old_axis = cfg.default_mirror_axis;
            egui::ComboBox::from_id_salt("mirror_axis")
                .selected_text(match cfg.default_mirror_axis {
                    MirrorAxis::Horizontal => "水平翻转",
                    MirrorAxis::Vertical => "垂直翻转",
                })
                .show_ui(ui, |ui| {
                    theme::combo_choice(
                        ui,
                        &mut cfg.default_mirror_axis,
                        MirrorAxis::Horizontal,
                        "水平翻转",
                    );
                    theme::combo_choice(
                        ui,
                        &mut cfg.default_mirror_axis,
                        MirrorAxis::Vertical,
                        "垂直翻转",
                    );
                });
            if cfg.default_mirror_axis != old_axis {
                cfg.default_keep_side = cfg
                    .default_keep_side
                    .normalized_for_axis(cfg.default_mirror_axis);
            }
            ui.end_row();

            ui.label("默认保留侧：");
            let side_label = match (cfg.default_mirror_axis, cfg.default_keep_side) {
                (_, KeepSide::Auto) => "自动",
                (MirrorAxis::Horizontal, KeepSide::Left) => "保留左半",
                (MirrorAxis::Horizontal, KeepSide::Right) => "保留右半",
                (MirrorAxis::Vertical, KeepSide::Top) => "保留上半",
                (MirrorAxis::Vertical, KeepSide::Bottom) => "保留下半",
                (axis, side) => match side.normalized_for_axis(axis) {
                    KeepSide::Left => "保留左半",
                    KeepSide::Right => "保留右半",
                    KeepSide::Top => "保留上半",
                    KeepSide::Bottom => "保留下半",
                    KeepSide::Auto => "自动",
                },
            };
            egui::ComboBox::from_id_salt("keep_side")
                .selected_text(side_label)
                .show_ui(ui, |ui| {
                    theme::combo_choice(ui, &mut cfg.default_keep_side, KeepSide::Auto, "自动");
                    match cfg.default_mirror_axis {
                        MirrorAxis::Horizontal => {
                            theme::combo_choice(
                                ui,
                                &mut cfg.default_keep_side,
                                KeepSide::Left,
                                "保留左半",
                            );
                            theme::combo_choice(
                                ui,
                                &mut cfg.default_keep_side,
                                KeepSide::Right,
                                "保留右半",
                            );
                        }
                        MirrorAxis::Vertical => {
                            theme::combo_choice(
                                ui,
                                &mut cfg.default_keep_side,
                                KeepSide::Top,
                                "保留上半",
                            );
                            theme::combo_choice(
                                ui,
                                &mut cfg.default_keep_side,
                                KeepSide::Bottom,
                                "保留下半",
                            );
                        }
                    }
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
