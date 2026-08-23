//! 薄入口：解析 argv（拖到 exe 上打开 / 命令行传路径），其余全在 lib。

// 不显示控制台窗口；panic 由 lib 里的弹窗钩子兜底
#![windows_subsystem = "windows"]

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let initial_image = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    qp_meme_gen::run(initial_image)
}
