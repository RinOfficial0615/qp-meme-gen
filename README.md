# qp-meme-gen

「！？强强？！」对称镜像梗图生成器。Windows 桌面，单个 exe，运行时完全离线。

在选框内以中心竖线为轴，把保留的一半水平翻转覆盖另一半。输出尺寸与输入相同，框外像素不动。

## 功能

- 打开图片：按钮、拖入窗口、拖到 exe、Ctrl+V / Shift+Insert 粘贴
- 默认框选：单人脸 / 多人脸 / 整张图片
- 多处框选：每框独立方向与「查看原图」；加框按未框住的最高分人脸，没有则放画面中央（宽高各 40%）
- 翻转方向：自动 / 保留左半 / 保留右半
- 复制到剪贴板或另存为图片
- WinUI 3 风格浅色 / 深色 / 跟随系统

## 构建

需要 [Rust](https://rustup.rs/)（edition 2024）和 Windows。

```bat
cargo build --release
```

产物：`target\release\qp-meme-gen.exe`。人脸模型编译期内嵌，发布时只需这一个文件。

```bat
cargo test
```

## 使用

1. 运行 exe，拖入、选择或粘贴一张图。
2. 拖动手柄调整选框；空白处拖拽可加框（整图模式除外）。
3. 选中某个框后，可单独改方向和「查看原图」。
4. **保存图片** 或 **复制图片**。

更细的操作见 [docs/usage.md](docs/usage.md)。

## 配置

exe 同目录 `qp-meme-gen.toml`。主页设置里改动会立刻写盘。读失败用默认值；写失败仅当前会话生效并提示。

```toml
default_crop_mode = "single"   # "single" | "multi" | "full"
default_direction = "auto"     # "left" | "right" | "auto"
appearance = "system"          # "system" | "light" | "dark"
```

## 限制

- 仅 Windows
- GIF 只取首帧
- 不处理 EXIF 方向

## 文档

- [使用说明](docs/usage.md)
- [架构](docs/architecture.md)
- [开发](docs/development.md)
