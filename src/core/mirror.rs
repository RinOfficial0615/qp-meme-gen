//! 镜像核心算法：纯函数，无 GUI 依赖。
//!
//! 语义：输出尺寸与输入一致。选框内按水平或垂直方式，
//! 把保留侧的一半翻转后覆盖另一侧；选框外像素保持不变。

use image::RgbaImage;
use serde::{Deserialize, Serialize};

/// 像素坐标选框，半开区间 `[x0, x1) × [y0, y1)`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// 交换坐标使 x0 <= x1, y0 <= y1。
    pub fn normalized(self) -> Self {
        Self {
            x0: self.x0.min(self.x1),
            y0: self.y0.min(self.y1),
            x1: self.x0.max(self.x1),
            y1: self.y0.max(self.y1),
        }
    }

    /// 钳制到 `[0, w] × [0, h]`。
    pub fn clamped(self, w: i32, h: i32) -> Self {
        Self {
            x0: self.x0.clamp(0, w),
            y0: self.y0.clamp(0, h),
            x1: self.x1.clamp(0, w),
            y1: self.y1.clamp(0, h),
        }
    }

    pub fn width(&self) -> i32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> i32 {
        self.y1 - self.y0
    }

    pub fn center_x(&self) -> f32 {
        (self.x0 + self.x1) as f32 / 2.0
    }

    pub fn center_y(&self) -> f32 {
        (self.y0 + self.y1) as f32 / 2.0
    }

    pub fn area(&self) -> i32 {
        let n = self.normalized();
        n.width().max(0) * n.height().max(0)
    }

    /// 两框交并比；无交集为 0。
    pub fn iou(self, other: Self) -> f32 {
        let a = self.normalized();
        let b = other.normalized();
        let x0 = a.x0.max(b.x0);
        let y0 = a.y0.max(b.y0);
        let x1 = a.x1.min(b.x1);
        let y1 = a.y1.min(b.y1);
        let inter = (x1 - x0).max(0) * (y1 - y0).max(0);
        let union = a.area() + b.area() - inter;
        if union <= 0 {
            0.0
        } else {
            inter as f32 / union as f32
        }
    }

    /// 是否有可镜像的最小尺寸（宽至少 2 像素）。
    pub fn is_mirrorable(&self) -> bool {
        self.width() >= 2 && self.height() >= 1
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

/// 镜像方式。`Horizontal` 表示左右翻转（对称轴为竖线），
/// `Vertical` 表示上下翻转（对称轴为横线）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MirrorAxis {
    Horizontal,
    Vertical,
}

/// 保留哪一侧。`Auto` 会按当前镜像方式比较两侧信息量。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeepSide {
    Auto,
    Left,
    Right,
    Top,
    Bottom,
}

impl KeepSide {
    /// 将旧轴向的侧名映射为新轴向对应的第一/第二半区。
    /// 这样从左右切换到上下时仍保留用户选择的相对侧。
    pub fn normalized_for_axis(self, axis: MirrorAxis) -> Self {
        match (axis, self) {
            (MirrorAxis::Horizontal, KeepSide::Top) => KeepSide::Left,
            (MirrorAxis::Horizontal, KeepSide::Bottom) => KeepSide::Right,
            (MirrorAxis::Vertical, KeepSide::Left) => KeepSide::Top,
            (MirrorAxis::Vertical, KeepSide::Right) => KeepSide::Bottom,
            _ => self,
        }
    }

    pub fn is_compatible(self, axis: MirrorAxis) -> bool {
        matches!(
            (axis, self),
            (_, KeepSide::Auto)
                | (MirrorAxis::Horizontal, KeepSide::Left | KeepSide::Right)
                | (MirrorAxis::Vertical, KeepSide::Top | KeepSide::Bottom)
        )
    }
}

/// 镜像方向：保留哪一半。保留此类型和旧 API，默认表示左右镜像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// 保留左半，镜像覆盖右半。
    Left,
    /// 保留右半，镜像覆盖左半。
    Right,
}

