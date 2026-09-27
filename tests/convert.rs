//! 端到端转换测试：对 `samples/` 下的样例文档逐一解析。
//!
//! 这些测试依赖仓库内随附的小体积样例文件，用于验证各格式的解析链路可用，
//! 不校验具体文本内容（不同库版本可能微调换行与空白）。

use std::path::{Path, PathBuf};

use multype2txt::{convert_file, is_supported};

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
