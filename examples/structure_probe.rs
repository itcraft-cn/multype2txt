//! 结构还原探针：量化 `plain_text()` 与 `to_markdown()` / `to_html()`
//! 之间的结构信息差距。
//!
//! 用途：回答「把 multype2txt 从纯文本升级为结构化输出，能拿到多少增量」。
//! 输出为诊断信息，走日志（stderr），不污染标准输出。
//!
//! 运行：
//! ```bash
//! cargo run --example structure_probe
//! ```

use office_oxide::Document;

fn count_markers(md: &str) -> (usize, usize, usize, usize) {
    let headings = md
        .lines()
        .filter(|l| l.trim_start().starts_with('#'))
        .count();
    let table_rows = md
        .lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .count();
    let list_items = md
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ")
        })
        .count();
    let links = md.matches("](").count();
    (headings, table_rows, list_items, links)
}

/// 统计 HTML 中才可能保留的合并单元格数量。
fn merge_attrs(html: &str) -> (usize, usize) {
    let colspan = html.matches("colspan=").count();
    let rowspan = html.matches("rowspan=").count();
    (colspan, rowspan)
}

/// 统计图片占位：markdown 形如 `![alt](...)`。
fn images(md: &str) -> (usize, usize) {
    let all = md.matches("![");
    let total = all.count();
    let empty_url = md.matches("]()").count();
    (total, empty_url)
}

fn main() {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .init();

    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/samples");
    for name in ["demo.docx", "demo.xlsx", "demo.pptx"] {
        let path = std::path::Path::new(dir).join(name);
        if !path.exists() {
            log::warn!("跳过缺失样例: {}", path.display());
            continue;
        }
        let doc = match Document::open(&path) {
            Ok(d) => d,
            Err(e) => {
                log::error!("打开 {name} 失败: {e}");
                continue;
            }
        };

        let text = doc.plain_text();
        let md = doc.to_markdown();
        let html = doc.to_html();

        let (h, t, l, links) = count_markers(&md);
        let (cs, rs) = merge_attrs(&html);
        let (img, img_empty) = images(&md);

        log::info!("=== {name} ===");
        log::info!("  plain_text : {:>6} 字符", text.chars().count());
        log::info!("  markdown   : {:>6} 字符", md.chars().count());
        log::info!("  html       : {:>6} 字符", html.chars().count());
        log::info!(
            "  markdown 结构标记: 标题 {} / 表格行 {} / 列表项 {} / 链接 {}",
            h,
            t,
            l,
            links
        );
        log::info!("  html 合并单元格: colspan={} rowspan={}", cs, rs);
        log::info!(
            "  图片: markdown 共 {} 个，其中 URL 为空 {} 个",
            img,
            img_empty
        );

        // 首个样例附带 markdown 预览，用于人工判断结构还原质量。
        if name == "demo.docx" {
            for (i, line) in md.lines().take(40).enumerate() {
                log::info!("    {:>2} | {}", i + 1, line);
            }
        }
    }
}
