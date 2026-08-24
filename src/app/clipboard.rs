//! Windows 剪贴板读图，参考 Chromium `ui/base/clipboard/clipboard_win.cc`。
//!
//! 像素：注册格式 `"PNG"` → `CF_DIB`（系统可从 `CF_BITMAP` / `CF_DIBV5` 合成）。
//! 文件：`CF_HDROP`。再回退 HTML 里的本地 `file://` 与文本中的本地图片路径。
//! OpenClipboard 失败时重试（远程桌面 rdpclip 争用，见 Chromium bug 815425）。

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;
use image::codecs::bmp::BmpDecoder;
use image::{DynamicImage, RgbaImage};

/// 剪贴板里拿到的图：已解码像素，或本地文件路径（由调用方 `image::open`）。
pub(crate) enum PastedImage {
    Pixels(RgbaImage),
    File(PathBuf),
}

const MAX_CLIPBOARD_BYTES: usize = 256 * 1024 * 1024;
const OPEN_ATTEMPTS: u32 = 5;
const OPEN_RETRY_MS: u64 = 5;

const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "bmp", "gif", "webp", "tiff", "tif", "ico",
];

pub(crate) fn take_image() -> Result<PastedImage, String> {
    if let Some(img) = read_pixels() {
        return Ok(PastedImage::Pixels(img));
    }
    if let Some(path) = read_hdrop_image() {
        return Ok(PastedImage::File(path));
    }
    if let Some(path) = read_html_file_image() {
        return Ok(PastedImage::File(path));
    }
    if let Some(path) = read_text_image_path() {
        return Ok(PastedImage::File(path));
    }
    Err("剪贴板里没有可用的图片".into())
}

fn read_pixels() -> Option<RgbaImage> {
    let png_id = png_format_id();
    let raw = copy_png_and_dib(png_id)?;
    if let Some(bytes) = raw.png
        && let Some(img) = decode_encoded(&bytes)
    {
        return Some(img);
    }
    if let Some(bytes) = raw.dib
        && let Some(img) = decode_dib(&bytes)
    {
        return Some(img);
    }
    None
}

fn read_hdrop_image() -> Option<PathBuf> {
    let files = arboard::Clipboard::new().ok()?.get().file_list().ok()?;
    files.into_iter().find(|p| is_image_path(p))
}

fn read_html_file_image() -> Option<PathBuf> {
    let html = arboard::Clipboard::new().ok()?.get().html().ok()?;
    let src = img_src_from_html(&html)?;
    path_from_clipboard_text(&src)
}

fn read_text_image_path() -> Option<PathBuf> {
    let text = arboard::Clipboard::new().ok()?.get_text().ok()?;
    path_from_clipboard_text(&text)
}

fn png_format_id() -> u32 {
    const PNG: [u16; 4] = [b'P' as u16, b'N' as u16, b'G' as u16, 0];
    unsafe { windows_sys::Win32::System::DataExchange::RegisterClipboardFormatW(PNG.as_ptr()) }
}

struct PixelFormats {
    png: Option<Vec<u8>>,
    dib: Option<Vec<u8>>,
}

/// 一次打开剪贴板，拷出 PNG 与 DIB 后立刻关掉，避免解码时占锁。
fn copy_png_and_dib(png_id: u32) -> Option<PixelFormats> {
    use windows_sys::Win32::System::DataExchange::CloseClipboard;
    use windows_sys::Win32::System::Ole::{CF_DIB, CF_DIBV5};

    if !open_clipboard() {
        return None;
    }
    let png = if png_id != 0 {
        clipboard_bytes(png_id)
    } else {
        None
    };
    // Chromium 读的是 CF_DIB（系统会从 BITMAP/DIBV5 合成）；合成失败时再取 DIBV5。
    let dib = clipboard_bytes(CF_DIB as u32).or_else(|| clipboard_bytes(CF_DIBV5 as u32));
    // SAFETY: open_clipboard 成功后剪贴板由本线程持有。
    unsafe { CloseClipboard() };
    Some(PixelFormats { png, dib })
}

