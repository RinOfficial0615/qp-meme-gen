//! qp-meme-gen："！？强强？！" 对称镜像梗图生成器。
//! 全部逻辑在 lib 内，main.rs 只是薄壳，测试通过本 crate 的模块接口进行。

pub mod app;
pub mod config;
pub mod core;
pub mod detect;
pub mod ui;

use std::path::PathBuf;

/// 启动 GUI。`initial_image` 为拖放到 exe / 命令行传入的图片路径。
pub fn run(initial_image: Option<PathBuf>) -> eframe::Result<()> {
    install_panic_hook();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("qp-meme-gen 强强梗图生成器")
            .with_inner_size([1440.0, 860.0])
            .with_min_inner_size([1280.0, 720.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "qp-meme-gen",
        options,
        Box::new(|cc| {
            install_cjk_fonts(&cc.egui_ctx);
            Ok(Box::new(app::QpApp::new(cc, initial_image)))
        }),
    )
}

/// 崩溃时弹系统错误对话框：release 无控制台窗口，错误必须有出口。
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("程序遇到内部错误，即将退出。\n\n{info}");
        let _ = rfd::MessageDialog::new()
            .set_title("qp-meme-gen 出错了")
            .set_description(&msg)
            .set_level(rfd::MessageLevel::Error)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        eprintln!("{msg}");
    }));
}

/// 注入系统中文字体（egui 内置字体不含 CJK）。全失败时保持默认字体。
fn install_cjk_fonts(ctx: &eframe::egui::Context) {
    for path in crate::core::text::FONT_CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let mut fonts = eframe::egui::FontDefinitions::default();
        fonts.font_data.insert(
            "cjk".into(),
            std::sync::Arc::new(eframe::egui::FontData::from_owned(bytes)),
        );
        for family in [
            eframe::egui::FontFamily::Proportional,
            eframe::egui::FontFamily::Monospace,
        ] {
            fonts.families.entry(family).or_default().push("cjk".into());
        }
        ctx.set_fonts(fonts);
        return;
    }
    eprintln!("警告：未找到系统中文字体，界面文字可能显示为方块");
}
