//! 行聚类与区块切分。
//!
//! # 行
//!
//! 同一行的充要几何条件是「垂直投影重叠」：两词的词带在 y 方向有交叠。判定用
//! 重叠比例而非中心距，是为了容忍列间基线偏移与上标——上标（脚注编号）与正文
//! 同行时中心可偏移数 point，但词带必然重叠。
//!
//! # 区块
//!
//! 区块切分不引入「段落间距」这类需要调参的概念，改用**行内列间空白**区分两类行：
//!
//! - 表格行：行内存在 ≥ [`Profile::column_separation_em`] 倍字号的间隙（多列结构）；
//! - 正文行：行内所有间隙都小于该值（连续行文）。
//!
//! 连续的表格行构成一个候选区块；正文行是天然的分隔符。这样「两张紧邻但列数不同
//! 的表」会被表头之间的正文行自然切开，不需要再判断「列锚点是否突变」。

use std::ops::Range;

use super::profile::Profile;
use super::word::PlacedWord;

/// 聚类后的一行。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    /// 行内词，按 x 升序。
    pub words: Vec<PlacedWord>,
    /// 行带下沿（行内最小 y0）。
    pub y0: f32,
    /// 行带上沿（行内最大 y1）。
    pub y1: f32,
}

impl Row {
    /// 行内相邻词的最大间隙；单词行为 0。
    pub fn max_gap(&self) -> f32 {
        self.words
            .windows(2)
            .map(|pair| pair[1].x0 - pair[0].x1)
            .fold(0.0, f32::max)
    }
}

