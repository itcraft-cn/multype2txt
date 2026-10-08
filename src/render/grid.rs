//! 电子表格网格 → CSV 渲染器。
//!
//! # 为什么绕开 IR 直接读原生网格
//!
//! 底层库把工作表转成 IR 时会做一轮**有损**取舍：按「这段是散文还是表格」
//! 的启发式决定输出段落还是表格、跳过空行、把单元格重新组织成块级元素。
//! 对「生成给人看的文档」没问题，对「全量导出数据」是错的——CSV 的全部
//! 价值在于列对齐与行完整，任何启发式都会破坏它。
//!
//! 因此这里直接读工作表的原生单元格网格，本模块自己保证三件事：
//!
//! 1. **列对齐**：先算出整表列数，再按列号补齐每个单元格；
//! 2. **行完整**：按工作表真实行号补齐中间的空行，不让后续行整体上移；
//! 3. **全量**：不做采样、不做截断——略读是阅读方的任务，不是转换器的。
//!
//! # 多工作表
//!
//! CSV 天生是单表格式，多工作表以注释行分隔：
//!
//! ```text
//! # sheet: 销售明细  rows=1200 cols=8  merged=2
//! # merged: A1:C1
//! 日期,产品,数量,...
//! ```
//!
//! 以 `#` 开头的行是本工具的元信息，不是数据；严格的 CSV 解析器需要跳过
//! 它们（绝大多数都支持注释或可自行过滤首列以 `#` 开头的行）。
//!
//! # 能力边界
//!
//! - xls（BIFF8）解析器未保留合并单元格信息，该格式的输出不含 `# merged` 行；
//! - 公式一律导出**计算结果**，不导出公式文本。

use office_oxide::xls;
use office_oxide::xlsx;

use crate::error::ConvertError;
use crate::output::OutputFormat;
use crate::render::{DocumentRenderer, Source};

/// 与后端无关的单元格值。
///
/// 三种电子表格取值类型（xlsx 的 `CellValue`、xls 的 `CellValue`）在适配层
/// 统一到本枚举，CSV 序列化只针对它写一次。
#[derive(Debug, Clone, PartialEq)]
pub enum GridCell {
    /// 空单元格。
    Empty,
    /// 文本。
    Text(String),
    /// 数值。
    Number(f64),
    /// 布尔。
    Bool(bool),
    /// 日期时间，已格式化为 ISO 8601。
    DateTime(String),
    /// 错误值（`#REF!` 等），原样保留。
    Error(String),
}

impl GridCell {
    /// 序列化为 CSV 字段文本。
    pub fn to_text(&self) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Text(text) => text.clone(),
            Self::Number(value) => format_number(*value),
            Self::Bool(value) => if *value { "TRUE" } else { "FALSE" }.to_string(),
            Self::DateTime(iso) => iso.clone(),
            Self::Error(code) => code.clone(),
        }
    }
}

