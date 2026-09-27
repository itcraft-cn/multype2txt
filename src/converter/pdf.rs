//! PDF 文档转纯文本。
//!
//! 使用 [`pdf_oxide::PdfDocument::extract_all_text`] 一次性抽取全部页面。
//! 该接口会自动读取页面阅读顺序，并在页与页之间插入换页符 (`U+000C`)，
//! 既保留分页边界，又不会向正文混入可见的标记文本。

use std::path::Path;

use crate::error::ConvertError;

/// 提取 PDF 文档的纯文本内容。
///
/// # 参数
///
/// - `path`: PDF 文件路径。
///
/// # 返回
///
/// 成功时返回全文文本（页间以换页符分隔）；失败时返回 [`ConvertError::Pdf`]。
pub fn extract(path: &Path) -> Result<String, ConvertError> {
    let document = pdf_oxide::PdfDocument::open(path)?;
    Ok(document.extract_all_text()?)
}
