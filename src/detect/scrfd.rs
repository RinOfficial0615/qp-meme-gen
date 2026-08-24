//! SCRFD 推理：letterbox 预处理、anchor 解码、NMS。
//! 默认权重是 InsightFace buffalo_l 的 SCRFD-10GF（`det_10g.onnx`，5 关键点）。
//! 仅供 `detect::FaceDetector` 内部使用。

use anyhow::{Context, Result, anyhow, bail};
use image::RgbaImage;
use rten::{Model, Value};
use rten_tensor::NdTensor;

use super::{Face, Point2};

const INPUT: usize = 640;
const STRIDES: [usize; 3] = [8, 16, 32];
const NUM_ANCHORS: usize = 2;
const SCORE_THRESH: f32 = 0.5;
const NMS_IOU: f32 = 0.4;

/// 按 stride 分组的原始输出张量（行 = anchor 数）。
struct Branch {
    scores: Vec<f32>,    // [n]
    bboxes: Vec<f32>,    // [n * 4]，到 anchor 中心的 ltrb 距离
    keypoints: Vec<f32>, // [n * 10]
    rows: usize,
}

pub(super) fn detect(model: &Model, img: &RgbaImage) -> Result<Vec<Face>> {
    let Letterbox {
        data,
        scale,
        pad_x,
        pad_y,
    } = letterbox(img);

    let input_id = *model.input_ids().first().context("模型没有输入节点")?;
    let input = NdTensor::from_data([1, 3, INPUT, INPUT], data);

    let output_ids = model.output_ids().to_vec();
    let outputs = model
        .run(vec![(input_id, input.into())], &output_ids, None)
        .map_err(|e| anyhow!("SCRFD 推理失败: {e}"))?;

    let branches = collect_branches(model, &output_ids, &outputs)?;
    let mut faces = decode_all(&branches);
    faces = faces
        .into_iter()
        .filter(|f| f.score >= SCORE_THRESH)
        .map(|f| unletterbox(f, scale, pad_x, pad_y))
        .collect();
    let mut faces = nms(faces, NMS_IOU);
    faces.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(faces)
}

/// 等比缩放 + 114 灰边填充到 640×640，输出 NCHW 归一化数据。
struct Letterbox {
    data: Vec<f32>,
    scale: f32,
    pad_x: f32,
    pad_y: f32,
}

fn letterbox(img: &RgbaImage) -> Letterbox {
    let (w, h) = (img.width(), img.height());
    let scale = (INPUT as f32 / w as f32).min(INPUT as f32 / h as f32);
    let nw = ((w as f32 * scale).round() as u32).max(1);
    let nh = ((h as f32 * scale).round() as u32).max(1);
    let resized = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle);
    let pad_x = (INPUT as u32 - nw) / 2;
    let pad_y = (INPUT as u32 - nh) / 2;

    let pad_val = (114.0f32 - 127.5) / 128.0;
    let mut data = vec![pad_val; 3 * INPUT * INPUT];
    let plane = INPUT * INPUT;
    for y in 0..nh {
        for x in 0..nw {
            let p = resized.get_pixel(x, y);
            let dst = (y + pad_y) as usize * INPUT + (x + pad_x) as usize;
            data[dst] = (p[0] as f32 - 127.5) / 128.0; // R
            data[plane + dst] = (p[1] as f32 - 127.5) / 128.0; // G
            data[2 * plane + dst] = (p[2] as f32 - 127.5) / 128.0; // B
        }
    }
    Letterbox {
        data,
        scale,
        pad_x: pad_x as f32,
        pad_y: pad_y as f32,
    }
}

fn unletterbox(mut f: Face, scale: f32, pad_x: f32, pad_y: f32) -> Face {
    let fix = |x: f32, y: f32| Point2 {
        x: (x - pad_x) / scale,
        y: (y - pad_y) / scale,
    };
    let p0 = fix(f.bbox[0], f.bbox[1]);
    let p1 = fix(f.bbox[2], f.bbox[3]);
    f.bbox = [p0.x, p0.y, p1.x, p1.y];
    for kp in &mut f.keypoints {
        *kp = fix(kp.x, kp.y);
    }
    f
}