impl Direction {
    pub fn opposite(self) -> Self {
        match self {
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
        }
    }
}

/// 兼容旧调用：在选框内以框中心竖线为轴镜像，返回新图（尺寸不变）。
pub fn mirror_image(img: &RgbaImage, sel: Rect, dir: Direction) -> RgbaImage {
    mirror_image_with_axis(img, sel, MirrorAxis::Horizontal, dir.into_keep_side())
}

/// 在选框内按指定镜像方式镜像，返回新图（尺寸不变）。
pub fn mirror_image_with_axis(
    img: &RgbaImage,
    sel: Rect,
    axis: MirrorAxis,
    side: KeepSide,
) -> RgbaImage {
    let mut out = img.clone();
    apply_mirror_with_axis(&mut out, img, sel, axis, side);
    out
}

/// 多个选框各自从原图镜像后写入输出；后写覆盖重叠区。
pub fn mirror_regions(img: &RgbaImage, regions: &[(Rect, Direction)]) -> RgbaImage {
    let regions: Vec<_> = regions
        .iter()
        .map(|&(sel, dir)| (sel, MirrorAxis::Horizontal, dir.into_keep_side()))
        .collect();
    mirror_regions_with_axis(img, &regions)
}

/// 多个选框各自按轴向镜像；每个元组为 `(选框, 镜像方式, 保留侧)`。
pub fn mirror_regions_with_axis(
    img: &RgbaImage,
    regions: &[(Rect, MirrorAxis, KeepSide)],
) -> RgbaImage {
    let mut out = img.clone();
    for &(sel, axis, side) in regions {
        apply_mirror_with_axis(&mut out, img, sel, axis, side);
    }
    out
}

/// 裁出选框内像素，输出尺寸为框的宽高。空框则原样返回。
pub fn crop_to_rect(img: &RgbaImage, sel: Rect) -> RgbaImage {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let sel = sel.normalized().clamped(w, h);
    let bw = sel.width().max(0) as u32;
    let bh = sel.height().max(0) as u32;
    if bw == 0 || bh == 0 {
        return img.clone();
    }
    image::imageops::crop_imm(img, sel.x0 as u32, sel.y0 as u32, bw, bh).to_image()
}

/// 把 `src` 的选框区域复制到 `dst`（用于某框「查看原图」覆盖先前镜像）。
pub fn copy_rect(dst: &mut RgbaImage, src: &RgbaImage, sel: Rect) {
    let (w, h) = (src.width() as i32, src.height() as i32);
    let sel = sel.normalized().clamped(w, h);
    for y in sel.y0..sel.y1 {
        for x in sel.x0..sel.x1 {
            dst.put_pixel(x as u32, y as u32, *src.get_pixel(x as u32, y as u32));
        }
    }
}

pub fn apply_mirror(out: &mut RgbaImage, src: &RgbaImage, sel: Rect, dir: Direction) {
    apply_mirror_with_axis(out, src, sel, MirrorAxis::Horizontal, dir.into_keep_side());
}

/// 在选框内按指定轴向镜像。`src` 始终是原图，避免重叠选框互相污染。
pub fn apply_mirror_with_axis(
    out: &mut RgbaImage,
    src: &RgbaImage,
    sel: Rect,
    axis: MirrorAxis,
    side: KeepSide,
) {
    let (w, h) = (src.width() as i32, src.height() as i32);
    let sel = sel.normalized().clamped(w, h);
    if !sel.is_mirrorable_for(axis) {
        return;
    }
    let side = resolve_side(src, sel, axis, side);
    match axis {
        MirrorAxis::Horizontal => {
            for y in sel.y0..sel.y1 {
                for x in sel.x0..sel.x1 {
                    let sx = sel.x0 + sel.x1 - 1 - x;
                    let replace = match side {
                        KeepSide::Left => x > sx,
                        KeepSide::Right => x < sx,
                        _ => false,
                    };
                    if replace {
                        out.put_pixel(x as u32, y as u32, *src.get_pixel(sx as u32, y as u32));
                    }
                }
            }
        }
        MirrorAxis::Vertical => {
            for y in sel.y0..sel.y1 {
                for x in sel.x0..sel.x1 {
                    let sy = sel.y0 + sel.y1 - 1 - y;
                    let replace = match side {
                        KeepSide::Top => y > sy,
                        KeepSide::Bottom => y < sy,
                        _ => false,
                    };
                    if replace {
                        out.put_pixel(x as u32, y as u32, *src.get_pixel(x as u32, sy as u32));
                    }
                }
            }
        }
    }
}

