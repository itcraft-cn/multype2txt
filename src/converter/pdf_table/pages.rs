//! 回归门槛：真实页面的词坐标与整篇文档的结构判据。
//!
//! # 数据来源
//!
//! `tests/data/pdf/page<N>.words.tsv` 由 `examples/pdf_words_probe` 从
//! `samples/demo2.pdf` / `samples/demo.pdf` 导出，只含词坐标与文本（不含 PDF 本体），
//! 因此这些测试毫秒级、确定性强，也不把仓库撑大。更新方式见该 example 的文档。
//!
//! # 断言取向
//!
//! 只断言**结构性质**（列数、行归属、是否退化、是否丢字），不断言整段字符串——
//! 整串快照会把「空格多一个」这种无害改动变成红灯，反而掩盖真正的错位。

#![cfg(test)]

use super::word::RawWord;
use super::{rebuild_page, rebuild_with, Profile};

/// 从 TSV 读一页的词。
fn load(page: &str) -> Vec<RawWord> {
    let path = format!(
        "{}/tests/data/pdf/{page}.words.tsv",
        env!("CARGO_MANIFEST_DIR")
    );
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读取 {path}: {e}"));
    let mut words = Vec::new();
    for line in content.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 9 {
            continue;
        }
        words.push(RawWord {
            x0: fields[0].parse().unwrap_or(0.0),
            y0: fields[1].parse().unwrap_or(0.0),
            x1: fields[2].parse().unwrap_or(0.0),
            y1: fields[3].parse().unwrap_or(0.0),
            font_size: fields[4].parse().unwrap_or(0.0),
            sequence: fields[7].parse().unwrap_or(0),
            text: fields[8..].join("\t"),
        });
    }
    words
}

/// 去掉空白后的字符集合，用于「没丢字」的断言。
fn compact(text: &str) -> Vec<char> {
    let mut chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    chars.sort_unstable();
    chars
}

/// 去掉空白后判断某行是否包含指定片段：空格多少属于版式噪声，不该让断言变红。
fn line_has_compact(text: &str, needle: &str) -> bool {
    let target: String = needle.chars().filter(|c| !c.is_whitespace()).collect();
    text.lines().any(|line| {
        line.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .contains(&target)
    })
}

/// 找出包含指定文本的行。
fn line_with<'a>(text: &'a str, needle: &str) -> &'a str {
    text.lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("输出中找不到 {needle:?}，实际输出：\n{text}"))
}

fn max_line_width(text: &str) -> usize {
    text.lines()
        .map(super::word::display_width)
        .max()
        .unwrap_or(0)
}

/// 三列表：行、列归属与 rowspan 留空都要对。
#[test]
fn page58_three_column_table() {
    let text = rebuild_page(load("page58")).expect("应检出表格");
    // 表头同一行，三列各自成格。
    let header = line_with(&text, "区域");
    assert!(header.contains("特例的数量") && header.contains("放大倍数"));
    // 数据行：rowspan 单元格与其右侧两列同行。
    let green = line_with(&text, "绿区");
    assert!(green.contains("40-59") && green.contains("(= 1)"));
    let red = line_with(&text, "红区");
    assert!(
        line_has_compact(red, "200 及200以上"),
        "多词单元格内容缺失：{red}"
    );
    assert!(red.contains("3.0"));
    // 列对齐：第一列两行的结束位置一致（该列在原文档为右对齐，渲染复制了这一点）。
    let end = |line: &str, needle: &str| {
        let start = line.find(needle).expect("定位");
        super::word::display_width(&line[..start + needle.len()])
    };
    assert_eq!(end(header, "区域"), end(green, "绿区"));
    assert!(max_line_width(&text) < 120);
}

/// 两行的表也要还原：区块门槛是结构定义（两行即可验证列的复现）。
#[test]
fn page36_two_row_tables() {
    let text = rebuild_page(load("page36")).expect("应检出两行小表");
    let ratings = line_with(&text, "信用评级");
    assert!(ratings.contains("AAA 至 AA-"), "实际：{ratings}");
    assert!(ratings.contains("未评级"));
    let weights = text
        .lines()
        .find(|line| line.contains("风险权重") && line.contains("150%"))
        .unwrap_or_else(|| panic!("找不到风险权重行，实际输出：\n{text}"));
    assert!(weights.contains("0%"), "实际：{weights}");
}

