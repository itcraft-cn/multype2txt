//! multype2txt 的错误类型定义。
//!
//! 转换过程可能失败的环节分为三类：输入路径无法确定格式、底层解析库报错、
//! 输出文件写入失败。统一收敛到 [`ConvertError`]，由调用方决定如何呈现。

use std::path::PathBuf;

/// 文档转换过程中可能出现的错误。
#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    /// 输入文件没有可识别的扩展名，无法据此选择解析器。
    #[error("无法确定文件扩展名: {0}")]
    MissingExtension(PathBuf),

    /// 输入文件的扩展名不在支持列表内。
    #[error("不支持的文件格式 .{ext}（支持: {supported}）")]
    UnsupportedFormat {
        /// 实际检测到的扩展名（小写）。
        ext: String,
        /// 当前支持的扩展名列表，便于使用者纠正输入。
        supported: String,
    },

    /// Office 文档解析失败，携带底层库的原始错误。
    #[error("Office 文档解析失败: {0}")]
    Office(#[from] office_oxide::OfficeError),

    /// PDF 文档解析失败，携带底层库的原始错误。
    #[error("PDF 解析失败: {0}")]
    Pdf(#[from] pdf_oxide::Error),
}