/// 自动选择当前轴向信息量更高的一半。
pub fn auto_keep_side(img: &RgbaImage, sel: Rect, axis: MirrorAxis) -> KeepSide {
    let first = side_energy(img, sel, axis, true);
    let second = side_energy(img, sel, axis, false);
    if first >= second {
        match axis {
            MirrorAxis::Horizontal => KeepSide::Left,
            MirrorAxis::Vertical => KeepSide::Top,
        }
    } else {
        match axis {
            MirrorAxis::Horizontal => KeepSide::Right,
            MirrorAxis::Vertical => KeepSide::Bottom,
        }
    }
}

fn resolve_side(img: &RgbaImage, sel: Rect, axis: MirrorAxis, side: KeepSide) -> KeepSide {
    let side = side.normalized_for_axis(axis);
    if side == KeepSide::Auto {
        auto_keep_side(img, sel, axis)
    } else {
        side
    }
}

/// 兼容旧调用的水平自动方向：选“素材侧信息量更高”的一侧保留。
/// 信息侧重度为保留半区的水平梯度能量，隔行采样控制大图开销；打平取左。
pub fn auto_direction(img: &RgbaImage, sel: Rect) -> Direction {
    match auto_keep_side(img, sel, MirrorAxis::Horizontal) {
        KeepSide::Left => Direction::Left,
        KeepSide::Right => Direction::Right,
        _ => unreachable!("horizontal auto side must be left/right"),
    }
}

/// 指定轴向与半区的梯度能量。
fn side_energy(img: &RgbaImage, sel: Rect, axis: MirrorAxis, first: bool) -> f64 {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let sel = sel.normalized().clamped(w, h);
    if !sel.is_mirrorable_for(axis) {
        return 0.0;
    }
    let mut sum = 0f64;
    match axis {
        MirrorAxis::Horizontal => {
            let mid = (sel.x0 + sel.x1) / 2;
            let (xa, xb) = if first {
                (sel.x0, mid)
            } else {
                (mid, sel.x1 - 1)
            };
            if xb <= xa {
                return 0.0;
            }
            let step_y = (sel.height() / 64).max(1);
            let mut y = sel.y0;
            while y < sel.y1 {
                let mut prev: Option<f64> = None;
                for x in xa..=xb {
                    let p = img.get_pixel(x as u32, y as u32);
                    let lum = 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64;
                    if let Some(q) = prev {
                        let d = lum - q;
                        sum += d * d;
                    }
                    prev = Some(lum);
                }
                y += step_y;
            }
        }
        MirrorAxis::Vertical => {
            let mid = (sel.y0 + sel.y1) / 2;
            let (ya, yb) = if first {
                (sel.y0, mid)
            } else {
                (mid, sel.y1 - 1)
            };
            if yb <= ya {
                return 0.0;
            }
            let step_x = (sel.width() / 64).max(1);
            let mut x = sel.x0;
            while x < sel.x1 {
                let mut prev: Option<f64> = None;
                for y in ya..=yb {
                    let p = img.get_pixel(x as u32, y as u32);
                    let lum = 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64;
                    if let Some(q) = prev {
                        let d = lum - q;
                        sum += d * d;
                    }
                    prev = Some(lum);
                }
                x += step_x;
            }
        }
    }
    sum
}

impl Direction {
    fn into_keep_side(self) -> KeepSide {
        match self {
            Direction::Left => KeepSide::Left,
            Direction::Right => KeepSide::Right,
        }
    }
}