/// 紧邻的两张不同列表必须各自成表，不能把后表表头塞进前表槽位。
#[test]
fn page127_adjacent_tables_are_split() {
    let text = rebuild_page(load("page127")).expect("应检出表格");
    assert!(
        line_has_compact(&text, "B+ 及B+以下或未评级的"),
        "长期评级表头缺失"
    );
    // 短期评级表必须独立成块：它的表头不得与长期评级表的数值同行。
    for line in text.lines() {
        assert!(
            !(line.contains("短期评级种类") && line.contains("扣减")),
            "两张表被合并：{line}"
        );
    }
}

/// 旋转页：包围盒高度被污染，但不应退化成一行巨串，且内容完整。
#[test]
fn page193_rotated_page_is_rebuilt_without_degeneration() {
    let words = load("page193");
    let expected = compact(&words.iter().map(|w| w.text.clone()).collect::<String>());
    let text = rebuild_page(words).expect("旋转页应可还原");
    assert_eq!(compact(&text), expected, "内容不得丢失");
    assert!(max_line_width(&text) < 120, "不得出现超长行");
    let first = line_with(&text, "1.30%");
    assert!(first.contains("63.88%"), "末行数值应完整：{first}");
}

/// 正文页（含两端对齐的英文行）不得被判为表格，保持基线阅读顺序。
#[test]
fn page57_prose_stays_prose() {
    assert!(rebuild_page(load("page57")).is_none());
}

/// 目录页不产生表格：引导线行是排版填充，页面结构留给基线文本。
#[test]
fn page5_toc_rows_are_not_tables() {
    assert!(rebuild_page(load("page5")).is_none());
}

/// 产品线对应表：三列 + 两级目录 + 单元格内换行。
///
/// 这类表的续行与列间空白交织，区块会被切成单行片段，列结构不稳定——本测试只锁
/// 「不丢字、不退化」；列间整并属于已知边界（见模块文档）。
#[test]
fn page213_mapping_table_keeps_content() {
    let words = load("page213");
    let expected = compact(&words.iter().map(|w| w.text.clone()).collect::<String>());
    match rebuild_page(words) {
        Some(text) => {
            assert_eq!(compact(&text), expected);
            assert!(max_line_width(&text) < 120);
        }
        None => assert!(!expected.is_empty(), "回退基线时内容仍在 PDF 里"),
    }
}

/// 参数化入口可用：提高行数门槛会让两行表回退。
#[test]
fn profile_is_the_only_knob() {
    let strict = Profile {
        min_block_rows: 3,
        ..Profile::default()
    };
    assert!(rebuild_with(load("page36"), &strict).is_none());
    assert!(rebuild_with(load("page36"), &Profile::default()).is_some());
}

/// 整篇回归门槛：232 页不漏字、不出现退化行、还原页数不低于基线水位。
///
/// 这条测试直接跑真实 PDF（约 1 秒），是「改了就量化」的落点：任何算法改动
/// 都必须同时满足内容完整与不退化两个硬指标。
#[test]
fn whole_document_regression_gate() {
    let path = format!("{}/samples/demo2.pdf", env!("CARGO_MANIFEST_DIR"));
    let document = pdf_oxide::PdfDocument::open(std::path::Path::new(&path))
        .unwrap_or_else(|e| panic!("打开 {path}: {e}"));
    let baseline = document.extract_all_text().expect("基线文本");
    let pages: Vec<&str> = baseline.split('\x0C').collect();

    let mut accepted_pages = 0;
    for (index, page_text) in pages.iter().enumerate() {
        let words = match document.extract_words(index) {
            Ok(words) => words,
            Err(_) => continue,
        };
        let input: Vec<RawWord> = words
            .iter()
            .map(|w| RawWord {
                text: w.text.clone(),
                x0: w.bbox.x,
                y0: w.bbox.y,
                x1: w.bbox.x + w.bbox.width,
                y1: w.bbox.y + w.bbox.height,
                font_size: w.avg_font_size,
                sequence: w.sequence,
            })
            .collect();
        let Some(text) = rebuild_page(input) else {
            continue;
        };

        // 1. 不得少字（允许多出页脚页码等基线未取到的内容）。
        let baseline_chars = compact(page_text);
        let mut remaining = compact(&text);
        let mut missing = 0;
        for ch in &baseline_chars {
            match remaining.iter().position(|c| c == ch) {
                Some(pos) => {
                    remaining.remove(pos);
                }
                None => missing += 1,
            }
        }
        // 2. 不得出现退化行（旧实现在旋转页上曾输出 700+ 字符的一行）。
        assert!(
            max_line_width(&text) <= 200,
            "第 {} 页出现退化行（{} 显示列）",
            index + 1,
            max_line_width(&text)
        );
        // 1. 生产路径只接受不少字的结果；少字的页（基线含重复绘制）会回退基线，
        //    因此这里只统计被接受的页，并由下界断言防止「大面积回退」的退化。
        if missing == 0 {
            accepted_pages += 1;
        }
    }
    assert!(
        accepted_pages >= 60,
        "还原页数退化：{accepted_pages}（基线水位 60+）"
    );
}

