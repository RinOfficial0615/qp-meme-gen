//! 人脸 → 选框：纯几何，无 GUI。
//!
//! 双眼中点为轴，左右对称扩张覆盖 bbox 并留边距；无人脸时画面中央 40% 框。
//! 一张脸被「占用」：中心落在某框内，或与脸 bbox 的 IoU > 0.3。

use crate::core::mirror::Rect;
use crate::detect::Face;

/// 可拖、可镜像的最小边。
pub const MIN_BOX: i32 = 4;
const FACE_IOU: f32 = 0.3;
/// 无人脸时中央框占画面宽、高的比例。
const CENTER_BOX_FRAC: f32 = 0.4;

/// 画面中央的默认新框：宽、高各为画面的 40%。
pub fn center_box(w: i32, h: i32) -> Rect {
    let bw = ((w as f32) * CENTER_BOX_FRAC)
        .round()
        .clamp(MIN_BOX as f32, w.max(MIN_BOX) as f32) as i32;
    let bh = ((h as f32) * CENTER_BOX_FRAC)
        .round()
        .clamp(MIN_BOX as f32, h.max(MIN_BOX) as f32) as i32;
    let x0 = ((w - bw) / 2).max(0);
    let y0 = ((h - bh) / 2).max(0);
    Rect::new(x0, y0, x0 + bw, y0 + bh).clamped(w, h)
}

fn face_as_rect(face: &Face) -> Rect {
    Rect::new(
        face.bbox[0] as i32,
        face.bbox[1] as i32,
        face.bbox[2] as i32,
        face.bbox[3] as i32,
    )
    .normalized()
}

fn face_covered(face: &Face, boxes: &[Rect]) -> bool {
    let cx = ((face.bbox[0] + face.bbox[2]) * 0.5) as i32;
    let cy = ((face.bbox[1] + face.bbox[3]) * 0.5) as i32;
    let fb = face_as_rect(face);
    boxes
        .iter()
        .any(|b| b.contains(cx, cy) || b.iou(fb) > FACE_IOU)
}

/// 未覆盖人脸中 score 最高者。
pub fn pick_next_face<'a>(faces: &'a [Face], boxes: &[Rect]) -> Option<&'a Face> {
    faces
        .iter()
        .filter(|f| !face_covered(f, boxes))
        .max_by(|a, b| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// 由人脸检测结果构造选框：双眼中点为轴心，左右对称扩张覆盖整脸并留边距。
pub fn face_box(face: &Face, img_w: i32, img_h: i32) -> Rect {
    let [x0, y0, x1, y1] = face.bbox;
    let (bw, bh) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    let eye_mid = (face.keypoints[0].x + face.keypoints[1].x) / 2.0;
    let axis = if eye_mid.is_finite() && eye_mid > x0 && eye_mid < x1 {
        eye_mid
    } else {
        (x0 + x1) / 2.0
    };
    let half = ((axis - x0).max(x1 - axis) + bw * 0.1).max(bw * 0.5);
    Rect::new(
        (axis - half) as i32,
        (y0 - bh * 0.15) as i32,
        (axis + half) as i32,
        (y1 + bh * 0.05) as i32,
    )
    .normalized()
    .clamped(img_w, img_h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::Point2;

    fn face(bbox: [f32; 4], score: f32) -> Face {
        Face {
            bbox,
            keypoints: {
                let mut k = [Point2::default(); 5];
                k[0] = Point2 {
                    x: (bbox[0] + bbox[2]) * 0.4,
                    y: bbox[1] + 20.0,
                };
                k[1] = Point2 {
                    x: (bbox[0] + bbox[2]) * 0.6,
                    y: bbox[1] + 20.0,
                };
                k
            },
            score,
        }
    }

    #[test]
    fn face_box_symmetric_around_eyes() {
        let face = Face {
            bbox: [100.0, 50.0, 300.0, 250.0],
            keypoints: {
                let mut k = [Point2::default(); 5];
                k[0] = Point2 { x: 150.0, y: 120.0 };
                k[1] = Point2 { x: 250.0, y: 120.0 };
                k
            },
            score: 0.9,
        };
        let r = face_box(&face, 400, 400);
        // 轴 = 双眼中点 200；框必须关于 200 对称
        assert_eq!(r.x0 + r.x1, 400);
        // 覆盖整脸并留边距
        assert!(r.x0 <= 100 && r.x1 >= 300);
        // 顶部多扩：y0 < 50
        assert!(r.y0 < 50);
    }

    #[test]
    fn face_box_clamped_to_image() {
        let face = Face {
            bbox: [0.0, 0.0, 100.0, 100.0],
            keypoints: {
                let mut k = [Point2::default(); 5];
                k[0] = Point2 { x: 30.0, y: 40.0 };
                k[1] = Point2 { x: 70.0, y: 40.0 };
                k
            },
            score: 0.9,
        };
        let r = face_box(&face, 200, 200);
        assert!(r.x0 >= 0 && r.y0 >= 0 && r.x1 <= 200 && r.y1 <= 200);
        assert!(r.is_mirrorable());
    }

    #[test]
    fn pick_next_face_highest_unused_score() {
        let faces = [
            face([10.0, 10.0, 40.0, 40.0], 0.6),
            face([80.0, 10.0, 120.0, 50.0], 0.95),
            face([10.0, 80.0, 50.0, 120.0], 0.8),
        ];
        let used = [Rect::new(70, 0, 130, 60)];
        let next = pick_next_face(&faces, &used).unwrap();
        assert!((next.score - 0.8).abs() < f32::EPSILON);
        assert_eq!(pick_next_face(&faces, &[]).unwrap().score, 0.95);
        let all_used = [
            Rect::new(0, 0, 50, 50),
            Rect::new(70, 0, 130, 60),
            Rect::new(0, 70, 60, 130),
        ];
        assert!(pick_next_face(&faces, &all_used).is_none());
    }

    #[test]
    fn center_box_is_centered() {
        let r = center_box(200, 100);
        assert_eq!(r.center_x(), 100.0);
        assert_eq!(r.center_y(), 50.0);
        assert_eq!(r.width(), 80);
        assert_eq!(r.height(), 40);
        assert!(r.x0 >= 0 && r.x1 <= 200 && r.y0 >= 0 && r.y1 <= 100);
    }
}