/// 把一页的词聚类为行，按页面从上到下排序。
///
/// 贪心分配：按 y1 从大到小（页面自上而下）扫描，词与已有行带的垂直重叠达到
/// [`Profile::row_overlap`] 倍较小高度时并入该行，否则开新行；行带随并入扩张。
/// 从最近建立的行往回找，是因为相邻行在 y 上必然最接近。
pub(crate) fn group_rows(mut words: Vec<PlacedWord>, profile: &Profile) -> Vec<Row> {
    words.sort_by(|a, b| {
        b.y1.partial_cmp(&a.y1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut rows: Vec<Row> = Vec::new();
    for word in words {
        let mut hit: Option<usize> = None;
        for index in (0..rows.len()).rev() {
            let row = &rows[index];
            let overlap = row.y1.min(word.y1) - row.y0.max(word.y0);
            let min_height = (row.y1 - row.y0).min(word.height());
            if overlap >= profile.row_overlap * min_height {
                hit = Some(index);
                break;
            }
        }
        match hit {
            Some(index) => {
                let row = &mut rows[index];
                row.y0 = row.y0.min(word.y0);
                row.y1 = row.y1.max(word.y1);
                row.words.push(word);
            }
            None => rows.push(Row {
                y0: word.y0,
                y1: word.y1,
                words: vec![word],
            }),
        }
    }

    for row in &mut rows {
        row.words
            .sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
        row.y0 = row.words.iter().map(|w| w.y0).fold(f32::INFINITY, f32::min);
        row.y1 = row
            .words
            .iter()
            .map(|w| w.y1)
            .fold(f32::NEG_INFINITY, f32::max);
    }
    rows.sort_by(|a, b| b.y1.partial_cmp(&a.y1).unwrap_or(std::cmp::Ordering::Equal));
    rows
}

/// 页面字号的代表值：全页词字号的中位数。
///
/// 用中位数而非均值：脚注、页码、上标会把均值拉偏。
pub(crate) fn page_em(rows: &[Row]) -> f32 {
    let mut sizes: Vec<f32> = rows
        .iter()
        .flat_map(|r| r.words.iter().map(|w| w.font_size))
        .collect();
    super::word::median(&mut sizes)
}

/// 把行序列切成「候选表格区块」与「正文区块」，保持页面顺序。
///
/// 每个返回区间要么是连续的表格行（行内存在列间空白），要么是连续的正文行。
/// 单行区间仍会返回，由调用方按原样输出——不做任何取舍。
pub(crate) fn segments(rows: &[Row], em: f32, profile: &Profile) -> Vec<Range<usize>> {
    let threshold = profile.column_separation_em * em;
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut cursor = 0;
    while cursor < rows.len() {
        let columnar = is_columnar_row(&rows[cursor], threshold);
        let start = cursor;
        while cursor < rows.len() && is_columnar_row(&rows[cursor], threshold) == columnar {
            cursor += 1;
        }
        out.push(start..cursor);
    }
    out
}

/// 行是否可能属于多列结构：行内有列间空白，且不是目录行。
fn is_columnar_row(row: &Row, threshold: f32) -> bool {
    row.max_gap() >= threshold && !is_leader_row(row)
}

/// 目录行判定：行内存在引导线词。
///
/// 引导线（dot leader）是排版填充物，用来把条目标题与页码连起来，不是单元格内容：
/// 目录的「列」是视觉连接效果，不是数据的行列关系。带引导线的行因此按正文输出，
/// 页面结构留给基线文本——目录本来就没有需要还原的表格。
pub(crate) fn is_leader_row(row: &Row) -> bool {
    row.words.iter().any(|word| has_leader_run(&word.text))
}

/// 文本中是否含连续 ≥4 个引导线字符。
///
/// 取 4 而不是 3：引导线是成串的，而正文里的省略号、句点组合通常不超过 3 个
/// （如「...」表示省略）。词法上不需要「整词皆为点」，因为 PDF 常把标题与引导线
/// 合成为一个词（实测 `Acknowledgements..........`）。
fn has_leader_run(text: &str) -> bool {
    let mut run = 0;
    for c in text.chars() {
        if matches!(c, '.' | '·' | '‥' | '…' | '_') {
            run += 1;
            if run >= 4 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x0: f32, y0: f32, x1: f32, y1: f32) -> PlacedWord {
        PlacedWord {
            text: text.to_string(),
            x0,
            y0,
            x1,
            y1,
            font_size: 10.0,
            sequence: 0,
        }
    }

    /// 上标（字号更小、中心偏移）仍应与正文同行。
    #[test]
    fn superscript_joins_its_line() {
        let profile = Profile::default();
        let words = vec![
            word("行账户资本要求的规定是一致的", 70.0, 709.9, 238.9, 721.9),
            word("96", 238.9, 713.9, 246.9, 721.9),
        ];
        let rows = group_rows(words, &profile);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].words.len(), 2);
    }

    /// 上下两行互不重叠时不得合并。
    #[test]
    fn distinct_lines_stay_separate() {
        let profile = Profile::default();
        let words = vec![
            word("上一行", 70.0, 700.0, 120.0, 712.0),
            word("下一行", 70.0, 680.0, 120.0, 692.0),
        ];
        assert_eq!(group_rows(words, &profile).len(), 2);
    }

    /// 行内大间隙（列间空白）与行内小间隙（正文）分别切出表格区块与正文区块。
    #[test]
    fn segments_separate_columnar_rows_from_prose() {
        let profile = Profile::default();
        let rows = vec![
            Row {
                y0: 690.0,
                y1: 700.0,
                words: vec![
                    word("甲", 100.0, 690.0, 110.0, 700.0),
                    word("乙", 300.0, 690.0, 310.0, 700.0),
                ],
            },
            Row {
                y0: 670.0,
                y1: 680.0,
                words: vec![
                    word("一", 100.0, 670.0, 110.0, 680.0),
                    word("二", 300.0, 670.0, 310.0, 680.0),
                ],
            },
            Row {
                y0: 650.0,
                y1: 660.0,
                words: vec![
                    word("正文", 100.0, 650.0, 160.0, 660.0),
                    word("连续", 164.0, 650.0, 220.0, 660.0),
                ],
            },
        ];
        let segments = segments(&rows, 10.0, &profile);
        assert_eq!(segments, vec![0..2, 2..3]);
    }

    /// 带引导线的目录行不参与表格判定，整行按正文输出。
    #[test]
    fn leader_rows_are_not_columnar() {
        let profile = Profile::default();
        let rows = vec![
            Row {
                y0: 690.0,
                y1: 700.0,
                words: vec![
                    word("3．计值调整", 70.0, 690.0, 160.0, 700.0),
                    word(".......................", 160.0, 690.0, 400.0, 700.0),
                    word("150", 480.0, 690.0, 500.0, 700.0),
                ],
            },
            Row {
                y0: 670.0,
                y1: 680.0,
                words: vec![
                    word("C.交易账户", 70.0, 670.0, 160.0, 680.0),
                    word("......................", 160.0, 670.0, 400.0, 680.0),
                    word("151", 480.0, 670.0, 500.0, 680.0),
                ],
            },
        ];
        assert_eq!(segments(&rows, 10.0, &profile), vec![0..2]);
    }
}
