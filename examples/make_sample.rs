//! 人工验收辅助：用夹具图生成人脸框/整图两种镜像结果，输出到临时目录供肉眼检查。

use qp_meme_gen::core::mirror::{self, Rect};
use qp_meme_gen::detect::FaceDetector;
use qp_meme_gen::ui::editor::face_box;

fn main() {
    let img = image::open("tests/fixtures/face.png")
        .expect("读取夹具失败")
        .to_rgba8();
    let (w, h) = (img.width() as i32, img.height() as i32);

    let det = FaceDetector::new().expect("模型加载失败");
    let face = det
        .detect_primary(&img)
        .expect("推理失败")
        .expect("未检出人脸");
    println!(
        "人脸 bbox={:?} 左眼=({:.1},{:.1}) 右眼=({:.1},{:.1})",
        face.bbox,
        face.keypoints[0].x,
        face.keypoints[0].y,
        face.keypoints[1].x,
        face.keypoints[1].y
    );

    // 人脸框 + 自动方向
    let sel = face_box(&face, w, h);
    let dir = mirror::auto_direction(&img, sel);
    println!("人脸框={sel:?} 自动方向={dir:?}");
    let out = mirror::mirror_image(&img, sel, dir);
    out.save(r"C:\Users\admin\AppData\Local\Temp\opencode\sample_face.png")
        .unwrap();

    // 整图框 + 自动方向（对应梗图的原始做法）
    let full = Rect::new(0, 0, w, h);
    let dir2 = mirror::auto_direction(&img, full);
    println!("整图框自动方向={dir2:?}");
    let out2 = mirror::mirror_image(&img, full, dir2);
    out2.save(r"C:\Users\admin\AppData\Local\Temp\opencode\sample_full.png")
        .unwrap();

    println!("输出完成");
}
