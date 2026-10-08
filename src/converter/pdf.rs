//! PDF 文档转纯文本，可选按页导出图片。
//!
//! 使用 [`pdf_oxide::PdfDocument::extract_all_text`] 一次性抽取全部页面。
//! 该接口按 `0..page_count` 逐页读取阅读顺序，并在页与页之间插入换页符
//! (`U+000C`)，既保留分页边界，又不会向正文混入可见的标记文本。
//!
//! # 图片
//!
//! PDF 的文字流里没有图片的落点，因此默认输出不含任何图片痕迹。只有请求
//! [`ImagePolicy::Export`] 时才按页扫描内容流抽图，并在**对应页文本末尾**
//! 追加与 Markdown 输出同款的占位符 `![图片N](<url>)`：PDF 没有标题、列表
//! 结构可挂占位符，页末是唯一既不打断正文、又能保留页归属的位置。
//!
//! 抽图走 [`pdf_oxide::PdfDocument::page_image_handles`]：它同时覆盖内联
//! 图像（`BI...ID...EI`）与 XObject 图。内联图在 PDF 里很常见，用另一条
//! `extract_images` 路径会因缺少 `/Subtype` 条目而被底层静默跳过。
//!
//! 页对齐依赖底层契约：`extract_all_text` 的分段数恒等于 `page_count()`，
//! 因此「按换页符切分的第 N 段」就是「第 N 页」。本模块不自行推算页数，
//! 也不允许错位插入——占位符挂在错误的页上比不插更糟。

use std::path::Path;

use pdf_oxide::extractors::{ImageData, PdfImage};

use crate::error::ConvertError;
use crate::image::{ensure_dir, join_url, png_file_name, write_image_bytes};
use crate::output::{ConvertOptions, ImagePolicy};

/// 换页符 `U+000C`：PDF 纯文本里的页分隔符。
///
/// Rust 字符串字面量没有 `\f` 转义（那是 C 的写法），用常量表达既避免误写，
/// 也让「这里是页边界」这个语义在代码中显式可见。取 `&str` 是因为
/// `str::split` 与 `[String]::join` 都接受它。
const FEED: &str = "\x0C";

/// 提取 PDF 文档的文本内容，按需导出图片。
///
/// # 参数
///
/// - `path`: PDF 文件路径。
/// - `options`: 转换选项；[`ImagePolicy::Export`] 时按页导出图片并在页末
///   追加占位符，其余策略只返回纯文本。
///
/// # 返回
///
/// 成功时返回全文文本（页间以换页符分隔，`Export` 模式下页末另附占位符）；
/// 失败时返回 [`ConvertError::Pdf`]（解析失败）或
/// [`ConvertError::ImageExport`]（图片导出失败）。
pub fn extract(path: &Path, options: &ConvertOptions) -> Result<String, ConvertError> {
    let document = pdf_oxide::PdfDocument::open(path)?;
    let text = document.extract_all_text()?;

    match options.image_policy() {
        ImagePolicy::Export { dir, url_prefix } => {
            export_page_images(&document, text, &dir, &url_prefix)
        }
        ImagePolicy::Virtual | ImagePolicy::Suppress => Ok(text),
    }
}

/// 按页抽取图片并在各页文本末尾追加占位符。
///
/// # 参数
///
/// - `document`: 已打开的 PDF 文档。
/// - `text`: [`pdf_oxide::PdfDocument::extract_all_text`] 的产出，页间以
///   换页符分隔。
/// - `dir`: 图片写入目录。
/// - `url_prefix`: 占位符 URL 前缀，来自 `--images-dir` 的原始输入。
///
/// # 返回
///
/// 成功时返回重组后的全文。
///
/// 两类失败的处理方式不同：
///
/// - **整体性失败**（目录不可写、某页无法访问）返回
///   [`ConvertError::ImageExport`]——此时继续只会产出残缺结果。
/// - **单张图失败**（解码被拒、单文件写盘失败）不中断：该图不落盘，改为在
///   页末写一行 `[图片N 导出失败: 原因]` 并记 error 日志。直接跳过就成了
///   与「文档里本来没有图」无法区分的静默失败，而为了让一张图放弃整篇正文
///   又过度——PDF 的内联图常省略 `/ColorSpace` 等条目，底层库会拒绝解码。
fn export_page_images(
    document: &pdf_oxide::PdfDocument,
    text: String,
    dir: &Path,
    url_prefix: &str,
) -> Result<String, ConvertError> {
    // 目录不可写是整体性失败，一开始就报错，不要等到逐张写盘时才发现。
    ensure_dir(dir)?;

    let mut pages: Vec<String> = text.split(FEED).map(str::to_string).collect();

    let mut sequence = 0usize;
    for (page_index, page) in pages.iter_mut().enumerate() {
        // 走 handle 路径枚举图片，而不是 `extract_images`：后者处理内联图像
        // （BI...ID...EI）时未补 `/Subtype`，会报「XObject missing /Subtype」
        // 并被底层吞掉，整页表现为「没有图」。PDF 里内联图并不少见（扫描件、
        // 位图字体、矢量转位图的插图都是），漏掉它等于对大量真实文档失效。
        // `page_image_handles` 会为内联图合成带 `/Subtype /Image` 的流，
        // 内联图与 XObject 图因此都能取到。
        let handles =
            document
                .page_image_handles(page_index)
                .map_err(|error| ConvertError::ImageExport {
                    path: dir.display().to_string(),
                    message: format!("第 {} 页抽图失败: {error}", page_index + 1),
                })?;

        // 本页追加到正文末尾的内容：解得开的图给占位符，解不开的给说明行。
        let mut tail: Vec<String> = Vec::with_capacity(handles.len());
        for handle in &handles {
            sequence += 1;
            match decode_and_save(dir, sequence, handle) {
                Ok(file_name) => tail.push(format!(
                    "![图片{sequence}]({})",
                    join_url(url_prefix, &file_name)
                )),
                Err(message) => {
                    // 单张图解不开不该让整篇正文一起消失：PDF 的内联图常省略
                    // `/ColorSpace` 等条目，底层库会拒绝解码。但也不能直接跳过
                    // ——那样就和「文档里本来没有图」无法区分，成了静默失败。
                    // 如实标注这张图没能导出，信息才不丢。
                    log::error!(
                        "第 {} 页第 {sequence} 张图导出失败: {message}",
                        page_index + 1
                    );
                    tail.push(format!("[图片{sequence} 导出失败: {message}]"));
                }
            }
        }

        if !tail.is_empty() {
            // 页末追加：不改动无图的页，保证「带 --images-dir 但无图」与不带该
            // 参数两种情况的输出完全一致。
            *page = attach_placeholders(page, &tail);
        }
    }

    Ok(pages.join(FEED))
}

