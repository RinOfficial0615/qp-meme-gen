# qp-meme-gen

「！？强强？！」对称镜像梗图生成器。Windows 桌面，单个 exe，运行时完全离线。

在选框内以中心竖线为轴，把保留的一半水平翻转覆盖另一半。输出尺寸与输入相同，框外像素不动。叠加文字先画到源图，再走同一套镜像。

## 功能

- 打开图片：按钮、拖入窗口、拖到 exe、Ctrl+V / Shift+Insert 粘贴
- 默认框选：单人脸 / 多人脸 / 整张图片
- 多处框选：每框独立方向与「查看原图」；「显示角标」控制角上编号；加框按未框住的最高分人脸，没有则放画面中央（宽高各 40%）
- 翻转方向：自动 / 保留左半 / 保留右半
- 叠加文字：点工具栏「文字」，再点画面添加；字号、白/黑/黄/红；拖动移动，再点一次进入编辑。文字随选框一起镜像
- 复制到剪贴板或另存为图片
- WinUI 3 风格浅色 / 深色 / 跟随系统

## 构建

需要 [Rust](https://rustup.rs/)（edition 2024）、Windows，以及首次构建时的网络（从 Hugging Face 拉人脸模型）。

```bat
cargo build --release
```

第一次编译会下载 InsightFace `buffalo_l` 里的 SCRFD-10GF（`det_10g.onnx`，约 16 MiB），校验 sha256 后缓存在 `assets/`（已 gitignore）。之后离线也能编。手动放置：

<https://huggingface.co/deepghs/insightface/blob/main/buffalo_l/det_10g.onnx>

保存为 `assets/det_10g.onnx`。

产物：`target\release\qp-meme-gen.exe`。模型编译期内嵌，发布时只需这一个文件。

```bat
cargo test
```

## 使用

1. 运行 exe，拖入、选择或粘贴一张图。
2. 拖动鼠标调整选框；空白处拖拽可加框（整图模式除外）。
3. 选中某个框后，可单独改方向和「查看原图」。
4. 需要字幕时点「文字」，再点画面输入；文字会跟选框一起左右对称。
5. **保存图片** 或 **复制图片**。

更细的操作见 [docs/usage.md](docs/usage.md)。

## 配置

exe 同目录 `qp-meme-gen.toml`。主页设置里改动会立刻写入。读失败用默认值；写失败仅当前会话生效并提示。

```toml
default_crop_mode = "single"   # "single" | "multi" | "full"
default_direction = "auto"     # "left" | "right" | "auto"
appearance = "system"          # "system" | "light" | "dark"
```

## 限制

- 仅 Windows
- GIF 只取首帧
- 不处理 EXIF 方向
- 文字依赖系统中文字体（微软雅黑 / 黑体 / 宋体）

## 文档

- [使用说明](docs/usage.md)
- [架构](docs/architecture.md)
- [开发](docs/development.md)
