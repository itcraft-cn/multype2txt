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

    /// 请求的输出格式与输入不兼容（例如对 PDF 要 CSV、对文档要 CSV）。
    ///
    /// 刻意选择报错而非「尽力降级」：降级会让调用方以为拿到了目标格式的
    /// 数据，实际却是别的东西，属于最危险的静默失败。
    #[error("无法以 {format} 格式输出: {detail}")]
    IncompatibleOutput {
        /// 被拒绝的输出格式名。
        format: String,
        /// 不兼容的具体原因。
        detail: String,
    },

    /// 工作表下标越界（底层文档结构与预期不符）。
    #[error("工作表下标越界: index={index}, 共 {count} 张")]
    SheetOutOfRange {
        /// 请求的下标。
        index: usize,
        /// 实际工作表数量。
        count: usize,
    },

    /// 图片字节导出失败（`--images-dir` 指定后无法写入）。
    #[error("导出图片到 {path} 失败: {message}")]
    ImageExport {
        /// 目标路径（目录或具体文件）。
        path: String,
        /// 底层 I/O 错误描述。
        message: String,
    },
}
