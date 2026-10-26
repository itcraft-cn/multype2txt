//! 词与字符串：几何归一化、显示宽度、词间空格判定。
//!
//! # 归一化（[`normalize`]）
//!
//! 底层库给出的词包围盒并非总是可信：`pdf_oxide 0.3.73` 在带 `/Rotate` 的
//! 页面上把「词宽」写进了包围盒高度（5 字词的包围盒变成 48×53 的方块），
//! 于是同一档的两行词在垂直方向上互相吞并。本模块用**字体度量**作为裁判：
//! 一行文字的高度不可能超过字号 × [`Profile::line_box_em`]；一旦超过，就判定
//! 高度字段失效，只采信与真值吻合的那条边（实测下沿 `y1` 与 `pdftotext -bbox`
//! 的真值差 ≤3pt，见 `tests/data/pdf/page193.words.tsv`），高度改由字号推导。
//!
//! 这样做的意义在于：旋转页不需要「猜方向」，也不需要 `h > 2×字号` 这类经验
//! 判据——旋转与否只影响「高度字段是否可采信」这一件事，而这件事由字体度量
//! 直接判定。

use super::profile::Profile;

/// 底层库给出的一个词，坐标原样保留（PDF 设备坐标，y 轴向上）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawWord {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub font_size: f32,
    /// 内容流发射序（同一文本段的词共享一个序号）。
    pub sequence: usize,
}

/// 归一化后的词：几何已可用，其余字段原样保留。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlacedWord {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub font_size: f32,
    /// 内容流发射序：相邻词序号不同，说明它们是两次独立的绘制操作。
    pub sequence: usize,
}

impl PlacedWord {
    /// 水平中心：列归属判定用它，配合区间命中。
    pub fn center_x(&self) -> f32 {
        (self.x0 + self.x1) / 2.0
    }

    /// 词带高度。
    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }
}

/// 归一化：剔除空词，修正在版式上不可能成立的高度。
///
/// 判定只用字号，不参考页面是否旋转：高度超过 [`Profile::line_box_em`] 倍字号
/// 即视为不可信，此时保留下沿、按字号重算上沿。
pub(crate) fn normalize(raw: Vec<RawWord>, profile: &Profile) -> Vec<PlacedWord> {
    let mut words: Vec<PlacedWord> = Vec::with_capacity(raw.len());
    for w in raw {
        if w.text.trim().is_empty() {
            continue;
        }
        if w.x1 <= w.x0 {
            continue;
        }
        let mut word = PlacedWord {
            text: w.text,
            x0: w.x0,
            y0: w.y0,
            x1: w.x1,
            y1: w.y1,
            font_size: w.font_size.max(1.0),
            sequence: w.sequence,
        };
        let line_box = profile.line_box_em * word.font_size;
        if word.height() > line_box {
            // 下沿可信、高度不可信：保留下沿，按字体度量重算上沿。
            word.y0 = word.y1 - line_box;
        }
        words.push(word);
    }
    deduplicate(&mut words);
    words
}

