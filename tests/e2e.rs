//! 端到端：内嵌 SCRFD 模型在真实图片上检出人脸；镜像输出尺寸不变且对称。

use image::RgbaImage;
use qp_meme_gen::core::mirror::{self, Direction, Rect};
use qp_meme_gen::detect::FaceDetector;

fn fixture() -> RgbaImage {
    image::open(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/face.png"
    ))
    .expect("读取测试夹具失败")
    .to_rgba8()
}

#[test]
fn ui_screenshot_has_no_face() {
    let img = image::open(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/no_face_ui.png"
    ))
    .expect("读取无脸截图失败")
    .to_rgba8();
    let det = FaceDetector::new().expect("加载内嵌模型失败");
    let faces = det.detect(&img).expect("推理失败");
    assert!(
        faces.is_empty(),
        "主界面截图不应检出人脸，实际 {} 个，最高分 {:?}",
        faces.len(),
        faces.first().map(|f| f.score)
    );
}

#[test]
fn detects_face_in_fixture() {
    let img = fixture();
    let (w, h) = (img.width() as f32, img.height() as f32);
    let det = FaceDetector::new().expect("加载内嵌模型失败");
    let face = det.detect_primary(&img).expect("推理失败");
    let face = face.expect("示例图中应检出人脸");
    // 人脸应大致位于画面中央区域，面积占比合理
    let cx = (face.bbox[0] + face.bbox[2]) / 2.0;
    let cy = (face.bbox[1] + face.bbox[3]) / 2.0;
    assert!(
        (cx - w / 2.0).abs() < w * 0.3,
        "人脸中心水平偏移过大: cx={cx}, w={w}"
    );
    assert!(
        (cy - h / 2.0).abs() < h * 0.3,
        "人脸中心垂直偏移过大: cy={cy}, h={h}"
    );
    assert!(face.area() > w * h * 0.05, "人脸面积过小: {}", face.area());
    // 关键点应在 bbox 附近
    for kp in face.keypoints {
        assert!(kp.x > -w * 0.2 && kp.x < w * 1.2, "关键点 x 越界: {kp:?}");
    }
}

#[test]
fn mirror_full_image_is_symmetric() {
    let img = fixture();
    let (w, h) = img.dimensions();
    let out = mirror::mirror_image(&img, Rect::new(0, 0, w as i32, h as i32), Direction::Left);
    assert_eq!(out.dimensions(), (w, h), "输出尺寸必须与输入一致");
    // 抽查对称像素：右半 == 左半翻转
    for &(x, y) in &[
        (w - 1, 0),
        (w - 1, h - 1),
        (w - 10, h / 2),
        (w * 3 / 4, h / 4),
    ] {
        assert_eq!(
            out.get_pixel(x, y),
            out.get_pixel(w - 1 - x, y),
            "({x},{y}) 处不对称"
        );
    }
}

#[test]
fn auto_direction_runs_on_fixture() {
    let img = fixture();
    let (w, h) = img.dimensions();
    let _ = mirror::auto_direction(&img, Rect::new(0, 0, w as i32, h as i32));
}
