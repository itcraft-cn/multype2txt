//! PDF 表格重建：基于词坐标的行列聚类，把无边框表格还原为对齐的纯文本网格。
//!
//! # 为什么自研
//!
//! `extract_all_text` 产出扁平文本流，词的坐标被丢弃，无边框表格（如 Basel
//! 文档的密集数字矩阵）的行列结构无从还原，表现为逐词流水或行列错乱。底层
//! 库的 `extract_tables()` 是黑盒启发式，行为随版本漂移且难以针对特定版式调优；
//! 本模块反过来只消费最底层、最可靠的词包围盒（`pdf_oxide::layout::Word`），
//! 聚类逻辑全部在自己手里，可测、可调、可控。
//!
//! # 算法
//!
//! 1. **行聚类**：按 y 区间重叠归并——词与行带的垂直重叠 ≥ 两者较小高度的
//!    20% 即属同一行。比「y 中心容差」更能兼容上标/下标（脚注编号、公式下标
//!    与正文同行但中心偏移可达数个 point）与列间基线错位。
//! 2. **表格行判定**：行内存在相邻词间隙 ≥ [`GAP_FOR_TABLE`]（正文行内间隙
//!    仅 3~10pt），且与前一表格行存在列对齐（x0/中心/x1 任一容差
//!    [`COL_TOL`] 内命中，兼容左对齐文本列、右对齐数字列、居中列）。
//! 3. **表格区域**：连续 ≥ [`MIN_TABLE_ROWS`] 行成链才算表格——正文首行缩进
//!    的孤立行、两行定义式不会误判。
//! 4. **列槽位**：区域内所有词的 x 区间做合并（重叠或间隙 < [`SLOT_MERGE_GAP`]
//!    归并），每个合并区间是一个列槽位；词按中心落入槽位完成网格填充。
//!    多词单元格（「A+ 至 BBB-」）与漂移的右对齐数字列自然归并。
//! 5. **渲染**：按显示宽度（CJK=2 列）计算列宽对齐；整列皆为数字/百分比时
//!    右对齐，其余左对齐。跨行合并单元格（绿区/黄区/红区）表现为下方行的
//!    空单元格——纯文本无法表达 rowspan，如实留空而不是猜填。词间空格由
//!    [`needs_space`] 按「间隙/字高比 + 边界字符类型」判定，中英皆准。
//!
//! # 边界
//!
//! - 本模块只负责「检出表格的页」的重建；无表格的页由调用方沿用
//!   `extract_all_text` 原文，保证非表格内容零回归。
//! - 不处理跨页表格拼接：每页独立重建。跨页表在页边界处自然断开，两页各自
//!   对齐，信息不丢（拼接属推断，违反「只还原，不推断」）。
//! - 满宽合并表头（单行无间隙）会打断表格链，链被切成两个区域分别渲染；
//!   两段仍各自对齐，只是中间多一个空行，可接受。

/// 行内相邻词间隙达到该值（PDF point）才认为本行可能是表格行。
///
/// 正文行内的词间隙（含中英混排空格）实测 3~10pt；表格的列间隙通常 >30pt。
/// 取 12pt 留出安全边际：既能抓住窄列小表，又不把正文行误判为表格。
const GAP_FOR_TABLE: f32 = 12.0;

/// 列对齐容差（point）。兼容右对齐数字列的 x0 漂移（同一列内 x0 可差 15pt
/// 以上）与上标导致的轻微偏移；超过该值视为不同列。
const COL_TOL: f32 = 8.0;

/// 列槽位归并阈值（point）：词 x 区间间隙小于该值归并进同一列槽位。
///
/// 必须大于单元格内多词间隙（实测最大 5.3pt）——否则「200 及 200 以上」
/// 会被拆成两列；又必须远小于真实列间隙（>30pt）。取 8pt 与列对齐容差一致。
const SLOT_MERGE_GAP: f32 = 8.0;

/// 连续多少行满足表格行条件才认定为表格区域。≥3 可排除正文首行缩进、
/// 两行定义式（如「K = 用标准法计算的资本要求」的公式+释义对）。
const MIN_TABLE_ROWS: usize = 3;

/// 已定位的词：坐标全部来自底层包围盒。
///
/// PDF 坐标 y 轴向上，`y0`（Rect.y）为下沿、`y1` 为上沿；聚类只关心相对
/// 关系，本模块统一沿用该坐标系，不做翻转。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlacedWord {
    pub text: String,
    /// 词左边界
    pub x0: f32,
    /// 词下边界（PDF y 轴向上，值越大越靠页面上方）
    pub y0: f32,
    /// 词右边界
    pub x1: f32,
    /// 词上边界
    pub y1: f32,
}

