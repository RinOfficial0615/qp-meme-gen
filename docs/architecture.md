# 架构

运行时无网络、无遥测。人脸模型在 **构建期** 从 Hugging Face 下载，再以 `include_bytes!` 编进二进制，`rten::Model::load_static_slice` 加载。

```
src/
  main.rs           入口：argv、隐藏控制台、转交 lib
  lib.rs            启动 GUI、注入 CJK 字体、panic 弹窗
  app/              Home / Editor 状态机；clipboard 只给粘贴用
  config.rs         设置模型 + exe 旁 qp-meme-gen.toml
  core/             镜像、叠加文字、人脸→选框（纯函数，无 GUI）
  detect/           FaceDetector 门面 + SCRFD
  ui/               主页、编辑器（canvas / overlay）、设置、主题、toast
assets/
  det_10g.onnx      构建脚本下载，不进 git
build.rs            Hugging Face 下载 + sha256 校验
```

`main.rs` 只做薄壳。测试走 crate 模块，不依赖 GUI 进程。

## 镜像

选框半开区间 `[x0, x1) × [y0, y1)`，镜像方式与保留侧分开保存。

- 水平翻转处理左右：对称轴是框中心竖线；保留左半时 `x > 轴` 的像素取 `src(x0+x1-1-x, y)`，保留右半时反向。
- 垂直翻转处理上下：对称轴是框中心横线；保留上半时 `y > 轴` 的像素取 `src(x, y0+y1-1-y)`，保留下半时反向。
- 奇数宽/高：对应中心列/中心行不变。
- 框外：不动。

多个框：从原图分别镜像再写入输出，后写覆盖重叠。某框勾了「查看原图」则该区域拷回原图像素。

自动保留侧不比较接缝（镜像后轴两侧必然同源，违和度恒零），而是沿当前轴比较两半的梯度能量，隔行或隔列采样；打平取第一侧（左右方式取左，上下方式取上）。

编辑器用稳定框 ID 保存多选集合，焦点框作为主选框。Ctrl+点击切换集合，Ctrl+A 全选，普通点击单选；多选拖动按相同位移移动全部框，缩放手柄只属于主选框。每框独立保存 `MirrorAxis`、`KeepSide`、`show_original` 和 `show_badge`。多选属性值不一致时，工具栏不显示选中态，用户明确点击后才批量写入。角标关闭后，画布仍用独立勾选标记标出次级选中框。

编辑器历史用最多 100 个 `EditorSnapshot` 保存框、文字、焦点和选择集合。拖动、加框、删框和批量属性在手势结束时提交为一个条目；Ctrl+点击、Ctrl+A、Esc 等选择变化单独入栈。撤销/重做按时间顺序交错处理选择和文档编辑；只恢复选择时保留合成纹理与 dirty 状态。

## 叠加文字

`core::text` 用系统 CJK 字体（`msyh.ttc` → `simhei.ttf` → `simsun.ttc`）把字符串画到 RGBA。中心点 `(cx, cy)`，8 方向描边后再填内部。`core::crop` 把检测结果变成选框，与 GUI 无关。

编辑器合成顺序：克隆源图 → 逐条 `draw`（正在输入的那条跳过）→ 再对每个选框按其轴向做 `mirror`。因此框内文字与照片一起水平或垂直翻转，而不是画在镜像结果上面。复制/保存在仅一个框且勾了「仅选框」时再裁到框内。

## 人脸框

双眼中点为轴心，左右对称扩张覆盖 bbox，左右各外扩约 10%，顶部多扩。越界钳制到图像范围。

无人脸时的中央框：宽、高各为画面的 40%，居中。已有框是否「占用」一张脸：脸中心落在某框内，或与脸 bbox 的 IoU > 0.3。

## SCRFD

InsightFace 1.0 相比 0.7 没有换检测架构，仍是 SCRFD。Python 包默认模型包是 **buffalo_l**，其中检测器为 **SCRFD-10GF**（`det_10g.onnx`，5 关键点）。独立发布的 `scrfd_2.5g_bnkps.onnx` 是同一家族的小模型，不在 buffalo 包里，Hugging Face `deepghs/insightface` 也不提供。

本仓库构建时从该镜像拉取 `buffalo_l/det_10g.onnx`。I/O 与 2.5G bnkps 同族：letterbox 到 640×640 时三个 stride 的行数为 12800 / 3200 / 800，每行 score 1、bbox 4、kps 10。buffalo 导出是动态空间维、输出名为数字节点；解码按列数和行数归类，不依赖 `score_8` 这类名字。

- letterbox 到 640×640，114 灰边，`(x-127.5)/128`，RGB，NCHW
- stride `{8,16,32}` × 2 anchors
- ONNX 分数已是 sigmoid 概率，不用再套一层；阈值 0.5，NMS IoU 0.4
- `detect()` 按分数降序；单人模式再取面积最大者

打开图片且默认模式为人脸时同步检测一次，结果缓存在编辑器里。之后「人脸框选」复用缓存，整图框选再切回来不必再跑；换图会丢掉缓存。检测失败不写入缓存，方便重试。

## 粘贴

格式链：注册格式 `"PNG"` → `CF_DIB`（系统可从 BITMAP / DIBV5 合成）→ `CF_HDROP` 图片文件 → HTML `file://` / 文本里的本地图片路径。

`OpenClipboard` 失败重试 5 次、间隔 5ms（rdpclip 争用）。单次数据上限 256MiB。

egui-winit 在剪贴板无文本时会吞掉 Ctrl+V 按下。粘贴命令由 `Event::Paste` 与被吞按键的松开合成，不轮询 `GetAsyncKeyState`。

## UI

WinUI 3 浅/深双色板。选项类控件：未选中灰边，选中或悬停强调色边；三态复选框的横线与勾都使用强调色底，禁用时按 WinUI disabled token 给复选框叠中性灰层、将文字变灰，并以 167ms 过渡，保留值不变。人脸/整图/加框、角标与选择标记带 250ms ease-out。主页 ↔ 编辑器淡入+水平滑入。Toast 底部独立定位，淡出时收掉占位高度。

界面中文字体启动时按序尝试 `msyh.ttc` / `simhei.ttf` / `simsun.ttc`，与叠加文字同一套候选；工具栏的 ↶ / ↷ 再回退到系统符号字体。
