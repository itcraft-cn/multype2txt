//! 图片落盘的公共约定：命名、扩展名识别与占位符 URL 拼接。
//!
//! Markdown 输出（Office 文档按 IR 取图）与 PDF 输出（按页扫描内容流取图）
//! 走的是两条完全不同的解析路径，但对使用方而言它们是同一个工具、同一个
//! `--images-dir`：文件名必须一致（`img-0001.png`），URL 拼法必须一致
//! （不出现双斜杠），否则调用方要为同一参数维护两套规则。
//!
//! 本模块只做「怎么写文件」，不关心「图从哪来」——图片来源由各解析层负责。

use std::path::Path;

use crate::error::ConvertError;

/// 生成第 `sequence` 张图片的文件名，扩展名按魔数识别。
///
/// # 参数
///
/// - `sequence`: 图片序号，从 1 开始，跨页/跨节全局递增。
/// - `bytes`: 图片原始字节，仅用于识别扩展名。
pub(crate) fn image_file_name(sequence: usize, bytes: &[u8]) -> String {
    format!("img-{sequence:04}.{}", sniff_image_extension(bytes))
}

/// 生成第 `sequence` 张 PNG 图片的文件名。
///
/// 用于没有现成编码字节的来源（如 PDF 的未编码像素）：底层库编码成 PNG
/// 后落盘，扩展名必须与实际内容一致，不能靠魔数反推。
pub(crate) fn png_file_name(sequence: usize) -> String {
    format!("img-{sequence:04}.png")
}

/// 确保导出目录存在（不存在时创建）。
///
/// # 参数
///
/// - `dir`: 目标目录。
///
/// # 返回
///
/// 成功返回 `()`；失败返回 [`ConvertError::ImageExport`]。
pub(crate) fn ensure_dir(dir: &Path) -> Result<(), ConvertError> {
    std::fs::create_dir_all(dir).map_err(|error| ConvertError::ImageExport {
        path: dir.display().to_string(),
        message: error.to_string(),
    })
}

/// 把图片字节写入 `dir`，返回生成的文件名。
///
/// # 参数
///
/// - `dir`: 目标目录，不存在时自动创建。
/// - `sequence`: 图片序号，从 1 开始。
/// - `bytes`: 图片原始字节。
///
/// # 返回
///
/// 成功时返回形如 `img-0001.png` 的文件名；失败返回
/// [`ConvertError::ImageExport`]，绝不悄悄退回虚拟 URL——静默降级会让
/// 调用方以为图已导出。
pub(crate) fn write_image_bytes(
    dir: &Path,
    sequence: usize,
    bytes: &[u8],
) -> Result<String, ConvertError> {
    ensure_dir(dir)?;

    let file_name = image_file_name(sequence, bytes);
    let target = dir.join(&file_name);
    std::fs::write(&target, bytes).map_err(|error| ConvertError::ImageExport {
        path: target.display().to_string(),
        message: error.to_string(),
    })?;

    Ok(file_name)
}

/// 拼接占位符 URL 前缀与文件名，避免出现双斜杠。
///
/// # 参数
///
/// - `prefix`: URL 前缀，来自 `--images-dir` 的原始输入（相对、绝对或带
///   scheme 均可），可为空。
/// - `file_name`: [`image_file_name`] 生成的文件名。
///
/// # 返回
///
/// 形如 `imgs/img-0001.png` 的 URL。
///
/// # 注意
///
/// Windows 上 `--images-dir C:\data\imgs` 会给出反斜杠路径，而 Markdown
/// 链接里的反斜杠是转义字符，URL 会被解析器切碎。这里统一归一为 `/`。
/// 是否补 `file://` 由调用方自己决定——传 `--images-dir file:///…` 即可，
/// 工具不替调用方猜语义。
pub(crate) fn join_url(prefix: &str, file_name: &str) -> String {
    let prefix = prefix.replace('\\', "/");
    if prefix.is_empty() {
        file_name.to_string()
    } else if prefix.ends_with('/') {
        format!("{prefix}{file_name}")
    } else {
        format!("{prefix}/{file_name}")
    }
}

/// 按魔数识别图片扩展名，识别不出时用 `bin`。
///
/// 不依赖底层库的 `ImageFormat` 枚举：Office 文档里的 EMF/WMF/EMF+ 等图元
/// 格式库未必枚举得到，而魔数是文件本身给出的事实——扩展名写错会导致看图
/// 软件拒绝打开，写对则至少能被识别。
///
/// # 参数
///
/// - `bytes`: 图片字节的前若干字节。
///
/// # 返回
///
/// 不含点的扩展名。
pub(crate) fn sniff_image_extension(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        "png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "jpg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "gif"
    } else if bytes.starts_with(b"BM") {
        "bmp"
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "webp"
    } else if is_emf(bytes) {
        "emf"
    } else if bytes.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A]) {
        "wmf"
    } else if contains_svg_marker(bytes) {
        "svg"
    } else {
        "bin"
    }
}