impl PlacedWord {
    /// 构造一个词。
    pub(crate) fn new(text: impl Into<String>, x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self {
            text: text.into(),
            x0,
            y0,
            x1,
            y1,
        }
    }

    /// 水平中心，用于列对齐判定（右对齐列的 x0 漂移但中心相对稳定）。
    fn center_x(&self) -> f32 {
        (self.x0 + self.x1) / 2.0
    }

    /// 词带高度。
    fn height(&self) -> f32 {
        self.y1 - self.y0
    }
}

/// 聚类后的一行词（按 x 升序）。
#[derive(Debug)]
struct Row {
    words: Vec<PlacedWord>,
    /// 行带下沿（PDF y 轴向上，为行内最小 y0）
    band_y0: f32,
    /// 行带上沿（行内最大 y1）
    band_y1: f32,
}

impl Row {
    /// 行带与给定词的垂直重叠高度。
    fn vertical_overlap(&self, word: &PlacedWord) -> f32 {
        self.band_y1.min(word.y1) - self.band_y0.max(word.y0)
    }

    /// 行内相邻词的最大间隙；单词行为 0（正文短行不会误判为表格行）。
    fn max_gap(&self) -> f32 {
        self.words
            .windows(2)
            .map(|pair| pair[1].x0 - pair[0].x1)
            .fold(0.0, f32::max)
    }

    /// 本行是否存在与前一行对齐的列（x0、中心、右边界任一在容差内即算）。
    fn aligns_with(&self, prev: &Row) -> bool {
        self.words.iter().any(|w| {
            prev.words.iter().any(|p| {
                (w.x0 - p.x0).abs() <= COL_TOL
                    || (w.center_x() - p.center_x()).abs() <= COL_TOL
                    || (w.x1 - p.x1).abs() <= COL_TOL
            })
        })
    }
}

/// 跨列标签行/单列续行与上一行的垂直间距上限（point，词带之间的空白）。
///
/// 表格内的行距实测 15~26pt（demo2.pdf 页 156：16.4~25.5pt）；正文段落之间的
/// 空白显著更大（页 58 表格与下一段「152.」之间 44.5pt）。不加此约束时，
/// 横跨全页的公式行（词多且散，与表格列易有多点巧合对齐）会把紧随其后的
/// 正文行经由跨列标签判定卷进表格链，公式词再桥接槽位毁掉整个网格。
const ROW_PROXIMITY: f32 = 25.0;

/// 两行之间的垂直空白（PDF y 轴向上：上行带下沿 − 下行带上沿；重叠时为 0）。
fn vertical_gap(above: &Row, below: &Row) -> f32 {
    (above.band_y0 - below.band_y1).max(0.0)
}

/// 重叠比例阈值：20% 是「同行」与「上下行」的分界——相邻正文行/表格行的
/// 词带重叠恒为 0（行距 > 词高），同行词带必有重叠。取低值是为了兼容列间
/// 基线偏移：demo2.pdf 中「不合格参照资产」与其数值列（10%）垂直错位 7.4pt，
/// 重叠仅 30%，50% 阈值会把同一行拆成两行。
const OVERLAP_RATIO: f32 = 0.2;