/// 把模型输出按名字/形状归类为三个 stride 分支。
fn collect_branches(model: &Model, ids: &[rten::NodeId], outputs: &[Value]) -> Result<Vec<Branch>> {
    let mut by_stride: [Option<Branch>; 3] = [None, None, None];

    for (id, value) in ids.iter().zip(outputs) {
        let name = model
            .node_info(*id)
            .and_then(|info| info.name().map(|s| s.to_string()))
            .unwrap_or_default();
        let (shape, data) = flatten_f32(value)?;
        let rows = if shape.len() >= 2 {
            shape[shape.len() - 2]
        } else {
            data.len()
        };
        let cols = if shape.len() >= 2 {
            shape[shape.len() - 1]
        } else {
            1
        };
        let kind = classify(&name, cols, data.len())
            .with_context(|| format!("无法识别的模型输出: name={name:?} shape={shape:?}"))?;
        let stride_idx = stride_index(&name, rows)
            .with_context(|| format!("无法从输出行数推断 stride: name={name:?} rows={rows}"))?;
        let slot = &mut by_stride[stride_idx];
        let entry = slot.get_or_insert_with(|| Branch {
            scores: Vec::new(),
            bboxes: Vec::new(),
            keypoints: Vec::new(),
            rows,
        });
        match kind {
            Kind::Score => entry.scores = data,
            Kind::BBox => entry.bboxes = data,
            Kind::Kps => entry.keypoints = data,
        }
    }

    let mut branches = Vec::with_capacity(3);
    for (i, slot) in by_stride.into_iter().enumerate() {
        let branch = slot.ok_or_else(|| anyhow!("模型缺少 stride={} 的输出分支", STRIDES[i]))?;
        if branch.scores.len() != branch.rows
            || branch.bboxes.len() != branch.rows * 4
            || branch.keypoints.len() != branch.rows * 10
        {
            bail!(
                "stride={} 分支形状不符: rows={} score={} bbox={} kps={}",
                STRIDES[i],
                branch.rows,
                branch.scores.len(),
                branch.bboxes.len(),
                branch.keypoints.len()
            );
        }
        branches.push(branch);
    }
    Ok(branches)
}

enum Kind {
    Score,
    BBox,
    Kps,
}

fn classify(name: &str, cols: usize, len: usize) -> Option<Kind> {
    let n = name.to_ascii_lowercase();
    if n.contains("score") || n.contains("cls") {
        return Some(Kind::Score);
    }
    if n.contains("bbox") || n.contains("reg") {
        return Some(Kind::BBox);
    }
    if n.contains("kps") || n.contains("landmark") || n.contains("point") {
        return Some(Kind::Kps);
    }
    let _ = len;
    // 名字不可靠时按列数判断
    match cols {
        1 => Some(Kind::Score),
        4 => Some(Kind::BBox),
        10 => Some(Kind::Kps),
        _ => None,
    }
}

fn stride_index(name: &str, rows: usize) -> Option<usize> {
    for (i, s) in STRIDES.iter().enumerate() {
        if name.ends_with(&s.to_string()) {
            return Some(i);
        }
    }
    // 按 anchor 行数推断：rows = 2 * (640/stride)^2
    STRIDES
        .iter()
        .position(|s| NUM_ANCHORS * (INPUT / s) * (INPUT / s) == rows)
}

/// 展平任意 rank 的 f32 输出为 (shape, data)。
/// `into_shape_vec` 按值消费且错误不带回原值，故逐 rank 克隆尝试（仅失败路径有开销）。
fn flatten_f32(value: &Value) -> Result<(Vec<usize>, Vec<f32>)> {
    if let Ok((shape, data)) = value.clone().into_shape_vec::<f32, 3>() {
        return Ok((shape.to_vec(), data));
    }
    if let Ok((shape, data)) = value.clone().into_shape_vec::<f32, 2>() {
        return Ok((shape.to_vec(), data));
    }
    if let Ok((shape, data)) = value.clone().into_shape_vec::<f32, 1>() {
        return Ok((shape.to_vec(), data));
    }
    Err(anyhow!("输出不是 f32 张量或 rank 超出预期"))
}

