//! 文档格式分派层：根据扩展名选择合适的解析器。
//!
//! 支持的格式分为两组：
//!
//! - Office 六件套：`doc`/`docx`/`xls`/`xlsx`/`ppt`/`pptx`，由 [`office`] 处理；
//! - PDF：`pdf`，由 [`pdf`] 处理。
//!
//! 扩展名判断不区分大小写，因此 `.PDF`、`.DocX` 同样可用。

pub mod office;
pub mod pdf;

use std::path::Path;

use crate::error::ConvertError;

/// 当前支持的文档扩展名（均为小写、不带点）。
pub const SUPPORTED_EXTENSIONS: &[&str] = &["doc", "docx", "ppt", "pptx", "xls", "xlsx", "pdf"];

/// 将单个文档转换为纯文本。
///
/// # 参数
///
/// - `path`: 待转换文档的路径。
///
/// # 返回
///
/// 成功时返回纯文本内容；失败时返回 [`ConvertError`]，覆盖扩展名缺失、
/// 格式不支持、底层解析失败等情形。
pub fn convert_file(path: &Path) -> Result<String, ConvertError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .ok_or_else(|| ConvertError::MissingExtension(path.to_path_buf()))?;

    match ext.as_str() {
        "doc" | "docx" | "ppt" | "pptx" | "xls" | "xlsx" => office::extract(path),
        "pdf" => pdf::extract(path),
        other => Err(ConvertError::UnsupportedFormat {
            ext: other.to_string(),
            supported: SUPPORTED_EXTENSIONS.join(", "),
        }),
    }
}

/// 判断给定路径的扩展名是否受支持。
///
/// 与 [`convert_file`] 使用同一套小写扩展名规则，便于 CLI 在真正解析前
/// 给出友好提示。
pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.as_str()))
        .unwrap_or(false)
}