impl Rect {
    /// 是否有可镜像的最小尺寸（按指定轴至少 2 像素）。
    pub fn is_mirrorable_for(&self, axis: MirrorAxis) -> bool {
        match axis {
            MirrorAxis::Horizontal => self.width() >= 2 && self.height() >= 1,
            MirrorAxis::Vertical => self.height() >= 2 && self.width() >= 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    /// 生成 w×h 图片，像素 R 通道 = x 坐标，G 通道 = y 坐标，便于断言来源。
    fn coord_image(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| Rgba([x as u8, y as u8, 0, 255]))
    }

    #[test]
    fn mirror_left_full_width() {
        let img = coord_image(6, 1);
        let out = mirror_image(&img, Rect::new(0, 0, 6, 1), Direction::Left);
        for x in 0..6 {
            let expect = if x < 3 { x as u8 } else { (5 - x) as u8 };
            assert_eq!(out.get_pixel(x, 0).0[0], expect, "x={x}");
        }
    }

    #[test]
    fn mirror_right_full_width() {
        let img = coord_image(6, 1);
        let out = mirror_image(&img, Rect::new(0, 0, 6, 1), Direction::Right);
        for x in 0..6 {
            let expect = if x < 3 { (5 - x) as u8 } else { x as u8 };
            assert_eq!(out.get_pixel(x, 0).0[0], expect, "x={x}");
        }
    }

    #[test]
    fn mirror_vertical_top_and_bottom() {
        let img = coord_image(2, 6);
        let top = mirror_image_with_axis(
            &img,
            Rect::new(0, 0, 2, 6),
            MirrorAxis::Vertical,
            KeepSide::Top,
        );
        let bottom = mirror_image_with_axis(
            &img,
            Rect::new(0, 0, 2, 6),
            MirrorAxis::Vertical,
            KeepSide::Bottom,
        );
        for y in 0..6 {
            let top_expected = if y < 3 { y as u8 } else { (5 - y) as u8 };
            let bottom_expected = if y < 3 { (5 - y) as u8 } else { y as u8 };
            assert_eq!(top.get_pixel(0, y).0[1], top_expected, "top y={y}");
            assert_eq!(bottom.get_pixel(0, y).0[1], bottom_expected, "bottom y={y}");
        }
    }

    #[test]
    fn vertical_partial_box_leaves_outside_untouched() {
        let img = coord_image(4, 10);
        let out = mirror_image_with_axis(
            &img,
            Rect::new(1, 2, 3, 8),
            MirrorAxis::Vertical,
            KeepSide::Top,
        );
        for y in 0..10 {
            for x in 0..4 {
                let inside = (1..3).contains(&x) && (2..8).contains(&y);
                let expected = if inside && y >= 5 {
                    (9 - y) as u8
                } else {
                    y as u8
                };
                assert_eq!(out.get_pixel(x, y).0[1], expected, "x={x} y={y}");
            }
        }
    }

    #[test]
    fn auto_vertical_picks_busier_bottom() {
        let img = RgbaImage::from_fn(8, 10, |_, y| {
            if y < 5 {
                Rgba([128, 128, 128, 255])
            } else if y % 2 == 0 {
                Rgba([255, 255, 255, 255])
            } else {
                Rgba([0, 0, 0, 255])
            }
        });
        assert_eq!(
            auto_keep_side(&img, Rect::new(0, 0, 8, 10), MirrorAxis::Vertical),
            KeepSide::Bottom
        );
    }

    #[test]
    fn mirror_odd_width_keeps_center() {
        let img = coord_image(5, 1);
        let out = mirror_image(&img, Rect::new(0, 0, 5, 1), Direction::Left);
        // 右半 (x=3,4) 取自 (1,0)，中心列 x=2 不变
        let expect = [0u8, 1, 2, 1, 0];
        for x in 0..5 {
            assert_eq!(out.get_pixel(x, 0).0[0], expect[x as usize], "x={x}");
        }
    }

    #[test]
    fn mirror_partial_box_leaves_outside_untouched() {
        let img = coord_image(10, 4);
        let out = mirror_image(&img, Rect::new(2, 1, 8, 3), Direction::Left);
        for y in 0..4 {
            for x in 0..10 {
                let inside = (2..8).contains(&x) && (1..3).contains(&y);
                let expect = if inside && x >= 5 {
                    (9 - x) as u8
                } else {
                    x as u8
                };
                assert_eq!(out.get_pixel(x, y).0[0], expect, "x={x} y={y}");
            }
        }
    }

    #[test]
    fn mirror_output_size_unchanged() {
        let img = coord_image(17, 9);
        let out = mirror_image(&img, Rect::new(-5, -5, 30, 30), Direction::Left);
        assert_eq!(out.dimensions(), (17, 9));
    }

    #[test]
    fn mirror_unnormalized_box() {
        let img = coord_image(6, 1);
        let out = mirror_image(&img, Rect::new(6, 0, 0, 1), Direction::Left);
        assert_eq!(out.get_pixel(5, 0).0[0], 0);
    }

    #[test]
    fn auto_direction_picks_busier_side() {
        // 左半均匀灰，右半强对比竖条纹（信息量大）。
        let img = RgbaImage::from_fn(10, 8, |x, _| {
            if x < 5 {
                Rgba([128, 128, 128, 255])
            } else if x % 2 == 0 {
                Rgba([255, 255, 255, 255])
            } else {
                Rgba([0, 0, 0, 255])
            }
        });
        let sel = Rect::new(0, 0, 10, 8);
        assert_eq!(auto_direction(&img, sel), Direction::Right);
    }

    #[test]
    fn degenerate_box_is_noop() {
        let img = coord_image(4, 4);
        let out = mirror_image(&img, Rect::new(1, 1, 2, 2), Direction::Left);
        assert_eq!(out, img);
    }

    #[test]
    fn iou_overlap_and_disjoint() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 15, 15);
        assert!((a.iou(b) - 25.0 / 175.0).abs() < 1e-5);
        assert_eq!(a.iou(Rect::new(20, 20, 30, 30)), 0.0);
        assert_eq!(a.iou(a), 1.0);
    }

    #[test]
    fn mirror_regions_applies_each_box_from_original() {
        let img = coord_image(12, 2);
        let left = Rect::new(0, 0, 4, 2);
        let right = Rect::new(8, 0, 12, 2);
        let out = mirror_regions(&img, &[(left, Direction::Left), (right, Direction::Right)]);
        // 左框保留左半：x=0,1 不变，x=2,3 取对称
        assert_eq!(out.get_pixel(0, 0).0[0], 0);
        assert_eq!(out.get_pixel(3, 0).0[0], 0);
        // 右框保留右半：x=10,11 不变，x=8,9 取对称
        assert_eq!(out.get_pixel(11, 0).0[0], 11);
        assert_eq!(out.get_pixel(8, 0).0[0], 11);
        // 框外不动
        assert_eq!(out.get_pixel(6, 0).0[0], 6);
    }

    #[test]
    fn copy_rect_restores_original_over_mirror() {
        let img = coord_image(8, 2);
        let sel = Rect::new(0, 0, 8, 2);
        let mut out = mirror_image(&img, sel, Direction::Left);
        copy_rect(&mut out, &img, Rect::new(4, 0, 8, 2));
        assert_eq!(out.get_pixel(6, 0).0[0], 6);
        assert_eq!(out.get_pixel(1, 0).0[0], 1);
    }

    #[test]
    fn crop_to_rect_extracts_region() {
        let img = coord_image(10, 8);
        let out = crop_to_rect(&img, Rect::new(2, 3, 6, 7));
        assert_eq!(out.dimensions(), (4, 4));
        assert_eq!(out.get_pixel(0, 0).0, [2, 3, 0, 255]);
        assert_eq!(out.get_pixel(3, 3).0, [5, 6, 0, 255]);
    }

    #[test]
    fn crop_to_rect_empty_is_clone() {
        let img = coord_image(4, 4);
        let out = crop_to_rect(&img, Rect::new(1, 1, 1, 3));
        assert_eq!(out.dimensions(), img.dimensions());
    }
}
