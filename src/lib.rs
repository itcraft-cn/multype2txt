//! multype2txt 库入口。
//!
//! 本库负责把 Office（DOC/DOCX/XLS/XLSX/PPT/PPTX）与 PDF 文档转换为可
//! 供程序或模型阅读的文本，是命令行程序 `multype2txt` 的可复用内核。
//!
//! # 能做什么
//!
//! | 输入 | 默认输出 |
//! |------|----------|
//! | doc / docx | Markdown（标题、列表、表格、超链接保留） |
//! | xls / xlsx | CSV（全量数据，多工作表以注释行分隔） |
//! | ppt / pptx | Markdown（页、标题、列表层级保留） |
//! | pdf | 纯文本 |
//!
//! 图片不解析——占位符只说明「这里有一张图」，需要时可用
//! [`ConvertOptions::images`] 要求把字节另存为文件，是否要看由调用方决定。
//!
//! # 示例
//!
//! ```no_run
//! use std::path::Path;
//!
//! let text = multype2txt::convert_file(Path::new("report.docx"))?;
//! println!("{}", text);
//! # Ok::<(), multype2txt::ConvertError>(())
//! ```
//!
//! 需要指定输出格式或图片策略时使用 [`convert_with`]：
//!
//! ```no_run
//! use std::path::Path;
//! use multype2txt::{ConvertOptions, OutputFormat};
//!
//! let options = ConvertOptions::new().with_format(OutputFormat::Markdown);
//! let text = multype2txt::convert_with(Path::new("report.docx"), &options)?;
//! # Ok::<(), multype2txt::ConvertError>(())
//! ```

pub mod converter;
pub mod error;
pub mod output;
pub mod render;

pub use converter::{convert_file, convert_with, is_supported, SUPPORTED_EXTENSIONS};
pub use error::ConvertError;
pub use output::{ConvertOptions, ImagePolicy, OutputFormat};
