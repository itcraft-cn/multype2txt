//! PDF 词坐标探针：验证 `extract_words()` 的坐标数据质量，为自研表格重建铺路。
//!
//! 输出指定页的每个词及其包围盒（x0/y0/x1/y1，PDF point，y 轴向上），
//! 供人工核对行聚类与列锚点的可行性。输出走 stdout（探针工具，非生产路径）。
//!
//! 运行：
//! ```bash
//! cargo run --example pdf_words_probe -- samples/demo2.pdf 57 58
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
        println!("=== 第 {page_1} 页：{} 词 ===", words.len());
        // 按 y 降序（页面从上到下）再按 x 升序输出，便于直接观察行列结构。
        let mut sorted: Vec<_> = words.iter().collect();
        sorted.sort_by(|a, b| {
            b.bbox
                .y
                .partial_cmp(&a.bbox.y)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    a.bbox
                        .x
                        .partial_cmp(&b.bbox.x)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });
        for word in sorted {
            let b = word.bbox;
            println!(
                "x0={:7.1} y0={:7.1} x1={:7.1} y1={:7.1} fs={:4.1} seq={:4} {}",
                b.x,
                b.y,
                b.x + b.width,
                b.y + b.height,
                word.avg_font_size,
                word.sequence,
                word.text.escape_debug()
            );
        }
    }
}