/// 解码单张图并落盘，返回文件名。
///
/// 失败只返回原因字符串而非 [`ConvertError`]：调用方要据此决定「跳过这张图
/// 并如实标注」，而不是让整篇转换一起失败——部分结果仍有价值，前提是失败
/// 被明确说出来。
///
/// # 参数
///
/// - `dir`: 写入目录。
/// - `sequence`: 全局图片序号，从 1 开始。
/// - `handle`: 底层库给出的图片句柄（含内联图）。
///
/// # 返回
///
/// 成功时返回文件名；失败时返回可读的原因描述。
fn decode_and_save(
    dir: &Path,
    sequence: usize,
    handle: &pdf_oxide::PdfImageHandle<'_>,
) -> Result<String, String> {
    let image = handle.decode().map_err(|error| error.to_string())?;
    save_image(dir, sequence, &image).map_err(|error| error.to_string())
}

/// 把图片占位符追加到某页正文的末尾。
///
/// # 参数
///
/// - `page`: 单页正文（不含换页符）。
/// - `placeholders`: 该页的占位符与说明行，按出现顺序。
///
/// # 返回
///
/// 追加后的页面文本。先去掉正文尾部空白，避免与占位符粘连；页内原本为空时
/// 直接以占位符填充，不产生前导空行。
fn attach_placeholders(page: &str, placeholders: &[String]) -> String {
    let body = page.trim_end();
    if body.is_empty() {
        placeholders.join("\n")
    } else {
        format!("{body}\n\n{}", placeholders.join("\n"))
    }
}

/// 把单张 PDF 图片落盘，返回文件名。
///
/// # 参数
///
/// - `dir`: 写入目录，不存在时自动创建。
/// - `sequence`: 全局图片序号，从 1 开始。
/// - `image`: 底层库抽出的图片。
///
/// # 返回
///
/// 成功时返回文件名；失败返回 [`ConvertError::ImageExport`]。
///
/// # 说明
///
/// JPEG 直接写字节——与 Office 路径的「存原始字节」策略一致，省一次解码
/// 重编码；其余（未编码的像素）必须由底层库编码成 PNG 才能被看图软件打开，
/// 扩展名也相应固定为 `png`，此时不走魔数识别（识别的是像素，不是文件头）。
fn save_image(dir: &Path, sequence: usize, image: &PdfImage) -> Result<String, ConvertError> {
    ensure_dir(dir)?;

    match image.data() {
        ImageData::Jpeg(bytes) if !bytes.is_empty() => write_image_bytes(dir, sequence, bytes),
        _ => {
            let file_name = png_file_name(sequence);
            let target = dir.join(&file_name);
            image
                .save_as_png(&target)
                .map_err(|error| ConvertError::ImageExport {
                    path: target.display().to_string(),
                    message: error.to_string(),
                })?;
            Ok(file_name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 页末追加占位符：正文尾部空白应被去掉，占位符另起两行。
    #[test]
    fn placeholder_is_attached_after_page_body() {
        let out = attach_placeholders("正文内容\n\n", &["![图片1](imgs/img-0001.png)".to_string()]);
        assert_eq!(out, "正文内容\n\n![图片1](imgs/img-0001.png)");
    }

    /// 页内原本为空时直接填充，不产生前导空行；多张图逐行排列。
    #[test]
    fn placeholder_fills_empty_page_without_leading_blanks() {
        let out = attach_placeholders(
            "  \n",
            &[
                "![图片1](imgs/img-0001.png)".to_string(),
                "![图片2](imgs/img-0002.png)".to_string(),
            ],
        );
        assert_eq!(
            out,
            "![图片1](imgs/img-0001.png)\n![图片2](imgs/img-0002.png)"
        );
    }

    /// 页对齐依赖换页符切分：段数即页数，末段不该凭空多出换页符。
    #[test]
    fn page_split_matches_form_feed_contract() {
        let text = "第一页内容\x0C第二页内容\x0C第三页内容";
        let pages: Vec<&str> = text.split(FEED).collect();
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0], "第一页内容");
        assert_eq!(pages[2], "第三页内容");
    }
}
