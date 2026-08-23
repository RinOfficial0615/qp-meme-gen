# 架构

运行时无网络、无遥测。SCRFD 模型以 `include_bytes!` 编进二进制，`rten::Model::load_static_slice` 加载。

```
src/
  main.rs           入口：argv、隐藏控制台、转交 lib
  lib.rs            启动 GUI、注入 CJK 字体、panic 弹窗
  app.rs            Home / Editor 状态机，打开与检测的汇合点
  config.rs         exe 旁 qp-meme-gen.toml
  clipboard.rs      Windows 读图（对齐 Chromium clipboard_win.cc）
  core/mirror.rs    镜像纯函数
  detect/           FaceDetector 门面 + SCRFD
  ui/               主页、编辑器、设置、主题、toast
assets/
  scrfd_2.5g_bnkps.onnx
```

`main.rs` 只做薄壳。测试走 crate 模块，不依赖 GUI 进程。

## 镜像

选框半开区间 `[x0, x1) × [y0, y1)`，轴在框中心。

- 保留左半：`x > 对称列` 的像素取 `src(x0+x1-1-x, y)`
- 保留右半：对称
- 奇数宽：中心列不变
- 框外：不动

多个框：从原图分别镜像再写入输出，后写覆盖重叠。某框勾了「查看原图」则该区域拷回原图像素。

自动方向不比较接缝（镜像后轴两侧必然同源，违和度恒零），改为比较保留半区的水平梯度能量，隔行采样；打平取左。

## 人脸框

双眼中点为轴心，左右对称扩张覆盖 bbox，左右各外扩约 10%，顶部多扩。越界钳制到图像范围。

无人脸时的中央框：宽、高各为画面的 40%，居中。已有框是否「占用」一张脸：脸中心落在某框内，或与脸 bbox 的 IoU > 0.3。

## SCRFD

InsightFace `scrfd_2.5g_bnkps`（5 关键点）。

- letterbox 到 640×640，114 灰边，`(x-127.5)/128`，RGB，NCHW
- stride `{8,16,32}` × 2 anchors
- ONNX 分数已是 sigmoid 概率，不用再套一层；阈值 0.5，NMS IoU 0.4
- `detect()` 按分数降序；单人模式再取面积最大者

打开图片且默认模式为人脸时同步检测一次；编辑器「人脸框选」再跑。

## 粘贴

格式链：注册格式 `"PNG"` → `CF_DIB`（系统可从 BITMAP / DIBV5 合成）→ `CF_HDROP` 图片文件 → HTML `file://` / 文本里的本地图片路径。

`OpenClipboard` 失败重试 5 次、间隔 5ms（rdpclip 争用）。单次数据上限 256MiB。

egui-winit 在剪贴板无文本时会吞掉 Ctrl+V 按下。粘贴命令由 `Event::Paste` 与被吞按键的松开合成，不轮询 `GetAsyncKeyState`。

## UI

WinUI 3 浅/深双色板。选项类控件：未选中灰边，选中或悬停强调色边。人脸/整图/加框带 250ms ease-out。主页 ↔ 编辑器淡入+水平滑入。Toast 底部独立定位，淡出时收掉占位高度。

中文字体启动时按序尝试 `msyh.ttc` / `simhei.ttf` / `simsun.ttc`。
