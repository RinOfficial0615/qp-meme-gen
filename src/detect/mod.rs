//! 人脸检测门面。接口刻意做窄：`new` + `detect`。
//! ONNX session、letterbox、anchor 解码、NMS 全部隐藏在 `scrfd` 实现里。

mod scrfd;

use anyhow::Result;
use image::RgbaImage;

/// 原图坐标系下的点。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point2 {
    pub x: f32,
    pub y: f32,
}

/// 一次人脸检测结果（原图坐标）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    /// `[x0, y0, x1, y1]`。
    pub bbox: [f32; 4],
    /// 5 关键点：左眼、右眼、鼻尖、左嘴角、右嘴角。
    pub keypoints: [Point2; 5],
    pub score: f32,
}

impl Face {
    pub fn area(&self) -> f32 {
        (self.bbox[2] - self.bbox[0]).max(0.0) * (self.bbox[3] - self.bbox[1]).max(0.0)
    }
}

/// SCRFD 检测器。模型字节编译期内嵌，构造即可用，运行时无 IO。
pub struct FaceDetector {
    model: rten::Model,
}

static MODEL_BYTES: &[u8] = include_bytes!("../../assets/scrfd_2.5g_bnkps.onnx");

impl FaceDetector {
    pub fn new() -> Result<Self> {
        let model = rten::Model::load_static_slice(MODEL_BYTES)
            .map_err(|e| anyhow::anyhow!("加载内嵌人脸模型失败: {e}"))?;
        Ok(Self { model })
    }

    /// 检测所有人脸，按置信度降序。
    pub fn detect(&self, img: &RgbaImage) -> Result<Vec<Face>> {
        scrfd::detect(&self.model, img)
    }

    /// 置信度最高的最大人脸（先按分数过滤，再取面积最大者）。
    pub fn detect_primary(&self, img: &RgbaImage) -> Result<Option<Face>> {
        let mut faces = self.detect(img)?;
        faces.sort_by(|a, b| {
            b.area()
                .partial_cmp(&a.area())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(faces.into_iter().next())
    }
}