/// 值表跨行串位是实质性错误：同一行的数值必须来自同一视觉行。
///
/// 页 194 是 /Rotate 页，且基线（`extract_all_text`）对这页几乎无输出；此处的
/// 断言全部落在「行归属」上：10.00% 行只允许出现它自己的八个数值，不得混入
/// 上一行（203.12%…）或下一行（307.24%…）。
#[test]
fn page194_rows_are_not_merged() {
    let text = rebuild_page(load("page194")).expect("应检出表格");
    let row = line_with(&text, "250.22%");
    // 按去空白比较：数值被拆开时的空格属版式噪声，行列归属才是本测试的对象。
    let compact_row: String = row.chars().filter(|c| !c.is_whitespace()).collect();
    for expected in ["204.50%", "260.66%", "144.81%", "69.51%", "131.30%"] {
        assert!(
            compact_row.contains(expected),
            "10.00% 行缺少 {expected}：{row}"
        );
    }
    for foreign in ["307.24%", "203.12%", "176.35%"] {
        assert!(
            !compact_row.contains(foreign),
            "10.00% 行混入了相邻行的 {foreign}：{row}"
        );
    }
    // 被拆开的数值必须重连：`117.` + `95%`（按去空白比较，间距属版式噪声）。
    assert!(line_has_compact(row, "117.95%"), "数值未重连：{row}");
    assert!(max_line_width(&text) < 120);
}

/// 长文本多列表：目前只保证「不退化、不丢字、行不串」，列间整并是已知边界。
#[test]
fn page195_long_text_table_does_not_degenerate() {
    let words = load("page195");
    let expected = compact(&words.iter().map(|w| w.text.clone()).collect::<String>());
    let text = rebuild_page(words).expect("应检出表格");
    assert_eq!(compact(&text), expected, "内容不得丢失");
    assert!(max_line_width(&text) < 200, "不得出现退化行");
}

/// 伪粗体（同一字形绘制两遍）不得产生 `fairfair` 这类重复。
#[test]
fn market_risk_page6_has_no_duplicated_text() {
    let text = rebuild_page(load("market-risk6")).expect("应检出表格");
    let _ = &text;
    for doubled in ["fairfair", "valuedvalued", "thethe", "P&LP&L"] {
        assert!(!text.contains(doubled), "出现重复绘制残留：{doubled}");
    }
    assert!(text.contains("fair valued"));
}

#[test]
fn dbg_57() {
    let words = load("page57");
    let profile = Profile::default();
    let normalized = super::word::normalize(words, &profile);
    let em = super::lines::page_em(&normalized);
    let rows = super::lines::group_rows(normalized, em, &profile);
    for seg in super::lines::segments(&rows, em, &profile) {
        if seg.len() < 2 {
            continue;
        }
        let cols = super::columns::detect(&rows[seg.clone()], em, &profile);
        println!(
            "seg {}..{} n={} cols={:?}",
            seg.start,
            seg.end,
            seg.len(),
            cols.as_ref().map(|c| c.len())
        );
        if let Some(c) = cols {
            println!(
                "  {:?}",
                c.iter()
                    .map(|x| (x.x0 as i32, x.x1 as i32))
                    .collect::<Vec<_>>()
            );
        }
    }
}
