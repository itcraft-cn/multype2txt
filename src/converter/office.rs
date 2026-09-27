//! Office 文档（DOC/DOCX/XLS/XLSX/PPT/PPTX）转纯文本。
//!
//! 六种格式统一走 [`office_oxide::Document`]：该库会结合扩展名与文件头签名
//! 识别真实格式，因此扩展名写错（例如把 XLS 存成 .doc）也能正确解析；
//! 同时对加密的 OOXML 包给出明确的“不支持解密”错误，而不是底层的压缩包错误。

use std::path::Path;

use crate::error::ConvertError;

/// 提取 Office 文档的纯文本内容。
///
/// # 参数
///
/// - `path`: Office 文档路径。
///
/// # 返回
///
/// 成功时返回文档的纯文本表示；失败时返回 [`ConvertError::Office`]。
pub fn extract(path: &Path) -> Result<String, ConvertError> {
    let document = office_oxide::Document::open(path)?;
    Ok(document.plain_text())
}
