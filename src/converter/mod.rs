//! 文档格式分派层：根据扩展名选择合适的解析器，并按目标格式渲染。
//!
//! 支持的格式分为两组：
//!
//! - Office 六件套：`doc`/`docx`/`xls`/`xlsx`/`ppt`/`pptx`，由 [`office`] 处理；
//! - PDF：`pdf`，由 [`pdf`] 处理。
//!
//! 扩展名判断不区分大小写，因此 `.PDF`、`DocX` 同样可用。
//!
//! # 输出格式
//!
//! 默认按输入类型选目标格式（见 [`default_format`]），调用方可通过
//! [`ConvertOptions::format`] 覆盖。请求了输入无法提供的格式时返回
//! [`ConvertError::IncompatibleOutput`]，绝不静默降级成别的格式。

pub mod office;
pub mod pdf;

use std::path::Path;

use crate::error::ConvertError;
use crate::output::{default_format, ConvertOptions};

/// 当前支持的文档扩展名（均为小写、不带点）。
pub const SUPPORTED_EXTENSIONS: &[&str] = &["doc", "docx", "ppt", "pptx", "xls", "xlsx", "pdf"];

/// 用默认选项将单个文档转换为文本。
///
/// 等价于 `convert_with(path, &ConvertOptions::default())`，保留给只关心
/// 「拿到文本」的调用方。
///
/// # 参数
///
/// - `path`: 待转换文档的路径。
///
/// # 返回
///
/// 成功时返回输出文本；失败时返回 [`ConvertError`]，覆盖扩展名缺失、
/// 格式不支持、底层解析失败等情形。
pub fn convert_file(path: &Path) -> Result<String, ConvertError> {
    convert_with(path, &ConvertOptions::default())
}

/// 按给定选项将单个文档转换为文本。
///
/// # 参数
///
/// - `path`: 待转换文档的路径。
/// - `options`: 输出格式与图片策略；`options.format` 为 `None` 时按扩展名
///   选择默认格式。
///
/// # 返回
///
/// 成功时返回输出文本；失败时返回 [`ConvertError`]。
pub fn convert_with(path: &Path, options: &ConvertOptions) -> Result<String, ConvertError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .ok_or_else(|| ConvertError::MissingExtension(path.to_path_buf()))?;

    let format = match options.format {
        Some(format) => format,
        None => default_format(&ext).ok_or_else(|| ConvertError::UnsupportedFormat {
            ext: ext.clone(),
            supported: SUPPORTED_EXTENSIONS.join(", "),
        })?,
    };

    match ext.as_str() {
        "doc" | "docx" | "ppt" | "pptx" | "xls" | "xlsx" => office::extract(path, format, options),
        "pdf" => {
            // PDF 只有文字流，没有任何结构可还原；请求其他格式时报错，
            // 避免产出一份看似「结构化」实则凭空编造的文档。
            if !matches!(format, crate::output::OutputFormat::Text) {
                return Err(ConvertError::IncompatibleOutput {
                    format: format.name().to_string(),
                    detail: "PDF 只能提取纯文本".to_string(),
                });
            }
            pdf::extract(path)
        }
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
