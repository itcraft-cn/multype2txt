//! multype2txt 库入口。
//!
//! 本库负责把 Office（DOC/DOCX/XLS/XLSX/PPT/PPTX）与 PDF 文档转换为纯文本，
//! 是命令行程序 `multype2txt` 的可复用内核。
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

pub mod converter;
pub mod error;

pub use converter::{convert_file, is_supported, SUPPORTED_EXTENSIONS};
pub use error::ConvertError;
