//! 把文字画进 RGBA 图：纯函数，无 GUI 依赖。
//! 编辑器先把叠加文字画到源图，再走与照片相同的选框镜像。

use std::sync::OnceLock;

use ab_glyph::{Font, FontVec, Glyph, PxScale, ScaleFont, point};
use image::RgbaImage;

pub(crate) const FONT_CANDIDATES: [&str; 3] = [
    r"C:\Windows\Fonts\msyh.ttc",
    r"C:\Windows\Fonts\simhei.ttf",
    r"C:\Windows\Fonts\simsun.ttc",
];

static FONT: OnceLock<Option<FontVec>> = OnceLock::new();

/// 系统中文字体。全失败时返回 `None`。
pub fn system_font() -> Option<&'static FontVec> {
    FONT.get_or_init(load_system_font).as_ref()
}

fn load_system_font() -> Option<FontVec> {
    for path in FONT_CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if let Ok(font) = FontVec::try_from_vec_and_index(bytes.clone(), 0) {
            return Some(font);
        }
        if let Ok(font) = FontVec::try_from_vec(bytes) {
            return Some(font);
        }
    }
    None
}

struct Layout {
    glyphs: Vec<Glyph>,
    width: f32,
    height: f32,
}

fn layout(font: &FontVec, content: &str, size: f32) -> Option<Layout> {
    if content.is_empty() || size <= 0.0 {
        return None;
    }
    let scale = PxScale::from(size);
    let scaled = font.as_scaled(scale);
    let line_h = scaled.height().max(1.0);
    let ascent = scaled.ascent();

    let mut glyphs = Vec::new();
    let mut x = 0.0f32;
    let mut line = 0u32;
    let mut max_w = 0.0f32;

    for c in content.chars() {
        if c == '\n' {
            max_w = max_w.max(x);
            x = 0.0;
            line += 1;
            continue;
        }
        let gid = font.glyph_id(c);
        let g = gid.with_scale_and_position(scale, point(x, ascent + line as f32 * line_h));
        x += scaled.h_advance(gid);
        if !c.is_whitespace() {
            glyphs.push(g);
        }
    }
    max_w = max_w.max(x);
    let height = (line + 1) as f32 * line_h;
    if max_w <= 0.0 || height <= 0.0 {
        return None;
    }
    Some(Layout {
        glyphs,
        width: max_w,
        height,
    })
}

/// 以 `(cx, cy)` 为中心的轴对齐包围盒（含描边余量）。
pub fn bounds(
    font: &FontVec,
    content: &str,
    cx: f32,
    cy: f32,
    size: f32,
) -> Option<(f32, f32, f32, f32)> {
    if size <= 0.0 {
        return None;
    }
    let (width, height) = match layout(font, content, size) {
        Some(laid) => (laid.width.max(size * 2.0), laid.height),
        None => (size * 4.0, size),
    };
    let pad = (size / 14.0).clamp(2.0, 8.0) + 6.0;
    let x0 = cx - width * 0.5 - pad;
    let y0 = cy - height * 0.5 - pad;
    Some((x0, y0, x0 + width + pad * 2.0, y0 + height + pad * 2.0))
}

pub fn hit(font: &FontVec, content: &str, cx: f32, cy: f32, size: f32, x: f32, y: f32) -> bool {
    let Some((x0, y0, x1, y1)) = bounds(font, content, cx, cy, size) else {
        return false;
    };
    x >= x0 && x < x1 && y >= y0 && y < y1
}

/// 在图上画带描边的文字，中心点 `(cx, cy)`。越界部分裁掉。
pub fn draw(
    img: &mut RgbaImage,
    font: &FontVec,
    content: &str,
    center: (f32, f32),
    size: f32,
    fill: [u8; 3],
    outline: [u8; 3],
) {
    let Some(laid) = layout(font, content, size) else {
        return;
    };
    let ox = center.0 - laid.width * 0.5;
    let oy = center.1 - laid.height * 0.5;
    let stroke = (size / 14.0).clamp(1.5, 8.0);
    let offsets = [
        (-stroke, 0.0),
        (stroke, 0.0),
        (0.0, -stroke),
        (0.0, stroke),
        (-stroke, -stroke),
        (stroke, -stroke),
        (-stroke, stroke),
        (stroke, stroke),
    ];

    for g in &laid.glyphs {
        let mut positioned = g.clone();
        positioned.position.x += ox;
        positioned.position.y += oy;
        let Some(outlined) = font.outline_glyph(positioned) else {
            continue;
        };
        let bb = outlined.px_bounds();
        for (dx, dy) in offsets {
            outlined.draw(|x, y, cov| {
                blend(
                    img,
                    bb.min.x + x as f32 + dx,
                    bb.min.y + y as f32 + dy,
                    outline,
                    cov,
                );
            });
        }
        outlined.draw(|x, y, cov| {
            blend(img, bb.min.x + x as f32, bb.min.y + y as f32, fill, cov);
        });
    }
}

fn blend(img: &mut RgbaImage, x: f32, y: f32, rgb: [u8; 3], cover: f32) {
    if cover <= 0.01 {
        return;
    }
    let ix = x.round() as i32;
    let iy = y.round() as i32;
    let w = img.width() as i32;
    let h = img.height() as i32;
    if ix < 0 || iy < 0 || ix >= w || iy >= h {
        return;
    }
    let a = cover.clamp(0.0, 1.0);
    let px = img.get_pixel_mut(ix as u32, iy as u32);
    for (dst, src) in px.0.iter_mut().take(3).zip(rgb) {
        *dst = (src as f32 * a + *dst as f32 * (1.0 - a)).round() as u8;
    }
    px.0[3] = 255;
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn font_or_skip() -> Option<&'static FontVec> {
        system_font()
    }

    #[test]
    fn empty_text_is_noop() {
        let mut img = RgbaImage::from_pixel(16, 16, Rgba([7, 7, 7, 255]));
        let Some(font) = font_or_skip() else {
            return;
        };
        draw(
            &mut img,
            font,
            "",
            (8.0, 8.0),
            12.0,
            [255, 255, 255],
            [0, 0, 0],
        );
        assert!(img.pixels().all(|p| p.0 == [7, 7, 7, 255]));
        let (x0, y0, x1, y1) = bounds(font, "", 8.0, 8.0, 12.0).unwrap();
        assert!(x0 < 8.0 && x1 > 8.0 && y0 < 8.0 && y1 > 8.0);
        assert!(!hit(font, "A", 8.0, 8.0, 12.0, 100.0, 100.0));
    }

    #[test]
    fn draw_paints_near_center() {
        let Some(font) = font_or_skip() else {
            eprintln!("skip: no system CJK font");
            return;
        };
        let mut img = RgbaImage::from_pixel(80, 80, Rgba([0, 0, 0, 255]));
        draw(
            &mut img,
            font,
            "强",
            (40.0, 40.0),
            36.0,
            [255, 255, 255],
            [0, 0, 0],
        );
        let painted = img
            .pixels()
            .filter(|p| p.0[0] > 20 || p.0[1] > 20 || p.0[2] > 20)
            .count();
        assert!(
            painted > 20,
            "expected glyphs to cover some pixels, got {painted}"
        );
        assert!(hit(font, "强", 40.0, 40.0, 36.0, 40.0, 40.0));
        let (x0, y0, x1, y1) = bounds(font, "强", 40.0, 40.0, 36.0).unwrap();
        assert!(x0 < 40.0 && x1 > 40.0 && y0 < 40.0 && y1 > 40.0);
    }
}
