//! 端到端转换测试：对 `samples/` 下的样例文档逐一解析。
//!
//! 分两层断言：
//!
//! - **链路可用**：各格式都能转出非空结果（不校验具体文本，避免底层库
//!   微调换行就误报）；
//! - **分派正确**：默认输出格式、`-f` 覆盖、不兼容格式的报错、图片导出，
//!   这些是本工具自己承担的契约，必须精确校验。

use std::path::{Path, PathBuf};

use multype2txt::{
    convert_file, convert_with, is_supported, ConvertOptions, ImagePolicy, OutputFormat,
};

/// 拼出 `samples/` 下样例文件的绝对路径。
fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("samples")
        .join(name)
}

/// 转换并断言结果非空。
fn assert_converts(name: &str) {
    let path = sample(name);
    assert!(path.exists(), "缺少样例文件: {}", path.display());

    let text = convert_file(&path).unwrap_or_else(|e| panic!("转换 {name} 失败: {e}"));
    assert!(!text.trim().is_empty(), "{name} 转换结果为空");
}

#[test]
fn converts_docx() {
    assert_converts("demo.docx");
}

#[test]
fn converts_pptx() {
    assert_converts("demo.pptx");
}

#[test]
fn converts_xlsx() {
    assert_converts("demo.xlsx");
}

#[test]
fn converts_pdf() {
    assert_converts("demo.pdf");
}

/// 二进制老格式（DOC/XLS/PPT）走的解析分支与 OOXML 不同，必须一并覆盖。
#[test]
fn converts_legacy_office_formats() {
    for name in ["demo.doc", "demo.xls", "demo.ppt"] {
        assert_converts(name);
    }
}

// ---------------------------------------------------------------------------
// 输出格式分派
// ---------------------------------------------------------------------------

/// doc/docx 默认输出 Markdown：标题必须是 ATX 形式。
///
/// 不限定层级：文档的顶层标题常是 `##` 或 `####`（取决于源文件的样式映射），
/// 这里只断言「存在 Markdown 标题」，层级是否合理属于底层库的样式解析范围。
#[test]
fn docx_defaults_to_markdown() {
    let text = convert_file(&sample("demo.docx")).expect("docx 转换失败");
    let is_heading = |line: &str| {
        let body = line.trim_start_matches('#');
        !body.is_empty() && body.starts_with(' ')
    };
    assert!(
        text.lines().any(is_heading),
        "docx 默认输出应含 Markdown 标题，实际输出前 500 字: {}",
        text.chars().take(500).collect::<String>()
    );
}

/// ppt/pptx 默认输出 Markdown，多张幻灯片之间以主题分隔线隔开。
#[test]
fn pptx_defaults_to_markdown_with_slide_breaks() {
    let text = convert_file(&sample("demo.pptx")).expect("pptx 转换失败");
    assert!(
        text.contains("\n---\n"),
        "pptx 默认输出应以 --- 分隔各幻灯片"
    );
}

/// xls/xlsx 默认输出 CSV，且必须带工作表元信息注释。
#[test]
fn xlsx_defaults_to_csv_with_sheet_header() {
    let text = convert_file(&sample("demo.xlsx")).expect("xlsx 转换失败");
    assert!(
        text.contains("# sheet:"),
        "xlsx 默认输出应以 # sheet: 标注工作表"
    );
    assert!(text.contains(','), "CSV 应含分隔符");
}

/// `-f text` 覆盖默认格式：拿到的是丢弃结构后的纯文本。
#[test]
fn format_can_be_overridden_to_text() {
    let options = ConvertOptions::new().with_format(OutputFormat::Text);
    let text = convert_with(&sample("demo.docx"), &options).expect("强制纯文本失败");
    assert!(!text.trim().is_empty());
    assert!(
        !text.contains("| --- |"),
        "纯文本输出不应残留 Markdown 表格分隔行"
    );
}

// ---------------------------------------------------------------------------
// 不兼容输入：宁可失败，也不静默降级成别的格式
// ---------------------------------------------------------------------------