/// 格式化数值：整数不带小数点，浮点用最短往返表示。
///
/// `1.0` 输出 `1`、`1.5` 输出 `1.5`，避免导出 `1.0000000000000002` 这类
/// 二进制浮点噪声；绝对值过大（超出 i64 精确范围）时交给 Rust 的最短
/// 往返格式化，避免 `as i64` 的饱和截断。
fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// 行遍历回调：收到（0 基行号，该行的稀疏单元格列表）。
///
/// 提取成别名有两个作用：trait 签名保持可读，以及让「一行长什么样」这个
/// 约定只在一处定义——新增后端时照此回调即可，CSV 序列化无需改动。
///
/// # 注意
///
/// 两处生命周期都不能省，省掉任何一处都编译不过，改动时请一并留意：
///
/// - `for<'b>` 绑定**切片**的生命周期为高阶绑定。省略后别名会把它固定成
///   某个具体生命周期（最终是 `'static`），后端就无法把本行的缓冲区借给
///   回调，只能复制或泄漏数据；
/// - `+ 'a` 绑定 **trait 对象自身**的生命周期。别名在定义处解析生命周期
///   默认值，此处不写就会默认成 `'static`；而回调是闭包，通常捕获了正在
///   拼装的输出缓冲区，不可能 `'static`。
pub type RowVisitor<'a> = dyn for<'b> FnMut(u32, &'b [(u32, GridCell)]) + 'a;

/// 一个可被 CSV 渲染的电子表格来源。
///
/// 由各格式的适配器实现；CSV 序列化只依赖本 trait，因此新增格式（例如
/// 后续接入的 xlsb）只需新增一个实现。
///
/// 方法设计为 `&mut self`，是为了给需要内部读取游标的后端（流式解析）留
/// 出余地；当前两个实现都只读。
pub trait SheetSource {
    /// 工作表数量。
    fn sheet_count(&self) -> usize;

    /// 第 `index` 张工作表的名称。
    fn sheet_name(&self, index: usize) -> String;

    /// 第 `index` 张工作表的列数（用于按列补齐）。
    fn column_count(&self, index: usize) -> usize;

    /// 第 `index` 张工作表的行数（含中间空行）。
    fn row_count(&self, index: usize) -> usize;

    /// 逐行遍历，回调收到（0 基行号，稀疏单元格列表）。
    ///
    /// 空行也会以空的单元格列表回调，调用方据此保持行号连续。
    fn for_each_row(&mut self, index: usize, visitor: &mut RowVisitor) -> Result<(), ConvertError>;

    /// 合并单元格区域（形如 `A1:C1`）；后端不支持时返回空列表。
    fn merged_ranges(&self, index: usize) -> Vec<String>;
}

/// CSV 渲染器：把电子表格导出为 RFC 4180 兼容的逗号分隔值。
#[derive(Debug, Clone, Copy, Default)]
pub struct CsvRenderer;

impl CsvRenderer {
    /// 构造渲染器（无配置，CSV 策略全部固定）。
    pub fn new() -> Self {
        Self
    }
}

impl DocumentRenderer for CsvRenderer {
    fn format(&self) -> OutputFormat {
        OutputFormat::Csv
    }

    fn render(&self, source: &Source<'_>) -> Result<String, ConvertError> {
        let document = match source {
            Source::Plain(_) => {
                return Err(ConvertError::IncompatibleOutput {
                    format: OutputFormat::Csv.name().to_string(),
                    detail: "该输入没有表格结构，无法导出 CSV".to_string(),
                });
            }
            Source::Office(document) => *document,
        };

        // 按实际解析出的文档类型选择适配器，而不是按扩展名：底层库支持
        // 文件头嗅探，扩展名写错时仍能拿到正确类型。
        let mut sheet_source: Box<dyn SheetSource + '_> = if let Some(xlsx) = document.as_xlsx() {
            Box::new(XlsxSource::new(xlsx))
        } else if let Some(xls) = document.as_xls() {
            Box::new(XlsSource::new(xls))
        } else {
            return Err(ConvertError::IncompatibleOutput {
                format: OutputFormat::Csv.name().to_string(),
                detail: "该文档不是电子表格（doc/docx/ppt/pptx 无网格可导出）".to_string(),
            });
        };

        render_sheets(&mut *sheet_source)
    }
}

/// 依次渲染全部工作表，表间以空行分隔。
fn render_sheets(source: &mut dyn SheetSource) -> Result<String, ConvertError> {
    let count = source.sheet_count();
    if count == 0 {
        return Ok(String::new());
    }

    let mut out = String::new();
    for index in 0..count {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&render_sheet(source, index)?);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    Ok(out)
}

/// 渲染单张工作表：元信息注释 + 全量数据。
fn render_sheet(source: &mut dyn SheetSource, index: usize) -> Result<String, ConvertError> {
    let name = source.sheet_name(index);
    let rows = source.row_count(index);
    let columns = source.column_count(index);
    let merges = source.merged_ranges(index);

    let mut out = format!("# sheet: {name}  rows={rows} cols={columns}");
    if !merges.is_empty() {
        out.push_str(&format!("  merged={}", merges.len()));
    }
    out.push('\n');
    if !merges.is_empty() {
        // 合并区域在 CSV 中无法表达，只能以元信息保留其位置语义。
        out.push_str(&format!("# merged: {}\n", merges.join(" ")));
    }

    source.for_each_row(index, &mut |_, cells| {
        let mut fields = vec![String::new(); columns];
        for (column, value) in cells {
            if let Some(slot) = fields.get_mut(*column as usize) {
                *slot = value.to_text();
            }
        }
        let line = fields
            .iter()
            .map(|field| escape_csv_field(field))
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&line);
        out.push('\n');
    })?;

    Ok(out)
}

/// 按 RFC 4180 转义字段：含分隔符、双引号或换行时用双引号包裹。
fn escape_csv_field(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

// ---------------------------------------------------------------------------
// XLSX 适配
// ---------------------------------------------------------------------------

/// xlsx 网格适配器。
struct XlsxSource<'a> {
    doc: &'a xlsx::XlsxDocument,
}

impl<'a> XlsxSource<'a> {
    fn new(doc: &'a xlsx::XlsxDocument) -> Self {
        Self { doc }
    }

