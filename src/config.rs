//! 配置模块：`exe 同目录 qp-meme-gen.toml`，读失败用默认值，写失败由调用方提示。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CropMode {
    /// 打开图片时检测并只框选主脸（面积最大）。
    #[serde(rename = "single", alias = "face")]
    Single,
    /// 打开图片时为检测到的每张脸各建一个选框。
    Multi,
    /// 打开图片时框选整张图片。
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DirectionPref {
    Left,
    Right,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    /// 跟随系统。
    System,
    Light,
    Dark,
}

impl Appearance {
    pub fn to_theme_preference(self) -> eframe::egui::ThemePreference {
        match self {
            Appearance::System => eframe::egui::ThemePreference::System,
            Appearance::Light => eframe::egui::ThemePreference::Light,
            Appearance::Dark => eframe::egui::ThemePreference::Dark,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_crop_mode: CropMode,
    pub default_direction: DirectionPref,
    pub appearance: Appearance,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_crop_mode: CropMode::Single,
            default_direction: DirectionPref::Auto,
            appearance: Appearance::System,
        }
    }
}

fn config_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("qp-meme-gen.toml")))
        .unwrap_or_else(|| PathBuf::from("qp-meme-gen.toml"))
}

impl Config {
    pub fn load() -> Self {
        let Ok(text) = std::fs::read_to_string(config_path()) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let text = toml::to_string_pretty(self).context("序列化配置失败")?;
        std::fs::write(config_path(), text).context("写入配置文件失败")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let cfg = Config {
            default_crop_mode: CropMode::Full,
            default_direction: DirectionPref::Right,
            appearance: Appearance::Dark,
        };
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn partial_toml_uses_defaults() {
        let cfg: Config = toml::from_str("default_direction = \"left\"").unwrap();
        assert_eq!(cfg.default_direction, DirectionPref::Left);
        assert_eq!(cfg.default_crop_mode, CropMode::Single);
        assert_eq!(cfg.appearance, Appearance::System);
    }

    #[test]
    fn face_alias_maps_to_single() {
        let cfg: Config = toml::from_str("default_crop_mode = \"face\"").unwrap();
        assert_eq!(cfg.default_crop_mode, CropMode::Single);
    }

    #[test]
    fn single_and_multi_roundtrip() {
        for mode in [CropMode::Single, CropMode::Multi, CropMode::Full] {
            let cfg = Config {
                default_crop_mode: mode,
                ..Config::default()
            };
            let text = toml::to_string_pretty(&cfg).unwrap();
            let back: Config = toml::from_str(&text).unwrap();
            assert_eq!(back.default_crop_mode, mode, "{text}");
        }
    }

    #[test]
    fn garbage_toml_falls_back() {
        let cfg: Config = toml::from_str("!!!not toml").unwrap_or_default();
        assert_eq!(cfg, Config::default());
    }
}
