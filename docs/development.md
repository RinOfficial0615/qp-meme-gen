# 开发

## 构建与测试

首次 `cargo build` / `cargo test` 需要访问 Hugging Face，下载 `assets/det_10g.onnx`（约 16 MiB，sha256 见 `build.rs`）。文件已存在且校验通过则不再下载，之后可离线编译。`assets/*.onnx` 已 gitignore，不要提交。

```bat
cargo build
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

Release：`lto = true`，`codegen-units = 1`，`strip = true`。debug / release 都隐藏控制台；panic 走系统错误对话框。

手动放模型：<https://huggingface.co/deepghs/insightface/resolve/main/buffalo_l/det_10g.onnx> → `assets/det_10g.onnx`。

## 人脸模型

InsightFace 1.0 的更新是 Evaluation Studio GUI 和默认双尺度检测（128 + 640），检测权重仍是 v0.7 起那套 SCRFD。官方默认包 `buffalo_l` 用 SCRFD-10GF（`det_10g.onnx`）。`deepghs/insightface` 还带 `buffalo_s/det_500m.onnx`（更小、Hard 集更差）；没有 2.5G bnkps。本项目跟默认检测器，不跟独立的 `scrfd_2.5g_bnkps`。

换权重时：确认 9 路输出在 640 输入下仍是 12800/3200/800 行、列 1/4/10，并改 `build.rs` 的 URL / size / sha256。

## 测试分层

| 位置 | 覆盖 |
| --- | --- |
| `core::mirror` | 左/右方向、奇数宽、框外不动、自动方向、多框 |
| `core::text` | 空串不画、中文字形落在中心附近（无系统字体则跳过） |
| `core::crop` | 人脸框几何、中央框比例、未覆盖脸选取 |
| `detect::scrfd` | 解码、NMS、低分过滤 |
| `app::clipboard` | 粘贴手势、DIB、file://、HTML `src=` |
| `config` | 读写、`"face"` 别名、缺省字段 |
| `ui::editor` | 加框选脸、整图禁止加框、文字叠层 |
| `tests/e2e.rs` | 真实模型：`face.png` 有脸、`people.png` 多人、`no_face_ui.png` 无脸、整图镜像对称 |

夹具在 `tests/fixtures/`。`examples/make_sample.rs` 用人脸夹具打一份镜像样本，方便肉眼看。

## 调试

环境变量 `QP_SHOT=路径.png`：下一帧用 egui Screenshot 把 UI 存成 PNG，不依赖屏幕捕获。适合远程排查和自动化验收。

## 离线

运行时无网络。源码无 HTTP 客户端依赖；`build.rs` 只在缺模型时调系统 `curl` / PowerShell。可用 `cargo tree` 检查运行时依赖里是否出现 HTTP 客户端之类。
