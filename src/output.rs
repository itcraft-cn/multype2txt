//! 输出格式与转换选项。
//!
//! 本模块只描述「要产出什么」，不涉及「怎么解析」，因此同时被命令行层与
//! 库层引用：CLI 负责把用户输入翻译成 [`OutputFormat`]，解析层据此选择
//! 渲染器。二者解耦后，新增输出格式只需在 [`OutputFormat`] 增加变体并
//! 实现对应渲染器，分派逻辑无需改动。
//!
//! # 格式与输入的对应关系
//!
//! | 输入 | 默认输出 | 理由 |
//! |------|----------|------|
//! | doc / docx | Markdown | OOXML/BIFF 中的标题、列表、表格是声明式结构，还原即可 |
//! | xls / xlsx | CSV | 表格是二维网格，CSV 是无损且最通用的文本承载 |
//! | ppt / pptx | Markdown | 幻灯片的页、标题、列表层级本身就是文档结构 |
//! | pdf | 纯文本 | PDF 没有结构语义，只能抽取文字流 |
//!
//! 略读、采样、摘要都不属于本工具的职责：那是阅读方（LLM）的任务，
//! 这里只负责把内容**完整**地交出去。

use std::path::PathBuf;

/// 目标输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// 结构化 Markdown：标题、列表、表格、超链接保留为 Markdown 语法。
    Markdown,
    /// 逗号分隔值，仅电子表格可用；多工作表以注释行分隔。
    Csv,
    /// 纯文本：丢弃全部结构，仅保留文字本身。
    Text,
}

impl OutputFormat {
    /// 解析命令行给出的格式名。
    ///
    /// 接受 `auto`（返回 `None`，表示按输入类型决定）、`markdown`/`md`、
    /// `csv`、`text`/`txt`。不区分大小写。
    ///
    /// # 返回
    ///
    /// 合法格式名对应 `Some(…)`；`auto` 对应 `None`；其余返回 `Err`，
    /// 错误信息中列出全部合法取值。
    pub fn parse(name: &str) -> Result<Option<Self>, String> {
        match name.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(None),
            "markdown" | "md" => Ok(Some(Self::Markdown)),
            "csv" => Ok(Some(Self::Csv)),
            "text" | "txt" => Ok(Some(Self::Text)),
            other => Err(format!(
                "未知输出格式 `{other}`，可选: auto, markdown, csv, text"
            )),
        }
    }

    /// 命令行回显用的名称，与 [`OutputFormat::parse`] 的取值一致。
    pub fn name(&self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Csv => "csv",
            Self::Text => "text",
        }
    }
}

/// 给定输入扩展名时的默认输出格式。
///
/// # 参数
///
/// - `ext`: 小写、不带点的扩展名。
///
/// # 返回
///
/// 已支持扩展名的默认格式；未知扩展名返回 `None`，由调用方给出
/// 「不支持该格式」的错误（而不是猜一个输出格式）。
pub fn default_format(ext: &str) -> Option<OutputFormat> {
    match ext {
        "doc" | "docx" | "ppt" | "pptx" => Some(OutputFormat::Markdown),
        "xls" | "xlsx" => Some(OutputFormat::Csv),
        "pdf" => Some(OutputFormat::Text),
        _ => None,
    }
}

/// 图片在结构化输出中的处理策略。
///
/// 本工具**不解析图片内容**，这是显式的能力边界：占位符只负责告诉阅读方
/// 「这里有一张图、它叫什么」，是否要看图由阅读方自己决定。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ImagePolicy {
    /// 只留占位符，URL 使用虚拟形式 `image://N`（不落盘）。
    #[default]
    Virtual,
    /// 把图片字节另存到指定目录，占位符 URL 指向实际文件。
    ///
    /// 阅读方若具备读图能力，可自行打开该文件；否则仍只是一行占位符。
    Export {
        /// 图片文件的写入目录（不存在时自动创建）。
        dir: PathBuf,
        /// 写入占位符 URL 的前缀，通常是 `dir` 的路径字符串。
        ///
        /// 由调用方给定而非在此推导：相对路径、`file://` 前缀等由使用场景
        /// 决定，工具不替调用方猜测。
        url_prefix: String,
    },
    /// 完全不输出图片占位符（用于只要文字的场景）。
    Suppress,
}

/// 一次转换的全部可选项。
#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    /// 目标输出格式；`None` 表示按输入扩展名选择默认格式。
    pub format: Option<OutputFormat>,
    /// 图片处理策略；`None` 等价于 [`ImagePolicy::Virtual`]。
    pub images: Option<ImagePolicy>,
}

impl ConvertOptions {
    /// 构造使用全部默认行为的选项（等价于 `Default`）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 指定目标输出格式。
    pub fn with_format(mut self, format: OutputFormat) -> Self {
        self.format = Some(format);
        self
    }

    /// 指定图片处理策略。
    pub fn with_images(mut self, policy: ImagePolicy) -> Self {
        self.images = Some(policy);
        self
    }

    /// 取出图片策略，未指定时使用 [`ImagePolicy::Virtual`]。
    pub fn image_policy(&self) -> ImagePolicy {
        self.images.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_format_aliases() {
        assert_eq!(OutputFormat::parse("auto"), Ok(None));
        assert_eq!(OutputFormat::parse("MD"), Ok(Some(OutputFormat::Markdown)));
        assert_eq!(OutputFormat::parse("text"), Ok(Some(OutputFormat::Text)));
        assert!(OutputFormat::parse("pdf").is_err());
    }

    #[test]
    fn default_format_covers_supported_inputs() {
        assert_eq!(default_format("docx"), Some(OutputFormat::Markdown));
        assert_eq!(default_format("ppt"), Some(OutputFormat::Markdown));
        assert_eq!(default_format("xlsx"), Some(OutputFormat::Csv));
        assert_eq!(default_format("pdf"), Some(OutputFormat::Text));
        assert_eq!(default_format("txt"), None);
    }
}