/// EMF 签名 `dSignature` 的偏移（[MS-EMF] `EMR_HEADER`）：iType(4) +
/// nSize(4) + rclBounds(16) + rclFrame(16) = 40。
const EMF_SIGNATURE_OFFSET: usize = 40;

/// 判断字节流是否为 EMF。
///
/// 仅凭前 4 字节 `01 00 00 00`（`iType = EMR_HEADER`）不足以定性：那是
/// 一个极常见的小端整数，任何以此开头的二进制都会被误判成 EMF。因此还
/// 要求偏移 40 处的 `dSignature` 等于 `" EMF"`（`0x464D4520`，小端）——
/// 这是 EMF 自己声明的身份，识别不出时落到 `bin` 兜底也比写错扩展名强。
fn is_emf(bytes: &[u8]) -> bool {
    if bytes.len() <= EMF_SIGNATURE_OFFSET || !bytes.starts_with(&[0x01, 0x00, 0x00, 0x00]) {
        return false;
    }
    let signature = u32::from_le_bytes([
        bytes[EMF_SIGNATURE_OFFSET],
        bytes[EMF_SIGNATURE_OFFSET + 1],
        bytes[EMF_SIGNATURE_OFFSET + 2],
        bytes[EMF_SIGNATURE_OFFSET + 3],
    ]);
    signature == 0x464D_4520
}

/// 粗略判断字节流是否为 SVG（XML 声明或根元素）。
///
/// 扫描窗口取 8 KiB：Illustrator 之类工具导出的 SVG 常在根元素前塞进
/// 长长的 XML 注释与许可证声明，只看文件开头 1 KiB 会漏判。
fn contains_svg_marker(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8 * 1024)];
    let text = String::from_utf8_lossy(head).to_ascii_lowercase();
    text.contains("<svg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_extension_recognizes_common_formats() {
        assert_eq!(
            sniff_image_extension(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            "png"
        );
        assert_eq!(sniff_image_extension(&[0xFF, 0xD8, 0xFF, 0xE0]), "jpg");
        assert_eq!(sniff_image_extension(b"BM0000"), "bmp");
        assert_eq!(sniff_image_extension(b"<svg xmlns=\"x\">"), "svg");
        assert_eq!(sniff_image_extension(&[0x00, 0x01, 0x02]), "bin");
    }

    /// EMF 要同时看 `iType` 与偏移 40 的 `dSignature`：只有前 4 字节的
    /// `01 00 00 00` 是个过于常见的小端整数，单独用它会把大量二进制
    /// 误判成 EMF。
    #[test]
    fn emf_requires_signature_not_just_header() {
        let mut emf = vec![0x01, 0x00, 0x00, 0x00];
        emf.resize(EMF_SIGNATURE_OFFSET + 4, 0);
        emf[EMF_SIGNATURE_OFFSET..EMF_SIGNATURE_OFFSET + 4]
            .copy_from_slice(&0x464D_4520u32.to_le_bytes());
        assert_eq!(sniff_image_extension(&emf), "emf");

        assert_eq!(sniff_image_extension(&[0x01, 0x00, 0x00, 0x00]), "bin");
        let mut too_short = vec![0x01, 0x00, 0x00, 0x00];
        too_short.resize(EMF_SIGNATURE_OFFSET, 0);
        assert_eq!(sniff_image_extension(&too_short), "bin");
    }

    /// SVG 根元素可能被长注释推到 1 KiB 之后，扫描窗口须足够大。
    #[test]
    fn svg_after_long_header_comment_is_recognized() {
        let mut bytes = b"<?xml version=\"1.0\"?>\n<!--".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', 4000));
        bytes.extend_from_slice(b"-->\n<svg xmlns=\"x\"/>");
        assert_eq!(sniff_image_extension(&bytes), "svg");
    }

    #[test]
    fn join_url_avoids_double_slash() {
        assert_eq!(join_url("imgs", "a.png"), "imgs/a.png");
        assert_eq!(join_url("imgs/", "a.png"), "imgs/a.png");
        assert_eq!(join_url("", "a.png"), "a.png");
    }

    /// Windows 路径的反斜杠在 Markdown 链接里是转义字符，必须归一为 `/`，
    /// 否则 URL 会被解析器切碎。
    #[test]
    fn join_url_normalizes_windows_separators() {
        assert_eq!(join_url(r"C:\data\imgs", "a.png"), "C:/data/imgs/a.png");
        assert_eq!(join_url(r"imgs\", "a.png"), "imgs/a.png");
    }

    #[test]
    fn file_name_pads_sequence() {
        assert_eq!(
            image_file_name(1, &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            "img-0001.png"
        );
        assert_eq!(image_file_name(42, &[0xFF, 0xD8, 0xFF]), "img-0042.jpg");
    }

    #[test]
    fn write_bytes_creates_dir_and_file() {
        let dir = std::env::temp_dir().join(format!("m2t-img-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let name = write_image_bytes(&dir, 1, &[0xFF, 0xD8, 0xFF, 0xE0]).expect("写入应成功");
        assert_eq!(name, "img-0001.jpg");
        assert!(dir.join(&name).is_file(), "图片文件应落盘");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
