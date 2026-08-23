# 开发

## 构建与测试

```bat
cargo build
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

Release：`lto = true`，`codegen-units = 1`，`strip = true`。debug / release 都隐藏控制台；panic 走系统错误对话框。

## 测试分层

| 位置 | 覆盖 |
| --- | --- |
| `core::mirror` | 左/右方向、奇数宽、框外不动、自动方向、多框 |
| `detect::scrfd` | 解码、NMS、低分过滤 |
| `clipboard` | 粘贴手势、DIB、file://、HTML `src=` |
| `config` | 读写、`"face"` 别名、缺省字段 |
| `ui::editor` | 人脸框几何、加框选脸、整图禁止加框、中央框比例 |
| `tests/e2e.rs` | 真实模型：`face.png` 有脸、`no_face_ui.png` 无脸、整图镜像对称 |

夹具在 `tests/fixtures/`。`examples/make_sample.rs` 用人脸夹具打一份镜像样本，方便肉眼看。

## 调试

环境变量 `QP_SHOT=路径.png`：下一帧用 egui Screenshot 把 UI 存成 PNG，不依赖屏幕捕获。适合远程排查和自动化验收。

## 离线

模型编进二进制。源码无网络调用。可用 `cargo tree` 检查依赖里是否出现 HTTP 客户端之类。