    /// 取第 `index` 张工作表。
    fn worksheet(&self, index: usize) -> Result<&xlsx::Worksheet, ConvertError> {
        self.doc
            .worksheets
            .get(index)
            .ok_or(ConvertError::SheetOutOfRange {
                index,
                count: self.doc.worksheets.len(),
            })
    }
}

impl SheetSource for XlsxSource<'_> {
    fn sheet_count(&self) -> usize {
        self.doc.worksheets.len()
    }

    fn sheet_name(&self, index: usize) -> String {
        self.doc
            .worksheets
            .get(index)
            .map(|worksheet| worksheet.name.clone())
            .unwrap_or_default()
    }

    fn column_count(&self, index: usize) -> usize {
        self.worksheet(index).map_or(0, |worksheet| {
            worksheet
                .rows
                .iter()
                .flat_map(|row| row.cells.iter().map(|cell| cell.reference.col + 1))
                .max()
                .unwrap_or(0) as usize
        })
    }

    fn row_count(&self, index: usize) -> usize {
        self.worksheet(index).map_or(0, |worksheet| {
            // 底层解析会跳过 XML 中省略的整行空行，必须按真实行号补出，
            // 否则后续数据行会整体上移。
            let mut expected = 0u32;
            let mut total = 0usize;
            for row in &worksheet.rows {
                let position = row.index.saturating_sub(1);
                if position > expected {
                    total += (position - expected) as usize;
                    expected = position;
                }
                total += 1;
                expected += 1;
            }
            total
        })
    }

    fn for_each_row(&mut self, index: usize, visitor: &mut RowVisitor) -> Result<(), ConvertError> {
        let worksheet = self.worksheet(index)?;
        let shared = &self.doc.shared_strings;
        let mut expected = 0u32;

        for row in &worksheet.rows {
            let position = row.index.saturating_sub(1);
            while expected < position {
                visitor(expected, &[]);
                expected += 1;
            }

            let cells: Vec<(u32, GridCell)> = row
                .cells
                .iter()
                .map(|cell| (cell.reference.col, convert_xlsx_value(&cell.value, shared)))
                .collect();
            visitor(position, &cells);
            expected = position.saturating_add(1);
        }

        Ok(())
    }

    fn merged_ranges(&self, index: usize) -> Vec<String> {
        self.worksheet(index)
            .map(|worksheet| worksheet.merged_cells.clone())
            .unwrap_or_default()
    }
}

/// 把 xlsx 单元格取值转换为统一的 [`GridCell`]。
///
/// 共享字符串表中的索引在这里解引用成真实文本——直接导出索引会得到一列
/// 无意义的数字。
fn convert_xlsx_value(value: &xlsx::CellValue, shared: &xlsx::SharedStringTable) -> GridCell {
    match value {
        xlsx::CellValue::Empty => GridCell::Empty,
        xlsx::CellValue::Number(number) => GridCell::Number(*number),
        xlsx::CellValue::String(text) => GridCell::Text(text.clone()),
        xlsx::CellValue::SharedString(index) => GridCell::Text(
            shared
                .get_shared(*index)
                .map(|entry| entry.text.clone())
                .unwrap_or_default(),
        ),
        xlsx::CellValue::Boolean(value) => GridCell::Bool(*value),
        xlsx::CellValue::Error(code) => GridCell::Error(code.clone()),
        xlsx::CellValue::Date(value) => GridCell::DateTime(value.to_iso_string()),
    }
}

// ---------------------------------------------------------------------------
// XLS 适配
// ---------------------------------------------------------------------------

/// xls（BIFF8）网格适配器。
struct XlsSource<'a> {
    doc: &'a xls::XlsDocument,
}

impl<'a> XlsSource<'a> {
    fn new(doc: &'a xls::XlsDocument) -> Self {
        Self { doc }
    }

    fn sheet(&self, index: usize) -> Result<&xls::Sheet, ConvertError> {
        self.doc
            .sheets
            .get(index)
            .ok_or(ConvertError::SheetOutOfRange {
                index,
                count: self.doc.sheets.len(),
            })
    }
}

