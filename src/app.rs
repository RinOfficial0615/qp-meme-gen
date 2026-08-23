//! 应用状态机：Home / Editor 两个屏幕，四种打开方式在此汇合
//! （对话框 / 拖入窗口 / 拖到 exe / 粘贴）。

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use image::RgbaImage;

use crate::clipboard::{self, PastedImage};
use crate::config::{Config, CropMode};
use crate::detect::FaceDetector;
use crate::ui::theme::PageEnter;
use crate::ui::toast::{ToastKind, Toasts};
use crate::ui::{editor, home};

enum Screen {
    Home,
    Editor(Box<editor::Editor>),
}

pub struct QpApp {
    config: Config,
    screen: Screen,
    detector: Option<FaceDetector>,
    toasts: Toasts,
    /// 调试钩子：QP_SHOT=路径 时自拍一帧 UI 存为 PNG（无窗口截图依赖）。
    shot_target: Option<PathBuf>,
    paste_cmd: clipboard::PasteCommand,
    /// 主页 ↔ 编辑器切入动画：`(开始时刻, 方向)`，+1 从右入，-1 从左入。
    nav: Option<(Instant, f32)>,
}

/// 懒加载检测器（模型已内嵌，构造即解析，无网络/磁盘 IO）。
fn ensure_detector(slot: &mut Option<FaceDetector>) -> anyhow::Result<&FaceDetector> {
    if slot.is_none() {
        *slot = Some(FaceDetector::new()?);
    }
    Ok(slot.as_ref().unwrap())
}

/// 对编辑器当前图片做人脸检测并设置选框。
/// 无人脸或检测失败时在画面中央放比例框（不回退整图）。
/// `multi` = 为每张脸建框，否则只框面积最大的主脸。检测结果会缓存供「加框」使用。
fn apply_face_boxes(
    detector_slot: &mut Option<FaceDetector>,
    ed: &mut editor::Editor,
    multi: bool,
) -> Result<(), String> {
    let det = match ensure_detector(detector_slot) {
        Ok(d) => d,
        Err(e) => {
            ed.set_faces(Vec::new());
            ed.apply_face_boxes(&[]);
            return Err(format!("人脸检测初始化失败：{e}，已在中央放置选框"));
        }
    };
    match det.detect(&ed.img) {
        Ok(faces) => {
            if faces.is_empty() {
                ed.apply_face_boxes(&[]);
                ed.set_faces(faces);
                Err("未检测到人脸，已在中央放置选框".into())
            } else if multi {
                ed.apply_face_boxes(&faces);
                ed.set_faces(faces);
                Ok(())
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
                ed.apply_face_boxes(&[primary]);
                ed.set_faces(faces);
                Ok(())
            }
        }
        Err(e) => {
            ed.set_faces(Vec::new());
            ed.apply_face_boxes(&[]);
            Err(format!("人脸检测失败：{e}，已在中央放置选框"))
        }
    }
}

/// 加框前确保已有检测缓存；检测失败仍可在中央加框。
fn ensure_faces_for_add(
    detector_slot: &mut Option<FaceDetector>,
    ed: &mut editor::Editor,
) -> Result<(), String> {
    if ed.is_full_image() {
        return Err("整图框选时不能添加选框".into());
    }
    if ed.faces_cached() {
        return Ok(());
    }
    let det = match ensure_detector(detector_slot) {
        Ok(d) => d,
        Err(e) => {
            ed.set_faces(Vec::new());
            return Err(format!("人脸检测初始化失败：{e}，已在中央加框"));
        }
    };
    match det.detect(&ed.img) {
        Ok(faces) => {
            ed.set_faces(faces);
            Ok(())
        }
        Err(e) => {
            ed.set_faces(Vec::new());
            Err(format!("人脸检测失败：{e}，已在中央加框"))
        }
    }
}

