//! PDF 词坐标探针 / 测试夹具生成器。
//!
//! 按页输出 `extract_words()` 的原始数据：包围盒、字号、文本矩阵方向、
//! 内容流发射序与词文本。输出走 stdout（探针工具，非生产路径），制表符分隔，
//! 一页一组，组首为 `# page <页码> <词数>`。
//!
//! 两个用途：
//!
//! 1. 人工核对：直接观察某页的词坐标、字号与方向，判断行聚类/列边界是否可能成立；
//! 2. 生成 `tests/fixtures/pdf/` 下的夹具：夹具只存词坐标，不存 PDF，
//!    单元测试因此无需解析 PDF，毫秒级且确定。
//!
//! 运行：
//! ```bash
//! cargo run --example pdf_words_probe -- samples/demo2.pdf 58 > page58.words.tsv
//! cargo run --example pdf_words_probe -- samples/demo2.pdf 193 207 | head -20
//! ```

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("用法: pdf_words_probe <file.pdf> <页码(1开始)>...");
        process::exit(2);
    }
    let doc = match pdf_oxide::PdfDocument::open(std::path::Path::new(&args[1])) {
        Ok(doc) => doc,
        Err(error) => {
            eprintln!("打开失败: {error}");
            process::exit(1);
        }
    };
    for arg in &args[2..] {
        let page_1: usize = match arg.parse() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("跳过非法页码: {arg}");
                continue;
            }
        };
        let index = page_1.saturating_sub(1);
        let words = match doc.extract_words(index) {
            Ok(words) => words,
            Err(error) => {
                eprintln!("第 {page_1} 页抽取失败: {error}");
                continue;
            }
        };
        println!("# page {page_1} {}", words.len());
        for word in &words {
            // 文本矩阵的 (a, b) 给出该词在设备空间的推进方向，
            // 旋转判定只依赖这两个分量（完整矩阵对聚类无用）。
            let (a, b) = word
                .chars
                .first()
                .and_then(|ch| ch.matrix)
                .map(|m| (m[0], m[1]))
                .unwrap_or((1.0, 0.0));
            let bbox = word.bbox;
            println!(
                "{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.2}\t{:.4}\t{:.4}\t{}\t{}",
                bbox.x,
                bbox.y,
                bbox.x + bbox.width,
                bbox.y + bbox.height,
                word.avg_font_size,
                a,
                b,
                word.sequence,
                // 制表符/换行会破坏 TSV 结构，夹具里以空格代替（不影响几何与文本判定）。
                word.text.replace(['\t', '\n', '\r'], " ")
            );
        }
    }
}