/// 逐 stride 解码 anchor 为人脸框（letterbox 坐标系）。
fn decode_all(branches: &[Branch]) -> Vec<Face> {
    let mut out = Vec::new();
    for (i, b) in branches.iter().enumerate() {
        let stride = STRIDES[i] as f32;
        let fmc = INPUT / STRIDES[i];
        debug_assert_eq!(b.rows, fmc * fmc * NUM_ANCHORS);
        for idx in 0..b.rows {
            // InsightFace 导出的 SCRFD ONNX 已在图内做完 sigmoid，分数就是 [0,1] 概率。
            // 再套一次会把背景 ~0.03 抬成 ~0.51，0.5 阈值形同虚设。
            let score = b.scores[idx];
            if score < SCORE_THRESH {
                continue;
            }
            let cell = idx / NUM_ANCHORS;
            let cx = ((cell % fmc) as f32) * stride;
            let cy = ((cell / fmc) as f32) * stride;

            let d = &b.bboxes[idx * 4..idx * 4 + 4];
            let x0 = cx - d[0] * stride;
            let y0 = cy - d[1] * stride;
            let x1 = cx + d[2] * stride;
            let y1 = cy + d[3] * stride;

            let mut keypoints = [Point2::default(); 5];
            for (k, kp) in keypoints.iter_mut().enumerate() {
                *kp = Point2 {
                    x: cx + b.keypoints[idx * 10 + k * 2] * stride,
                    y: cy + b.keypoints[idx * 10 + k * 2 + 1] * stride,
                };
            }
            out.push(Face {
                bbox: [x0, y0, x1, y1],
                keypoints,
                score,
            });
        }
    }
    out
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = a[2].min(b[2]);
    let y1 = a[3].min(b[3]);
    let inter = (x1 - x0).max(0.0) * (y1 - y0).max(0.0);
    let area_a = (a[2] - a[0]).max(0.0) * (a[3] - a[1]).max(0.0);
    let area_b = (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let union = area_a + area_b - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// 贪心 NMS：按分数降序保留，抑制 IoU 超阈值者。
fn nms(mut faces: Vec<Face>, thresh: f32) -> Vec<Face> {
    faces.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut kept: Vec<Face> = Vec::with_capacity(faces.len());
    'outer: for f in faces {
        for k in &kept {
            if iou(&f.bbox, &k.bbox) > thresh {
                continue 'outer;
            }
        }
        kept.push(f);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(x0: f32, y0: f32, x1: f32, y1: f32, score: f32) -> Face {
        Face {
            bbox: [x0, y0, x1, y1],
            keypoints: [Point2::default(); 5],
            score,
        }
    }

    #[test]
    fn nms_suppresses_overlap() {
        let faces = vec![
            face(0.0, 0.0, 10.0, 10.0, 0.9),
            face(1.0, 1.0, 11.0, 11.0, 0.8), // 与上一个 IoU ≈ 0.68
            face(100.0, 100.0, 110.0, 110.0, 0.7),
        ];
        let kept = nms(faces, 0.4);
        assert_eq!(kept.len(), 2);
        assert!((kept[0].score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn nms_keeps_disjoint() {
        let faces = vec![
            face(0.0, 0.0, 10.0, 10.0, 0.9),
            face(20.0, 20.0, 30.0, 30.0, 0.8),
        ];
        assert_eq!(nms(faces, 0.4).len(), 2);
    }

    #[test]
    fn decode_rejects_low_score() {
        // 概率分数 < 0.5 → 解码为空（ONNX 输出已是概率，不再套 sigmoid）
        let b = Branch {
            scores: vec![0.04; 12800],
            bboxes: vec![1.0; 12800 * 4],
            keypoints: vec![0.0; 12800 * 10],
            rows: 12800,
        };
        assert!(decode_all(&[b]).is_empty());
    }

    #[test]
    fn decode_places_box_around_anchor() {
        // stride=8 分支第 0 行 anchor 中心 (0,0)，距离全 1 -> 框 [-8,-8,8,8]
        let mut scores = vec![0.04f32; 12800];
        scores[0] = 0.9;
        let mut bboxes = vec![0.0f32; 12800 * 4];
        bboxes[0..4].copy_from_slice(&[1.0, 1.0, 1.0, 1.0]);
        let b = Branch {
            scores,
            bboxes,
            keypoints: vec![0.0; 12800 * 10],
            rows: 12800,
        };
        let faces = decode_all(&[b]);
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].bbox, [-8.0, -8.0, 8.0, 8.0]);
    }

    #[test]
    fn classify_by_columns() {
        assert!(matches!(classify("foo", 1, 100).unwrap(), Kind::Score));
        assert!(matches!(classify("foo", 4, 400).unwrap(), Kind::BBox));
        assert!(matches!(classify("foo", 10, 1000).unwrap(), Kind::Kps));
        assert!(classify("foo", 7, 70).is_none());
    }
}