impl QpApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial_image: Option<PathBuf>) -> Self {
        let config = Config::load();
        crate::ui::theme::apply(&cc.egui_ctx, config.appearance.to_theme_preference());
        let mut app = Self {
            config,
            screen: Screen::Home,
            detector: None,
            toasts: Toasts::default(),
            shot_target: std::env::var("QP_SHOT").ok().map(PathBuf::from),
            paste_cmd: clipboard::PasteCommand::new(),
            nav: None,
        };
        if let Some(path) = initial_image {
            app.open_image(&cc.egui_ctx, path);
        }
        app
    }

    fn kick_nav(&mut self, dir: f32) {
        self.nav = Some((Instant::now(), dir));
    }

    fn page_enter(&self) -> PageEnter {
        match self.nav {
            Some((at, dir)) => PageEnter::from_start(at, dir),
            None => PageEnter::done(),
        }
    }

    /// 统一的"进入编辑器"路径：设置初始框、刷新结果、切屏。
    fn enter_editor(&mut self, ctx: &egui::Context, img: RgbaImage, path: Option<PathBuf>) {
        let from_home = matches!(self.screen, Screen::Home);
        let mut ed = editor::Editor::new(img, path, self.config.default_direction);
        match self.config.default_crop_mode {
            CropMode::Full => {}
            CropMode::Single | CropMode::Multi => {
                let multi = self.config.default_crop_mode == CropMode::Multi;
                if let Err(msg) = apply_face_boxes(&mut self.detector, &mut ed, multi) {
                    self.toasts.info(msg);
                }
            }
        }
        ed.refresh_result(ctx);
        self.screen = Screen::Editor(Box::new(ed));
        if from_home {
            self.kick_nav(1.0);
        }
    }

    fn open_image_dialog(&mut self, ctx: &egui::Context) {
        let picked = rfd::FileDialog::new()
            .add_filter(
                "图片",
                &["png", "jpg", "jpeg", "bmp", "gif", "webp", "tiff", "ico"],
            )
            .pick_file();
        if let Some(path) = picked {
            self.open_image(ctx, path);
        }
    }

    fn open_image(&mut self, ctx: &egui::Context, path: PathBuf) {
        match image::open(&path) {
            Ok(img) => self.enter_editor(ctx, img.to_rgba8(), Some(path)),
            Err(e) => self.toasts.error(format!("打开失败：{e}")),
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.open_image(ctx, path);
        }
    }

    /// Ctrl+V / Win+V / Shift+Insert：走 Chromium 那套格式优先级读图。
    fn handle_paste(&mut self, ctx: &egui::Context) {
        if self.paste_cmd.poll(ctx) {
            self.paste_from_clipboard(ctx);
        }
    }

    fn paste_from_clipboard(&mut self, ctx: &egui::Context) {
        match clipboard::take_image() {
            Ok(PastedImage::Pixels(img)) => {
                self.enter_editor(ctx, img, None);
                self.toasts.success("已粘贴剪贴板图片");
            }
            Ok(PastedImage::File(path)) => match image::open(&path) {
                Ok(img) => {
                    self.enter_editor(ctx, img.to_rgba8(), Some(path));
                    self.toasts.success("已粘贴剪贴板图片");
                }
                Err(e) => self.toasts.error(format!("打开失败：{e}")),
            },
            Err(msg) => self.toasts.error(msg),
        }
    }

    /// 调试钩子：QP_SHOT=路径 时请求下一帧截图，收到后存 PNG 并停止。
    /// 自拍不依赖屏幕捕获，可做远程排查与自动化验收。
    fn handle_shot_request(&mut self, ctx: &egui::Context) {
        let Some(target) = self.shot_target.clone() else {
            return;
        };
        // 每帧请求，直到收到截图事件
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        for e in ctx.input(|i| i.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = e {
                if let Some(img) = RgbaImage::from_raw(
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image.as_raw().to_vec(),
                ) {
                    let _ = img.save(&target);
                }
                self.shot_target = None;
            }
        }
    }
}

/// 屏幕内产生的、需要对 `self` 整体操作的后置动作。
enum Post {
    None,
    OpenDialog,
    ConfigChanged,
    GoHome,
    RedetectFace,
    AddBox,
    Toast(ToastKind, String),
}

impl eframe::App for QpApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_dropped_files(&ctx);
        self.handle_paste(&ctx);
        self.handle_shot_request(&ctx);

        let enter = self.page_enter();
        let post = match &mut self.screen {
            Screen::Home => {
                let action = egui::CentralPanel::default()
                    .show(ui, |ui| {
                        enter.apply(ui);
                        home::show(ui, &mut self.config)
                    })
                    .inner;
                match action {
                    home::HomeAction::None => Post::None,
                    home::HomeAction::OpenDialog => Post::OpenDialog,
                    home::HomeAction::ConfigChanged => Post::ConfigChanged,
                }
            }
            Screen::Editor(ed) => match editor::show(ui, ed, enter) {
                editor::EditorRequest::None => Post::None,
                editor::EditorRequest::OpenNew => Post::OpenDialog,
                editor::EditorRequest::GoHome => Post::GoHome,
                editor::EditorRequest::RedetectFace => Post::RedetectFace,
                editor::EditorRequest::AddBox => Post::AddBox,
                editor::EditorRequest::Toast(k, m) => Post::Toast(k, m),
            },
        };

        match post {
            Post::None => {}
            Post::OpenDialog => self.open_image_dialog(&ctx),
            Post::ConfigChanged => {
                crate::ui::theme::apply(&ctx, self.config.appearance.to_theme_preference());
                if let Err(e) = self.config.save() {
                    self.toasts
                        .error(format!("配置保存失败：{e}（本次会话内生效）"));
                }
            }
            Post::GoHome => {
                self.screen = Screen::Home;
                self.kick_nav(-1.0);
            }
            Post::Toast(k, m) => self.toasts.push(k, m),
            Post::RedetectFace => {
                if let Screen::Editor(ed) = &mut self.screen {
                    let multi = self.config.default_crop_mode == CropMode::Multi;
                    if let Err(msg) = apply_face_boxes(&mut self.detector, ed, multi) {
                        self.toasts.info(msg);
                    }
                    ed.refresh_result(&ctx);
                }
            }
            Post::AddBox => {
                if let Screen::Editor(ed) = &mut self.screen {
                    if let Err(msg) = ensure_faces_for_add(&mut self.detector, ed) {
                        self.toasts.info(msg);
                    }
                    ed.add_box();
                    ed.refresh_result(&ctx);
                }
            }
        }

        self.toasts.show(&ctx);
    }
}