fn open_clipboard() -> bool {
    use windows_sys::Win32::System::DataExchange::OpenClipboard;
    for attempt in 0..OPEN_ATTEMPTS {
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
            return true;
        }
        if attempt + 1 < OPEN_ATTEMPTS {
            std::thread::sleep(Duration::from_millis(OPEN_RETRY_MS));
        }
    }
    false
}

fn clipboard_bytes(format: u32) -> Option<Vec<u8>> {
    use windows_sys::Win32::System::DataExchange::{GetClipboardData, IsClipboardFormatAvailable};
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    if unsafe { IsClipboardFormatAvailable(format) } == 0 {
        return None;
    }
    let handle = unsafe { GetClipboardData(format) };
    if handle.is_null() {
        return None;
    }
    let size = unsafe { GlobalSize(handle) };
    if size == 0 || size > MAX_CLIPBOARD_BYTES {
        return None;
    }
    let ptr = unsafe { GlobalLock(handle) };
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, size) }.to_vec();
    unsafe { GlobalUnlock(handle) };
    Some(bytes)
}

fn decode_encoded(bytes: &[u8]) -> Option<RgbaImage> {
    image::load_from_memory(bytes)
        .ok()
        .map(|img| img.to_rgba8())
}

/// `CF_DIB` 是无文件头的 BMP（`BITMAPINFOHEADER` 或 V4/V5）。
fn decode_dib(dib: &[u8]) -> Option<RgbaImage> {
    if let Some(img) = bmp_without_file_header(dib) {
        return Some(img);
    }
    // Chromium / Electron 常写 32-bit BI_RGB 并把 alpha 放在最高字节；
    // BMP 规范说 BI_RGB 的高字节不用，解码器会丢掉透明度。改成 BI_BITFIELDS。
    let mut tweaked = dib.to_vec();
    if tweak_dib_alpha_header(&mut tweaked) {
        bmp_without_file_header(&tweaked)
    } else {
        None
    }
}

fn bmp_without_file_header(dib: &[u8]) -> Option<RgbaImage> {
    let decoder = BmpDecoder::new_without_file_header(Cursor::new(dib)).ok()?;
    DynamicImage::from_decoder(decoder)
        .ok()
        .map(|img| img.into_rgba8())
}

/// 仅在带色掩码的头（≥56 字节）上改压缩方式，40 字节 `BITMAPINFOHEADER` 没有掩码字段。
fn tweak_dib_alpha_header(dib: &mut [u8]) -> bool {
    const BI_RGB: u32 = 0;
    const BI_BITFIELDS: u32 = 3;
    if dib.len() < 56 {
        return false;
    }
    let bit_count = u16::from_le_bytes(dib[14..16].try_into().unwrap());
    let compression = u32::from_le_bytes(dib[16..20].try_into().unwrap());
    if bit_count != 32 || compression != BI_RGB {
        return false;
    }
    dib[16..20].copy_from_slice(&BI_BITFIELDS.to_le_bytes());
    let r = u32::from_le_bytes(dib[40..44].try_into().unwrap());
    let g = u32::from_le_bytes(dib[44..48].try_into().unwrap());
    let b = u32::from_le_bytes(dib[48..52].try_into().unwrap());
    if r == 0 && g == 0 && b == 0 {
        dib[40..44].copy_from_slice(&0x00ff_0000u32.to_le_bytes());
        dib[44..48].copy_from_slice(&0x0000_ff00u32.to_le_bytes());
        dib[48..52].copy_from_slice(&0x0000_00ffu32.to_le_bytes());
    }
    true
}

fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// 文本 / HTML `src` → 本地图片路径。不认 http(s)（运行时离线）。
pub(crate) fn path_from_clipboard_text(text: &str) -> Option<PathBuf> {
    let t = text.trim().trim_matches('"').trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return None;
    }
    let path = if let Some(rest) = strip_file_url(t) {
        PathBuf::from(percent_decode(&rest))
    } else {
        PathBuf::from(t)
    };
    is_image_path(&path).then_some(path)
}

