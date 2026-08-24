//! 首次构建时从 Hugging Face 拉取 InsightFace buffalo_l 的 SCRFD-10GF。
//! 权重不进 git；本地已有且 sha256 匹配则跳过下载。

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MODEL_NAME: &str = "det_10g.onnx";
const MODEL_URL: &str =
    "https://huggingface.co/deepghs/insightface/resolve/main/buffalo_l/det_10g.onnx";
const MODEL_SHA256: &str = "5838f7fe053675b1c7a08b633df49e7af5495cee0493c7dcf6697200b85b5b91";
const MODEL_SIZE: u64 = 16_923_827;

fn main() {
    let dest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("assets")
        .join(MODEL_NAME);
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", dest.display());

    if dest.is_file() && verify(&dest) {
        return;
    }
    if dest.exists() {
        let _ = fs::remove_file(&dest);
    }
    fs::create_dir_all(dest.parent().unwrap()).expect("创建 assets 目录失败");
    eprintln!("正在从 Hugging Face 下载 {MODEL_NAME}（约 16 MiB）…");
    if let Err(e) = download(&dest) {
        panic!(
            "下载人脸模型失败: {e}\n请手动下载:\n  {MODEL_URL}\n保存为:\n  {}\n并确认 sha256 为 {MODEL_SHA256}",
            dest.display()
        );
    }
    if !verify(&dest) {
        panic!(
            "下载的模型校验和不匹配，期望 sha256:{MODEL_SHA256} size:{MODEL_SIZE}。文件: {}",
            dest.display()
        );
    }
}

fn sha256_hex(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    format!("{:x}", hasher.finalize())
}

fn verify(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    meta.len() == MODEL_SIZE && sha256_hex(path) == MODEL_SHA256
}

fn download(dest: &Path) -> Result<(), String> {
    let tmp = dest.with_extension("onnx.part");
    let _ = fs::remove_file(&tmp);
    if curl_ok(&tmp)? || powershell_ok(&tmp)? {
        fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
        return Ok(());
    }
    let _ = fs::remove_file(&tmp);
    Err("curl 与 PowerShell 均未能下载".into())
}

fn curl_ok(tmp: &Path) -> Result<bool, String> {
    let status = match Command::new("curl")
        .args([
            "-L",
            "--fail",
            "--retry",
            "3",
            "--retry-delay",
            "2",
            "-A",
            "qp-meme-gen-build",
            "-o",
        ])
        .arg(tmp)
        .arg(MODEL_URL)
        .status()
    {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };
    Ok(status.success() && tmp.is_file())
}

fn powershell_ok(tmp: &Path) -> Result<bool, String> {
    let dest = tmp.to_string_lossy().replace('\'', "''");
    let status = match Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "Invoke-WebRequest -Uri '{MODEL_URL}' -OutFile '{dest}' -UserAgent 'qp-meme-gen-build' -UseBasicParsing"
            ),
        ])
        .status()
    {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };
    Ok(status.success() && tmp.is_file())
}