impl SheetSource for XlsSource<'_> {
    fn sheet_count(&self) -> usize {
        self.doc.sheets.len()
    }

    fn sheet_name(&self, index: usize) -> String {
        self.doc
            .sheets
            .get(index)
            .map(|sheet| sheet.name.clone())
            .unwrap_or_default()
    }

    fn column_count(&self, index: usize) -> usize {
        // xls 解析器已把网格补成矩形（缺列补 Empty），行内长度即列数。
        self.sheet(index).map_or(0, |sheet| {
            sheet.rows.iter().map(Vec::len).max().unwrap_or(0)
        })
    }

    fn row_count(&self, index: usize) -> usize {
        self.sheet(index).map_or(0, |sheet| sheet.rows.len())
    }

    fn for_each_row(&mut self, index: usize, visitor: &mut RowVisitor) -> Result<(), ConvertError> {
        let sheet = self.sheet(index)?;
        for (position, row) in sheet.rows.iter().enumerate() {
            let cells: Vec<(u32, GridCell)> = row
                .iter()
                .enumerate()
                .map(|(column, value)| (column as u32, convert_xls_value(value)))
                .collect();
            visitor(position as u32, &cells);
        }
        Ok(())
    }

    fn merged_ranges(&self, _index: usize) -> Vec<String> {
        // BIFF8 解析路径未保留合并区域，此处只能留空（能力边界）。
        Vec::new()
    }
}

/// 把 xls 单元格取值转换为统一的 [`GridCell`]。
fn convert_xls_value(value: &xls::CellValue) -> GridCell {
    match value {
        xls::CellValue::Empty => GridCell::Empty,
        xls::CellValue::Number(number) => GridCell::Number(*number),
        xls::CellValue::String(text) => GridCell::Text(text.clone()),
        xls::CellValue::Bool(value) => GridCell::Bool(*value),
        // 错误码是 BIFF 私有编码，复用底层库的映射得到 `#REF!` 等文本。
        xls::CellValue::Error(_) => GridCell::Error(value.as_text()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_render_without_decimal_point() {
        assert_eq!(GridCell::Number(3.0).to_text(), "3");
        assert_eq!(GridCell::Number(-12.0).to_text(), "-12");
        assert_eq!(GridCell::Number(1.5).to_text(), "1.5");
    }

    #[test]
    fn booleans_render_as_excel_convention() {
        assert_eq!(GridCell::Bool(true).to_text(), "TRUE");
        assert_eq!(GridCell::Bool(false).to_text(), "FALSE");
    }

    #[test]
    fn empty_and_error_keep_their_meaning() {
        assert_eq!(GridCell::Empty.to_text(), "");
        assert_eq!(GridCell::Error("#REF!".to_string()).to_text(), "#REF!");
        assert_eq!(
            GridCell::DateTime("2024-05-01T00:00:00".to_string()).to_text(),
            "2024-05-01T00:00:00"
        );
    }

    #[test]
    fn csv_field_escaping_follows_rfc4180() {
        assert_eq!(escape_csv_field("plain"), "plain");
        assert_eq!(escape_csv_field("a,b"), "\"a,b\"");
        assert_eq!(escape_csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(escape_csv_field("line1\nline2"), "\"line1\nline2\"");
        // 不含特殊字符时不加引号，保持可读性。
        assert_eq!(escape_csv_field("  保留空格  "), "  保留空格  ");
    }

    /// 用一个固定的假工作表验证「列补齐 + 行补齐」两个核心不变量。
    struct FakeSource;

    impl SheetSource for FakeSource {
        fn sheet_count(&self) -> usize {
            1
        }

        fn sheet_name(&self, _index: usize) -> String {
            "数据".to_string()
        }

        fn column_count(&self, _index: usize) -> usize {
            3
        }

        fn row_count(&self, _index: usize) -> usize {
            3
        }

        fn for_each_row(
            &mut self,
            _index: usize,
            visitor: &mut RowVisitor<'_>,
        ) -> Result<(), ConvertError> {
            // 第 1 行空行、第 2 行只有第 3 列有值 —— 两个错位陷阱。
            visitor(0, &[(0, GridCell::Text("甲".to_string()))]);
            visitor(1, &[]);
            visitor(2, &[(2, GridCell::Number(7.0))]);
            Ok(())
        }

        fn merged_ranges(&self, _index: usize) -> Vec<String> {
            vec!["A1:C1".to_string()]
        }
    }

    #[test]
    fn rows_and_columns_stay_aligned() {
        let mut source = FakeSource;
        let out = render_sheets(&mut source).expect("渲染不应失败");

        let lines: Vec<&str> = out.lines().collect();
        assert!(
            lines[0].contains("# sheet: 数据  rows=3 cols=3  merged=1"),
            "{out}"
        );
        assert_eq!(lines[1], "# merged: A1:C1");
        assert_eq!(lines[2], "甲,,");
        assert_eq!(lines[3], ",,");
        assert_eq!(lines[4], ",,7");
    }
}