fn strip_file_url(s: &str) -> Option<String> {
    let rest = s
        .strip_prefix("file://")
        .or_else(|| s.strip_prefix("FILE://"))?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    // file:///C:/foo → /C:/foo，Windows 上去掉多余的斜杠。
    if rest.len() >= 3 {
        let bytes = rest.as_bytes();
        if bytes[0] == b'/' && bytes.get(2) == Some(&b':') {
            return Some(rest[1..].replace('/', "\\"));
        }
    }
    Some(rest.replace('/', "\\"))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 从 CF_HTML / 普通 HTML 里取第一个 `src=`。
pub(crate) fn img_src_from_html(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find("src=") {
        let i = from + rel + 4;
        let rest = html.get(i..)?;
        let quote = rest.as_bytes().first().copied()?;
        if quote != b'"' && quote != b'\'' {
            from = i;
            continue;
        }
        let inner = &rest[1..];
        let end = inner.as_bytes().iter().position(|&b| b == quote)?;
        return Some(inner[..end].to_string());
    }
    None
}

/// egui-winit 在 Ctrl+V 时：有文本 → `Event::Paste` 并吞掉 Key 按下；
/// 无文本 → 按下事件全无，只留下 Key 松开。把这两种收成一次粘贴命令。
pub(crate) struct PasteCommand {
    fired: bool,
    saw_v_press: bool,
    saw_insert_press: bool,
    saw_paste_press: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum PasteKey {
    V,
    Insert,
    Paste,
}

#[derive(Clone, Copy)]
pub(crate) enum PasteSignal {
    ClipboardText,
    Key {
        key: PasteKey,
        pressed: bool,
        command: bool,
        shift: bool,
    },
}

impl PasteCommand {
    pub(crate) fn new() -> Self {
        Self {
            fired: false,
            saw_v_press: false,
            saw_insert_press: false,
            saw_paste_press: false,
        }
    }

    pub(crate) fn poll(&mut self, ctx: &egui::Context) -> bool {
        let signals: Vec<PasteSignal> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Paste(_) => Some(PasteSignal::ClipboardText),
                    egui::Event::Key {
                        key,
                        pressed,
                        modifiers,
                        ..
                    } => {
                        let key = match key {
                            egui::Key::V => PasteKey::V,
                            egui::Key::Insert => PasteKey::Insert,
                            egui::Key::Paste => PasteKey::Paste,
                            _ => return None,
                        };
                        Some(PasteSignal::Key {
                            key,
                            pressed: *pressed,
                            command: modifiers.command,
                            shift: modifiers.shift,
                        })
                    }
                    _ => None,
                })
                .collect()
        });
        self.feed(&signals)
    }

    pub(crate) fn feed(&mut self, signals: &[PasteSignal]) -> bool {
        let mut want = false;
        for s in signals {
            match *s {
                PasteSignal::ClipboardText => {
                    if !self.fired {
                        want = true;
                        self.fired = true;
                    }
                }
                PasteSignal::Key {
                    key: PasteKey::Paste,
                    pressed: true,
                    ..
                } => {
                    self.saw_paste_press = true;
                    if !self.fired {
                        want = true;
                        self.fired = true;
                    }
                }
                PasteSignal::Key {
                    key: PasteKey::Paste,
                    pressed: false,
                    ..
                } => {
                    if !self.fired && !self.saw_paste_press {
                        want = true;
                    }
                    self.fired = false;
                    self.saw_paste_press = false;
                }
                PasteSignal::Key {
                    key: PasteKey::V,
                    pressed: true,
                    command,
                    ..
                } => {
                    self.saw_v_press = true;
                    if command && !self.fired {
                        want = true;
                        self.fired = true;
                    }
                }
                PasteSignal::Key {
                    key: PasteKey::V,
                    pressed: false,
                    command,
                    ..
                } => {
                    if command && !self.fired && !self.saw_v_press {
                        want = true;
                    }
                    self.fired = false;
                    self.saw_v_press = false;
                }
                PasteSignal::Key {
                    key: PasteKey::Insert,
                    pressed: true,
                    shift,
                    ..
                } => {
                    self.saw_insert_press = true;
                    if shift && !self.fired {
                        want = true;
                        self.fired = true;
                    }
                }
                PasteSignal::Key {
                    key: PasteKey::Insert,
                    pressed: false,
                    shift,
                    ..
                } => {
                    if shift && !self.fired && !self.saw_insert_press {
                        want = true;
                    }
                    self.fired = false;
                    self.saw_insert_press = false;
                }
            }
        }
        want
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(pressed: bool, command: bool) -> PasteSignal {
        PasteSignal::Key {
            key: PasteKey::V,
            pressed,
            command,
            shift: false,
        }
    }

    fn ins(pressed: bool, shift: bool) -> PasteSignal {
        PasteSignal::Key {
            key: PasteKey::Insert,
            pressed,
            command: false,
            shift,
        }
    }

    #[test]
    fn paste_text_event_fires_once_and_release_does_not_repeat() {
        let mut d = PasteCommand::new();
        assert!(d.feed(&[PasteSignal::ClipboardText]));
        assert!(!d.feed(&[v(false, true)]));
    }

    #[test]
    fn image_only_clipboard_fires_on_swallowed_v_release() {
        let mut d = PasteCommand::new();
        // egui-winit 吞掉 Ctrl+V 按下，本帧只有松开
        assert!(d.feed(&[v(false, true)]));
        assert!(!d.feed(&[]));
        // 下一次完整的 Ctrl+V 再触发
        assert!(d.feed(&[v(false, true)]));
    }

    #[test]
    fn v_without_ctrl_then_ctrl_on_release_is_not_paste() {
        let mut d = PasteCommand::new();
        assert!(!d.feed(&[v(true, false)]));
        assert!(!d.feed(&[v(false, true)]));
    }

    #[test]
    fn shift_insert_swallowed_press() {
        let mut d = PasteCommand::new();
        assert!(d.feed(&[ins(false, true)]));
        assert!(!d.feed(&[]));
    }

    #[test]
    fn ctrl_v_press_if_egui_stops_swallowing() {
        let mut d = PasteCommand::new();
        assert!(d.feed(&[v(true, true)]));
        assert!(!d.feed(&[v(false, true)]));
    }

    #[test]
    fn path_from_quoted_windows_path() {
        let p = path_from_clipboard_text(r#" "D:\pics\a.PNG" "#).unwrap();
        assert_eq!(p, PathBuf::from(r"D:\pics\a.PNG"));
    }

    #[test]
    fn path_from_file_url() {
        let p = path_from_clipboard_text("file:///C:/Users/a/b.jpg").unwrap();
        assert_eq!(p, PathBuf::from(r"C:\Users\a\b.jpg"));
    }

    #[test]
    fn path_rejects_http() {
        assert!(path_from_clipboard_text("https://example.com/a.png").is_none());
    }

    #[test]
    fn path_rejects_non_image() {
        assert!(path_from_clipboard_text(r"C:\notes.txt").is_none());
    }

    #[test]
    fn html_src_file_url() {
        let html = r#"Version:0.9
StartHTML:00000000
<html><body><!--StartFragment--><img src="file:///D:/x/y.webp"><!--EndFragment--></body></html>"#;
        let src = img_src_from_html(html).unwrap();
        assert_eq!(src, "file:///D:/x/y.webp");
        assert_eq!(
            path_from_clipboard_text(&src).unwrap(),
            PathBuf::from(r"D:\x\y.webp")
        );
    }

    #[test]
    fn decode_1x1_24bit_dib() {
        // BITMAPINFOHEADER 40 字节 + 4 字节对齐的 24-bit 像素（BGR + pad）
        let mut dib = vec![0u8; 44];
        dib[0..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1i32.to_le_bytes());
        dib[8..12].copy_from_slice(&1i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24u16.to_le_bytes());
        dib[40] = 10; // B
        dib[41] = 20; // G
        dib[42] = 30; // R
        let img = decode_dib(&dib).expect("dib");
        assert_eq!(img.dimensions(), (1, 1));
        assert_eq!(img.get_pixel(0, 0).0, [30, 20, 10, 255]);
    }

    #[test]
    fn percent_decode_space() {
        let p = path_from_clipboard_text("file:///C:/a%20b.png").unwrap();
        assert_eq!(p, PathBuf::from(r"C:\a b.png"));
    }
}
