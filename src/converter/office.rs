//! Office 文档（DOC/DOCX/XLS/XLSX/PPT/PPTX）转结构化文本。
//!
//! 六种格式统一走 [`office_oxide::Document`]：该库会结合扩展名与文件头签名
//! 识别真实格式，因此扩展名写错（例如把 XLS 存成 .doc）也能正确解析；
//! 同时对加密的 OOXML 包给出明确的“不支持解密”错误，而不是底层的压缩包错误。
//!
//! 打开文档后按目标格式选择渲染器，解析与渲染各自独立：新增输出格式时
//! 这里不需要改动。

use std::path::Path;

use crate::error::ConvertError;
use crate::output::{ConvertOptions, OutputFormat};
use crate::render::{self, Source};

/// 把 Office 文档转换为目标格式的文本。
///
/// # 参数
///
/// - `path`: Office 文档路径。
/// - `format`: 目标输出格式。
/// - `options`: 转换选项（当前用于传递图片策略）。
///
/// # 返回
///
/// 成功时返回完整输出文本；失败时返回 [`ConvertError`]，包括解析失败与
/// 格式不兼容（如对 docx 请求 CSV）。
///
/// # 注意
///
/// 格式与内容的兼容性由渲染器判定，本函数不做预检——预检需要重复一套
/// 「哪种输入支持哪种输出」的知识，容易与渲染器脱节。
pub fn extract(
    path: &Path,
    format: OutputFormat,
    options: &ConvertOptions,
) -> Result<String, ConvertError> {
    let document = office_oxide::Document::open(path)?;
    let renderer = render::renderer_for(format, options);
    renderer.render(&Source::Office(&document))
}