/// 把一页的词聚类为行（页面从上到下排序）。
///
/// 贪心分配：按 y0 从大到小（页面自上而下）扫描，词与已有行带的垂直重叠
/// ≥ 两者较小高度的 [`OVERLAP_RATIO`] 时并入该行，否则开新行；行带随并入
/// 扩张。上标（脚注编号 fs=7 与正文 fs=12 同行）靠重叠判定正确归行。
fn group_rows(mut words: Vec<PlacedWord>) -> Vec<Row> {
    // y0 降序 = 页面从上到下。
    words.sort_by(|a, b| {
        b.y0.partial_cmp(&a.y0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut rows: Vec<Row> = Vec::new();
    for word in words {
        // 从最近添加（最靠近当前词所在水平带）的行开始找，命中即止。
        let candidate = rows.iter_mut().rev().find(|row| {
            let overlap = row.vertical_overlap(&word);
            let min_height = (row.band_y1 - row.band_y0).min(word.height());
            overlap >= OVERLAP_RATIO * min_height
        });
        match candidate {
            Some(row) => {
                row.band_y0 = row.band_y0.min(word.y0);
                row.band_y1 = row.band_y1.max(word.y1);
                row.words.push(word);
            }
            None => rows.push(Row {
                band_y0: word.y0,
                band_y1: word.y1,
                words: vec![word],
            }),
        }
    }

    for row in &mut rows {
        row.words
            .sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
    }
    rows
}

/// 检出一页中的表格区域，返回行下标区间（`[start, end)`，页面从上到下）。
///
/// 判定链，三类行可续链：
///
/// - **多列行**：行内有列间隙（≥ [`GAP_FOR_TABLE`]），且与前行存在列对齐；
/// - **单列续行**：行内紧凑（无大间隙，是某单元格内容的换行），x 起点在
///   表格区域内（≥ 区域最小 x0 + [`COL_TOL`]，排除版心左缘起排的正文续行），
///   且与前行对齐——如「1%(剩余期限在6个月至24个月)」这类只有第二列内容
///   的行，丢出网格就错位了；
/// - **跨列标签行**：行内紧凑但下一行是与链内任一行对齐的多列行——如
///   「总收益率互换(total return swap)」这种横跨整表的分组标签，靠前瞻的
///   下一行确认它身处表格之中。
///
/// 连续满足 ≥ [`MIN_TABLE_ROWS`] 行才成区域；链首行只需自身有列间隙
/// （上方没有同行可对齐）。
fn detect_table_regions(rows: &[Row]) -> Vec<std::ops::Range<usize>> {
    let mut regions: Vec<std::ops::Range<usize>> = Vec::new();
    let mut chain_start: Option<usize> = None;

    for i in 0..rows.len() {
        let chainable = match chain_start {
            None => rows[i].max_gap() >= GAP_FOR_TABLE,
            Some(start) => {
                let region_x0 = region_min_x0(&rows[start..i]);
                if rows[i].max_gap() >= GAP_FOR_TABLE {
                    // 多列行：通常与紧邻前行对齐；若前行是单标签行
                    // （跨列表题），跳过它与标签前的网格行对齐——两张列锚点
                    // 连续的表（如长期评级/短期评级）隔着一行列表题仍是同一
                    // 版式块，切开会让后一张表掉出网格。
                    rows[i].aligns_with(&rows[i - 1])
                        || (rows[i - 1].words.len() == 1
                            && i >= 2
                            && rows[i].aligns_with(&rows[i - 2]))
                } else if is_single_column_continuation(&rows[i], region_x0) {
                    // 单列续行必须紧贴上一行（表格内行距 ≤ [`ROW_PROXIMITY`]）
                    // 且对齐——版心左缘外的窄正文行（如右侧边注）不满足。
                    vertical_gap(&rows[i - 1], &rows[i]) <= ROW_PROXIMITY
                        && rows[i].aligns_with(&rows[i - 1])
                } else {
                    is_spanning_label_row(rows, i, start)
                }
            }
        };

        if chainable {
            chain_start.get_or_insert(i);
        } else if let Some(start) = chain_start.take() {
            if i - start >= MIN_TABLE_ROWS {
                regions.push(start..i);
            }
        }
    }
    if let Some(start) = chain_start {
        if rows.len() - start >= MIN_TABLE_ROWS {
            regions.push(start..rows.len());
        }
    }
    regions
}

/// 跨列标签行判定：本行紧贴链尾（垂直间距 ≤ [`ROW_PROXIMITY`]），且下一行
/// 也是紧贴本行的多列行，并与链内任一行对齐。
///
/// 标签行自身横跨整表、不与任何单列对齐，只能靠「下一行仍在表格网格中」
/// 反证它属于表格。垂直邻近约束排除正文段落的紧凑行——公式行词多且散，
/// 与表格列易有多点巧合对齐，若无垂直约束会把表格与后续正文连成一个区域，
/// 公式词随后桥接列槽位毁掉整个网格（demo2.pdf 页 58 实测回归）。
fn is_spanning_label_row(rows: &[Row], i: usize, chain_start: usize) -> bool {
    let Some(next) = rows.get(i + 1) else {
        return false;
    };
    next.max_gap() >= GAP_FOR_TABLE
        && vertical_gap(&rows[i - 1], &rows[i]) <= ROW_PROXIMITY
        && vertical_gap(&rows[i], next) <= ROW_PROXIMITY
        && rows[chain_start..i].iter().any(|row| next.aligns_with(row))
}

/// 表格区域内的最小词左边界（链首行到当前行之前的全部词）。
fn region_min_x0(rows: &[Row]) -> f32 {
    rows.iter()
        .flat_map(|r| r.words.iter())
        .map(|w| w.x0)
        .fold(f32::INFINITY, f32::min)
}

/// 单列续行判定：行内词紧凑（无表格级大间隙）且不从版心左缘起排。
///
/// 正文续行通常与上一行同起于版心左缘（表格区域最小 x0 附近或更左），
/// 而单元格续行的 x 起点落在表格内部列上——用「≥ 区域最小 x0 + [`COL_TOL`]」
/// 把正文短行挡在链外。
fn is_single_column_continuation(row: &Row, region_x0: f32) -> bool {
    row.words
        .first()
        .is_some_and(|w| w.x0 >= region_x0 + COL_TOL)
}

/// 一个列槽位：区域内词 x 区间合并后的结果。
struct ColumnSlot {
    x0: f32,
    x1: f32,
}

/// 把区域内的词 x 区间合并为列槽位（按 x0 升序扫描归并）。
///
/// 重叠或间隙 < [`SLOT_MERGE_GAP`] 的区间归并：数字列（0-19 / 100-119 / 200 及
/// 200 以上，右对齐导致 x0 从 246 到 279 漂移）归并为同一槽位；真正的列间隙
/// （>30pt）不会被误并。
fn build_column_slots(words: &[&PlacedWord]) -> Vec<ColumnSlot> {
    let mut intervals: Vec<(f32, f32)> = words.iter().map(|w| (w.x0, w.x1)).collect();
    intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut slots: Vec<ColumnSlot> = Vec::new();
    for (x0, x1) in intervals {
        match slots.last_mut() {
            Some(last) if x0 - last.x1 <= SLOT_MERGE_GAP => {
                last.x1 = last.x1.max(x1);
            }
            _ => slots.push(ColumnSlot { x0, x1 }),
        }
    }
    slots
}

/// 判断一列是否全是数字型内容（数字、百分比、区间、比较符），决定右对齐。
///
/// 「0-19」「18%」「(=1)」「200 及 200 以上」「无 (=1)」都算数字型——靠
/// 「整列皆匹配」而非单格判定降低误伤；含中文描述的文本列不会命中。
fn is_numeric_column(cells: &[String]) -> bool {
    let non_empty: Vec<&String> = cells.iter().filter(|c| !c.trim().is_empty()).collect();
    !non_empty.is_empty()
        && non_empty.iter().all(|cell| {
            cell.chars().all(|c| {
                c.is_ascii_digit()
                    || matches!(
                        c,
                        ' ' | '.'
                            | ','
                            | '%'
                            | '-'
                            | '−'
                            | '('
                            | ')'
                            | '='
                            | '+'
                            | '至'
                            | '及'
                            | '以'
                            | '上'
                            | '无'
                    )
            })
        })
}

/// 找出词应归属的列槽位：中心落在槽位内直接命中，否则取中心最近的槽位
/// （多词单元格归并后的边界情形）。
fn slot_index_for(slots: &[ColumnSlot], word: &PlacedWord) -> usize {
    let center = word.center_x();
    slots
        .iter()
        .position(|slot| center >= slot.x0 && center <= slot.x1)
        .unwrap_or_else(|| {
            slots
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    let da = (center - (a.x0 + a.x1) / 2.0).abs();
                    let db = (center - (b.x0 + b.x1) / 2.0).abs();
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0)
        })
}

/// 渲染表格区域为对齐的纯文本行。
///
/// 每行 = 各列槽位内的词按 x 拼接（[`needs_space`] 决定词间空格）后按
/// 列宽对齐。数值列右对齐，文本列左对齐；空单元格如实留空（跨行合并的
/// rowspan 表现为空白）。
fn render_region(rows: &[Row], region: &std::ops::Range<usize>) -> Vec<String> {
    let region_rows = &rows[region.clone()];
    // 槽位塑形只用多列行（≥2 词）：跨列标签行（「短期评级种类」居中横跨
    // 整表）与单列续行的词区间会桥接相邻列槽位，把两列并成一列导致网格
    // 错位；它们仍按中心归属到多列行塑形出的槽位中。
    let shape_words: Vec<&PlacedWord> = region_rows
        .iter()
        .filter(|r| r.words.len() >= 2)
        .flat_map(|r| r.words.iter())
        .collect();
    let fallback_words: Vec<&PlacedWord> =
        region_rows.iter().flat_map(|r| r.words.iter()).collect();
    let shape_refs: Vec<&PlacedWord> = if shape_words.is_empty() {
        fallback_words
    } else {
        shape_words
    };
    let slots = build_column_slots(&shape_refs);

    // 网格填充：每个 (行, 列) 的单元格文本；列内按 x 升序拼接词。
    let mut grid: Vec<Vec<String>> = Vec::with_capacity(region_rows.len());
    for row in region_rows {
        let mut cells: Vec<String> = vec![String::new(); slots.len()];
        // 记录每个单元格最后追加的词，用于按间隙+字符类型决定是否补空格。
        let mut last_word: Vec<Option<&PlacedWord>> = vec![None; slots.len()];
        for word in &row.words {
            let col = slot_index_for(&slots, word);
            if let Some(left) = last_word[col] {
                let gap = word.x0 - left.x1;
                let height = left.height().max(word.height());
                if needs_space(&left.text, &word.text, gap, height) {
                    cells[col].push(' ');
                }
            }
            cells[col].push_str(&word.text);
            last_word[col] = Some(word);
        }
        grid.push(cells);
    }

    // 列对齐方式与宽度：数值列右对齐，其余左对齐；宽度按显示宽度取最大。
    let numeric: Vec<bool> = (0..slots.len())
        .map(|col| {
            let column_cells: Vec<String> = grid.iter().map(|cells| cells[col].clone()).collect();
            is_numeric_column(&column_cells)
        })
        .collect();
    let widths: Vec<usize> = (0..slots.len())
        .map(|col| {
            grid.iter()
                .map(|cells| display_width(cells[col].trim()))
                .fold(0, usize::max)
        })
        .collect();

    grid.iter()
        .map(|cells| {
            let mut line = String::new();
            for (col, cell) in cells.iter().enumerate() {
                if col > 0 {
                    line.push_str("  ");
                }
                let text = cell.trim();
                let pad = widths[col].saturating_sub(display_width(text));
                if numeric[col] {
                    line.push_str(&" ".repeat(pad));
                }
                line.push_str(text);
                if !numeric[col] {
                    line.push_str(&" ".repeat(pad));
                }
            }
            line.trim_end().to_string()
        })
        .collect()
}

/// 重建一页文本：检出表格则整页按坐标重建，否则返回 `None`（调用方沿用
/// `extract_all_text` 原文，保证非表格页零回归）。
///
/// 非表格行按词间隙拼接（间隙 ≥ [`CELL_GAP`] 补一个空格），表格区域按列
/// 槽位对齐渲染，区域前后各留一个空行。
pub(crate) fn rebuild_page(words: Vec<PlacedWord>) -> Option<String> {
    if words.is_empty() {
        return None;
    }
    let rows = group_rows(words);
    let regions = detect_table_regions(&rows);
    if regions.is_empty() {
        return None;
    }

    let mut lines: Vec<String> = Vec::new();
    let mut row_index = 0;
    for region in regions {
        // 表格前的普通行。
        while row_index < region.start {
            lines.push(render_prose_row(&rows[row_index]));
            row_index += 1;
        }
        if !lines.is_empty() && !lines.last().is_some_and(|l| l.is_empty()) {
            lines.push(String::new());
        }
        lines.extend(render_region(&rows, &region));
        lines.push(String::new());
        row_index = region.end;
    }
    while row_index < rows.len() {
        lines.push(render_prose_row(&rows[row_index]));
        row_index += 1;
    }

    // 去掉首尾空行；表格前后已自然留有空行。
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    Some(lines.join("\n"))
}

/// 词间隙与字高的比值低于该值视为「词内/紧排」，不补空格。
///
/// 实测依据：CJK 词内间隙 0.22~0.26 倍字高（demo2.pdf：2.7~3.0pt @fs10.5/12），
/// 英文词间空格约 0.28 倍字高（demo.pdf @fs12: 3.9~4.8pt；@fs9 脚注: ~2.5pt）。
/// 取 0.26 作为分界——纯距离阈值在中英之间没有安全边界（英文小字的空格比
/// CJK 大字的词内间隙还小），必须按字高归一化。
const GAP_RATIO_TIGHT: f32 = 0.26;

/// 词拼接时判断是否补一个空格。
///
/// 三段式判定：
///
/// 1. 间隙 < 0.26×字高：词内切分残片或紧排（CJK 词间、数字与 CJK 紧贴、
///    下标紧贴基字），不补；
/// 2. 其余情况补空格——英文词粘连（`havemultipleinterfaces`）比多出空格
///    严重得多；
/// 3. **例外**：两侧边界字符都是 CJK 时不补——中文词间无空格是排版惯例，
///    CJK 之间的大间隙（≥0.26 倍字高）多为标点或版式留白，补空格反而噪
///    声。真实中英混排空格（0.5 倍字高以上）仍会补上。
fn needs_space(left_text: &str, right_text: &str, gap: f32, font_height: f32) -> bool {
    const MIN_HEIGHT: f32 = 1.0;
    if gap < GAP_RATIO_TIGHT * font_height.max(MIN_HEIGHT) {
        return false;
    }
    let left_cjk = left_text.chars().last().is_some_and(is_wide_char);
    let right_cjk = right_text.chars().next().is_some_and(is_wide_char);
    !(left_cjk && right_cjk)
}

/// 渲染一个非表格行：词按 x 升序拼接，按 [`needs_space`] 决定词间空格。
fn render_prose_row(row: &Row) -> String {
    let mut line = String::new();
    let mut prev: Option<&PlacedWord> = None;
    for word in &row.words {
        if let Some(left) = prev {
            let gap = word.x0 - left.x1;
            let height = left.height().max(word.height());
            if needs_space(&left.text, &word.text, gap, height) {
                line.push(' ');
            }
        }
        line.push_str(&word.text);
        prev = Some(word);
    }
    line
}

/// 字符是否为宽字符（CJK 及全角符号，显示占 2 列）。
///
/// 自实现而非引 `unicode-width` 依赖：对齐与空格判定只需覆盖中日韩宽字符
/// 的主要码位区间，覆盖不全的后果只是对齐偏差一列，不影响数据正确性。
fn is_wide_char(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        0x1100..=0x115F   // 韩文字母
        | 0x2E80..=0x303E // CJK 部首、假名、标点
        | 0x3041..=0x33FF // 假名、注音、CJK 兼容、半角全角
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

/// 字符显示宽度：宽字符按 2 列计，其余按 1 列。
fn char_width(c: char) -> usize {
    if is_wide_char(c) {
        2
    } else {
        1
    }
}

/// 字符串显示宽度（CJK=2，ASCII=1）。
fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个词：参数按「页面视觉位置」给出（y_top > y_bottom），内部转为
    /// PDF y 轴向上的坐标（y0=下沿，y1=上沿）。
    fn word(text: &str, x0: f32, y_top: f32, x1: f32, y_bottom: f32) -> PlacedWord {
        PlacedWord::new(text, x0, y_bottom, x1, y_top)
    }

    /// 三列三行的小表（含跨行合并单元格）应被检出并渲染为对齐网格。
    #[test]
    fn detects_and_renders_simple_table() {
        let words = vec![
            // 表头
            word("区域", 173.0, 698.0, 195.0, 687.0),
            word("数量", 271.0, 698.0, 302.0, 687.0),
            word("倍数", 390.0, 698.0, 420.0, 687.0),
            // 数据行 1（绿区为 rowspan，视觉上与本行同带）
            word("绿区", 174.0, 617.2, 195.0, 606.7),
            word("40-59", 273.0, 614.4, 300.0, 603.9),
            word("无", 392.0, 614.4, 403.0, 603.9),
            word("(=1)", 406.0, 614.4, 429.0, 603.9), // 词间隙 3pt（实测值）
            // 数据行 2（黄区 rowspan 中心在本行）
            word("黄区", 174.0, 475.0, 195.0, 464.5),
            word("140-159", 262.0, 472.2, 300.0, 461.7),
            word("2.4", 406.0, 472.2, 420.0, 461.7),
            // 数据行 3
            word("红区", 174.0, 389.3, 195.0, 378.8),
            word("200", 246.4, 389.3, 262.3, 378.8),
            word("及", 267.6, 389.3, 278.1, 378.8),
            word("200", 280.8, 389.3, 296.6, 378.8),
            word("以上", 299.3, 389.3, 320.4, 378.8),
            word("3.0", 406.0, 389.3, 420.0, 378.8),
        ];
        let rows = group_rows(words);
        assert_eq!(rows.len(), 4, "应聚类为 4 行（表头 + 3 数据行）");
        let regions = detect_table_regions(&rows);
        assert_eq!(regions.len(), 1, "应检出 1 个表格区域");
        assert_eq!(regions[0], 0..4);

        let rendered = render_region(&rows, &regions[0]);
        assert_eq!(rendered.len(), 4);
        // 行内容完整。
        assert!(rendered[0].contains("区域") && rendered[0].contains("数量"));
        assert!(rendered[1].contains("绿区") && rendered[1].contains("40-59"));
        // 「无 (=1)」同一单元格内：间隙 3pt 超过紧排阈值（0.26×10.5≈2.73pt）
        // 且边界字符非双 CJK，补一个空格——与原版「无 (= 1)」一致。
        assert!(rendered[1].contains("无 (=1)"), "实际: {}", rendered[1]);
        // 多词单元格「200 及200以上」归并（间隙 5.3/2.7/2.7pt：仅 5.3pt 处补空格，
        // CJK 词间 2.7pt 保持相邻，数据完整无丢失）。
        assert!(
            rendered[3].contains("200 及200以上"),
            "实际: {}",
            rendered[3]
        );
        assert!(rendered[3].contains("3.0"));

        // 对齐性：同一列在各行的起始显示列一致（数字列右对齐 => 结束位置一致）。
        let pos = |line: &str, needle: &str| display_width(&line[..line.find(needle).unwrap()]);
        assert_eq!(pos(&rendered[0], "区域"), pos(&rendered[1], "绿区"));
        assert_eq!(pos(&rendered[0], "数量"), pos(&rendered[1], "40-59"));
    }

    /// 正文行（间隙小、无列对齐链）不检出表格。
    #[test]
    fn prose_rows_are_not_tables() {
        let words = vec![
            word("151.", 70.0, 331.0, 94.0, 319.0),
            word("如果采用模型，银行则应使用", 100.0, 331.0, 400.0, 319.0),
            word("20", 403.0, 331.0, 415.0, 319.0),
            word("个交易对象的样本", 418.0, 331.0, 524.0, 319.0),
            word("测试。这些应包括银行按照", 70.0, 313.0, 300.0, 301.0),
            word("风险暴露计量确定的", 303.0, 313.0, 450.0, 301.0),
            word("10", 453.0, 313.0, 465.0, 301.0),
            word("个最大交易对象", 468.0, 313.0, 524.0, 301.0),
        ];
        let rows = group_rows(words);
        assert_eq!(rows.len(), 2);
        assert!(detect_table_regions(&rows).is_empty(), "正文不应判为表格");
    }

    /// 行内最大间隙：只有跨列大间隙才达标，正文行内小间隙不达标。
    #[test]
    fn row_gap_threshold() {
        let words = vec![
            word("年度违约概率。", 85.0, 119.0, 148.0, 110.0),
            word("1996", 258.0, 119.0, 278.0, 110.0),
        ];
        let rows = group_rows(words);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].max_gap() >= GAP_FOR_TABLE);

        // 正文词间隙（3pt）远小于阈值。
        let prose = vec![
            word("值。", 203.0, 269.0, 230.0, 257.0),
            word("前一交易日的", 70.0, 269.0, 200.0, 257.0),
        ];
        let rows = group_rows(prose);
        assert!(rows[0].max_gap() < GAP_FOR_TABLE);
    }

    /// 上标脚注编号与正文同行：垂直重叠判定应归入同一行。
    #[test]
    fn superscript_joins_body_row() {
        let words = vec![
            word("行账户资本要求的规定是一致的", 70.0, 721.9, 238.9, 709.9),
            word("96。", 238.9, 721.9, 256.9, 709.9), // fs=8 的脚注上标，词带偏小
        ];
        let rows = group_rows(words);
        assert_eq!(rows.len(), 1, "上标应与正文同行");
        assert_eq!(rows[0].words.len(), 2);
    }

    /// 右对齐数字列：x0 漂移大但右边界稳定，应归并为同一列槽位。
    #[test]
    fn right_aligned_numbers_share_slot() {
        let words = [
            word("0-19", 279.4, 670.9, 302.7, 660.4),
            word("100-119", 261.9, 528.7, 300.3, 518.2),
            word("200", 246.4, 389.3, 262.3, 378.8),
            word("及", 267.6, 389.3, 278.1, 378.8),
            word("200", 280.8, 389.3, 296.6, 378.8),
            word("以上", 299.3, 389.3, 320.4, 378.8),
        ];
        let refs: Vec<&PlacedWord> = words.iter().collect();
        let slots = build_column_slots(&refs);
        assert_eq!(slots.len(), 1, "漂移的数字列应归并为一个槽位");
        assert!(slots[0].x0 <= 246.5 && slots[0].x1 >= 320.0);
    }

    /// 数字列识别：百分比、区间、(=1) 算数字型；含中文描述的不算。
    #[test]
    fn numeric_column_detection() {
        assert!(is_numeric_column(&[
            "0-19".to_string(),
            "100-119".to_string(),
            "200 及 200 以上".to_string()
        ]));
        assert!(is_numeric_column(&[
            "无 (=1)".to_string(),
            "2.4".to_string()
        ]));
        assert!(!is_numeric_column(&[
            "外部信用评级".to_string(),
            "AAA 至 AA-".to_string()
        ]));
        assert!(!is_numeric_column(&[String::new(), String::new()]));
    }

    /// CJK 宽字符按 2 列计宽。
    #[test]
    fn display_width_counts_cjk() {
        assert_eq!(display_width("区域"), 4);
        assert_eq!(display_width("0-19"), 4);
        assert_eq!(display_width("200 及 200 以上"), 15);
    }

    /// 检不出表格时 rebuild_page 返回 None（调用方回退基线文本）。
    #[test]
    fn prose_page_returns_none() {
        let words = vec![
            word("前一交易日的", 70.0, 269.0, 200.0, 257.0),
            word("值。", 203.0, 269.0, 230.0, 257.0),
        ];
        assert!(rebuild_page(words).is_none());
    }

    /// 空页返回 None。
    #[test]
    fn empty_page_returns_none() {
        assert!(rebuild_page(vec![]).is_none());
    }

    /// 正文行拼接：CJK 小间隙不补空格，真实空格补一个。
    ///
    /// 间隙值取自 demo2.pdf 实测：CJK 词间隙 2.7~3.0pt，中英混排空格 6pt。
    #[test]
    fn prose_row_spacing() {
        let words = vec![
            word("前一交易日的", 70.0, 269.0, 200.0, 257.0),
            word("VaR", 206.0, 269.0, 228.0, 257.0), // 前间隙 6pt：补空格
            word("值。", 231.0, 269.0, 258.0, 257.0), // 前间隙 3pt：不补
        ];
        let rows = group_rows(words);
        assert_eq!(render_prose_row(&rows[0]), "前一交易日的 VaR值。");
    }

    /// 英文词间隙（0.28~0.4 倍字高）必须补空格——demo.pdf（fs=12 时间隙
    /// 3.9~4.8pt）曾因固定 4pt 阈值被粘连成 `havemultipleinterfaces`。
    ///
    /// 间隙值取自 demo.pdf 实测；紧排阈值按字高比例（0.26）而非绝对距离，
    /// 英文小字（fs=9 脚注，空格 ~2.5pt）同样能补上空格。
    #[test]
    fn english_word_spacing() {
        let words = vec![
            word("have", 86.0, 680.0, 111.8, 668.0),
            word("multiple", 115.8, 680.0, 161.6, 668.0), // 间隙 4.0pt
            word("interfaces", 165.6, 680.0, 217.5, 668.0), // 间隙 4.0pt
        ];
        let rows = group_rows(words);
        assert_eq!(render_prose_row(&rows[0]), "have multiple interfaces");

        // fs=9 脚注：英文空格缩至 ~2.5pt，按比例仍超阈值。
        let footnote = vec![
            word("see", 100.0, 119.0, 115.0, 110.0),
            word("Section", 117.5, 119.0, 150.0, 110.0), // 间隙 2.5pt @fs9
        ];
        let rows = group_rows(footnote);
        assert_eq!(render_prose_row(&rows[0]), "see Section");
    }

    /// 表格与后续正文之间隔大段空白时，正文不得并入表格链——公式行词多且
    /// 散，与表格列的多点巧合对齐曾把区域扩到公式行，公式词桥接列槽位把
    /// 整个网格毁成单列（demo2.pdf 页 58 实测回归）。
    #[test]
    fn distant_prose_row_stays_out_of_table() {
        let mut words = vec![
            // 三列小表（2 行 + 表头）
            word("区域", 173.0, 698.0, 195.0, 687.0),
            word("数量", 271.0, 698.0, 302.0, 687.0),
            word("倍数", 390.0, 698.0, 420.0, 687.0),
            word("绿区", 174.0, 617.0, 195.0, 606.0),
            word("40-59", 273.0, 614.0, 300.0, 603.0),
            word("2.0", 392.0, 614.0, 410.0, 603.0),
            word("黄区", 174.0, 527.0, 195.0, 516.0),
            word("140-159", 262.0, 524.0, 300.0, 513.0),
            word("2.4", 392.0, 524.0, 410.0, 513.0),
        ];
        // 表格结束后隔 44.5pt 空白的正文段（首行缩进小标题 + 公式行）。
        // 公式词刻意与表格列多点对齐（x1 与「倍数」「数量」列对齐）。
        words.push(word("152.", 70.0, 471.0, 94.0, 459.0));
        words.push(word("公式", 100.0, 471.0, 130.0, 459.0));
        words.push(word("E", 82.0, 444.0, 95.0, 432.0));
        words.push(word("x", 308.0, 444.0, 320.0, 432.0)); // x1=320 与数量列巧合
        words.push(word("y", 406.0, 444.0, 420.0, 432.0)); // 与倍数列巧合
        let rows = group_rows(words);
        let regions = detect_table_regions(&rows);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0], 0..3, "表格链不得越过段落空白吞并正文行");
    }
}