/// 同一位置重复绘制的词只保留一份。
///
/// PDF 里「伪粗体」用同一字形画两遍（第二遍常有亚 point 偏移），部分生成器还会
/// 为了强调再叠一次；词路径会把它们都取出来，直接拼接就成了 `fairfair`。判据是
/// **文本相同且包围盒几乎重合**——位置不同的相同词（表格里重复出现的关键字）不受
/// 影响。
fn deduplicate(words: &mut Vec<PlacedWord>) {
    // 按 (文本, x0) 排序后只需要与相邻候选比较，避免 O(n²)。
    let mut order: Vec<usize> = (0..words.len()).collect();
    order.sort_by(|a, b| {
        words[*a].text.cmp(&words[*b].text).then(
            words[*a]
                .x0
                .partial_cmp(&words[*b].x0)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    let mut dropped = vec![false; words.len()];
    for (position, &index) in order.iter().enumerate() {
        if dropped[index] {
            continue;
        }
        for &other in order.iter().skip(position + 1) {
            if words[other].text != words[index].text {
                break;
            }
            if words[other].x0 - words[index].x0 > 1.0 {
                break;
            }
            if overlaps_almost(&words[index], &words[other]) {
                dropped[other] = true;
            }
        }
    }
    let mut keep = dropped.iter().map(|d| !d);
    words.retain(|_| keep.next().unwrap_or(true));
}

/// 两个词是否几乎完全重合（中心距 < 半个字宽且垂直重叠过半）。
fn overlaps_almost(left: &PlacedWord, right: &PlacedWord) -> bool {
    let center_dx = (left.center_x() - right.center_x()).abs();
    let tolerance = 0.5 * left.font_size.max(right.font_size);
    if center_dx > tolerance {
        return false;
    }
    let overlap = left.y1.min(right.y1) - left.y0.max(right.y0);
    let min_height = left.height().min(right.height());
    overlap > 0.5 * min_height
}

/// 中位数（会排序传入切片）。
pub(crate) fn median(values: &mut [f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

/// 字符是否占两列显示宽度（CJK 与全角符号）。
///
/// 自实现而不引 `unicode-width`：对齐与空格判定只需要覆盖中日韩宽字符的主要
/// 码位区间，覆盖不全的后果只是对齐差一列，不影响数据。
pub(crate) fn is_wide_char(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        0x1100..=0x115F   // 谚文字母
        | 0x2E80..=0x303E // CJK 部首、假名标点
        | 0x3041..=0x33FF // 假名、注音、CJK 兼容
        | 0x3400..=0x4DBF // CJK 扩展 A
        | 0x4E00..=0x9FFF // CJK 主区
        | 0xA000..=0xA4CF // 彝文
        | 0xAC00..=0xD7A3 // 韩文音节
        | 0xF900..=0xFAFF // CJK 兼容表意
        | 0xFE30..=0xFE4F // CJK 兼容形式
        | 0xFF00..=0xFF60 // 全角字符
        | 0xFFE0..=0xFFE6 // 全角符号
    )
}

/// 字符串显示宽度：宽字符 2 列，其余 1 列。
pub(crate) fn display_width(s: &str) -> usize {
    s.chars().map(|c| if is_wide_char(c) { 2 } else { 1 }).sum()
}

/// 两个相邻词之间是否补一个空格。
///
/// 判定只用排版事实与内容流事实，不依赖具体文档：
///
/// 1. 两侧都是 CJK：中文没有词间分隔符，补空格等于凭空制造词边界，不补；
/// 2. 两个西文词来自**不同绘制段**（[`PlacedWord::sequence`] 不同）且不重叠：
///    不同绘制段不可能是同一个词的拆分，即使间隙不足一个空格宽也要分开——
///    图示里的标签框、并排的独立文本框都属于这种情况；
/// 3. 间隙小于 [`Profile::space_em`] 倍字号：是字距微调或紧排，不补；
/// 4. 其余情况补一个空格——宁可多一个空格，也不能让英文单词粘连。
pub(crate) fn needs_space(
    left: &PlacedWord,
    right: &PlacedWord,
    em: f32,
    profile: &Profile,
) -> bool {
    let left_wide = left.text.chars().last().is_some_and(is_wide_char);
    let right_wide = right.text.chars().next().is_some_and(is_wide_char);
    if left_wide && right_wide {
        return false;
    }
    let gap = right.x0 - left.x1;
    // 数字 token 被拆开（`117.` + `95%`、`1` + `31.30%`、`77.6` + `6%`）时，
    // 拼接不需要空格：一个数值内部不可能有词间空格，而间隙小到触发拆分的场合
    // 必然是同一 token 的碎片。
    if is_numeric_token(&left.text) && is_numeric_token(&right.text) {
        // 只重连「被拆开的同一数值」：拆分处的间隙不足十分之一个字宽
        // （实测 demo2 页 194：`117.`+`95%` 间隙 0.13pt），而真正相邻的两个
        // 数值间隙 ≥ 0.25 em。用 0.1 em 作分界，避免把两个独立数值粘成一个。
        return gap >= 0.1 * em;
    }
    if left.sequence != right.sequence && gap >= 0.0 {
        // 例外：同一个词被拆到两个段（两端对齐时常见，如 `standa`+`rd`）。
        // 两侧都是小写字母且间隙小于一个空格宽时按续词处理。
        if gap < profile.space_em * em
            && left
                .text
                .chars()
                .last()
                .is_some_and(|c| c.is_ascii_lowercase())
            && right
                .text
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase())
        {
            return false;
        }
        return true;
    }
    gap >= profile.space_em * em
}

/// 文本是否是纯数值 token（数字、千分位、小数点、百分号、正负号、区间符）。
pub(crate) fn is_numeric_token(text: &str) -> bool {
    let mut has_digit = false;
    for c in text.chars() {
        match c {
            '0'..='9' => has_digit = true,
            '.' | ',' | '%' | '-' | '+' | '−' | '–' | '/' | '(' | ')' | '=' | ' ' => {}
            _ => return false,
        }
    }
    has_digit
}

/// 把一行词按 x 升序拼接为文本。
///
/// 词间是否补空格由 [`needs_space`] 决定；调用方负责保证传入顺序。
pub(crate) fn join_words(words: &[&PlacedWord], em: f32, profile: &Profile) -> String {
    let mut text = String::new();
    let mut prev: Option<&PlacedWord> = None;
    for word in words {
        if let Some(left) = prev {
            if needs_space(left, word, em, profile) {
                text.push(' ');
            }
        }
        text.push_str(&word.text);
        prev = Some(word);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(text: &str, x0: f32, y0: f32, x1: f32, y1: f32, fs: f32) -> RawWord {
        RawWord {
            text: text.to_string(),
            x0,
            y0,
            x1,
            y1,
            font_size: fs,
            sequence: 0,
        }
    }

    /// 高度超过行盒上限时，保留下沿（y1）、按字号重算：旋转页的高度污染由此消解。
    #[test]
    fn oversized_height_is_rebuilt_from_font_metrics() {
        let profile = Profile::default();
        // 5 个汉字 @10.5pt 被库写成 47.8 × 53.2 的方块。
        let words = normalize(
            vec![raw("资产类别:", 108.8, 386.4, 156.7, 439.7, 10.5)],
            &profile,
        );
        assert_eq!(words.len(), 1);
        let w = &words[0];
        assert_eq!(w.y1, 439.7, "下沿必须保持不动");
        assert!((w.height() - 1.3 * 10.5).abs() < 0.01, "高度改由字号推导");
    }

    /// 高度在行盒上限内的词不做任何改动，避免影响正常页。
    #[test]
    fn sane_height_is_kept() {
        let profile = Profile::default();
        let words = normalize(
            vec![raw("信用评级", 76.6, 608.5, 119.1, 619.0, 10.5)],
            &profile,
        );
        assert_eq!(words[0].y0, 608.5);
        assert_eq!(words[0].y1, 619.0);
    }

    /// 空词与退化区间被丢弃，避免零宽词进入聚类。
    #[test]
    fn empty_and_degenerate_words_are_dropped() {
        let profile = Profile::default();
        let words = normalize(
            vec![
                raw("   ", 0.0, 0.0, 10.0, 10.0, 10.0),
                raw("x", 10.0, 0.0, 10.0, 10.0, 10.0),
                raw("ok", 20.0, 0.0, 30.0, 10.0, 10.0),
            ],
            &profile,
        );
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].text, "ok");
    }

    /// 中文之间不补空格；中英之间按间隙补；英文词间按间隙补。
    #[test]
    fn spacing_follows_typography_not_distance_alone() {
        let p = Profile::default();
        let em = 12.0;
        let w = |text: &str, x0: f32, x1: f32, seq: usize| PlacedWord {
            text: text.to_string(),
            x0,
            y0: 600.0,
            x1,
            y1: 610.0,
            font_size: em,
            sequence: seq,
        };
        // 中文：即使间隙达到 0.5 em 也不补（没有词间分隔符）。
        assert!(!needs_space(
            &w("的", 0.0, 6.0, 1),
            &w("值", 12.0, 18.0, 1),
            em,
            &p
        ));
        // 中英：0.5 em 补。
        assert!(needs_space(
            &w("的", 0.0, 12.0, 1),
            &w("VaR", 18.0, 40.0, 1),
            em,
            &p
        ));
        // 英文：0.33 em 补（demo.pdf fs=12 实测 3.9~4.8pt 的词间空格）。
        assert!(needs_space(
            &w("have", 0.0, 25.0, 1),
            &w("multiple", 29.0, 70.0, 1),
            em,
            &p
        ));
        // 紧排且同段：小于 0.25 em 不补。
        assert!(!needs_space(
            &w("have", 0.0, 25.0, 1),
            &w("multiple", 27.0, 70.0, 1),
            em,
            &p
        ));
        // 紧排但跨段：不同绘制段必须分开（图示标签框的典型情形）。
        assert!(needs_space(
            &w("Rule", 0.0, 13.0, 1),
            &w("Memory", 15.0, 38.0, 2),
            em,
            &p
        ));
        // 重叠运行（两端对齐）：即使跨段也不补。
        assert!(!needs_space(
            &w("的", 0.0, 12.0, 1),
            &w("值", 10.0, 22.0, 2),
            em,
            &p
        ));
    }

    /// CJK 按 2 列计宽，ASCII 按 1 列。
    #[test]
    fn display_width_counts_wide_chars_as_two() {
        assert_eq!(display_width("区域"), 4);
        assert_eq!(display_width("0-19"), 4);
        assert_eq!(display_width("200 及200以上"), 13);
    }

    /// 中位数对奇偶长度都正确。
    #[test]
    fn median_handles_even_and_odd_lengths() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&mut [4.0, 1.0, 3.0, 2.0]), 2.5);
        assert_eq!(median(&mut []), 0.0);
    }
}
