//! 输出自检：重建结果必须先证明「没有丢字、没有退化」，才允许替换基线文本。
//!
//! 这两条不变式是整条管线的安全阀，也是旧实现最缺的一环——旧实现对「列槽位塑形
//! 失败」没有任何自检，直接把一页渲染成几百个空格填充的一行。
//!
//! 1. **字符守恒**：重建后去掉空白字符，必须与输入词文本去空白后逐字符相等。
//!    丢字、重复输出、文本被吞都会被这一条拦下。
//! 2. **行宽有界**：纯文本一行的显示宽度不可能超过该页词内容横向跨度所对应的字符
//!    数上界（跨度 ÷ 最窄合理字宽 0.5 em）。超过即为槽位/列宽失控，整页降级。
//!
//! 自检失败一律返回基线文本并记 warn 日志——宁可不还原，也不输出错误的结构。

use super::lines::Row;
use super::word::display_width;

/// 最窄合理字宽：西文窄字（i、l、标点）的推进可低至 0.35 em。
///
/// 来源：字体度量。它只用于给行宽设一个**上界**，不参与任何版式判定，
/// 因此取理论下限，宁可放宽也不误杀。
const MIN_ADVANCE_EM: f32 = 0.35;

/// 自检：字符守恒 + 行宽有界。
///
/// 返回 `Err(原因)` 时调用方必须回退基线。
pub(crate) fn check(rows: &[Row], output: &str, columns: usize) -> Result<(), String> {
    let expected = concatenated_text(rows);
    let actual: String = output.chars().filter(|c| !c.is_whitespace()).collect();
    if expected != actual {
        let expected_len = expected.chars().count();
        let actual_len = actual.chars().count();
        return Err(format!(
            "重建结果与输入词不一致（输入 {expected_len} 字符，输出 {actual_len} 字符）"
        ));
    }

    if let Some((span, em)) = page_extent(rows) {
        // 列间分隔符（每列两个空格）是渲染的显式开销，计入上界。
        let bound = (span / (MIN_ADVANCE_EM * em)).ceil() as usize + 2 * columns + 1;
        for line in output.lines() {
            let width = display_width(line);
            if width > bound {
                return Err(format!("行宽 {width} 超过物理上界 {bound}，判定渲染失控"));
            }
        }
    }
    Ok(())
}

/// 重建结果是否覆盖基线的全部字符（忽略空白与顺序）。
///
/// 供调用方做「跨抽取路径」的一致性检查：重建可以改变空白与顺序，可以多出基线
/// 没取到的字符（实测 `extract_all_text` 不取页脚页码，而词路径会取到），但**绝不
/// 允许少字**——少字意味着丢数据。
///
/// 两条路径偶尔会给出不同字符集合（实测 demo2 页 45/127 基线含重复绘制的评级
/// 字符串，词路径只有一份），这类分歧由调用方裁决为「回退基线」，不在重建侧猜。
pub(crate) fn covers_baseline(baseline: &str, rebuilt: &str) -> bool {
    let (missing, _, _, _) = char_diff(baseline, rebuilt);
    missing == 0
}

/// 两段文本的字符多重集差异：`(缺失数, 缺失样张, 溢出数, 溢出样张)`。
///
/// 返回样张而非全量，便于日志里定位问题页而不刷屏。
pub(crate) fn char_diff(left: &str, right: &str) -> (usize, String, usize, String) {
    use std::collections::HashMap;
    let mut counts: HashMap<char, i64> = HashMap::new();
    for c in left.chars().filter(|c| !c.is_whitespace()) {
        *counts.entry(c).or_insert(0) += 1;
    }
    for c in right.chars().filter(|c| !c.is_whitespace()) {
        *counts.entry(c).or_insert(0) -= 1;
    }
    let mut missing = 0;
    let mut missing_sample = String::new();
    let mut extra = 0;
    let mut extra_sample = String::new();
    let mut keys: Vec<char> = counts.keys().copied().collect();
    keys.sort_unstable();
    for key in keys {
        let value = counts[&key];
        for _ in 0..value.abs() {
            if value > 0 {
                missing += 1;
                if missing_sample.chars().count() < 24 {
                    missing_sample.push(key);
                }
            } else if value < 0 {
                extra += 1;
                if extra_sample.chars().count() < 24 {
                    extra_sample.push(key);
                }
            }
        }
    }
    (missing, missing_sample, extra, extra_sample)
}

/// 输入词文本拼成一串（去空白），用于字符守恒比较。
fn concatenated_text(rows: &[Row]) -> String {
    let mut text = String::new();
    for row in rows {
        for word in &row.words {
            text.extend(word.text.chars().filter(|c| !c.is_whitespace()));
        }
    }
    text
}

/// 页面词内容的横向跨度与字号代表值。
fn page_extent(rows: &[Row]) -> Option<(f32, f32)> {
    let mut min_x0 = f32::INFINITY;
    let mut max_x1 = f32::NEG_INFINITY;
    let mut sizes: Vec<f32> = Vec::new();
    for row in rows {
        for word in &row.words {
            min_x0 = min_x0.min(word.x0);
            max_x1 = max_x1.max(word.x1);
            sizes.push(word.font_size);
        }
    }
    if !min_x0.is_finite() || !max_x1.is_finite() || sizes.is_empty() {
        return None;
    }
    let em = super::word::median(&mut sizes);
    if em <= 0.0 {
        return None;
    }
    Some((max_x1 - min_x0, em))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::pdf_table::word::PlacedWord;

    fn rows(words: &[(&str, f32, f32)]) -> Vec<Row> {
        let placed: Vec<PlacedWord> = words
            .iter()
            .map(|(text, x0, x1)| PlacedWord {
                text: text.to_string(),
                x0: *x0,
                y0: 600.0,
                x1: *x1,
                y1: 610.0,
                font_size: 10.0,
                sequence: 0,
            })
            .collect();
        vec![Row {
            words: placed,
            y0: 600.0,
            y1: 610.0,
        }]
    }

    /// 字符守恒：空白差异不算，丢字/多字算。
    #[test]
    fn chars_must_be_preserved() {
        let input = rows(&[("区域", 100.0, 130.0), ("数量", 200.0, 230.0)]);
        assert!(check(&input, "区域  数量", 2).is_ok());
        assert!(check(&input, "区域\n  数量  ", 2).is_ok());
        assert!(check(&input, "区域", 2).is_err(), "丢字必须被拦下");
        assert!(
            check(&input, "区域  数量  倍数", 2).is_err(),
            "多字必须被拦下"
        );
    }

    /// 跨路径检查：忽略空白与顺序，允许多出，不允许少字。
    #[test]
    fn cross_path_check_allows_extras_but_never_missing_chars() {
        assert!(covers_baseline("区域  数量", "区域\n数量"));
        assert!(covers_baseline("区域  数量", "数量区域"), "顺序可容忍");
        assert!(covers_baseline("区域", "区域 11"), "页码等多出的内容可接受");
        assert!(!covers_baseline("区域 数量", "区域"), "少字必须被拦下");
        assert!(!covers_baseline("AAA", "AA"));
    }

    /// 行宽失控必须被拦下：这正是旧实现在旋转页上输出的 700+ 字符巨串。
    #[test]
    fn runaway_line_width_is_rejected() {
        let input = rows(&[("甲", 100.0, 110.0), ("乙", 300.0, 310.0)]);
        let wide = format!("甲{}乙", " ".repeat(400));
        assert!(check(&input, &wide, 2).is_err());
    }
}
