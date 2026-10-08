//! 渲染层：把解析结果变成目标文本格式。
//!
//! 渲染层与解析层分离，是本工具「可扩展」的关键：解析只做一次
//! （文档 → 中间结果），渲染可以有多套实现，各自实现
//! [`DocumentRenderer`] 即可接入，分派逻辑完全不变。
//!
//! # 三个实现
//!
//! | 实现 | 目标格式 | 输入要求 |
//! |------|----------|----------|
//! | [`markdown::MarkdownRenderer`] | Markdown | 任意输入 |
//! | [`grid::CsvRenderer`] | CSV | 仅电子表格（xls/xlsx） |
//! | [`PlainTextRenderer`] | 纯文本 | 任意输入 |
//!
//! 输入统一抽象为 [`Source`]：Office 文档解析出结构后交给渲染器，
//! 已是纯文本的输入（PDF）直接透传。
//!
//! 无表格结构的输入请求 CSV、或 PDF 请求 Markdown，由各渲染器返回
//! [`ConvertError::IncompatibleOutput`]，而不是悄悄降级成别的格式——
//! 静默降级会让调用方以为拿到了想要的数据。

pub mod grid;
pub mod markdown;

use crate::error::ConvertError;
use crate::output::{ConvertOptions, OutputFormat};

/// 渲染器的输入来源。
pub enum Source<'a> {
    /// 已打开的 Office 文档，可取 IR、原生网格与纯文本。
    Office(&'a office_oxide::Document),
    /// 已经是纯文本的内容（例如 PDF 抽取结果）。
    Plain(&'a str),
}

impl Source<'_> {
    /// 若来源是 Office 文档则返回它，否则返回 `None`。
    pub fn as_office(&self) -> Option<&office_oxide::Document> {
        match self {
            Self::Office(doc) => Some(*doc),
            Self::Plain(_) => None,
        }
    }
}

/// 目标文本格式的渲染器。
///
/// 实现方需保证：同一输入多次渲染结果稳定；渲染过程中不得向标准输出
/// 写入任何内容（日志走 `log`，由调用方决定落点）。
pub trait DocumentRenderer {
    /// 本渲染器产出的格式。
    fn format(&self) -> OutputFormat;

    /// 把输入渲染为目标文本。
    ///
    /// # 参数
    ///
    /// - `source`: 待渲染的来源。
    ///
    /// # 返回
    ///
    /// 成功时为完整的输出文本；失败时为 [`ConvertError`]，包括来源与
    /// 目标格式不兼容、图片导出失败等情形。
    fn render(&self, source: &Source<'_>) -> Result<String, ConvertError>;
}

/// 按目标格式构造对应渲染器。
///
/// 这是解析层与渲染层之间唯一的耦合点：新增格式只需在此增加一个分支。
///
/// # 参数
///
/// - `format`: 目标输出格式。
/// - `options`: 转换选项，当前用于传递图片策略。
///
/// # 返回
///
/// 可直接调用 [`DocumentRenderer::render`] 的渲染器实例。
pub fn renderer_for(format: OutputFormat, options: &ConvertOptions) -> Box<dyn DocumentRenderer> {
    match format {
        OutputFormat::Markdown => Box::new(markdown::MarkdownRenderer::new(options.image_policy())),
        OutputFormat::Csv => Box::new(grid::CsvRenderer::new()),
        OutputFormat::Text => Box::new(PlainTextRenderer),
    }
}

/// 纯文本渲染器：丢弃全部结构，只保留文字。
///
/// 对 Office 文档调用底层库的 `plain_text()`；对本来就是文本的来源
/// 原样返回，避免无谓的重写改变内容。
#[derive(Debug, Clone, Copy, Default)]
pub struct PlainTextRenderer;

impl DocumentRenderer for PlainTextRenderer {
    fn format(&self) -> OutputFormat {
        OutputFormat::Text
    }

    fn render(&self, source: &Source<'_>) -> Result<String, ConvertError> {
        Ok(match source {
            Source::Office(doc) => doc.plain_text(),
            Source::Plain(text) => (*text).to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::ImagePolicy;

    #[test]
    fn renderer_selection_matches_format() {
        let options = ConvertOptions::default();
        assert_eq!(
            renderer_for(OutputFormat::Markdown, &options).format(),
            OutputFormat::Markdown
        );
        assert_eq!(
            renderer_for(OutputFormat::Csv, &options).format(),
            OutputFormat::Csv
        );
        assert_eq!(
            renderer_for(OutputFormat::Text, &options).format(),
            OutputFormat::Text
        );
    }

    #[test]
    fn plain_source_passes_through_text_renderer() {
        let renderer = PlainTextRenderer;
        let out = renderer
            .render(&Source::Plain("内容"))
            .expect("纯文本渲染不应失败");
        assert_eq!(out, "内容");
    }

    #[test]
    fn image_policy_defaults_to_virtual() {
        let options = ConvertOptions::new();
        assert_eq!(options.image_policy(), ImagePolicy::Virtual);
    }
}