#[test]
fn csv_refused_for_document() {
    let options = ConvertOptions::new().with_format(OutputFormat::Csv);
    let err = convert_with(&sample("demo.docx"), &options).expect_err("docx 不应能导出 CSV");
    assert!(matches!(
        err,
        multype2txt::ConvertError::IncompatibleOutput { .. }
    ));
}

#[test]
fn markdown_refused_for_pdf() {
    let options = ConvertOptions::new().with_format(OutputFormat::Markdown);
    let err = convert_with(&sample("demo.pdf"), &options).expect_err("PDF 不应能输出 Markdown");
    assert!(matches!(
        err,
        multype2txt::ConvertError::IncompatibleOutput { .. }
    ));
}

// ---------------------------------------------------------------------------
// 图片导出
// ---------------------------------------------------------------------------

/// `--images-dir` 语义：字节落盘、占位符指向真实文件。
///
/// 样例幻灯片含多张图片，若底层未提供字节，这里会以「文档未提供图片字节」
/// 失败——那属于能力缺陷，应当暴露而不是被吞掉。
#[test]
fn images_dir_exports_bytes_and_points_to_them() {
    let dir = std::env::temp_dir().join(format!("multype2txt-img-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let options = ConvertOptions::new().with_images(ImagePolicy::Export {
        dir: dir.clone(),
        url_prefix: dir.to_string_lossy().to_string(),
    });
    let text = convert_with(&sample("demo.pptx"), &options)
        .unwrap_or_else(|e| panic!("带图片导出的转换失败: {e}"));

    let exported = std::fs::read_dir(&dir)
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert!(exported > 0, "未导出任何图片文件到 {}", dir.display());
    assert!(
        text.contains("img-0001"),
        "占位符应指向导出的文件，实际输出前 500 字: {}",
        text.chars().take(500).collect::<String>()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// PDF 的 `--images-dir` 必须跑通且不破坏正文。
///
/// 样例 PDF 有 15 张内联图，底层库对这类图可能拒绝解码（缺 `/ColorSpace`）。
/// 因此这里只锁两件事：页边界不因追加占位符而改变；图片无论导出成功还是
/// 失败，都必须在页末留下痕迹——静默无痕会让阅读方以为文档里本来没有图。
#[test]
fn pdf_images_dir_keeps_full_text_and_marks_images() {
    let dir = std::env::temp_dir().join(format!("multype2txt-pdf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let options = ConvertOptions::new().with_images(ImagePolicy::Export {
        dir: dir.clone(),
        url_prefix: dir.to_string_lossy().to_string(),
    });
    let text = convert_with(&sample("demo.pdf"), &options)
        .unwrap_or_else(|e| panic!("带图片导出的 PDF 转换失败: {e}"));

    assert_eq!(text.split('\x0C').count(), 42, "追加占位符不应改变页边界");
    assert!(
        text.contains("图片"),
        "样例 PDF 含 15 张图，页末应有占位符或导出失败说明，实际输出末尾 300 字: {}",
        text.chars()
            .rev()
            .take(300)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// 支持范围
// ---------------------------------------------------------------------------

#[test]
fn rejects_unsupported_extension() {
    let path = sample("demo.txt");
    assert!(!is_supported(&path));
    let err = convert_file(&path).expect_err("txt 不应被支持");
    assert!(matches!(
        err,
        multype2txt::ConvertError::UnsupportedFormat { .. }
    ));
}

#[test]
fn rejects_missing_extension() {
    let path = PathBuf::from("no_extension");
    assert!(!is_supported(&path));
    let err = convert_file(&path).expect_err("无扩展名不应被支持");
    assert!(matches!(
        err,
        multype2txt::ConvertError::MissingExtension(_)
    ));
}

/// XLSB 明确不在支持范围内（依赖的 rxlsb 存在列错位与公式值丢失缺陷）。
#[test]
fn rejects_xlsb() {
    let path = sample("demo.xlsb");
    assert!(!is_supported(&path));
    let err = convert_file(&path).expect_err("xlsb 暂不应被支持");
    assert!(matches!(
        err,
        multype2txt::ConvertError::UnsupportedFormat { .. }
    ));
}
